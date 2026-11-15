//! Phase 0 spike (b): portable-pty + alacritty_terminal grid rendered by GPUI.
//! Set SPIKE_AUTOTEST=1 to type a command, check the grid for its output, print PASS/FAIL and exit.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::StreamExt;
use gpui::{
    div, font, prelude::*, px, rgb, size, App, Bounds, Context, FocusHandle, Font, FontWeight, Hsla, KeyDownEvent,
    Pixels, StyledText, TextRun, Window, WindowBounds, WindowOptions,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};

const FONT_SIZE: f32 = 14.0;
const PAD: f32 = 8.0;

#[derive(Clone)]
struct Listener(UnboundedSender<String>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // Replies the terminal owes the shell (e.g. ConPTY's cursor-position query).
        if let Event::PtyWrite(text) = event {
            let _ = self.0.unbounded_send(text);
        }
    }
}

struct TermView {
    term: Term<Listener>,
    processor: Processor,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    focus: FocusHandle,
    cols: usize,
    rows: usize,
    font: Font,
}

const ANSI: [u32; 16] = [
    0x1e1e1e, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c, 0x23d18b,
    0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
];
const FG: u32 = 0xd4d4d4;
const BG: u32 = 0x18181b;

fn indexed(i: u8) -> u32 {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let i = i - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            (lv(i / 36) << 16) | (lv((i / 6) % 6) << 8) | lv(i % 6)
        }
        _ => {
            let g = 8 + (i as u32 - 232) * 10;
            (g << 16) | (g << 8) | g
        }
    }
}

fn color(c: Color, default: u32) -> u32 {
    match c {
        Color::Spec(Rgb { r, g, b }) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        Color::Indexed(i) => indexed(i),
        Color::Named(n) => {
            let v = n as usize;
            if v < 16 {
                ANSI[v]
            } else if n == NamedColor::Foreground || n == NamedColor::BrightForeground {
                FG
            } else if n == NamedColor::Background {
                BG
            } else {
                default
            }
        }
    }
}

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

impl TermView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (reply_tx, mut reply_rx) = unbounded::<String>();
        let (cols, rows) = (100usize, 30usize);
        let term = Term::new(Config::default(), &TermSize::new(cols, rows), Listener(reply_tx));

        let pair = native_pty_system()
            .openpty(PtySize { rows: rows as u16, cols: cols as u16, pixel_width: 0, pixel_height: 0 })
            .expect("openpty");
        let mut cmd = CommandBuilder::new(if cfg!(windows) { "powershell.exe" } else { "/bin/zsh" });
        cmd.arg(if cfg!(windows) { "-NoLogo" } else { "-l" });
        let child = pair.slave.spawn_command(cmd).expect("spawn shell");
        Box::leak(Box::new(child)); // spike: shell lives as long as the window
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().expect("reader");
        let writer = pair.master.take_writer().expect("writer");

        let (out_tx, mut out_rx) = unbounded::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || out_tx.unbounded_send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        // PTY output -> parser -> repaint.
        cx.spawn(async move |this, cx| {
            while let Some(bytes) = out_rx.next().await {
                if this
                    .update(cx, |v, cx| {
                        v.processor.advance(&mut v.term, &bytes);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // Terminal replies -> PTY.
        cx.spawn(async move |this, cx| {
            while let Some(text) = reply_rx.next().await {
                let _ = this.update(cx, |v, _| v.send(text.as_bytes()));
            }
        })
        .detach();

        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            term,
            processor: Processor::new(),
            writer,
            master: pair.master,
            focus,
            cols,
            rows,
            font: font(if cfg!(windows) { "Cascadia Mono" } else { "Menlo" }),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    fn screen_text(&self) -> String {
        let mut lines = vec![String::new(); self.rows];
        for cell in self.term.grid().display_iter() {
            let line = cell.point.line.0;
            if line >= 0 && (line as usize) < self.rows {
                lines[line as usize].push(cell.c);
            }
        }
        lines.join("\n")
    }

    fn fit(&mut self, window: &mut Window) {
        let ts = window.text_system();
        let id = ts.resolve_font(&self.font);
        let cell_w = ts.advance(id, px(FONT_SIZE), 'm').map(|s| s.width).unwrap_or(px(8.0));
        let line_h = px(FONT_SIZE * 1.3);
        let vp = window.viewport_size();
        let cols = (((vp.width - px(PAD * 2.0)) / cell_w).floor() as usize).max(20);
        let rows = (((vp.height - px(PAD * 2.0)) / line_h).floor() as usize).max(5);
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            self.term.resize(TermSize::new(cols, rows));
            let _ = self.master.resize(PtySize { rows: rows as u16, cols: cols as u16, pixel_width: 0, pixel_height: 0 });
        }
    }

    fn key(&mut self, ev: &KeyDownEvent) {
        let k = &ev.keystroke;
        let seq: Option<Vec<u8>> = match k.key.as_str() {
            "enter" => Some(b"\r".to_vec()),
            "backspace" => Some(b"\x7f".to_vec()),
            "tab" => Some(b"\t".to_vec()),
            "escape" => Some(b"\x1b".to_vec()),
            "up" => Some(b"\x1b[A".to_vec()),
            "down" => Some(b"\x1b[B".to_vec()),
            "right" => Some(b"\x1b[C".to_vec()),
            "left" => Some(b"\x1b[D".to_vec()),
            "home" => Some(b"\x1b[H".to_vec()),
            "end" => Some(b"\x1b[F".to_vec()),
            "delete" => Some(b"\x1b[3~".to_vec()),
            "pageup" => Some(b"\x1b[5~".to_vec()),
            "pagedown" => Some(b"\x1b[6~".to_vec()),
            key if k.modifiers.control && key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() => {
                Some(vec![key.as_bytes()[0].to_ascii_lowercase() & 0x1f])
            }
            _ => k.key_char.as_ref().map(|s| s.as_bytes().to_vec()),
        };
        if let Some(s) = seq {
            self.send(&s);
        }
    }
}

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fit(window);
        let content = self.term.renderable_content();
        let cursor = content.cursor.point;
        let mut rows: Vec<Vec<(char, u32, u32, bool)>> = vec![Vec::with_capacity(self.cols); self.rows];
        for cell in content.display_iter {
            let line = cell.point.line.0 + content.display_offset as i32;
            if line < 0 || line as usize >= self.rows || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let (mut fg, mut bg) = (color(cell.fg, FG), color(cell.bg, BG));
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.point == cursor {
                std::mem::swap(&mut fg, &mut bg);
                if fg == bg {
                    bg = FG;
                    fg = BG;
                }
            }
            let c = if cell.c == '\0' { ' ' } else { cell.c };
            rows[line as usize].push((c, fg, bg, cell.flags.contains(Flags::BOLD)));
        }

        let line_h: Pixels = px(FONT_SIZE * 1.3);
        let lines = rows.into_iter().map(|cells| {
            let mut text = String::new();
            let mut runs: Vec<TextRun> = Vec::new();
            for (c, fg, bg, bold) in cells {
                let start = text.len();
                text.push(c);
                let len = text.len() - start;
                let font = Font { weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL }, ..self.font.clone() };
                let bg_c = if bg == BG { None } else { Some(hsla(bg)) };
                match runs.last_mut() {
                    Some(r) if r.color == hsla(fg) && r.background_color == bg_c && r.font == font => r.len += len,
                    _ => runs.push(TextRun { len, font, color: hsla(fg), background_color: bg_c, underline: None, strikethrough: None }),
                }
            }
            div().h(line_h).child(StyledText::new(text).with_runs(runs))
        });

        div()
            .size_full()
            .bg(rgb(BG))
            .p(px(PAD))
            .text_size(px(FONT_SIZE))
            .line_height(line_h)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, _| this.key(ev)))
            .children(lines)
    }
}

fn main() {
    let autotest = std::env::var("SPIKE_AUTOTEST").is_ok();
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(640.)), cx);
        let win = cx
            .open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |w, cx| {
                cx.new(|cx| TermView::new(w, cx))
            })
            .expect("window");
        cx.activate(true);
        if autotest {
            cx.spawn(async move |cx| {
                let exec = cx.background_executor().clone();
                let wait = move |ms| exec.timer(std::time::Duration::from_millis(ms));
                wait(3000).await;
                let _ = win.update(cx, |v, _, _| v.send(b"echo spike-$(6*7); Write-Host -ForegroundColor Green colour-check\r"));
                let mut ok = false;
                for _ in 0..40 {
                    wait(250).await;
                    ok = win.update(cx, |v, _, _| v.screen_text().lines().any(|l| l.trim_end() == "spike-42")).unwrap_or(false);
                    if ok {
                        break;
                    }
                }
                println!("{}", if ok { "PASS: PTY round trip, output parsed into grid" } else { "FAIL: output not seen" });
                if let Some(path) = std::env::var_os("SPIKE_DUMP") {
                    let _ = win.update(cx, |v, _, _| std::fs::write(path, v.screen_text()));
                }
                wait(1500).await; // leave time for a screenshot
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
}
