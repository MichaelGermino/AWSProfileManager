//! AWS Profile Manager 2.0: owns the process. Starts the core (on a tokio runtime), the GPUI app,
//! the tray icon and the background scheduler, and implements the core's `Host` for this OS.
//!
//! Closing the window destroys it (plan decision: no hidden window holding memory); the process
//! stays in the tray until Exit. The scheduler runs on the tokio runtime and does not depend on
//! any window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use apm_core::core::{Core, CoreConfig};
use apm_core::host::{DialogRequest, Host, SignInRequest, SignInWindow};
use apm_ui::bridge::{Bridge, CoreEvent};
use apm_ui::model::AppModel;
use futures::channel::mpsc as fmpsc;
use futures::StreamExt;
use gpui::{App, AppContext, Entity};
use serde_json::{json, Value};
use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[cfg(feature = "test-hooks")]
mod control;
mod platform;
mod tray;
mod webwin;

/// 1.x `SPLASH_MIN_MS`: a floor so a fast start does not flash the splash on and off.
const SPLASH_MIN_MS: u64 = 700;
/// Expiry moves with the clock rather than with any event, so the tray status is re-checked on a timer.
const STATUS_CHECK: Duration = Duration::from_secs(30);
/// Shortcut icons are repaired after start-up, never competing with the first paint (1.x).
const SHORTCUT_SYNC_DELAY: Duration = Duration::from_secs(5);

/// Work for the UI thread, from the core's threads, the tray and a second launch.
enum AppCmd {
    /// Show the main window (tray, second instance, the core asking for it).
    Open,
    /// Profiles, folders or lock state changed: rebuild the tray menu.
    TrayChanged,
    /// Something the tray badge depends on may have changed.
    TrayStatus,
    /// App icon, tray style or badge setting changed.
    IconChanged,
    OpenLogViewer,
    LogViewerChanged,
    Menu(tray::Action),
}

/// A running embedded Identity Center sign-in (the core's [`SignInWindow`]).
struct SignIn(webwin::Handle);

impl SignInWindow for SignIn {
    fn reveal(&self) {
        self.0.reveal();
    }
    fn close(&self) {
        self.0.close();
    }
}

impl Drop for SignIn {
    /// Never leave a hidden webview behind (it holds ~70 MB of WebView2 processes).
    fn drop(&mut self) {
        self.0.close();
    }
}

/// The core's window into the OS and the UI.
struct AppHost {
    events: fmpsc::UnboundedSender<CoreEvent>,
    cmds: fmpsc::UnboundedSender<AppCmd>,
    /// Test mode: record events, external opens and dialog answers for the flow runner instead of
    /// touching the OS (1.x installExternalOpenCapture).
    #[cfg(feature = "test-hooks")]
    test: Option<control::Shared>,
    /// Placement of the embedded web windows (test mode: right display, unfocused).
    web: webwin::Options,
    /// Window icon (.ico) of the selected app icon, for windows opened from now on.
    icon: Mutex<PathBuf>,
    /// App data directory: the console windows' WebView2 data lives under it.
    app_data: PathBuf,
}

impl AppHost {
    fn cmd(&self, c: AppCmd) {
        let _ = self.cmds.unbounded_send(c);
    }

    fn icon(&self) -> PathBuf {
        self.icon.lock().unwrap().clone()
    }
}

/// Release packages (built without the `test-hooks` feature) touch the login item and shortcuts;
/// development builds never register target/ binaries (1.x: packaged builds only).
const IS_RELEASE: bool = cfg!(not(feature = "test-hooks"));

fn apply_launch_at_startup(enabled: bool) {
    if !IS_RELEASE {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let cmd = platform::run_command(&exe);
    let _ = platform::set_run_entry(platform::RUN_VALUE_NAME, enabled.then_some(cmd.as_str()));
}

impl Host for AppHost {
    fn emit(&self, channel: &str, payload: Vec<Value>) {
        #[cfg(feature = "test-hooks")]
        if let Some(t) = &self.test {
            t.lock().unwrap().record(channel, &payload);
        }
        if matches!(channel, "scheduler:pausedChanged" | "auth:credentialsRefreshed" | "auth:credentialsExpired" | "auth:autoRefreshPausedForFailures") {
            self.cmd(AppCmd::TrayStatus);
        }
        let _ = self.events.unbounded_send(CoreEvent { channel: channel.to_string(), payload });
    }

    fn open_external(&self, kind: &str, target: &str) {
        #[cfg(feature = "test-hooks")]
        if let Some(t) = &self.test {
            t.lock().unwrap().opens.push(serde_json::json!({ "kind": kind, "target": target }));
            return;
        }
        let _ = kind;
        // ShellExecute / `open`, never a shell command line (a URL's '&' would split it).
        let _ = open::that_detached(target);
    }

    fn dialog(&self, request: DialogRequest) -> Option<String> {
        #[cfg(feature = "test-hooks")]
        if let Some(t) = &self.test {
            return t.lock().unwrap().dialogs.pop_front().flatten();
        }
        let mut d = rfd::FileDialog::new().set_title(&request.title);
        for (label, exts) in &request.filters {
            let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
            d = d.add_filter(label, &exts);
        }
        if let Some(name) = &request.default_name {
            d = d.set_file_name(name);
        }
        let picked = if request.save { d.save_file() } else { d.pick_file() };
        picked.map(|p| p.to_string_lossy().into_owned())
    }

    fn tray_changed(&self) {
        self.cmd(AppCmd::TrayChanged);
    }

    fn apply_launch_at_startup(&self, enabled: bool) {
        apply_launch_at_startup(enabled);
    }

    fn app_icon_changed(&self) {
        self.cmd(AppCmd::IconChanged);
    }

    fn log_viewer_changed(&self) {
        self.cmd(AppCmd::LogViewerChanged);
    }

    fn open_log_viewer(&self) {
        self.cmd(AppCmd::OpenLogViewer);
    }

    fn show_main_window(&self) {
        self.cmd(AppCmd::Open);
    }

    #[cfg(feature = "test-hooks")]
    fn launch_browser(&self, exe: &str, url: &str) -> Result<(), String> {
        if self.test.is_some() {
            self.open_external("url", url);
            return Ok(());
        }
        // Same as the Host default (which an override cannot call).
        let mut cmd = if cfg!(target_os = "macos") {
            let mut c = std::process::Command::new("open");
            c.arg("-a").arg(exe).arg(url);
            c
        } else {
            let mut c = std::process::Command::new(exe);
            c.arg(url);
            c
        };
        cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    /// 1.x: a 520x720 `BrowserWindow`, hidden until revealed, on the org's persistent partition.
    fn open_sign_in(&self, request: SignInRequest, closed_by_user: Box<dyn FnOnce() + Send>) -> Box<dyn SignInWindow> {
        let handle = webwin::open(
            webwin::Spec {
                kind: "sign-in",
                url: request.url,
                title: request.title,
                data_dir: request.data_dir,
                profile_name: None,
                size: (520, 720),
                visible: false,
                icon: Some(self.icon()),
                on_user_close: Some(closed_by_user),
                on_failed: None,
            },
            &self.web,
        );
        Box::new(SignIn(handle))
    }

    /// 1.x: a 1400x900 `BrowserWindow` on partition `persist:console-<profileId>`. 2.0: one
    /// WebView2 data folder for all console windows (one set of browser processes however many are
    /// open) with a WebView2 profile per AWS profile, so each account's console session stays
    /// independent.
    fn open_console_window(&self, profile_id: &str, title: &str, url: &str) {
        let target = url.to_string();
        webwin::open(
            webwin::Spec {
                kind: "console",
                url: url.to_string(),
                title: title.to_string(),
                data_dir: self.app_data.join("console-webview"),
                profile_name: Some(webwin::profile_name("console-", profile_id)),
                size: (1400, 900),
                visible: true,
                icon: Some(self.icon()),
                on_user_close: None,
                // No WebView2 runtime: the browser still gets the user to the console.
                on_failed: Some(Box::new(move |_| {
                    let _ = open::that_detached(&target);
                })),
            },
            &self.web,
        );
    }

    fn install_update(&self, installer: &std::path::Path) -> Result<(), String> {
        // NSIS silent install; it replaces this process's files, so quit right after starting it.
        // `--updated` keeps the shortcuts as they are; `--force-run` starts the new version when
        // the install is done (the same flags electron-updater passes to 1.x installers).
        std::process::Command::new(installer).args(["/S", "--updated", "--force-run"]).spawn().map_err(|e| e.to_string())?;
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            std::process::exit(0);
        });
        Ok(())
    }
}

fn test_mode() -> bool {
    cfg!(feature = "test-hooks") && std::env::var("APM_TEST_MODE").as_deref() == Ok("1")
}

/// Test seams (plan §5.3), the same variables as 1.x's testHooks.ts. Only with the `test-hooks`
/// feature AND `APM_TEST_MODE=1`; release packages are built without the feature.
#[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
fn test_hooks(mut cfg: CoreConfig) -> CoreConfig {
    #[cfg(feature = "test-hooks")]
    if test_mode() {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        if let Some(d) = var("APM_APPDATA_DIR") {
            cfg.paths.app_data = d.into();
        }
        if let Some(f) = var("APM_AWS_CREDENTIALS_FILE") {
            cfg.paths.credentials_file = f.into();
        }
        if let Some(s) = var("APM_KEYCHAIN_SERVICE") {
            cfg.keychain_service = s;
        }
        if let Some(map) = var("APM_HOST_MAP").and_then(|m| serde_json::from_str::<std::collections::HashMap<String, String>>(&m).ok()) {
            cfg.http.host_map = map;
        }
        if let Some(ca) = var("APM_TEST_CA_FILE").and_then(|f| std::fs::read_to_string(f).ok()) {
            cfg.http.extra_ca_pem = Some(ca);
        }
    }
    // Development builds run from target/, which has no resources/ next to the exe (packaging adds
    // it): use the repository's, so app icons and the like are there.
    #[cfg(feature = "test-hooks")]
    if !cfg.resources_dir.exists() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources");
        if repo.exists() {
            cfg.resources_dir = repo;
        }
    }
    cfg
}

/// How long the splash stays up at least, or `None` for no splash. Test runs show none unless
/// `APM_SPLASH_MIN_MS` asks for one (1.x `testSplashMinMs`), so the runner's window is the main one.
fn splash_min_ms() -> Option<u64> {
    let forced = std::env::var("APM_SPLASH_MIN_MS").ok().and_then(|v| v.trim().parse::<u64>().ok());
    if test_mode() {
        return forced.filter(|ms| *ms > 0);
    }
    Some(forced.unwrap_or(SPLASH_MIN_MS))
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32EmptyWorkingSet(process: isize) -> i32;
}

/// After the last window closes, hand untouched pages back to the OS (Phase 0: under 2 MB in the tray).
fn trim_working_set() {
    #[cfg(windows)]
    unsafe {
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

/// The OS window handle of a GPUI window.
fn hwnd_of(window: &gpui::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// The .ico for windows of the selected app icon.
fn window_icon_path(core: &Core) -> PathBuf {
    let key = core.current_app_icon_key();
    core.app_icons().window_icon_path(core.cfg.platform, &key).unwrap_or_else(|| core.cfg.resources_dir.join("icon.ico"))
}

/// The tray icon and what it currently shows.
struct TrayState {
    icon: Option<TrayIcon>,
    actions: Arc<Mutex<HashMap<MenuId, tray::Action>>>,
    shown: Option<(tray::TrayStatus, tray::TrayLook)>,
    consoles_in_flight: HashSet<String>,
    /// Last menu model, so an unchanged one is not rebuilt.
    menu: Vec<tray::Item>,
}

fn build_muda(items: &[tray::Item], actions: &mut HashMap<MenuId, tray::Action>) -> Vec<Box<dyn IsMenuItem>> {
    items
        .iter()
        .map(|i| -> Box<dyn IsMenuItem> {
            match i {
                tray::Item::Separator => Box::new(PredefinedMenuItem::separator()),
                tray::Item::Action { label, enabled, action } => {
                    let m = MenuItem::new(label, *enabled, None);
                    if let Some(a) = action {
                        actions.insert(m.id().clone(), a.clone());
                    }
                    Box::new(m)
                }
                tray::Item::Submenu { label, enabled, items } => {
                    let children = build_muda(items, actions);
                    let refs: Vec<&dyn IsMenuItem> = children.iter().map(|c| c.as_ref()).collect();
                    Box::new(Submenu::with_items(label, *enabled, &refs).expect("tray submenu"))
                }
            }
        })
        .collect()
}

impl TrayState {
    fn rebuild_menu(&mut self, core: &Core) {
        let model = tray::build_menu(&tray::menu_profiles(core), core.is_locked(), &self.consoles_in_flight);
        if model == self.menu {
            return;
        }
        let Some(icon) = &self.icon else { return };
        let mut actions = HashMap::new();
        let items = build_muda(&model, &mut actions);
        let refs: Vec<&dyn IsMenuItem> = items.iter().map(|c| c.as_ref()).collect();
        let Ok(menu) = Menu::with_items(&refs) else { return };
        icon.set_menu(Some(Box::new(menu)));
        *self.actions.lock().unwrap() = actions;
        self.menu = model;
    }

    /// Rebuild the image when the status or the look changed (cheap when nothing did).
    fn refresh_icon(&mut self, core: &Core) {
        let status = tray::status(core);
        let look = tray::look(core, platform::taskbar_is_dark(), platform::tray_icon_px());
        if self.shown.as_ref() == Some(&(status, look.clone())) {
            return;
        }
        let Some(icon) = &self.icon else { return };
        if let Some((rgba, w, h)) = tray::tray_image(&core.cfg.resources_dir, &look, status) {
            if let Ok(img) = tray_icon::Icon::from_rgba(rgba, w, h) {
                let _ = icon.set_icon(Some(img));
            }
        }
        let _ = icon.set_tooltip(Some(tray::tooltip(status)));
        self.shown = Some((status, look));
    }
}

/// Where the HUD card rests: against the taskbar by the tray icon (physical pixels converted to
/// GPUI's logical screen space of that display).
fn hud_placement(tray: &Rc<RefCell<TrayState>>, cx: &mut App) -> Option<apm_ui::hud::HudPlacement> {
    let rect = tray.borrow().icon.as_ref()?.rect()?;
    let (tx, ty) = (rect.position.x, rect.position.y);
    let (tw, th) = (rect.size.width as f64, rect.size.height as f64);
    let m = platform::monitor_at((tx + tw / 2.) as i32, (ty + th / 2.) as i32)?;
    let r = |(l, t, rr, b): (i32, i32, i32, i32)| tray::Rect { x: l as f64, y: t as f64, w: (rr - l) as f64, h: (b - t) as f64 };
    let s = m.scale as f64;
    let card = tray::compute_card_rect(
        tray::Rect { x: tx, y: ty, w: tw, h: th },
        r(m.monitor),
        r(m.work),
        apm_ui::hud::CARD_W as f64 * s,
        apm_ui::hud::CARD_H as f64 * s,
        4. * s,
    );
    // GPUI's logical coordinates for a display are its physical ones divided by its scale.
    let display = cx.displays().into_iter().find(|d| {
        let b = d.bounds();
        (f64::from(b.origin.x) - m.monitor.0 as f64 / s).abs() < 2. && (f64::from(b.origin.y) - m.monitor.1 as f64 / s).abs() < 2.
    });
    Some(apm_ui::hud::HudPlacement {
        card: gpui::Bounds {
            origin: gpui::point(gpui::px((card.x / s) as f32), gpui::px((card.y / s) as f32)),
            size: gpui::size(gpui::px(apm_ui::hud::CARD_W), gpui::px(apm_ui::hud::CARD_H)),
        },
        display: display.map(|d| d.id()),
    })
}

fn hud_palette(core: &Core, cx: &App) -> apm_ui::theme::Palette {
    let settings = core.settings().get();
    let s = |k: &str| settings.get(k).and_then(Value::as_str).map(str::to_string);
    apm_ui::theme::Palette::from_settings(s("theme").as_deref(), s("appStyle").as_deref(), cx.window_appearance())
}

/// 1.x `openConsoleFromTray`: progress and failures go to the HUD, not a notification (the window
/// is usually hidden when the tray is in use, and Focus Assist can swallow a toast).
fn open_console_from_tray(id: String, label: String, bridge: &Bridge, tray: &Rc<RefCell<TrayState>>, cx: &mut App) {
    let core = bridge.core().clone();
    if core.is_locked() || tray.borrow().consoles_in_flight.contains(&id) {
        return;
    }
    tray.borrow_mut().consoles_in_flight.insert(id.clone());
    tray.borrow_mut().rebuild_menu(&core);
    cx.set_global(apm_ui::hud::HudPalette(hud_palette(&core, cx)));
    apm_ui::hud::show_busy("Opening console…", label, cx);
    let call = bridge.call("console:open", vec![json!(id)]);
    let tray = tray.clone();
    cx.spawn(async move |cx| {
        let result = call.await;
        cx.update(|cx| {
            match result {
                Ok(v) if v.get("success") == Some(&Value::Bool(true)) => apm_ui::hud::hide(cx),
                Ok(v) => apm_ui::hud::show_error("Could not open console", v.get("error").and_then(Value::as_str).unwrap_or("Unknown error").to_string(), cx),
                Err(e) => apm_ui::hud::show_error("Could not open console", e, cx),
            }
            tray.borrow_mut().consoles_in_flight.remove(&id);
            tray.borrow_mut().rebuild_menu(&core);
        });
    })
    .detach();
}

/// main.ts `applyAppIconChange`: every window's icon, the tray, and (release builds) the shortcuts.
fn apply_app_icon_change(core: &Arc<Core>, host: &AppHost, tray: &Rc<RefCell<TrayState>>, cx: &mut App) {
    let ico = window_icon_path(core);
    *host.icon.lock().unwrap() = ico.clone();
    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, _| {
            if let Some(h) = hwnd_of(window) {
                platform::set_window_icon(h, &ico);
            }
        });
    }
    tray.borrow_mut().refresh_icon(core);
    sync_shortcut_icons(core.clone());
}

/// 1.x `syncShortcutIcons`: point the Start menu / desktop / pinned shortcuts at the selected
/// icon. Release builds only (no shortcut targets a development exe), on a thread of its own.
fn sync_shortcut_icons(core: Arc<Core>) {
    if !IS_RELEASE {
        return;
    }
    std::thread::spawn(move || {
        let Ok(exe) = std::env::current_exe() else { return };
        let key = core.current_app_icon_key();
        let design = core.app_icons().design_ico_path(&key);
        let icon = platform::shortcut_icon_for(&core.cfg.paths.app_data, &key, design.as_deref(), &exe);
        let dirs = platform::shortcut_dirs();
        platform::sync_shortcut_icons_in(&dirs, &exe, &icon);
        platform::sync_shortcut_app_ids_in(&dirs, &exe, platform::APP_USER_MODEL_ID);
    });
}

fn handle_cmd(cmd: AppCmd, model: &Entity<AppModel>, bridge: &Bridge, host: &Arc<AppHost>, tray: &Rc<RefCell<TrayState>>, cx: &mut App) {
    let core = bridge.core().clone();
    match cmd {
        AppCmd::Open => apm_ui::open_main_window(model, cx),
        AppCmd::TrayChanged => {
            let mut t = tray.borrow_mut();
            t.rebuild_menu(&core);
            t.refresh_icon(&core);
        }
        AppCmd::TrayStatus => tray.borrow_mut().refresh_icon(&core),
        AppCmd::IconChanged => apply_app_icon_change(&core, host, tray, cx),
        AppCmd::OpenLogViewer => apm_ui::open_log_viewer(model, cx),
        AppCmd::LogViewerChanged => apm_ui::log_viewer::reload(cx),
        AppCmd::Menu(action) => match action {
            tray::Action::Open => apm_ui::open_main_window(model, cx),
            tray::Action::Console(id, label) => open_console_from_tray(id, label, bridge, tray, cx),
            tray::Action::Pause | tray::Action::Resume => {
                if !core.is_locked() {
                    let paused = matches!(action, tray::Action::Pause);
                    bridge.call("scheduler:setPaused", vec![json!(paused)]).map(drop).now_or_never_detached(cx);
                }
            }
            tray::Action::RefreshAll => {
                if !core.is_locked() {
                    bridge.call("auth:refreshAll", vec![]).map(drop).now_or_never_detached(cx);
                }
            }
            tray::Action::Refresh(id) => {
                if !core.is_locked() {
                    bridge.call("auth:refresh", vec![json!(id)]).map(drop).now_or_never_detached(cx);
                }
            }
            tray::Action::Exit => {
                // Remove the icon now, or Windows leaves a ghost in the tray until hovered.
                tray.borrow_mut().icon.take();
                cx.quit();
            }
        },
    }
}

/// Run a core call to completion in the background, ignoring its result.
trait Detach {
    fn now_or_never_detached(self, cx: &mut App);
}

impl<F: std::future::Future<Output = ()> + 'static> Detach for F {
    fn now_or_never_detached(self, cx: &mut App) {
        cx.spawn(async move |_| self.await).detach();
    }
}

use futures::FutureExt as _;

/// Start-up: splash (1.x), then the main window; or only the tray with "start minimized to tray".
fn start_windows(model: &Entity<AppModel>, core: &Arc<Core>, cx: &mut App) {
    let settings = core.settings().get();
    // 1.x hid the window at start and then showed it anyway once the splash finished; 2.0 keeps
    // the setting's meaning: only the tray.
    if settings.get("startMinimizedToTray") == Some(&Value::Bool(true)) && !test_mode() {
        return;
    }
    let Some(min_ms) = splash_min_ms() else {
        apm_ui::open_main_window(model, cx);
        return;
    };
    let icon = core.app_icons().png_path(&core.current_app_icon_key());
    let splash = apm_ui::splash::open(icon, hud_palette(core, cx), cx);
    let started = Instant::now();
    let model = model.clone();
    // The main window opens when the floor has passed (a GPUI window is ready in a frame or two,
    // so nothing is gained by building it hidden behind the splash, and a hidden GPUI window can
    // only be revealed by activating it, which test runs must never do).
    cx.spawn(async move |cx| {
        let floor = Duration::from_millis(min_ms);
        let elapsed = started.elapsed();
        if elapsed < floor {
            cx.background_executor().timer(floor - elapsed).await;
        }
        cx.update(|cx| apm_ui::open_main_window(&model, cx));
        // One frame for the main window to paint before the splash goes.
        cx.background_executor().timer(Duration::from_millis(50)).await;
        cx.update(|cx| {
            if let Some(s) = splash {
                apm_ui::splash::close(s, cx);
            }
        });
    })
    .detach();
}

fn main() {
    let cfg = test_hooks(CoreConfig::for_user(env!("CARGO_PKG_VERSION")));
    // 1.x's shortcut identity, so pins and the taskbar button stay one app (test runs keep their own).
    if !test_mode() {
        platform::set_process_app_id(platform::APP_USER_MODEL_ID);
    }

    // Single instance (1.x requestSingleInstanceLock): a second launch shows the first one's window
    // and exits. Keyed on the app-data directory, so sandboxed test runs never collide.
    let instance_key = platform::stable_hash(&cfg.paths.app_data.to_string_lossy());
    #[cfg(windows)]
    let _instance = match platform::acquire_single_instance(&instance_key) {
        platform::Acquire::First(i) => i,
        platform::Acquire::Second => {
            platform::signal_first_instance(&instance_key);
            return;
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    let (event_tx, mut event_rx) = fmpsc::unbounded::<CoreEvent>();
    let (cmd_tx, mut cmd_rx) = fmpsc::unbounded::<AppCmd>();
    {
        let tx = cmd_tx.clone();
        platform::listen_for_activation(&instance_key, move || {
            let _ = tx.unbounded_send(AppCmd::Open);
        });
    }
    #[cfg(feature = "test-hooks")]
    let test_recorded: Option<control::Shared> = test_mode().then(|| Arc::new(std::sync::Mutex::new(control::Recorded::default())));
    #[allow(unused_mut)]
    let mut web = webwin::Options::default();
    #[cfg(feature = "test-hooks")]
    if test_recorded.is_some() {
        web = webwin::Options { test_mode: true, display: Some(std::env::var("APM_WINDOW_DISPLAY").unwrap_or_else(|_| "right".into())) };
    }
    let host = Arc::new(AppHost {
        events: event_tx,
        cmds: cmd_tx.clone(),
        #[cfg(feature = "test-hooks")]
        test: test_recorded.clone(),
        web,
        icon: Mutex::new(cfg.resources_dir.join("icon.ico")),
        app_data: cfg.paths.app_data.clone(),
    });
    let core = {
        let _guard = runtime.enter();
        let core = Core::new(cfg, host.clone()).into_shared();
        core.start_scheduler(None);
        core.window_started();
        core
    };
    *host.icon.lock().unwrap() = window_icon_path(&core);
    // 1.x applied the login item on every launch, which also repoints 1.x's entry at this exe.
    apply_launch_at_startup(core.settings().get().get("launchAtStartup") == Some(&Value::Bool(true)));
    // 1.x checked for updates in packaged builds only; so does 2.0 (release packages, which carry
    // the release key). APM_DISABLE_UPDATER turns it off (plan §5.3).
    if IS_RELEASE && std::env::var_os("APM_DISABLE_UPDATER").is_none() {
        let _guard = runtime.enter();
        core.start_update_checks();
    }
    let bridge = Bridge::new(core.clone(), runtime.handle().clone());

    let app = gpui_platform::application().with_assets(apm_ui::assets::Assets).with_quit_mode(gpui::QuitMode::Explicit);
    app.run(move |cx: &mut App| {
        apm_ui::init(cx);
        #[cfg(feature = "test-hooks")]
        if let Some(rec) = test_recorded.clone() {
            control::start(bridge.clone(), rec, cx);
        }
        #[cfg(feature = "test-hooks")]
        if test_mode() {
            // APM_WINDOW_DISPLAY=right|left: test windows open on that monitor, unfocused.
            if let Ok(which) = std::env::var("APM_WINDOW_DISPLAY") {
                let mut displays = cx.displays();
                displays.sort_by(|a, b| a.bounds().origin.x.partial_cmp(&b.bounds().origin.x).unwrap_or(std::cmp::Ordering::Equal));
                let pick = if which == "left" { displays.first() } else { displays.last() };
                if let Some(d) = pick {
                    cx.set_global(apm_ui::TestPlacement(d.id()));
                }
            }
        }
        let model = cx.new(|cx| AppModel::new(bridge.clone(), cx.window_appearance(), cx));

        // Every window gets the selected app icon (title bar, taskbar button); the HUD never activates.
        {
            let host = host.clone();
            cx.set_global(apm_ui::WindowOpenedHook(Rc::new(move |handle, kind, cx| {
                let ico = host.icon();
                let _ = handle.update(cx, |_, window, _| {
                    if let Some(h) = hwnd_of(window) {
                        platform::set_window_icon(h, &ico);
                        if kind == "hud" {
                            platform::set_no_activate(h);
                        }
                    }
                });
            })));
        }

        // Core events -> the model. Terminal output is high volume: whatever has queued up is
        // taken in one go and its `terminal:data` chunks go to the terminal view as one batch,
        // without touching the model (one parse and one repaint per batch).
        let m = model.clone();
        cx.spawn(async move |cx| {
            while let Some(first) = event_rx.next().await {
                let mut batch = vec![first];
                while batch.len() < 512 {
                    match event_rx.try_recv() {
                        Ok(ev) => batch.push(ev),
                        _ => break,
                    }
                }
                cx.update(|cx| {
                    let mut output = String::new();
                    for ev in batch {
                        if ev.channel == "terminal:data" {
                            if let Some(s) = ev.payload.first().and_then(Value::as_str) {
                                output.push_str(s);
                            }
                            continue;
                        }
                        if !output.is_empty() {
                            apm_ui::terminal_view::deliver(&output, cx);
                            output.clear();
                        }
                        if ev.channel == "ai:chat-chunk" {
                            apm_ui::terminal_ai::deliver(&ev.payload, cx);
                            continue;
                        }
                        if ev.channel == "terminal:error" {
                            let message = ev.payload.first().and_then(Value::as_str).unwrap_or("").to_string();
                            apm_ui::terminal_view::deliver_error(message, cx);
                            continue;
                        }
                        m.update(cx, |m, cx| m.on_core_event(ev, cx));
                    }
                    if !output.is_empty() {
                        apm_ui::terminal_view::deliver(&output, cx);
                    }
                });
            }
        })
        .detach();

        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    trim_working_set();
                })
                .detach();
            }
        })
        .detach();

        // Tray: left click opens the window; the menu is 1.x's (rebuilt on `Host::tray_changed`).
        let actions: Arc<Mutex<HashMap<MenuId, tray::Action>>> = Arc::default();
        let tray_icon = TrayIconBuilder::new().with_menu_on_left_click(false).with_tooltip("AWS Profile Manager").build().ok();
        let tray = Rc::new(RefCell::new(TrayState { icon: tray_icon, actions: actions.clone(), shown: None, consoles_in_flight: HashSet::new(), menu: Vec::new() }));
        {
            let mut t = tray.borrow_mut();
            t.rebuild_menu(&core);
            t.refresh_icon(&core);
        }
        let tx_menu = cmd_tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some(a) = actions.lock().unwrap().get(&e.id).cloned() {
                let _ = tx_menu.unbounded_send(AppCmd::Menu(a));
            }
        }));
        let tx_click = cmd_tx.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tx_click.unbounded_send(AppCmd::Open);
            }
        }));
        {
            let tray = tray.clone();
            cx.set_global(apm_ui::hud::HudPlacer(Rc::new(move |cx| hud_placement(&tray, cx))));
        }

        // Commands for the UI thread.
        {
            let (m, b, h, t) = (model.clone(), bridge.clone(), host.clone(), tray.clone());
            cx.spawn(async move |cx| {
                while let Some(cmd) = cmd_rx.next().await {
                    cx.update(|cx| handle_cmd(cmd, &m, &b, &h, &t, cx));
                }
            })
            .detach();
        }
        // Expiry moves with the clock, and the taskbar colour can change while running.
        {
            let tx = cmd_tx.clone();
            cx.spawn(async move |cx| loop {
                cx.background_executor().timer(STATUS_CHECK).await;
                if tx.unbounded_send(AppCmd::TrayStatus).is_err() {
                    break;
                }
            })
            .detach();
        }
        // An update reinstalls and resets the shortcuts to the exe's icon (1.x: every launch).
        {
            let core = core.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(SHORTCUT_SYNC_DELAY).await;
                sync_shortcut_icons(core);
            })
            .detach();
        }

        start_windows(&model, &core, cx);
    });
    drop(runtime);
}
