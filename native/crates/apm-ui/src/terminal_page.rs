//! The Terminal page. Port of 1.x `TerminalScreen.tsx`: top bar (shell, AWS CLI profile and
//! "append --profile", docs link), the command explorer on the left, the embedded terminal in the
//! middle, the AI assistant on the right (`terminal_ai`) and the command details underneath.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui::{div, prelude::*, px, AnyElement, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Task, Window};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use serde_json::{json, Value};

use crate::a11y::{A11yDisabledExt as _, PressExt as _};
use crate::model::AppModel;
use crate::picker::{Picker, PickerEvent, PickerOption};
use crate::terminal_ai::{AiEvent, AiPanel};
use crate::terminal_view::TerminalView;
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{icon, icon_button, primary_button, read_text, FocusRing as _};

const AWS_CLI_DOCS: &str = "https://docs.aws.amazon.com/cli/latest/";
const AWS_CLI_REFERENCE_BASE: &str = "https://docs.aws.amazon.com/cli/latest/reference";
const EXPLORER_W: f32 = 280.;
const COLLAPSED_W: f32 = 40.;
const DETAILS_H: f32 = 240.;
/// 1.x: scraped search waits for a pause in typing and needs two characters.
const SCRAPED_DEBOUNCE: Duration = Duration::from_millis(280);
const MIN_SCRAPED_QUERY: usize = 2;

// ------------------------------------------------------------------------------------------------
// AWS CLI command data (1.x awsCliMockData.ts + the scraped per-service commands)
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CliCommand {
    pub id: String,
    pub name: String,
    pub description: String,
    pub syntax: String,
    /// (name, description, required)
    pub options: Vec<(String, String, bool)>,
    /// (description, command)
    pub examples: Vec<(String, String)>,
    pub doc_url: Option<String>,
}

impl CliCommand {
    fn from_value(v: &Value) -> Self {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let list = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let st = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        CliCommand {
            id: s("id"),
            name: s("name"),
            description: s("description"),
            syntax: s("syntax"),
            options: list("options")
                .iter()
                .map(|o| (st(o, "name"), st(o, "description"), o.get("required").and_then(Value::as_bool).unwrap_or(false)))
                .collect(),
            examples: list("examples").iter().map(|e| (st(e, "description"), st(e, "command"))).collect(),
            doc_url: v.get("docUrl").and_then(Value::as_str).map(str::to_string),
        }
    }

    /// 1.x getCommandDocUrl: `s3-mb` -> `.../reference/s3/mb.html`.
    fn doc_url(&self) -> String {
        if let Some(u) = &self.doc_url {
            return u.clone();
        }
        match self.id.split_once('-') {
            None => format!("{AWS_CLI_REFERENCE_BASE}/{}/", self.id),
            Some((service, cmd)) => format!("{AWS_CLI_REFERENCE_BASE}/{service}/{cmd}.html"),
        }
    }
}

pub struct CliService {
    pub id: String,
    pub name: String,
    pub commands: Vec<Arc<CliCommand>>,
}

/// The built-in command tree, parsed once.
fn built_in() -> &'static [CliService] {
    static TREE: OnceLock<Vec<CliService>> = OnceLock::new();
    TREE.get_or_init(|| {
        let raw: Vec<Value> = serde_json::from_str(include_str!("../assets/aws-cli-commands.json")).unwrap_or_default();
        raw.iter()
            .map(|s| CliService {
                id: s.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                name: s.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                commands: s.get("children").and_then(Value::as_array).into_iter().flatten().map(|c| Arc::new(CliCommand::from_value(c))).collect(),
            })
            .collect()
    })
}

fn matches_query(c: &CliCommand, q: &str) -> bool {
    c.name.to_lowercase().contains(q) || c.description.to_lowercase().contains(q)
}

// ------------------------------------------------------------------------------------------------
// The page
// ------------------------------------------------------------------------------------------------

pub struct TerminalPage {
    model: Entity<AppModel>,
    terminal: Entity<TerminalView>,
    ai: Entity<AiPanel>,
    shell_picker: Entity<Picker>,
    profile_picker: Entity<Picker>,
    search: Entity<InputState>,
    query: String,
    selected: Option<Arc<CliCommand>>,
    /// Profile id for inserted commands (`None`: no profile). A per-session choice; the saved
    /// default is Settings > Terminal (`defaultTerminalProfileId`).
    profile_id: Option<String>,
    /// The `defaultTerminalProfileId` last applied (1.x `appliedDefaultRef`): re-applied only when
    /// the setting itself changes, so a manual pick here survives revisits.
    applied_default: Option<Option<String>>,
    explorer_collapsed: bool,
    details_collapsed: bool,
    /// Scraped AWS CLI docs: the service list (fetched on the first search) and matches.
    service_list: Option<Vec<String>>,
    scraped: Vec<(String, Arc<CliCommand>)>,
    scraped_loading: bool,
    search_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

fn is_windows() -> bool {
    cfg!(windows)
}

/// (value, label) of the shells on offer. 'powershell' is the system shell: PowerShell on Windows,
/// the login shell elsewhere. Bash needs a configured path on Windows (1.x TerminalTopBar).
fn shell_options(bash_path: &str) -> Vec<PickerOption> {
    let mut v = vec![PickerOption { value: "powershell".into(), label: if is_windows() { "PowerShell" } else { "Default shell" }.into() }];
    if !is_windows() || !bash_path.trim().is_empty() {
        v.push(PickerOption { value: "bash".into(), label: "Bash".into() });
    }
    v
}

fn preferred_shell(settings: &Value) -> String {
    let bash_path = settings.get("bashPath").and_then(Value::as_str).unwrap_or("");
    let bash_unavailable = is_windows() && bash_path.trim().is_empty();
    match settings.get("terminalShell").and_then(Value::as_str) {
        Some("bash") if !bash_unavailable => "bash".into(),
        _ => "powershell".into(),
    }
}

impl TerminalPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (core, settings) = {
            let m = model.read(cx);
            (m.bridge.core().clone(), m.settings.clone())
        };
        let shell = preferred_shell(&settings);
        let terminal = cx.new(|cx| TerminalView::new(core, shell.clone(), cx));
        let shell_picker = cx.new(|cx| {
            let mut p = Picker::new("terminal-shell", "Terminal shell", cx);
            p.set_options(shell_options(settings.get("bashPath").and_then(Value::as_str).unwrap_or("")), cx);
            p.set_selected(Some(shell), cx);
            p
        });
        let profile_picker = cx.new(|cx| Picker::new("terminal-profile", "AWS CLI profile for inserted commands", cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search commands"));
        let ai = cx.new(|cx| AiPanel::new(model.clone(), window, cx));
        let subs = vec![
            cx.subscribe_in(&ai, window, |this, _, ev: &AiEvent, window, cx| match ev {
                AiEvent::Insert(cmd) => this.insert_command(&cmd.clone(), window, cx),
            }),
            // Configured or not decides the "Ask AI" button in the command details.
            cx.observe(&ai, |_, _, cx| cx.notify()),
            cx.observe(&model, |this, _, cx| {
                this.sync_from_model(cx);
                cx.notify();
            }),
            // Observed, not subscribed to Change: an accessible SetValue edits without that event.
            cx.observe(&search, |this, input, cx| {
                let q = input.read(cx).value().to_string();
                if q != this.query {
                    this.query = q;
                    this.schedule_scraped(cx);
                    cx.notify();
                }
            }),
            cx.subscribe(&shell_picker, |this, _, ev: &PickerEvent, cx| {
                let PickerEvent::Changed(Some(shell)) = ev else { return };
                let shell = shell.clone();
                if this.terminal.read(cx).shell() != shell {
                    this.model.update(cx, |m, cx| m.set_setting("terminalShell", json!(shell), cx));
                    this.terminal.update(cx, |t, cx| t.restart(shell, cx));
                }
            }),
            cx.subscribe(&profile_picker, |this, _, ev: &PickerEvent, cx| {
                let PickerEvent::Changed(v) = ev;
                this.profile_id = v.clone().filter(|s| !s.is_empty());
                cx.notify();
            }),
        ];
        let mut this = TerminalPage {
            model,
            terminal,
            ai,
            shell_picker,
            profile_picker,
            search,
            query: String::new(),
            selected: None,
            profile_id: None,
            applied_default: None,
            explorer_collapsed: false,
            details_collapsed: false,
            service_list: None,
            scraped: Vec::new(),
            scraped_loading: false,
            search_task: None,
            _subs: subs,
        };
        this.sync_from_model(cx);
        let t = this.terminal.clone();
        t.update(cx, |t, cx| t.focus(window, cx));
        this
    }

    /// Profiles and settings changed: picker options, the default profile, Bash availability.
    fn sync_from_model(&mut self, cx: &mut Context<Self>) {
        let (profiles, settings, loaded) = {
            let m = self.model.read(cx);
            (m.profiles.clone(), m.settings.clone(), m.loaded)
        };
        let bash_path = settings.get("bashPath").and_then(Value::as_str).unwrap_or("").to_string();
        self.shell_picker.update(cx, |p, cx| p.set_options(shell_options(&bash_path), cx));

        let mut opts = vec![PickerOption { value: String::new(), label: "No profile".into() }];
        opts.extend(profiles.iter().map(|p| PickerOption {
            value: p.get("id").and_then(Value::as_str).unwrap_or("").into(),
            label: p.get("name").and_then(Value::as_str).unwrap_or("").to_string().into(),
        }));
        if let Some(id) = &self.profile_id {
            if !profiles.iter().any(|p| p["id"] == id.as_str()) {
                self.profile_id = None;
            }
        }
        let default = settings.get("defaultTerminalProfileId").and_then(Value::as_str).map(str::to_string);
        if loaded && !profiles.is_empty() && settings.is_object() && self.applied_default.as_ref() != Some(&default) {
            self.applied_default = Some(default.clone());
            let ids: Vec<&str> = profiles.iter().filter_map(|p| p.get("id").and_then(Value::as_str)).collect();
            self.profile_id = apm_core::terminal_profile::resolve_terminal_profile_id(default.as_deref(), &ids).map(str::to_string);
        }
        let selected = Some(self.profile_id.clone().unwrap_or_default());
        self.profile_picker.update(cx, |p, cx| {
            p.set_options(opts, cx);
            p.set_selected(selected, cx);
        });
    }

    /// The `--profile` value for the chosen profile: its credentials section, else its name.
    fn profile_flag_name(&self, cx: &Context<Self>) -> Option<String> {
        let id = self.profile_id.as_ref()?;
        let m = self.model.read(cx);
        let p = m.profile(id)?;
        let pick = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        pick("credentialProfileName").or_else(|| pick("name"))
    }

    /// 1.x handleInsertCommand: the command (no Enter), with `--profile` when one is chosen.
    fn insert_command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut cmd = command.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        if !cmd.contains("--profile") {
            if let Some(name) = self.profile_flag_name(cx) {
                cmd = format!("{cmd} --profile {name}");
            }
        }
        self.terminal.update(cx, |t, cx| {
            t.write(&cmd, cx);
            t.focus(window, cx);
        });
    }

    /// 1.x handleInsertProfileFlag: append ` --profile <name>` to whatever is typed at the prompt.
    fn append_profile_flag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.profile_flag_name(cx) else { return };
        self.terminal.update(cx, |t, cx| {
            t.write(&format!(" --profile {name}"), cx);
            t.focus(window, cx);
        });
    }

    fn select_command(&mut self, cmd: Arc<CliCommand>, cx: &mut Context<Self>) {
        self.selected = Some(cmd);
        self.details_collapsed = false;
        cx.notify();
    }

    /// Search the scraped AWS CLI docs for services the built-in tree does not cover.
    fn schedule_scraped(&mut self, cx: &mut Context<Self>) {
        self.scraped.clear();
        self.scraped_loading = false;
        let q = self.query.trim().to_lowercase();
        if q.chars().count() < MIN_SCRAPED_QUERY {
            self.search_task = None;
            return;
        }
        let bridge = self.model.read(cx).bridge.clone();
        let known = self.service_list.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCRAPED_DEBOUNCE).await;
            let list = match known {
                Some(l) => l,
                None => {
                    let _ = this.update(cx, |t, cx| {
                        t.scraped_loading = true;
                        cx.notify();
                    });
                    match bridge.call("awsCli:getCachedServiceList", vec![]).await {
                        Ok(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>(),
                        _ => {
                            let _ = this.update(cx, |t, cx| {
                                t.scraped_loading = false;
                                cx.notify();
                            });
                            return;
                        }
                    }
                }
            };
            let built_in_ids: Vec<&str> = built_in().iter().filter(|s| !s.commands.is_empty()).map(|s| s.id.as_str()).collect();
            let services: Vec<String> = list.iter().filter(|s| s.to_lowercase().contains(&q) && !built_in_ids.contains(&s.as_str())).cloned().collect();
            let _ = this.update(cx, |t, cx| {
                if !list.is_empty() {
                    t.service_list = Some(list.clone());
                }
                t.scraped_loading = !services.is_empty();
                cx.notify();
            });
            if services.is_empty() {
                return;
            }
            let calls = services.iter().map(|slug| bridge.call("awsCli:getCachedCommands", vec![json!(slug)]));
            let results = futures::future::join_all(calls).await;
            let mut out = Vec::new();
            for (slug, r) in services.iter().zip(results) {
                for v in r.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default() {
                    let c = CliCommand::from_value(&v);
                    if matches_query(&c, &q) || c.syntax.to_lowercase().contains(&q) {
                        out.push((slug.clone(), Arc::new(c)));
                    }
                }
            }
            let _ = this.update(cx, |t, cx| {
                if t.query.trim().to_lowercase() == q {
                    t.scraped = out;
                    t.scraped_loading = false;
                    cx.notify();
                }
            });
        }));
    }

    // --------------------------------------------------------------------------------------------
    // Rendering
    // --------------------------------------------------------------------------------------------

    fn top_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let flag = self.profile_flag_name(cx);
        let append_label: SharedString = match &flag {
            Some(n) => format!("Append --profile {n} to the terminal").into(),
            None => "Append --profile".into(),
        };
        let tip: SharedString = match &flag {
            Some(n) => format!("Append --profile {n} to the terminal").into(),
            None => "Select a profile to append --profile".into(),
        };
        let enabled = flag.is_some();
        let label = |t: &'static str| div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(t);
        let append = div()
            .id("terminal-append-profile")
            .role(Role::Button)
            .aria_label(append_label)
            .focusable()
            .tab_stop(true)
            .focus_ring(p)
            .size(px(size::CONTROL))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .border_1()
            .border_color(p.border)
            .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(p.hover)))
            .when(!enabled, |d| d.opacity(0.5))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_press(cx.listener(|this, _, window, cx| this.append_profile_flag(window, cx)))
            .child(icon("plus", 14., p.text_muted))
            .a11y_disabled(!enabled);

        let bridge = self.model.read(cx).bridge.clone();
        let docs = div()
            .id("terminal-docs")
            .role(Role::Link)
            .aria_label("AWS CLI docs")
            .focusable()
            .tab_stop(true)
            .focus_ring(p)
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .h(px(size::CONTROL))
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(size::TEXT_SM))
            .text_color(p.text_muted)
            .hover(|s| s.bg(p.hover).text_color(p.text))
            .tooltip(|window, cx| Tooltip::new("Open AWS CLI documentation").build(window, cx))
            .on_press(move |_, _, _| {
                let _ = bridge.call("app:openExternal", vec![json!(AWS_CLI_DOCS)]);
            })
            .child(icon("book-open", 14., p.text_muted))
            .child("AWS CLI docs");

        div()
            .flex_shrink_0()
            .h(px(52.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(16.))
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("terminal", 18., p.text_muted))
                    .child(div().id("terminal-title").role(Role::Heading).aria_label("Terminal").font_weight(FontWeight::MEDIUM).child("Terminal")),
            )
            .child(div().flex().items_center().gap(px(8.)).child(label("Shell")).child(div().w(px(150.)).child(self.shell_picker.clone())))
            .child(div().flex().items_center().gap(px(8.)).child(label("Profile")).child(div().w(px(180.)).child(self.profile_picker.clone())).child(append))
            .child(div().flex_1())
            .child(icon_button("terminal-clear", "eraser", "Clear terminal", p).on_press(cx.listener(|this, _, window, cx| {
                this.terminal.update(cx, |t, cx| {
                    t.clear(cx);
                    t.focus(window, cx);
                });
            })))
            .child(icon_button("terminal-restart", "rotate-ccw", "Restart shell", p).on_press(cx.listener(|this, _, window, cx| {
                this.terminal.update(cx, |t, cx| {
                    let shell = t.shell().to_string();
                    t.restart(shell, cx);
                    t.focus(window, cx);
                });
            })))
            .child(docs)
    }

    fn command_button(&self, id: SharedString, cmd: &Arc<CliCommand>, parent: Option<&str>, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_ref().is_some_and(|s| Arc::ptr_eq(s, cmd) || (s.id == cmd.id && !s.id.is_empty()));
        let c = cmd.clone();
        let tip: SharedString = if cmd.description.is_empty() { cmd.syntax.clone().into() } else { cmd.description.clone().into() };
        let base = div()
            .id(id)
            .role(Role::Button)
            .focusable()
            .tab_stop(true)
            .focus_ring(p)
            .w_full()
            .rounded(px(8.))
            .px(px(10.))
            .cursor_pointer()
            .text_size(px(size::TEXT_SM))
            .when(selected, |d| d.bg(p.raised).text_color(p.text))
            .when(!selected, |d| d.text_color(p.text_muted).hover(|s| s.bg(p.hover).text_color(p.text)))
            .on_press(cx.listener(move |this, _, _, cx| this.select_command(c.clone(), cx)));
        match parent {
            // Tree: the command name alone (1.x TreeView).
            None => base
                .aria_label(SharedString::from(cmd.name.clone()))
                .h(px(30.))
                .flex()
                .items_center()
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(div().font_family(FONT_MONO).child(cmd.name.clone()))
                .into_any_element(),
            // Search results: "service command" and the description (1.x SearchResults).
            Some(parent) => {
                let title = if parent.is_empty() { cmd.name.clone() } else { format!("{parent} {}", cmd.name) };
                let detail = if cmd.description.is_empty() { cmd.syntax.clone() } else { cmd.description.clone() };
                let name = if detail.is_empty() { title.clone() } else { format!("{title} {detail}") };
                base.aria_label(SharedString::from(name))
                    .py(px(6.))
                    .child(div().font_family(FONT_MONO).child(title))
                    .when(!detail.is_empty(), |d| d.child(div().mt(px(2.)).truncate().text_color(p.text_faint).child(detail)))
                    .into_any_element()
            }
        }
    }

    fn explorer(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        if self.explorer_collapsed {
            return div()
                .flex_shrink_0()
                .w(px(COLLAPSED_W))
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .py(px(6.))
                .rounded(px(size::RADIUS))
                .border_1()
                .border_color(p.border)
                .bg(p.panel)
                .child(icon_button("explorer-toggle", "chevrons-right", "Expand Command explorer", p).on_press(cx.listener(|this, _, _, cx| {
                    this.explorer_collapsed = false;
                    cx.notify();
                })))
                .into_any_element();
        }
        let q = self.query.trim().to_lowercase();
        let mut list = div().id("command-list").flex_1().min_h(px(0.)).overflow_y_scroll().p(px(6.)).flex().flex_col().gap(px(2.));
        if q.is_empty() {
            for (si, service) in built_in().iter().enumerate() {
                if service.commands.is_empty() {
                    let c = Arc::new(CliCommand {
                        id: service.id.clone(),
                        name: service.name.clone(),
                        description: String::new(),
                        syntax: String::new(),
                        options: vec![],
                        examples: vec![],
                        doc_url: None,
                    });
                    list = list.child(self.command_button(format!("svc-{si}").into(), &c, None, p, cx));
                    continue;
                }
                list = list.child(div().px(px(10.)).pt(px(10.)).pb(px(2.)).text_size(px(size::TEXT_SM)).text_color(p.text_faint).child(service.name.clone()));
                let mut group = div().ml(px(10.)).pl(px(6.)).border_l_1().border_color(p.border).flex().flex_col().gap(px(1.));
                for (ci, cmd) in service.commands.iter().enumerate() {
                    group = group.child(self.command_button(format!("cmd-{si}-{ci}").into(), cmd, None, p, cx));
                }
                list = list.child(group);
            }
        } else {
            let mut found = 0;
            for (si, service) in built_in().iter().enumerate() {
                for (ci, cmd) in service.commands.iter().enumerate() {
                    if matches_query(cmd, &q) {
                        found += 1;
                        list = list.child(self.command_button(format!("hit-{si}-{ci}").into(), cmd, Some(&service.name), p, cx));
                    }
                }
            }
            for (i, (slug, cmd)) in self.scraped.iter().enumerate() {
                found += 1;
                list = list.child(self.command_button(format!("scraped-{i}").into(), cmd, Some(slug), p, cx));
            }
            if self.scraped_loading {
                list = list.child(
                    div()
                        .id("explorer-searching")
                        .role(Role::Status)
                        .aria_label("Searching")
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(size::TEXT_XS))
                        .text_color(p.text_faint)
                        .child("Searching the AWS CLI docs…"),
                );
            } else if found == 0 {
                list = list.child(
                    div().px(px(10.)).py(px(12.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(
                        "No commands match. Try \u{201c}s3\u{201d}, \u{201c}sts\u{201d}, \u{201c}lambda\u{201d}, \u{201c}list\u{201d}, or \u{201c}describe\u{201d} (built-in). More services appear when AWS CLI docs load.",
                    ),
                );
            }
        }

        div()
            .id("command-explorer")
            .role(Role::Complementary)
            .aria_label("Command explorer")
            .flex_shrink_0()
            .w(px(EXPLORER_W))
            .h_full()
            .flex()
            .flex_col()
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .overflow_hidden()
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(44.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(icon_button("explorer-toggle", "chevrons-left", "Collapse Command explorer", p).on_press(cx.listener(|this, _, _, cx| {
                        this.explorer_collapsed = true;
                        cx.notify();
                    })))
                    .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("Command explorer")),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .p(px(8.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(Input::new(&self.search).cleanable(true).role(Role::SearchInput).aria_label("Search AWS CLI commands")),
            )
            .child(list)
            .into_any_element()
    }

    fn details(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.details_collapsed;
        let toggle = icon_button(
            "details-toggle",
            if collapsed { "chevron-up" } else { "chevron-down" },
            if collapsed { "Expand Command details" } else { "Collapse Command details" },
            p,
        )
        .on_press(cx.listener(move |this, _, _, cx| {
            this.details_collapsed = !collapsed;
            cx.notify();
        }));
        let header = div()
            .flex_shrink_0()
            .h(px(40.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(toggle)
            .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child("Command details"));
        let mut panel = div().flex_shrink_0().flex().flex_col().border_t_1().border_color(p.border).bg(p.panel).child(header);
        if collapsed {
            return panel;
        }
        match &self.selected {
            // Nothing chosen yet: just the hint, so the terminal keeps the room.
            None => panel.child(
                div()
                    .px(px(16.))
                    .pb(px(12.))
                    .text_size(px(size::TEXT_SM))
                    .text_color(p.text_muted)
                    .child("Select a command from the explorer to view details and examples."),
            ),
            Some(cmd) => {
                let body = self.command_details(cmd.clone(), p, cx);
                panel = panel.h(px(DETAILS_H)).child(div().id("details-body").flex_1().min_h(px(0.)).overflow_y_scroll().child(body));
                panel
            }
        }
    }

    fn command_details(&self, cmd: Arc<CliCommand>, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let bridge = self.model.read(cx).bridge.clone();
        let url = cmd.doc_url();
        let section = |t: &'static str| div().mt(px(14.)).mb(px(6.)).text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).child(t);
        let code = |text: String| {
            div().p(px(8.)).rounded(px(8.)).border_1().border_color(p.border).bg(p.sunken).font_family(FONT_MONO).text_size(px(size::TEXT_XS)).child(text)
        };
        let mut body = div()
            .px(px(16.))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(div().font_family(FONT_MONO).font_weight(FontWeight::MEDIUM).child(cmd.name.clone()))
                    .children(self.ai.read(cx).configured().then(|| {
                        let c = cmd.clone();
                        div()
                            .id("cmd-ask-ai")
                            .role(Role::Button)
                            .aria_label("Ask AI how to use this")
                            .focusable()
                            .tab_stop(true)
                            .focus_ring(p)
                            .h(px(26.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.border)
                            .text_size(px(size::TEXT_XS))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover))
                            .on_press(cx.listener(move |this, _, _, cx| {
                                let prompt = ask_ai_prompt(&c);
                                this.ai.update(cx, |a, cx| a.send_prompt(prompt, cx));
                            }))
                            .child(icon("sparkles", 13., p.text_muted))
                            .child("Ask AI how to use this")
                    })),
            )
            .child(div().mt(px(4.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(read_text("cmd-description", cmd.description.clone())))
            .child(
                div()
                    .id("cmd-docs")
                    .role(Role::Link)
                    .aria_label("Read more about this command in the AWS CLI Docs")
                    .mt(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(size::TEXT_SM))
                    .text_color(p.accent)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .on_press(move |_, _, _| {
                        let _ = bridge.call("app:openExternal", vec![json!(url)]);
                    })
                    .child("Read more about this command in the AWS CLI Docs")
                    .child(icon("external-link", 12., p.accent)),
            )
            .child(section("Syntax"))
            .child(code(cmd.syntax.clone()));
        if !cmd.options.is_empty() {
            let mut opts = div().flex().flex_col().gap(px(4.)).text_size(px(size::TEXT_SM));
            for (name, desc, required) in &cmd.options {
                opts = opts.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(div().font_family(FONT_MONO).child(name.clone()))
                        .when(*required, |d| d.child(div().text_size(px(size::TEXT_XS)).text_color(p.danger).child("required")))
                        .child(div().text_color(p.text_muted).child(format!("\u{2014} {desc}"))),
                );
            }
            body = body.child(section("Options")).child(opts);
        }
        if cmd.examples.is_empty() {
            body = body.child(div().mt(px(14.)).text_size(px(size::TEXT_SM)).text_color(p.text_muted).child("No examples for this command."));
        } else {
            body = body.child(section("Examples"));
            for (i, (desc, command)) in cmd.examples.iter().enumerate() {
                let command_owned = command.clone();
                body = body.child(
                    div()
                        .mb(px(10.))
                        .p(px(10.))
                        .rounded(px(size::RADIUS))
                        .border_1()
                        .border_color(p.border)
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(desc.clone()))
                        .child(code(command.clone()))
                        .child(
                            div().flex().child(
                                primary_button(SharedString::from(format!("insert-{i}")), "Insert into terminal", None, p)
                                    .h(px(30.))
                                    .text_size(px(size::TEXT_SM))
                                    .on_press(cx.listener(move |this, _, window, cx| this.insert_command(&command_owned, window, cx))),
                            ),
                        ),
                );
            }
        }
        body
    }
}

/// 1.x CommandDetailsPanel handleAskAI: anchor the question to this command with its scraped
/// description and option names, rather than the bare syntax.
fn ask_ai_prompt(c: &CliCommand) -> String {
    let mut parts = vec![format!("How do I use the AWS CLI command `{}`?", c.name)];
    if !c.description.trim().is_empty() {
        parts.push(format!(
            "
What the docs say it does: {}",
            c.description.trim()
        ));
    }
    parts.push(format!(
        "
Syntax: {}",
        c.syntax
    ));
    let names: Vec<&str> = c.options.iter().map(|o| o.0.as_str()).filter(|n| !n.is_empty()).take(25).collect();
    if !names.is_empty() {
        parts.push(format!(
            "
Documented options: {}",
            names.join(", ")
        ));
    }
    parts.push("
Explain what it does, cover the options that matter most in practice, and give at least two worked examples with realistic values. Call out anything destructive.".into());
    parts.join(
        "
",
    )
}

impl Render for TerminalPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.model.read(cx).surface_palette();
        let terminal_card =
            div().flex_1().min_w(px(300.)).h_full().rounded(px(size::RADIUS)).border_1().border_color(p.border).overflow_hidden().child(self.terminal.clone());
        div()
            .id("terminal-page")
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .child(self.top_bar(&p, cx))
            .child(div().flex_1().min_h(px(0.)).p(px(8.)).flex().gap(px(8.)).child(self.explorer(&p, cx)).child(terminal_card).child(self.ai.clone()))
            .child(self.details(&p, cx))
    }
}
