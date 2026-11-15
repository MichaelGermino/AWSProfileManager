//! STS `AssumeRoleWithSAML` as a plain HTTPS POST (the call is unauthenticated: the SAML
//! assertion is the credential, so no SigV4 and no AWS SDK). Mirrors what 1.x's
//! `@aws-sdk/client-sts` did: awsQuery form body, XML response, the SDK's error shape
//! (`name` = AWS error code, `$metadata.httpStatusCode`, `$metadata.requestId`) and its standard
//! retry policy (3 attempts for transient and throttling failures).

use std::time::Duration;

use rand::Rng;

use super::{transport_error, AuthError};
use crate::env::parse_js_date_ms;
use crate::http::HttpConfig;

pub const REGION: &str = "us-west-2";
const STS_URL: &str = "https://sts.us-west-2.amazonaws.com/";
const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct StsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    /// Epoch ms, when STS returned one.
    pub expiration_ms: Option<i64>,
}

/// An AWS service error the way the SDK surfaced it.
#[derive(Debug, Clone)]
pub struct ServiceError {
    /// The AWS error code (`InvalidIdentityToken`), else the HTTP status, else `UnknownError`.
    pub name: String,
    /// `<Message>`, else `UnknownError`.
    pub message: String,
    pub status: u16,
    pub request_id: Option<String>,
}

/// Port of 1.x `describeAuthError` for SDK service errors. STS in particular returns a literal
/// `<Message>Unknown</Message>` body for `503 ServiceUnavailable`, so the message alone rendered
/// as the uninformative "Error: Unknown". Name the actual fault and carry the AWS request id, so
/// the audit log records something you can act on or quote to AWS.
pub fn describe_service_error(e: &ServiceError) -> String {
    let code = if !e.name.is_empty() && e.name != "Error" { e.name.clone() } else { format!("HTTP {}", e.status) };
    let detail = if crate::js::trim(&e.message).to_lowercase() != "unknown" && !e.message.is_empty() {
        format!(": {}", e.message)
    } else {
        String::new()
    };
    let request_id = e.request_id.as_ref().map(|r| format!(" [requestId {r}]")).unwrap_or_default();
    let hint = if e.status >= 500 || e.status == 429 {
        " AWS was temporarily unable to serve the request. Your sign-in worked — try refreshing again."
    } else {
        ""
    };
    format!("AWS STS {code} (HTTP {}){detail}{request_id}.{hint}", e.status)
}

/// Throttling codes the SDK's standard retry strategy retries.
const THROTTLING_CODES: &[&str] = &[
    "BandwidthLimitExceeded",
    "EC2ThrottledException",
    "LimitExceededException",
    "PriorRequestNotComplete",
    "ProvisionedThroughputExceededException",
    "RequestLimitExceeded",
    "RequestThrottled",
    "RequestThrottledException",
    "SlowDown",
    "ThrottledException",
    "Throttling",
    "ThrottlingException",
    "TooManyRequestsException",
    "TransactionInProgressException",
];

fn retryable(e: &ServiceError) -> bool {
    matches!(e.status, 500 | 502 | 503 | 504 | 429)
        || THROTTLING_CODES.contains(&e.name.as_str())
        || ["TimeoutError", "RequestTimeout", "RequestTimeoutException"].contains(&e.name.as_str())
}

fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.descendants().find(|n| n.is_element() && n.tag_name().name() == name).and_then(|n| n.text())
}

fn parse_error(status: u16, header_request_id: Option<String>, body: &str) -> ServiceError {
    let doc = roxmltree::Document::parse(body).ok();
    let root = doc.as_ref().map(|d| d.root_element());
    let error = root.and_then(|r| r.descendants().find(|n| n.is_element() && n.tag_name().name() == "Error"));
    let code = error.and_then(|e| child_text(e, "Code")).map(str::to_string).filter(|c| !c.is_empty());
    let message = error.and_then(|e| child_text(e, "Message")).map(str::to_string).filter(|m| !m.is_empty());
    // smithy: name = Code || ("NotFound" for a bare 404) || status || "UnknownError".
    let name = code.unwrap_or_else(|| if status == 404 { "NotFound".into() } else { status.to_string() });
    let request_id = header_request_id.or_else(|| root.and_then(|r| child_text(r, "RequestId")).map(str::to_string));
    ServiceError { name, message: message.unwrap_or_else(|| "UnknownError".into()), status, request_id }
}

/// `Ok(None)`: STS answered 200 without usable credentials ("STS did not return credentials").
fn parse_success(body: &str) -> Option<StsCredentials> {
    let doc = roxmltree::Document::parse(body).ok()?;
    let creds = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == "Credentials")?;
    let field = |n: &str| child_text(creds, n).map(|s| crate::js::trim(s).to_string()).filter(|s| !s.is_empty());
    Some(StsCredentials {
        access_key_id: field("AccessKeyId")?,
        secret_access_key: field("SecretAccessKey")?,
        session_token: field("SessionToken")?,
        expiration_ms: field("Expiration").and_then(|e| parse_js_date_ms(&e)),
    })
}

/// The outcome of one AssumeRoleWithSAML call.
pub enum StsResult {
    Ok(StsCredentials),
    /// 200 without the three credential fields.
    NoCredentials,
}

pub async fn assume_role_with_saml(
    http: &HttpConfig,
    role_arn: &str,
    principal_arn: &str,
    assertion: &str,
    duration_seconds: i64,
) -> Result<StsResult, AuthError> {
    let client = http.client(false, false);
    let url = http.remap(STS_URL);
    let duration = duration_seconds.to_string();
    let form = [
        ("RoleArn", role_arn),
        ("PrincipalArn", principal_arn),
        ("SAMLAssertion", assertion),
        ("DurationSeconds", duration.as_str()),
        ("Action", "AssumeRoleWithSAML"),
        ("Version", "2011-06-15"),
    ];
    let mut attempt = 0;
    loop {
        attempt += 1;
        let sent = client.post(&url).form(&form).send().await;
        let failure = match sent {
            Err(e) => {
                let err = transport_error(&e, STS_URL);
                if attempt < MAX_ATTEMPTS {
                    backoff(attempt).await;
                    continue;
                }
                return Err(err);
            }
            Ok(res) => {
                let status = res.status().as_u16();
                let header_id = ["x-amzn-requestid", "x-amzn-request-id", "x-amz-request-id"]
                    .iter()
                    .find_map(|h| res.headers().get(*h).and_then(|v| v.to_str().ok()).map(str::to_string));
                let body = match res.text().await {
                    Ok(b) => b,
                    Err(e) => return Err(transport_error(&e, STS_URL)),
                };
                if (200..300).contains(&status) {
                    return Ok(match parse_success(&body) {
                        Some(c) => StsResult::Ok(c),
                        None => StsResult::NoCredentials,
                    });
                }
                parse_error(status, header_id, &body)
            }
        };
        if attempt < MAX_ATTEMPTS && retryable(&failure) {
            backoff(attempt).await;
            continue;
        }
        return Err(AuthError {
            message: describe_service_error(&failure),
            info: Some(crate::network_classifier::ErrorInfo { message: Some(failure.message.clone()), ..Default::default() }),
        });
    }
}

/// Full-jitter exponential backoff (SDK default base 100 ms).
async fn backoff(attempt: u32) {
    let cap = 100u64 * (1 << attempt.min(8));
    let ms = rand::thread_rng().gen_range(0..=cap);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_errors_name_the_fault_and_request_id() {
        let body = "<ErrorResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><Error><Type>Sender</Type><Code>InvalidIdentityToken</Code><Message>Fake InvalidIdentityToken</Message></Error><RequestId>fake-request-id</RequestId></ErrorResponse>";
        let e = parse_error(400, Some("fake-request-id".into()), body);
        assert_eq!(
            describe_service_error(&e),
            "AWS STS InvalidIdentityToken (HTTP 400): Fake InvalidIdentityToken [requestId fake-request-id]."
        );
        // STS's literal "Unknown" message is dropped; 5xx gets the try-again hint.
        let body = "<ErrorResponse><Error><Code>ServiceUnavailable</Code><Message>Unknown</Message></Error></ErrorResponse>";
        let e = parse_error(503, None, body);
        assert_eq!(
            describe_service_error(&e),
            "AWS STS ServiceUnavailable (HTTP 503). AWS was temporarily unable to serve the request. Your sign-in worked — try refreshing again."
        );
        // An empty error body: the SDK named it after the status.
        let e = parse_error(502, Some("r1".into()), "<ErrorResponse/>");
        assert_eq!(e.name, "502");
        assert_eq!(e.message, "UnknownError");
    }

    #[test]
    fn success_body_yields_credentials() {
        let body = "<AssumeRoleWithSAMLResponse><AssumeRoleWithSAMLResult><Credentials><AccessKeyId>A</AccessKeyId><SecretAccessKey>S</SecretAccessKey><SessionToken>T</SessionToken><Expiration>2030-01-01T00:00:00Z</Expiration></Credentials></AssumeRoleWithSAMLResult></AssumeRoleWithSAMLResponse>";
        let c = parse_success(body).unwrap();
        assert_eq!((c.access_key_id.as_str(), c.expiration_ms), ("A", Some(1_893_456_000_000)));
        assert!(parse_success("<AssumeRoleWithSAMLResponse/>").is_none());
    }
}
