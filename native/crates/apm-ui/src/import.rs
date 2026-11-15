//! Bulk import of accounts (1.x `wizard/importModel.ts`, `persistImport.ts` and `AccountPicker.tsx`).
//!
//! SAML role listing and Identity Center account listing return different shapes; both are
//! normalized to [`ImportableAccount`] so the picker and the profile building are written once.

use std::collections::{HashMap, HashSet};

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, Role, SharedString, Toggled};
use gpui_component::input::{Input, InputState};
use serde_json::{json, Map, Value};

use crate::a11y::PressExt as _;
use crate::bridge::Bridge;
use crate::theme::{size, Palette};
use crate::widgets::{icon, read_text, FocusRing as _};

#[derive(Debug, Clone)]
pub struct ImportableRole {
    /// The role ARN for SAML, `accountId|roleName` for Identity Center.
    pub key: String,
    pub role_name: String,
    /// SAML only: (role ARN, principal ARN).
    pub saml: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ImportableAccount {
    pub account_id: String,
    pub account_name: String,
    pub email: Option<String>,
    pub roles: Vec<ImportableRole>,
}

#[derive(Debug, Clone)]
pub enum ImportSource {
    IdentityCenter { start_url: String, region: String },
    Saml { idp_entry_url: String },
}

fn st<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn name_from(names: &Map<String, Value>, id: &str) -> Option<String> {
    names.get(id).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Accounts by name, roles by name: both APIs return an unstable order between calls.
fn sorted(mut accounts: Vec<ImportableAccount>) -> Vec<ImportableAccount> {
    for a in accounts.iter_mut() {
        a.roles.sort_by(|x, y| apm_core::assist::locale_compare(&x.role_name, &y.role_name));
    }
    accounts.sort_by(|a, b| apm_core::assist::locale_compare(&a.account_name, &b.account_name));
    accounts
}

/// `sso:listAccounts` accounts. A name configured in Settings wins over the AWS account name.
pub fn from_sso_accounts(accounts: &[Value], names: &Map<String, Value>) -> Vec<ImportableAccount> {
    sorted(
        accounts
            .iter()
            .map(|a| {
                let id = st(a, "accountId").to_string();
                let aws_name = st(a, "accountName");
                let account_name = name_from(names, &id).unwrap_or_else(|| if aws_name.is_empty() { id.clone() } else { aws_name.to_string() });
                let roles = a
                    .get("roles")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|r| ImportableRole { key: format!("{id}|{r}"), role_name: r.to_string(), saml: None })
                    .collect();
                ImportableAccount { account_id: id, account_name, email: a.get("emailAddress").and_then(Value::as_str).map(str::to_string), roles }
            })
            .collect(),
    )
}

/// `arn:aws:iam::<account>:role/<name>` → (account, name).
fn parse_role_arn(arn: &str) -> Option<(String, String)> {
    let at = arn.find("arn:aws:iam::")?;
    let rest = &arn[at + "arn:aws:iam::".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let name = rest[digits.len()..].strip_prefix(":role/")?;
    (!name.is_empty()).then(|| (digits, name.to_string()))
}

/// SAML roles arrive as a flat list of ARNs; group them by account. The IdP page does not always
/// name the account, so fall back to the name configured in Settings, then the bare account id.
/// Roles whose ARN does not parse are dropped: they cannot make a usable profile.
pub fn from_saml_roles(roles: &[Value], names: &Map<String, Value>) -> Vec<ImportableAccount> {
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, ImportableAccount> = HashMap::new();
    for role in roles {
        let arn = st(role, "roleArn");
        let Some((account_id, role_name)) = parse_role_arn(arn) else { continue };
        let named = st(role, "accountName").trim().to_string();
        let account = by.entry(account_id.clone()).or_insert_with(|| {
            order.push(account_id.clone());
            let account_name = if !named.is_empty() { named.clone() } else { name_from(names, &account_id).unwrap_or_else(|| account_id.clone()) };
            ImportableAccount { account_id: account_id.clone(), account_name, email: None, roles: Vec::new() }
        });
        // A later role of the same account may be the one carrying the friendly name.
        if (account.account_name.is_empty() || account.account_name == account_id) && !named.is_empty() {
            account.account_name = named;
        }
        account.roles.push(ImportableRole { key: arn.to_string(), role_name, saml: Some((arn.to_string(), st(role, "principalArn").to_string())) });
    }
    sorted(order.into_iter().filter_map(|id| by.remove(&id)).collect())
}

/// Credentials-file section names cannot hold whitespace or brackets without confusing consumers.
pub fn to_credential_section_name(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            if c == '-' {
                if !dash {
                    out.push('-');
                }
                dash = true;
            } else {
                out.push(c);
                dash = false;
            }
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_lowercase()
}

fn display_text_for(a: &ImportableAccount, role: &str, names: &Map<String, Value>) -> String {
    let friendly = name_from(names, &a.account_id).unwrap_or_else(|| a.account_name.clone());
    if !friendly.is_empty() && friendly != a.account_id {
        format!("{friendly} ({}) - {role}", a.account_id)
    } else {
        format!("{} / {role}", a.account_id)
    }
}

/// Turn a selection into profiles with section names unique across ALL profiles (two profiles
/// writing one `~/.aws/credentials` section would overwrite each other). Returns the profiles and
/// the account display names to add (fill-if-empty: hand-edited names survive a re-import).
pub fn build_profiles(
    accounts: &[ImportableAccount],
    selected: &HashSet<String>,
    source: &ImportSource,
    existing: &[Value],
    names: &Map<String, Value>,
    refresh_interval_minutes: i64,
    has_stored_credentials: bool,
) -> (Vec<Value>, Map<String, Value>) {
    let mut taken: HashSet<String> = existing
        .iter()
        .map(|p| {
            let c = st(p, "credentialProfileName");
            (if c.is_empty() { st(p, "name") } else { c }).trim().to_lowercase()
        })
        .filter(|s| !s.is_empty())
        .collect();
    let mut new_names = Map::new();
    let mut profiles = Vec::new();
    for a in accounts {
        for role in &a.roles {
            if !selected.contains(&role.key) {
                continue;
            }
            let base_name = format!("{} {}", a.account_name, role.role_name);
            let base = Some(to_credential_section_name(&base_name)).filter(|s| !s.is_empty()).unwrap_or_else(|| to_credential_section_name(&a.account_id));
            let mut section = base.clone();
            let mut n = 2;
            while taken.contains(&section) {
                section = format!("{base}-{n}");
                n += 1;
            }
            taken.insert(section.clone());
            let mut p = json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "name": base_name,
                "label": a.email.clone().unwrap_or_default(),
                "credentialProfileName": section,
                "roleDisplayText": display_text_for(a, &role.role_name, names),
                // Off: importing a dozen accounts should not start a dozen background sign-ins.
                "autoRefresh": false,
                "refreshIntervalMinutes": refresh_interval_minutes,
                // Only when some are stored, or the profile promises a refresh it cannot do.
                "useDefaultCredentials": matches!(source, ImportSource::Saml { .. }) && has_stored_credentials,
            });
            match source {
                ImportSource::IdentityCenter { start_url, region } => {
                    p["idpEntryUrl"] = json!("");
                    p["authType"] = json!("identityCenter");
                    p["ssoStartUrl"] = json!(start_url);
                    p["ssoRegion"] = json!(region);
                    p["ssoAccountId"] = json!(a.account_id);
                    p["ssoRoleName"] = json!(role.role_name);
                }
                ImportSource::Saml { idp_entry_url } => {
                    p["idpEntryUrl"] = json!(idp_entry_url);
                    p["authType"] = json!("saml");
                    if let Some((arn, principal)) = &role.saml {
                        p["roleArn"] = json!(arn);
                        p["principalArn"] = json!(principal);
                    }
                }
            }
            profiles.push(p);
            if name_from(names, &a.account_id).is_none() && a.account_name != a.account_id && !new_names.contains_key(&a.account_id) {
                new_names.insert(a.account_id.clone(), json!(a.account_name));
            }
        }
    }
    (profiles, new_names)
}

/// Save the selection as profiles, fold the discovered account names into
/// `settings.accountDisplayNames`, and remember the IdP / Identity Center org for the next import
/// (fill-if-empty throughout). Returns the profiles created.
pub async fn persist_import(
    b: Bridge,
    lock: std::sync::Arc<futures::lock::Mutex<()>>,
    accounts: Vec<ImportableAccount>,
    selected: HashSet<String>,
    source: ImportSource,
) -> Result<Vec<Value>, String> {
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let settings = b.call("settings:get", vec![]).await?;
    let existing = match b.call("profiles:getAll", vec![]).await? {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    let creds = b.call("credentials:getDefaultDisplay", vec![]).await.unwrap_or(Value::Null);
    let names = settings.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
    let hours = settings.get("defaultSessionDurationHours").and_then(Value::as_f64).unwrap_or(1.0);
    let (profiles, new_names) = build_profiles(
        &accounts,
        &selected,
        &source,
        &existing,
        &names,
        ((hours * 60.0).floor() as i64).max(60),
        creds.get("hasPassword") == Some(&Value::Bool(true)),
    );
    if profiles.is_empty() {
        return Ok(profiles);
    }
    b.call("profiles:createMany", vec![Value::Array(profiles.clone())]).await?;

    let _guard = lock.lock().await;
    // Re-read: settings:save replaces the whole object and an earlier step may have written it.
    let mut fresh = match b.call("settings:get", vec![]).await {
        Ok(Value::Object(m)) => m,
        _ => return Ok(profiles),
    };
    let blank = |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_str).map(str::trim).unwrap_or("").is_empty();
    let mut changed = !new_names.is_empty();
    match &source {
        ImportSource::IdentityCenter { start_url, region } => {
            if blank(&fresh, "defaultSsoStartUrl") {
                fresh.insert("defaultSsoStartUrl".into(), json!(start_url));
                changed = true;
            }
            if blank(&fresh, "defaultSsoRegion") {
                fresh.insert("defaultSsoRegion".into(), json!(region));
                changed = true;
            }
        }
        ImportSource::Saml { idp_entry_url } => {
            if blank(&fresh, "defaultIdpEntryUrl") {
                fresh.insert("defaultIdpEntryUrl".into(), json!(idp_entry_url));
                changed = true;
            }
        }
    }
    if changed {
        let mut all = fresh.get("accountDisplayNames").and_then(Value::as_object).cloned().unwrap_or_default();
        for (k, v) in new_names {
            if name_from(&all, &k).is_none() {
                all.insert(k, v);
            }
        }
        fresh.insert("accountDisplayNames".into(), Value::Object(all));
        b.call("settings:save", vec![Value::Object(fresh)]).await?;
    }
    Ok(profiles)
}

// -------------------------------------------------------------------------------------------------
// Account picker
// -------------------------------------------------------------------------------------------------

/// The picker's state: the accounts on offer, the ticked role keys and the filter field.
pub struct PickerState {
    pub accounts: Vec<ImportableAccount>,
    pub selected: HashSet<String>,
    pub filter: Entity<InputState>,
}

fn total(accounts: &[ImportableAccount]) -> usize {
    accounts.iter().map(|a| a.roles.len()).sum()
}

/// Matching the account keeps all its roles; otherwise narrow to the matching roles.
fn filtered(accounts: &[ImportableAccount], query: &str) -> Vec<ImportableAccount> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return accounts.to_vec();
    }
    accounts
        .iter()
        .filter_map(|a| {
            let hit = a.account_name.to_lowercase().contains(&q) || a.account_id.contains(&q);
            let roles: Vec<ImportableRole> =
                if hit { a.roles.clone() } else { a.roles.iter().filter(|r| r.role_name.to_lowercase().contains(&q)).cloned().collect() };
            (!roles.is_empty()).then(|| ImportableAccount { roles, ..a.clone() })
        })
        .collect()
}

/// A checkbox of ours: UI Automation toggles it through the accessible Click.
fn checkbox(id: SharedString, label: SharedString, state: Toggled, p: &Palette) -> gpui::Stateful<gpui::Div> {
    let on = state != Toggled::False;
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(label)
        .aria_toggled(state)
        .focusable()
        .tab_stop(true)
        .focus_ring(p)
        .size(px(16.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(if on { p.primary } else { p.border_strong })
        .when(on, |d| d.bg(p.primary))
        .cursor_pointer()
        .when(state == Toggled::True, |d| d.child(icon("check", 12., p.on_primary)))
        .when(state == Toggled::Mixed, |d| d.child(icon("minus", 12., p.on_primary)))
}

/// 1.x `AccountPicker`: a filter, Select all / Clear, then each account with its roles ticked
/// individually or all at once. `picker` reaches the state inside the owning view.
pub fn render_picker<V: 'static>(state: &PickerState, picker: fn(&mut V) -> Option<&mut PickerState>, p: &Palette, cx: &mut Context<V>) -> gpui::Div {
    let query = state.filter.read(cx).value().to_string();
    let cx: &Context<V> = cx;
    let edit = move |f: Box<dyn Fn(&mut PickerState)>| {
        cx.listener(move |this: &mut V, _: &gpui::ClickEvent, _, cx| {
            if let Some(s) = picker(this) {
                f(s);
                cx.notify();
            }
        })
    };
    if state.accounts.is_empty() {
        return div().child(
            read_text("picker-empty", "No accounts were returned. If that looks wrong, check that you signed in with the right account.")
                .p(px(16.))
                .rounded(px(size::RADIUS_LG))
                .border_1()
                .border_color(p.border)
                .bg(p.sunken)
                .text_size(px(size::TEXT_SM))
                .text_color(p.text_muted),
        );
    }
    let shown = filtered(&state.accounts, &query);
    let text_link = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_stop(true)
            .focus_ring(p)
            .px(px(6.))
            .h(px(28.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_size(px(size::TEXT_SM))
            .text_color(p.text_muted)
            .cursor_pointer()
            .hover(|d| d.text_color(p.text).bg(p.hover))
            .child(label)
    };
    let all_keys: HashSet<String> = state.accounts.iter().flat_map(|a| a.roles.iter().map(|r| r.key.clone())).collect();
    let toolbar = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().flex_1().child(Input::new(&state.filter).cleanable(true).aria_label("Filter accounts")))
        .child(text_link("picker-all", "Select all").on_press(edit(Box::new(move |s| s.selected = all_keys.clone()))))
        .child(text_link("picker-clear", "Clear").on_press(edit(Box::new(|s| s.selected.clear()))));

    let mut list = div()
        .id("picker-list")
        .role(Role::Group)
        .aria_label("Accounts")
        .mt(px(12.))
        .max_h(px(360.))
        .overflow_y_scroll()
        .p(px(12.))
        .rounded(px(size::RADIUS_LG))
        .border_1()
        .border_color(p.border)
        .bg(p.sunken)
        .flex()
        .flex_col()
        .gap(px(12.));
    for (ai, a) in shown.iter().enumerate() {
        let here = a.roles.iter().filter(|r| state.selected.contains(&r.key)).count();
        let all_on = here == a.roles.len() && !a.roles.is_empty();
        let tri = if all_on {
            Toggled::True
        } else if here > 0 {
            Toggled::Mixed
        } else {
            Toggled::False
        };
        let keys: Vec<String> = a.roles.iter().map(|r| r.key.clone()).collect();
        let mut block = div().flex().flex_col().gap(px(6.)).child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(div().mt(px(2.)).child(checkbox(SharedString::from(format!("acct-{ai}")), a.account_name.clone().into(), tri, p).on_press(edit(
                    Box::new(move |s| {
                        for k in &keys {
                            if all_on {
                                s.selected.remove(k);
                            } else {
                                s.selected.insert(k.clone());
                            }
                        }
                    }),
                ))))
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child(a.account_name.clone()))
                        .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(a.account_id.clone())),
                ),
        );
        let mut roles = div().pl(px(26.)).flex().flex_col().gap(px(4.));
        for (ri, r) in a.roles.iter().enumerate() {
            let on = state.selected.contains(&r.key);
            let key = r.key.clone();
            roles = roles.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        checkbox(
                            SharedString::from(format!("role-{ai}-{ri}")),
                            format!("{} ({})", r.role_name, a.account_name).into(),
                            if on { Toggled::True } else { Toggled::False },
                            p,
                        )
                        .on_press(edit(Box::new(move |s| {
                            if !s.selected.remove(&key) {
                                s.selected.insert(key.clone());
                            }
                        }))),
                    )
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(r.role_name.clone())),
            );
        }
        block = block.child(roles);
        list = list.child(block);
    }
    if shown.is_empty() {
        list = list.child(
            read_text("picker-nomatch", format!("Nothing matches “{query}”.")).py(px(24.)).text_center().text_size(px(size::TEXT_SM)).text_color(p.text_muted),
        );
    }
    div().flex().flex_col().child(toolbar).child(list).child(
        read_text("picker-count", format!("{} of {} selected", state.selected.len(), total(&state.accounts)))
            .mt(px(8.))
            .text_size(px(size::TEXT_XS))
            .text_color(p.text_muted),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_names() {
        assert_eq!(to_credential_section_name("  Shared Services Dev "), "shared-services-dev");
        assert_eq!(to_credential_section_name("a [b] -- c"), "a-b-c");
        assert_eq!(to_credential_section_name("--x--"), "x");
    }

    #[test]
    fn saml_grouping_and_build() {
        let roles = vec![
            json!({ "roleArn": "arn:aws:iam::111122223333:role/Dev", "principalArn": "arn:aws:iam::111122223333:saml-provider/ADFS", "accountName": "Shared Services" }),
            json!({ "roleArn": "arn:aws:iam::111122223333:role/Admin", "principalArn": "p" }),
            json!({ "roleArn": "garbage", "principalArn": "p" }),
        ];
        let accts = from_saml_roles(&roles, &Map::new());
        assert_eq!(accts.len(), 1);
        assert_eq!(accts[0].account_name, "Shared Services");
        assert_eq!(accts[0].roles[0].role_name, "Admin");
        let sel: HashSet<String> = accts[0].roles.iter().map(|r| r.key.clone()).collect();
        let existing = vec![json!({ "credentialProfileName": "shared-services-dev" })];
        let (ps, names) = build_profiles(&accts, &sel, &ImportSource::Saml { idp_entry_url: "u".into() }, &existing, &Map::new(), 60, true);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[1]["credentialProfileName"], "shared-services-dev-2");
        assert_eq!(ps[1]["roleDisplayText"], "Shared Services (111122223333) - Dev");
        assert_eq!(ps[0]["useDefaultCredentials"], true);
        assert_eq!(names["111122223333"], "Shared Services");
    }
}
