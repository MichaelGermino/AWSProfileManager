//! Where the app keeps its files, the clock, and the platform it behaves as. Port of
//! `appPaths.ts` plus the test seams of `testHooks.ts` (docs/native-rewrite-plan.md §5.3).

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};

pub const APP_NAME: &str = "AWSProfileManager";
pub const KEYCHAIN_SERVICE: &str = "AWSProfileManager";

/// The OS a behaviour depends on, spelled like Node's `process.platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }

    pub fn node_name(self) -> &'static str {
        match self {
            Platform::Windows => "win32",
            Platform::MacOs => "darwin",
            Platform::Linux => "linux",
        }
    }

    /// `os.EOL`: the line ending for files the app creates.
    pub fn eol(self) -> &'static str {
        if self == Platform::Windows {
            "\r\n"
        } else {
            "\n"
        }
    }
}

/// Everything location-dependent. Production values come from [`Paths::default_for_user`]; tests
/// point them into a sandbox (APM_APPDATA_DIR and friends in 1.x).
#[derive(Debug, Clone)]
pub struct Paths {
    /// `%APPDATA%\AWSProfileManager` / `~/Library/Application Support/AWSProfileManager`.
    pub app_data: PathBuf,
    /// `~/.aws/credentials`.
    pub credentials_file: PathBuf,
}

impl Paths {
    pub fn default_for_user() -> Self {
        let home = home_dir();
        let app_data = match Platform::current() {
            Platform::Windows => std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData").join("Roaming")).join(APP_NAME),
            Platform::MacOs => home.join("Library").join("Application Support").join(APP_NAME),
            Platform::Linux => std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config")).join(APP_NAME),
        };
        Paths { app_data, credentials_file: home.join(".aws").join("credentials") }
    }

    pub fn app_file(&self, name: &str) -> PathBuf {
        self.app_data.join(name)
    }
}

/// `USERPROFILE` on Windows, else `HOME`.
pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// Wall clock with a test offset (the scenarios' `advanceClock`; 1.x patched `Date.now`).
#[derive(Debug, Clone, Default)]
pub struct Clock {
    offset_ms: Arc<AtomicI64>,
}

impl Clock {
    pub fn now_ms(&self) -> i64 {
        Utc::now().timestamp_millis() + self.offset_ms.load(Ordering::Relaxed)
    }

    pub fn now(&self) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(self.now_ms()).single().unwrap_or_else(Utc::now)
    }

    pub fn advance(&self, ms: i64) {
        self.offset_ms.fetch_add(ms, Ordering::Relaxed);
    }
}

/// `new Date(s).getTime()` for the timestamps the app stores (ISO 8601). `None` = Invalid Date.
pub fn parse_js_date_ms(s: &str) -> Option<i64> {
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp_millis());
    }
    // Date-only ISO strings are UTC midnight in JavaScript.
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    // Date-time without a zone is local time in JavaScript.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(n) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return chrono::Local.from_local_datetime(&n).single().map(|d| d.timestamp_millis());
        }
    }
    None
}

/// `new Date(ms).toISOString()`.
pub fn to_iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms).single().map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_default()
}
