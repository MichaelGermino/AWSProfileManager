//! Layer A (docs/native-rewrite-plan.md §5.1): the pure-function vectors in
//! `native/tests/fixtures/vectors`, shared with the TypeScript runner (tests/vectors/vectors.test.ts).
//! Each file is `{ "function": name, "cases": [{ name, args, expect }] }`; JSON `null` args stand for
//! "absent", and `"$fill:file.json#case"` reuses another case's expectation.

use std::path::{Path, PathBuf};

use apm_core::{
    credentials_ini, crypto, grouping, master_password, network_classifier, org_config, saml_roles, sso_org,
    terminal_profile,
};
use serde_json::{json, Map, Value};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/vectors")
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("expected a string argument, got {v}"))
}

fn opt_s(v: &Value) -> Option<&str> {
    v.as_str()
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn org(v: &Value) -> sso_org::SsoOrg {
    sso_org::SsoOrg {
        start_url: v["startUrl"].as_str().unwrap_or("").to_string(),
        region: v["region"].as_str().unwrap_or("").to_string(),
    }
}

fn section_pairs(v: &Value) -> Vec<(String, String)> {
    obj(v).into_iter().map(|(k, v)| (k, v.as_str().unwrap_or("").to_string())).collect()
}

fn error_info(v: &Value) -> Option<network_classifier::ErrorInfo> {
    match v {
        Value::Null => None,
        Value::String(m) => Some(network_classifier::ErrorInfo { message: Some(m.clone()), ..Default::default() }),
        Value::Object(o) => {
            let str_field = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
            Some(network_classifier::ErrorInfo {
                message: str_field("error").or_else(|| str_field("message")),
                code: str_field("code"),
                errno: str_field("errno"),
                no_response: o.get("isAxiosError") == Some(&Value::Bool(true))
                    && o.get("response").is_none_or(Value::is_null),
                cause: o.get("cause").and_then(error_info).map(Box::new),
            })
        }
        _ => None,
    }
}

fn grouped_json(g: &grouping::Grouped<Value, Value>) -> Value {
    json!({
        "groups": g.groups.iter().map(|x| json!({ "folder": x.folder, "profiles": x.profiles })).collect::<Vec<_>>(),
        "ungrouped": g.ungrouped,
    })
}

fn hex16(v: &Value) -> [u8; 16] {
    hex::decode(s(v)).unwrap().try_into().unwrap()
}

fn hex12(v: &Value) -> [u8; 12] {
    hex::decode(s(v)).unwrap().try_into().unwrap()
}

fn call(function: &str, a: &[Value]) -> Value {
    match function {
        "validateMasterPassword" => json!(master_password::validate(s(&a[0]))),
        "parseOrgConfig" => json!(org_config::parse(&a[0])),
        "describeOrgConfig" => json!(org_config::describe(&obj(&a[0]))),
        "isOrgConfigEmpty" => json!(org_config::is_empty(&obj(&a[0]))),
        "applyOrgConfigToSettings" => {
            let now = chrono::DateTime::parse_from_rfc3339(s(&a[2])).unwrap().with_timezone(&chrono::Utc);
            Value::Object(org_config::apply_to_settings(&obj(&a[0]), &obj(&a[1]), now))
        }
        "groupProfilesByFolder" => {
            grouped_json(&grouping::group_profiles_by_folder(a[0].as_array().unwrap(), a[1].as_array().unwrap()))
        }
        "flattenGrouping" => json!(grouping::flatten_grouping(&grouping::group_profiles_by_folder(
            a[0].as_array().unwrap(),
            a[1].as_array().unwrap()
        ))),
        "resolveAuthType" => json!(sso_org::resolve_auth_type(opt_s(&a[0]["authType"]))),
        "normalizeStartUrl" => json!(sso_org::normalize_start_url(s(&a[0]))),
        "orgKey" => json!(sso_org::org_key(&org(&a[0]))),
        "orgOfProfile" => json!(sso_org::org_of_profile(opt_s(&a[0]["ssoStartUrl"]), opt_s(&a[0]["ssoRegion"]))),
        "sameOrg" => json!(sso_org::same_org(&org(&a[0]), &org(&a[1]))),
        "distinctOrgs" => json!(sso_org::distinct_orgs(a[0].as_array().unwrap().iter().map(|p| sso_org::OrgFields {
            auth_type: opt_s(&p["authType"]),
            sso_start_url: opt_s(&p["ssoStartUrl"]),
            sso_region: opt_s(&p["ssoRegion"]),
        }))),
        "resolveTerminalProfileId" => {
            let ids: Vec<&str> = a[1].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
            json!(terminal_profile::resolve_terminal_profile_id(opt_s(&a[0]), &ids))
        }
        "ssoKeytarAccount" => json!(sso_org::sso_keychain_account(&org(&a[0]))),
        "classifyAuthFailure" => json!(network_classifier::classify(error_info(&a[0]).as_ref()).as_str()),
        "parseSamlRoles" => json!(saml_roles::parse_saml_roles_xml(s(&a[0]))),
        "credentialsParse" => {
            let parsed = credentials_ini::parse(s(&a[0]));
            Value::Object(
                parsed
                    .into_iter()
                    .map(|(name, sec)| (name, Value::Object(sec.into_iter().map(|(k, v)| (k, Value::String(v))).collect())))
                    .collect(),
            )
        }
        "credentialsSetSection" => json!(credentials_ini::set_section(s(&a[0]), s(&a[1]), &section_pairs(&a[2]), s(&a[3]))),
        "credentialsRenameSection" => json!(credentials_ini::rename_section(s(&a[0]), s(&a[1]), s(&a[2]), s(&a[3]))),
        "credentialsRemoveSection" => json!(credentials_ini::remove_section(s(&a[0]), s(&a[1]), s(&a[2]))),
        "encryptV1With" => json!(crypto::encrypt_with(s(&a[0]), &hex16(&a[1]), &hex12(&a[2]), s(&a[3]))),
        "decryptV1" => json!(crypto::decrypt(s(&a[0]), s(&a[1]))),
        other => panic!("no Rust port registered for vector function {other}"),
    }
}

fn fill(arg: &Value) -> Value {
    let Some(r) = arg.as_str().and_then(|a| a.strip_prefix("$fill:")) else { return arg.clone() };
    let (file, name) = r.split_once('#').unwrap();
    let other: Value = serde_json::from_str(&std::fs::read_to_string(dir().join(file)).unwrap()).unwrap();
    other["cases"].as_array().unwrap().iter().find(|c| c["name"] == name).expect("fill target")["expect"].clone()
}

#[test]
fn vectors() {
    let mut files: Vec<_> = std::fs::read_dir(dir()).unwrap().map(|e| e.unwrap().path()).collect();
    files.retain(|p| p.extension().is_some_and(|e| e == "json"));
    files.sort();
    assert!(!files.is_empty(), "no vector files in {}", dir().display());
    let (mut passed, mut failures) = (0, Vec::new());
    for file in files {
        let data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let function = data["function"].as_str().unwrap();
        for case in data["cases"].as_array().unwrap() {
            let args: Vec<Value> = case["args"].as_array().unwrap().iter().map(fill).collect();
            let actual = std::panic::catch_unwind(|| call(function, &args));
            match actual {
                Ok(v) if v == case["expect"] => passed += 1,
                Ok(v) => failures.push(format!(
                    "{} / {}\n    expected {}\n    actual   {}",
                    file.file_name().unwrap().to_string_lossy(),
                    case["name"],
                    case["expect"],
                    v
                )),
                Err(_) => failures.push(format!("{} / {}: panicked", file.file_name().unwrap().to_string_lossy(), case["name"])),
            }
        }
    }
    eprintln!("vectors: {passed} passed, {} failed", failures.len());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
