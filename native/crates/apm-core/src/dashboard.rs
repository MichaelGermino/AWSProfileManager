//! Profile list summaries. Port of `dashboardService.ts`.
//!
//! Status is cross-checked against the credentials file: after a restore or a hand edit a profile
//! must not show a live countdown while `--profile x` fails.

use std::sync::LazyLock;

use chrono::{TimeZone, Timelike};
use regex::Regex;
use serde_json::{Map, Value};

use crate::credentials_ini::Section;
use crate::env::parse_js_date_ms;
use crate::store::{section_name_of, Obj};

static ROLE_ARN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"arn:aws:iam::(\d+):role/(.+)").unwrap());

fn str_field<'a>(o: &'a Obj, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

/// Account id and role, from the Identity Center fields or the SAML role ARN.
fn account_and_role(p: &Obj) -> (Option<String>, Option<String>) {
    if let Some(acct) = str_field(p, "ssoAccountId").filter(|s| !s.is_empty()) {
        return (Some(acct.to_string()), str_field(p, "ssoRoleName").map(str::to_string));
    }
    match str_field(p, "roleArn").and_then(|arn| ROLE_ARN.captures(arn)) {
        Some(c) => (Some(c[1].to_string()), Some(c[2].to_string())),
        None => (None, None),
    }
}

fn friendly_name<'a>(names: &'a Map<String, Value>, account: &str) -> Option<&'a str> {
    names.get(account).and_then(Value::as_str).map(crate::js::trim)
}

/// "Friendly (123456789012) - Role" when a display name is configured, else the stored text.
fn resolve_display_text(p: &Obj, names: &Map<String, Value>) -> Value {
    let (account, role) = account_and_role(p);
    if let Some(acct) = account.as_deref() {
        if let Some(friendly) = friendly_name(names, acct).filter(|f| !f.is_empty()) {
            return Value::String(match role {
                Some(r) => format!("{friendly} ({acct}) - {r}"),
                None => format!("{friendly} ({acct})"),
            });
        }
    }
    // `p.roleDisplayText ?? p.accountNumber ?? p.label ?? ''`
    for key in ["roleDisplayText", "accountNumber", "label"] {
        match p.get(key) {
            None | Some(Value::Null) => continue,
            Some(v) => return v.clone(),
        }
    }
    Value::String(String::new())
}

/// Compact "account - role" for menus (tray); falls back to the profile name.
pub fn profile_menu_label(p: &Obj, names: &Map<String, Value>) -> String {
    let (account, role) = account_and_role(p);
    let shown = account.as_deref().map(|a| friendly_name(names, a).map(str::to_string).unwrap_or_else(|| a.to_string()));
    if let (Some(a), Some(r)) = (shown.filter(|a| !a.is_empty()), role.filter(|r| !r.is_empty())) {
        return format!("{a} - {r}");
    }
    let text = str_field(p, "roleDisplayText").map(crate::js::trim).unwrap_or("");
    if text.is_empty() {
        str_field(p, "name").unwrap_or("").to_string()
    } else {
        text.to_string()
    }
}

/// `new Date(iso).toLocaleString('en-US', { timeZone: 'America/Los_Angeles' })`, e.g.
/// "12/31/2098, 4:00:00 PM" (ICU puts U+202F before AM/PM).
pub fn format_pacific(iso: &str) -> String {
    let Some(ms) = parse_js_date_ms(iso) else { return "Invalid Date".to_string() };
    let Some(utc) = chrono::Utc.timestamp_millis_opt(ms).single() else { return "Invalid Date".to_string() };
    let t = utc.with_timezone(&chrono_tz::America::Los_Angeles);
    let (pm, hour12) = t.hour12();
    format!(
        "{}, {}:{:02}:{:02}\u{202f}{}",
        t.format("%-m/%-d/%Y"),
        hour12,
        t.minute(),
        t.second(),
        if pm { "PM" } else { "AM" }
    )
}

/// `DashboardProfileSummary[]` for the profile list, at `now_ms`.
pub fn summaries(profiles: &[Obj], sections: Option<&[(String, Section)]>, names: &Map<String, Value>, now_ms: i64) -> Vec<Value> {
    profiles
        .iter()
        .map(|p| {
            let section = section_name_of(p);
            let section_missing =
                sections.is_some_and(|s| !section.is_empty() && !s.iter().any(|(n, _)| *n == section));
            let expiration = str_field(p, "expiration").filter(|e| !e.is_empty());
            let exp_ms = expiration.and_then(parse_js_date_ms);
            let is_expired = exp_ms.is_some_and(|e| e > 0 && e <= now_ms);
            let never = expiration.is_none() || section_missing;
            let status = if never { "never" } else if is_expired { "expired" } else { "active" };
            let mut out = Map::new();
            for key in ["id", "name"] {
                if let Some(v) = p.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
            out.insert("accountNumber".into(), resolve_display_text(p, names));
            if let Some(v) = p.get("label") {
                out.insert("label".into(), v.clone());
            }
            out.insert("status".into(), status.into());
            if let (false, Some(e)) = (section_missing, expiration) {
                out.insert("expiresAtPst".into(), format_pacific(e).into());
            }
            if status == "active" {
                // Math.floor((exp - now) / 1000), clamped at 0; an Invalid Date gives 0.
                let secs = exp_ms.map_or(0, |e| (e - now_ms).div_euclid(1000)).max(0);
                out.insert("timeRemainingSeconds".into(), secs.into());
            }
            for key in ["iconName", "iconColor", "folderId"] {
                if let Some(v) = p.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
            Value::Object(out)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn pacific_format() {
        assert_eq!(super::format_pacific("2099-01-01T00:00:00.000Z"), "12/31/2098, 4:00:00\u{202f}PM");
        assert_eq!(super::format_pacific("2000-07-01T07:05:09Z"), "7/1/2000, 12:05:09\u{202f}AM");
        assert_eq!(super::format_pacific("nope"), "Invalid Date");
    }
}
