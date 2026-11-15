//! AWS Profile Manager 2.0 views (GPUI). Everything the app does lives in `apm-core`; this crate
//! draws it and turns clicks into IPC-contract calls (see `bridge`).

pub mod a11y;
pub mod assets;
pub mod bridge;
pub mod dialogs;
pub mod form;
pub mod lock;
pub mod menu;
pub mod picker;
pub mod model;
pub mod profiles;
pub mod settings;
pub mod shell;
pub mod status;
pub mod theme;
pub mod widgets;

use gpui::{prelude::*, px, size, App, Bounds, Entity, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};

use crate::model::AppModel;
use crate::shell::Shell;

/// Register fonts and component styles. Call once at start-up.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    let _ = cx.text_system().add_fonts(assets::fonts());
}

/// Test-only window placement (apm-app's `APM_WINDOW_DISPLAY` hook): open the window on this
/// display and without taking focus, so automated runs stay off the monitor someone is using.
pub struct TestPlacement(pub gpui::DisplayId);
impl gpui::Global for TestPlacement {}

/// Open (or focus) the main window. Closing it destroys it; the app stays in the tray.
pub fn open_main_window(model: &Entity<AppModel>, cx: &mut App) {
    if let Some(handle) = cx.windows().first().cloned() {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let test_display = cx.try_global::<TestPlacement>().map(|t| t.0);
    let bounds = Bounds::centered(test_display, size(px(1280.), px(800.)), cx);
    let model = model.clone();
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.), px(560.))),
            app_id: Some("AWSProfileManager".into()),
            focus: test_display.is_none(),
            display_id: test_display,
            ..TitleBar::window_options()
        },
        |window, cx| {
            let a = window.appearance();
            model.update(cx, |m, cx| m.set_os_appearance(a, cx));
            let view = cx.new(|cx| Shell::new(model.clone(), window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
    if test_display.is_none() {
        cx.activate(true);
    }
}
