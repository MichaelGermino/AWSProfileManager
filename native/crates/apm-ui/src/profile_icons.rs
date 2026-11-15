//! The profile icon set and colours (1.x src/renderer/data/profileIcons.ts): Heroicons outline,
//! MIT licence (assets/icons/profile/LICENSE-HEROICONS). A profile stores the icon id in
//! `iconName` and a #rrggbb colour in `iconColor`; neither set means the defaults below.

macro_rules! profile_icons {
    ($(($id:literal, $search:literal)),* $(,)?) => {
        /// id (saved as `iconName`), search words for the picker, the SVG.
        pub const PROFILE_ICONS: &[(&str, &str, &[u8])] = &[
            $( ($id, $search, include_bytes!(concat!("../assets/icons/profile/", $id, ".svg"))), )*
        ];
    };
}

profile_icons!(
    ("user", "user person account profile"),
    ("users", "users people team group"),
    ("briefcase", "briefcase work job office"),
    ("building-office", "building office company"),
    ("cloud", "cloud aws"),
    ("server", "server stack compute"),
    ("key", "key credential access"),
    ("shield-check", "shield security lock"),
    ("cog", "cog settings gear"),
    ("folder", "folder directory"),
    ("document", "document file"),
    ("chart-bar", "chart bar graph"),
    ("cube", "cube box package"),
    ("bolt", "bolt lightning power"),
    ("globe", "globe world web"),
    ("home", "home house"),
    ("command-line", "terminal command dev"),
    ("beaker", "beaker lab test flask"),
    ("rocket", "rocket launch deploy"),
    ("puzzle-piece", "puzzle piece plugin"),
    ("circle-stack", "layers stack"),
    ("star", "star favorite"),
    ("heart", "heart favorite"),
    ("tag", "tag label"),
    ("identification", "id card identification"),
    ("envelope", "email envelope mail"),
    ("phone", "phone call"),
    ("academic-cap", "academic cap education"),
    ("banknotes", "money bank payment"),
    ("squares-2x2", "grid app dashboard"),
    ("cpu-chip", "chip cpu tech"),
    ("lock-closed", "lock secure"),
    ("arrow-path", "refresh sync"),
    ("sparkles", "sparkles magic"),
    ("code-bracket-square", "code bracket square dev"),
    ("eye-dropper", "eye dropper color picker"),
    ("face-frown", "face frown sad"),
    ("forward", "forward play next"),
    ("hand-thumbs-down", "hand thumbs down dislike"),
    ("hand-thumbs-up", "hand thumbs up like"),
    ("light-bulb", "light bulb idea"),
    ("paper-clip", "paper clip attach"),
    ("pencil-square", "pencil square edit"),
    ("server-stack", "server stack servers"),
    ("wrench", "wrench tool"),
);

/// Shown when a profile has no (known) icon.
pub const DEFAULT_ICON: &str = "user";

/// 1.x PRESET_ICON_COLORS (also the folder colours).
pub const PRESET_COLORS: [&str; 20] = [
    "#64748b", "#3b82f6", "#0ea5e9", "#06b6d4", "#10b981", "#22c55e", "#84cc16", "#eab308", "#f59e0b", "#ef4444", "#f43f5e", "#ec4899", "#a855f7", "#6366f1",
    "#0f172a", "#475569", "#94a3b8", "#c2410c", "#15803d", "#1e40af",
];

/// Shown when a profile has no (valid) colour: slate-400.
pub const DEFAULT_COLOR: &str = "#94a3b8";

const PREFIX: &str = "apm/icons/profile/";

/// Asset path of a profile icon.
pub fn asset(id: &str) -> gpui::SharedString {
    format!("{PREFIX}{id}.svg").into()
}

/// The SVG for an asset path, when it is one of ours.
pub fn load(path: &str) -> Option<&'static [u8]> {
    let id = path.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
    PROFILE_ICONS.iter().find(|(i, _, _)| *i == id).map(|(_, _, b)| *b)
}

/// Every asset path under `prefix`.
pub fn list(prefix: &str) -> impl Iterator<Item = gpui::SharedString> + '_ {
    PROFILE_ICONS.iter().map(|(id, _, _)| asset(id)).filter(move |p| p.starts_with(prefix))
}

/// The icon a profile shows: its own when known, else the default.
pub fn effective_icon(name: Option<&str>) -> &'static str {
    name.and_then(|n| PROFILE_ICONS.iter().find(|(i, _, _)| *i == n)).map(|(i, _, _)| *i).unwrap_or(DEFAULT_ICON)
}

pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

/// The colour a profile shows: its own when valid, else the default (lower case).
pub fn effective_color(color: Option<&str>) -> String {
    color.filter(|c| valid_color(c)).unwrap_or(DEFAULT_COLOR).to_lowercase()
}

pub fn rgba(color: &str) -> gpui::Rgba {
    u32::from_str_radix(color.trim_start_matches('#'), 16).map(gpui::rgb).unwrap_or(gpui::rgb(0x94a3b8))
}

/// Icons whose id or search words contain the query (1.x picker filter).
pub fn search(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_lowercase();
    PROFILE_ICONS.iter().filter(|(id, words, _)| q.is_empty() || id.contains(q.as_str()) || words.contains(q.as_str())).map(|(id, _, _)| *id).collect()
}

/// Default avatar hues, for profiles with no icon of their own: a tile tinted with the hue and the
/// profile's initials in a shade of it. Each entry is (base, text on light themes, text on dark
/// themes); the base is also the profile's colour elsewhere (the card's top edge). Green, amber and
/// red are left out: they mean credential state.
pub const AVATAR_HUES: [(u32, u32, u32); 9] = [
    (0x6366f1, 0x3730a3, 0xc7d2fe), // indigo
    (0x3b82f6, 0x1e40af, 0xbfdbfe), // blue
    (0x0ea5e9, 0x075985, 0xbae6fd), // sky
    (0x06b6d4, 0x155e75, 0xa5f3fc), // cyan
    (0x14b8a6, 0x115e59, 0x99f6e4), // teal
    (0x8b5cf6, 0x5b21b6, 0xddd6fe), // violet
    (0xd946ef, 0x86198f, 0xf5d0fe), // fuchsia
    (0xec4899, 0x9d174d, 0xfbcfe8), // pink
    (0x64748b, 0x334155, 0xe2e8f0), // slate
];

/// How strongly the hue tints the avatar tile, over the surface it sits on.
pub fn avatar_tint_alpha(dark: bool) -> f32 {
    if dark {
        0.24
    } else {
        0.16
    }
}

/// A stable hue for a profile: FNV-1a of its account id (else its name), so the same account
/// keeps its colour across machines and renames.
pub fn avatar_hue(seed: &str) -> (u32, u32, u32) {
    let mut h: u32 = 0x811c9dc5;
    for b in seed.trim().to_lowercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    AVATAR_HUES[(h as usize) % AVATAR_HUES.len()]
}

/// One or two letters for the avatar: the first letters of the first two words ("Prod EU" is
/// "PE", "data-lake-dev" is "DL"), or the first letter of a one-word name ("Sandbox" is "S").
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.')).filter(|w| !w.is_empty()).collect();
    let first = |w: &str| w.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    match words.as_slice() {
        [] => "?".into(),
        [one] => first(one),
        [a, b, ..] => format!("{}{}", first(a), first(b)),
    }
}

/// The account a profile belongs to: its Identity Center account id, else the one in its role ARN.
pub fn account_of(prof: &serde_json::Value) -> Option<String> {
    let s = |k: &str| prof.get(k).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty());
    s("ssoAccountId").map(str::to_string).or_else(|| s("roleArn").and_then(|a| a.split(':').nth(4)).filter(|a| !a.is_empty()).map(str::to_string))
}

/// What a profile's avatar shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Avatar {
    /// A picked icon in a colour (#rrggbb).
    Icon { icon: &'static str, color: String },
    /// No icon picked: initials on a tile tinted with `base`, text `light` / `dark` by theme.
    Initials { text: String, base: u32, light: u32, dark: u32 },
}

impl Avatar {
    /// A picked icon wins; else initials, in the picked colour when there is one, otherwise in a
    /// hue from the account (or the name).
    pub fn of(icon_name: Option<&str>, icon_color: Option<&str>, name: &str, account: Option<&str>) -> Self {
        let known = icon_name.and_then(|n| PROFILE_ICONS.iter().find(|(i, _, _)| *i == n)).map(|(i, _, _)| *i);
        if let Some(icon) = known {
            return Avatar::Icon { icon, color: effective_color(icon_color) };
        }
        let text = initials(name);
        match icon_color.filter(|c| valid_color(c)) {
            Some(c) => {
                let base = u32::from_str_radix(&c[1..], 16).unwrap_or(0x94a3b8);
                Avatar::Initials { text, base, light: shade(base, 0.6, false), dark: shade(base, 0.72, true) }
            }
            None => {
                let (base, light, dark) = avatar_hue(account.filter(|a| !a.is_empty()).unwrap_or(name));
                Avatar::Initials { text, base, light, dark }
            }
        }
    }

    /// The profile's colour: the picked colour, else the avatar hue.
    pub fn color(&self) -> gpui::Rgba {
        match self {
            Avatar::Icon { color, .. } => rgba(color),
            Avatar::Initials { base, .. } => gpui::rgb(*base),
        }
    }
}

/// A picked colour darkened (towards black) or lightened (towards white) by `amount`, so initials
/// in it keep their contrast on the tinted tile.
fn shade(c: u32, amount: f32, lighten: bool) -> u32 {
    let ch = |shift: u32| {
        let v = ((c >> shift) & 0xff) as f32;
        let out = if lighten { v + (255. - v) * amount } else { v * (1. - amount) };
        (out.round() as u32).min(255)
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_defaults() {
        assert_eq!(search("rocket"), vec!["rocket"]);
        assert!(search("").len() == PROFILE_ICONS.len());
        assert_eq!(effective_icon(Some("nope")), "user");
        assert_eq!(effective_color(Some("#EF4444")), "#ef4444");
        assert_eq!(effective_color(Some("red")), DEFAULT_COLOR);
    }

    #[test]
    fn initials_and_hues() {
        assert_eq!(initials("Sandbox"), "S");
        assert_eq!(initials("Prod EU"), "PE");
        assert_eq!(initials("data-lake-dev"), "DL");
        assert_eq!(initials(""), "?");
        assert_eq!(avatar_hue("123456789012"), avatar_hue("123456789012"));
        assert!(matches!(Avatar::of(Some("rocket"), None, "X", None), Avatar::Icon { icon: "rocket", .. }));
        assert!(matches!(Avatar::of(None, None, "Dev", Some("1")), Avatar::Initials { .. }));
    }

    fn lum(c: u32) -> f32 {
        let f = |v: u32| {
            let v = v as f32 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f((c >> 16) & 0xff) + 0.7152 * f((c >> 8) & 0xff) + 0.0722 * f(c & 0xff)
    }

    fn over(fg: u32, a: f32, bg: u32) -> u32 {
        let ch = |s: u32| (((fg >> s) & 0xff) as f32 * a + ((bg >> s) & 0xff) as f32 * (1. - a)).round() as u32;
        (ch(16) << 16) | (ch(8) << 8) | ch(0)
    }

    fn contrast(a: u32, b: u32) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Initials keep 4.5:1 or better on their tinted tile, on every surface they sit on, in all
    /// four style / theme combinations.
    #[test]
    fn avatar_contrast() {
        use crate::theme::{Mode, Palette, Style};
        let hex = |c: gpui::Rgba| ((c.r * 255.).round() as u32) << 16 | ((c.g * 255.).round() as u32) << 8 | (c.b * 255.).round() as u32;
        for style in [Style::Modern, Style::Classic] {
            for mode in [Mode::Dark, Mode::Light] {
                let p = Palette::new(style, mode);
                let dark = mode == Mode::Dark;
                for surface in [p.bg, p.panel, p.hover, p.selected] {
                    let picked = PRESET_COLORS.iter().map(|c| match Avatar::of(None, Some(c), "X", None) {
                        Avatar::Initials { base, light, dark, .. } => (base, light, dark),
                        Avatar::Icon { .. } => unreachable!(),
                    });
                    for (base, light, darkc) in AVATAR_HUES.into_iter().chain(picked) {
                        let tile = over(base, avatar_tint_alpha(dark), hex(surface));
                        let r = contrast(if dark { darkc } else { light }, tile);
                        assert!(r >= 4.5, "{style:?} {mode:?} base {base:06x} on {:06x}: {r:.2}", hex(surface));
                    }
                }
            }
        }
    }
}
