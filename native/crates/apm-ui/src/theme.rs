//! Design tokens for 2.0 (plan §10.3, "direction B, refined 1.x"; mockups at
//! https://claude.ai/artifact/5BG8wFHPVNfGpZj55XnUe8).
//!
//! Neutral surfaces; the 1.x blurple only on the main button and the selection; green / amber / red
//! only for credential state. Two independent settings choose the palette:
//! - `theme`: system (follows the OS), light or dark;
//! - `appStyle`: modern (this palette) or classic (the 1.x Discord-like colours).

use gpui::{rgb, Hsla, Rgba, WindowAppearance};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Modern,
    Classic,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub mode: Mode,
    pub style: Style,
    /// Window background (main content).
    pub bg: Rgba,
    /// Title bar and navigation rail.
    pub rail: Rgba,
    /// Raised surfaces: side panel, cards, menus, dialogs.
    pub panel: Rgba,
    /// Inputs and wells.
    pub sunken: Rgba,
    /// Secondary buttons, selected segments.
    pub raised: Rgba,
    pub hover: Rgba,
    /// Selected row.
    pub selected: Rgba,
    pub border: Rgba,
    pub border_strong: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_faint: Rgba,
    /// The one strong colour: main button.
    pub primary: Rgba,
    pub on_primary: Rgba,
    /// Selection marker, focus ring, links.
    pub accent: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub danger: Rgba,
    /// Fill for destructive buttons (white text on it reaches 4.5:1).
    pub danger_solid: Rgba,
    /// Dimming behind dialogs.
    pub scrim: Hsla,
}

impl Palette {
    pub fn new(style: Style, mode: Mode) -> Self {
        match (style, mode) {
            (Style::Modern, Mode::Dark) => Palette {
                mode,
                style,
                bg: rgb(0x17181c),
                rail: rgb(0x111215),
                panel: rgb(0x1c1e23),
                sunken: rgb(0x121317),
                raised: rgb(0x262830),
                hover: rgb(0x2b2e37),
                selected: rgb(0x24263a),
                border: rgb(0x2b2e36),
                border_strong: rgb(0x3a3d47),
                text: rgb(0xeceef2),
                text_muted: rgb(0xa4a9b4),
                text_faint: rgb(0x8d93a0),
                primary: rgb(0x5865f2),
                on_primary: rgb(0xffffff),
                accent: rgb(0x8189f6),
                success: rgb(0x3ba55d),
                warning: rgb(0xe8a93a),
                danger: rgb(0xf0616d),
                danger_solid: rgb(0xc4313d),
                scrim: gpui::hsla(0., 0., 0., 0.55),
            },
            (Style::Modern, Mode::Light) => Palette {
                mode,
                style,
                bg: rgb(0xf7f7f9),
                rail: rgb(0xeeeff3),
                panel: rgb(0xffffff),
                sunken: rgb(0xffffff),
                raised: rgb(0xeff0f4),
                hover: rgb(0xe7e8ee),
                selected: rgb(0xeceefd),
                border: rgb(0xe1e2e8),
                border_strong: rgb(0xcfd1d9),
                text: rgb(0x16171b),
                text_muted: rgb(0x4f5361),
                text_faint: rgb(0x666b78),
                primary: rgb(0x4752c4),
                on_primary: rgb(0xffffff),
                accent: rgb(0x4752c4),
                success: rgb(0x1d7f45),
                warning: rgb(0x94600a),
                danger: rgb(0xc8232f),
                danger_solid: rgb(0xc8232f),
                scrim: gpui::hsla(0., 0., 0., 0.35),
            },
            (Style::Classic, Mode::Dark) => Palette {
                mode,
                style,
                bg: rgb(0x313338),
                rail: rgb(0x1e1f22),
                panel: rgb(0x2b2d31),
                sunken: rgb(0x1e1f22),
                raised: rgb(0x3b3d43),
                hover: rgb(0x393c41),
                selected: rgb(0x404249),
                border: rgb(0x3f4147),
                border_strong: rgb(0x4e5058),
                text: rgb(0xf2f3f5),
                text_muted: rgb(0xb5bac1),
                text_faint: rgb(0xa3a9b1),
                primary: rgb(0x5865f2),
                on_primary: rgb(0xffffff),
                accent: rgb(0x7983f5),
                success: rgb(0x3ba55d),
                warning: rgb(0xf0b232),
                danger: rgb(0xf5595d),
                danger_solid: rgb(0xda373c),
                scrim: gpui::hsla(0., 0., 0., 0.6),
            },
            // 1.x had no light mode; classic light keeps the blurple on neutral light surfaces.
            (Style::Classic, Mode::Light) => Palette {
                mode,
                style,
                bg: rgb(0xf2f3f5),
                rail: rgb(0xe3e5e8),
                panel: rgb(0xffffff),
                sunken: rgb(0xe3e5e8),
                raised: rgb(0xebedef),
                hover: rgb(0xdfe1e5),
                selected: rgb(0xd7daf9),
                border: rgb(0xd4d7dc),
                border_strong: rgb(0xb8bcc4),
                text: rgb(0x060607),
                text_muted: rgb(0x4e5058),
                text_faint: rgb(0x5c5e66),
                primary: rgb(0x5865f2),
                on_primary: rgb(0xffffff),
                accent: rgb(0x4752c4),
                success: rgb(0x1f8b4c),
                warning: rgb(0x9a6a00),
                danger: rgb(0xd83c3e),
                danger_solid: rgb(0xc8282b),
                scrim: gpui::hsla(0., 0., 0., 0.35),
            },
        }
    }

    /// From the saved settings and the OS appearance.
    pub fn from_settings(theme: Option<&str>, app_style: Option<&str>, os: WindowAppearance) -> Self {
        let mode = match theme {
            Some("light") => Mode::Light,
            Some("dark") => Mode::Dark,
            _ => match os {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Mode::Light,
                _ => Mode::Dark,
            },
        };
        let style = if app_style == Some("classic") { Style::Classic } else { Style::Modern };
        Palette::new(style, mode)
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }
}

/// Type and spacing scale (px). One place, so views stay on the same grid.
pub mod size {
    pub const TITLE_BAR: f32 = 40.;
    pub const RAIL: f32 = 56.;
    /// The expanded sidebar (icons and labels).
    pub const SIDEBAR: f32 = 200.;
    pub const ROW: f32 = 52.;
    pub const CONTROL: f32 = 32.;
    pub const BUTTON: f32 = 36.;
    pub const RADIUS: f32 = 10.;
    pub const RADIUS_LG: f32 = 12.;
    pub const SIDE_PANEL: f32 = 400.;
    pub const TEXT: f32 = 14.;
    pub const TEXT_SM: f32 = 13.;
    pub const TEXT_XS: f32 = 12.;
}

pub const FONT_SANS: &str = "Geist";
pub const FONT_MONO: &str = "Geist Mono";

/// Make gpui-component's widgets (inputs, menus, tooltips, title-bar buttons, scrollbars) use the
/// same palette and type as our own views.
pub fn sync_component_theme(p: &Palette, cx: &mut gpui::App) {
    use gpui_component::theme::{Theme, ThemeMode};
    Theme::change(if p.is_dark() { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    let h = |c: Rgba| -> Hsla { c.into() };
    let theme = Theme::global_mut(cx);
    theme.font_family = FONT_SANS.into();
    theme.font_size = gpui::px(size::TEXT);
    let c = &mut theme.colors;
    c.background = h(p.bg);
    c.foreground = h(p.text);
    c.border = h(p.border);
    c.input = h(p.border);
    c.ring = h(p.accent);
    c.caret = h(p.text);
    c.selection = { let mut s = h(p.accent); s.a = 0.35; s };
    c.muted = h(p.raised);
    c.muted_foreground = h(p.text_muted);
    c.accent = h(p.hover);
    c.accent_foreground = h(p.text);
    c.popover = h(p.panel);
    c.popover_foreground = h(p.text);
    c.primary = h(p.primary);
    c.primary_foreground = h(p.on_primary);
    c.primary_hover = h(p.primary);
    c.secondary = h(p.raised);
    c.secondary_foreground = h(p.text);
    c.secondary_hover = h(p.hover);
    c.list_hover = h(p.hover);
    c.list_active = h(p.selected);
    c.title_bar = h(p.rail);
    c.title_bar_border = h(p.border);
    c.window_border = h(p.border);
    c.scrollbar = gpui::transparent_black();
    c.scrollbar_thumb = h(p.border_strong);
    c.scrollbar_thumb_hover = h(p.text_faint);
    c.danger = h(p.danger);
    c.warning = h(p.warning);
    c.success = h(p.success);
    c.link = h(p.accent);
    c.overlay = p.scrim;
    cx.refresh_windows();
}
