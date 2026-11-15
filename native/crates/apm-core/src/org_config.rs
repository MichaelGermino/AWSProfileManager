//! Organization configuration file. Port of `src/shared/orgConfig.ts`.
//!
//! SECURITY: the file is meant to be emailed around, so it carries no secrets: no API key, IdP
//! credentials, master password, SSO tokens or credentials-file content. Only URLs, region, model
//! and account display names. Unknown keys are dropped on parse so a file from a newer version
//! cannot inject settings.
//!
//! Configs and settings are handled as JSON objects: settings must round-trip unknown keys, and
//! "undefined" in 1.x means "key absent", which a `Map` expresses directly.

use std::sync::LazyLock;

use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{Map, Value};

use crate::{js, sso_org::normalize_start_url};

pub const VERSION: u64 = 1;
pub const KIND: &str = "aws-profile-manager-org-config";

static ACCOUNT_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{12}$").unwrap());

fn clean_string(v: Option<&Value>) -> Option<String> {
    let s = js::trim(v?.as_str()?);
    (!s.is_empty()).then(|| s.to_string())
}

/// Only 12-digit account ids mapped to non-empty names; anything else is ignored.
fn clean_display_names(v: Option<&Value>) -> Option<Map<String, Value>> {
    let obj = v?.as_object()?;
    let out: Map<String, Value> = obj
        .iter()
        .filter(|(id, _)| ACCOUNT_ID.is_match(id))
        .filter_map(|(id, name)| clean_string(Some(name)).map(|n| (id.clone(), Value::String(n))))
        .collect();
    (!out.is_empty()).then_some(out)
}

fn put(out: &mut Map<String, Value>, key: &str, v: Option<impl Into<Value>>) {
    if let Some(v) = v {
        out.insert(key.to_string(), v.into());
    }
}

/// Parse and sanitise an untrusted file. `None` when it is not an org config at all.
pub fn parse(raw: &Value) -> Option<Map<String, Value>> {
    let o = raw.as_object()?;
    if o.get("kind").and_then(Value::as_str) != Some(KIND) {
        return None;
    }
    let mut out = Map::new();
    out.insert("kind".into(), KIND.into());
    let version = match o.get("version") {
        Some(v @ Value::Number(_)) => v.clone(),
        _ => VERSION.into(),
    };
    out.insert("version".into(), version);
    put(&mut out, "organizationName", clean_string(o.get("organizationName")));
    put(&mut out, "idpEntryUrl", clean_string(o.get("idpEntryUrl")));
    put(&mut out, "ssoStartUrl", clean_string(o.get("ssoStartUrl")).map(|u| normalize_start_url(&u)));
    put(&mut out, "ssoRegion", clean_string(o.get("ssoRegion")));
    put(&mut out, "openWebUiApiUrl", clean_string(o.get("openWebUiApiUrl")));
    put(&mut out, "openWebUiModel", clean_string(o.get("openWebUiModel")));
    put(&mut out, "accountDisplayNames", clean_display_names(o.get("accountDisplayNames")));
    Some(out)
}

fn str_of<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// What a config provides, one line each, for the import confirmation.
pub fn describe(config: &Map<String, Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if str_of(config, "idpEntryUrl").is_some() {
        lines.push("SAML IdP URL".to_string());
    }
    if str_of(config, "ssoStartUrl").is_some() {
        lines.push("Identity Center start URL".to_string());
    }
    if let Some(r) = str_of(config, "ssoRegion") {
        lines.push(format!("Identity Center region ({r})"));
    }
    if str_of(config, "openWebUiApiUrl").is_some() {
        lines.push("AI assistant URL".to_string());
    }
    if let Some(m) = str_of(config, "openWebUiModel") {
        lines.push(format!("AI model ({m})"));
    }
    let names = config.get("accountDisplayNames").and_then(Value::as_object).map_or(0, Map::len);
    if names > 0 {
        lines.push(format!("{names} account name{}", if names == 1 { "" } else { "s" }));
    }
    lines
}

pub fn is_empty(config: &Map<String, Value>) -> bool {
    describe(config).is_empty()
}

/// `settings.X?.trim() || fallback`: the trimmed setting when non-empty, else the fallback.
fn existing_or(settings: &Map<String, Value>, key: &str, fallback: Option<&str>) -> Option<String> {
    let own = settings.get(key).and_then(Value::as_str).map(js::trim).filter(|s| !s.is_empty());
    own.or(fallback).map(str::to_string)
}

fn set_or_remove(out: &mut Map<String, Value>, key: &str, v: Option<String>) {
    match v {
        Some(v) => {
            out.insert(key.to_string(), Value::String(v));
        }
        None => {
            out.remove(key);
        }
    }
}

/// Merge a config into settings, fill-if-empty: existing values always win and hand-edited
/// account names are kept, so importing is safe at any time.
pub fn apply_to_settings(settings: &Map<String, Value>, config: &Map<String, Value>, now: DateTime<Utc>) -> Map<String, Value> {
    let mut out = settings.clone();
    let cfg = |k: &str| config.get(k).and_then(Value::as_str);
    out.insert(
        "defaultIdpEntryUrl".into(),
        existing_or(settings, "defaultIdpEntryUrl", cfg("idpEntryUrl")).unwrap_or_default().into(),
    );
    set_or_remove(&mut out, "defaultSsoStartUrl", existing_or(settings, "defaultSsoStartUrl", cfg("ssoStartUrl")));
    set_or_remove(&mut out, "defaultSsoRegion", existing_or(settings, "defaultSsoRegion", cfg("ssoRegion")));
    out.insert(
        "openWebUiApiUrl".into(),
        existing_or(settings, "openWebUiApiUrl", cfg("openWebUiApiUrl")).unwrap_or_default().into(),
    );
    out.insert(
        "openWebUiModel".into(),
        existing_or(settings, "openWebUiModel", cfg("openWebUiModel")).unwrap_or_default().into(),
    );
    // { ...config.accountDisplayNames, ...settings.accountDisplayNames }
    let mut names = config.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(own) = settings.get("accountDisplayNames").and_then(Value::as_object) {
        for (k, v) in own {
            names.insert(k.clone(), v.clone());
        }
    }
    out.insert("accountDisplayNames".into(), Value::Object(names));
    out.insert("orgConfigImportedAt".into(), now.to_rfc3339_opts(SecondsFormat::Millis, true).into());
    set_or_remove(&mut out, "orgConfigImportedName", config.get("organizationName").and_then(Value::as_str).map(str::to_string));
    out
}
