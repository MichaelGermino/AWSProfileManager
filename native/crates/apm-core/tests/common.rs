//! Shared by the backend scenario runner (scenarios.rs) and the 2.0 UI flow runner
//! (apm-app/tests/flows.rs): matching rules, sandboxes, given/then files, the fakes.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use apm_core::env::Paths;
use apm_core::{crypto, json as jsjson};
use regex::Regex;
use serde_json::Value;

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

// ------------------------------------------------------------------------------------------------
// Matching (same rules as tests/parity/support/scenario.ts)
// ------------------------------------------------------------------------------------------------

pub fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 300 {
        format!("{}…", &s[..s.char_indices().nth(300).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

pub fn placeholder(name: &str, v: &Value) -> Option<bool> {
    let uuid = Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap();
    let iso = Regex::new(r"\d{4}-\d{2}-\d{2}T").unwrap();
    Some(match name {
        "$any" => true,
        "$string" => v.is_string(),
        "$nonEmptyString" => v.as_str().is_some_and(|s| !s.is_empty()),
        "$number" => v.is_number(),
        "$boolean" => v.is_boolean(),
        "$uuid" => v.as_str().is_some_and(|s| uuid.is_match(s)),
        "$iso" => v.as_str().is_some_and(|s| apm_core::env::parse_js_date_ms(s).is_some() && iso.is_match(s)),
        "$v1cipher" => v.as_str().is_some_and(|s| s.starts_with("v1:")),
        _ => return None,
    })
}

pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

pub fn matches(actual: &Value, expected: &Value, at: &str) -> Vec<String> {
    if let Some(d) = expected.get("$decrypt").filter(|_| expected.is_object()) {
        let Some(blob) = actual.as_str().filter(|s| s.starts_with("v1:")) else {
            return vec![format!("{at}: expected v1: ciphertext, got {}", short(actual))];
        };
        let Some(plain) = crypto::decrypt(d["password"].as_str().unwrap_or(""), blob) else {
            return vec![format!("{at}: does not decrypt with the given password")];
        };
        if let Some(j) = d.get("json") {
            return match serde_json::from_str::<Value>(&plain) {
                Ok(v) => matches(&v, j, &format!("{at}<decrypted json>")),
                Err(_) => vec![format!("{at}: decrypted text is not JSON")],
            };
        }
        return matches(&Value::String(plain), d.get("equals").unwrap_or(&Value::Null), &format!("{at}<decrypted>"));
    }
    if let Some(each) = expected.get("$each").filter(|_| expected.is_object()) {
        let Some(items) = actual.as_array() else { return vec![format!("{at}: expected array, got {}", short(actual))] };
        let min = expected.get("$minItems").and_then(Value::as_u64).unwrap_or(1) as usize;
        if items.len() < min {
            return vec![format!("{at}: expected at least {min} items, got {}", items.len())];
        }
        return items.iter().enumerate().flat_map(|(i, a)| matches(a, each, &format!("{at}[{i}]"))).collect();
    }
    if let Some(re) = expected.as_str().and_then(|s| s.strip_prefix("$match:")) {
        let re = Regex::new(re).unwrap_or_else(|e| panic!("bad $match regex {re}: {e}"));
        return if actual.as_str().is_some_and(|s| re.is_match(s)) {
            vec![]
        } else {
            vec![format!("{at}: expected to match /{re}/, got {}", short(actual))]
        };
    }
    if let Some(name) = expected.as_str() {
        if let Some(ok) = placeholder(name, actual) {
            return if ok { vec![] } else { vec![format!("{at}: expected {name}, got {}", short(actual))] };
        }
    }
    match expected {
        Value::Array(exp) => {
            let Some(act) = actual.as_array() else { return vec![format!("{at}: expected array, got {}", short(actual))] };
            if act.len() != exp.len() {
                return vec![format!("{at}: expected {} items, got {}: {}", exp.len(), act.len(), short(actual))];
            }
            exp.iter().zip(act).enumerate().flat_map(|(i, (e, a))| matches(a, e, &format!("{at}[{i}]"))).collect()
        }
        Value::Object(exp) => {
            let Some(act) = actual.as_object() else { return vec![format!("{at}: expected object, got {}", short(actual))] };
            let partial = exp.get("$partial") == Some(&Value::Bool(true));
            let mut out = Vec::new();
            for (k, v) in exp {
                if k == "$partial" {
                    continue;
                }
                match v.as_str() {
                    Some("$absent") => {
                        if let Some(a) = act.get(k) {
                            out.push(format!("{at}.{k}: expected absent, got {}", short(a)));
                        }
                        continue;
                    }
                    Some("$optional") => continue,
                    Some(o) if o.starts_with("$optional:") => {
                        if let Some(a) = act.get(k) {
                            out.extend(matches(a, &Value::String(format!("${}", &o["$optional:".len()..])), &format!("{at}.{k}")));
                        }
                        continue;
                    }
                    _ => {}
                }
                match act.get(k) {
                    None => out.push(format!("{at}.{k}: missing")),
                    Some(a) => out.extend(matches(a, v, &format!("{at}.{k}"))),
                }
            }
            if !partial {
                for (k, a) in act {
                    if !exp.contains_key(k) {
                        out.push(format!("{at}.{k}: unexpected key (value {})", short(a)));
                    }
                }
            }
            out
        }
        _ => {
            if same(actual, expected) {
                vec![]
            } else {
                vec![format!("{at}: expected {}, got {}", short(expected), short(actual))]
            }
        }
    }
}

pub fn with_tokens(v: &Value, tokens: &[(&str, String)]) -> Value {
    let sub = |s: &str| tokens.iter().fold(s.to_string(), |acc, (k, val)| acc.replace(&format!("{{{k}}}"), val));
    match v {
        Value::String(s) => Value::String(sub(s)),
        Value::Array(a) => Value::Array(a.iter().map(|x| with_tokens(x, tokens)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (sub(k), with_tokens(x, tokens))).collect()),
        other => other.clone(),
    }
}

pub fn resolve_refs(v: &Value, refs: &HashMap<String, Value>) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve_refs(x, refs)).collect()),
        Value::Object(o) => {
            if let (Some(Value::String(r)), 1) = (o.get("$ref"), o.len()) {
                let mut parts = r.split('.');
                let mut cur = refs.get(parts.next().unwrap()).cloned().unwrap_or(Value::Null);
                for p in parts {
                    cur = cur.get(p).cloned().unwrap_or(Value::Null);
                }
                return cur;
            }
            Value::Object(o.iter().map(|(k, x)| (k.clone(), resolve_refs(x, refs))).collect())
        }
        other => other.clone(),
    }
}

// ------------------------------------------------------------------------------------------------
// Sandbox
// ------------------------------------------------------------------------------------------------

pub struct Sandbox {
    pub root: PathBuf,
    pub paths: Paths,
}

impl Sandbox {
    pub fn new(id: &str) -> Self {
        let root = std::env::temp_dir().join(format!("apm-scn-{id}-{}", uuid_like()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths { app_data: root.join("appdata"), credentials_file: root.join("aws").join("credentials") };
        std::fs::create_dir_all(&paths.app_data).unwrap();
        Sandbox { root, paths }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn uuid_like() -> String {
    format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

pub fn write_given_file(path: &Path, v: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    match v {
        Value::String(s) => std::fs::write(path, s).unwrap(),
        other => std::fs::write(path, jsjson::stringify_pretty(other)).unwrap(),
    }
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

pub fn check_file(label: &str, path: &Path, exp: &Value, refs: &HashMap<String, Value>, eol: &str) -> Vec<String> {
    let exists = path.exists();
    if exp.get("absent").is_some() {
        return if exists { vec![format!("{label}: expected file to be absent")] } else { vec![] };
    }
    if !exists {
        return vec![format!("{label}: file missing")];
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if let Some(want) = exp.get("text").and_then(Value::as_str) {
        let want = want.replace("{EOL}", eol);
        return if text == want {
            vec![]
        } else {
            vec![format!("{label}: text differs\n  expected {want:?}\n  actual   {text:?}")]
        };
    }
    if exp.get("contains").is_some() || exp.get("notContains").is_some() {
        let list = |k: &str| exp.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut out = Vec::new();
        for s in list("contains").iter().filter_map(Value::as_str) {
            if !text.contains(s) {
                out.push(format!("{label}: missing {s:?}"));
            }
        }
        for s in list("notContains").iter().filter_map(Value::as_str) {
            if text.contains(s) {
                out.push(format!("{label}: must not contain {s:?}"));
            }
        }
        return out;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => matches(&v, &resolve_refs(&exp["json"], refs), label),
        Err(e) => vec![format!("{label}: not JSON ({e})")],
    }
}

// ------------------------------------------------------------------------------------------------
// Fake services: tests/fakes/serve.ts as a child process (the same fakes the Electron suite uses)
// ------------------------------------------------------------------------------------------------

pub struct Fakes {
    pub base_url: String,
    pub host_map: HashMap<String, String>,
    pub ca_pem: String,
    pub control: String,
    pub ca_file: String,
    _child: tokio::process::Child,
}

pub fn node_exe() -> String {
    if let Ok(n) = std::env::var("APM_NODE") {
        return n;
    }
    let pf = Path::new(r"C:\Program Files\nodejs\node.exe");
    if cfg!(windows) && pf.exists() {
        return pf.to_string_lossy().into_owned();
    }
    "node".to_string()
}

pub async fn start_fakes(config: &Value) -> Fakes {
    use tokio::io::AsyncBufReadExt;
    let script = fixtures().join("../../../tests/fakes/serve.ts");
    let mut child = tokio::process::Command::new(node_exe())
        .arg(script)
        .arg(jsjson::stringify(config))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start node for the fakes (set APM_NODE to node.exe if it is not on PATH)");
    let stdout = child.stdout.take().unwrap();
    let mut line = String::new();
    tokio::io::BufReader::new(stdout).read_line(&mut line).await.unwrap();
    let info: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("fakes did not start: {e}: {line}"));
    let host_map = info["hostMap"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    Fakes {
        base_url: info["baseUrl"].as_str().unwrap().to_string(),
        host_map,
        ca_pem: std::fs::read_to_string(info["caFile"].as_str().unwrap()).unwrap(),
        control: info["control"].as_str().unwrap().to_string(),
        ca_file: info["caFile"].as_str().unwrap().to_string(),
        _child: child,
    }
}

impl Fakes {
    pub async fn requests(&self) -> Value {
        reqwest::get(format!("{}/requests", self.control)).await.unwrap().json().await.unwrap()
    }

    pub async fn reset(&self) {
        reqwest::Client::new().post(format!("{}/reset", self.control)).send().await.unwrap();
    }
}

/// Real hosts the app may call. Without fakes they go nowhere (the Electron runner's BLACKHOLE in
/// tests/parity/support/electronApp.ts), so a scenario never reaches the real AWS or GitHub.
pub const BLACKHOLE: &str = "https://127.0.0.1:9";

pub fn blackhole_host_map() -> HashMap<String, String> {
    let mut hosts: Vec<String> =
        ["api.github.com", "sts.us-west-2.amazonaws.com", "signin.aws.amazon.com", "docs.aws.amazon.com"].iter().map(|s| s.to_string()).collect();
    for r in ["us-east-1", "us-west-2", "us-east-2", "us-west-1", "eu-west-1"] {
        hosts.push(format!("oidc.{r}.amazonaws.com"));
        hosts.push(format!("portal.sso.{r}.amazonaws.com"));
        hosts.push(format!("{r}.signin.aws.amazon.com"));
    }
    hosts.into_iter().map(|h| (h, BLACKHOLE.to_string())).collect()
}


/// given.keychain value: a plain string, or {"$encrypt": {"password", "plaintext" | "json"}}.
pub fn materialize_secret(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    let e = &v["$encrypt"];
    let plain = match e.get("plaintext").and_then(Value::as_str) {
        Some(p) => p.to_string(),
        None => jsjson::stringify(&e["json"]),
    };
    crypto::encrypt(e["password"].as_str().unwrap_or(""), &plain)
}


/// The 1.x package.json version (both apps report the same one in tests).
pub fn app_version() -> String {
    let pkg: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("../../../package.json")).unwrap()).unwrap();
    pkg["version"].as_str().unwrap().to_string()
}
