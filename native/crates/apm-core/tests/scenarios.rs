//! Layer B (docs/native-rewrite-plan.md §5.2): the backend scenarios in
//! `native/tests/fixtures/scenarios`, run against `apm_core::Core::dispatch`. The Electron runner
//! (tests/parity/scenarios.spec.ts) runs the same files through `window.electron`; the format and
//! the matching rules are in the scenarios README and tests/parity/support/scenario.ts.
//!
//! A channel `dispatch` does not know fails the scenario. `APM_SCENARIO=BE-PROF-003` runs one
//! scenario.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use apm_core::core::{Core, CoreConfig, DispatchError};
use apm_core::env::{Clock, Platform};
use apm_core::host::{DialogRequest, Host, NoSignInWindow, SignInRequest, SignInWindow};
use apm_core::json as jsjson;
use apm_core::secrets::{Keychain, SecretStore};
use regex::Regex;
use serde_json::{Map, Value};

mod common;
use common::*;

// ------------------------------------------------------------------------------------------------
// Recording host: events, external opens and scripted dialog answers.
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Value>>,
    streams: Mutex<HashMap<String, Vec<Value>>>,
    opens: Mutex<Vec<Value>>,
    dialogs: Mutex<VecDeque<Option<String>>>,
    /// The fakes' CA, trusted by the stand-in sign-in browser.
    test_ca: Option<String>,
}

impl Recorder {
    fn new(fakes: Option<&Fakes>) -> Arc<Self> {
        Arc::new(Recorder { test_ca: fakes.map(|f| f.ca_pem.clone()), ..Default::default() })
    }
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

    /// Nothing is started in tests; a specific browser counts as a URL the app asked to open.
    fn launch_browser(&self, _exe: &str, url: &str) -> Result<(), String> {
        self.open_external("url", url);
        Ok(())
    }

    /// Stand-in for the embedded sign-in window: load the authorize URL and follow its redirects
    /// down to the loopback callback, as a browser does when the IdP needs no input. A page that
    /// asks for input (authorizeNeedsInput) just sits there, like a hidden window would.
    fn open_sign_in(&self, request: SignInRequest, _closed_by_user: Box<dyn FnOnce() + Send>) -> Box<dyn SignInWindow> {
        let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(10));
        if let Some(pem) = &self.test_ca {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(pem.as_bytes()).unwrap());
        }
        let client = builder.build().unwrap();
        tokio::runtime::Handle::current().spawn(async move {
            let _ = client.get(&request.url).send().await;
        });
        Box::new(NoSignInWindow)
    }
}

// ------------------------------------------------------------------------------------------------
// Running one scenario
// ------------------------------------------------------------------------------------------------

enum Outcome {
    Pass,
    Fail(Vec<String>),
    Skipped,
}

fn new_core(sandbox: &Sandbox, clock: &Clock, host: Arc<Recorder>, service: &str, keychain: &Arc<Keychain>, fakes: Option<&Fakes>) -> Core {
    let version = app_version();
    let mut host_map = blackhole_host_map();
    if let Some(f) = fakes {
        host_map.extend(f.host_map.clone());
    }
    let cfg = CoreConfig {
        paths: sandbox.paths.clone(),
        platform: Platform::current(),
        keychain_service: service.to_string(),
        app_version: version.clone(),
        clock: clock.clone(),
        http: apm_core::http::HttpConfig { host_map, extra_ca_pem: fakes.map(|f| f.ca_pem.clone()), user_agent: format!("AWSProfileManager/{version}") },
        update_public_key: None,
        resources_dir: fixtures().join("../../../resources"),
    };
    let core = Core::with_secrets(cfg, host, keychain.clone());
    // The window starts with the app, on every launch (1.x: App.tsx's startup checks).
    core.window_started();
    core
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
    let raw_given = s.get("given").cloned().unwrap_or(Value::Object(Map::new()));
    let fakes = match raw_given.get("fakes") {
        Some(cfg) => Some(start_fakes(&with_tokens(cfg, &[("APP_VERSION", app_version())])).await),
        None => None,
    };
    let given = match &fakes {
        Some(f) => with_tokens(&raw_given, &[("FAKES", f.base_url.clone())]),
        None => raw_given,
    };
    // given.env: only the scheduler tick is a test hook in 2.0 (the rest are runner settings).
    let tick_ms = given.get("env").and_then(|e| e.get("APM_SCHEDULER_TICK_MS")).and_then(Value::as_str).and_then(|v| v.parse::<u64>().ok());

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
    // Process id in the name: parallel runs (worktrees) must not share keychain entries.
    let service = format!("AWSProfileManager-test-{id}-{}", std::process::id());
    let keychain = Arc::new(Keychain::new(&service));
    let mut cleanup_accounts: Vec<String> = ["__default__", "__default___username", "__master_password_verifier__"].iter().map(|s| s.to_string()).collect();
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
    let mut host = Recorder::new(fakes.as_ref());
    let mut core = (new_core(&sandbox, &clock, host.clone(), &service, &keychain, fakes.as_ref())).into_shared();
    core.start_scheduler(tick_ms);
    if let Some(t) = tick_ms {
        // Like the Electron runner's settle(): let start-up work (here the first scheduler ticks) run,
        // then begin each scenario with nothing recorded.
        tokio::time::sleep(std::time::Duration::from_millis(t * 3)).await;
        host.events.lock().unwrap().clear();
    }
    let mut inflight: HashMap<String, tokio::task::JoinHandle<Result<Value, DispatchError>>> = HashMap::new();
    let mut ready: HashMap<String, Result<Value, DispatchError>> = HashMap::new();
    let mut tokens = vec![("SANDBOX", sandbox.root.to_string_lossy().into_owned()), ("APP_VERSION", app_version())];
    if let Some(f) = &fakes {
        tokens.push(("FAKES", f.base_url.clone()));
    }
    let tok = |v: &Value| with_tokens(v, &tokens);
    let mut refs: HashMap<String, Value> = HashMap::new();
    let mut problems = Vec::new();
    let eol = Platform::current().eol();

    for (i, step) in s["steps"].as_array().unwrap().iter().enumerate() {
        let label = format!("step {}", i + 1);
        if step.get("restart").is_some() {
            host = Recorder::new(fakes.as_ref());
            core = (new_core(&sandbox, &clock, host.clone(), &service, &keychain, fakes.as_ref())).into_shared();
            core.start_scheduler(tick_ms);
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
        if let Some(stream) = step.get("waitForStream").and_then(Value::as_str) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(step.get("timeoutMs").and_then(Value::as_u64).unwrap_or(15_000));
            #[allow(clippy::regex_creation_in_loops)] // once per waitForStream step, not per poll
            let ansi = Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap();
            loop {
                let items = host.streams.lock().unwrap().get(stream).cloned().unwrap_or_default();
                let text: String = items.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| jsjson::stringify(x))).collect();
                let text = ansi.replace_all(&text, "");
                let count_ok = step.get("count").and_then(Value::as_u64).is_none_or(|c| items.len() as u64 >= c);
                let text_ok = step.get("text").and_then(Value::as_str).is_none_or(|t| text.contains(t));
                if count_ok && text_ok {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    return Outcome::Fail(vec![format!(
                        "{label}: stream {stream} did not reach the condition; got {:?}",
                        text.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()
                    )]);
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            continue;
        }
        if let Some(name) = step.get("await").and_then(Value::as_str) {
            let result = match (ready.remove(name), inflight.remove(name)) {
                (Some(r), _) => r,
                (None, Some(handle)) => handle.await.expect("async call panicked"),
                (None, None) => return Outcome::Fail(vec![format!("{label}: nothing started as {name:?}")]),
            };
            let outcome = match result {
                Err(e @ DispatchError::Unsupported(_)) => return Outcome::Fail(vec![format!("{label} await {name}: {e}")]),
                Err(DispatchError::Failed(m)) => Err(m),
                Ok(v) => Ok(v),
            };
            if let Some(save) = step.get("save").and_then(Value::as_str) {
                refs.insert(save.to_string(), outcome.clone().unwrap_or_else(|m| serde_json::json!({ "failed": m })));
            }
            match (outcome, step.get("expectError")) {
                (Err(m), Some(want)) => problems.extend(matches(&Value::String(m), &tok(&resolve_refs(want, &refs)), &format!("{label} error"))),
                (Err(m), None) => problems.push(format!("{label} await {name}: call failed: {m}")),
                (Ok(v), Some(_)) => problems.push(format!("{label} await {name}: expected the call to fail, got {}", short(&v))),
                (Ok(v), None) => {
                    if let Some(exp) = step.get("expect") {
                        problems.extend(matches(&v, &tok(&resolve_refs(exp, &refs)), &format!("{label} await {name}")));
                    }
                }
            }
            if !problems.is_empty() {
                return Outcome::Fail(problems);
            }
            continue;
        }
        if step.get("resetFakeRequests").is_some() {
            if let Some(f) = &fakes {
                f.reset().await;
            }
            continue;
        }
        let Some(channel) = step.get("call").and_then(Value::as_str) else {
            return Outcome::Fail(vec![format!("{label}: unknown step {}", short(step))]);
        };
        let args = tok(&resolve_refs(step.get("args").unwrap_or(&Value::Array(vec![])), &refs));
        let args: Vec<Value> = args.as_array().cloned().unwrap_or_default();
        if let Some(name) = step.get("async").and_then(Value::as_str) {
            let (c, ch) = (core.clone(), channel.to_string());
            let handle = tokio::spawn(async move { c.dispatch(&ch, &args).await });
            // Let the call get going before the next step (1.x: the invoke was already in flight).
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if handle.is_finished() {
                let r = handle.await.expect("async call panicked");
                if let Err(e @ DispatchError::Unsupported(_)) = &r {
                    return Outcome::Fail(vec![format!("{label} {channel}: {e}")]);
                }
                ready.insert(name.to_string(), r);
            } else {
                inflight.insert(name.to_string(), handle);
            }
            continue;
        }
        let result = core.dispatch(channel, &args).await;
        let outcome = match result {
            Err(e @ DispatchError::Unsupported(_)) => return Outcome::Fail(vec![format!("{label} {channel}: {e}")]),
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
    for want in then.get("fakeRequests").and_then(Value::as_array).cloned().unwrap_or_default() {
        let service = want["service"].as_str().unwrap_or("");
        let all = match &fakes {
            Some(f) => f.requests().await,
            None => Value::Array(vec![]),
        };
        let got: Vec<Value> = all.as_array().unwrap().iter().filter(|r| r["service"] == service).cloned().collect();
        problems.extend(matches(&Value::Array(got), &tok(&resolve_refs(&want["expect"], &refs)), &format!("fake {service} requests")));
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
    let (mut pass, mut skipped, mut failures) = (0, 0, Vec::new());
    for (file, s) in load(&fixtures().join("scenarios")) {
        let id = s["id"].as_str().unwrap_or("?").to_string();
        if only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        match run(&s).await {
            Outcome::Pass => pass += 1,
            Outcome::Skipped => skipped += 1,
            Outcome::Fail(p) => failures.push(format!("{id} {} ({file})\n  {}", s["title"].as_str().unwrap_or(""), p.join("\n  "))),
        }
    }
    eprintln!("\nscenarios: {pass} passed, {} failed, {skipped} skipped (platform)", failures.len());
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
