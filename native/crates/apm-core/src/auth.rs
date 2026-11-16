//! SAML sign-in, STS, role fetching and profile refresh. Port of `awsAuthService.ts`, plus the
//! pause state of `refreshScheduler.ts` (`getRefreshPauseState` / `setRefreshPaused`), which the
//! two-strike auto-pause here drives. The scheduler loop itself lives elsewhere and calls the
//! public refresh methods below.
//!
//! Every refresh path funnels through [`Core::refresh_profile_with`], which is also the single
//! "app is locked" chokepoint.

mod idp;
mod sts;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::core::{Core, DispatchResult};
use crate::env::{parse_js_date_ms, to_iso};
use crate::master::{IdpCredentials, DEFAULT_CREDENTIALS_ID};
use crate::network_classifier::{classify, ErrorInfo, FailureKind};
use crate::saml_roles::AwsRole;
use crate::sso_org::{is_identity_center, org_of_profile};
use crate::store::Obj;

pub use idp::NO_ASSERTION;
pub use sts::REGION;

/// Refresh when credentials expire within this many minutes (the scheduler uses the same check).
const REFRESH_THRESHOLD_MINUTES: i64 = 15;

/// A failure on the auth path: the message 1.x showed (`err.message`, or `describeAuthError` for
/// STS) and, when there is one, what the network classifier needs to tell offline from a bad
/// password (1.x passed the original error object as `errorObject`).
#[derive(Debug, Clone)]
pub struct AuthError {
    pub message: String,
    pub info: Option<ErrorInfo>,
}

/// A transport failure (DNS, refused, timeout, TLS) described the way Node/axios did, so the
/// message reads like 1.x's and the classifier sees the same codes.
pub(crate) fn transport_error(e: &reqwest::Error, url: &str) -> AuthError {
    let mut info = crate::http::error_info(e);
    let parsed = reqwest::Url::parse(url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let port = parsed.as_ref().and_then(|u| u.port_or_known_default()).map(|p| p.to_string()).unwrap_or_default();

    let mut chain = Vec::new();
    let mut io_kind = None;
    let mut source: Option<&dyn std::error::Error> = Some(e);
    while let Some(s) = source {
        chain.push(s.to_string());
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            io_kind.get_or_insert(io.kind());
        }
        source = s.source();
    }
    let joined = chain.join(": ").to_lowercase();
    let message = if e.is_timeout() {
        info.code = Some("ETIMEDOUT".into());
        format!("connect ETIMEDOUT {host}:{port}")
    } else if joined.contains("dns error") || joined.contains("failed to lookup address") || joined.contains("no such host") {
        info.code = Some("ENOTFOUND".into());
        format!("getaddrinfo ENOTFOUND {host}")
    } else if io_kind == Some(std::io::ErrorKind::ConnectionRefused) || joined.contains("refused") {
        info.code = Some("ECONNREFUSED".into());
        format!("connect ECONNREFUSED {host}:{port}")
    } else if io_kind == Some(std::io::ErrorKind::ConnectionReset) {
        info.code = Some("ECONNRESET".into());
        "socket hang up".to_string()
    } else {
        chain.join(": ")
    };
    info.message = Some(message.clone());
    AuthError { message, info: Some(info) }
}

/// The assertion and roles of a sign-in that matched several roles, kept until the user picks
/// one (`auth:selectRole`).
#[derive(Debug, Clone)]
struct PendingAuth {
    assertion: String,
    roles: Vec<AwsRole>,
}

/// In-memory auth state that 1.x kept in module globals; one per running core (a restart forgets
/// it, as quitting 1.x did).
#[derive(Debug, Default)]
pub struct AuthState {
    pending_auth: Mutex<HashMap<String, PendingAuth>>,
    /// For the scheduler (1.x `refreshScheduler.ts`): SAML profiles it has asked the user to enter
    /// credentials for. While a profile is in here, an expired credential does not trigger a new
    /// prompt on every tick — only its refresh interval does. Keyed to the expiration it was set
    /// for, so a refresh the user does themselves (which changes the expiration) clears it.
    pub awaiting_credentials: Mutex<HashMap<String, Option<String>>>,
    /// For the scheduler: last time (epoch ms) a scheduled refresh ran per profile. The interval is
    /// "time since last scheduled refresh", not time until credential expiry.
    pub last_scheduled_refresh_at: Mutex<HashMap<String, i64>>,
}

/// 1.x `RefreshResult`.
#[derive(Debug, Clone, PartialEq)]
pub enum RefreshResult {
    Success,
    Failed(String),
    /// The window must ask for the IdP password (`auth:credentialsRequired` was sent).
    /// `prefill_username: None` = the key is absent (the profile does not use default credentials).
    Required {
        profile_id: String,
        prefill_username: Option<String>,
    },
    /// Several roles and none remembered: the user picks one, then `auth:selectRole`.
    Roles {
        roles: Vec<AwsRole>,
        profile_id: String,
    },
    /// An Identity Center session needs a human sign-in. Not a failure.
    SsoLoginRequired {
        profile_id: String,
        start_url: String,
        region: String,
    },
}

impl RefreshResult {
    pub fn to_value(&self) -> Value {
        match self {
            RefreshResult::Success => json!({ "success": true }),
            RefreshResult::Failed(e) => json!({ "success": false, "error": e }),
            RefreshResult::Required { profile_id, prefill_username } => {
                let mut v = json!({ "required": true, "profileId": profile_id });
                if let Some(u) = prefill_username {
                    v["prefillUsername"] = u.clone().into();
                }
                v
            }
            RefreshResult::Roles { roles, profile_id } => json!({ "roles": roles, "profileId": profile_id }),
            RefreshResult::SsoLoginRequired { profile_id, start_url, region } => {
                json!({ "ssoLoginRequired": true, "profileId": profile_id, "startUrl": start_url, "region": region })
            }
        }
    }

    /// `'required' in result` (the scheduler's awaiting-credentials bookkeeping).
    pub fn is_required(&self) -> bool {
        matches!(self, RefreshResult::Required { .. })
    }

    /// `'ssoLoginRequired' in result` (the scheduler stops touching that org for the tick).
    pub fn is_sso_login_required(&self) -> bool {
        matches!(self, RefreshResult::SsoLoginRequired { .. })
    }
}

/// `{ paused, pausedDueToFailures }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPauseState {
    pub paused: bool,
    pub paused_due_to_failures: bool,
}

impl RefreshPauseState {
    pub fn to_value(self) -> Value {
        json!({ "paused": self.paused, "pausedDueToFailures": self.paused_due_to_failures })
    }
}

// ---- small JavaScript-semantics helpers for profile records ----------------------------------

fn s<'a>(p: &'a Obj, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}

fn truthy(v: Option<&Value>) -> bool {
    v.is_some_and(crate::js::truthy)
}

fn has_idp_url(p: &Obj) -> bool {
    !crate::js::trim(s(p, "idpEntryUrl").unwrap_or("")).is_empty()
}

fn is_ic(p: &Obj) -> bool {
    is_identity_center(s(p, "authType"))
}

fn id_of(p: &Obj) -> String {
    s(p, "id").unwrap_or("").to_string()
}

/// `${profile.name}` in a template string.
fn name_for_message(p: &Obj) -> String {
    match p.get("name") {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(v) => v.to_string(),
    }
}

/// `(profile.credentialProfileName || profile.name || 'default').trim() || 'default'`
fn section_name(p: &Obj) -> String {
    let pick = s(p, "credentialProfileName").filter(|v| !v.is_empty()).or_else(|| s(p, "name").filter(|v| !v.is_empty()));
    let t = crate::js::trim(pick.unwrap_or("default"));
    if t.is_empty() {
        "default".into()
    } else {
        t.to_string()
    }
}

/// `Math.min(43200, Math.max(3600, profile.refreshIntervalMinutes * 60))`
fn duration_seconds(p: &Obj) -> i64 {
    let minutes = p.get("refreshIntervalMinutes").and_then(Value::as_f64).unwrap_or(60.0);
    (minutes * 60.0).clamp(3600.0, 43200.0) as i64
}

/// `!expiration || new Date(expiration).getTime() <= now + 15 min` (an unparseable date is NaN,
/// which is never due).
pub(crate) fn should_refresh_by_expiration(expiration: Option<&Value>, now_ms: i64) -> bool {
    if !truthy(expiration) {
        return true;
    }
    let threshold = now_ms + REFRESH_THRESHOLD_MINUTES * 60 * 1000;
    let exp = match expiration {
        Some(Value::String(s)) => parse_js_date_ms(s),
        Some(Value::Number(n)) => n.as_f64().map(|f| f as i64),
        _ => None,
    };
    exp.is_some_and(|e| e <= threshold)
}

impl Core {
    // ---- events ------------------------------------------------------------------------------

    fn notify(&self, title: &str, body: &str) {
        self.host.emit("notify", vec![title.into(), body.into()]);
    }

    fn send_credentials_required(&self, profile_id: &str, prefill_username: Option<&str>) {
        self.host.show_main_window();
        self.host.emit("auth:credentialsRequired", vec![profile_id.into(), prefill_username.unwrap_or("").into()]);
    }

    fn send_credentials_refreshed(&self, profile_id: &str) {
        self.host.emit("auth:credentialsRefreshed", vec![profile_id.into()]);
    }

    fn send_refresh_started(&self, profile_id: &str) {
        self.host.emit("auth:refreshStarted", vec![profile_id.into()]);
    }

    fn send_credentials_expired(&self, profile_id: &str, message: &str) {
        self.host.show_main_window();
        self.host.emit("auth:credentialsExpired", vec![profile_id.into(), message.into()]);
    }

    fn send_network_unavailable(&self, profile_id: &str) {
        // No show/focus: offline failures shouldn't yank the window forward.
        self.host.emit("auth:networkUnavailable", vec![profile_id.into()]);
    }

    fn send_refresh_all_required(&self, credential_profile_ids: &[String], default_profile_ids: &[String]) {
        self.host.show_main_window();
        self.host.emit("auth:refreshAllRequired", vec![json!(credential_profile_ids), json!(default_profile_ids)]);
    }

    fn send_sso_login_required(&self, profile_id: &str, start_url: &str, region: &str) {
        // Deliberately no show/focus: the scheduler can hit this while the user is elsewhere, and
        // a window yanked forward mid-task is worse than a quiet badge.
        self.host.emit("auth:ssoLoginRequired", vec![profile_id.into(), start_url.into(), region.into()]);
    }

    /// Log a failure, count it toward the consecutive-failure auto-pause, and notify the window.
    ///
    /// Network failures (DNS, offline, refused, timeout) are classified separately: they do NOT
    /// count toward the lockout-prevention pause (no login attempt reached the IdP, so no lockout
    /// risk), are not written to the auth audit log as failures, and surface a non-alarming
    /// `auth:networkUnavailable` event instead of `auth:credentialsExpired`.
    fn note_auth_failure(&self, profile_id: &str, error: &str, info: Option<&ErrorInfo>, source: &str, notify_expired: bool) {
        let kind = match info {
            Some(i) => classify(Some(i)),
            None => crate::network_classifier::classify_message(error),
        };
        if kind == FailureKind::Network {
            self.note_network_failure();
            if notify_expired {
                self.send_network_unavailable(profile_id);
            }
            return;
        }
        // error.slice(0, 2000) counts UTF-16 units.
        let units: Vec<u16> = error.encode_utf16().take(2000).collect();
        let truncated = String::from_utf16_lossy(&units);
        self.append_auth_audit(crate::audit::AuditEntry::new("failure", source).with("profileId", profile_id).with("error", truncated));
        let pause = self.record_consecutive_refresh_failure();
        if notify_expired {
            self.send_credentials_expired(profile_id, error);
        }
        // 1.x paused through a dynamic import, so the pause events land after credentialsExpired.
        if pause {
            self.set_refresh_paused(true, true);
            self.host.emit("auth:autoRefreshPausedForFailures", vec![]);
        }
    }

    fn note_success(&self) {
        self.reset_consecutive_refresh_failures();
        self.clear_network_failure();
    }

    // ---- scheduler pause state (refreshScheduler.ts) -----------------------------------------

    /// 1.x `getRefreshPauseState`. Persisted in ui-prefs.json, so a pause survives restarts.
    pub fn refresh_pause_state(&self) -> RefreshPauseState {
        let prefs = self.ui_prefs();
        RefreshPauseState { paused: prefs.refresh_paused(), paused_due_to_failures: prefs.refresh_paused_due_to_failures() }
    }

    /// 1.x `getRefreshPaused` (the scheduler tick skips while paused).
    pub fn refresh_paused(&self) -> bool {
        self.refresh_pause_state().paused
    }

    /// 1.x `setRefreshPaused`. Resuming resets the consecutive-failure count.
    pub fn set_refresh_paused(&self, value: bool, due_to_failures: bool) {
        let prefs = self.ui_prefs();
        prefs.set_refresh_paused(value);
        let due = value && due_to_failures;
        if !value {
            self.reset_consecutive_refresh_failures();
        }
        prefs.set_refresh_paused_due_to_failures(due);
        self.host.emit("scheduler:pausedChanged", vec![RefreshPauseState { paused: value, paused_due_to_failures: due }.to_value()]);
    }

    /// The scheduler's state that lives with auth (awaiting credentials, last scheduled refresh).
    pub fn auth_state(&self) -> &AuthState {
        &self.auth_state
    }

    // ---- credentials -------------------------------------------------------------------------

    /// Splits profiles into those that need a prompt and those we have credentials for (stored
    /// default), so "Refresh all" can prompt once and refresh all, regardless of the
    /// useDefaultCredentials checkbox.
    fn split_profiles_by_credentials(&self, profiles: &[Obj]) -> (Vec<String>, Vec<String>) {
        let (mut need, mut have) = (Vec::new(), Vec::new());
        let mut default_stored: Option<Option<IdpCredentials>> = None;
        for p in profiles {
            if !truthy(p.get("useDefaultCredentials")) {
                need.push(id_of(p));
                continue;
            }
            let stored = default_stored.get_or_insert_with(|| self.stored_credentials(DEFAULT_CREDENTIALS_ID));
            if stored.as_ref().is_some_and(|c| !crate::js::trim(&c.password).is_empty()) {
                have.push(id_of(p));
            } else {
                need.push(id_of(p));
            }
        }
        (need, have)
    }

    // ---- role listing ------------------------------------------------------------------------

    /// 1.x `fetchRolesForIdp`: list roles to populate the role dropdown. Uses the default
    /// credentials when `use_default_credentials`, else asks the window for credentials.
    pub async fn fetch_roles_for_idp(&self, idp_entry_url: &str, use_default_credentials: bool, profile_id: Option<&str>) -> Value {
        if crate::js::trim(idp_entry_url).is_empty() {
            return json!({ "success": false, "error": "IdP entry URL is required." });
        }
        let required = |prefill: Option<String>| {
            let mut v = json!({ "credentialsRequired": true });
            if let Some(id) = profile_id {
                v["profileId"] = id.into();
            }
            if let Some(u) = prefill {
                v["prefillUsername"] = u.into();
            }
            v
        };
        if !use_default_credentials {
            return required(None);
        }
        let stored = self.stored_credentials(DEFAULT_CREDENTIALS_ID);
        let Some(creds) = stored.clone().filter(|c| !c.password.is_empty()) else {
            return required(Some(stored.map(|c| c.username).unwrap_or_default()));
        };
        self.list_roles(idp_entry_url, &creds.username, &creds.password, "fetchRolesForIdp", profile_id).await
    }

    /// 1.x `fetchRolesWithCredentials`: after the user was prompted for load-roles.
    pub async fn fetch_roles_with_credentials(&self, idp_entry_url: &str, username: &str, password: &str) -> Value {
        if crate::js::trim(idp_entry_url).is_empty() {
            return json!({ "success": false, "error": "IdP entry URL is required." });
        }
        self.list_roles(idp_entry_url, username, password, "fetchRolesWithCredentials", None).await
    }

    async fn list_roles(&self, idp_entry_url: &str, username: &str, password: &str, source: &str, profile_id: Option<&str>) -> Value {
        let meta = idp::LoginMeta { profile_id, source };
        match self.perform_login(idp_entry_url, username, password, Some(meta)).await {
            Ok(mut login) => {
                self.apply_account_names(&login.assertion, &mut login.roles).await;
                self.note_success();
                let roles = serde_json::to_value(&login.roles).unwrap_or(Value::Array(vec![]));
                self.roles_cache().set(idp_entry_url, roles.clone(), self.cfg.clock.now_ms());
                json!({ "roles": roles })
            }
            Err(e) => {
                self.note_auth_failure(profile_id.unwrap_or("fetch-roles"), &e.message, e.info.as_ref(), source, false);
                json!({ "success": false, "error": e.message })
            }
        }
    }

    // ---- refresh -----------------------------------------------------------------------------

    /// Refresh one profile with its saved credentials. `interactive`: only a direct user action
    /// (the Refresh button, the profile form) may open a browser window; the scheduler and
    /// "refresh all" pass false so nothing pops up unprompted.
    pub async fn refresh_profile(&self, profile_id: &str, interactive: bool) -> RefreshResult {
        self.refresh_profile_with(profile_id, None, interactive).await
    }

    /// 1.x `refreshProfile(profileId, overrideCredentials, { interactive })`.
    pub async fn refresh_profile_with(&self, profile_id: &str, override_credentials: Option<IdpCredentials>, interactive: bool) -> RefreshResult {
        // Nothing can succeed while the app is locked: SAML needs the IdP password out of the
        // keychain and Identity Center needs the stored SSO session, and both are AES-encrypted
        // under a master password this session does not hold. Refusing here rather than failing
        // downstream keeps the failure counters clean — two consecutive failures pause
        // auto-refresh outright, so a locked app left sitting would otherwise come back with the
        // scheduler permanently paused.
        //
        // Deliberately the single chokepoint: the scheduler, "refresh all", the tray and the
        // console sign-in path all funnel through here.
        if self.is_locked() {
            return RefreshResult::Failed("App is locked. Unlock it with your master password to refresh.".into());
        }

        let Some(profile) = self.profiles().by_id(profile_id) else {
            self.note_auth_failure(profile_id, "Profile not found", None, "refreshProfile", false);
            return RefreshResult::Failed("Profile not found".into());
        };

        // Identity Center profiles take an entirely different path: no IdP form post, no SAML
        // assertion, no STS. Everything downstream (credentials file, expiration, dashboard) is
        // shared.
        if is_ic(&profile) {
            return self.refresh_identity_center_profile(&profile, interactive).await;
        }

        if !has_idp_url(&profile) {
            let error = "Profile has no IdP entry URL. Edit the profile and set the IdP URL.";
            self.note_auth_failure(profile_id, error, None, "refreshProfile", false);
            return RefreshResult::Failed(error.into());
        }
        let idp_entry_url = s(&profile, "idpEntryUrl").unwrap_or("").to_string();

        let creds = match override_credentials {
            Some(c) => c,
            None => {
                if !truthy(profile.get("useDefaultCredentials")) {
                    self.send_credentials_required(profile_id, None);
                    return RefreshResult::Required { profile_id: profile_id.into(), prefill_username: None };
                }
                // When using default credentials, only the default entry counts. If it has no
                // password, prompt every time (no fallback to a profile-stored one).
                // (1.x also caught a keychain read error here, "Could not read saved
                // credentials"; the Rust keychain reports a failed read as no entry.)
                match self.stored_credentials(DEFAULT_CREDENTIALS_ID) {
                    Some(c) if !crate::js::trim(&c.password).is_empty() => c,
                    other => {
                        let user = other.map(|c| c.username);
                        self.send_credentials_required(profile_id, user.as_deref());
                        return RefreshResult::Required { profile_id: profile_id.into(), prefill_username: Some(user.unwrap_or_default()) };
                    }
                }
            }
        };

        self.send_refresh_started(profile_id);
        let meta = idp::LoginMeta { profile_id: Some(profile_id), source: "refreshProfile" };
        let login = self.perform_login(&idp_entry_url, &creds.username, &creds.password, Some(meta)).await;
        drop(creds);
        let login = match login {
            Ok(l) => l,
            Err(e) => {
                self.note_auth_failure(profile_id, &e.message, e.info.as_ref(), "refreshProfile", true);
                return RefreshResult::Failed(e.message);
            }
        };

        let role = if login.roles.len() == 1 {
            Some(login.roles[0].clone())
        } else {
            match (s(&profile, "roleArn").filter(|v| !v.is_empty()), s(&profile, "principalArn").filter(|v| !v.is_empty())) {
                (Some(r), Some(p)) => login.roles.iter().find(|x| x.role_arn == r && x.principal_arn == p).cloned(),
                _ => None,
            }
        };
        let Some(role) = role else {
            self.auth_state.pending_auth.lock().unwrap().insert(profile_id.into(), PendingAuth { assertion: login.assertion, roles: login.roles.clone() });
            self.note_success();
            return RefreshResult::Roles { roles: login.roles, profile_id: profile_id.into() };
        };

        self.assume_and_save(profile, &role, &login.assertion, "refreshProfile").await
    }

    /// STS AssumeRoleWithSAML, then the credentials file, the profile's expiration and the
    /// events. Shared tail of `refreshProfile` and `selectRole`.
    async fn assume_and_save(&self, profile: Obj, role: &AwsRole, assertion: &str, source: &str) -> RefreshResult {
        let profile_id = id_of(&profile);
        let duration = duration_seconds(&profile);
        let creds = match sts::assume_role_with_saml(&self.cfg.http, &role.role_arn, &role.principal_arn, assertion, duration).await {
            Ok(sts::StsResult::Ok(c)) => c,
            Ok(sts::StsResult::NoCredentials) => {
                self.note_auth_failure(&profile_id, "STS did not return credentials", None, source, true);
                return RefreshResult::Failed("STS did not return credentials".into());
            }
            Err(e) => {
                self.note_auth_failure(&profile_id, &e.message, e.info.as_ref(), source, true);
                return RefreshResult::Failed(e.message);
            }
        };
        let expiration = to_iso(creds.expiration_ms.unwrap_or_else(|| self.cfg.clock.now_ms() + duration * 1000));
        let written = self.credentials.write_profile(
            &section_name(&profile),
            &crate::credentials_file::ProfileCredentials {
                access_key_id: &creds.access_key_id,
                secret_access_key: &creds.secret_access_key,
                session_token: &creds.session_token,
                region: Some(REGION),
                output: Some("json"),
            },
        );
        if let Err(e) = written {
            // 1.x's writeFileSync threw into the same catch.
            let message = e.to_string();
            self.note_auth_failure(&profile_id, &message, None, source, true);
            return RefreshResult::Failed(message);
        }

        let mut updated = profile.clone();
        updated.insert("roleArn".into(), role.role_arn.clone().into());
        updated.insert("principalArn".into(), role.principal_arn.clone().into());
        updated.insert("expiration".into(), expiration.into());
        self.profiles().save(updated);
        self.note_success();
        self.send_credentials_refreshed(&profile_id);
        self.notify("Credentials refreshed", &format!("Profile \"{}\" has been refreshed.", name_for_message(&profile)));
        RefreshResult::Success
    }

    /// Refresh an Identity Center profile.
    ///
    /// Silent whenever the SSO session can be renewed from its refresh token, which is the common
    /// case. When it can't, this returns `SsoLoginRequired` instead of an error — a human is
    /// needed, but nothing has failed, so it must not count toward the consecutive-failure
    /// auto-pause.
    ///
    /// `interactive` is true only when the user explicitly asked (Refresh button, profile form),
    /// never from the scheduler — that rule is what keeps browser windows from popping up
    /// unprompted.
    async fn refresh_identity_center_profile(&self, profile: &Obj, interactive: bool) -> RefreshResult {
        let profile_id = id_of(profile);
        let Some(org) = org_of_profile(s(profile, "ssoStartUrl"), s(profile, "ssoRegion")) else {
            let error = "Profile is missing its SSO start URL or region. Edit the profile to set them.";
            self.note_auth_failure(&profile_id, error, None, "refreshIdentityCenter", false);
            return RefreshResult::Failed(error.into());
        };
        let (Some(account_id), Some(role_name)) = (s(profile, "ssoAccountId").filter(|v| !v.is_empty()), s(profile, "ssoRoleName").filter(|v| !v.is_empty()))
        else {
            let error = "Profile has no account or role selected. Edit the profile to choose one.";
            self.note_auth_failure(&profile_id, error, None, "refreshIdentityCenter", false);
            return RefreshResult::Failed(error.into());
        };

        self.send_refresh_started(&profile_id);
        let fail = |e: ErrorInfo| {
            let message = e.message.clone().unwrap_or_default();
            self.note_auth_failure(&profile_id, &message, Some(&e), "refreshIdentityCenter", true);
            RefreshResult::Failed(message)
        };
        let access_token = match self.sso_access_token(&org, interactive).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                self.send_sso_login_required(&profile_id, &org.start_url, &org.region);
                // Not a failure: reset the counters so a waiting-on-human state can't trip the
                // auto-pause.
                self.note_success();
                return RefreshResult::SsoLoginRequired { profile_id: profile_id.clone(), start_url: org.start_url.clone(), region: org.region.clone() };
            }
            Err(e) => return fail(e),
        };
        let creds = match self.sso_role_credentials(&org, &access_token, account_id, role_name).await {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let expiration = to_iso(creds.expiration_ms);
        let region = s(profile, "ssoRegion").filter(|r| !r.is_empty()).unwrap_or(REGION);
        let written = self.credentials.write_profile(
            &section_name(profile),
            &crate::credentials_file::ProfileCredentials {
                access_key_id: &creds.access_key_id,
                secret_access_key: &creds.secret_access_key,
                session_token: &creds.session_token,
                region: Some(region),
                output: Some("json"),
            },
        );
        if let Err(e) = written {
            return fail(ErrorInfo { message: Some(e.to_string()), ..Default::default() });
        }
        let mut updated = profile.clone();
        updated.insert("expiration".into(), expiration.into());
        self.profiles().save(updated);
        self.append_auth_audit(crate::audit::AuditEntry::new("idp_success", "refreshIdentityCenter").with("profileId", profile_id.as_str()));
        self.note_success();
        self.send_credentials_refreshed(&profile_id);
        self.notify("Credentials refreshed", &format!("Profile \"{}\" has been refreshed.", name_for_message(profile)));
        RefreshResult::Success
    }

    /// 1.x `submitCredentials`. Always passes the typed credentials through; never persists them,
    /// so a default entry without a password keeps prompting.
    pub async fn submit_credentials(&self, profile_id: &str, username: &str, password: &str) -> RefreshResult {
        let creds = IdpCredentials { username: username.into(), password: password.into() };
        self.refresh_profile_with(profile_id, Some(creds), false).await
    }

    /// 1.x `selectRole`: finish a sign-in that matched several roles. The pending sign-in is used
    /// up even when the index is invalid.
    pub async fn select_role(&self, profile_id: &str, role_index: f64) -> RefreshResult {
        let pending = self.auth_state.pending_auth.lock().unwrap().remove(profile_id);
        let Some(pending) = pending else {
            let error = "No pending role selection. Please refresh again.";
            self.note_auth_failure(profile_id, error, None, "selectRole", false);
            return RefreshResult::Failed(error.into());
        };
        // (A fractional index made 1.x throw on `roles[1.5].roleArn`; treated as invalid here.)
        if !(role_index >= 0.0 && role_index < pending.roles.len() as f64 && role_index.fract() == 0.0) {
            self.note_auth_failure(profile_id, "Invalid role index", None, "selectRole", false);
            return RefreshResult::Failed("Invalid role index".into());
        }
        let role = pending.roles[role_index as usize].clone();
        let Some(profile) = self.profiles().by_id(profile_id) else {
            self.note_auth_failure(profile_id, "Profile not found", None, "selectRole", false);
            return RefreshResult::Failed("Profile not found".into());
        };
        self.assume_and_save(profile, &role, &pending.assertion, "selectRole").await
    }

    /// 1.x `refreshAllProfiles` ("Refresh all" in the tray). Splits profiles by whether we have
    /// credentials (stored default). If any need a prompt, show one modal and use that credential
    /// for all; otherwise refresh all with stored credentials.
    pub async fn refresh_all_profiles(&self) {
        let all = self.profiles().all();

        // Identity Center profiles have no idpEntryUrl and are not part of the shared-IdP
        // credential prompt at all. Refresh them non-interactively: any that need a sign-in emit
        // auth:ssoLoginRequired and the UI surfaces it, rather than "refresh all" throwing browser
        // windows at the user.
        for p in all.iter().filter(|p| is_ic(p)) {
            self.refresh_profile(&id_of(p), false).await;
        }

        let profiles: Vec<Obj> = all.into_iter().filter(|p| !is_ic(p) && has_idp_url(p)).collect();
        let (need, have) = self.split_profiles_by_credentials(&profiles);
        if !need.is_empty() {
            self.send_refresh_all_required(&need, &have);
            return;
        }
        for id in have {
            self.refresh_profile(&id, false).await;
        }
    }

    /// 1.x `refreshAutoRefreshProfiles`: only auto-refresh profiles that are expired or expiring
    /// soon (e.g. after unlock). Refreshes those with stored credentials first, then one prompt
    /// for the rest.
    pub async fn refresh_auto_refresh_profiles(&self) {
        let now = self.cfg.clock.now_ms();
        let due: Vec<Obj> = self
            .profiles()
            .all()
            .into_iter()
            .filter(|p| truthy(p.get("autoRefresh")))
            .filter(|p| should_refresh_by_expiration(p.get("expiration"), now))
            .collect();

        for p in due.iter().filter(|p| is_ic(p)) {
            self.refresh_profile(&id_of(p), false).await;
        }

        let profiles: Vec<Obj> = due.into_iter().filter(|p| !is_ic(p) && has_idp_url(p)).collect();
        if profiles.is_empty() {
            return;
        }
        let (need, have) = self.split_profiles_by_credentials(&profiles);
        // Refresh the profiles we have credentials for (no prompt).
        for id in &have {
            self.refresh_profile(id, false).await;
        }
        // If any need credentials, prompt once; the others are already refreshed, so pass [].
        if !need.is_empty() {
            self.send_refresh_all_required(&need, &[]);
        }
    }

    /// 1.x `submitCredentialsForRefreshAll`: first every `credential_profile_ids` with the typed
    /// credentials, then every `default_profile_ids` with the stored default credentials.
    pub async fn submit_credentials_for_refresh_all(&self, credential_profile_ids: &[String], default_profile_ids: &[String], username: &str, password: &str) {
        for id in credential_profile_ids {
            let creds = IdpCredentials { username: username.into(), password: password.into() };
            self.refresh_profile_with(id, Some(creds), false).await;
        }
        for id in default_profile_ids {
            self.refresh_profile(id, false).await;
        }
    }

    // ---- IPC ---------------------------------------------------------------------------------

    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_auth(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let arg = |i: usize| args.get(i).unwrap_or(&Value::Null);
        let arg_str = |i: usize| arg(i).as_str().unwrap_or("").to_string();
        let ids =
            |i: usize| -> Vec<String> { arg(i).as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let result = match channel {
            // Direct user action, so an Identity Center profile may open a sign-in window.
            "auth:refresh" => self.refresh_profile(&arg_str(0), true).await.to_value(),
            "auth:refreshAll" => {
                self.refresh_all_profiles().await;
                Value::Null
            }
            "auth:refreshAutoRefreshProfiles" => {
                self.refresh_auto_refresh_profiles().await;
                Value::Null
            }
            "auth:submitCredentials" => self.submit_credentials(&arg_str(0), &arg_str(1), &arg_str(2)).await.to_value(),
            "auth:submitCredentialsForRefreshAll" => {
                self.submit_credentials_for_refresh_all(&ids(0), &ids(1), &arg_str(2), &arg_str(3)).await;
                Value::Null
            }
            "auth:selectRole" => self.select_role(&arg_str(0), arg(1).as_f64().unwrap_or(f64::NAN)).await.to_value(),
            "auth:fetchRoles" => {
                let profile_id = arg(2).as_str().map(str::to_string);
                self.fetch_roles_for_idp(&arg_str(0), truthy(Some(arg(1))), profile_id.as_deref()).await
            }
            "auth:fetchRolesWithCredentials" => self.fetch_roles_with_credentials(&arg_str(0), &arg_str(1), &arg_str(2)).await,
            "scheduler:getPaused" => self.refresh_pause_state().to_value(),
            "scheduler:setPaused" => {
                self.set_refresh_paused(truthy(Some(arg(0))), false);
                Value::Null
            }
            _ => return None,
        };
        Some(Ok(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refused_connection_reads_like_node_and_classifies_as_network() {
        let url = "https://127.0.0.1:9/idp/sso";
        let e = reqwest::Client::new().get(url).send().await.unwrap_err();
        let err = transport_error(&e, url);
        assert_eq!(err.message, "connect ECONNREFUSED 127.0.0.1:9");
        assert_eq!(classify(err.info.as_ref()), FailureKind::Network);
    }

    #[test]
    fn expiry_threshold() {
        let now = 1_000_000_000_000;
        assert!(should_refresh_by_expiration(None, now));
        assert!(should_refresh_by_expiration(Some(&json!("")), now));
        assert!(!should_refresh_by_expiration(Some(&json!("not a date")), now));
        assert!(should_refresh_by_expiration(Some(&json!(to_iso(now + 14 * 60_000))), now));
        assert!(!should_refresh_by_expiration(Some(&json!(to_iso(now + 16 * 60_000))), now));
    }

    #[test]
    fn section_and_duration_follow_1x() {
        let p = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(section_name(&p(json!({ "credentialProfileName": "", "name": " Dev " }))), "Dev");
        assert_eq!(section_name(&p(json!({ "name": "   " }))), "default");
        assert_eq!(duration_seconds(&p(json!({ "refreshIntervalMinutes": 120 }))), 7200);
        assert_eq!(duration_seconds(&p(json!({ "refreshIntervalMinutes": 2000 }))), 43200);
    }
}
