//! The start-up splash (1.x `createSplash` + `resources/splash.html`): a 280x280 frameless card
//! with the selected app icon, a quiet indeterminate bar and "Starting", shown while the main
//! window is built behind it. It never takes focus; the app closes it once the main window shows.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    div, img, prelude::*, px, relative, size, Animation, AnimationExt as _, AnyWindowHandle, App, Bounds, IntoElement, Role, Window, WindowBounds, WindowKind,
    WindowOptions,
};

use crate::theme::{Mode, Palette, FONT_SANS};

pub const SIZE: f32 = 280.;

pub struct SplashView {
    icon: PathBuf,
    palette: Palette,
}

impl Render for SplashView {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let p = self.palette;
        // The lock screen's backdrop (accent trace, dot field) and the accent for the bar, so the
        // first thing on screen already looks like the app.
        let (track, bar) = (if p.mode == Mode::Dark { p.raised } else { p.border }, p.accent);
        div()
            .id("splash")
            .role(Role::Application)
            .aria_label("AWS Profile Manager: starting")
            .size_full()
            .relative()
            .bg(crate::widgets::identity_bg(&p))
            .child(crate::widgets::identity_field(&p))
            .border_1()
            .border_color(p.border)
            .font_family(FONT_SANS)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .child(img(self.icon.clone()).size(px(72.)))
            .child(div().w(px(96.)).h(px(2.)).rounded(px(1.)).bg(track).overflow_hidden().relative().child(
                div().absolute().top_0().h_full().w(px(32.)).bg(bar).with_animation(
                    "splash-slide",
                    Animation::new(Duration::from_millis(1200)).repeat().with_easing(gpui::ease_out_quint()),
                    |d, t| d.left(relative(-32. / 96. + t * (128. / 96.))),
                ),
            ))
            .child(div().text_size(px(13.)).text_color(p.text_muted).child("Starting"))
    }
}

/// Open the splash centred on the primary display (the test display in test runs), unfocused.
pub fn open(icon: PathBuf, palette: Palette, cx: &mut App) -> Option<AnyWindowHandle> {
    let display = cx.try_global::<crate::TestPlacement>().map(|t| t.0);
    let bounds = Bounds::centered(display, size(px(SIZE), px(SIZE)), cx);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: display,
                app_id: Some("AWSProfileManager".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| SplashView { icon, palette }),
        )
        .ok()?;
    let any: AnyWindowHandle = handle.into();
    crate::notify_window_opened(any, "splash", cx);
    Some(any)
}

pub fn close(handle: AnyWindowHandle, cx: &mut App) {
    let _ = handle.update(cx, |_, window, _| window.remove_window());
}
