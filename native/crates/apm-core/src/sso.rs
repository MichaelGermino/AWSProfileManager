//! IAM Identity Center (Entra-federated) authentication. Port of `identityCenterService.ts`; the
//! session store (`ssoTokenStore.ts`) is in `sso/store.rs`.
//!
//! Talks to the sso-oidc and portal.sso REST APIs directly over HTTPS rather than through an AWS
//! SDK: the surface is six calls. Every request goes through the one shared client setup
//! (`http.rs`: OS trust store for TLS-inspecting proxies, User-Agent, test host map).
//!
//! Flow: RegisterClient (cached ~90d) → authorization code + PKCE in a browser → CreateToken →
//! access token (1h) + refresh token. Renewal via the refresh token is headless; only a dead
//! refresh token forces a browser trip.

mod loopback;
pub mod protect;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, watch, Notify};
use tokio::time::Instant;

use crate::core::{Core, DispatchResult};
use crate::host::{NoSignInWindow, SignInRequest, SignInWindow};
use crate::network_classifier::ErrorInfo;
use crate::sso_org::{normalize_start_url, org_key, org_of_profile, sso_keychain_account, SsoOrg};
use store::{is_registration_usable, is_token_usable, SsoRegistration, SsoSession, SsoToken};

const CLIENT_NAME: &str = "AWSProfileManager";
const SCOPES: &[&str] = &["sso:account:access"];
/// The user has to complete an Entra sign-in plus MFA in here.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long to let a sign-in try to complete invisibly before showing the window. A round-trip that
/// needs no input finishes well inside this; anything slower means the IdP is asking for something.
const SILENT_ATTEMPT: Duration = Duration::from_secs(4);
/// Cap for a sign-in that may NEVER be shown (the scheduler's headless attempt). Without it a window
/// nobody can see would sit on an MFA prompt for the full LOGIN_TIMEOUT.
const HEADLESS_ATTEMPT: Duration = Duration::from_secs(20);
/// Don't retry a headless attempt for the same org more often than this. The scheduler ticks every
/// 60s; without a cooldown a genuinely expired session would spawn a hidden window every minute.
const HEADLESS_RETRY_COOLDOWN_MS: i64 = 30 * 60_000;
const TIMED_OUT: &str = "Sign-in timed out. Try again.";
const NEEDS_INPUT: &str = "Sign-in needs input; not available headlessly.";
/// Folder (under app data) holding one embedded-browser data directory per org.
const SIGN_IN_DATA_DIR: &str = "sso-webview";

fn oidc_host(region: &str) -> String {
    format!("oidc.{region}.amazonaws.com")
}

fn portal_host(region: &str) -> String {
    format!("portal.sso.{region}.amazonaws.com")
}

/// Role credentials from the portal's GetRoleCredentials.
#[derive(Debug, Clone)]
pub struct SsoRoleCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    /// Epoch ms.
    pub expiration_ms: i64,
}

fn failure(message: impl Into<String>) -> ErrorInfo {
    ErrorInfo { message: Some(message.into()), ..Default::default() }
}

fn message_of(e: &ErrorInfo) -> String {
    e.message.clone().unwrap_or_default()
}

/// JavaScript `String(x)` for the JSON values these APIs return.
fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(Value::Number(n)) => crate::json::stringify(&Value::Number(n.clone())),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(a)) => a.iter().map(|x| if x.is_null() { String::new() } else { js_string(Some(x)) }).collect::<Vec<_>>().join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `x ?? y`: the first value that is present and not null.
fn coalesce<'a>(obj: Option<&'a Value>, keys: &[&str]) -> Option<&'a Value> {
    let obj = obj?;
    keys.iter().find_map(|k| obj.get(*k).filter(|v| !v.is_null()))
}

/// JavaScript `Number(x)` truncated to an integer (NaN → 0).
fn js_number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// JavaScript `a.localeCompare(b)` for plain names: case-insensitive first, lowercase first on ties.
fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

// ------------------------------------------------------------------------------------------------
// Per-Core state
// ------------------------------------------------------------------------------------------------

type LoginResult = Result<String, ErrorInfo>;

/// What 1.x kept in module-level variables: one instance per running core.
#[derive(Default)]
pub(crate) struct SsoState {
    pub(crate) memory: store::MemorySessions,
    /// One in-flight interactive login per org. Eight profiles in the same org refreshing at once
    /// must produce one browser window, and one successful sign-in must satisfy all eight.
    inflight: Mutex<HashMap<String, watch::Receiver<Option<LoginResult>>>>,
    /// orgKey -> promote the in-flight sign-in to a visible, full-deadline one. Present only while
    /// a sign-in is actually running.
    promotable: Mutex<HashMap<String, Arc<Notify>>>,
    /// orgKey -> when a headless attempt last failed.
    last_headless_attempt: Mutex<HashMap<String, i64>>,
    /// One pooled client, so kept-alive connections are reused (and retried when stale).
    client: OnceLock<reqwest::Client>,
}

/// Removes the org's in-flight entry when the login finishes or is dropped.
struct InflightGuard<'a> {
    state: &'a SsoState,
    key: String,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.state.inflight.lock().unwrap().remove(&self.key);
    }
}

// ------------------------------------------------------------------------------------------------
// Transport
// ------------------------------------------------------------------------------------------------

struct HttpResult {
    status: u16,
    json: Option<Value>,
    text: String,
}

/// An OIDC / portal API failure: `<what> failed: <code> — <description>`.
fn expect_ok(res: HttpResult, what: &str) -> Result<Value, ErrorInfo> {
    if (200..300).contains(&res.status) {
        return Ok(res.json.unwrap_or_else(|| json!({})));
    }
    let code = match coalesce(res.json.as_ref(), &["error", "__type"]) {
        Some(v) => js_string(Some(v)),
        None => format!("HTTP {}", res.status),
    };
    let description = coalesce(res.json.as_ref(), &["error_description", "message"]).map(|v| js_string(Some(v))).unwrap_or_default();
    let detail = if !description.is_empty() {
        format!(" — {description}")
    } else if !res.text.is_empty() {
        format!(" — {}", res.text.chars().take(200).collect::<String>())
    } else {
        String::new()
    };
    // The OAuth / API error code rides along so callers can tell "rejected" from "unavailable".
    let mut err = failure(format!("{what} failed: {code}{detail}"));
    err.code = Some(code);
    Err(err)
}

/// OAuth codes meaning the refresh token (or the client it belongs to) is no longer accepted.
/// Only these end the silent session; anything else (network, throttling, 5xx) is transient.
const REFRESH_REJECTED: &[&str] = &["invalid_grant", "expired_token", "invalid_client", "unauthorized_client", "access_denied"];

fn is_refresh_rejected(err: &ErrorInfo) -> bool {
    err.code.as_deref().is_some_and(|c| REFRESH_REJECTED.contains(&c))
}

const fn bearer_header() -> &'static str {
    "x-amz-sso_bearer_token"
}

fn b64url_random(n: usize) -> String {
    let bytes: Vec<u8> = (0..n).map(|_| rand::random::<u8>()).collect();
    B64URL.encode(bytes)
}

/// Best-effort: read the subject/email claim so the UI can show which account signed in.
fn identity_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1).filter(|p| !p.is_empty())?;
    // Node's Buffer.from(_, 'base64') accepts both alphabets, with or without padding.
    let normalized: String = payload
        .chars()
        .filter(|c| *c != '=')
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(normalized).ok()?;
    let decoded: Value = serde_json::from_slice(&bytes).ok()?;
    match coalesce(Some(&decoded), &["email", "preferred_username", "sub"])? {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// A token from CreateToken, plus who it was issued to.
struct IssuedToken {
    token: SsoToken,
    identity: Option<String>,
}

/// Argument helpers for `dispatch_sso`.
fn arg_str(args: &[Value], i: usize) -> &str {
    args.get(i).and_then(Value::as_str).unwrap_or("")
}

impl Core {
    pub(crate) fn sso_state(&self) -> Arc<SsoState> {
        self.sso.clone()
    }

    fn sso_client(&self) -> reqwest::Client {
        self.sso_state().client.get_or_init(|| self.cfg.http.client(false, false)).clone()
    }

    async fn request_json(&self, host: &str, path: &str, body: Option<&Value>, headers: &[(&str, &str)]) -> Result<HttpResult, ErrorInfo> {
        // Identity in production; test mode may send it to the fake server.
        let url = self.cfg.http.remap(&format!("https://{host}{path}"));
        let client = self.sso_client();
        let mut req = match body {
            Some(_) => client.post(&url),
            None => client.get(&url),
        }
        .header("accept", "application/json");
        if let Some(b) = body {
            req = req.header("content-type", "application/json").body(crate::json::stringify(b));
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = req.send().await.map_err(|e| crate::http::error_info(&e))?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| crate::http::error_info(&e))?;
        // Non-JSON body (proxy error page, HTML 403): keep the text for the message.
        let json = if text.is_empty() { None } else { serde_json::from_str(&text).ok() };
        Ok(HttpResult { status, json, text })
    }

    // -------------------------------------------------------------------------------- OIDC calls

    async fn register_client(&self, org: &SsoOrg, redirect_uri: &str) -> Result<SsoRegistration, ErrorInfo> {
        let body = json!({
            "clientName": CLIENT_NAME,
            "clientType": "public",
            "scopes": SCOPES,
            "grantTypes": ["authorization_code", "refresh_token"],
            "redirectUris": [redirect_uri],
            "issuerUrl": org.start_url,
        });
        let res = expect_ok(self.request_json(&oidc_host(&org.region), "/client/register", Some(&body), &[]).await?, "RegisterClient")?;
        Ok(SsoRegistration {
            client_id: js_string(res.get("clientId")),
            client_secret: js_string(res.get("clientSecret")),
            client_secret_expires_at: js_number(res.get("clientSecretExpiresAt")).unwrap_or(0.0) as i64,
            redirect_uri: redirect_uri.to_string(),
        })
    }

    async fn create_token(&self, org: &SsoOrg, body: Value) -> Result<IssuedToken, ErrorInfo> {
        let res = expect_ok(self.request_json(&oidc_host(&org.region), "/token", Some(&body), &[]).await?, "CreateToken")?;
        let expires_in = match coalesce(Some(&res), &["expiresIn"]) {
            None => 3600.0,
            Some(v) => js_number(Some(v)).unwrap_or(0.0),
        };
        let refresh_token = res.get("refreshToken").filter(|v| match v {
            Value::String(s) => !s.is_empty(),
            Value::Null | Value::Bool(false) => false,
            _ => true,
        });
        Ok(IssuedToken {
            token: SsoToken {
                access_token: js_string(res.get("accessToken")),
                refresh_token: refresh_token.map(|v| js_string(Some(v))),
                expires_at: self.cfg.clock.now_ms() + (expires_in * 1000.0) as i64,
            },
            identity: res.get("idToken").and_then(Value::as_str).and_then(identity_from_id_token),
        })
    }

    // -------------------------------------------------------------------------------- sign-in

    fn sso_browser_mode(&self) -> String {
        self.settings().get().get("ssoBrowserMode").and_then(Value::as_str).unwrap_or("embedded").to_string()
    }

    /// The org's embedded-browser data directory (1.x: partition `persist:<sso account>`).
    fn sign_in_data_dir(&self, account: &str) -> PathBuf {
        self.cfg.paths.app_file(SIGN_IN_DATA_DIR).join(account.replace(':', "-"))
    }

    fn emit_progress(&self, progress: Value) {
        self.host.emit("sso:loginProgress", vec![progress]);
    }

    /// Open the authorize URL for the user to complete.
    ///
    /// Default is an embedded window on a per-org **persistent** browser data directory. That
    /// matters for organizations that issue separate day-to-day and privileged (SA) identities: the
    /// normal browser session is signed in as the wrong identity, so the external browser would
    /// silently authenticate the wrong account. A dedicated data directory is isolated like a
    /// private window but — unlike one — keeps the session across app restarts, which is what makes
    /// repeat sign-ins ~1s and click-free.
    ///
    /// Falls back to the external browser when settings.ssoBrowserMode is 'external', for tenants
    /// whose Conditional Access policy rejects embedded webviews.
    ///
    /// The window starts HIDDEN. Most re-authentications need no input at all: the data directory
    /// still holds a live IdP session, so the whole OAuth round-trip completes on its own and the
    /// window is closed having never been seen. It is only worth showing when the IdP actually
    /// wants something — a password, consent, or an MFA code — and the caller detects that by the
    /// callback simply not arriving. There is no way to tell "needs MFA" from "needs a password"
    /// from outside; both look like silence. Which is fine, because the answer is the same.
    fn open_authorize_url(&self, org: &SsoOrg, authorize_url: &str) -> (Box<dyn SignInWindow>, oneshot::Receiver<()>) {
        let target = self.cfg.http.remap(authorize_url);
        let (tx, rx) = oneshot::channel();
        if self.sso_browser_mode() == "external" {
            self.host.open_external("url", &target);
            // No window of ours to watch, so this can never settle — and nothing to hide or reveal.
            drop(tx);
            return (Box::new(NoSignInWindow), rx);
        }
        let request = SignInRequest { url: target, data_dir: self.sign_in_data_dir(&sso_keychain_account(org)), title: "Sign in to AWS".into() };
        let window = self.host.open_sign_in(
            request,
            Box::new(move || {
                let _ = tx.send(());
            }),
        );
        (window, rx)
    }

    async fn interactive_login(&self, org: &SsoOrg, headless: bool) -> Result<IssuedToken, ErrorInfo> {
        // A headless attempt must be invisible in every sense: no window, and no progress events
        // either, or the window would announce a sign-in the user never asked for.
        let report = |p: Value| {
            if !headless {
                self.emit_progress(p);
            }
        };
        let now = self.cfg.clock.now_ms();
        let existing = self.read_session(org);
        let cached_reg = existing.as_ref().and_then(|s| s.registration.clone()).filter(|r| is_registration_usable(Some(r), now));

        // Reuse the registered port when we have a registration, otherwise take any free one.
        let cached_port = cached_reg.as_ref().and_then(|r| reqwest::Url::parse(&r.redirect_uri).ok()).and_then(|u| u.port());
        let server = match loopback::start(cached_port).await {
            Ok(s) => s,
            // Port taken by something else; re-register on a fresh port rather than failing.
            Err(_) if cached_port.is_some() => loopback::start(None).await.map_err(|e| failure(e.to_string()))?,
            Err(e) => return Err(failure(e.to_string())),
        };
        let result = self.login_with_server(org, headless, existing, cached_reg, server, &report).await;
        if let Err(e) = &result {
            // Reset the IdP session on ANY failed attempt, so the next one starts at the account
            // picker.
            //
            // An IdP-side rejection (AADSTS50105 for an unassigned account, conditional access, MFA
            // refusal) never reaches our callback: the user just sees an error page and closes the
            // window, which lands here as "cancelled". Leaving the browser data intact meant Entra
            // silently reused the rejected identity forever, so picking the right account changed
            // nothing and the only escape was resetting the whole app.
            //
            // Awaited before returning so the next attempt cannot race a half-finished wipe. The
            // cost is re-entering credentials after a cancel, which is the right trade against a
            // dead end.
            self.clear_auth_partition(org).await;
            report(json!({ "phase": "failed", "startUrl": org.start_url, "error": message_of(e) }));
        }
        result
    }

    async fn login_with_server(
        &self,
        org: &SsoOrg,
        headless: bool,
        existing: Option<SsoSession>,
        cached_reg: Option<SsoRegistration>,
        mut server: loopback::CallbackServer,
        report: &(dyn Fn(Value) + Sync),
    ) -> Result<IssuedToken, ErrorInfo> {
        let registration = match cached_reg {
            Some(r) if r.redirect_uri == server.redirect_uri => r,
            _ => self.register_client(org, &server.redirect_uri).await?,
        };
        let mut with_reg = existing.unwrap_or_default();
        with_reg.registration = Some(registration.clone());
        let _ = self.write_session(org, &with_reg);

        let verifier = b64url_random(32);
        let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
        let state = b64url_random(16);

        // AWS uses 'scopes' (plural) here, not the OAuth-standard 'scope'.
        let mut authorize = reqwest::Url::parse(&format!("https://{}/authorize", oidc_host(&org.region))).map_err(|e| failure(e.to_string()))?;
        authorize
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &registration.client_id)
            .append_pair("redirect_uri", &server.redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("scopes", &SCOPES.join(" "));

        report(json!({ "phase": "opening", "startUrl": org.start_url }));
        let (browser, closed_by_user) = self.open_authorize_url(org, authorize.as_str());
        report(json!({ "phase": "waiting", "startUrl": org.start_url }));

        // The window starts hidden. Reveal it only once it is clear the IdP wants something, which
        // from out here looks simply like the callback not arriving.
        //
        // `headless` callers (the scheduler) never reveal, because a background tick must not put
        // a window on screen. They take a much shorter deadline and then give up, leaving the user
        // to be prompted; completing it from that prompt runs the interactive path, where the
        // window shows.
        let (mut deadline, mut timeout_message) =
            if headless { (Instant::now() + HEADLESS_ATTEMPT, NEEDS_INPUT) } else { (Instant::now() + LOGIN_TIMEOUT, TIMED_OUT) };

        // Turn a headless attempt into a visible one.
        //
        // Registrations pin the loopback redirect port, so two sign-ins for an org cannot run at
        // once — a user who clicks "Sign in" while a background attempt is running has to join that
        // attempt rather than start their own. Joining it unchanged would mean staring at nothing
        // until the 20-second headless deadline killed it, so joining promotes it: show the window
        // and give them the full interactive deadline to finish a password or MFA code.
        let key = org_key(org);
        let promote = Arc::new(Notify::new());
        let st = self.sso_state();
        st.promotable.lock().unwrap().insert(key.clone(), promote.clone());
        let mut reveal_at = if headless { None } else { Some(Instant::now() + SILENT_ATTEMPT) };

        // Closing the window is how a user says "not now" — without watching for it they would sit
        // on "waiting for browser" until LOGIN_TIMEOUT. The external browser never reports.
        let cancelled = async move {
            if closed_by_user.await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        tokio::pin!(cancelled);

        let outcome = loop {
            let reveal_sleep = async {
                match reveal_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                params = &mut server.received => break params.map_err(|_| failure(TIMED_OUT)),
                _ = tokio::time::sleep_until(deadline) => break Err(failure(timeout_message)),
                _ = &mut cancelled => break Err(failure("Sign-in was cancelled.")),
                _ = promote.notified() => {}
                _ = reveal_sleep => {}
            }
            // Promote (by a joining caller or the silent-attempt timer).
            reveal_at = None;
            browser.reveal();
            deadline = Instant::now() + LOGIN_TIMEOUT;
            timeout_message = TIMED_OUT;
        };
        // Settled: no late reveal can flash a window the user never needed to see.
        st.promotable.lock().unwrap().remove(&key);
        browser.close();
        let params = outcome?;
        drop(server);

        if let Some(error) = params.get("error") {
            return Err(failure(format!("Sign-in failed: {}", params.get("error_description").unwrap_or(error))));
        }
        if params.get("state") != Some(&state) {
            return Err(failure("Sign-in failed: state mismatch."));
        }

        let issued = self
            .create_token(
                org,
                json!({
                    "clientId": registration.client_id,
                    "clientSecret": registration.client_secret,
                    "grantType": "authorization_code",
                    "code": params.get("code").cloned().map_or(Value::Null, Value::String),
                    "redirectUri": registration.redirect_uri,
                    "codeVerifier": verifier,
                }),
            )
            .await?;

        let session =
            SsoSession { registration: Some(registration), token: Some(issued.token.clone()), identity: issued.identity.clone(), ..Default::default() };
        if let Err(reason) = self.write_session(org, &session) {
            // The session still works for this run (it is cached in memory), but it won't survive
            // a restart. Say so rather than letting the user rediscover it as a surprise re-login.
            eprintln!("[sso] session not persisted ({}); sign-in will be required again after restart", reason.as_str());
        }
        let mut done = json!({ "phase": "done", "startUrl": org.start_url });
        if let Some(identity) = &issued.identity {
            done["identity"] = Value::String(identity.clone());
        }
        report(done);
        Ok(issued)
    }

    /// Run (or join) the org's one in-flight login.
    async fn coalesced_login(&self, org: &SsoOrg, headless: bool) -> LoginResult {
        let key = org_key(org);
        let st = self.sso_state();
        let (tx, rx) = watch::channel(None);
        let joined = {
            let mut inflight = st.inflight.lock().unwrap();
            match inflight.get(&key) {
                Some(existing) => Some(existing.clone()),
                None => {
                    inflight.insert(key.clone(), rx);
                    None
                }
            }
        };
        if let Some(rx) = joined {
            return join_login(rx).await;
        }
        let guard = InflightGuard { state: &st, key };
        let result = self.interactive_login(org, headless).await.map(|t| t.token.access_token);
        drop(guard);
        let _ = tx.send(Some(result.clone()));
        result
    }

    /// Headless sign-in is only possible in the embedded browser — 'external' hands the URL to the
    /// real browser, which would pop a window for a background tick. The cooldown stops a genuinely
    /// expired session spawning a hidden window on every 60-second scheduler tick.
    fn can_attempt_headless_login(&self, org: &SsoOrg) -> bool {
        if self.sso_browser_mode() != "embedded" {
            return false;
        }
        let last = self.sso_state().last_headless_attempt.lock().unwrap().get(&org_key(org)).copied();
        last.is_none_or(|l| self.cfg.clock.now_ms() - l >= HEADLESS_RETRY_COOLDOWN_MS)
    }

    pub fn sso_has_inflight_login(&self, org: &SsoOrg) -> bool {
        self.sso_state().inflight.lock().unwrap().contains_key(&org_key(org))
    }

    /// A usable access token for `org` (1.x `getAccessToken`). `Ok(None)`: a human must sign in
    /// (never an error). `interactive: false` (the scheduler) never opens a browser: it uses the
    /// cached token or the refresh token, and returns None if neither works. The caller then
    /// reports ssoLoginRequired rather than treating it as a failure.
    pub async fn sso_access_token(&self, org: &SsoOrg, interactive: bool) -> Result<Option<String>, ErrorInfo> {
        let key = org_key(org);
        let st = self.sso_state();
        // A sign-in the user is already completing counts, whatever mode we were called in.
        // Without this, a scheduler tick landing while the browser window is open reports "login
        // required" and throws a second prompt at a user who is mid-sign-in.
        let inflight = st.inflight.lock().unwrap().get(&key).cloned();
        if let Some(rx) = inflight {
            // If that in-flight attempt is a headless one, an interactive caller joining it would
            // wait on a window that is never shown and dies at the headless deadline — their click
            // would look like it did nothing. Promote it instead: show the window, extend the
            // deadline.
            if interactive {
                if let Some(p) = st.promotable.lock().unwrap().get(&key) {
                    p.notify_one();
                }
            }
            return Ok(join_login(rx).await.ok());
        }

        let session = self.read_session(org);
        let now = self.cfg.clock.now_ms();
        if let Some(token) = session.as_ref().and_then(|s| s.token.as_ref()).filter(|t| is_token_usable(Some(t), now)) {
            return Ok(Some(token.access_token.clone()));
        }

        // Silent renewal — the path that keeps the scheduler headless.
        let registration = session.as_ref().and_then(|s| s.registration.clone());
        let refresh_token = session.as_ref().and_then(|s| s.token.as_ref()).and_then(|t| t.refresh_token.clone()).filter(|r| !r.is_empty());
        if let (Some(registration), Some(refresh_token)) = (registration.filter(|r| is_registration_usable(Some(r), now)), refresh_token) {
            let renewed = self
                .create_token(
                    org,
                    json!({
                        "clientId": registration.client_id,
                        "clientSecret": registration.client_secret,
                        "grantType": "refresh_token",
                        "refreshToken": refresh_token,
                    }),
                )
                .await;
            match renewed {
                Ok(issued) => {
                    // Identity Center does not currently rotate the refresh token, but honor it if
                    // it ever does.
                    let mut token = issued.token;
                    token.refresh_token = token.refresh_token.or(Some(refresh_token));
                    let access = token.access_token.clone();
                    let merged =
                        SsoSession { registration: Some(registration), token: Some(token), identity: session.and_then(|s| s.identity), ..Default::default() };
                    let _ = self.write_session(org, &merged);
                    return Ok(Some(access));
                }
                // Refresh token rejected (SSO session expired): drop it and fall through to
                // interactive sign-in.
                Err(e) if is_refresh_rejected(&e) => self.clear_token_keep_registration(org),
                // Anything else (offline, throttled, AWS unavailable) says nothing about the token.
                // 1.x dropped it here too, so one network blip at a scheduler tick meant a browser
                // sign-in later. Keep it and report the failure; the next attempt renews silently.
                // Changed 2026-11-14.
                Err(e) => return Err(e),
            }
        }

        // Non-interactive callers (the scheduler) now get one more chance before the user is
        // bothered: a completely headless sign-in. When the IdP session is still alive this
        // succeeds on its own, and a re-authentication that used to mean a prompt plus a browser
        // window becomes invisible.
        //
        // If it needs a human it is abandoned rather than shown — a background tick must never put
        // a window on screen — and we fall back to returning None, so the caller emits the same
        // "sign in required" prompt as before. The user clicks it, and the interactive path may
        // then reveal the window for the password or MFA code.
        if !interactive {
            if !self.can_attempt_headless_login(org) {
                return Ok(None);
            }
            st.last_headless_attempt.lock().unwrap().insert(key.clone(), self.cfg.clock.now_ms());
            return Ok(match self.coalesced_login(org, true).await {
                Ok(token) => {
                    st.last_headless_attempt.lock().unwrap().remove(&key);
                    Some(token)
                }
                Err(_) => None,
            });
        }

        self.coalesced_login(org, false).await.map(Some)
    }

    // -------------------------------------------------------------------------------- portal API

    async fn portal_get(&self, org: &SsoOrg, path: &str, access_token: &str, what: &str) -> Result<Value, ErrorInfo> {
        expect_ok(self.request_json(&portal_host(&org.region), path, None, &[(bearer_header(), access_token)]).await?, what)
    }

    /// Accounts plus the permission sets assigned to the signed-in user, sorted for a stable UI.
    pub async fn sso_list_accounts_with_roles(&self, org: &SsoOrg, access_token: &str) -> Result<Vec<Value>, ErrorInfo> {
        struct Account {
            id: String,
            name: String,
            email: Option<String>,
            roles: Vec<String>,
        }
        let mut accounts: Vec<Account> = Vec::new();
        let mut next_token: Option<String> = None;
        loop {
            let mut qs = form(&[("max_result", "100")]);
            if let Some(t) = &next_token {
                qs.push('&');
                qs.push_str(&form(&[("next_token", t)]));
            }
            let page = self.portal_get(org, &format!("/assignment/accounts?{qs}"), access_token, "ListAccounts").await?;
            for a in page.get("accountList").and_then(Value::as_array).cloned().unwrap_or_default() {
                let id = js_string(a.get("accountId"));
                let name = coalesce(Some(&a), &["accountName"]).map(|v| js_string(Some(v))).unwrap_or_else(|| id.clone());
                let email = a.get("emailAddress").filter(|v| truthy(v)).map(|v| js_string(Some(v)));
                accounts.push(Account { id, name, email, roles: Vec::new() });
            }
            next_token = page.get("nextToken").filter(|v| truthy(v)).map(|v| js_string(Some(v)));
            if next_token.is_none() {
                break;
            }
        }

        for account in &mut accounts {
            let qs = form(&[("account_id", &account.id), ("max_result", "100")]);
            let page = self.portal_get(org, &format!("/assignment/roles?{qs}"), access_token, "ListAccountRoles").await?;
            let mut roles: Vec<String> =
                page.get("roleList").and_then(Value::as_array).map(|l| l.iter().map(|r| js_string(r.get("roleName"))).collect()).unwrap_or_default();
            roles.sort_by(|a, b| locale_compare(a, b));
            account.roles = roles;
        }

        // ListAccounts returns a different order on every call; sort so the picker doesn't reshuffle.
        accounts.sort_by(|a, b| locale_compare(&a.name, &b.name));
        Ok(accounts
            .into_iter()
            .map(|a| {
                let mut o = Map::new();
                o.insert("accountId".into(), a.id.into());
                o.insert("accountName".into(), a.name.into());
                if let Some(e) = a.email {
                    o.insert("emailAddress".into(), e.into());
                }
                o.insert("roles".into(), a.roles.into());
                Value::Object(o)
            })
            .collect())
    }

    /// 1.x `getRoleCredentials`.
    pub async fn sso_role_credentials(&self, org: &SsoOrg, access_token: &str, account_id: &str, role_name: &str) -> Result<SsoRoleCredentials, ErrorInfo> {
        let qs = form(&[("account_id", account_id), ("role_name", role_name)]);
        let res = self.portal_get(org, &format!("/federation/credentials?{qs}"), access_token, "GetRoleCredentials").await?;
        let creds = res.get("roleCredentials");
        let field = |k: &str| creds.and_then(|c| c.get(k)).filter(|v| truthy(v));
        let (Some(key), Some(secret), Some(token)) = (field("accessKeyId"), field("secretAccessKey"), field("sessionToken")) else {
            return Err(failure("Identity Center did not return credentials."));
        };
        Ok(SsoRoleCredentials {
            access_key_id: js_string(Some(key)),
            secret_access_key: js_string(Some(secret)),
            session_token: js_string(Some(token)),
            expiration_ms: js_number(creds.and_then(|c| c.get("expiration"))).unwrap_or(0.0) as i64,
        })
    }

    // -------------------------------------------------------------------------------- sessions

    pub fn sso_session_status(&self, org: &SsoOrg) -> Value {
        let Some(session) = self.read_session(org) else { return json!({ "signedIn": false }) };
        let Some(token) = &session.token else { return json!({ "signedIn": false }) };
        let now = self.cfg.clock.now_ms();
        let renewable = token.refresh_token.as_deref().is_some_and(|r| !r.is_empty()) && is_registration_usable(session.registration.as_ref(), now);
        if !is_token_usable(Some(token), now) && !renewable {
            return json!({ "signedIn": false });
        }
        let mut out = json!({ "signedIn": true, "expiresAt": crate::env::to_iso(token.expires_at) });
        if let Some(identity) = &session.identity {
            out["identity"] = Value::String(identity.clone());
        }
        out
    }

    /// Wipe the embedded browser's IdP session for an org, without touching the stored SSO session.
    ///
    /// The data directory is deliberately persistent so repeat sign-ins are fast, but that also
    /// means a failed attempt leaves the IdP logged in as whoever was used. Entra then reuses that
    /// session silently on the next attempt, so retrying with the right account replays the *old*
    /// account's error — most visibly AADSTS50105, which names a user the person never chose this
    /// time.
    async fn clear_auth_partition(&self, org: &SsoOrg) {
        let dir = self.sign_in_data_dir(&sso_keychain_account(org));
        // The webview may take a moment to release its files after the window closed.
        for _ in 0..10 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => return,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => tokio::time::sleep(Duration::from_millis(300)).await,
            }
        }
    }

    /// Clear the embedded browser's IdP session for EVERY org the app knows about.
    ///
    /// Deleting stored SSO tokens is not enough to force a re-authentication: the persistent
    /// browser data still holds the Entra session, so the next sign-in is satisfied silently and
    /// the user sees only AWS's consent screen. After an explicit "forget everything" that is the
    /// wrong answer — the whole point is that getting AWS credentials again should require proving
    /// who you are.
    ///
    /// Session accounts are read first because they name the data directories; profiles are
    /// folded in so an org whose session already expired still gets its browser data cleared.
    pub fn clear_all_auth_partitions(&self) {
        let mut accounts: Vec<String> = self.stored_session_accounts();
        for p in self.profiles().all() {
            let s = |k: &str| p.get(k).and_then(Value::as_str);
            if let Some(org) = org_of_profile(s("ssoStartUrl"), s("ssoRegion")) {
                accounts.push(sso_keychain_account(&org));
            }
        }
        for account in accounts {
            // The data directory may not exist.
            let _ = std::fs::remove_dir_all(self.sign_in_data_dir(&account));
        }
    }

    /// Sign out of an org. Also clears the browser data so the next sign-in starts clean.
    pub async fn sso_sign_out(&self, org: &SsoOrg) {
        self.clear_session(org);
        self.clear_auth_partition(org).await;
    }

    /// Explicit user-driven sign-in, used by the profile form and the import wizard.
    pub async fn sso_sign_in(&self, org: &SsoOrg) -> Value {
        match self.sso_access_token(org, true).await {
            Ok(_) => {
                let mut out = json!({ "success": true });
                if let Some(identity) = self.sso_session_status(org).get("identity") {
                    out["identity"] = identity.clone();
                }
                out
            }
            Err(e) => json!({ "success": false, "error": message_of(&e) }),
        }
    }

    /// Probe candidate regions for a start URL. The portal displays the region, so this is a
    /// convenience for users whose portal page doesn't, not the primary path.
    pub async fn sso_detect_region(&self, start_url_raw: &str) -> Value {
        let start_url = normalize_start_url(start_url_raw);
        if start_url.is_empty() {
            return json!({ "error": "Enter a start URL first." });
        }
        for region in ["us-east-1", "us-west-2", "us-east-2", "us-west-1"] {
            let org = SsoOrg { start_url: start_url.clone(), region: region.to_string() };
            // A failure is the wrong region for this start URL — try the next.
            let Ok(reg) = self.register_client(&org, &loopback::redirect_uri(loopback::PORT_RANGE.0)).await else { continue };
            let body = json!({ "clientId": reg.client_id, "clientSecret": reg.client_secret, "startUrl": start_url });
            if let Ok(res) = self.request_json(&oidc_host(region), "/device_authorization", Some(&body), &[]).await {
                if (200..300).contains(&res.status) {
                    return json!({ "region": region });
                }
            }
        }
        json!({ "error": "Could not determine the region. Copy it from the access portal instead." })
    }

    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_sso(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let org = || SsoOrg { start_url: normalize_start_url(arg_str(args, 0)), region: arg_str(args, 1).to_string() };
        Some(Ok(match channel {
            "sso:signIn" => self.sso_sign_in(&org()).await,
            "sso:getSessionStatus" => self.sso_session_status(&org()),
            "sso:signOut" => {
                self.sso_sign_out(&org()).await;
                Value::Null
            }
            "sso:detectRegion" => self.sso_detect_region(arg_str(args, 0)).await,
            "sso:listAccounts" => {
                let org = org();
                // Interactive: this is only reached from an explicit user action in the profile
                // form or the import wizard, so opening a sign-in window is expected here.
                match self.sso_access_token(&org, true).await {
                    Ok(None) => json!({ "error": "Sign-in was cancelled." }),
                    Ok(Some(token)) => match self.sso_list_accounts_with_roles(&org, &token).await {
                        Ok(accounts) => json!({ "accounts": accounts }),
                        Err(e) => json!({ "error": message_of(&e) }),
                    },
                    Err(e) => json!({ "error": message_of(&e) }),
                }
            }
            _ => return None,
        }))
    }
}

async fn join_login(mut rx: watch::Receiver<Option<LoginResult>>) -> LoginResult {
    match rx.wait_for(Option::is_some).await {
        Ok(v) => v.clone().unwrap_or_else(|| Err(failure("Sign-in was cancelled."))),
        Err(_) => Err(failure("Sign-in was cancelled.")),
    }
}

/// JavaScript truthiness for a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `new URLSearchParams(pairs).toString()`.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut s = reqwest::Url::parse("http://x/").unwrap();
    {
        let mut q = s.query_pairs_mut();
        for (k, v) in pairs {
            q.append_pair(k, v);
        }
    }
    s.query().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_claims() {
        let claims = B64URL.encode(br#"{"preferred_username":"bob","sub":"x"}"#);
        assert_eq!(identity_from_id_token(&format!("e30.{claims}.")).as_deref(), Some("bob"));
        assert_eq!(identity_from_id_token("nope"), None);
    }

    #[test]
    fn api_error_message() {
        let res = HttpResult { status: 400, json: Some(json!({ "error": "invalid_grant", "error_description": "expired" })), text: String::new() };
        assert_eq!(expect_ok(res, "CreateToken").unwrap_err().message.unwrap(), "CreateToken failed: invalid_grant — expired");
        let res = HttpResult { status: 502, json: None, text: "<html>proxy</html>".into() };
        assert_eq!(expect_ok(res, "ListAccounts").unwrap_err().message.unwrap(), "ListAccounts failed: HTTP 502 — <html>proxy</html>");
    }

    #[test]
    fn query_strings_match_url_search_params() {
        assert_eq!(form(&[("max_result", "100"), ("next_token", "a b+/")]), "max_result=100&next_token=a+b%2B%2F");
    }
}
