//! AWS Profile Manager 2.0: owns the process. Starts the core (on a tokio runtime), the GPUI app,
//! the tray icon and the background scheduler, and implements the core's `Host` for this OS.
//!
//! Closing the window destroys it (plan decision: no hidden window holding memory); the process
//! stays in the tray until Quit.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use apm_core::core::{Core, CoreConfig};
use apm_core::host::{DialogRequest, Host, SignInRequest, SignInWindow};
use apm_ui::bridge::{Bridge, CoreEvent};
use apm_ui::model::AppModel;
use futures::channel::mpsc as fmpsc;
use futures::StreamExt;
use gpui::{App, AppContext};
use serde_json::Value;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

#[cfg(feature = "test-hooks")]
mod control;
mod webwin;

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
    /// Test mode: record events, external opens and dialog answers for the flow runner instead of
    /// touching the OS (1.x installExternalOpenCapture).
    #[cfg(feature = "test-hooks")]
    test: Option<control::Shared>,
    /// Placement of the embedded web windows (test mode: right display, unfocused).
    web: webwin::Options,
    /// Window icon for the web windows.
    icon: std::path::PathBuf,
    /// App data directory: the console windows' WebView2 data lives under it.
    app_data: std::path::PathBuf,
}

impl Host for AppHost {
    fn emit(&self, channel: &str, payload: Vec<Value>) {
        #[cfg(feature = "test-hooks")]
        if let Some(t) = &self.test {
            t.lock().unwrap().record(channel, &payload);
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
                icon: Some(self.icon.clone()),
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
                icon: Some(self.icon.clone()),
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
        std::process::Command::new(installer).arg("/S").spawn().map_err(|e| e.to_string())?;
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            std::process::exit(0);
        });
        Ok(())
    }
}

/// Test seams (plan §5.3), the same variables as 1.x's testHooks.ts. Only with the `test-hooks`
/// feature AND `APM_TEST_MODE=1`; release packages are built without the feature.
fn test_hooks(mut cfg: CoreConfig) -> CoreConfig {
    #[cfg(feature = "test-hooks")]
    if std::env::var("APM_TEST_MODE").as_deref() == Ok("1") {
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

enum TrayCmd {
    Open,
    Quit,
}

fn tray_icon_image() -> tray_icon::Icon {
    let png = include_bytes!("../../../../resources/icon.png");
    let img = image::load_from_memory(png).expect("bundled icon").resize(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).expect("tray icon")
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

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    let (event_tx, mut event_rx) = fmpsc::unbounded::<CoreEvent>();
    #[cfg(feature = "test-hooks")]
    let test_recorded: Option<control::Shared> =
        (std::env::var("APM_TEST_MODE").as_deref() == Ok("1")).then(|| Arc::new(std::sync::Mutex::new(control::Recorded::default())));
    let cfg = test_hooks(CoreConfig::for_user(env!("CARGO_PKG_VERSION")));
    #[allow(unused_mut)]
    let mut web = webwin::Options::default();
    #[cfg(feature = "test-hooks")]
    if test_recorded.is_some() {
        web = webwin::Options { test_mode: true, display: Some(std::env::var("APM_WINDOW_DISPLAY").unwrap_or_else(|_| "right".into())) };
    }
    let host = Arc::new(AppHost {
        events: event_tx,
        #[cfg(feature = "test-hooks")]
        test: test_recorded.clone(),
        web,
        icon: cfg.resources_dir.join("icon.ico"),
        app_data: cfg.paths.app_data.clone(),
    });
    let core = {
        let _guard = runtime.enter();
        let core = Core::new(cfg, host).into_shared();
        core.start_scheduler(None);
        core.window_started();
        core
    };
    let bridge = Bridge::new(core, runtime.handle().clone());

    let app = gpui_platform::application().with_assets(apm_ui::assets::Assets).with_quit_mode(gpui::QuitMode::Explicit);
    app.run(move |cx: &mut App| {
        apm_ui::init(cx);
        #[cfg(feature = "test-hooks")]
        if let Some(rec) = test_recorded.clone() {
            control::start(bridge.clone(), rec, cx);
        }
        #[cfg(feature = "test-hooks")]
        if std::env::var("APM_TEST_MODE").as_deref() == Ok("1") {
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
        let model = cx.new(|cx| AppModel::new(bridge.clone(), gpui::WindowAppearance::Dark, cx));

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

        // Tray: left click opens; menu has Open and Quit (profile items come with the tray step).
        let (tx, rx) = mpsc::channel::<TrayCmd>();
        let menu = Menu::new();
        let open_item = MenuItem::new("Open AWS Profile Manager", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        menu.append_items(&[&open_item, &quit_item]).expect("tray menu");
        let (open_id, quit_id) = (open_item.id().clone(), quit_item.id().clone());
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("AWS Profile Manager")
            .with_icon(tray_icon_image())
            .build()
            .expect("tray");
        Box::leak(Box::new(tray)); // lives as long as the process
        let tx_menu = tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let cmd = if e.id == open_id { Some(TrayCmd::Open) } else if e.id == quit_id { Some(TrayCmd::Quit) } else { None };
            if let Some(c) = cmd {
                let _ = tx_menu.send(c);
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tx.send(TrayCmd::Open);
            }
        }));
        let m = model.clone();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            while let Ok(cmd) = rx.try_recv() {
                let _ = cx.update(|cx| match cmd {
                    TrayCmd::Open => apm_ui::open_main_window(&m, cx),
                    TrayCmd::Quit => cx.quit(),
                });
            }
        })
        .detach();

        apm_ui::open_main_window(&model, cx);
    });
    drop(runtime);
}
