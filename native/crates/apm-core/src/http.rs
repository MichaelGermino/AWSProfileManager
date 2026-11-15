//! The one HTTP client setup every network call uses. Port of `enterpriseTls.ts`:
//! - trust: the OS certificate store (corporate TLS-inspection roots included), plus the test CA
//!   in test mode;
//! - 120 s connect timeout;
//! - `User-Agent: AWSProfileManager/<version>`;
//! - in test mode, real AWS / GitHub hosts are sent to the fake services (APM_HOST_MAP in 1.x).
//!
//! A request that failed because an idle kept-alive connection was closed is retried by hyper's
//! pool before anything reached the server (the 1.x `staleSocket.ts` fix).

use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct HttpConfig {
    /// Real host (lowercase) -> fake base URL. Empty outside tests.
    pub host_map: HashMap<String, String>,
    /// PEM of an extra root CA (the fakes' test CA). `None` outside tests.
    pub extra_ca_pem: Option<String>,
    pub user_agent: String,
}

impl HttpConfig {
    /// Where a request for `url` really goes: the fake for a mapped host, else `url` unchanged.
    pub fn remap(&self, url: &str) -> String {
        if self.host_map.is_empty() {
            return url.to_string();
        }
        let Ok(parsed) = reqwest::Url::parse(url) else { return url.to_string() };
        let host = match (parsed.host_str(), parsed.port()) {
            (Some(h), Some(p)) => format!("{}:{p}", h.to_lowercase()),
            (Some(h), None) => h.to_lowercase(),
            _ => return url.to_string(),
        };
        let Some(base) = self.host_map.get(&host) else { return url.to_string() };
        let mut out = base.trim_end_matches('/').to_string();
        out.push_str(parsed.path());
        if let Some(q) = parsed.query() {
            out.push('?');
            out.push_str(q);
        }
        if let Some(f) = parsed.fragment() {
            out.push('#');
            out.push_str(f);
        }
        out
    }

    /// A client with the shared settings. `cookies`: keep a cookie jar (SAML sign-in flows).
    /// `redirects`: follow redirects automatically.
    pub fn client(&self, cookies: bool, redirects: bool) -> reqwest::Client {
        let mut b = reqwest::Client::builder()
            .user_agent(self.user_agent.clone())
            .connect_timeout(Duration::from_secs(120))
            .cookie_store(cookies)
            .redirect(if redirects { reqwest::redirect::Policy::limited(20) } else { reqwest::redirect::Policy::none() });
        if let Some(pem) = &self.extra_ca_pem {
            if let Ok(cert) = reqwest::Certificate::from_pem(pem.as_bytes()) {
                b = b.add_root_certificate(cert);
            }
        }
        b.build().expect("HTTP client configuration is valid")
    }
}

/// What the network classifier needs from a reqwest failure.
pub fn error_info(err: &reqwest::Error) -> crate::network_classifier::ErrorInfo {
    use crate::network_classifier::ErrorInfo;
    let code = if err.is_timeout() {
        Some("ETIMEDOUT".to_string())
    } else if err.is_connect() {
        Some("ECONNREFUSED".to_string())
    } else {
        None
    };
    ErrorInfo {
        message: Some(err.to_string()),
        code,
        no_response: err.status().is_none() && (err.is_connect() || err.is_timeout() || err.is_request()),
        ..Default::default()
    }
}
