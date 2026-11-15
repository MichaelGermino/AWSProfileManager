//! Test control channel (`test-hooks` feature, `APM_TEST_MODE=1` + `APM_TEST_CONTROL_PORT`).
//!
//! The UI flow runner drives controls through UI Automation, like a screen reader. What UI
//! Automation cannot do without injecting real OS input (which would disturb whoever is using the
//! PC) goes through this channel instead, one JSON object per line on 127.0.0.1:
//!
//! - `{"call": "profiles:getAll", "args": []}`  -> `{"ok": <result>}` / `{"err": "..."}` (a flow's backend step)
//! - `{"key": "Enter"}`                          -> the key is dispatched inside the main window
//! - `{"type": "text"}`                          -> typed into the focused element
//! - `{"events": true}`                          -> `{"ok": [[channel, payload...], ...]}` emitted so far
//! - `{"opens": true}`                           -> `{"ok": [{kind, target}, ...]}` external opens since the last ask
//! - `{"dialogs": ["path", null]}`               -> answers for the next file dialogs
//!
//! Never compiled into release packages (built with `--no-default-features`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use apm_ui::bridge::Bridge;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use gpui::{App, AsyncApp, Keystroke, Modifiers};
use serde_json::{json, Value};

/// What the app host recorded for the runner.
#[derive(Default)]
pub struct Recorded {
    pub events: Vec<Value>,
    pub opens: Vec<Value>,
    /// Answers for the next file dialogs (a flow's `dialogs` step); `None` cancels.
    pub dialogs: std::collections::VecDeque<Option<String>>,
}

pub type Shared = Arc<Mutex<Recorded>>;

enum UiCmd {
    Key(String),
    Type(String),
}

/// Playwright key names to GPUI keystrokes.
fn keystroke(name: &str) -> Keystroke {
    let mut parts: Vec<&str> = name.split('+').collect();
    let key = parts.pop().unwrap_or("");
    let mut modifiers = Modifiers::default();
    for m in parts {
        match m {
            "Control" | "Ctrl" => modifiers.control = true,
            "Shift" => modifiers.shift = true,
            "Alt" => modifiers.alt = true,
            "Meta" => modifiers.platform = true,
            _ => {}
        }
    }
    let (key, ch) = match key {
        "Enter" => ("enter", None),
        "Escape" => ("escape", None),
        "Tab" => ("tab", None),
        "Backspace" => ("backspace", None),
        "Delete" => ("delete", None),
        "Space" | " " => ("space", Some(" ")),
        "ArrowDown" => ("down", None),
        "ArrowUp" => ("up", None),
        "ArrowLeft" => ("left", None),
        "ArrowRight" => ("right", None),
        "Home" => ("home", None),
        "End" => ("end", None),
        other => (other, None),
    };
    let key = key.to_lowercase();
    let key_char = ch.map(str::to_string).or_else(|| (key.chars().count() == 1 && !modifiers.control && !modifiers.alt).then(|| key.clone()));
    Keystroke { modifiers, key, key_char }
}

fn char_keystroke(c: char) -> Keystroke {
    let s = c.to_string();
    Keystroke { modifiers: Modifiers { shift: c.is_uppercase(), ..Default::default() }, key: s.to_lowercase(), key_char: Some(s) }
}

fn run_ui(cmd: UiCmd, cx: &mut App) -> Result<(), String> {
    let window = cx.windows().first().cloned().ok_or("no window")?;
    window
        .update(cx, |_, window, cx| match cmd {
            UiCmd::Key(k) => {
                window.dispatch_keystroke(keystroke(&k), cx);
            }
            UiCmd::Type(text) => {
                for c in text.chars() {
                    window.dispatch_keystroke(char_keystroke(c), cx);
                }
            }
        })
        .map_err(|e| e.to_string())
}

/// Start the channel when the hook is on. Call from inside `Application::run`.
pub fn start(bridge: Bridge, recorded: Shared, cx: &mut App) {
    let Some(port) = std::env::var("APM_TEST_CONTROL_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else { return };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("test control: cannot listen on {port}: {e}");
            return;
        }
    };
    let (ui_tx, mut ui_rx) = mpsc::unbounded::<(UiCmd, oneshot::Sender<Result<(), String>>)>();
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some((cmd, reply)) = ui_rx.next().await {
            let r = cx.update(|cx| run_ui(cmd, cx));
            let _ = reply.send(r);
        }
    })
    .detach();

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (bridge, recorded, ui_tx) = (bridge.clone(), recorded.clone(), ui_tx.clone());
            std::thread::spawn(move || {
                let mut out = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let reply = match serde_json::from_str::<Value>(&line) {
                        Err(e) => json!({ "err": format!("bad command: {e}") }),
                        Ok(cmd) => handle(&cmd, &bridge, &recorded, &ui_tx),
                    };
                    if writeln!(out, "{reply}").is_err() {
                        break;
                    }
                }
            });
        }
    });
}

fn handle(
    cmd: &Value,
    bridge: &Bridge,
    recorded: &Shared,
    ui_tx: &mpsc::UnboundedSender<(UiCmd, oneshot::Sender<Result<(), String>>)>,
) -> Value {
    let ui = |c: UiCmd| -> Value {
        let (tx, rx) = oneshot::channel();
        if ui_tx.unbounded_send((c, tx)).is_err() {
            return json!({ "err": "app is shutting down" });
        }
        match futures::executor::block_on(rx) {
            Ok(Ok(())) => json!({ "ok": null }),
            Ok(Err(e)) => json!({ "err": e }),
            Err(_) => json!({ "err": "no reply" }),
        }
    };
    if let Some(channel) = cmd.get("call").and_then(Value::as_str) {
        let args = cmd.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
        return match futures::executor::block_on(bridge.call(channel, args)) {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "err": e }),
        };
    }
    if let Some(k) = cmd.get("key").and_then(Value::as_str) {
        return ui(UiCmd::Key(k.to_string()));
    }
    if let Some(t) = cmd.get("type").and_then(Value::as_str) {
        return ui(UiCmd::Type(t.to_string()));
    }
    if cmd.get("events").is_some() {
        return json!({ "ok": recorded.lock().map(|r| r.events.clone()).unwrap_or_default() });
    }
    if cmd.get("opens").is_some() {
        return json!({ "ok": recorded.lock().map(|mut r| std::mem::take(&mut r.opens)).unwrap_or_default() });
    }
    if let Some(Value::Array(answers)) = cmd.get("dialogs") {
        if let Ok(mut r) = recorded.lock() {
            r.dialogs.extend(answers.iter().map(|a| a.as_str().map(str::to_string)));
        }
        return json!({ "ok": null });
    }
    json!({ "err": "unknown command" })
}
