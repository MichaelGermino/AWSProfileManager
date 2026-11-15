//! The IdP side of a SAML sign-in: fetch the login form, post the user's credentials, follow the
//! redirects to the page carrying the `SAMLResponse`, and read the roles out of it. Port of 1.x
//! `performLogin` (which itself copied the original Python script's `requests.Session()` flow),
//! plus the best-effort account-name lookup on the AWS SAML sign-in page.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION, REFERER, USER_AGENT};
use scraper::{Html, Selector};

use super::{transport_error, AuthError};
use crate::audit::{idp_host_from_url, mask_username, AuditEntry};
use crate::core::Core;
use crate::saml_roles::{parse_saml_roles, AwsRole};

// Match Python requests.Session() behaviour: follow redirects, keep cookies. Use a User-Agent
// that matches what works with this IdP (the Python script used the requests default; some ADFS
// servers return different content for browsers).
const SESSION_UA: &str = "python-requests/2.31.0";
const SESSION_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

pub const NO_ASSERTION: &str =
    "Response did not contain a valid SAML assertion. Check your username and password, and that the IdP URL is correct.";

/// Who is signing in, for the audit log.
pub struct LoginMeta<'a> {
    pub profile_id: Option<&'a str>,
    pub source: &'a str,
}

pub struct LoginResult {
    pub assertion: String,
    pub roles: Vec<AwsRole>,
}

static INPUT: LazyLock<Selector> = LazyLock::new(|| Selector::parse("input").unwrap());
static ACCOUNT_NAME: LazyLock<Selector> = LazyLock::new(|| Selector::parse(".saml-account-name").unwrap());
static ACCOUNT_TEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Account:\s*([^()]+?)\s*\((\d{12})\)").unwrap());
static ROLE_ACCOUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"arn:aws:iam::(\d+):role/").unwrap());
static LONG_VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"value="[^"]{50,}""#).unwrap());
static PASSWORD_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)password[^>]*").unwrap());

fn err(message: impl Into<String>) -> AuthError {
    AuthError { message: message.into(), info: None }
}

/// `loc.startsWith('http') ? loc : new URL(loc, base).href`
fn resolve(loc: &str, base: &str) -> Result<String, AuthError> {
    if loc.starts_with("http") {
        return Ok(loc.to_string());
    }
    let base = reqwest::Url::parse(base).map_err(|_| err("Invalid URL"))?;
    base.join(loc).map(|u| u.to_string()).map_err(|_| err("Invalid URL"))
}

/// `(name, value)` of every `<input>` in document order (HTML attribute names are
/// case-insensitive, so `INPUT`/`NAME` are covered by the parser).
fn inputs(html: &str) -> Vec<(Option<String>, Option<String>)> {
    let doc = Html::parse_document(html);
    doc.select(&INPUT)
        .map(|el| (el.value().attr("name").map(str::to_string), el.value().attr("value").map(str::to_string)))
        .collect()
}

fn safe_snippet(s: &str, max: usize) -> String {
    let head: String = s.chars().take(max).collect();
    let head = LONG_VALUE.replace_all(&head, "value=\"[REDACTED]\"");
    PASSWORD_ATTR.replace_all(&head, "password=[REDACTED]").into_owned()
}

impl Core {
    /// 1.x `performLogin`.
    pub(crate) async fn perform_login(
        &self,
        idp_entry_url: &str,
        username: &str,
        password: &str,
        meta: Option<LoginMeta<'_>>,
    ) -> Result<LoginResult, AuthError> {
        // A fresh cookie jar per sign-in; redirects are followed by hand so we know the exact URL
        // that serves the form and cookies are set at each step.
        let client = self.cfg.http.client(true, false);
        let get = |url: String| {
            let client = client.clone();
            async move {
                client
                    .get(&url)
                    .header(USER_AGENT, SESSION_UA)
                    .header(ACCEPT, SESSION_ACCEPT)
                    .send()
                    .await
                    .map_err(|e| transport_error(&e, &url))
            }
        };

        let mut current_url = idp_entry_url.to_string();
        let mut form_response = None;
        for _ in 0..10 {
            let res = get(current_url.clone()).await?;
            let status = res.status().as_u16();
            if (200..300).contains(&status) {
                form_response = Some(res);
                break;
            }
            if status == 301 || status == 302 {
                let Some(loc) = res.headers().get(LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string) else {
                    return Err(err("Redirect response missing Location header"));
                };
                current_url = resolve(&loc, &current_url)?;
                form_response = Some(res);
                continue;
            }
            return Err(err(format!("Unexpected response {status} from IdP")));
        }
        let Some(form_response) = form_response else { return Err(err("IdP did not return a login form")) };
        let final_get_url = current_url;
        let form_status = form_response.status().as_u16();
        let html = form_response.text().await.map_err(|e| transport_error(&e, &final_get_url))?;

        // Python: BeautifulSoup(formresponse.text) then find_all(re.compile('(INPUT|input)')).
        let fields = inputs(&html);
        let names: Vec<&str> = fields.iter().filter_map(|(n, _)| n.as_deref().filter(|n| !n.is_empty())).collect();
        eprintln!("[AWS Profile Manager] GET login form: status={form_status}, url={final_get_url}");
        eprintln!("[AWS Profile Manager] Form input names: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") });
        eprintln!("[AWS Profile Manager] Page snippet (first 1200 chars):\n{}", safe_snippet(&html, 1200));

        // Build the payload like Python's dict: duplicate names overwrite (last wins) but keep
        // the position of the first.
        let mut payload: Vec<(String, String)> = Vec::new();
        for (name, value) in fields {
            let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
            let n = name.to_lowercase();
            let v = if n.contains("user") || n.contains("email") {
                username.to_string()
            } else if n.contains("pass") {
                password.to_string()
            } else {
                value.unwrap_or_default()
            };
            match payload.iter_mut().find(|(k, _)| *k == name) {
                Some(slot) => slot.1 = v,
                None => payload.push((name, v)),
            }
        }

        eprintln!("[AWS Profile Manager] POST to: {final_get_url}");
        eprintln!(
            "[AWS Profile Manager] Payload keys: {}",
            payload.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
        );
        if let Some(m) = &meta {
            let mut e = AuditEntry::new("idp_request", m.source);
            if let Some(pid) = m.profile_id {
                e = e.with("profileId", pid);
            }
            self.append_auth_audit(
                e.with("idpHost", idp_host_from_url(&final_get_url)).with("usernameHint", mask_username(username)),
            );
        }
        let idp_post_started_at = self.cfg.clock.now_ms();
        let mut saml_page = client
            .post(&final_get_url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(REFERER, &final_get_url)
            .header(USER_AGENT, SESSION_UA)
            .header(ACCEPT, SESSION_ACCEPT)
            .body(serde_urlencoded_body(&payload))
            .send()
            .await
            .map_err(|e| transport_error(&e, &final_get_url))?;

        // ADFS often returns 302 after login; the SAML response is on the redirected page.
        let mut redirect_url = final_get_url.clone();
        for _ in 0..5 {
            let status = saml_page.status().as_u16();
            if status == 301 || status == 302 {
                let Some(loc) = saml_page.headers().get(LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string) else {
                    break;
                };
                redirect_url = resolve(&loc, &redirect_url)?;
                saml_page = get(redirect_url.clone()).await?;
                continue;
            }
            break;
        }

        let status = saml_page.status().as_u16();
        let headers = format!("{:?}", saml_page.headers());
        let post_html = saml_page.text().await.map_err(|e| transport_error(&e, &redirect_url))?;
        let post_inputs = inputs(&post_html);
        let mut assertion = String::new();
        for (name, value) in &post_inputs {
            if name.as_deref() == Some("SAMLResponse") {
                assertion = value.clone().unwrap_or_default();
            }
        }
        if assertion.is_empty() {
            for (name, value) in &post_inputs {
                if name.as_deref().unwrap_or("").to_lowercase() == "samlresponse" {
                    assertion = value.clone().unwrap_or_default();
                }
            }
        }

        if assertion.is_empty() {
            let len = post_html.encode_utf16().count();
            let has_in_body = post_html.to_lowercase().contains("samlresponse");
            eprintln!(
                "[AWS Profile Manager] No SAML assertion. Final response status={status}, body length={len}, body contains \"SAMLResponse\"={has_in_body}"
            );
            eprintln!("[AWS Profile Manager] Response headers: {headers}");
            let body: String =
                if len <= 3000 { post_html.clone() } else { format!("{}\n... [truncated]", post_html.chars().take(1500).collect::<String>()) };
            eprintln!("[AWS Profile Manager] Response body:\n{body}");
            return Err(err(NO_ASSERTION));
        }

        let roles = parse_saml_roles(&assertion);
        if roles.is_empty() {
            return Err(err("No roles were found in the SAML assertion"));
        }

        if let Some(m) = &meta {
            let mut e = AuditEntry::new("idp_success", m.source);
            if let Some(pid) = m.profile_id {
                e = e.with("profileId", pid);
            }
            self.append_auth_audit(
                e.with("idpHost", idp_host_from_url(&final_get_url))
                    .with("usernameHint", mask_username(username))
                    .with("roleCount", roles.len())
                    .with("durationMs", self.cfg.clock.now_ms() - idp_post_started_at),
            );
        }

        // We only receive the SAML post form (no role-picker HTML). Friendly names come from
        // Settings → Account display names, or from `apply_account_names` when listing roles.
        Ok(LoginResult { assertion, roles })
    }

    /// Ask AWS for the friendly account names behind a SAML assertion.
    ///
    /// The assertion carries only role/principal ARNs — no names — which is why imported SAML
    /// profiles showed bare account numbers. AWS's role-selection page renders
    /// "Account: <name> (<id>)" for every account in the assertion, so POSTing the assertion there
    /// and parsing the result names them all in one request.
    ///
    /// Best-effort by design: used only when listing roles for the import picker, never on the
    /// refresh path, and any failure simply leaves the names blank rather than breaking role
    /// discovery. Keeping it off the refresh path also avoids spending the assertion anywhere near
    /// AssumeRoleWithSAML.
    async fn fetch_saml_account_names(&self, assertion: &str) -> HashMap<String, String> {
        let mut names = HashMap::new();
        let client = self.cfg.http.client(false, true);
        let sent = client
            .post(self.cfg.http.remap("https://signin.aws.amazon.com/saml"))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(USER_AGENT, SESSION_UA)
            .header(ACCEPT, SESSION_ACCEPT)
            .body(serde_urlencoded_body(&[("SAMLResponse".to_string(), assertion.to_string())]))
            .timeout(Duration::from_secs(20))
            .send()
            .await;
        let Ok(res) = sent else { return names };
        let Ok(body) = res.text().await else { return names };
        // axios parsed a JSON body into an object, which 1.x then ignored.
        if serde_json::from_str::<serde_json::Value>(&body).is_ok_and(|v| !v.is_string()) {
            return names;
        }
        let doc = Html::parse_document(&body);
        let mut texts: Vec<String> = doc.select(&ACCOUNT_NAME).map(|el| el.text().collect()).collect();
        // Fall back to the whole document when the markup differs from the standard page.
        if texts.is_empty() {
            texts.push(doc.root_element().text().collect());
        }
        for text in texts {
            for m in ACCOUNT_TEXT.captures_iter(&text) {
                let name = crate::js::trim(&m[1]);
                if !name.is_empty() {
                    names.insert(m[2].to_string(), name.to_string());
                }
            }
        }
        names
    }

    /// Attach friendly account names in place. Used by both role-listing paths, never by refresh.
    pub(crate) async fn apply_account_names(&self, assertion: &str, roles: &mut [AwsRole]) {
        let names = self.fetch_saml_account_names(assertion).await;
        if names.is_empty() {
            return;
        }
        for role in roles {
            let id = ROLE_ACCOUNT.captures(&role.role_arn).map(|c| c[1].to_string());
            if let Some(name) = id.and_then(|id| names.get(&id)) {
                role.account_name = Some(name.clone());
            }
        }
    }
}

/// `new URLSearchParams(obj).toString()`.
fn serde_urlencoded_body(pairs: &[(String, String)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, v);
    }
    s.finish()
}
