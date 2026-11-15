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
use apm_core::host::{DialogRequest, Host};
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

/// The core's window into the OS and the UI.
struct AppHost {
    events: fmpsc::UnboundedSender<CoreEvent>,
    /// Test mode: record events, external opens and dialog answers for the flow runner instead of
    /// touching the OS (1.x installExternalOpenCapture).
    #[cfg(feature = "test-hooks")]
    test: Option<control::Shared>,
}

impl Host for AppHost {
    fn emit(&self, channel: &str, payload: Vec<Value>) {
        #[cfg(feature = "test-hooks")]
        if let Some(t) = &self.test {
            let mut ev = vec![Value::String(channel.to_string())];
            ev.extend(payload.iter().cloned());
            t.lock().unwrap().events.push(Value::Array(ev));
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
    let host = Arc::new(AppHost {
        events: event_tx,
        #[cfg(feature = "test-hooks")]
        test: test_recorded.clone(),
    });
    let cfg = test_hooks(CoreConfig::for_user(env!("CARGO_PKG_VERSION")));
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

        // Core events -> the model.
        let m = model.clone();
        cx.spawn(async move |cx| {
            while let Some(ev) = event_rx.next().await {
                cx.update(|cx| m.update(cx, |m, cx| m.on_core_event(ev, cx)));
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
