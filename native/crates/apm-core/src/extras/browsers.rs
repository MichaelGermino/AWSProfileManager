//! Opening a URL in a specific installed browser. Port of `externalBrowser.ts`.
//!
//! Private/incognito windows are deliberately NOT offered. The AWS console keeps one session per
//! browser session context, and multi-session opt-in lives in a cookie a private window discards —
//! so a private window can only ever hold one AWS account, which defeats the point.
//!
//! ⚠️ Always spawn the browser executable DIRECTLY with an args array (see
//! [`crate::host::Host::launch_browser`]). Going through `cmd /c start` makes cmd treat '&' as a
//! command separator, silently truncating a URL at its first query parameter. A direct spawn also
//! avoids ShellExecute's URL length limit.

use std::path::{Path, PathBuf};

use crate::env::{home_dir, Platform};
use crate::host::Host;

struct BrowserDef {
    key: &'static str,
    name: &'static str,
    /// Windows: candidate .exe paths. macOS: candidate .app bundle paths (launched via `open -a`).
    paths: Vec<PathBuf>,
}

/// A browser found on this machine, for the Settings picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledBrowser {
    pub key: String,
    pub name: String,
    /// The executable (Windows) or .app bundle (macOS) that was found.
    pub exe: PathBuf,
}

/// macOS bundles. Keys match the Windows list so a settings.consoleBrowser value means the same
/// browser on both. ~/Applications is where a non-admin install lands.
fn mac_app(name: &str) -> Vec<PathBuf> {
    vec![Path::new("/Applications").join(name), home_dir().join("Applications").join(name)]
}

fn definitions(platform: Platform) -> Vec<BrowserDef> {
    match platform {
        Platform::Windows => {
            let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
            vec![
                BrowserDef {
                    key: "edge",
                    name: "Microsoft Edge",
                    paths: vec![
                        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into(),
                        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe".into(),
                    ],
                },
                BrowserDef {
                    key: "chrome",
                    name: "Google Chrome",
                    paths: vec![
                        r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".into(),
                        format!(r"{local}\Google\Chrome\Application\chrome.exe").into(),
                    ],
                },
                BrowserDef {
                    key: "firefox",
                    name: "Mozilla Firefox",
                    paths: vec![r"C:\Program Files\Mozilla Firefox\firefox.exe".into(), r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe".into()],
                },
            ]
        }
        Platform::MacOs => vec![
            BrowserDef { key: "chrome", name: "Google Chrome", paths: mac_app("Google Chrome.app") },
            BrowserDef { key: "edge", name: "Microsoft Edge", paths: mac_app("Microsoft Edge.app") },
            BrowserDef { key: "firefox", name: "Mozilla Firefox", paths: mac_app("Firefox.app") },
            BrowserDef { key: "safari", name: "Safari", paths: vec!["/Applications/Safari.app".into()] },
        ],
        Platform::Linux => Vec::new(),
    }
}

fn resolve(b: &BrowserDef) -> Option<InstalledBrowser> {
    let exe = b.paths.iter().find(|p| !p.as_os_str().is_empty() && p.exists())?;
    Some(InstalledBrowser { key: b.key.into(), name: b.name.into(), exe: exe.clone() })
}

/// Browsers actually installed, in preference order. Empty on Linux.
pub fn list_installed(platform: Platform) -> Vec<InstalledBrowser> {
    definitions(platform).iter().filter_map(resolve).collect()
}

/// The requested browser if installed, else the first one that is.
fn find_browser(platform: Platform, preferred_key: &str) -> Option<InstalledBrowser> {
    let defs = definitions(platform);
    if !preferred_key.is_empty() && preferred_key != "default" {
        if let Some(hit) = defs.iter().find(|b| b.key == preferred_key).and_then(resolve) {
            return Some(hit);
        }
        // Chosen browser was uninstalled since; fall through rather than failing outright.
    }
    defs.iter().find_map(resolve)
}

/// `openInBrowser`: open `url` in the chosen browser (`settings.consoleBrowser`).
///
/// Spawning the executable directly also sidesteps a length limit: the OS "open URL" call goes
/// through ShellExecute, which truncates long URLs, and the multi-session console URL runs to
/// ~3300 characters. A direct spawn passes it as an argv entry (32 KB limit), so it survives.
///
/// 'default' (or no key) means "whatever the OS is set to"; so does having no known browser
/// installed, so the action still does something rather than silently failing.
pub fn open_in_browser(host: &dyn Host, platform: Platform, url: &str, browser_key: Option<&str>) -> Result<String, String> {
    let browser = match browser_key {
        Some(k) if k != "default" => find_browser(platform, k),
        _ => None,
    };
    let Some(browser) = browser else {
        host.open_external("url", url);
        return Ok("default browser".into());
    };
    host.launch_browser(&browser.exe.to_string_lossy(), url).map(|()| browser.name)
}
