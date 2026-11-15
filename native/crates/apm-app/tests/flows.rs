//! Layer C for 2.0 (docs/native-rewrite-plan.md §5.1): the shared UI flows in
//! `native/tests/fixtures/flows`, run against the built GPUI app through Windows UI Automation.
//! The Electron runner (tests/parity/scenarios.spec.ts + support/uiSteps.ts) runs the same files.
//!
//! The app is driven only the way assistive technology drives it: elements are found by role and
//! accessible name and operated through UI Automation patterns (Invoke, Value, Toggle,
//! ExpandCollapse, SelectionItem). No mouse or keyboard input is injected into the desktop, so the
//! suite can run while someone uses the PC; the window opens off-screen and unfocused. Keys,
//! typed text and a flow's backend `call` steps go through the app's test control channel
//! (apm-app/src/control.rs).
//!
//! Needs a built app: `cargo build -p apm-app --profile fastrel` (or set APM_APP_EXE).
//! `APM_FLOW=UI-FOLDERS-002` runs one flow; `APM_FLOW_KEEP=1` keeps going after a failure.

#![cfg(windows)]

#[path = "../../apm-core/tests/common.rs"]
mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use apm_core::secrets::{Keychain, SecretStore};
use common::*;
use regex::Regex;
use serde_json::{json, Value};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UISelectionItemPattern, UITogglePattern, UIValuePattern,
};
use uiautomation::types::{ExpandCollapseState, ToggleState, TreeScope};
use uiautomation::{UIAutomation, UIElement};

const TIMEOUT: Duration = Duration::from_secs(6);

// ------------------------------------------------------------------------------------------------
// The running app
// ------------------------------------------------------------------------------------------------

fn app_exe() -> PathBuf {
    if let Ok(p) = std::env::var("APM_APP_EXE") {
        return p.into();
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    for profile in ["fastrel", "release", "debug"] {
        let p = target.join(profile).join("aws-profile-manager.exe");
        if p.exists() {
            return p;
        }
    }
    panic!("build the app first: cargo build -p apm-app --profile fastrel");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct App {
    child: Child,
    control: BufReader<TcpStream>,
    window: UIElement,
}

impl App {
    fn launch(env: &HashMap<String, String>) -> Result<App, String> {
        let port = free_port();
        let mut cmd = Command::new(app_exe());
        cmd.envs(env).env("APM_TEST_MODE", "1").env("APM_WINDOW_OFFSCREEN", "1").env("APM_TEST_CONTROL_PORT", port.to_string());
        let child = cmd.spawn().map_err(|e| format!("start app: {e}"))?;
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(20);
        let control = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            if Instant::now() > deadline {
                return Err("app did not open its control channel".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        control.set_read_timeout(Some(Duration::from_secs(60))).ok();
        let uia = uia();
        let window = loop {
            let root = uia.get_root_element().map_err(|e| e.to_string())?;
            let cond = uia.create_true_condition().map_err(|e| e.to_string())?;
            let found = root
                .find_all(TreeScope::Children, &cond)
                .unwrap_or_default()
                .into_iter()
                .find(|w| w.get_process_id().ok() == Some(pid));
            if let Some(w) = found {
                break w;
            }
            if Instant::now() > deadline {
                return Err("app window did not appear".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        Ok(App { child, control: BufReader::new(control), window })
    }

    fn send(&mut self, cmd: Value) -> Result<Value, String> {
        let stream = self.control.get_mut();
        writeln!(stream, "{cmd}").map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.control.read_line(&mut line).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("control reply {line:?}: {e}"))?;
        match v.get("err") {
            Some(e) => Err(e.as_str().unwrap_or("error").to_string()),
            None => Ok(v.get("ok").cloned().unwrap_or(Value::Null)),
        }
    }

    fn close(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

thread_local! {
    static UIA: UIAutomation = UIAutomation::new().expect("UI Automation");
}

fn uia() -> UIAutomation {
    UIA.with(|u| u.clone())
}

// ------------------------------------------------------------------------------------------------
// Locators: ARIA role + accessible name (flows/README.md, "Role mapping for the GPUI runner")
// ------------------------------------------------------------------------------------------------

/// UI Automation's localized control type names for the flow roles (AccessKit's mapping; the
/// runner expects an English Windows display language).
fn role_types(role: &str) -> &'static [&'static str] {
    match role {
        "button" => &["button"],
        "link" => &["link"],
        "textbox" | "searchbox" => &["edit"],
        "spinbutton" => &["spinner", "edit"],
        "checkbox" => &["check box"],
        "switch" => &["button", "check box", "switch"],
        "combobox" => &["combo box"],
        "option" => &["list item"],
        "tab" => &["tab item"],
        "tablist" => &["tab"],
        "heading" => &["heading"],
        "dialog" => &["dialog"],
        "alertdialog" => &["alert dialog", "dialog"],
        "menu" => &["menu"],
        "menuitem" => &["menu item"],
        "list" => &["list"],
        "listitem" => &["list item"],
        "group" => &["group"],
        "navigation" => &["navigation"],
        "main" => &["main"],
        "complementary" => &["complementary"],
        "status" => &["status"],
        "alert" => &["alert"],
        "img" => &["image"],
        _ => &[],
    }
}

fn name_matches(want: Option<&str>, got: &str) -> bool {
    match want {
        None => true,
        Some(w) => match w.strip_prefix("$match:") {
            Some(re) => Regex::new(re).map(|r| r.is_match(got)).unwrap_or(false),
            None => w == got,
        },
    }
}

fn describe(l: &Value) -> String {
    let mut s = l["role"].as_str().unwrap_or("?").to_string();
    if let Some(n) = l["name"].as_str() {
        s.push_str(&format!(" \"{n}\""));
    }
    if let Some(n) = l["nth"].as_u64() {
        s.push_str(&format!(" #{n}"));
    }
    s
}

/// Every element matching the locator right now (no waiting).
fn find_now(app: &App, l: &Value) -> Result<Vec<UIElement>, String> {
    let scopes = match l.get("within") {
        Some(w) => find_now(app, w)?,
        None => vec![app.window.clone()],
    };
    let uia = uia();
    let cond = uia.create_true_condition().map_err(|e| e.to_string())?;
    let types = role_types(l["role"].as_str().unwrap_or(""));
    let name = l["name"].as_str();
    let mut out = Vec::new();
    for scope in scopes {
        for el in scope.find_all(TreeScope::Descendants, &cond).unwrap_or_default() {
            let t = el.get_localized_control_type().unwrap_or_default();
            if !types.contains(&t.as_str()) {
                continue;
            }
            if name_matches(name, &el.get_name().unwrap_or_default()) {
                out.push(el);
            }
        }
    }
    if let Some(n) = l["nth"].as_u64() {
        return Ok(out.into_iter().nth(n as usize).into_iter().collect());
    }
    Ok(out)
}

/// The one element for a locator, waiting for it to appear.
fn resolve(app: &App, l: &Value) -> Result<UIElement, String> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let found = find_now(app, l)?;
        match found.len() {
            1 => return Ok(found.into_iter().next().unwrap()),
            n if n > 1 && Instant::now() > deadline => return Err(format!("{}: {n} elements match", describe(l))),
            _ if Instant::now() > deadline => return Err(format!("{}: not found", describe(l))),
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Activate an element the way a screen reader does.
fn activate(el: &UIElement) -> Result<(), String> {
    if let Ok(p) = el.get_pattern::<UIInvokePattern>() {
        return p.invoke().map_err(|e| e.to_string());
    }
    if let Ok(p) = el.get_pattern::<UITogglePattern>() {
        return p.toggle().map_err(|e| e.to_string());
    }
    if let Ok(p) = el.get_pattern::<UIExpandCollapsePattern>() {
        return match p.get_state() {
            Ok(ExpandCollapseState::Expanded) => p.collapse(),
            _ => p.expand(),
        }
        .map_err(|e| e.to_string());
    }
    if let Ok(p) = el.get_pattern::<UISelectionItemPattern>() {
        return p.select().map_err(|e| e.to_string());
    }
    Err(format!("\"{}\" supports no Invoke, Toggle, ExpandCollapse or SelectionItem pattern", el.get_name().unwrap_or_default()))
}

/// The element's own name plus every descendant's: what a reader announces inside it.
fn text_of(el: &UIElement) -> String {
    let uia = uia();
    let mut parts = vec![el.get_name().unwrap_or_default()];
    if let Ok(cond) = uia.create_true_condition() {
        for d in el.find_all(TreeScope::Descendants, &cond).unwrap_or_default() {
            parts.push(d.get_name().unwrap_or_default());
            if let Ok(v) = d.get_pattern::<UIValuePattern>() {
                parts.push(v.get_value().unwrap_or_default());
            }
        }
    }
    parts.join(" ")
}

fn state_ok(el: &UIElement, state: &str) -> Result<bool, String> {
    let toggle = || el.get_pattern::<UITogglePattern>().and_then(|p| p.get_toggle_state()).ok();
    let expand = || el.get_pattern::<UIExpandCollapsePattern>().and_then(|p| p.get_state()).ok();
    Ok(match state {
        "visible" => true,
        "enabled" => el.is_enabled().unwrap_or(true),
        "disabled" => !el.is_enabled().unwrap_or(true),
        "checked" | "pressed" => toggle() == Some(ToggleState::On),
        "unchecked" | "unpressed" => toggle() == Some(ToggleState::Off),
        "selected" => el.get_pattern::<UISelectionItemPattern>().and_then(|p| p.is_selected()).unwrap_or(false),
        "expanded" => expand() == Some(ExpandCollapseState::Expanded),
        "collapsed" => expand() == Some(ExpandCollapseState::Collapsed),
        "focused" => el.has_keyboard_focus().unwrap_or(false),
        other => return Err(format!("unknown state {other}")),
    })
}

fn wait_until(what: &str, mut f: impl FnMut() -> Result<bool, String>) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    let mut last = Ok(false);
    while Instant::now() < deadline {
        last = f();
        if matches!(last, Ok(true)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(match last {
        Err(e) => format!("{what}: {e}"),
        _ => format!("{what}: timed out"),
    })
}

fn fill(app: &mut App, el: &UIElement, value: &str) -> Result<(), String> {
    if let Ok(p) = el.get_pattern::<UIValuePattern>() {
        if p.set_value(value).is_ok() {
            return Ok(());
        }
    }
    // Password fields expose no value: focus through UI Automation, then replace the text with keys
    // dispatched inside the app (never the desktop's keyboard).
    el.set_focus().map_err(|e| format!("focus: {e}"))?;
    std::thread::sleep(Duration::from_millis(100));
    app.send(json!({ "key": "Control+a" }))?;
    app.send(json!({ "key": "Backspace" }))?;
    app.send(json!({ "type": value }))?;
    Ok(())
}

fn ui_step(app: &mut App, s: &Value) -> Result<(), String> {
    if let Some(l) = s.get("click") {
        if s.get("button").and_then(Value::as_str) == Some("right") {
            return Err("right-click has no UI Automation equivalent yet".into());
        }
        return activate(&resolve(app, l)?);
    }
    if let Some(l) = s.get("hover") {
        resolve(app, l)?;
        return Ok(()); // nothing to hover without a pointer; the element exists
    }
    if let Some(l) = s.get("focus") {
        return resolve(app, l)?.set_focus().map_err(|e| e.to_string());
    }
    if let Some(l) = s.get("fill") {
        let el = resolve(app, l)?;
        return fill(app, &el, s["value"].as_str().unwrap_or(""));
    }
    if let Some(l) = s.get("select") {
        let el = resolve(app, l)?;
        let want = s["option"].as_str().unwrap_or("").to_string();
        if let Ok(p) = el.get_pattern::<UIExpandCollapsePattern>() {
            let _ = p.expand();
        }
        let option = json!({ "role": "option", "name": want });
        return activate(&resolve(app, &option)?);
    }
    if let Some(k) = s.get("press").and_then(Value::as_str) {
        return app.send(json!({ "key": k })).map(drop);
    }
    if let Some(t) = s.get("type").and_then(Value::as_str) {
        return app.send(json!({ "type": t })).map(drop);
    }
    if let Some(l) = s.get("hold") {
        let el = resolve(app, l)?;
        el.set_focus().map_err(|e| e.to_string())?;
        let ms = s.get("ms").and_then(Value::as_u64).unwrap_or(1200);
        app.send(json!({ "hold": "Space", "ms": ms }))?;
        return Ok(());
    }
    if let Some(l) = s.get("see") {
        let label = describe(l);
        if let Some(n) = s.get("count").and_then(Value::as_u64) {
            wait_until(&format!("{label} count {n}"), || Ok(find_now(app, l)?.len() as u64 == n))?;
        }
        match s.get("state").and_then(Value::as_str) {
            Some("hidden") => wait_until(&format!("{label} hidden"), || Ok(find_now(app, l)?.is_empty()))?,
            Some(state) => {
                let el = resolve(app, l)?;
                wait_until(&format!("{label} {state}"), || state_ok(&el, state))?;
            }
            None if s.get("count").is_none() => {
                resolve(app, l)?;
            }
            None => {}
        }
        if let Some(t) = s.get("text").and_then(Value::as_str) {
            let el = resolve(app, l)?;
            wait_until(&format!("{label} text {t:?}"), || Ok(name_matches(Some(&format!("$match:{}", t.strip_prefix("$match:").map(str::to_string).unwrap_or_else(|| regex::escape(t)))), &text_of(&el))))?;
        }
        if let Some(v) = s.get("value").and_then(Value::as_str) {
            let el = resolve(app, l)?;
            wait_until(&format!("{label} value {v:?}"), || {
                let got = el.get_pattern::<UIValuePattern>().and_then(|p| p.get_value()).map_err(|e| e.to_string())?;
                Ok(got == v)
            })?;
        }
        return Ok(());
    }
    Err(format!("unknown UI step {s}"))
}

const UI_KEYS: &[&str] = &["click", "hover", "hold", "focus", "fill", "select", "press", "type", "see"];

// ------------------------------------------------------------------------------------------------
// One flow
// ------------------------------------------------------------------------------------------------

struct Run {
    sandbox: Sandbox,
    env: HashMap<String, String>,
    service: String,
    keychain: Keychain,
}

fn prepare(flow: &Value, fakes: Option<&Fakes>) -> Run {
    let id = flow["id"].as_str().unwrap_or("flow");
    let sandbox = Sandbox::new(id);
    let tokens: Vec<(&str, String)> = fakes.map(|f| vec![("FAKES", f.base_url.clone())]).unwrap_or_default();
    let given = with_tokens(flow.get("given").unwrap_or(&Value::Null), &tokens);
    if let Some(d) = given.get("dataDir").and_then(Value::as_str) {
        copy_dir(&fixtures().join("datadirs").join(d), &sandbox.paths.app_data);
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
    if let Some(c) = given.get("credentials") {
        write_given_file(&sandbox.paths.credentials_file, c);
    } else {
        std::fs::create_dir_all(sandbox.paths.credentials_file.parent().unwrap()).unwrap();
    }
    let service = format!("AWSProfileManager-flow-{id}-{}", std::process::id());
    let keychain = Keychain::new(&service);
    if let Some(kc) = given.get("keychain").and_then(Value::as_object) {
        for (account, v) in kc {
            keychain.set(account, &materialize_secret(v)).expect("seed keychain");
        }
    }
    let mut host_map = blackhole_host_map();
    if let Some(f) = fakes {
        host_map.extend(f.host_map.clone());
    }
    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("APM_APPDATA_DIR".into(), sandbox.paths.app_data.to_string_lossy().into_owned());
    env.insert("APM_AWS_CREDENTIALS_FILE".into(), sandbox.paths.credentials_file.to_string_lossy().into_owned());
    env.insert("APM_KEYCHAIN_SERVICE".into(), service.clone());
    env.insert("APM_HOST_MAP".into(), serde_json::to_string(&host_map).unwrap());
    if let Some(f) = fakes {
        env.insert("APM_TEST_CA_FILE".into(), f.ca_file.clone());
    }
    if let Some(e) = given.get("env").and_then(Value::as_object) {
        for (k, v) in e {
            env.insert(k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        }
    }
    Run { sandbox, env, service, keychain }
}

/// Let the app finish starting, then forget what it emitted so far (the Electron runner's settle).
fn settle(app: &mut App) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(800));
    app.send(json!({ "events": true }))?;
    app.send(json!({ "opens": true }))?;
    Ok(())
}

fn check_outcome(outcome: Result<Value, String>, step: &Value, label: &str, tokens: &[(&str, String)], refs: &HashMap<String, Value>) -> Vec<String> {
    match outcome {
        Err(e) => match step.get("expectError") {
            None => vec![format!("{label}: call failed: {e}")],
            Some(exp) => matches(&Value::String(e), &with_tokens(&resolve_refs(exp, refs), tokens), &format!("{label} error")),
        },
        Ok(v) => {
            if step.get("expectError").is_some() {
                return vec![format!("{label}: expected the call to fail, got {v}")];
            }
            match step.get("expect") {
                None => vec![],
                Some(exp) => matches(&v, &with_tokens(&resolve_refs(exp, refs), tokens), label),
            }
        }
    }
}

async fn run_flow(flow: &Value) -> Result<(), Vec<String>> {
    let fakes = match flow.get("given").and_then(|g| g.get("fakes")) {
        Some(cfg) => Some(start_fakes(&with_tokens(cfg, &[("APP_VERSION", app_version())])).await),
        None => None,
    };
    let run = prepare(flow, fakes.as_ref());
    let mut tokens: Vec<(&str, String)> = vec![("SANDBOX", run.sandbox.root.to_string_lossy().into_owned()), ("APP_VERSION", app_version())];
    if let Some(f) = &fakes {
        tokens.push(("FAKES", f.base_url.clone()));
        f.reset().await;
    }
    let mut app = App::launch(&run.env).map_err(|e| vec![e])?;
    settle(&mut app).map_err(|e| vec![e])?;
    if let Some(f) = &fakes {
        f.reset().await;
    }
    let mut refs: HashMap<String, Value> = HashMap::new();
    let mut result: Result<(), Vec<String>> = Ok(());
    for (i, step) in flow["steps"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let label = format!("step {} {}", i + 1, short(step));
        let step_t = with_tokens(step, &tokens);
        let problems: Vec<String> = if step.get("restart").is_some() {
            app.close();
            app = match App::launch(&run.env) {
                Ok(a) => a,
                Err(e) => return Err(vec![format!("{label}: restart: {e}")]),
            };
            settle(&mut app).err().into_iter().collect()
        } else if UI_KEYS.iter().any(|k| step.get(*k).is_some()) {
            let r = ui_step(&mut app, &step_t).and_then(|_| app.send(json!({ "idle": true })).map(drop));
            r.err().map(|e| format!("{label}: {e}")).into_iter().collect()
        } else if let Some(d) = step_t.get("dialogs") {
            app.send(json!({ "dialogs": d })).err().into_iter().collect()
        } else if let Some(exp) = step.get("expectEvents") {
            match app.send(json!({ "events": true })) {
                Ok(got) => matches(&got, &with_tokens(&resolve_refs(exp, &refs), &tokens), &format!("{label} events")),
                Err(e) => vec![e],
            }
        } else if let Some(exp) = step.get("expectExternalOpens") {
            if let Some(ms) = step.get("waitMs").and_then(Value::as_u64) {
                std::thread::sleep(Duration::from_millis(ms));
            }
            match app.send(json!({ "opens": true })) {
                Ok(got) => matches(&got, &with_tokens(&resolve_refs(exp, &refs), &tokens), &format!("{label} external opens")),
                Err(e) => vec![e],
            }
        } else if let Some(ms) = step.get("wait").and_then(Value::as_u64) {
            std::thread::sleep(Duration::from_millis(ms));
            vec![]
        } else if step.get("resetFakeRequests").is_some() {
            if let Some(f) = &fakes {
                f.reset().await;
            }
            vec![]
        } else if let Some(channel) = step.get("call").and_then(Value::as_str) {
            let args = with_tokens(&resolve_refs(step.get("args").unwrap_or(&json!([])), &refs), &tokens);
            let outcome = app.send(json!({ "call": channel, "args": args }));
            if let (Some(name), Ok(v)) = (step.get("save").and_then(Value::as_str), &outcome) {
                refs.insert(name.to_string(), v.clone());
            }
            check_outcome(outcome, step, &label, &tokens, &refs)
        } else {
            vec![format!("{label}: step not supported by the GPUI runner yet")]
        };
        if !problems.is_empty() {
            result = Err(problems);
            break;
        }
    }

    // then: files, credentials, keychain, fake requests
    if result.is_ok() {
        let then = flow.get("then").cloned().unwrap_or(Value::Null);
        let mut problems = Vec::new();
        let eol = "\r\n";
        for (name, exp) in then.get("files").and_then(Value::as_object).into_iter().flatten() {
            problems.extend(check_file(name, &run.sandbox.paths.app_data.join(name), &with_tokens(exp, &tokens), &refs, eol));
        }
        for (name, exp) in then.get("sandboxFiles").and_then(Value::as_object).into_iter().flatten() {
            problems.extend(check_file(name, &run.sandbox.root.join(name), &with_tokens(exp, &tokens), &refs, eol));
        }
        if let Some(exp) = then.get("credentials") {
            problems.extend(check_file("credentials", &run.sandbox.paths.credentials_file, &with_tokens(exp, &tokens), &refs, eol));
        }
        for (account, exp) in then.get("keychain").and_then(Value::as_object).into_iter().flatten() {
            let got = run.keychain.get(account);
            match (exp.as_str(), got) {
                (Some("$absent"), Some(_)) => problems.push(format!("keychain {account}: expected absent")),
                (Some("$absent"), None) => {}
                (_, None) => problems.push(format!("keychain {account}: missing")),
                (_, Some(v)) => problems.extend(matches(&Value::String(v), &resolve_refs(exp, &refs), &format!("keychain {account}"))),
            }
        }
        if let (Some(list), Some(f)) = (then.get("fakeRequests").and_then(Value::as_array), &fakes) {
            let got = f.requests().await;
            for item in list {
                let service = item["service"].as_str().unwrap_or("");
                let mine: Vec<Value> = got.as_array().into_iter().flatten().filter(|r| r["service"] == service).cloned().collect();
                problems.extend(matches(&Value::Array(mine), &with_tokens(&resolve_refs(&item["expect"], &refs), &tokens), &format!("fake {service} requests")));
            }
        }
        if !problems.is_empty() {
            result = Err(problems);
        }
    }
    app.close();
    // Every entry this flow could have written, under its own throwaway service.
    for a in run.keychain.touched().into_iter().chain(["__default__".to_string(), "__master_password_verifier__".to_string()]) {
        run.keychain.delete(&a);
    }
    let _ = run.service;
    result
}

fn load_flows() -> Vec<Value> {
    let dir = fixtures().join("flows");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    files.iter().flat_map(|p| serde_json::from_str::<Vec<Value>>(&std::fs::read_to_string(p).unwrap()).unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flows() {
    let only = std::env::var("APM_FLOW").ok();
    let mut failed = Vec::new();
    let mut passed = 0;
    let flows: Vec<Value> = load_flows()
        .into_iter()
        .filter(|f| only.as_deref().is_none_or(|o| f["id"].as_str().is_some_and(|id| o.split(',').any(|x| x == id))))
        .filter(|f| f.get("platforms").and_then(Value::as_array).is_none_or(|p| p.iter().any(|x| x == "win32")))
        .collect();
    for flow in &flows {
        let id = flow["id"].as_str().unwrap_or("?").to_string();
        let f = flow.clone();
        let outcome = tokio::task::spawn_blocking(move || tokio::runtime::Handle::current().block_on(run_flow(&f))).await.unwrap();
        match outcome {
            Ok(()) => {
                passed += 1;
                println!("PASS {id}");
            }
            Err(p) => {
                println!("FAIL {id}\n  {}", p.join("\n  "));
                failed.push(id);
            }
        }
    }
    println!("\n{passed} passed, {} failed of {}", failed.len(), flows.len());
    assert!(failed.is_empty(), "failed flows: {}", failed.join(", "));
}
