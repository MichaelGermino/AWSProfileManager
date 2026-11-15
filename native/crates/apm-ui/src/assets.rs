//! Bundled assets: the app's icons (Lucide, ISC licence; profile icons Heroicons, MIT) and fonts (Geist, SIL OFL 1.1). Anything
//! not ours falls through to gpui-component's own icon set.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $( (concat!("apm/icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg"))), )*
        ];
    };
}

icons!(
    "users", "terminal", "sliders-horizontal", "search", "plus", "folder", "folder-plus", "folder-open",
    "refresh-cw", "ellipsis", "chevron-down", "chevron-right", "x", "copy", "external-link", "log-in",
    "app-window", "minus", "square", "settings", "key-round", "lock", "moon", "sun", "monitor",
    "grip-vertical", "pencil", "trash", "info", "triangle-alert", "check", "panel-left", "panel-right",
    "circle-check", "circle-x", "download", "upload", "file-text", "film", "sparkles", "building-2", "cloud",
    "id-card", "message-square", "layout-grid", "list", "panel-left-close", "panel-left-open",
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = crate::profile_icons::load(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = ICONS.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| (*p).into()).collect();
        out.extend(crate::profile_icons::list(path));
        out.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(out)
    }
}

/// Geist and Geist Mono (regular, medium, semibold), registered at start-up.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
    ]
}

/// Asset path of one of our icons.
pub fn icon(name: &str) -> SharedString {
    format!("apm/icons/{name}.svg").into()
}
