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
    "#64748b", "#3b82f6", "#0ea5e9", "#06b6d4", "#10b981", "#22c55e", "#84cc16", "#eab308", "#f59e0b", "#ef4444",
    "#f43f5e", "#ec4899", "#a855f7", "#6366f1", "#0f172a", "#475569", "#94a3b8", "#c2410c", "#15803d", "#1e40af",
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
}
