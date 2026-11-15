//! The Profiles screen (mockup "direction B"): every profile visible with one-click Console /
//! Refresh, folders as sections, and an optional side panel with the session, shell snippets,
//! sign-in details and recent activity. The 1.x-style card list is the `Cards` option (later).

use gpui::{div, prelude::*, px, Context, Entity, FontWeight, IntoElement, Role, SharedString, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::Value;

use crate::model::{AppModel, Filter};
use crate::status::{format_countdown, format_expiry, short_label, CredState};
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{icon, icon_button, mono, primary_button, secondary_button, status_dot};

pub struct ProfilesView {
    model: Entity<AppModel>,
    search: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl ProfilesView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subs = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |this, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    let q = input.read(cx).value().to_string();
                    this.model.update(cx, |m, cx| {
                        m.search = q;
                        cx.notify();
                    });
                }
            }),
        ];
        ProfilesView { model, search, _subs: subs }
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn dot_color(p: &Palette, state: CredState) -> Option<gpui::Rgba> {
    match state {
        CredState::Active => Some(p.success),
        CredState::Expiring => Some(p.warning),
        CredState::Expired => Some(p.danger),
        CredState::Never => None,
    }
}

fn parse_hex(color: &str) -> Option<gpui::Rgba> {
    let h = color.strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten().map(gpui::rgb)
}

/// The role / account line under the name: what the profile list showed in 1.x (`accountNumber`).
fn subtitle(summary: Option<&Value>) -> String {
    summary.map(|d| s(d, "accountNumber").to_string()).unwrap_or_default()
}

impl Render for ProfilesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx).snapshot();
        let p = m.palette;
        let query = m.search.to_lowercase();

        // Which profiles show, per the search box and the filter.
        let visible: Vec<&Value> = m
            .profiles
            .iter()
            .filter(|prof| {
                let id = s(prof, "id");
                let state = m.summary(id).map(CredState::of).unwrap_or(CredState::Never);
                let filter_ok = match m.filter {
                    Filter::All => true,
                    Filter::Expiring => state == CredState::Expiring,
                    Filter::NeedsSignIn => state.needs_sign_in(),
                };
                let hay = format!("{} {} {}", s(prof, "name"), s(prof, "label"), subtitle(m.summary(id))).to_lowercase();
                filter_ok && (query.is_empty() || hay.contains(&query))
            })
            .collect();

        let total = m.profiles.len();
        let active = m.dashboard.iter().filter(|d| s(d, "status") == "active").count();
        let next_expiry = m
            .dashboard
            .iter()
            .filter(|d| s(d, "status") == "active")
            .filter_map(|d| Some((d.get("timeRemainingSeconds")?.as_i64()?, s(d, "name").to_string())))
            .min_by_key(|(t, _)| *t);
        let mut counts = format!("{total} · {active} active");
        if let Some((t, name)) = next_expiry {
            counts.push_str(&format!(" · next expiry {name} in {}", format_countdown(t)));
        }

        // Folder sections, then the profiles with no (existing) folder.
        let folder_ids: Vec<String> = m.folders.iter().map(|f| s(f, "id").to_string()).collect();
        let mut sections: Vec<(Option<&Value>, Vec<&Value>)> =
            m.folders.iter().map(|f| (Some(f), visible.iter().copied().filter(|prof| s(prof, "folderId") == s(f, "id")).collect())).collect();
        let ungrouped: Vec<&Value> =
            visible.iter().copied().filter(|prof| !folder_ids.iter().any(|id| id == s(prof, "folderId"))).collect();
        sections.push((None, ungrouped));
        let searching = !query.is_empty() || m.filter != Filter::All;

        let mut list = div().id("profile-list").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(16.)).pb(px(16.));
        for (folder, rows) in sections {
            if folder.is_none() && rows.is_empty() {
                continue;
            }
            if folder.is_some() && rows.is_empty() && searching {
                continue;
            }
            list = list.child(self.render_section(folder, rows, &p, cx));
        }
        if m.loaded && total == 0 {
            list = list.child(
                div()
                    .mt(px(48.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).child("No profiles yet"))
                    .child(div().text_color(p.text_muted).child("A profile signs in to one AWS role and keeps its credentials file entry fresh")),
            );
        }

        let filters = [(Filter::All, "All"), (Filter::Expiring, "Expiring"), (Filter::NeedsSignIn, "Needs sign-in")];
        let mut segmented = div()
            .id("filter")
            .role(Role::Group)
            .aria_label("Show")
            .flex()
            .p(px(2.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.sunken);
        for (f, label) in filters {
            let on = m.filter == f;
            segmented = segmented.child(
                div()
                    .id(label)
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                    .focusable()
                    .h(px(30.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .text_size(px(size::TEXT_SM))
                    .cursor_pointer()
                    .when(on, |d| d.bg(p.raised).text_color(p.text))
                    .when(!on, |d| d.text_color(p.text_muted))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.filter = f;
                            cx.notify();
                        })
                    }))
                    .child(label),
            );
        }

        let header = div()
            .px(px(24.))
            .pt(px(20.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().id("profiles-heading").role(Role::Heading).text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("Profiles"))
                    .child(mono(counts, p.text_faint))
                    .child(div().flex_1())
                    .child(icon_button("new-folder", "folder-plus", "New folder", &p).on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.run("folders:save", vec![serde_json::json!({})], cx))
                    })))
                    .child(primary_button("add-profile", "Add profile", Some("plus"), &p).on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.show_toast("Add profile", "The profile form comes with the next Phase 4 step.", false, cx))
                    }))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().w(px(320.)).child(Input::new(&self.search).cleanable(true)))
                    .child(segmented),
            );

        let main = div().flex_1().min_w(px(0.)).flex().flex_col().child(header).child(list);

        let mut root = div().size_full().flex().bg(p.bg).text_color(p.text).child(main);
        if let Some(id) = m.selected.clone() {
            root = root.child(self.render_side_panel(&id, &p, cx));
        }
        root
    }
}

impl ProfilesView {
    fn render_section(&self, folder: Option<&Value>, rows: Vec<&Value>, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx).snapshot();
        let (fid, name, color) = match folder {
            Some(f) => (s(f, "id").to_string(), s(f, "name").to_string(), parse_hex(s(f, "color")).unwrap_or(p.text_muted)),
            None => (String::new(), "No folder".to_string(), p.text_faint),
        };
        let collapsed = folder.is_some() && m.collapsed_folders.contains(&fid);
        let count = rows.len();

        let mut header = div()
            .h(px(28.))
            .mt(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .text_size(px(size::TEXT_SM))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.text_muted);
        if folder.is_some() {
            let fid2 = fid.clone();
            let label: SharedString = if collapsed { format!("Expand {name}") } else { format!("Collapse {name}") }.into();
            header = header.child(
                div()
                    .id(SharedString::from(format!("fold-{fid}")))
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_expanded(!collapsed)
                    .focusable()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.toggle_folder(&fid2, cx))))
                    .child(icon(if collapsed { "chevron-right" } else { "chevron-down" }, 14., p.text_faint)),
            );
        }
        header = header
            .child(icon("folder", 14., color))
            .child(div().child(name.clone()))
            .child(div().font_family(FONT_MONO).text_size(px(size::TEXT_XS)).font_weight(FontWeight::NORMAL).text_color(p.text_faint).child(count.to_string()));

        let mut section = div()
            .id(SharedString::from(format!("section-{}", if fid.is_empty() { "none".into() } else { fid.clone() })))
            .role(Role::Group)
            .aria_label(if folder.is_some() { format!("{name} folder") } else { "No folder".into() })
            .child(header);
        if !collapsed {
            for prof in rows {
                section = section.child(self.render_row(prof, p, cx));
            }
        }
        section
    }

    fn render_row(&self, prof: &Value, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx).snapshot();
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let summary = m.summary(&id);
        let state = summary.map(CredState::of).unwrap_or(CredState::Never);
        let remaining = summary.and_then(|d| d.get("timeRemainingSeconds")).and_then(Value::as_i64);
        let selected = m.selected.as_deref() == Some(id.as_str());
        let refreshing = m.refreshing.contains(&id);
        let account = prof.get("ssoAccountId").and_then(Value::as_str).map(str::to_string).or_else(|| {
            prof.get("roleArn").and_then(Value::as_str).and_then(|a| a.split(':').nth(4)).map(str::to_string)
        });

        let count_color = match state {
            CredState::Expiring => p.warning,
            CredState::Expired => p.danger,
            _ => p.text_muted,
        };
        let id_details = id.clone();
        let id_console = id.clone();
        let id_refresh = id.clone();
        let (console_icon, console_label) = if state.needs_sign_in() {
            ("log-in", format!("Sign in to {name}"))
        } else {
            ("app-window", format!("Open AWS console for {name}"))
        };

        div()
            .id(SharedString::from(format!("row-{id}")))
            .role(Role::Group)
            .aria_label(name.clone())
            .flex()
            .items_center()
            .rounded(px(size::RADIUS_LG))
            .when(selected, |d| d.bg(p.selected).border_l_2().border_color(p.accent))
            .hover(|d| d.bg(p.hover))
            .child(
                div()
                    .id(SharedString::from(format!("details-{id}")))
                    .role(Role::Button)
                    .aria_label(format!("Show details for {name}"))
                    .focusable()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(size::ROW))
                    .pl(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = id_details.clone();
                        this.model.update(cx, |m, cx| {
                            m.selected = if m.selected.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                            cx.notify();
                        })
                    }))
                    .child(div().id(SharedString::from(format!("dot-{id}"))).role(Role::Image).aria_label(state.name()).child(status_dot(dot_color(p, state), p.text_faint, 8.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).truncate().child(name.clone()))
                            .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).truncate().child(subtitle(summary))),
                    )
                    .child(div().w(px(120.)).child(mono(account.unwrap_or_default(), p.text_faint)))
                    .child(
                        div()
                            .w(px(110.))
                            .flex()
                            .justify_end()
                            .child(mono(if refreshing { "refreshing…".into() } else { short_label(state, remaining) }, count_color)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .child(
                        icon_button(SharedString::from(format!("console-{id}")), console_icon, console_label, p)
                            .border_1()
                            .border_color(p.border)
                            .bg(p.raised)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = id_console.clone();
                                this.model.update(cx, |m, cx| {
                                    if state.needs_sign_in() {
                                        m.refresh(id, cx)
                                    } else {
                                        m.open_console(id, cx)
                                    }
                                })
                            })),
                    )
                    .child(
                        icon_button(SharedString::from(format!("refresh-{id}")), "refresh-cw", format!("Refresh {name}"), p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let id = id_refresh.clone();
                                this.model.update(cx, |m, cx| m.refresh(id, cx))
                            })),
                    )
                    .child(icon_button(SharedString::from(format!("more-{id}")), "ellipsis", format!("More actions for {name}"), p)),
            )
    }

    fn render_side_panel(&self, id: &str, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx).snapshot();
        let prof = m.profile(id).cloned().unwrap_or(Value::Null);
        let summary = m.summary(id).cloned();
        let state = summary.as_ref().map(CredState::of).unwrap_or(CredState::Never);
        let remaining = summary.as_ref().and_then(|d| d.get("timeRemainingSeconds")).and_then(Value::as_i64);
        let name = s(&prof, "name").to_string();
        let section = {
            let c = s(&prof, "credentialProfileName");
            if c.is_empty() { name.clone() } else { c.to_string() }
        };
        let big = match state {
            CredState::Expired => "Expired".to_string(),
            CredState::Never => "Not signed in".to_string(),
            _ => remaining.map(|r| format!("{} left", format_countdown(r))).unwrap_or_else(|| "Active".into()),
        };
        let sub = summary
            .as_ref()
            .and_then(|d| d.get("expiresAtPst"))
            .and_then(Value::as_str)
            .map(|e| format!("{} {}", if state == CredState::Expired { "at" } else { "until" }, format_expiry(e)))
            .unwrap_or_else(|| "Sign in to get credentials".into());
        let big_color = match state {
            CredState::Expiring => p.warning,
            CredState::Expired => p.danger,
            _ => p.text,
        };
        let method = if s(&prof, "authType") == "identityCenter" { "IAM Identity Center" } else { "SAML / ADFS" };
        let role = prof
            .get("roleArn")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| prof.get("ssoRoleName").and_then(Value::as_str).map(|r| format!("{r} · {}", s(&prof, "ssoRegion"))))
            .unwrap_or_else(|| "—".into());
        let auto = if prof.get("autoRefresh") == Some(&Value::Bool(true)) {
            let mins = prof.get("refreshIntervalMinutes").and_then(Value::as_i64).unwrap_or(60);
            if mins % 60 == 0 { format!("Every {} h", mins / 60) } else { format!("Every {mins} min") }
        } else {
            "Off".into()
        };

        let snippet = |label: &'static str, code: String, i: usize| {
            div()
                .h(px(40.))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(12.))
                .pr(px(6.))
                .bg(p.bg)
                .when(i > 0, |d| d.border_t_1().border_color(p.border))
                .child(div().flex_1().min_w(px(0.)).truncate().font_family(FONT_MONO).text_size(px(size::TEXT_XS)).child(code.clone()))
                .child(icon_button(SharedString::from(format!("copy-{label}")), "copy", format!("Copy {label} command"), p).on_click(
                    move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(code.clone())),
                ))
        };
        let fact = |k: &'static str, v: String, mono_value: bool| {
            div()
                .flex()
                .gap(px(8.))
                .text_size(px(size::TEXT_SM))
                .child(div().w(px(92.)).flex_shrink_0().text_color(p.text_muted).child(k))
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .when(mono_value, |d| d.font_family(FONT_MONO).text_size(px(size::TEXT_XS)))
                        .child(v),
                )
        };

        div()
            .id("details-panel")
            .role(Role::Region)
            .aria_label("Profile details")
            .w(px(size::SIDE_PANEL))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.border)
            .bg(p.panel)
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(s(&prof, "label").to_string())),
                    )
                    .child(icon_button("close-details", "x", "Close details", p).on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.selected = None;
                            cx.notify();
                        })
                    }))),
            )
            .child(
                div()
                    .p(px(16.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.bg)
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(status_dot(dot_color(p, state), p.text_faint, 10.))
                            .child(div().font_family(FONT_MONO).text_size(px(24.)).font_weight(FontWeight::MEDIUM).text_color(big_color).child(big)),
                    )
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(sub)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(secondary_button("panel-refresh", "Refresh", Some("refresh-cw"), p).on_click(cx.listener({
                        let id = id.to_string();
                        move |this, _, _, cx| this.model.update(cx, |m, cx| m.refresh(id.clone(), cx))
                    })))
                    .when(!state.needs_sign_in(), |d| {
                        d.child(primary_button("panel-console", "Open console", Some("app-window"), p).on_click(cx.listener({
                            let id = id.to_string();
                            move |this, _, _, cx| this.model.update(cx, |m, cx| m.open_console(id.clone(), cx))
                        })))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::SEMIBOLD).child("Use in a shell"))
                    .child(
                        div()
                            .rounded(px(size::RADIUS_LG))
                            .border_1()
                            .border_color(p.border)
                            .overflow_hidden()
                            .child(snippet("PowerShell", format!("$env:AWS_PROFILE = \"{section}\""), 0))
                            .child(snippet("bash", format!("export AWS_PROFILE={section}"), 1))
                            .child(snippet("CLI flag", format!("--profile {section}"), 2)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::SEMIBOLD).child("Sign-in"))
                    .child(fact("Method", method.to_string(), false))
                    .child(fact("Section", section.clone(), true))
                    .child(fact("Role", role, true))
                    .child(fact("Auto refresh", auto, false)),
            )
    }
}
