//! The embedded terminal: an `alacritty_terminal` grid fed with the core's PTY output and drawn by
//! GPUI. Port of 1.x `EmbeddedTerminal.tsx` (xterm.js).
//!
//! The core owns the shell process (`apm_core::terminal`, ConPTY through portable-pty) and emits
//! its output as `terminal:data`; the app's event pump hands those chunks, batched, to
//! [`deliver`]. Bytes go through alacritty's VT parser into the grid; painting walks only the
//! visible rows, merges cells into text runs and background rectangles, and shapes one line per
//! row (GPUI caches shaped lines between frames). Keyboard input becomes VT sequences written to
//! the PTY in order by one writer thread (calls spawned on the runtime could be reordered).
//!
//! Clean-room: Zed's terminal view (GPL) is the model for the approach only, no code from it.

use std::sync::{mpsc, Arc, Mutex};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{viewport_to_point, Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};
use apm_core::core::Core;
use gpui::{
    actions, canvas, div, fill, outline, point, prelude::*, px, size, App, Bounds, ClipboardItem, Context, FocusHandle, Focusable, Font, FontFeatures,
    FontStyle, FontWeight, Global, Hsla, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Role, ScrollEvent,
    SharedString, StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, WeakEntity, Window,
};

use crate::a11y::PressExt as _;
use crate::theme::{size as sz, Palette, FONT_MONO};

const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 17.0;
const PAD: f32 = 10.0;
/// xterm.js's default scrollback (1.x); alacritty's 10 000 would cost megabytes per terminal.
const SCROLLBACK: usize = 1000;
/// Below this the size is a collapsed layout, not a real one: resizing ConPTY that small rewraps
/// its whole buffer for good (1.x MIN_SANE_COLS / ROWS).
const MIN_COLS: usize = 20;
const MIN_ROWS: usize = 4;

actions!(terminal, [SendTab, SendBackTab, CtrlC, Copy, Paste]);

const CONTEXT: &str = "Terminal";

/// Keys the terminal takes before the window does (Root binds Tab to focus moves and Ctrl+C to
/// copy). The deeper key context wins, so these only apply while the terminal has focus.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", SendBackTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", CtrlC, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(CONTEXT)),
        KeyBinding::new("shift-insert", Paste, Some(CONTEXT)),
    ]);
}

/// The live terminal view, so the event pump can hand it output without going through the model.
struct Sink(WeakEntity<TerminalView>);
impl Global for Sink {}

/// PTY output from the core (`terminal:data`), already batched by the caller.
pub fn deliver(data: &str, cx: &mut App) {
    if let Some(view) = cx.try_global::<Sink>().and_then(|s| s.0.upgrade()) {
        view.update(cx, |v, cx| v.feed(data.as_bytes(), cx));
    }
}

/// The shell failed to start (`terminal:error`).
pub fn deliver_error(message: String, cx: &mut App) {
    if let Some(view) = cx.try_global::<Sink>().and_then(|s| s.0.upgrade()) {
        view.update(cx, |v, cx| {
            v.error = Some(message);
            cx.notify();
        });
    }
}

/// Replies the emulator owes the shell (cursor position reports and the like).
#[derive(Clone)]
struct Replies(Arc<Mutex<Vec<String>>>);

impl EventListener for Replies {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event {
            if let Ok(mut q) = self.0.lock() {
                q.push(text);
            }
        }
    }
}

/// Work for the PTY, done in order on one thread.
enum PtyCmd {
    Start(String),
    Write(String),
    Resize(u16, u16),
    Dispose,
}

fn spawn_writer(core: Arc<Core>) -> mpsc::Sender<PtyCmd> {
    let (tx, rx) = mpsc::channel::<PtyCmd>();
    let _ = std::thread::Builder::new().name("apm-terminal-input".into()).spawn(move || {
        for cmd in rx {
            match cmd {
                PtyCmd::Start(shell) => core.terminal_start(Some(&shell)),
                PtyCmd::Write(s) => core.terminal_write(&s),
                PtyCmd::Resize(c, r) => core.terminal_resize(c, r),
                PtyCmd::Dispose => core.terminal_dispose(),
            }
        }
    });
    tx
}

/// The 16 ANSI colours plus the defaults, per light / dark (1.x `TERMINAL_THEMES`, with the
/// background and text taken from the 2.0 palette so the terminal sits in the page).
#[derive(Clone, Copy)]
struct TermColors {
    fg: Hsla,
    bg: Hsla,
    cursor: Hsla,
    selection: Hsla,
    ansi: [u32; 16],
}

const ANSI_DARK: [u32; 16] = [
    0x27272a, 0xe5484d, 0x4cc38a, 0xf0b232, 0x6e9ef5, 0xc084fc, 0x3fb3a6, 0xd4d4d8, 0x71717a, 0xff6b70, 0x6ee7a8, 0xfcd34d, 0x93b8ff, 0xd8b4fe, 0x5fd4c6,
    0xfafafa,
];
const ANSI_LIGHT: [u32; 16] = [
    0x27272a, 0xc81e1e, 0x15803d, 0xa16207, 0x1d4ed8, 0x9333ea, 0x0f766e, 0x71717a, 0x52525b, 0xdc2626, 0x16a34a, 0xb45309, 0x2563eb, 0xa855f7, 0x0d9488,
    0x18181b,
];

impl TermColors {
    fn from_palette(p: &Palette) -> Self {
        let mut selection: Hsla = p.accent.into();
        selection.a = if p.is_dark() { 0.35 } else { 0.22 };
        TermColors { fg: p.text.into(), bg: p.sunken.into(), cursor: p.text.into(), selection, ansi: if p.is_dark() { ANSI_DARK } else { ANSI_LIGHT } }
    }
}

fn rgb_hsla(c: u32) -> Hsla {
    gpui::rgb(c).into()
}

/// xterm's 256-colour table above the 16 ANSI colours.
fn indexed_rgb(i: u8) -> u32 {
    match i {
        16..=231 => {
            let i = i - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            (lv(i / 36) << 16) | (lv((i / 6) % 6) << 8) | lv(i % 6)
        }
        _ => {
            let g = 8 + (i.saturating_sub(232) as u32) * 10;
            (g << 16) | (g << 8) | g
        }
    }
}

pub struct TerminalView {
    term: Term<Replies>,
    parser: Processor,
    replies: Arc<Mutex<Vec<String>>>,
    pty: mpsc::Sender<PtyCmd>,
    focus: FocusHandle,
    shell: String,
    cols: usize,
    rows: usize,
    font: Font,
    cell_w: Pixels,
    /// Top-left of the grid in window coordinates (from the last prepaint), for mouse mapping.
    origin: Point<Pixels>,
    selecting: bool,
    /// Wheel movement not yet worth a whole line.
    scroll_rem: f32,
    pub(crate) error: Option<String>,
    /// The visible text for assistive technology, rebuilt after output when next rendered.
    a11y_text: SharedString,
    a11y_dirty: bool,
    /// Reused per painted row.
    row_text: String,
    row_runs: Vec<TextRun>,
    row_bgs: Vec<(usize, usize, Hsla)>,
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalView {
    /// Starts `shell` right away (the 2.0 decision: the shell starts on the first visit to the
    /// Terminal page, which is when this view is built).
    pub fn new(core: Arc<Core>, shell: String, cx: &mut Context<Self>) -> Self {
        let replies = Arc::new(Mutex::new(Vec::new()));
        let (cols, rows) = (80, 24);
        let term = Term::new(Self::config(), &TermSize::new(cols, rows), Replies(replies.clone()));
        let pty = spawn_writer(core);
        let _ = pty.send(PtyCmd::Start(shell.clone()));
        cx.set_global(Sink(cx.entity().downgrade()));
        let mut font = gpui::font(FONT_MONO);
        // Ligatures would merge cells (`->`, `!=`) and break the grid.
        font.features = FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]));
        TerminalView {
            term,
            parser: Processor::new(),
            replies,
            pty,
            focus: cx.focus_handle(),
            shell,
            cols,
            rows,
            font,
            cell_w: px(8.),
            origin: Point::default(),
            selecting: false,
            scroll_rem: 0.,
            error: None,
            a11y_text: SharedString::default(),
            a11y_dirty: true,
            row_text: String::new(),
            row_runs: Vec::new(),
            row_bgs: Vec::new(),
        }
    }

    fn config() -> Config {
        Config { scrolling_history: SCROLLBACK, ..Config::default() }
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// Restart with `shell` (a new shell choice, the Restart button, Retry after a failure).
    pub fn restart(&mut self, shell: String, cx: &mut Context<Self>) {
        self.shell = shell.clone();
        self.error = None;
        self.term = Term::new(Self::config(), &TermSize::new(self.cols, self.rows), Replies(self.replies.clone()));
        self.parser = Processor::new();
        let _ = self.pty.send(PtyCmd::Start(shell));
        let _ = self.pty.send(PtyCmd::Resize(self.cols as u16, self.rows as u16));
        self.a11y_dirty = true;
        cx.notify();
    }

    /// Clear the screen and the scrollback. The shell redraws its prompt on Ctrl+L
    /// (PSReadLine ClearScreen, readline clear-screen).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.term.selection = None;
        self.term.grid_mut().clear_history();
        self.term.scroll_display(Scroll::Bottom);
        self.send("\x0c".into());
        self.a11y_dirty = true;
        cx.notify();
    }

    /// Text typed into the shell (an inserted command, `--profile`, a paste): goes to the PTY
    /// exactly as given, without Enter.
    pub fn write(&mut self, text: &str, cx: &mut Context<Self>) {
        self.term.scroll_display(Scroll::Bottom);
        self.send(text.to_string());
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn send(&self, s: String) {
        if !s.is_empty() {
            let _ = self.pty.send(PtyCmd::Write(s));
        }
    }

    fn feed(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.parser.advance(&mut self.term, bytes);
        let replies: Vec<String> = self.replies.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for r in replies {
            self.send(r);
        }
        self.a11y_dirty = true;
        cx.notify();
    }

    /// Fit the grid to the space the layout gave it; tell the PTY when it changes.
    fn fit(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        let ts = window.text_system();
        let id = ts.resolve_font(&self.font);
        self.cell_w = ts.advance(id, px(FONT_SIZE), 'm').map(|s| s.width).unwrap_or(px(8.));
        self.origin = bounds.origin;
        let cols = (bounds.size.width / self.cell_w).floor() as usize;
        let rows = (bounds.size.height / px(LINE_HEIGHT)).floor() as usize;
        if cols < MIN_COLS || rows < MIN_ROWS || (cols == self.cols && rows == self.rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.resize(TermSize::new(cols, rows));
        let _ = self.pty.send(PtyCmd::Resize(cols as u16, rows as u16));
        self.a11y_dirty = true;
    }

    fn visible_text(&self) -> String {
        let mut lines = vec![String::new(); self.rows];
        let offset = self.term.grid().display_offset() as i32;
        for cell in self.term.grid().display_iter() {
            let row = cell.point.line.0 + offset;
            if row >= 0 && (row as usize) < self.rows && !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                lines[row as usize].push(if cell.c == '\0' { ' ' } else { cell.c });
            }
        }
        let mut out: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }

    // ---------------------------------------------------------------------------------------
    // Input
    // ---------------------------------------------------------------------------------------

    fn key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let app_cursor = self.term.mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = key_bytes(&ev.keystroke, app_cursor) {
            self.term.selection = None;
            self.term.scroll_display(Scroll::Bottom);
            self.send(bytes);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        match self.term.selection_to_string().filter(|s| !s.is_empty()) {
            Some(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.term.selection = None;
                cx.notify();
                true
            }
            None => false,
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let out = if self.term.mode().contains(TermMode::BRACKETED_PASTE) { format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")) } else { text };
        self.write(&out, cx);
    }

    /// Window position to a grid point (and which half of the cell).
    fn grid_point(&self, pos: Point<Pixels>) -> (GridPoint, Side) {
        let x = (pos.x - self.origin.x).max(px(0.));
        let y = (pos.y - self.origin.y).max(px(0.));
        let col_f = x / self.cell_w;
        let col = (col_f.floor() as usize).min(self.cols.saturating_sub(1));
        let row = ((y / px(LINE_HEIGHT)).floor() as usize).min(self.rows.saturating_sub(1));
        let side = if col_f.fract() < 0.5 { Side::Left } else { Side::Right };
        (viewport_to_point(self.term.grid().display_offset(), GridPoint::new(row, Column(col))), side)
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (pt, side) = self.grid_point(ev.position);
        let ty = match ev.click_count {
            2 => SelectionType::Semantic,
            n if n >= 3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        if ev.modifiers.shift {
            if let Some(sel) = self.term.selection.as_mut() {
                sel.update(pt, side);
                cx.notify();
                return;
            }
        }
        self.term.selection = Some(Selection::new(ty, pt, side));
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let (pt, side) = self.grid_point(ev.position);
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(pt, side);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        self.selecting = false;
        if self.term.selection.as_ref().is_some_and(|s| s.is_empty()) {
            self.term.selection = None;
            cx.notify();
        }
    }

    /// Right-click: copy the selection if there is one, else paste (1.x / Windows console).
    fn right_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if !self.copy_selection(cx) {
            self.paste(cx);
        }
    }

    fn scroll(&mut self, ev: &ScrollEvent, cx: &mut Context<Self>) {
        let dy = ev.delta.pixel_delta(px(LINE_HEIGHT)).y;
        self.scroll_rem += f32::from(dy) / LINE_HEIGHT;
        let lines = self.scroll_rem.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_rem -= lines as f32;
        let mode = *self.term.mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            // Full-screen programs (less, vim) without mouse reporting: the wheel moves by lines.
            let app = mode.contains(TermMode::APP_CURSOR);
            let seq = match (lines > 0, app) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            self.send(seq.repeat(lines.unsigned_abs() as usize));
        } else {
            self.term.scroll_display(Scroll::Delta(lines));
        }
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------
    // Painting
    // ---------------------------------------------------------------------------------------

    fn paint(&mut self, bounds: Bounds<Pixels>, colors: TermColors, focused: bool, window: &mut Window, cx: &mut App) {
        let line_h = px(LINE_HEIGHT);
        let cell_w = self.cell_w;
        let origin = bounds.origin;
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let cursor = content.cursor;
        let cursor_shown = cursor.shape != CursorShape::Hidden;
        let cursor_row = cursor.point.line.0 + offset;
        let block_cursor = cursor_shown && focused && cursor.shape == CursorShape::Block;
        let selection = content.selection;
        let term_colors = content.colors;

        let resolve = |c: Color, is_fg: bool, bold: bool| -> Hsla {
            match c {
                Color::Spec(Rgb { r, g, b }) => rgb_hsla(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
                Color::Indexed(i) => {
                    if let Some(Rgb { r, g, b }) = term_colors[i as usize] {
                        return rgb_hsla(((r as u32) << 16) | ((g as u32) << 8) | b as u32);
                    }
                    if i < 16 {
                        let i = if bold && is_fg && i < 8 { i + 8 } else { i };
                        rgb_hsla(colors.ansi[i as usize])
                    } else {
                        rgb_hsla(indexed_rgb(i))
                    }
                }
                Color::Named(n) => {
                    let idx = n as usize;
                    if idx < 16 {
                        let idx = if bold && is_fg && idx < 8 { idx + 8 } else { idx };
                        return rgb_hsla(colors.ansi[idx]);
                    }
                    match n {
                        NamedColor::Background => colors.bg,
                        NamedColor::Cursor => colors.cursor,
                        NamedColor::DimForeground => {
                            let mut f = colors.fg;
                            f.a = 0.65;
                            f
                        }
                        _ => colors.fg,
                    }
                }
            }
        };

        let mut text = std::mem::take(&mut self.row_text);
        let mut runs = std::mem::take(&mut self.row_runs);
        let mut bgs = std::mem::take(&mut self.row_bgs);
        let mut current_row: i32 = -1;
        let mut col: usize = 0;

        let flush = |row: i32, text: &mut String, runs: &mut Vec<TextRun>, bgs: &mut Vec<(usize, usize, Hsla)>, window: &mut Window, cx: &mut App| {
            if row < 0 {
                return;
            }
            let y = origin.y + line_h * row as f32;
            for (start, end, color) in bgs.drain(..) {
                window.paint_quad(fill(Bounds::new(point(origin.x + cell_w * start as f32, y), size(cell_w * (end - start) as f32, line_h)), color));
            }
            // Trailing blanks need no shaping.
            let keep = text.trim_end_matches(' ').len();
            if keep > 0 {
                let mut left = keep;
                let mut n = 0;
                for r in runs.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    r.len = r.len.min(left);
                    left -= r.len;
                    n += 1;
                }
                runs.truncate(n);
                text.truncate(keep);
                let shaped = window.text_system().shape_line(SharedString::from(text.clone()), px(FONT_SIZE), runs, Some(cell_w));
                let _ = shaped.paint(point(origin.x, y), line_h, TextAlign::Left, None, window, cx);
            }
            text.clear();
            runs.clear();
        };

        for cell in content.display_iter {
            let row = cell.point.line.0 + offset;
            if row != current_row {
                flush(current_row, &mut text, &mut runs, &mut bgs, window, cx);
                current_row = row;
                col = 0;
            }
            let flags = cell.flags;
            let bold = flags.contains(Flags::BOLD);
            let mut fg = resolve(cell.fg, true, bold);
            let mut bg = resolve(cell.bg, false, false);
            if flags.contains(Flags::DIM) {
                fg.a *= 0.65;
            }
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            if selection.is_some_and(|s| s.contains(cell.point)) {
                bg = colors.selection;
            }
            if block_cursor && cell.point == cursor.point {
                bg = colors.cursor;
                fg = colors.bg;
            }
            if bg != colors.bg {
                match bgs.last_mut() {
                    Some((_, end, c)) if *end == col && *c == bg => *end = col + 1,
                    _ => bgs.push((col, col + 1, bg)),
                }
            }

            let start = text.len();
            if flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                text.push(' ');
            } else {
                text.push(if cell.c == '\0' || cell.c == '\t' { ' ' } else { cell.c });
                if let Some(extra) = cell.zerowidth() {
                    text.extend(extra.iter());
                }
            }
            let len = text.len() - start;
            let weight = if bold { FontWeight::MEDIUM } else { FontWeight::NORMAL };
            let style = if flags.contains(Flags::ITALIC) { FontStyle::Italic } else { FontStyle::Normal };
            let underline = flags.intersects(Flags::ALL_UNDERLINES);
            let strike = flags.contains(Flags::STRIKEOUT);
            match runs.last_mut() {
                Some(r)
                    if r.color == fg
                        && r.font.weight == weight
                        && r.font.style == style
                        && r.underline.is_some() == underline
                        && r.strikethrough.is_some() == strike =>
                {
                    r.len += len
                }
                _ => runs.push(TextRun {
                    len,
                    font: Font { weight, style, ..self.font.clone() },
                    color: fg,
                    background_color: None,
                    underline: underline.then_some(UnderlineStyle { thickness: px(1.), color: Some(fg), wavy: false }),
                    strikethrough: strike.then_some(StrikethroughStyle { thickness: px(1.), color: Some(fg) }),
                }),
            }
            col += 1;
        }
        flush(current_row, &mut text, &mut runs, &mut bgs, window, cx);

        // Cursors that are not a focused block.
        if cursor_shown && cursor_row >= 0 && (cursor_row as usize) < self.rows && !block_cursor {
            let x = origin.x + cell_w * cursor.point.column.0 as f32;
            let y = origin.y + line_h * cursor_row as f32;
            let cell = Bounds::new(point(x, y), size(cell_w, line_h));
            let quad = match (focused, cursor.shape) {
                (true, CursorShape::Beam) => fill(Bounds::new(point(x, y), size(px(2.), line_h)), colors.cursor),
                (true, CursorShape::Underline) => fill(Bounds::new(point(x, y + line_h - px(2.)), size(cell_w, px(2.))), colors.cursor),
                _ => outline(cell, colors.cursor, gpui::BorderStyle::Solid),
            };
            window.paint_quad(quad);
        }

        self.row_text = text;
        self.row_runs = runs;
        self.row_bgs = bgs;
    }
}

impl Drop for TerminalView {
    /// The window closed (it is destroyed, not hidden): end the shell, as 1.x did per window.
    fn drop(&mut self) {
        let _ = self.pty.send(PtyCmd::Dispose);
    }
}

/// A keystroke as the bytes a VT terminal sends (xterm conventions, which ConPTY translates back
/// into console key events).
fn key_bytes(k: &gpui::Keystroke, app_cursor: bool) -> Option<String> {
    let m = &k.modifiers;
    let param = 1 + m.shift as u8 + 2 * (m.alt as u8) + 4 * (m.control as u8);
    let csi_letter = |l: char| -> String {
        if param == 1 {
            if app_cursor {
                format!("\x1bO{l}")
            } else {
                format!("\x1b[{l}")
            }
        } else {
            format!("\x1b[1;{param}{l}")
        }
    };
    let tilde = |n: u8| -> String {
        if param == 1 {
            format!("\x1b[{n}~")
        } else {
            format!("\x1b[{n};{param}~")
        }
    };
    let ss3 = |l: char| -> String {
        if param == 1 {
            format!("\x1bO{l}")
        } else {
            format!("\x1b[1;{param}{l}")
        }
    };
    let s = match k.key.as_str() {
        "up" => csi_letter('A'),
        "down" => csi_letter('B'),
        "right" => csi_letter('C'),
        "left" => csi_letter('D'),
        "home" => csi_letter('H'),
        "end" => csi_letter('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "enter" => {
            if m.alt {
                "\x1b\r".into()
            } else {
                "\r".into()
            }
        }
        "backspace" => {
            if m.control {
                "\x08".into()
            } else if m.alt {
                "\x1b\x7f".into()
            } else {
                "\x7f".into()
            }
        }
        "tab" => {
            if m.shift {
                "\x1b[Z".into()
            } else {
                "\t".into()
            }
        }
        "escape" => "\x1b".into(),
        "space" if m.control => "\0".into(),
        // AltGr arrives as Ctrl+Alt with the character it produces.
        _ if m.control && m.alt && k.key_char.is_some() => k.key_char.clone()?,
        key if m.control && key.chars().count() == 1 => {
            let c = key.chars().next()?.to_ascii_lowercase();
            let b = match c {
                'a'..='z' => c as u8 & 0x1f,
                '@' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '-' | '_' | '/' | '7' => 0x1f,
                '8' => 0x7f,
                _ => return None,
            };
            let mut s = String::new();
            if m.alt {
                s.push('\x1b');
            }
            s.push(b as char);
            s
        }
        _ => {
            let ch = k.key_char.clone().or_else(|| (k.key == "space").then(|| " ".to_string()))?;
            if m.alt {
                format!("\x1b{ch}")
            } else {
                ch
            }
        }
    };
    Some(s)
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p =
            cx.try_global::<crate::picker::PickerPalette>().map(|g| g.0).unwrap_or_else(|| Palette::new(crate::theme::Style::Modern, crate::theme::Mode::Dark));
        let colors = TermColors::from_palette(&p);
        if self.a11y_dirty {
            self.a11y_text = self.visible_text().into();
            self.a11y_dirty = false;
        }
        let focused = self.focus.is_focused(window);
        let me = cx.entity();
        let me_paint = me.clone();
        let grid = canvas(
            move |bounds, window, cx| {
                me.update(cx, |v, _| v.fit(bounds, window));
            },
            move |bounds, _, window, cx| {
                me_paint.update(cx, |v, cx| v.paint(bounds, colors, focused, window, cx));
            },
        )
        .size_full();

        let error = self.error.clone();
        div()
            .id("terminal")
            .role(Role::Terminal)
            .aria_label("AWS CLI terminal")
            .aria_value(self.a11y_text.clone())
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(colors.bg)
            .p(px(PAD))
            .cursor_text()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.key_down(ev, cx)))
            .on_action(cx.listener(|this, _: &SendTab, _, _| this.send("\t".into())))
            .on_action(cx.listener(|this, _: &SendBackTab, _, _| this.send("\x1b[Z".into())))
            .on_action(cx.listener(|this, _: &CtrlC, _, cx| {
                // Copy when something is selected (Windows Terminal); otherwise interrupt.
                if !this.copy_selection(cx) {
                    this.term.scroll_display(Scroll::Bottom);
                    this.send("\x03".into());
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| {
                this.copy_selection(cx);
            }))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste(cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.mouse_down(ev, window, cx)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.mouse_up(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _: &MouseDownEvent, window, cx| this.right_click(window, cx)))
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _, cx| this.scroll(ev, cx)))
            .child(grid)
            .when_some(error, |d, message| {
                d.child(
                    div()
                        .id("terminal-error")
                        .role(Role::Alert)
                        .aria_label(SharedString::from(format!("Terminal failed to start: {message}")))
                        .absolute()
                        .inset_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(12.))
                        .bg(colors.bg)
                        .child(div().text_size(px(sz::TEXT)).text_color(p.text_muted).child("Terminal failed to start"))
                        .child(div().max_w(px(520.)).font_family(FONT_MONO).text_size(px(sz::TEXT_XS)).text_color(p.danger).child(message))
                        .child(crate::widgets::primary_button("terminal-retry", "Retry", None, &p).on_press(cx.listener(|this, _, _, cx| {
                            let shell = this.shell.clone();
                            this.restart(shell, cx);
                        }))),
                )
            })
    }
}
