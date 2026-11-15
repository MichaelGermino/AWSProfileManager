//! AWS Profile Manager 2.0 views (GPUI). Everything the app does lives in `apm-core`; this crate
//! draws it and turns clicks into IPC-contract calls (see `bridge`).

pub mod a11y;
pub mod assets;
pub mod bridge;
pub mod dialogs;
pub mod dnd;
pub mod form;
pub mod import;
pub mod lock;
pub mod log_viewer;
pub mod hud;
pub mod splash;
pub mod menu;
pub mod picker;
pub mod profile_icons;
pub mod model;
pub mod onboarding;
pub mod profiles;
pub mod settings;
pub mod shell;
pub mod status;
pub mod terminal_ai;
pub mod terminal_page;
pub mod terminal_view;
pub mod theme;
pub mod widgets;
pub mod wizard;

use gpui::{prelude::*, px, size, AnyWindowHandle, App, Bounds, Entity, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};

use crate::model::AppModel;
use crate::shell::Shell;

/// Register fonts and component styles. Call once at start-up.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    terminal_view::init(cx);
    let _ = cx.text_system().add_fonts(assets::fonts());
}

/// Test-only window placement (apm-app's `APM_WINDOW_DISPLAY` hook): open the window on this
/// display and without taking focus, so automated runs stay off the monitor someone is using.
pub struct TestPlacement(pub gpui::DisplayId);
impl gpui::Global for TestPlacement {}

/// The main window while it is open (other windows: log viewer, tray HUD, splash).
struct MainWindow(AnyWindowHandle);
impl gpui::Global for MainWindow {}

/// Called with every window this crate opens, so the app can apply OS details (the window icon).
pub struct WindowOpenedHook(pub std::rc::Rc<dyn Fn(AnyWindowHandle, &'static str, &mut App)>);
impl gpui::Global for WindowOpenedHook {}

pub(crate) fn notify_window_opened(handle: AnyWindowHandle, kind: &'static str, cx: &mut App) {
    if let Some(hook) = cx.try_global::<WindowOpenedHook>().map(|h| h.0.clone()) {
        hook(handle, kind, cx);
    }
}

/// The main window, if it is open.
pub fn main_window(cx: &App) -> Option<AnyWindowHandle> {
    let handle = cx.try_global::<MainWindow>()?.0;
    cx.windows().contains(&handle).then_some(handle)
}

/// Open (or focus) the main window. Closing it destroys it; the app stays in the tray.
pub fn open_main_window(model: &Entity<AppModel>, cx: &mut App) {
    open_main_window_with(model, true, cx);
}

/// Open the main window, or focus it when it is open. `show: false` creates it hidden (behind the
/// splash); [`show_main_window`] shows it.
pub fn open_main_window_with(model: &Entity<AppModel>, show: bool, cx: &mut App) {
    if let Some(handle) = main_window(cx) {
        if show {
            show_main_window(cx);
        }
        let _ = handle;
        return;
    }
    let test_display = cx.try_global::<TestPlacement>().map(|t| t.0);
    let bounds = Bounds::centered(test_display, size(px(1280.), px(800.)), cx);
    let model = model.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.), px(560.))),
            app_id: Some("AWSProfileManager".into()),
            focus: test_display.is_none() && show,
            show,
            display_id: test_display,
            titlebar: Some(gpui::TitlebarOptions { title: Some("AWS Profile Manager".into()), ..TitleBar::title_bar_options() }),
            ..TitleBar::window_options()
        },
        |window, cx| {
            let a = window.appearance();
            model.update(cx, |m, cx| m.set_os_appearance(a, cx));
            let view = cx.new(|cx| Shell::new(model.clone(), window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
    if let Ok(handle) = opened {
        let any: AnyWindowHandle = handle.into();
        cx.set_global(MainWindow(any));
        notify_window_opened(any, "main", cx);
    }
    if test_display.is_none() && show {
        cx.activate(true);
    }
}

/// Show and focus the main window (one created hidden behind the splash, or one already open).
pub fn show_main_window(cx: &mut App) {
    let Some(handle) = main_window(cx) else { return };
    let test = cx.try_global::<TestPlacement>().is_some();
    let _ = handle.update(cx, |_, window, _| {
        if !test {
            window.activate_window();
        }
    });
    if !test {
        cx.activate(true);
    }
}

/// Open the auth log viewer window (or focus it).
pub fn open_log_viewer(model: &Entity<AppModel>, cx: &mut App) {
    log_viewer::open(model, cx);
}
