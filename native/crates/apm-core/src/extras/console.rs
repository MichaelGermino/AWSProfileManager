//! One-click AWS console sign-in for a profile. Port of `consoleSignIn.ts`.
//!
//! Uses AWS's documented federation endpoint: exchange a set of temporary credentials for a
//! SigninToken, then build a console login URL from it. Deliberately identical for SAML and
//! Identity Center profiles — by the time credentials reach ~/.aws/credentials the two are
//! indistinguishable, so there is no IdP round-trip and no dependency on which identity the
//! user's browser happens to be signed into.
//!
//! Security: the SigninToken is a bearer credential that grants console access as the role. It is
//! built and consumed entirely in the core, is never sent to the window, and must never be logged.
//! The window only ever learns success or a non-sensitive error message.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::{browsers, encode_uri_component, failure};
use crate::core::{Core, CoreConfig, DispatchResult};
use crate::host::Host;
use crate::store::{self, Obj};
use crate::{env, js};

/// Refresh rather than federate when credentials are this close to expiring.
const MIN_REMAINING_MS: i64 = 2 * 60_000;

/// How long to give the opt-in redirect before assuming it failed and opening the console directly.
const FIRST_OPEN_FALLBACK: Duration = Duration::from_secs(6);

/// Regional signin host, not the global `signin.aws.amazon.com`.
///
/// Both mint sign-in tokens, but only the regional host is accepted as a `redirect_uri` by the
/// multi-session opt-in endpoint — the global one is rejected with HTTP 400. Using it everywhere
/// keeps token minting and the login URL on the same host.
fn federation_host(region: &str) -> String {
    format!("{region}.signin.aws.amazon.com")
}

/// Console landing page.
///
/// Deliberately the SHARED regional host, not the account-scoped multi-session subdomain
/// (`<accountId>.<region>.console.aws.amazon.com`). That subdomain exists and serves in a browser,
/// but the federation endpoint rejects it as a Destination with HTTP 400 — you cannot federate
/// straight into a session slot. AWS assigns the subdomain itself once multi-session is on.
fn console_destination(region: &str) -> String {
    format!("https://{region}.console.aws.amazon.com/console/home?region={}", encode_uri_component(region))
}

fn build_login_url(region: &str, signin_token: &str) -> String {
    format!(
        "https://{}/federation?Action=login&Issuer={}&Destination={}&SigninToken={}",
        federation_host(region),
        encode_uri_component("AWSProfileManager"),
        encode_uri_component(&console_destination(region)),
        encode_uri_component(signin_token)
    )
}

fn str_of<'a>(o: &'a Obj, k: &str) -> Option<&'a str> {
    o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `session` is the JSON credential blob; it and the returned token are bearer secrets — never
/// log either.
async fn mint_signin_token(cfg: &CoreConfig, region: &str, session: &str) -> Result<String, String> {
    let url = format!(
        "https://{}/federation?Action=getSigninToken&Session={}",
        federation_host(region),
        encode_uri_component(session)
    );
    // The first-open fallback fires 6 s after the first token request, by which time the server
    // has usually dropped the idle kept-alive socket; the shared client retries such a request
    // (1.x staleSocket.ts — without it the fallback console tab never opened).
    let res = cfg
        .http
        .client(false, true)
        .get(cfg.http.remap(&url))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Could not reach AWS sign-in: {e}"))?;
    let status = res.status().as_u16();
    if status != 200 {
        // Body can echo credential material; never surface or log it.
        return Err(format!("AWS rejected the sign-in request (HTTP {status})."));
    }
    let body = res.text().await.map_err(|e| format!("Could not reach AWS sign-in: {e}"))?;
    let parsed: Value = serde_json::from_str(&body).map_err(|e| format!("Could not reach AWS sign-in: {e}"))?;
    match parsed.get("SigninToken").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        Some(t) => Ok(t.to_string()),
        None => Err("AWS did not return a sign-in token.".into()),
    }
}

/// The AWS console allows one session per browser profile, so opening account B in the default
/// browser silently signs you out of account A. An in-app window on a per-profile storage
/// partition keeps each account's console session independent, which matters for a multi-account
/// tool. settings.consoleBrowserMode = 'external' (the default) uses a browser instead.
fn open_console_url(cfg: &CoreConfig, host: &dyn Host, profile: &Obj, login_url: &str) {
    let settings = store::Settings { paths: &cfg.paths, platform: cfg.platform }.get();
    let mode = settings.get("consoleBrowserMode").and_then(Value::as_str).unwrap_or("external");
    if mode == "external" {
        let _ = browsers::open_in_browser(host, cfg.platform, login_url, settings.get("consoleBrowser").and_then(Value::as_str));
        return;
    }
    let id = str_of(profile, "id").unwrap_or("");
    let name = profile.get("name").and_then(Value::as_str).unwrap_or("");
    host.open_console_window(id, &format!("AWS Console — {name}"), login_url);
}

/// Guarantee the console opens on the one attempt that routes through multi-session opt-in.
///
/// Whether opt-in is needed is a property of the *browser*, which the app cannot observe: the flag
/// is app-side state describing browser-side state, so a reinstall, a second machine or a
/// different default browser desyncs them. Guess wrong and AWS answers with a bare HTTP 400 and the
/// user is stranded on an error page with no idea that simply clicking again would work.
///
/// Detection is not available. `redirect_uri` is validated against an allowlist of AWS signin and
/// console hosts — a loopback URL we could serve ourselves is rejected with 400 — so there is no
/// callback to observe and no way to learn the outcome.
///
/// So don't try to detect: open the console unconditionally a few seconds later. Opt-in worked and
/// the user gets a harmless duplicate tab; it failed and they get a working console beside the
/// error. Either way nobody hits a dead end, and it costs one extra tab exactly once per install.
fn schedule_first_open_fallback(cfg: CoreConfig, host: Arc<dyn Host>, profile: Obj, region: String, session: String) {
    // Embedded mode would mean a second WINDOW rather than a background tab, which is worse than
    // the problem. It is also the mode where the user can simply close the window and click again.
    let settings = store::Settings { paths: &cfg.paths, platform: cfg.platform }.get();
    if settings.get("consoleBrowserMode").and_then(Value::as_str).unwrap_or("external") != "external" {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_OPEN_FALLBACK).await;
        // A fresh token, not the one already in the opt-in URL: sign-in tokens are single-use as
        // far as we know, so reusing that URL would risk the fallback tab failing in the success case.
        match mint_signin_token(&cfg, &region, &session).await {
            Ok(token) => open_console_url(&cfg, host.as_ref(), &profile, &build_login_url(&region, &token)),
            Err(e) => eprintln!("[console] multi-session fallback could not mint a sign-in token: {e}"),
        }
    });
}

impl Core {
    /// `auth:refresh` (1.x `refreshProfile(id, undefined, { interactive: true })`), through
    /// `dispatch` so the console path uses exactly what the auth area does.
    fn refresh_for_console<'a>(&'a self, args: &'a [Value]) -> Pin<Box<dyn Future<Output = DispatchResult> + Send + 'a>> {
        Box::pin(self.dispatch("auth:refresh", args))
    }

    /// `openConsoleForProfile`. Refresh is interactive: an Identity Center profile whose SSO
    /// session has lapsed can prompt for sign-in — this is always a direct user action.
    pub async fn open_console_for_profile(&self, profile_id: &str) -> DispatchResult {
        let Some(profile) = self.profiles().by_id(profile_id) else { return Ok(failure("Profile not found.")) };

        let section_name = store::section_name_of(&profile);
        if section_name.is_empty() {
            return Ok(failure("Profile has no credentials section name."));
        }

        // Federating with expired credentials just yields an error from AWS, so refresh first.
        let expires_at = str_of(&profile, "expiration").and_then(env::parse_js_date_ms).unwrap_or(0);
        let needs_refresh = expires_at == 0 || expires_at - self.cfg.clock.now_ms() < MIN_REMAINING_MS;
        if needs_refresh {
            let args = [Value::String(profile_id.to_string())];
            let result = self.refresh_for_console(&args).await?;
            if result.get("ssoLoginRequired").is_some() {
                return Ok(failure("Sign in to AWS first, then try again."));
            }
            if result.get("required").is_some() {
                return Ok(failure("Credentials are required for this profile. Refresh it first."));
            }
            if result.get("success") == Some(&Value::Bool(false)) {
                let error = result.get("error").and_then(Value::as_str).unwrap_or("").to_string();
                return Ok(failure(error));
            }
        }

        let sections = self.credentials.read();
        let section = crate::credentials_ini::get(&sections, &section_name);
        let value = |k: &str| {
            section.and_then(|s| s.iter().find(|(key, _)| key == k)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
        };
        let (Some(key_id), Some(secret), Some(token)) =
            (value("aws_access_key_id"), value("aws_secret_access_key"), value("aws_session_token"))
        else {
            return Ok(failure(format!(
                "No temporary credentials found under \"{section_name}\". Refresh the profile and try again."
            )));
        };

        // Region for the console landing page: the profile's SSO region, else what was written to the file.
        let region = js::trim(str_of(&profile, "ssoRegion").or(value("region")).unwrap_or("us-west-2")).to_string();

        // Note: no SessionDuration parameter. It is not valid with AssumeRole-derived credentials;
        // the console session simply inherits the lifetime of the credentials themselves.
        let session = crate::json::stringify(&json!({ "sessionId": key_id, "sessionKey": secret, "sessionToken": token }));

        let signin_token = match mint_signin_token(&self.cfg, &region, &session).await {
            Ok(t) => t,
            Err(e) => return Ok(failure(e)),
        };
        let login_url = build_login_url(&region, &signin_token);

        // Multi-session opt-in runs ONCE, not on every sign-in.
        //
        // Multi-session is what lets several accounts be signed in at the same browser; without it
        // the second profile is refused with "You must first log out before logging into a
        // different AWS account". But calling /sessions/v1/opt-in again when the browser is
        // already opted in with a live session returns HTTP 400 — so chaining it every time breaks
        // the second profile, which is exactly the case it exists to fix.
        //
        // Running it on first use keeps this invisible for a fresh browser. The redirect_uri must
        // be on the regional signin host; the global one returns HTTP 400.
        let mut settings = self.settings().get();
        if settings.get("consoleMultiSessionOptInDone") != Some(&Value::Bool(true)) {
            let opt_in_url = format!(
                "https://{}/sessions/v1/opt-in?redirect_uri={}",
                federation_host(&region),
                encode_uri_component(&login_url)
            );
            settings.insert("consoleMultiSessionOptInDone".into(), true.into());
            self.settings().save(&settings);
            open_console_url(&self.cfg, self.host.as_ref(), &profile, &opt_in_url);
            schedule_first_open_fallback(self.cfg.clone(), self.host.clone(), profile, region, session);
            return Ok(json!({ "success": true }));
        }

        open_console_url(&self.cfg, self.host.as_ref(), &profile, &login_url);
        Ok(json!({ "success": true }))
    }

    /// `openMultiSessionOptIn`: re-run multi-session opt-in on demand.
    ///
    /// The automatic attempt happens once per install, but whether it is *needed* is browser state
    /// the app cannot see. A new machine, a different default browser, or the user clearing cookies
    /// all leave the app thinking it is done when the browser has forgotten — and the symptom shows
    /// up much later as "You must first log out before logging into a different AWS account" on the
    /// second account. Without this the only cure is hand-editing settings.json.
    ///
    /// Opens the opt-in URL alone: nothing is chained to it, so a 400 from an already-opted-in
    /// browser is just an error page rather than a blocked console.
    pub fn open_multi_session_opt_in(&self) -> Value {
        let settings = self.settings().get();
        let region = js::trim(str_of(&settings, "defaultSsoRegion").unwrap_or("us-west-2")).to_string();
        // redirect_uri is allowlisted to AWS signin/console hosts, so send them to the console home.
        let url = format!(
            "https://{}/sessions/v1/opt-in?redirect_uri={}",
            federation_host(&region),
            encode_uri_component(&console_destination(&region))
        );
        let _ = browsers::open_in_browser(
            self.host.as_ref(),
            self.cfg.platform,
            &url,
            settings.get("consoleBrowser").and_then(Value::as_str),
        );
        json!({ "success": true })
    }
}
