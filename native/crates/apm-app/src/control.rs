//! Test control channel (`test-hooks` feature, `APM_TEST_MODE=1` + `APM_TEST_CONTROL_PORT`).
//!
//! The UI flow runner drives controls through UI Automation, like a screen reader. What UI
//! Automation cannot do without injecting real OS input (which would disturb whoever is using the
//! PC) goes through this channel instead, one JSON object per line on 127.0.0.1:
//!
//! - `{"call": "profiles:getAll", "args": []}`  -> `{"ok": <result>}` / `{"err": "..."}` (a flow's backend step)
//! - `{"key": "Enter"}`                          -> the key is dispatched inside the main window
//! - `{"type": "text"}`                          -> typed into the focused element
//! - `{"hold": "Space", "ms": 1200}`             -> key down on the focused element, wait, key up (press and hold)
//! - `{"idle": true}`                            -> returns once no core call is in progress
//! - `{"events": true}`                          -> `{"ok": [[channel, payload...], ...]}` emitted since the last ask
//!   (stream channels excepted, as in the Electron harness)
//! - `{"stream": "terminal:data"}`               -> `{"ok": [chunk, ...]}` everything a stream channel emitted
//!   since the last `clearStreams` (not taken: the runner polls it)
//! - `{"clearStreams": true}`                    -> forget recorded stream output
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
    /// High-volume channels ([`STREAM_CHANNELS`]), kept apart from `events`.
    pub streams: std::collections::HashMap<String, Vec<Value>>,
    pub opens: Vec<Value>,
    /// Answers for the next file dialogs (a flow's `dialogs` step); `None` cancels.
    pub dialogs: std::collections::VecDeque<Option<String>>,
}

pub type Shared = Arc<Mutex<Recorded>>;

/// Channels recorded as streams (tests/parity/support/electronApp.ts `STREAM_CHANNELS`).
pub const STREAM_CHANNELS: &[&str] = &["terminal:data", "ai:chat-chunk"];

impl Recorded {
    /// One emitted event, filed as a stream chunk or an event.
    pub fn record(&mut self, channel: &str, payload: &[Value]) {
        if STREAM_CHANNELS.contains(&channel) {
            let item = if payload.len() == 1 { payload[0].clone() } else { Value::Array(payload.to_vec()) };
            self.streams.entry(channel.to_string()).or_default().push(item);
            return;
        }
        let mut ev = vec![Value::String(channel.to_string())];
        ev.extend(payload.iter().cloned());
        self.events.push(Value::Array(ev));
    }
}

enum UiCmd {
    Key(String),
    Type(String),
    /// Only the key-down (or key-up) event of a key: a press and hold is a down, a wait, an up.
    KeyDown(String),
    KeyUp(String),
    /// A round trip through the UI thread: everything queued before it has run.
    Sync,
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
            UiCmd::Sync => window.refresh(),
            UiCmd::Key(k) => {
                window.dispatch_keystroke(keystroke(&k), cx);
            }
            UiCmd::KeyDown(k) => {
                let keystroke = keystroke(&k);
                window.dispatch_event(gpui::PlatformInput::KeyDown(gpui::KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }), cx);
            }
            UiCmd::KeyUp(k) => {
                let keystroke = keystroke(&k);
                window.dispatch_event(gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }), cx);
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
    if cmd.get("idle").is_some() {
        // Settled when no core call has been in progress for a few checks in a row (a click's
        // call, then the reload it starts, then the render).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for _ in 0..2 {
            // Queued accessibility actions and renders first, then the core calls they started.
            let _ = ui(UiCmd::Sync);
            let mut quiet = 0;
            while quiet < 4 && std::time::Instant::now() < deadline {
                quiet = if bridge.in_flight() == 0 { quiet + 1 } else { 0 };
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        return json!({ "ok": null });
    }
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
    if let Some(k) = cmd.get("hold").and_then(Value::as_str) {
        // The UI thread keeps running while this connection's thread waits between down and up.
        let ms = cmd.get("ms").and_then(Value::as_u64).unwrap_or(1200);
        let down = ui(UiCmd::KeyDown(k.to_string()));
        if down.get("err").is_some() {
            return down;
        }
        std::thread::sleep(std::time::Duration::from_millis(ms));
        return ui(UiCmd::KeyUp(k.to_string()));
    }
    if cmd.get("events").is_some() {
        // Taken, like the Electron harness: each ask returns what arrived since the last one.
        return json!({ "ok": recorded.lock().map(|mut r| std::mem::take(&mut r.events)).unwrap_or_default() });
    }
    if let Some(channel) = cmd.get("stream").and_then(Value::as_str) {
        return json!({ "ok": recorded.lock().map(|r| r.streams.get(channel).cloned().unwrap_or_default()).unwrap_or_default() });
    }
    if cmd.get("clearStreams").is_some() {
        if let Ok(mut r) = recorded.lock() {
            r.streams.clear();
        }
        return json!({ "ok": null });
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
