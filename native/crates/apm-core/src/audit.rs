//! Auth audit log and the refresh bookkeeping shared by SAML, Identity Center and the scheduler.
//! Ports of `authAuditLog.ts`, `networkStatus.ts` and `refreshFailureCounters.ts`.

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

use serde_json::{Map, Value};

use crate::core::Core;
use crate::env::to_iso;
use crate::json;

const LOG_FILE: &str = "auth-audit-log.json";
const RETENTION_MS: i64 = 5 * 24 * 60 * 60 * 1000;
/// After a network failure, scheduled refreshes skip for this long.
const NETWORK_SKIP_MS: i64 = 30_000;
/// Consecutive non-network failures that pause auto refresh.
const FAILURE_THRESHOLD: u32 = 2;

/// In-memory state that 1.x kept in module globals; one per running core.
#[derive(Debug, Default)]
pub struct RefreshBookkeeping {
    last_network_failure_ms: AtomicI64,
    consecutive_failures: AtomicU32,
}

/// One audit entry. `t` is added on append. Build with [`AuditEntry::new`] and the setters so the
/// key order matches 1.x (`{...entry, t}`).
#[derive(Debug, Clone, Default)]
pub struct AuditEntry(pub Map<String, Value>);

impl AuditEntry {
    /// `kind`: "idp_request" | "idp_success" | "failure".
    pub fn new(kind: &str, source: &str) -> Self {
        let mut m = Map::new();
        m.insert("type".into(), kind.into());
        m.insert("source".into(), source.into());
        AuditEntry(m)
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }
}

pub fn idp_host_from_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => u.host_str().filter(|h| !h.is_empty()).map(str::to_string).unwrap_or_else(|| url.to_string()),
        Err(_) => url.chars().take(80).collect(),
    }
}

/// "a***" — enough to tell accounts apart in a log without recording them.
pub fn mask_username(username: &str) -> String {
    let u = crate::js::trim(username);
    if u.is_empty() {
        return "(empty)".into();
    }
    // JavaScript length counts UTF-16 units: a single astral character still masks to "*".
    if u.encode_utf16().count() <= 1 {
        return "*".into();
    }
    let first = u.encode_utf16().next().unwrap();
    format!("{}***", String::from_utf16_lossy(&[first]))
}

impl Core {
    fn audit_path(&self) -> std::path::PathBuf {
        self.cfg.paths.app_file(LOG_FILE)
    }

    fn read_audit(&self) -> Vec<Value> {
        match json::read_file(&self.audit_path()) {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        }
    }

    fn prune_audit(&self, entries: Vec<Value>) -> Vec<Value> {
        let cutoff = self.cfg.clock.now_ms() - RETENTION_MS;
        entries.into_iter().filter(|e| e.get("t").and_then(Value::as_f64).is_some_and(|t| t >= cutoff as f64)).collect()
    }

    fn write_audit(&self, entries: Vec<Value>) {
        let _ = std::fs::create_dir_all(&self.cfg.paths.app_data);
        // JSON.stringify(entries, null, 0): compact.
        let _ = std::fs::write(self.audit_path(), json::stringify(&Value::Array(entries)));
    }

    pub fn auth_logging_enabled(&self) -> bool {
        self.settings().get().get("authLoggingEnabled") != Some(&Value::Bool(false))
    }

    pub fn append_auth_audit(&self, entry: AuditEntry) {
        if !self.auth_logging_enabled() {
            return;
        }
        let mut full = entry.0;
        full.insert("t".into(), self.cfg.clock.now_ms().into());
        let mut merged = self.prune_audit(self.read_audit());
        merged.push(Value::Object(full));
        let pruned = self.prune_audit(merged);
        self.write_audit(pruned);
        self.host.log_viewer_changed();
    }

    fn profile_display_names(&self) -> Vec<(String, String)> {
        self.profiles()
            .all()
            .iter()
            .filter_map(|p| {
                let s = |k: &str| crate::js::trim(p.get(k).and_then(Value::as_str).unwrap_or("")).to_string();
                let name = [s("name"), s("label"), s("credentialProfileName")].into_iter().find(|v| !v.is_empty())?;
                Some((p.get("id")?.as_str()?.to_string(), name))
            })
            .collect()
    }

    /// Newest first, with `profileName` added where the profile still exists.
    pub fn auth_audit_entries_for_viewer(&self) -> Vec<Value> {
        let names = self.profile_display_names();
        let mut entries = self.prune_audit(self.read_audit());
        entries.sort_by(|a, b| {
            let t = |v: &Value| v.get("t").and_then(Value::as_f64).unwrap_or(0.0);
            t(b).partial_cmp(&t(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
            .into_iter()
            .map(|mut e| {
                let pid = e.get("profileId").and_then(Value::as_str).map(str::to_string);
                if let (Some(pid), Some(obj)) = (pid, e.as_object_mut()) {
                    if let Some((_, n)) = names.iter().find(|(id, _)| *id == pid) {
                        obj.insert("profileName".into(), n.clone().into());
                    }
                }
                e
            })
            .collect()
    }

    pub fn clear_auth_audit(&self) {
        self.write_audit(Vec::new());
        self.host.log_viewer_changed();
    }

    /// Plain-text rendering (oldest first) for support requests.
    pub fn format_auth_logs(&self) -> String {
        let names = self.profile_display_names();
        let mut entries = self.prune_audit(self.read_audit());
        entries.sort_by(|a, b| {
            let t = |v: &Value| v.get("t").and_then(Value::as_f64).unwrap_or(0.0);
            t(a).partial_cmp(&t(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        if entries.is_empty() {
            return "No auth audit entries yet.\n\n(IdP requests and failures are recorded when logging is enabled.)".into();
        }
        let s = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
        let num = |e: &Value, k: &str| e.get(k).filter(|v| !v.is_null()).map(|v| v.to_string()).unwrap_or_else(|| "—".into());
        entries
            .iter()
            .map(|e| {
                let ts = to_iso(e.get("t").and_then(Value::as_i64).unwrap_or(0));
                let pref = match s(e, "profileId") {
                    None => "—".to_string(),
                    Some(id) => match names.iter().find(|(pid, _)| *pid == id) {
                        Some((_, n)) => format!("{n} ({id})"),
                        None => id,
                    },
                };
                let source = s(e, "source").unwrap_or_default();
                let host = s(e, "idpHost").unwrap_or_else(|| "—".into());
                match s(e, "type").as_deref() {
                    Some("idp_request") => format!(
                        "[{ts}] IdP sign-in request  source={source}  profile={pref}  host={host}  user={}",
                        s(e, "usernameHint").unwrap_or_else(|| "—".into())
                    ),
                    Some("idp_success") => format!(
                        "[{ts}] IdP sign-in success  source={source}  profile={pref}  host={host}  roles={}  {}ms",
                        num(e, "roleCount"),
                        num(e, "durationMs")
                    ),
                    _ => format!("[{ts}] FAILURE  source={source}  profile={pref}  error={}", s(e, "error").unwrap_or_else(|| "—".into())),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---- refresh bookkeeping ---------------------------------------------------------------

    pub fn note_network_failure(&self) {
        self.refresh_state.last_network_failure_ms.store(self.cfg.clock.now_ms(), Ordering::Relaxed);
    }

    pub fn clear_network_failure(&self) {
        self.refresh_state.last_network_failure_ms.store(0, Ordering::Relaxed);
    }

    pub fn should_skip_due_to_recent_network_failure(&self) -> bool {
        let last = self.refresh_state.last_network_failure_ms.load(Ordering::Relaxed);
        last != 0 && self.cfg.clock.now_ms() - last < NETWORK_SKIP_MS
    }

    /// Count a failure; true exactly when the count reaches the pause threshold.
    pub fn record_consecutive_refresh_failure(&self) -> bool {
        self.refresh_state.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1 == FAILURE_THRESHOLD
    }

    pub fn reset_consecutive_refresh_failures(&self) {
        self.refresh_state.consecutive_failures.store(0, Ordering::Relaxed);
    }
}
