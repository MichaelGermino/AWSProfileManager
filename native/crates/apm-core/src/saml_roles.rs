//! SAML assertion to AWS roles. Port of `src/main/services/samlRoles.ts`.
//!
//! Read namespace-agnostically (element prefixes `saml:`, `saml2:`, `samlp:` or none are ignored),
//! so every IdP's layout takes the same path. XML that does not parse falls back to scanning the
//! text for "role ARN, provider ARN" pairs.

use std::sync::LazyLock;

use base64::Engine;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::js;

pub const SAML_ATTR_ROLE: &str = "https://aws.amazon.com/SAML/Attributes/Role";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsRole {
    pub role_arn: String,
    pub principal_arn: String,
    pub display_text: String,
    /// Friendly account name scraped from the IdP page, when it shows one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub account_name: Option<String>,
}

/// "role ARN, provider ARN" in either order.
pub fn normalize_role_value(value: &str) -> Option<AwsRole> {
    let chunks: Vec<&str> = value.split(',').map(js::trim).collect();
    if chunks.len() != 2 {
        return None;
    }
    let (role_arn, principal_arn) =
        if chunks[0].contains("saml-provider") { (chunks[1], chunks[0]) } else { (chunks[0], chunks[1]) };
    if role_arn.is_empty() || principal_arn.is_empty() || !role_arn.starts_with("arn:") || !principal_arn.starts_with("arn:") {
        return None;
    }
    Some(AwsRole {
        role_arn: role_arn.to_string(),
        principal_arn: principal_arn.to_string(),
        display_text: role_arn.to_string(),
        account_name: None,
    })
}

/// First occurrence of each role ARN, in document order.
fn dedupe(roles: Vec<AwsRole>) -> Vec<AwsRole> {
    let mut seen = std::collections::HashSet::new();
    roles.into_iter().filter(|r| seen.insert(r.role_arn.clone())).collect()
}

static ARN_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(arn:aws:iam::\d+:role/[^,\s<>"&]+\s*,\s*arn:aws:iam::\d+:saml-provider/[^<\s"&]+|arn:aws:iam::\d+:saml-provider/[^,\s<>"&]+\s*,\s*arn:aws:iam::\d+:role/[^<\s"&]+)"#,
    )
    .unwrap()
});

/// Last resort for XML that does not parse.
pub fn extract_roles_from_text(xml: &str) -> Vec<AwsRole> {
    dedupe(ARN_PAIR.captures_iter(xml).filter_map(|c| normalize_role_value(&c[1])).collect())
}

fn children<'a, 'i>(node: roxmltree::Node<'a, 'i>, local: &'a str) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> + 'a {
    node.children().filter(move |c| c.is_element() && c.tag_name().name() == local)
}

/// The element's own text (xml2js `_` / text-only string): its direct text and CDATA children.
fn own_text(node: roxmltree::Node) -> String {
    node.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect()
}

pub fn parse_saml_roles_xml(xml: &str) -> Vec<AwsRole> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = match roxmltree::Document::parse_with_options(xml, opts) {
        Ok(d) => d,
        Err(_) => return extract_roles_from_text(xml),
    };
    let root = doc.root_element();
    let assertions: Vec<roxmltree::Node> = match root.tag_name().name() {
        "Assertion" => vec![root],
        "Response" => children(root, "Assertion").collect(),
        _ => Vec::new(),
    };
    let mut roles = Vec::new();
    for assertion in assertions {
        for statement in children(assertion, "AttributeStatement") {
            for attr in children(statement, "Attribute") {
                let name = attr.attribute("Name").or_else(|| attr.attribute("name"));
                if name != Some(SAML_ATTR_ROLE) {
                    continue;
                }
                for value in children(attr, "AttributeValue") {
                    if let Some(r) = normalize_role_value(js::trim(&own_text(value))) {
                        roles.push(r);
                    }
                }
            }
        }
    }
    dedupe(roles)
}

/// Lenient base64 like Node's `Buffer.from(s, 'base64')`: ignores characters outside the
/// alphabet and missing padding; invalid UTF-8 becomes U+FFFD.
pub fn decode_assertion(assertion_base64: &str) -> String {
    let cleaned: String = assertion_base64
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '-' | '_'))
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(cleaned.trim_end_matches('='))
        .or_else(|_| {
            // A dangling single character carries no full byte; Node drops it.
            let n = cleaned.len() - cleaned.len() % 4;
            let keep = if cleaned.len() % 4 == 1 { n } else { cleaned.len() };
            base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned[..keep])
        })
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn parse_saml_roles(assertion_base64: &str) -> Vec<AwsRole> {
    parse_saml_roles_xml(&decode_assertion(assertion_base64))
}
