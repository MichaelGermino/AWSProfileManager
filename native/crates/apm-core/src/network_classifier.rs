//! Network vs auth failure. Port of `src/main/services/networkErrorClassifier.ts`.
//!
//! Network failures (DNS, offline, refused, timeout) never reached the IdP, so they must not count
//! toward the two-strike auto-refresh pause: no login attempt was made, so no lockout risk.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Network,
    Auth,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FailureKind::Network => "network",
            FailureKind::Auth => "auth",
        }
    }
}

const NETWORK_CODES: &[&str] = &["ENOTFOUND", "EAI_AGAIN", "ETIMEDOUT", "ECONNREFUSED", "ECONNRESET", "ENETUNREACH", "ENETDOWN", "EHOSTUNREACH", "ERR_NETWORK"];

static NETWORK_MESSAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(getaddrinfo|ENOTFOUND|EAI_AGAIN|ETIMEDOUT|ECONNREFUSED|ECONNRESET|ENETUNREACH|ENETDOWN|EHOSTUNREACH|socket hang up|network error|network is unreachable|dns lookup failed)",
    )
    .unwrap()
});

/// What the classifier knows about one error in a cause chain. The HTTP layer fills this in from
/// its own error types: `code` for transport errors, `no_response` when a request got no reply.
#[derive(Debug, Clone, Default)]
pub struct ErrorInfo {
    pub message: Option<String>,
    pub code: Option<String>,
    pub errno: Option<String>,
    /// True for an HTTP client error that never got a response (axios `isAxiosError` without
    /// `response` in 1.x).
    pub no_response: bool,
    pub cause: Option<Box<ErrorInfo>>,
}

fn is_network_code(c: &Option<String>) -> bool {
    c.as_deref().is_some_and(|c| NETWORK_CODES.contains(&c))
}

pub fn classify(err: Option<&ErrorInfo>) -> FailureKind {
    let Some(err) = err else { return FailureKind::Auth };
    let mut current = Some(err);
    for _ in 0..5 {
        let Some(e) = current else { break };
        if is_network_code(&e.code) || is_network_code(&e.errno) || e.no_response {
            return FailureKind::Network;
        }
        current = e.cause.as_deref();
    }
    match &err.message {
        Some(m) if NETWORK_MESSAGE.is_match(m) => FailureKind::Network,
        _ => FailureKind::Auth,
    }
}

/// Classify a bare message (1.x accepted stringified errors).
pub fn classify_message(message: &str) -> FailureKind {
    classify(Some(&ErrorInfo { message: Some(message.to_string()), ..Default::default() }))
}
