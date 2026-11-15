//! The auth audit log viewer: a window of its own (1.x `authLogViewer.ts` +
//! `resources/auth-log-viewer.html`). Newest entries first, a search box, "Clear log" with a
//! confirmation, and live updates while it is open (`Host::log_viewer_changed` → [`reload`]).
//!
//! The window is destroyed when closed, like every 2.0 window. Its UIA name is "Auth audit log",
//! which is how the flow runner finds it (`"window": "logViewer"`).

use gpui::{
    div, prelude::*, px, size, AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window, WindowBounds,
    WindowOptions,
};
use gpui_component::input::{Input, InputState};
use gpui_component::{Root, TitleBar};
use serde_json::Value;

use crate::a11y::ActivateExt as _;
use crate::model::AppModel;
use crate::theme::{size as sz, Palette, FONT_MONO, FONT_SANS};
use crate::widgets::{primary_button, read_text, secondary_button};

pub const WINDOW_TITLE: &str = "Auth audit log";

/// The open viewer, if any.
struct OpenViewer(AnyWindowHandle, Entity<LogViewer>);
impl gpui::Global for OpenViewer {}

fn str_of<'a>(e: &'a Value, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn type_of(e: &Value) -> &str {
    e.get("type").and_then(Value::as_str).unwrap_or("")
}

/// Badge text by entry type (1.x `badgeLabel`).
pub fn badge_label(e: &Value) -> &'static str {
    match type_of(e) {
        "failure" => "Failure",
        "idp_success" => "Success",
        _ => "Request",
    }
}

/// First line of a card (1.x `primaryLine`).
pub fn primary_line(e: &Value) -> String {
    let host = str_of(e, "idpHost").map(|h| format!(" · {h}")).unwrap_or_default();
    match type_of(e) {
        "idp_request" => format!("POST IdP sign-in{host}"),
        "idp_success" => format!("IdP sign-in succeeded — SAML received{host}"),
        _ => {
            let first = str_of(e, "error").unwrap_or("Failure").split('\n').next().unwrap_or("").to_string();
            first.chars().take(200).collect()
        }
    }
}

fn num_of(e: &Value, k: &str) -> Option<String> {
    match e.get(k) {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Second line of a card (1.x `detailLine`).
pub fn detail_line(e: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("source: {}", e.get("source").and_then(Value::as_str).unwrap_or("undefined")));
    if let Some(pid) = str_of(e, "profileId") {
        let label = match str_of(e, "profileName") {
            Some(n) => format!("{n} ({pid})"),
            None => pid.to_string(),
        };
        parts.push(format!("profile: {label}"));
    }
    match type_of(e) {
        "idp_request" => {
            parts.insert(0, "method: POST".into());
            if let Some(h) = str_of(e, "idpHost") {
                parts.push(format!("host: {h}"));
            }
            if let Some(u) = str_of(e, "usernameHint") {
                parts.push(format!("user: {u}"));
            }
        }
        "idp_success" => {
            parts.insert(0, "result: SAML assertion".into());
            if let Some(n) = num_of(e, "roleCount") {
                parts.push(format!("roles: {n}"));
            }
            if let Some(n) = num_of(e, "durationMs") {
                parts.push(format!("{n} ms"));
            }
            if let Some(h) = str_of(e, "idpHost") {
                parts.push(format!("host: {h}"));
            }
            if let Some(u) = str_of(e, "usernameHint") {
                parts.push(format!("user: {u}"));
            }
        }
        "failure" => {
            if let Some(err) = str_of(e, "error") {
                let rest = err.split('\n').skip(1).collect::<Vec<_>>().join(" ");
                let rest = rest.trim();
                if !rest.is_empty() {
                    parts.push(rest.chars().take(180).collect());
                }
            }
        }
        _ => {}
    }
    parts.join(" · ")
}

/// Everything search matches against (1.x `haystack`), lower-cased.
pub fn haystack(e: &Value) -> String {
    let s = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    [
        type_of(e).to_string(),
        badge_label(e).to_string(),
        primary_line(e),
        detail_line(e),
        s("source"),
        s("profileId"),
        s("profileName"),
        s("idpHost"),
        s("usernameHint"),
        s("error"),
        num_of(e, "roleCount").unwrap_or_default(),
        num_of(e, "durationMs").unwrap_or_default(),
    ]
    .join(" ")
    .to_lowercase()
}

/// Entries matching a search (all of them for a blank query).
pub fn filter<'a>(entries: &'a [Value], query: &str) -> Vec<&'a Value> {
    let q = query.trim().to_lowercase();
    entries.iter().filter(|e| q.is_empty() || haystack(e).contains(&q)).collect()
}

/// Local time like 1.x's `toLocaleString` with numeric date and seconds ("3/14/2026, 9:05:07 AM").
fn format_ts(e: &Value) -> String {
    use chrono::TimeZone as _;
    let ms = e.get("t").and_then(Value::as_f64).unwrap_or(0.0) as i64;
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(t) => t.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string(),
        None => String::new(),
    }
}

pub struct LogViewer {
    model: Entity<AppModel>,
    entries: Vec<Value>,
    error: Option<String>,
    loaded: bool,
    search: Entity<InputState>,
    confirming: bool,
    _subs: Vec<Subscription>,
}

impl LogViewer {
    fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subs = vec![cx.observe(&model, |_, _, cx| cx.notify()), cx.observe(&search, |_, _, cx| cx.notify())];
        let mut this = LogViewer { model, entries: Vec::new(), error: None, loaded: false, search, confirming: false, _subs: subs };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let b = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let r = b.call("logs:getAuthAuditEntries", vec![]).await;
            let _ = this.update(cx, |v, cx| {
                match r {
                    Ok(Value::Array(a)) => {
                        v.entries = a;
                        v.error = None;
                    }
                    Ok(_) => v.entries.clear(),
                    Err(e) => v.error = Some(e),
                }
                v.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.confirming = false;
        let b = self.model.read(cx).bridge.clone();
        cx.spawn(async move |this, cx| {
            let r = b.call("logs:clearAuthAudit", vec![]).await;
            let _ = this.update(cx, |v, cx| {
                if let Err(e) = r {
                    v.error = Some(e);
                }
                v.load(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn card(i: usize, e: &Value, p: &Palette) -> impl IntoElement {
        let badge_color = match type_of(e) {
            "failure" => p.danger,
            "idp_success" => p.success,
            _ => p.text_muted,
        };
        let primary = primary_line(e);
        let detail = detail_line(e);
        let ts = format_ts(e);
        let label = SharedString::from(format!("{} {} {}", badge_label(e), ts, primary));
        div()
            .id(("audit-entry", i))
            .role(Role::ListItem)
            .aria_label(label)
            .p(px(12.))
            .px(px(14.))
            .rounded(px(sz::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .mb(px(4.))
                    .child(div().font_family(FONT_MONO).text_size(px(sz::TEXT_XS)).text_color(p.text_muted).child(ts))
                    .child(div().text_size(px(sz::TEXT_XS)).font_weight(FontWeight::MEDIUM).text_color(badge_color).child(badge_label(e))),
            )
            .child(div().text_color(p.text).child(primary))
            .child(div().text_size(px(sz::TEXT_XS)).text_color(p.text_muted).child(detail))
    }
}

impl Render for LogViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.model.read(cx).palette;
        let query = self.search.read(cx).value().to_string();
        let shown = filter(&self.entries, &query);

        let body: gpui::AnyElement = if let Some(err) = &self.error {
            div().p(px(16.)).text_color(p.danger).child(read_text("audit-error", format!("Could not load log: {err}"))).into_any_element()
        } else if !self.loaded {
            div().into_any_element()
        } else if self.entries.is_empty() {
            div().py(px(48.)).flex().justify_center().text_color(p.text_muted).child(read_text("audit-empty", "No log entries yet.")).into_any_element()
        } else if shown.is_empty() {
            div()
                .py(px(48.))
                .flex()
                .justify_center()
                .text_color(p.text_muted)
                .child(read_text("audit-empty", "No entries match your search."))
                .into_any_element()
        } else {
            div()
                .id("audit-list")
                .role(Role::List)
                .aria_label("Auth audit entries")
                .flex()
                .flex_col()
                .gap(px(10.))
                .children(shown.iter().enumerate().map(|(i, e)| Self::card(i, e, &p)))
                .into_any_element()
        };

        let title_bar = TitleBar::new()
            .bg(p.rail)
            .border_b_1()
            .border_color(p.border)
            .child(div().text_size(px(sz::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(WINDOW_TITLE));

        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("audit-heading")
                            .role(Role::Heading)
                            .aria_label(WINDOW_TITLE)
                            .text_size(px(20.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(WINDOW_TITLE),
                    )
                    .child(div().text_size(px(sz::TEXT_SM)).text_color(p.text_muted).child(read_text("audit-retention", "Entries are deleted after 5 days"))),
            )
            .child(primary_button("clear-log", "Clear log", None, &p).on_activate({
                let this = cx.entity().downgrade();
                move |_, cx| {
                    let _ = this.update(cx, |v, cx| {
                        v.confirming = true;
                        cx.notify();
                    });
                }
            }));

        let mut root = div()
            .id("log-viewer-root")
            .role(Role::Application)
            .aria_label(WINDOW_TITLE)
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.text)
            .font_family(FONT_SANS)
            .text_size(px(sz::TEXT))
            .child(title_bar)
            .child(
                div().id("audit-scroll").flex_1().min_h(px(0.)).overflow_y_scroll().child(
                    div()
                        .max_w(px(920.))
                        .mx_auto()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(32.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(header)
                        .child(Input::new(&self.search).cleanable(true).role(Role::SearchInput).aria_label("Search"))
                        .child(body),
                ),
            );

        if self.confirming {
            let this = cx.entity().downgrade();
            let this2 = this.clone();
            let this3 = this.clone();
            let card = div()
                .id("clear-confirm")
                // Escape cancels.
                .on_key_down(move |ev: &gpui::KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        let _ = this3.update(cx, |v, cx| {
                            v.confirming = false;
                            cx.notify();
                        });
                    }
                })
                .role(Role::AlertDialog)
                .aria_label("Clear log")
                .w(px(400.))
                .p(px(24.))
                .rounded(px(sz::RADIUS_LG + 2.))
                .border_1()
                .border_color(p.border)
                .bg(p.panel)
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(read_text("clear-confirm-text", "Clear all auth audit log entries?")))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(secondary_button("clear-cancel", "Cancel", None, &p).on_activate(move |_, cx| {
                            let _ = this.update(cx, |v, cx| {
                                v.confirming = false;
                                cx.notify();
                            });
                        }))
                        .child(primary_button("clear-ok", "Clear", None, &p).bg(p.danger_solid).text_color(gpui::white()).on_activate(move |_, cx| {
                            let _ = this2.update(cx, |v, cx| v.clear(cx));
                        })),
                );
            root = root.child(crate::motion::fade_in(
                div().absolute().inset_0().bg(p.scrim).flex().items_center().justify_center().child(crate::motion::dialog_in(card, "clear-confirm-in")),
                "clear-scrim-in",
                crate::motion::POPOVER,
            ));
        }
        root
    }
}

/// Open the viewer, or reload and focus it when it is already open (1.x reloaded the page).
pub fn open(model: &Entity<AppModel>, cx: &mut App) {
    if let Some(OpenViewer(handle, view)) = cx.try_global::<OpenViewer>().map(|o| OpenViewer(o.0, o.1.clone())) {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            view.update(cx, |v, cx| v.load(cx));
            return;
        }
    }
    let test_display = cx.try_global::<crate::TestPlacement>().map(|t| t.0);
    let bounds = Bounds::centered(test_display, size(px(880.), px(640.)), cx);
    let model = model.clone();
    let mut created: Option<Entity<LogViewer>> = None;
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(480.), px(360.))),
            app_id: Some("AWSProfileManager".into()),
            focus: test_display.is_none(),
            display_id: test_display,
            titlebar: Some(gpui::TitlebarOptions { title: Some(WINDOW_TITLE.into()), ..TitleBar::title_bar_options() }),
            ..TitleBar::window_options()
        },
        |window, cx| {
            let view = cx.new(|cx| LogViewer::new(model.clone(), window, cx));
            created = Some(view.clone());
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
    if let (Ok(handle), Some(view)) = (opened, created) {
        let any: AnyWindowHandle = handle.into();
        cx.set_global(OpenViewer(any, view));
        crate::notify_window_opened(any, "logViewer", cx);
    }
}

/// The audit log changed: an open viewer reloads.
pub fn reload(cx: &mut App) {
    let Some(view) = cx.try_global::<OpenViewer>().map(|o| o.1.clone()) else { return };
    view.update(cx, |v, cx| v.load(cx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lines_match_1x() {
        let req = json!({ "t": 0, "type": "idp_request", "source": "refresh", "profileId": "p1", "profileName": "Dev", "idpHost": "idp.example.com", "usernameHint": "j***" });
        assert_eq!(primary_line(&req), "POST IdP sign-in · idp.example.com");
        assert_eq!(detail_line(&req), "method: POST · source: refresh · profile: Dev (p1) · host: idp.example.com · user: j***");
        assert_eq!(badge_label(&req), "Request");

        let ok = json!({ "t": 0, "type": "idp_success", "source": "refresh", "roleCount": 3, "durationMs": 812 });
        assert_eq!(primary_line(&ok), "IdP sign-in succeeded — SAML received");
        assert_eq!(detail_line(&ok), "result: SAML assertion · source: refresh · roles: 3 · 812 ms");

        let fail = json!({ "t": 0, "type": "failure", "source": "scheduler", "error": "Bad password\n  at line 2\nmore" });
        assert_eq!(primary_line(&fail), "Bad password");
        assert_eq!(detail_line(&fail), "source: scheduler · at line 2 more");
        assert_eq!(badge_label(&fail), "Failure");
    }

    #[test]
    fn search_filters_case_insensitively() {
        let entries = vec![
            json!({ "type": "idp_request", "source": "refresh", "idpHost": "Okta.example.com" }),
            json!({ "type": "failure", "source": "scheduler", "error": "timeout" }),
        ];
        assert_eq!(filter(&entries, "").len(), 2);
        assert_eq!(filter(&entries, "  OKTA ").len(), 1);
        assert_eq!(filter(&entries, "failure").len(), 1);
        assert_eq!(filter(&entries, "nothing").len(), 0);
    }
}
