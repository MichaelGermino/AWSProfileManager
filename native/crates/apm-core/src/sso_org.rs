//! Identity Center org helpers. Port of `src/shared/ssoOrg.ts` and `src/main/services/ssoOrgKey.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::js;

/// An Identity Center organisation: one access portal in one region. One SSO session serves
/// every profile in an org.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoOrg {
    pub start_url: String,
    pub region: String,
}

/// `authType` absent means a profile created before Identity Center support existed: SAML.
/// Unknown values pass through unchanged, as in 1.x.
pub fn resolve_auth_type(auth_type: Option<&str>) -> &str {
    auth_type.unwrap_or("saml")
}

pub fn is_identity_center(auth_type: Option<&str>) -> bool {
    resolve_auth_type(auth_type) == "identityCenter"
}

static FRAGMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/?#.*$").unwrap());
static TRAILING_SLASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/+$").unwrap());

/// The portal shows `https://d-xxxx.awsapps.com/start/#/`, but the OIDC and portal APIs reject
/// the fragment, so users can paste what they see.
pub fn normalize_start_url(raw: &str) -> String {
    let s = js::trim(raw);
    let s = FRAGMENT.replace(s, "");
    TRAILING_SLASHES.replace(&s, "").into_owned()
}

/// Plain string identity for an org, for map keys. Not a secret and not hashed.
pub fn org_key(org: &SsoOrg) -> String {
    format!("{}|{}", normalize_start_url(&org.start_url), js::trim(&org.region))
}

pub fn org_of_profile(sso_start_url: Option<&str>, sso_region: Option<&str>) -> Option<SsoOrg> {
    let start_url = normalize_start_url(sso_start_url.unwrap_or(""));
    let region = js::trim(sso_region.unwrap_or("")).to_string();
    if start_url.is_empty() || region.is_empty() {
        return None;
    }
    Some(SsoOrg { start_url, region })
}

pub fn same_org(a: &SsoOrg, b: &SsoOrg) -> bool {
    org_key(a) == org_key(b)
}

/// The fields of a profile the org helpers read.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrgFields<'a> {
    pub auth_type: Option<&'a str>,
    pub sso_start_url: Option<&'a str>,
    pub sso_region: Option<&'a str>,
}

/// Distinct orgs referenced by Identity Center profiles, in first-seen order. A later profile of
/// the same org replaces the stored value but keeps its position (JavaScript `Map.set`).
pub fn distinct_orgs<'a>(profiles: impl IntoIterator<Item = OrgFields<'a>>) -> Vec<SsoOrg> {
    let mut out: Vec<(String, SsoOrg)> = Vec::new();
    for p in profiles {
        if !is_identity_center(p.auth_type) {
            continue;
        }
        let Some(org) = org_of_profile(p.sso_start_url, p.sso_region) else { continue };
        let key = org_key(&org);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = org,
            None => out.push((key, org)),
        }
    }
    out.into_iter().map(|(_, o)| o).collect()
}

/// Hashed identity for an org: the keychain account name for its SSO session (and, in 1.x, the
/// Electron session partition name).
pub fn sso_keychain_account(org: &SsoOrg) -> String {
    let digest = Sha256::digest(org_key(org).as_bytes());
    format!("sso:{}", &hex::encode(digest)[..32])
}
