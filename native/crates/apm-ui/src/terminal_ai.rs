//! The AI assistant panel on the Terminal page (1.x `AIAssistantPanel.tsx` + `AIMarkdown.tsx`).
//!
//! A multi-turn chat with Open WebUI through the core (`ai:chat-stream`; the API key never leaves
//! the core). The whole thread goes out on every turn so follow-ups work; error bubbles are left
//! out of that history. Tokens arrive on `ai:chat-chunk` (routed straight here by the app, see
//! [`deliver`]); the call's result carries the complete reply, which replaces the streamed text.
//!
//! Replies are Markdown. Prose renders through gpui-component's `TextView`; fenced code blocks are
//! drawn here so each runnable one gets an "Insert into terminal" button (and a copy button). Rich
//! text is not in the accessibility tree, so every text segment and code block also carries a
//! plain-text copy as a Label (as the release notes do in onboarding.rs).

use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, FontWeight, IntoElement, Role, ScrollHandle, SharedString, Subscription, Task, WeakEntity, Window,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::text::{TextView, TextViewStyle};
use serde_json::{json, Value};

use crate::a11y::{A11yDisabledExt as _, PressExt as _};
use crate::model::{AppModel, Page};
use crate::onboarding::plain_text;
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{icon, icon_button, primary_button, read_text, secondary_button};

pub const PANEL_W: f32 = 340.;
const COLLAPSED_W: f32 = 40.;
/// 1.x: one-click follow-ups after a successful reply (they read as continuations of the thread).
const FOLLOW_UP_PROMPTS: [&str; 3] = ["Show more options", "Give another example", "What could go wrong?"];
/// Within this many px of the bottom counts as parked there (1.x BOTTOM_PIN_THRESHOLD_PX).
const BOTTOM_PIN_THRESHOLD: f32 = 48.;

/// What the panel asks of the Terminal page.
pub enum AiEvent {
    /// Put this command at the terminal prompt (no Enter); the page adds `--profile`.
    Insert(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Who {
    User,
    Assistant,
}

struct Message {
    who: Who,
    content: String,
    /// A local/API error rather than a model reply; not sent back as history.
    is_error: bool,
}

/// The panel that receives `ai:chat-chunk` events (one Terminal page per window).
struct Sink(WeakEntity<AiPanel>);
impl gpui::Global for Sink {}

/// Stream tokens from the core (`ai:chat-chunk`: `{ requestId, delta }`).
pub fn deliver(payload: &[Value], cx: &mut App) {
    let Some(view) = cx.try_global::<Sink>().and_then(|s| s.0.upgrade()) else { return };
    let Some(p) = payload.first() else { return };
    let id = p.get("requestId").and_then(Value::as_str).unwrap_or("");
    let Some(delta) = p.get("delta").and_then(Value::as_str) else { return };
    view.update(cx, |v, cx| v.on_chunk(id, delta, cx));
}

pub struct AiPanel {
    model: Entity<AppModel>,
    input: Entity<InputState>,
    prompt: String,
    messages: Vec<Message>,
    /// Text of the in-flight reply so far; `None` when nothing is streaming.
    streaming: Option<String>,
    /// The in-flight request id (`None`: idle).
    request: Option<String>,
    configured: bool,
    /// (API URL, key saved) last checked with `ai:getConfigStatus`.
    config_key: Option<(String, bool)>,
    pub collapsed: bool,
    scroll: ScrollHandle,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<AiEvent> for AiPanel {}

fn new_request_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("ai-{ms}-{}", N.fetch_add(1, Ordering::Relaxed))
}

impl AiPanel {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Ask for an AWS CLI command…"));
        let subs = vec![
            // Observed, not subscribed to Change: an accessible SetValue edits without that event.
            cx.observe(&input, |this, input, cx| {
                let v = input.read(cx).value().to_string();
                if v != this.prompt {
                    this.prompt = v;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { shift: false, .. }) {
                    this.send_from_input(window, cx);
                }
            }),
            cx.observe(&model, |this, _, cx| this.check_config(cx)),
        ];
        cx.set_global(Sink(cx.entity().downgrade()));
        let mut this = AiPanel {
            model,
            input,
            prompt: String::new(),
            messages: Vec::new(),
            streaming: None,
            request: None,
            configured: false,
            config_key: None,
            collapsed: false,
            scroll: ScrollHandle::new(),
            _task: None,
            _subs: subs,
        };
        this.check_config(cx);
        this
    }

    pub fn configured(&self) -> bool {
        self.configured
    }

    fn loading(&self) -> bool {
        self.request.is_some()
    }

    /// Ask the core whether Open WebUI is set up, whenever the relevant settings change.
    fn check_config(&mut self, cx: &mut Context<Self>) {
        let key = {
            let st = &self.model.read(cx).settings;
            let url = st.get("openWebUiApiUrl").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let key_saved =
                st.get("openWebUiApiKeySet") == Some(&Value::Bool(true)) || st.get("openWebUiApiKey").and_then(Value::as_str).is_some_and(|k| !k.is_empty());
            (url, key_saved)
        };
        if self.config_key.as_ref() == Some(&key) {
            return;
        }
        self.config_key = Some(key);
        let call = self.model.read(cx).bridge.call("ai:getConfigStatus", vec![]);
        cx.spawn(async move |this, cx| {
            let configured = call.await.ok().and_then(|v| v.get("configured").and_then(Value::as_bool)).unwrap_or(false);
            let _ = this.update(cx, |t, cx| {
                if t.configured != configured {
                    t.configured = configured;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn send_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() || self.loading() {
            return;
        }
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.prompt.clear();
        self.send_prompt(prompt, cx);
    }

    /// Send one turn (also "Ask AI how to use this" and the follow-up buttons).
    pub fn send_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() || self.loading() || !self.configured {
            return;
        }
        self.collapsed = false;
        self.messages.push(Message { who: Who::User, content: prompt, is_error: false });
        let history: Vec<Value> = self
            .messages
            .iter()
            .filter(|m| !m.is_error)
            .map(|m| json!({ "role": if m.who == Who::User { "user" } else { "assistant" }, "content": m.content }))
            .collect();
        let id = new_request_id();
        self.request = Some(id.clone());
        self.streaming = Some(String::new());
        // Sending is an explicit "show me what happens next": re-pin to the bottom.
        self.scroll.scroll_to_bottom();
        let call = self.model.read(cx).bridge.call("ai:chat-stream", vec![json!({ "requestId": id, "messages": history })]);
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |t, cx| t.finish(&id, result, cx));
        }));
        cx.notify();
    }

    fn on_chunk(&mut self, id: &str, delta: &str, cx: &mut Context<Self>) {
        if self.request.as_deref() != Some(id) {
            return;
        }
        let pinned = self.pinned();
        if let Some(s) = &mut self.streaming {
            s.push_str(delta);
        }
        if pinned {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    /// Whether the view is parked at the bottom (scrolling up to read back stops auto-scroll).
    fn pinned(&self) -> bool {
        let max = self.scroll.max_offset().y;
        let off = -self.scroll.offset().y;
        max - off <= px(BOTTOM_PIN_THRESHOLD)
    }

    fn finish(&mut self, id: &str, result: Result<Value, String>, cx: &mut Context<Self>) {
        if self.request.as_deref() != Some(id) {
            return; // cleared with "New chat" meanwhile
        }
        let pinned = self.pinned();
        let msg = match result {
            Ok(v) => {
                let content = v.get("content").and_then(Value::as_str).unwrap_or("").to_string();
                let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
                // A stopped stream keeps whatever arrived: a partial answer is still useful.
                let stopped_empty = flag("aborted") && content.trim().is_empty();
                Message {
                    who: Who::Assistant,
                    content: if stopped_empty { "Stopped before any response arrived.".into() } else { content },
                    is_error: flag("isError") || stopped_empty,
                }
            }
            Err(e) => Message { who: Who::Assistant, content: format!("Error: {e}"), is_error: true },
        };
        self.messages.push(msg);
        self.request = None;
        self.streaming = None;
        if pinned {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = &self.request {
            let _ = self.model.read(cx).bridge.call("ai:chat-abort", vec![json!(id)]);
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.request = None;
        self.streaming = None;
        self.messages.clear();
        cx.notify();
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| {
            m.page = Page::Settings;
            m.settings_tab_request = Some(crate::settings::Tab::Terminal);
            cx.notify();
        });
    }

    // --------------------------------------------------------------------------------------------
    // Rendering
    // --------------------------------------------------------------------------------------------

    /// One reply as Markdown: prose through `TextView`, fenced code blocks drawn here.
    fn markdown(&self, key: &str, content: &str, p: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let bridge = self.model.read(cx).bridge.clone();
        let mut col = div().flex().flex_col().gap(px(8.)).min_w(px(0.));
        for (i, seg) in segments(content).into_iter().enumerate() {
            match seg {
                Segment::Text(md) => {
                    let plain: SharedString = plain_text(&md).into();
                    let b = bridge.clone();
                    col = col.child(
                        div().id(SharedString::from(format!("{key}-text-{i}"))).role(Role::Label).aria_label(plain.clone()).aria_value(plain).child(
                            TextView::markdown(SharedString::from(format!("{key}-md-{i}")), md)
                                // 1.x: every heading at body size or one step up.
                                .style(TextViewStyle::default().heading_font_size(|level, _| px(if level <= 2 { 15. } else { 14. })))
                                .selectable(true)
                                .stream_fade(true)
                                .image_source(|_| gpui::ImageSource::from(std::path::PathBuf::new()))
                                .on_link_click(move |url, _, _, _| {
                                    let _ = b.call("app:openExternal", vec![json!(url)]);
                                }),
                        ),
                    );
                }
                Segment::Code { lang, code } => {
                    let runnable = is_runnable(&lang, &code);
                    let copy_text = code.clone();
                    let insert_text = code.clone();
                    let block = div()
                        .relative()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(p.border)
                        .bg(p.sunken)
                        .pl(px(10.))
                        .pr(px(36.))
                        .py(px(8.))
                        .font_family(FONT_MONO)
                        .text_size(px(size::TEXT_XS))
                        .text_color(p.text)
                        .child(read_text(SharedString::from(format!("{key}-code-{i}")), code))
                        .child(
                            icon_button(SharedString::from(format!("{key}-copy-{i}")), "copy", "Copy code", p)
                                .absolute()
                                .top(px(3.))
                                .right(px(3.))
                                .size(px(26.))
                                .on_press(move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_text.clone()))),
                        );
                    col = col.child(div().flex().flex_col().gap(px(6.)).child(block).when(runnable, |d| {
                        d.child(
                            div().flex().child(
                                primary_button(SharedString::from(format!("{key}-insert-{i}")), "Insert into terminal", Some("terminal"), p)
                                    .h(px(28.))
                                    .px(px(10.))
                                    .text_size(px(size::TEXT_SM))
                                    .on_press(cx.listener(move |_, _, _, cx| cx.emit(AiEvent::Insert(insert_text.clone())))),
                            ),
                        )
                    }));
                }
            }
        }
        col
    }

    fn header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_shrink_0()
            .h(px(52.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(4.))
            .border_b_1()
            .border_color(p.border)
            .child(icon_button("ai-toggle", "chevrons-right", "Collapse AI assistant", p).on_press(cx.listener(|this, _, _, cx| {
                this.collapsed = true;
                cx.notify();
            })))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .id("ai-title")
                            .role(Role::Heading)
                            .aria_label("AI assistant")
                            .text_size(px(size::TEXT_SM))
                            .font_weight(FontWeight::MEDIUM)
                            .child("AI assistant"),
                    )
                    .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_faint).child("Ask for AWS CLI examples")),
            )
            .child(icon_button("ai-new-chat", "square-pen", "New chat", p).on_press(cx.listener(|this, _, _, cx| this.clear(cx))))
    }

    fn body(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .id("ai-messages")
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_size(px(size::TEXT_SM));

        if !self.configured {
            body = body.child(
                div()
                    .p(px(16.))
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(p.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .text_center()
                    .child(icon("sparkles", 20., p.text_faint))
                    .child(
                        div()
                            .text_color(p.text_muted)
                            .child(read_text("ai-not-configured", "To use the AI assistant, add your Open WebUI API URL and API key in Settings.")),
                    )
                    .child(div().text_size(px(size::TEXT_XS)).text_color(p.text_faint).child("Set it up in Settings, under Terminal & AI."))
                    .child(
                        div().mt(px(4.)).child(
                            primary_button("ai-open-settings", "Open settings", None, p)
                                .h(px(32.))
                                .text_size(px(size::TEXT_SM))
                                .on_press(cx.listener(|this, _, _, cx| this.open_settings(cx))),
                        ),
                    ),
            );
        } else if self.messages.is_empty() && self.streaming.is_none() {
            body = body.child(div().text_color(p.text_muted).child("e.g. \u{201c}How do I list S3 buckets?\u{201d}"));
        }

        for (i, m) in self.messages.iter().enumerate() {
            let key = format!("ai-msg-{i}");
            body = body.child(match (m.who, m.is_error) {
                (Who::User, _) => div()
                    .ml(px(24.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(size::RADIUS))
                    .bg(p.raised)
                    .text_color(p.text)
                    .child(read_text(SharedString::from(key), m.content.clone())),
                (Who::Assistant, true) => div()
                    .mr(px(24.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(size::RADIUS))
                    .border_1()
                    .border_color(p.danger)
                    .text_color(p.text)
                    .child(read_text(SharedString::from(key), m.content.clone())),
                (Who::Assistant, false) => div().text_color(p.text).child(self.markdown(&key, &m.content, p, cx)),
            });
        }

        if self.configured {
            match &self.streaming {
                Some(s) if !s.is_empty() => {
                    // Stop lives in the composer, not here: this bubble grows as tokens arrive.
                    body = body.child(div().text_color(p.text).child(self.markdown(&format!("ai-msg-{}", self.messages.len()), s, p, cx)));
                }
                Some(_) => {
                    let dot = || div().size(px(6.)).rounded_full().bg(p.text_faint);
                    body = body.child(
                        div()
                            .id("ai-thinking")
                            .role(Role::Status)
                            .aria_label("Thinking")
                            .mr(px(16.))
                            .px(px(12.))
                            .py(px(12.))
                            .rounded(px(size::RADIUS))
                            .bg(p.raised)
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(dot())
                            .child(dot().opacity(0.7))
                            .child(dot().opacity(0.4)),
                    );
                }
                None => {}
            }
            let last_ok = self.messages.last().is_some_and(|m| m.who == Who::Assistant && !m.is_error);
            if !self.loading() && last_ok {
                let mut row = div().mr(px(16.)).flex().flex_wrap().gap(px(6.));
                for (i, prompt) in FOLLOW_UP_PROMPTS.iter().enumerate() {
                    row = row.child(
                        secondary_button(SharedString::from(format!("ai-follow-up-{i}")), *prompt, None, p)
                            .h(px(28.))
                            .px(px(10.))
                            .text_size(px(size::TEXT_XS))
                            .on_press(cx.listener(move |this, _, _, cx| this.send_prompt(prompt.to_string(), cx))),
                    );
                }
                body = body.child(row);
            }
        }
        body
    }

    fn composer(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.loading();
        let can_send = !self.prompt.trim().is_empty() && !loading;
        let button = if loading {
            secondary_button("ai-stop", "Stop", Some("circle-stop"), p)
                .h(px(size::CONTROL))
                .px(px(10.))
                .text_size(px(size::TEXT_SM))
                .on_press(cx.listener(|this, _, _, cx| this.stop(cx)))
                .a11y_disabled(false)
        } else {
            primary_button("ai-send", "Send", Some("send"), p)
                .h(px(size::CONTROL))
                .px(px(12.))
                .text_size(px(size::TEXT_SM))
                .when(!can_send, |d| d.opacity(0.5).cursor_default())
                .on_press(cx.listener(|this, _, window, cx| this.send_from_input(window, cx)))
                .a11y_disabled(!can_send)
        };
        div()
            .flex_shrink_0()
            .p(px(10.))
            .border_t_1()
            .border_color(p.border)
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.input).disabled(loading).aria_label("AI prompt")))
            .child(button)
    }
}

impl Render for AiPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.model.read(cx).surface_palette();
        if self.collapsed {
            return div()
                .id("ai-assistant")
                .role(Role::Complementary)
                .aria_label("AI assistant")
                .flex_shrink_0()
                .w(px(COLLAPSED_W))
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .py(px(6.))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(p.border)
                .bg(p.panel)
                .child(icon_button("ai-toggle", "chevrons-left", "Expand AI assistant", &p).on_press(cx.listener(|this, _, _, cx| {
                    this.collapsed = false;
                    cx.notify();
                })))
                .child(icon("sparkles", 16., p.text_faint))
                .into_any_element();
        }
        div()
            .id("ai-assistant")
            .role(Role::Complementary)
            .aria_label("AI assistant")
            .flex_shrink_0()
            .w(px(PANEL_W))
            .h_full()
            .flex()
            .flex_col()
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .overflow_hidden()
            .child(self.header(&p, cx))
            .child(self.body(&p, cx))
            .when(self.configured, |d| d.child(self.composer(&p, cx)))
            .into_any_element()
    }
}

// ------------------------------------------------------------------------------------------------
// Markdown helpers (1.x AIMarkdown)
// ------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Segment {
    Text(String),
    Code { lang: String, code: String },
}

/// Split a reply into prose and fenced code blocks. An unclosed fence (mid-stream) runs to the end.
fn segments(md: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut fence: Option<(String, String, String)> = None; // (marker, lang, code)
    let flush_text = |text: &mut String, out: &mut Vec<Segment>| {
        if !text.trim().is_empty() {
            out.push(Segment::Text(std::mem::take(text).trim_end().to_string()));
        }
        text.clear();
    };
    for line in md.split('\n') {
        let t = line.trim_start();
        match &mut fence {
            None => {
                let marker: String = t.chars().take_while(|c| *c == '`' || *c == '~').collect();
                if marker.len() >= 3 && marker.chars().all(|c| c == marker.chars().next().unwrap()) {
                    flush_text(&mut text, &mut out);
                    let lang = t[marker.len()..].split_whitespace().next().unwrap_or("").to_lowercase();
                    fence = Some((marker, lang, String::new()));
                } else {
                    text.push_str(line);
                    text.push('\n');
                }
            }
            Some((marker, lang, code)) => {
                let tt = t.trim_end();
                if tt.starts_with(marker.as_str()) && tt.chars().all(|c| marker.starts_with(c)) {
                    out.push(Segment::Code { lang: std::mem::take(lang), code: code.trim_end_matches('\n').to_string() });
                    fence = None;
                } else {
                    code.push_str(line);
                    code.push('\n');
                }
            }
        }
    }
    if let Some((_, lang, code)) = fence {
        let code = code.trim_end_matches('\n').to_string();
        if !code.is_empty() {
            out.push(Segment::Code { lang, code });
        }
    }
    flush_text(&mut text, &mut out);
    out
}

/// 1.x isRunnable: whether a code block is something you'd actually run. Output samples and
/// syntax templates (`<bucket-name>`, `[options]`) get no Insert button.
fn is_runnable(language: &str, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || has_angle_placeholder(trimmed) || has_bracket_placeholder(trimmed) {
        return false;
    }
    if ["bash", "sh", "shell", "zsh", "console", "powershell", "ps1"].contains(&language) {
        return true;
    }
    if !language.is_empty() {
        return false; // an explicit non-shell language (json, yaml, python…)
    }
    let first = trimmed.lines().next().unwrap_or("");
    ["aws", "export", "set"].iter().any(|w| first.strip_prefix(w).is_some_and(|r| r.chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_')))
}

/// `/<[^>\s][^>]*>/`
fn has_angle_placeholder(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| b[i] == b'<' && b.get(i + 1).is_some_and(|c| *c != b'>' && !c.is_ascii_whitespace()) && b[i + 2..].contains(&b'>'))
}

/// `/\[[a-z][a-z0-9 -]*\]/i`
fn has_bracket_placeholder(s: &str) -> bool {
    let b = s.as_bytes();
    (0..b.len()).any(|i| {
        if b[i] != b'[' || !b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let mut j = i + 2;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b' ' || b[j] == b'-') {
            j += 1;
        }
        b.get(j) == Some(&b']')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_code_blocks() {
        let s = segments("List them with:\n\n```bash\naws s3 ls --profile dev\n```\n");
        assert_eq!(s, vec![Segment::Text("List them with:".into()), Segment::Code { lang: "bash".into(), code: "aws s3 ls --profile dev".into() }]);
        let s = segments("a\n```\naws sts get-caller-identity");
        assert_eq!(s[1], Segment::Code { lang: String::new(), code: "aws sts get-caller-identity".into() });
    }

    #[test]
    fn runnable_rules() {
        assert!(is_runnable("bash", "aws s3 ls"));
        assert!(is_runnable("", "aws s3 ls"));
        assert!(!is_runnable("json", "{}"));
        assert!(!is_runnable("bash", "aws s3 rb s3://<bucket-name> [options]"));
        assert!(!is_runnable("", "awsome"));
        assert!(is_runnable("sh", "echo a > b.txt"));
    }
}
