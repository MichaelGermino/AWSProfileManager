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
                text_faint: rgb(0x8f95a2),
                primary: rgb(0x5865f2),
                on_primary: rgb(0xffffff),
                accent: rgb(0x848bf6),
                success: rgb(0x41a862),
                warning: rgb(0xe8a93a),
                danger: rgb(0xf16974),
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
                border: rgb(0xdcdde4),
                border_strong: rgb(0xc6c8d1),
                text: rgb(0x16171b),
                text_muted: rgb(0x4f5361),
                text_faint: rgb(0x636874),
                primary: rgb(0x4752c4),
                on_primary: rgb(0xffffff),
                accent: rgb(0x4752c4),
                success: rgb(0x1b7741),
                warning: rgb(0x915e0a),
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
                accent: rgb(0x98a0f7),
                success: rgb(0x62b77d),
                warning: rgb(0xf0b232),
                danger: rgb(0xf88386),
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
                border: rgb(0xcdd0d6),
                border_strong: rgb(0xb3b7c0),
                text: rgb(0x060607),
                text_muted: rgb(0x4e5058),
                text_faint: rgb(0x5c5e66),
                primary: rgb(0x5865f2),
                on_primary: rgb(0xffffff),
                accent: rgb(0x4752c4),
                success: rgb(0x19723e),
                warning: rgb(0x865c00),
                danger: rgb(0xb83335),
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

    /// The groove of a segmented control or view toggle. Dark: a well. Light: a soft grey groove,
    /// so the chosen segment (white) stands out of it rather than sinking into it.
    pub fn track(&self) -> Rgba {
        if self.is_dark() { self.sunken } else { self.raised }
    }

    /// The fill of a text field, matching gpui-component's inputs (its `input_background`): the page
    /// colour in light mode, a faint tint of the border in dark mode. Our own pickers use it too, so
    /// a select box and a text box beside it look alike.
    pub fn field(&self) -> Rgba {
        if self.is_dark() { Rgba { a: 0.3, ..self.border } } else { self.bg }
    }

    /// The chosen segment of a segmented control, and the current page in the navigation rail.
    /// Dark: a lifted grey. Light: white with a hairline shadow (see [`chip_shadow`]); the light
    /// `raised` grey is too close to the rail and the light wells to read as chosen.
    pub fn chip(&self) -> Rgba {
        if self.is_dark() { self.raised } else { self.panel }
    }

    /// The hairline shadow under a [`Palette::chip`] in light mode (none in dark mode).
    pub fn chip_shadow(&self) -> Vec<gpui::BoxShadow> {
        if self.is_dark() {
            Vec::new()
        } else {
            vec![
                gpui::BoxShadow::new(gpui::px(0.), gpui::px(1.), gpui::hsla(0., 0., 0., 0.10)).blur_radius(gpui::px(2.)),
                gpui::BoxShadow::new(gpui::px(0.), gpui::px(0.), gpui::hsla(0., 0., 0., 0.06)).spread_radius(gpui::px(1.)),
            ]
        }
    }

    /// The keyboard focus ring: a 2px inset ring, so it never shifts layout or spills out of a
    /// clipped container. `on` is the fill it sits on (the primary button needs a light ring).
    pub fn focus_ring(&self, on_primary: bool) -> Vec<gpui::BoxShadow> {
        let c: Hsla = if on_primary { self.on_primary.into() } else { self.accent.into() };
        vec![gpui::BoxShadow::new(gpui::px(0.), gpui::px(0.), c).spread_radius(gpui::px(2.)).inset()]
    }

    /// Surfaces over the background video (1.x `[data-glass='on']`): the window background is
    /// left to the tint layer ([`Palette::glass_tint`]), and the structural surfaces (title bar,
    /// rail, cards, wells) become translucent so the video carries through. Borders do the work the
    /// fills stop doing. Hover, selection and buttons stay solid. Dialogs and menus keep using the
    /// solid palette (`AppModel::palette`), as in 1.x.
    pub fn glass(&self) -> Self {
        let a = |c: Rgba, alpha: f32| Rgba { a: alpha, ..c };
        let dark = self.is_dark();
        Palette {
            bg: a(self.bg, 0.0),
            rail: a(self.rail, if dark { 0.45 } else { 0.5 }),
            panel: a(self.panel, if dark { 0.55 } else { 0.6 }),
            sunken: a(self.sunken, if dark { 0.5 } else { 0.55 }),
            border: if dark { Rgba { r: 1., g: 1., b: 1., a: 0.12 } } else { Rgba { r: 0., g: 0., b: 0., a: 0.10 } },
            ..*self
        }
    }

    /// The tint laid over the whole video, under the UI. GPUI cannot blur what is behind an
    /// element, so the video itself is blurred by the decoder and this tint does the rest: its
    /// opacity follows the video's brightness (`luminance`, 0–1 linear) so body text keeps about
    /// 4.5:1 against the average frame: darker over bright footage in dark mode, lighter over dark
    /// footage in light mode, never below a floor that keeps the look consistent.
    pub fn glass_tint(&self, luminance: f32) -> Rgba {
        // Light text on dark tint holds up better over colour than dark text on light tint.
        let alpha = glass_alpha(self, luminance, if self.is_dark() { 0.45 } else { 0.55 });
        Rgba { a: alpha, ..self.bg }
    }

    /// The opacity a surface of this palette's background colour needs over video of mean
    /// `luminance` for ~4.5:1 text contrast; at least `floor`, at most 0.88.
    pub fn glass_alpha(&self, luminance: f32, floor: f32) -> f32 {
        glass_alpha(self, luminance, floor)
    }
}

fn glass_alpha(p: &Palette, video: f32, floor: f32) -> f32 {
    let lum = |c: Rgba| 0.2126 * c.r.powf(2.2) + 0.7152 * c.g.powf(2.2) + 0.0722 * c.b.powf(2.2);
    let surface = lum(p.bg);
    let video = video.clamp(0.0, 1.0);
    // Composite luminance a·surface + (1−a)·video must clear the contrast limit for the text.
    let needed = if p.is_dark() {
        let max_bg = (lum(p.text) + 0.05) / 4.5 - 0.05;
        if video <= max_bg { 0.0 } else { (video - max_bg) / (video - surface).max(1e-3) }
    } else {
        let min_bg = 4.5 * (lum(p.text_muted) + 0.05) - 0.05;
        if video >= min_bg { 0.0 } else { (min_bg - video) / (surface - video).max(1e-3) }
    };
    needed.clamp(floor, 0.88)
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
    // Through `Theme::update`, not `global_mut`: buttons and lists paint from the theme's tokens,
    // which only follow the colours when the edit goes through `update`.
    let p = *p;
    Theme::update(cx, move |theme| {
    theme.font_family = FONT_SANS.into();
    // The rem: gpui-component sizes its controls in rems, so 16 px gives 32 px text fields (our
    // control height) with 14 px text (our body size). Our own views use px throughout.
    theme.font_size = gpui::px(16.);
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
    c.secondary_active = h(p.hover);
    c.button = h(p.raised);
    c.button_foreground = h(p.text);
    c.button_hover = h(p.hover);
    c.button_active = h(p.hover);
    c.button_secondary = h(p.raised);
    c.button_secondary_foreground = h(p.text);
    c.button_secondary_hover = h(p.hover);
    c.button_secondary_active = h(p.hover);
    c.button_primary = h(p.primary);
    c.button_primary_foreground = h(p.on_primary);
    c.button_primary_hover = h(p.primary);
    c.button_primary_active = h(p.primary);
    c.primary_active = h(p.primary);
    c.list_active_border = h(p.accent);
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
    });
}
