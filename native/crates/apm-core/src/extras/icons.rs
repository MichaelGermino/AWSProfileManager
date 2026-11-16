//! The selectable app icon (Settings → Appearance → App icon). Port of the selection and preview
//! parts of `appIcon.ts`; applying an icon to windows, the tray and shortcuts is the host's job
//! ([`crate::host::Host::app_icon_changed`]), using the paths below.
//!
//! "Classic" is the original resources/icon.* set and stays the default, so an update never
//! changes an existing user's icon. Every other design is a folder of pre-rendered files under
//! resources/app-icons/<key>/, produced by `npm run icons` from scripts/app-icons/designs.browser.js,
//! and listed in resources/app-icons/manifest.json.
//!
//! What a runtime switch can reach: window and taskbar-button icons, the macOS Dock while running,
//! the tray, and (Windows) the Start menu, desktop and pinned shortcuts. The installer, the .exe
//! and the Finder icon are fixed at build time and keep Classic.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};

use crate::core::Core;
use crate::env::Platform;

/// Settings.appIcon value for the original icon set in resources/icon.*.
pub const CLASSIC_APP_ICON: &str = "classic";

/// One entry of manifest.json.
#[derive(Debug, Clone)]
pub struct Design {
    pub key: String,
    pub name: String,
    pub style: String,
}

fn existing(p: PathBuf) -> Option<PathBuf> {
    p.exists().then_some(p)
}

/// Icon files for a resources directory.
pub struct AppIcons<'a> {
    pub resources: &'a Path,
}

impl AppIcons<'_> {
    fn designs_dir(&self) -> PathBuf {
        self.resources.join("app-icons")
    }

    /// The designs in manifest order; empty when the manifest is missing or unreadable.
    pub fn designs(&self) -> Vec<Design> {
        let Some(Value::Array(items)) = crate::json::read_file(&self.designs_dir().join("manifest.json")) else {
            return Vec::new();
        };
        let field = |d: &Value, k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        items.iter().map(|d| Design { key: field(d, "key"), name: field(d, "name"), style: field(d, "style") }).collect()
    }

    /// An unknown key (a design removed in a later release, a hand-edited settings.json) falls
    /// back to Classic.
    pub fn resolve_key(&self, key: Option<&str>) -> String {
        match key {
            Some(k) if !k.is_empty() && self.designs().iter().any(|d| d.key == k) => k.to_string(),
            _ => CLASSIC_APP_ICON.to_string(),
        }
    }

    pub fn png_path(&self, key: &str) -> PathBuf {
        if key == CLASSIC_APP_ICON {
            self.resources.join("icon.png")
        } else {
            self.designs_dir().join(key).join("ui.png")
        }
    }

    /// Icon for windows. Windows uses it for the title bar and taskbar button; other platforms
    /// take the window icon from the app bundle, so this is Windows-only.
    pub fn window_icon_path(&self, platform: Platform, key: &str) -> Option<PathBuf> {
        if platform != Platform::Windows {
            return None;
        }
        if key == CLASSIC_APP_ICON {
            existing(self.resources.join("icon.ico"))
        } else {
            existing(self.designs_dir().join(key).join("icon.ico"))
        }
    }

    /// A design's Windows .ico, or `None` for Classic (whose icon is the .exe's own) and missing files.
    pub fn design_ico_path(&self, key: &str) -> Option<PathBuf> {
        if key == CLASSIC_APP_ICON {
            None
        } else {
            existing(self.designs_dir().join(key).join("icon.ico"))
        }
    }

    /// macOS Dock image. Classic uses the padded square the bundle icon is built from.
    pub fn dock_icon_path(&self, key: &str) -> Option<PathBuf> {
        if key == CLASSIC_APP_ICON {
            existing(self.resources.join("icon-mac.png"))
        } else {
            existing(self.designs_dir().join(key).join("mac.png"))
        }
    }
}

/// A PNG as a data URL, or `None` when the file is missing or empty.
///
/// 1.x went through `nativeImage.toDataURL()`, which re-encodes the PNG; this embeds the file's
/// own bytes. Both are a valid PNG of the same image.
pub fn png_data_url(file: &Path) -> Option<String> {
    let bytes = std::fs::read(file).ok().filter(|b| !b.is_empty())?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

impl Core {
    pub fn app_icons(&self) -> AppIcons<'_> {
        AppIcons { resources: &self.cfg.resources_dir }
    }

    /// The selected design key (`currentKey`).
    pub fn current_app_icon_key(&self) -> String {
        let settings = self.settings().get();
        self.app_icons().resolve_key(settings.get("appIcon").and_then(Value::as_str))
    }

    /// `getAppIconDataUrl`: the selected icon as a data URL, for the window's title bar.
    pub fn app_icon_data_url(&self) -> Option<String> {
        png_data_url(&self.app_icons().png_path(&self.current_app_icon_key()))
    }

    /// `listAppIcons`: every choice, Classic first, with a preview image for the picker.
    pub fn list_app_icons(&self) -> Value {
        let icons = self.app_icons();
        let classic = Design { key: CLASSIC_APP_ICON.into(), name: "Classic".into(), style: "Original".into() };
        let list: Vec<Value> = std::iter::once(classic)
            .chain(icons.designs())
            .map(|d| {
                let data_url = png_data_url(&icons.png_path(&d.key));
                json!({ "key": d.key, "name": d.name, "style": d.style, "dataUrl": data_url })
            })
            .collect();
        Value::Array(list)
    }
}
