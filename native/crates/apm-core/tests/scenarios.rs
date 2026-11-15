//! Layer B (docs/native-rewrite-plan.md §5.2): the backend scenarios in
//! `native/tests/fixtures/scenarios`, run against `apm_core::Core::dispatch`. The Electron runner
//! (tests/parity/scenarios.spec.ts) runs the same files through `window.electron`; the format and
//! the matching rules are in the scenarios README and tests/parity/support/scenario.ts.
//!
//! While Phase 3 is in progress a scenario that needs something not ported yet (a channel, the
//! fakes, the keychain) is reported as PENDING instead of failing. Phase 3 is done when none are.
//! `APM_SCENARIO=BE-PROF-003` runs one scenario.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use apm_core::core::{Core, CoreConfig, DispatchError};
use apm_core::env::{Clock, Paths, Platform};
use apm_core::host::{DialogRequest, Host};
use apm_core::secrets::{Keychain, SecretStore};
use apm_core::{crypto, json as jsjson};
use regex::Regex;
use serde_json::{Map, Value};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

// ------------------------------------------------------------------------------------------------
// Recording host: events, external opens and scripted dialog answers.
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Value>>,
    streams: Mutex<HashMap<String, Vec<Value>>>,
    opens: Mutex<Vec<Value>>,
    dialogs: Mutex<VecDeque<Option<String>>>,
}

const STREAMS: &[&str] = &["terminal:data", "ai:chat-chunk"];

impl Host for Recorder {
    fn emit(&self, channel: &str, payload: Vec<Value>) {
        if STREAMS.contains(&channel) {
            let item = if payload.len() == 1 { payload.into_iter().next().unwrap() } else { Value::Array(payload) };
            self.streams.lock().unwrap().entry(channel.to_string()).or_default().push(item);
            return;
        }
        let mut e = vec![Value::String(channel.to_string())];
        e.extend(payload);
        self.events.lock().unwrap().push(Value::Array(e));
    }

    fn open_external(&self, kind: &str, target: &str) {
        self.opens.lock().unwrap().push(serde_json::json!({ "kind": kind, "target": target }));
    }

    fn dialog(&self, _request: DialogRequest) -> Option<String> {
        self.dialogs.lock().unwrap().pop_front().flatten()
    }
}

// ------------------------------------------------------------------------------------------------
// Matching (same rules as tests/parity/support/scenario.ts)
// ------------------------------------------------------------------------------------------------

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 300 {
        format!("{}…", &s[..s.char_indices().nth(300).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn placeholder(name: &str, v: &Value) -> Option<bool> {
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

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn matches(actual: &Value, expected: &Value, at: &str) -> Vec<String> {
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

fn with_tokens(v: &Value, tokens: &[(&str, String)]) -> Value {
    let sub = |s: &str| tokens.iter().fold(s.to_string(), |acc, (k, val)| acc.replace(&format!("{{{k}}}"), val));
    match v {
        Value::String(s) => Value::String(sub(s)),
        Value::Array(a) => Value::Array(a.iter().map(|x| with_tokens(x, tokens)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (sub(k), with_tokens(x, tokens))).collect()),
        other => other.clone(),
    }
}

fn resolve_refs(v: &Value, refs: &HashMap<String, Value>) -> Value {
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

struct Sandbox {
    root: PathBuf,
    paths: Paths,
}

impl Sandbox {
    fn new(id: &str) -> Self {
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

fn uuid_like() -> String {
    format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

fn write_given_file(path: &Path, v: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    match v {
        Value::String(s) => std::fs::write(path, s).unwrap(),
        other => std::fs::write(path, jsjson::stringify_pretty(other)).unwrap(),
    }
}

fn copy_dir(from: &Path, to: &Path) {
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

fn check_file(label: &str, path: &Path, exp: &Value, refs: &HashMap<String, Value>, eol: &str) -> Vec<String> {
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
// Running one scenario
// ------------------------------------------------------------------------------------------------

enum Outcome {
    Pass,
    Pending(String),
    Fail(Vec<String>),
    Skipped,
}

fn app_version() -> String {
    let pkg: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("../../../package.json")).unwrap()).unwrap();
    pkg["version"].as_str().unwrap().to_string()
}

fn new_core(sandbox: &Sandbox, clock: &Clock, host: Arc<Recorder>, service: &str, keychain: &Arc<Keychain>) -> Core {
    let cfg = CoreConfig {
        paths: sandbox.paths.clone(),
        platform: Platform::current(),
        keychain_service: service.to_string(),
        app_version: app_version(),
        clock: clock.clone(),
    };
    Core::with_secrets(cfg, host, keychain.clone())
}

/// given.keychain value: a plain string, or {"$encrypt": {"password", "plaintext" | "json"}}.
fn materialize_secret(v: &Value) -> String {
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

/// Removes every keychain entry a scenario may have created under its test service.
struct KeychainCleanup {
    keychain: Arc<Keychain>,
    accounts: Vec<String>,
}

impl Drop for KeychainCleanup {
    fn drop(&mut self) {
        let mut all = self.accounts.clone();
        all.extend(self.keychain.touched());
        for a in all {
            self.keychain.delete(&a);
        }
    }
}

async fn run(s: &Value) -> Outcome {
    let id = s["id"].as_str().unwrap_or("?");
    if let Some(platforms) = s.get("platforms").and_then(Value::as_array) {
        if !platforms.iter().any(|p| p.as_str() == Some(Platform::current().node_name())) {
            return Outcome::Skipped;
        }
    }
    let given = s.get("given").cloned().unwrap_or(Value::Object(Map::new()));
    for unported in ["fakes"] {
        if given.get(unported).is_some() {
            return Outcome::Pending(format!("needs given.{unported}"));
        }
    }
    if given.get("env").is_some() {
        return Outcome::Pending("needs given.env (scheduler hooks)".into());
    }

    let sandbox = Sandbox::new(id);
    if let Some(dir) = given.get("dataDir").and_then(Value::as_str) {
        copy_dir(&fixtures().join("datadirs").join(dir), &sandbox.paths.app_data);
    }
    if let Some(files) = given.get("files").and_then(Value::as_object) {
        for (name, v) in files {
            write_given_file(&sandbox.paths.app_data.join(name), v);
        }
    }
    if let Some(files) = given.get("sandboxFiles").and_then(Value::as_object) {
        for (name, v) in files {
            write_given_file(&sandbox.root.join(name), v);
        }
    }
    if let Some(c) = given.get("credentials").and_then(Value::as_str) {
        write_given_file(&sandbox.paths.credentials_file, &Value::String(c.to_string()));
    }

    let clock = Clock::default();
    let service = format!("AWSProfileManager-test-{id}");
    let keychain = Arc::new(Keychain::new(&service));
    let mut cleanup_accounts: Vec<String> = ["__default__", "__default___username", "__master_password_verifier__"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for section in ["given", "then"] {
        if let Some(kc) = s.get(section).and_then(|g| g.get("keychain")).and_then(Value::as_object) {
            cleanup_accounts.extend(kc.keys().cloned());
        }
    }
    let _cleanup = KeychainCleanup { keychain: keychain.clone(), accounts: cleanup_accounts.clone() };
    for a in &cleanup_accounts {
        keychain.delete(a);
    }
    if let Some(kc) = given.get("keychain").and_then(Value::as_object) {
        for (account, v) in kc {
            keychain.set(account, &materialize_secret(v)).unwrap();
        }
    }
    let mut host = Arc::new(Recorder::default());
    let mut core = new_core(&sandbox, &clock, host.clone(), &service, &keychain);
    let tokens = vec![("SANDBOX", sandbox.root.to_string_lossy().into_owned()), ("APP_VERSION", app_version())];
    let tok = |v: &Value| with_tokens(v, &tokens);
    let mut refs: HashMap<String, Value> = HashMap::new();
    let mut problems = Vec::new();
    let eol = Platform::current().eol();

    for (i, step) in s["steps"].as_array().unwrap().iter().enumerate() {
        let label = format!("step {}", i + 1);
        if step.get("restart").is_some() {
            host = Arc::new(Recorder::default());
            core = new_core(&sandbox, &clock, host.clone(), &service, &keychain);
            continue;
        }
        if let Some(d) = step.get("dialogs").and_then(Value::as_array) {
            let mut q = host.dialogs.lock().unwrap();
            for item in d {
                q.push_back(tok(item).as_str().map(str::to_string));
            }
            continue;
        }
        if let Some(exp) = step.get("expectEvents") {
            let got = Value::Array(std::mem::take(&mut *host.events.lock().unwrap()));
            problems.extend(matches(&got, &tok(&resolve_refs(exp, &refs)), &format!("{label} events")));
            continue;
        }
        if let Some(ms) = step.get("advanceClock").and_then(Value::as_i64) {
            clock.advance(ms);
            continue;
        }
        if let Some(ms) = step.get("wait").and_then(Value::as_u64) {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            continue;
        }
        if let Some(exp) = step.get("expectExternalOpens") {
            if let Some(ms) = step.get("waitMs").and_then(Value::as_u64) {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
            let got = Value::Array(std::mem::take(&mut *host.opens.lock().unwrap()));
            problems.extend(matches(&got, &tok(&resolve_refs(exp, &refs)), &format!("{label} external opens")));
            continue;
        }
        if let Some(stream) = step.get("expectStream").and_then(Value::as_str) {
            let got = Value::Array(host.streams.lock().unwrap().get(stream).cloned().unwrap_or_default());
            problems.extend(matches(&got, &tok(&resolve_refs(&step["expect"], &refs)), &format!("{label} stream {stream}")));
            continue;
        }
        if step.get("waitForStream").is_some() || step.get("await").is_some() || step.get("async").is_some() {
            return Outcome::Pending(format!("{label}: async/stream steps not ported yet"));
        }
        if step.get("resetFakeRequests").is_some() {
            continue;
        }
        let Some(channel) = step.get("call").and_then(Value::as_str) else {
            return Outcome::Fail(vec![format!("{label}: unknown step {}", short(step))]);
        };
        let args = tok(&resolve_refs(step.get("args").unwrap_or(&Value::Array(vec![])), &refs));
        let result = core.dispatch(channel, args.as_array().map(Vec::as_slice).unwrap_or(&[])).await;
        let outcome = match result {
            Err(DispatchError::Unsupported(c)) => return Outcome::Pending(format!("channel {c}")),
            Err(DispatchError::Failed(m)) => Err(m),
            Ok(v) => Ok(v),
        };
        if let Some(name) = step.get("save").and_then(Value::as_str) {
            let saved = match &outcome {
                Ok(v) => v.clone(),
                Err(m) => serde_json::json!({ "failed": m }),
            };
            refs.insert(name.to_string(), saved);
        }
        match (outcome, step.get("expectError")) {
            (Err(m), Some(want)) => problems.extend(matches(&Value::String(m), &tok(&resolve_refs(want, &refs)), &format!("{label} error"))),
            (Err(m), None) => problems.push(format!("{label} {channel}: call failed: {m}")),
            (Ok(v), Some(_)) => problems.push(format!("{label} {channel}: expected the call to fail, got {}", short(&v))),
            (Ok(v), None) => {
                if let Some(exp) = step.get("expect") {
                    problems.extend(matches(&v, &tok(&resolve_refs(exp, &refs)), &format!("{label} {channel}")));
                }
            }
        }
        if !problems.is_empty() {
            return Outcome::Fail(problems);
        }
    }

    let then = s.get("then").cloned().unwrap_or(Value::Object(Map::new()));
    if then.get("fakeRequests").is_some() {
        return Outcome::Pending("then.fakeRequests not ported yet".into());
    }
    for (account, exp) in then.get("keychain").and_then(Value::as_object).cloned().unwrap_or_default() {
        let got = keychain.get(&account);
        match (exp.as_str() == Some("$absent"), got) {
            (true, Some(_)) => problems.push(format!("keychain {account}: expected absent")),
            (true, None) => {}
            (false, None) => problems.push(format!("keychain {account}: missing")),
            (false, Some(v)) => problems.extend(matches(&Value::String(v), &resolve_refs(&exp, &refs), &format!("keychain {account}"))),
        }
    }
    for (name, exp) in then.get("files").and_then(Value::as_object).cloned().unwrap_or_default() {
        problems.extend(check_file(&name, &sandbox.paths.app_data.join(&name), &tok(&exp), &refs, eol));
    }
    for (name, exp) in then.get("sandboxFiles").and_then(Value::as_object).cloned().unwrap_or_default() {
        problems.extend(check_file(&name, &sandbox.root.join(&name), &tok(&exp), &refs, eol));
    }
    if let Some(exp) = then.get("credentials") {
        problems.extend(check_file("credentials", &sandbox.paths.credentials_file, &tok(exp), &refs, eol));
    }
    if problems.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(problems)
    }
}

fn load(dir: &Path) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            out.extend(load(&p));
        } else if p.extension().is_some_and(|e| e == "json") {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            let file = p.file_name().unwrap().to_string_lossy().into_owned();
            match v {
                Value::Array(items) => out.extend(items.into_iter().map(|s| (file.clone(), s))),
                s => out.push((file, s)),
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn scenarios() {
    let only = std::env::var("APM_SCENARIO").ok();
    let (mut pass, mut pending, mut skipped, mut failures) = (0, Vec::new(), 0, Vec::new());
    for (file, s) in load(&fixtures().join("scenarios")) {
        let id = s["id"].as_str().unwrap_or("?").to_string();
        if only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        match run(&s).await {
            Outcome::Pass => pass += 1,
            Outcome::Skipped => skipped += 1,
            Outcome::Pending(why) => pending.push(format!("{id} ({file}): {why}")),
            Outcome::Fail(p) => failures.push(format!("{id} {} ({file})\n  {}", s["title"].as_str().unwrap_or(""), p.join("\n  "))),
        }
    }
    eprintln!("\nscenarios: {pass} passed, {} failed, {} pending, {skipped} skipped (platform)", failures.len(), pending.len());
    if std::env::var("APM_SHOW_PENDING").is_ok() {
        for p in &pending {
            eprintln!("  PENDING {p}");
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
