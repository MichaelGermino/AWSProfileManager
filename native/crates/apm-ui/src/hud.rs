//! The tray HUD: a small progress / error card parked against the taskbar while the tray menu's
//! "AWS Console" launch runs (1.x `trayHud.ts` + `resources/tray-hud.html`).
//!
//! A tray menu dismisses before its click handler runs and cannot show progress, so feedback goes
//! in a window of our own where the menu just was. It never takes focus. Timing rules are 1.x's:
//! the busy card waits 350 ms before it appears (a quick launch shows nothing), stays at least
//! 600 ms once shown, gives up after 3 minutes; an error shows at once and dismisses after 6 s.
//! Every call bumps a generation, so a stale timer can never undo a newer show or hide.
//!
//! 2.0: the window is destroyed when hidden (no idle window in the tray, plan §4); a GPUI window
//! opens in a few milliseconds, so there is nothing to prewarm.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, size, svg, AnyWindowHandle, App, Bounds, DisplayId, Entity,
    FontWeight, IntoElement, Pixels, Role, SharedString, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions,
};

use crate::theme::{Mode, Palette, FONT_SANS};

/// Visible card.
pub const CARD_W: f32 = 300.;
pub const CARD_H: f32 = 64.;
/// Transparent margin around the card for its shadow.
pub const SHADOW_PAD: f32 = 10.;

const BUSY_SHOW_DELAY: Duration = Duration::from_millis(350);
const MIN_VISIBLE: Duration = Duration::from_millis(600);
const ERROR_DISMISS: Duration = Duration::from_secs(6);
const MAX_BUSY: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudState {
    Busy,
    Error,
}

/// Where the card rests: its rect in GPUI screen coordinates (logical pixels) and the display.
#[derive(Debug, Clone, Copy)]
pub struct HudPlacement {
    pub card: Bounds<Pixels>,
    pub display: Option<DisplayId>,
}

/// Set by the app: where the card goes (next to the tray icon, against the taskbar).
pub struct HudPlacer(pub Rc<dyn Fn(&mut App) -> Option<HudPlacement>>);
impl gpui::Global for HudPlacer {}

/// The palette the card uses (set by the app from the theme setting and the OS appearance).
pub struct HudPalette(pub Palette);
impl gpui::Global for HudPalette {}

#[derive(Default)]
struct HudGlobal {
    generation: u64,
    window: Option<(AnyWindowHandle, Entity<HudView>)>,
    shown_at: Option<Instant>,
}
impl gpui::Global for HudGlobal {}

fn state(cx: &mut App) -> &mut HudGlobal {
    if cx.try_global::<HudGlobal>().is_none() {
        cx.set_global(HudGlobal::default());
    }
    cx.global_mut::<HudGlobal>()
}

fn bump(cx: &mut App) -> u64 {
    let s = state(cx);
    s.generation += 1;
    s.generation
}

fn current(cx: &mut App) -> u64 {
    state(cx).generation
}

/// Run `f` after `delay` unless a newer call has superseded `gen` by then.
fn after(delay: Duration, gen: u64, cx: &mut App, f: impl FnOnce(&mut App) + 'static) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(delay).await;
        cx.update(|cx| {
            if current(cx) == gen {
                f(cx);
            }
        });
    })
    .detach();
}

/// Show the spinner card after a short delay, so quick work never flickers it.
pub fn show_busy(title: impl Into<String>, subtitle: impl Into<String>, cx: &mut App) {
    let (title, subtitle) = (title.into(), subtitle.into());
    let gen = bump(cx);
    after(BUSY_SHOW_DELAY, gen, cx, move |cx| {
        paint(HudState::Busy, title, subtitle, cx);
        after(MAX_BUSY, gen, cx, hide);
    });
}

/// Swap to an error card, which dismisses itself after a few seconds.
pub fn show_error(title: impl Into<String>, subtitle: impl Into<String>, cx: &mut App) {
    let gen = bump(cx);
    paint(HudState::Error, title.into(), subtitle.into(), cx);
    after(ERROR_DISMISS, gen, cx, hide);
}

/// Hide (destroy) the card, honouring the minimum time on screen.
pub fn hide(cx: &mut App) {
    let gen = bump(cx);
    let shown_for = state(cx).shown_at.map(|t| t.elapsed());
    match shown_for {
        Some(d) if d < MIN_VISIBLE => after(MIN_VISIBLE - d, gen, cx, destroy),
        _ => destroy(cx),
    }
}

/// Whether a card is on screen (test control).
pub fn is_visible(cx: &mut App) -> bool {
    state(cx).window.is_some()
}

fn destroy(cx: &mut App) {
    let s = state(cx);
    s.shown_at = None;
    if let Some((handle, _)) = s.window.take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// Bottom-right of the primary display's work area: where the card goes without a placer.
fn default_placement(cx: &mut App) -> Option<HudPlacement> {
    let display = match cx.try_global::<crate::TestPlacement>().map(|t| t.0) {
        Some(id) => cx.find_display(id),
        None => cx.primary_display(),
    }?;
    let area = display.visible_bounds();
    let margin = px(4.);
    let origin = gpui::point(
        area.origin.x + area.size.width - px(CARD_W) - margin,
        area.origin.y + area.size.height - px(CARD_H) - margin,
    );
    Some(HudPlacement { card: Bounds { origin, size: size(px(CARD_W), px(CARD_H)) }, display: Some(display.id()) })
}

fn paint(st: HudState, title: String, subtitle: String, cx: &mut App) {
    if let Some((_, view)) = state(cx).window.clone() {
        view.update(cx, |v, cx| {
            v.state = st;
            v.title = title.into();
            v.subtitle = subtitle.into();
            cx.notify();
        });
        return;
    }
    // Test runs keep every window on the test display.
    let placed = if cx.try_global::<crate::TestPlacement>().is_some() {
        None
    } else {
        cx.try_global::<HudPlacer>().map(|p| p.0.clone()).and_then(|f| f(cx))
    };
    let Some(place) = placed.or_else(|| default_placement(cx)) else { return };
    let palette = cx.try_global::<HudPalette>().map(|p| p.0).unwrap_or_else(|| Palette::new(crate::theme::Style::Modern, Mode::Dark));
    let pad = px(SHADOW_PAD);
    let bounds = Bounds {
        origin: gpui::point(place.card.origin.x - pad, place.card.origin.y - pad),
        size: size(place.card.size.width + pad * 2., place.card.size.height + pad * 2.),
    };
    let mut created = None;
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: place.display,
            window_background: WindowBackgroundAppearance::Transparent,
            app_id: Some("AWSProfileManager".into()),
            ..Default::default()
        },
        |_, cx| {
            let view = cx.new(|_| HudView { state: st, title: title.into(), subtitle: subtitle.into(), palette });
            created = Some(view.clone());
            view
        },
    );
    if let (Ok(handle), Some(view)) = (opened, created) {
        let any: AnyWindowHandle = handle.into();
        let s = state(cx);
        s.window = Some((any, view));
        s.shown_at = Some(Instant::now());
        crate::notify_window_opened(any, "hud", cx);
    }
}

pub struct HudView {
    state: HudState,
    title: SharedString,
    subtitle: SharedString,
    palette: Palette,
}

impl Render for HudView {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let dark = p.mode == Mode::Dark;
        let error = self.state == HudState::Error;
        // tray-hud.html colours (theme.css).
        let danger_edge = if dark { gpui::rgb(0x5c2a2c) } else { gpui::rgb(0xf3c4c4) };
        // Lucide glyphs like the rest of the app; the spinner is the shared calm one.
        let glyph = if error {
            crate::widgets::icon("circle-x", 18., p.danger).into_any_element()
        } else {
            crate::motion::spin(svg().path(crate::assets::icon("loader-circle")).size(px(18.)).flex_shrink_0().text_color(p.accent), "hud-spin").into_any_element()
        };
        let label = SharedString::from(format!("{}: {}", self.title, self.subtitle));
        div().size_full().p(px(SHADOW_PAD)).font_family(FONT_SANS).child(
            div()
                .id("tray-hud")
                .role(if error { Role::Alert } else { Role::Status })
                .aria_label(label)
                .size_full()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .rounded(px(10.))
                .bg(p.panel)
                .border_1()
                .border_color(if error { danger_edge } else { p.border })
                .shadow_md()
                .child(glyph)
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(p.text).truncate().child(self.title.clone()))
                        .child(div().text_size(px(12.)).text_color(p.text_muted).truncate().child(self.subtitle.clone())),
                ),
        )
    }
}
