//! The embedded terminal's shell process. Port of `ptyService.ts`: one PTY per window, output
//! streamed to the window as `terminal:data`, start failures reported as `terminal:error`.
//!
//! 1.x used winpty on Windows (`useConpty: false`) to dodge node-pty's ConPTY problems; 2.0 uses
//! ConPTY through `portable-pty` (Phase 0 spike b), which handles keyboard input correctly.
//! Terminal emulation (the screen grid) lives in the UI crate; this is only the process and bytes.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::Value;

use crate::core::Core;
use crate::env::Platform;
use crate::host::Host;

/// Git Bash on Windows: a lone Enter repeated within this window is dropped (1.x workaround for
/// double submits). Kept for parity.
const BASH_ENTER_THROTTLE_MS: i64 = 600;

pub struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    shell: String,
    last_bash_enter_ms: i64,
}

#[derive(Default)]
pub struct TerminalState {
    session: Mutex<Option<TerminalSession>>,
}

/// Shell and arguments for the shell type. "powershell" is the system shell (login shell
/// elsewhere); "bash" is Git Bash at settings.bashPath on Windows (PowerShell when unset), else
/// bashPath or /bin/bash. Non-Windows shells start as login shells so PATH has Homebrew etc.
fn shell_and_args(platform: Platform, shell: &str, bash_path: &str) -> (String, Vec<String>) {
    let bash_path = crate::js::trim(bash_path);
    if shell == "bash" {
        if platform == Platform::Windows {
            if !bash_path.is_empty() {
                return (bash_path.to_string(), vec!["--login".into(), "-i".into()]);
            }
            return ("powershell.exe".into(), vec![]);
        }
        let b = if bash_path.is_empty() { "/bin/bash" } else { bash_path };
        return (b.to_string(), vec!["-l".into()]);
    }
    if platform == Platform::Windows {
        return ("powershell.exe".into(), vec![]);
    }
    let default = if platform == Platform::MacOs { "/bin/zsh" } else { "/bin/bash" };
    (std::env::var("SHELL").unwrap_or_else(|_| default.to_string()), vec!["-l".into()])
}

/// ConPTY asks for the cursor position (DSR, `ESC[6n`) when it starts and renders nothing until
/// it gets an answer. The core answers that first request itself and keeps it out of the window's
/// stream, so every front end (and the tests) works without its own emulator replying.
const DSR: &[u8] = b"\x1b[6n";
const DSR_REPLY: &[u8] = b"\x1b[1;1R";

/// Streams PTY output as UTF-8 text, holding back a code point split across reads.
fn pump_output(mut reader: Box<dyn Read + Send>, writer: Arc<Mutex<Box<dyn Write + Send>>>, host: Arc<dyn Host>) {
    let mut buf = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    let mut answered_dsr = false;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&buf[..n]);
        if !answered_dsr {
            if let Some(at) = pending.windows(DSR.len()).position(|w| w == DSR) {
                pending.drain(at..at + DSR.len());
                let mut w = writer.lock().unwrap();
                let _ = w.write_all(DSR_REPLY);
                let _ = w.flush();
                answered_dsr = true;
            }
        }
        let valid_up_to = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            // Invalid bytes (not just a split sequence): emit lossily rather than stall.
            Err(_) => pending.len(),
        };
        if valid_up_to > 0 {
            let text = String::from_utf8_lossy(&pending[..valid_up_to]).into_owned();
            pending.drain(..valid_up_to);
            host.emit("terminal:data", vec![Value::String(text)]);
        }
    }
}

impl Core {
    /// Start (or restart) the window's shell. Errors become a `terminal:error` event.
    pub fn terminal_start(&self, shell: Option<&str>) {
        self.terminal_dispose();
        let settings = self.settings().get();
        let shell_type = shell
            .map(str::to_string)
            .or_else(|| settings.get("terminalShell").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "powershell".into());
        let bash_path = settings.get("bashPath").and_then(Value::as_str).unwrap_or("");
        let (program, args) = shell_and_args(self.cfg.platform, &shell_type, bash_path);

        match self.spawn_pty(&program, &args, &shell_type) {
            Ok(session) => {
                *self.terminal.session.lock().unwrap() = Some(session);
            }
            Err(message) => self.host.emit("terminal:error", vec![Value::String(message)]),
        }
    }

    fn spawn_pty(&self, program: &str, args: &[String], shell_type: &str) -> Result<TerminalSession, String> {
        // portable-pty reports a missing program only once the process fails to launch; check
        // explicit paths up front so the error is immediate and clear.
        if (program.contains('/') || program.contains('\\')) && !std::path::Path::new(program).exists() {
            return Err(format!("File not found: {program}"));
        }
        let pty = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        let home = crate::env::home_dir();
        if !home.as_os_str().is_empty() {
            cmd.cwd(home);
        }
        cmd.env("TERM", "xterm-256color");
        if self.cfg.platform == Platform::Windows && shell_type == "bash" {
            cmd.env("MSYSTEM", "MINGW64");
        }
        if self.cfg.platform != Platform::Windows {
            if std::env::var_os("LANG").is_none() {
                cmd.env("LANG", "en_US.UTF-8");
            }
            cmd.env("TERM_PROGRAM", "AWSProfileManager");
        }
        let child = pty.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pty.slave);
        let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(pty.master.take_writer().map_err(|e| e.to_string())?));

        let host = self.host.clone();
        let reply_writer = writer.clone();
        std::thread::Builder::new().name("apm-terminal-output".into()).spawn(move || pump_output(reader, reply_writer, host)).map_err(|e| e.to_string())?;

        Ok(TerminalSession { master: pty.master, writer, child, shell: shell_type.to_string(), last_bash_enter_ms: 0 })
    }

    /// Input from the window. Git Bash on Windows wants a lone CR for Enter.
    pub fn terminal_write(&self, data: &str) {
        let mut guard = self.terminal.session.lock().unwrap();
        let Some(session) = guard.as_mut() else { return };
        let mut out = data.to_string();
        if self.cfg.platform == Platform::Windows && session.shell == "bash" {
            out = out.replace("\r\n", "\r").replace('\n', "\r");
            while out.contains("\r\r") {
                out = out.replace("\r\r", "\r");
            }
            if out == "\r" {
                let now = self.cfg.clock.now_ms();
                if now - session.last_bash_enter_ms < BASH_ENTER_THROTTLE_MS {
                    return;
                }
                session.last_bash_enter_ms = now;
            }
        }
        let mut w = session.writer.lock().unwrap();
        let _ = w.write_all(out.as_bytes());
        let _ = w.flush();
    }

    pub fn terminal_resize(&self, cols: u16, rows: u16) {
        if let Some(s) = self.terminal.session.lock().unwrap().as_ref() {
            let _ = s.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// Kill the shell (window closed, or a restart).
    pub fn terminal_dispose(&self) {
        if let Some(mut s) = self.terminal.session.lock().unwrap().take() {
            let _ = s.child.kill();
        }
    }

    pub(crate) fn dispatch_terminal(&self, channel: &str, args: &[Value]) -> Option<crate::core::DispatchResult> {
        let ok = Some(Ok(Value::Null));
        match channel {
            "terminal:start" => {
                self.terminal_start(args.first().and_then(|o| o.get("shell")).and_then(Value::as_str));
                ok
            }
            "terminal:write" => {
                self.terminal_write(args.first().and_then(Value::as_str).unwrap_or(""));
                ok
            }
            "terminal:resize" => {
                let n = |i: usize| args.get(i).and_then(Value::as_f64).map(|v| v.max(1.0).min(u16::MAX as f64) as u16);
                if let (Some(c), Some(r)) = (n(0), n(1)) {
                    self.terminal_resize(c, r);
                }
                ok
            }
            _ => None,
        }
    }
}

impl Drop for TerminalState {
    fn drop(&mut self) {
        if let Some(mut s) = self.session.lock().unwrap().take() {
            let _ = s.child.kill();
        }
    }
}
