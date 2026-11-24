//! The Profiles screen (mockup "direction B"): every profile visible with one-click Console /
//! Refresh, folders as sections, and an optional side panel with the session, shell snippets,
//! sign-in details and recent activity. The `profileListLayout` setting "cards" swaps the rows for
//! the 1.x card grid (Profiles.tsx ProfileCardBody, grid view), with 1.x's per-card buttons.
//! Profiles and folders reorder by drag and drop, with the mouse or the keyboard (`dnd.rs`).

use std::collections::HashMap;

use gpui::{
    div, prelude::*, px, Context, DismissEvent, Entity, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, Role, SharedString, Subscription, Window,
};
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::Value;

use gpui_component::button::{Button, ButtonVariants};
use gpui_component::popover::Popover;
use gpui_component::Icon;

use crate::a11y::{A11yDisabledExt as _, PressExt as _};
use crate::dialogs::{ConfirmAction, DialogKind};
use crate::dnd::{self, DragItem, DragPreview, DraggedFolder, DraggedProfile, KbDrag, Place, Section};
use crate::form::FormView;
use crate::menu::{Entry, MenuView, Swatch};
use crate::model::{AppModel, Filter, Snap};
use crate::ring::expiry_ring;
use crate::status::{format_countdown, format_expiry, session_fraction, short_label, CredState};
use crate::theme::{size, Palette, FONT_MONO};
use crate::widgets::{
    avatar_of, chip, icon, icon_button, primary_button, profile_avatar, read_text, refresh_button, refresh_secondary_button, secondary_button, status_dot,
    FocusRing as _,
};

pub struct ProfilesView {
    model: Entity<AppModel>,
    form: Option<(u64, Entity<FormView>)>,
    /// The open row menu: profile id, the menu, its dismiss subscription.
    menu: Option<(String, Entity<MenuView>, Subscription)>,
    /// Inline folder rename: folder id, its name field, the field's subscription.
    renaming: Option<(String, Entity<InputState>, Subscription)>,
    search: Entity<InputState>,
    /// The feature tour opened the add menu to point at "Add accounts…"; it closes it on the way out.
    tour_opened_menu: bool,
    /// A keyboard drag in progress (focus a "Reorder …" handle, Space, arrows, Space).
    kb_drag: Option<KbDrag>,
    /// Focus handles of the drag handles ("p:<id>" / "f:<id>"), kept here so focus stays on the
    /// handle while its row moves between sections.
    handles: HashMap<String, FocusHandle>,
    /// The last drag announcement (1.x dnd-kit live region).
    announce: Option<SharedString>,
    /// The width the profile list had at the last render, for the row columns that fit it.
    list_width: f32,
    _subs: Vec<Subscription>,
}

/// `menu` key of the "More ways to add profiles" menu.
const ADD_MENU: &str = "add";

impl ProfilesView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subs = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            // Observed, not subscribed to Change: an accessible SetValue edits without that event.
            cx.observe(&search, |this, input, cx| {
                let q = input.read(cx).value().to_string();
                if this.model.read(cx).search != q {
                    this.model.update(cx, |m, cx| {
                        m.search = q;
                        cx.notify();
                    });
                }
            }),
        ];
        ProfilesView {
            model,
            form: None,
            menu: None,
            renaming: None,
            search,
            tour_opened_menu: false,
            kb_drag: None,
            handles: HashMap::new(),
            announce: None,
            list_width: 1000.,
            _subs: subs,
        }
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The list as shown: each folder with its profiles that pass the search box and the filter, then
/// the profiles with no (existing) folder. Every folder and the no-folder section are included;
/// the render leaves out the empty ones it does not show.
fn arrange(m: &crate::model::Snap) -> Vec<Section> {
    let query = m.search.to_lowercase();
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
            let account = m.summary(id).map(|d| s(d, "accountNumber")).unwrap_or("");
            let hay =
                format!("{} {} {} {} {}", s(prof, "name"), s(prof, "label"), account, s(prof, "ssoRoleName"), s(prof, "credentialProfileName")).to_lowercase();
            filter_ok && (query.is_empty() || hay.contains(&query))
        })
        .collect();
    let mut sections: Vec<Section> = m
        .folders
        .iter()
        .map(|f| Section { folder: Some(f.clone()), rows: visible.iter().filter(|prof| s(prof, "folderId") == s(f, "id")).map(|v| (*v).clone()).collect() })
        .collect();
    let ungrouped = visible.iter().filter(|prof| !m.folders.iter().any(|f| s(f, "id") == s(prof, "folderId"))).map(|v| (*v).clone()).collect();
    sections.push(Section { folder: None, rows: ungrouped });
    sections
}

/// What a compact row says under the name.
struct Facts {
    /// The account's display name (Settings > account names), when one is set.
    account_name: Option<String>,
    account_id: Option<String>,
    role: Option<String>,
    method: &'static str,
    /// The profile's description, for profiles with no account to describe.
    label: Option<String>,
    /// The credentials file section (`--profile`).
    section: String,
}

impl Facts {
    fn of(prof: &Value, settings: &Value) -> Self {
        let account_id = crate::profile_icons::account_of(prof);
        let account_name = account_id.as_ref().and_then(|a| {
            settings.get("accountDisplayNames").and_then(|n| n.get(a)).and_then(Value::as_str).map(str::trim).filter(|f| !f.is_empty()).map(str::to_string)
        });
        let nonempty = |k: &str| prof.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let role = nonempty("ssoRoleName").or_else(|| nonempty("roleArn").and_then(|a| a.rsplit('/').next().map(str::to_string)).filter(|r| !r.contains(':')));
        let method = if s(prof, "authType") == "identityCenter" { "Identity Center" } else { "SAML" };
        let section = nonempty("credentialProfileName").unwrap_or_else(|| s(prof, "name").to_string());
        Facts { account_name, account_id, role, method, label: nonempty("label"), section }
    }

    /// One line that reads as a sentence, as 1.x's list did: "platform-prod (412398765432) ·
    /// PlatformAdmin". 1.x showed the same facts this way; spread over unlabelled columns they
    /// were harder to take in. Without an account: the description, then the `--profile` name.
    fn detail(&self) -> String {
        let account = match (&self.account_name, &self.account_id) {
            (Some(n), Some(id)) => Some(format!("{n} ({id})")),
            (None, Some(id)) => Some(id.clone()),
            _ => None,
        };
        match (account, &self.role) {
            (Some(a), Some(r)) => format!("{a} · {r}"),
            (Some(a), None) => a,
            (None, Some(r)) => r.clone(),
            (None, None) => self.label.clone().unwrap_or_else(|| self.section.clone()),
        }
    }
}

/// Below this list width the Identity Center tag drops out, leaving the detail line its room.
const METHOD_TAG_MIN_WIDTH: f32 = 640.;
/// A compact row: room for a 15 px name over a 13 px detail line.
const ROW_HEIGHT: f32 = 60.;

/// The small sign-in method tag on a row ("SAML" / "Identity Center").
fn method_tag(id: SharedString, method: &'static str, p: &Palette) -> impl IntoElement {
    let sso = method != "SAML";
    read_text(id, method)
        .h(px(20.))
        .px(px(7.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(if sso { gpui::Rgba { a: 0.35, ..p.accent } } else { p.border })
        .bg(if sso { gpui::Rgba { a: 0.08, ..p.accent } } else { p.raised })
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(if sso { p.accent } else { p.text_muted })
        .whitespace_nowrap()
}

fn state_color(p: &Palette, state: CredState) -> gpui::Rgba {
    match state {
        CredState::Active => p.success,
        CredState::Expiring => p.warning,
        CredState::Expired => p.danger,
        CredState::Never => p.text_faint,
    }
}

/// The session summary over the list: every live session as a bar from now to its expiry on one
/// timeline (the bars overlap, so the strip is densest where most sessions are alive), a tick at
/// each expiry naming its profile on hover, and "Next expiry Dev in 14m / 3 active" beside it.
/// Nothing at all when no session is live.
fn session_strip(m: &crate::model::Snap, p: &Palette) -> Option<gpui::AnyElement> {
    let mut live: Vec<(String, i64, CredState)> = m
        .dashboard
        .iter()
        .filter(|d| s(d, "status") == "active")
        .filter_map(|d| Some((s(d, "name").to_string(), d.get("timeRemainingSeconds")?.as_i64()?, CredState::of(d))))
        .collect();
    if live.is_empty() {
        return None;
    }
    live.sort_by_key(|(_, t, _)| *t);
    let (next_name, next_t, next_state) = live[0].clone();
    let expiring = live.iter().filter(|(_, _, st)| *st == CredState::Expiring).count();
    // The window: whole hours up to the longest session, at most 12.
    let hours = ((live.last().map(|l| l.1).unwrap_or(0) + 3599) / 3600).clamp(1, 12);
    let span = (hours * 3600) as f32;
    let step = match hours {
        0..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    let at = |t: i64| (t as f32 / span).clamp(0., 1.);

    let n = live.len() as f32;
    // Each bar's opacity, so where all of them overlap the strip reaches 0.8 and each expiry
    // steps it down.
    let alpha = (1. - 0.2f32.powf(1. / n)).max(0.12);
    let mut track = div().relative().h(px(8.)).rounded(px(4.)).bg(p.track()).overflow_hidden();
    for (_, t, st) in &live {
        track =
            track.child(div().absolute().left(px(0.)).top(px(0.)).bottom(px(0.)).w(gpui::relative(at(*t))).bg(gpui::Rgba { a: alpha, ..state_color(p, *st) }));
    }
    let mut chart = div().relative().h(px(24.)).flex().flex_col().justify_center().child(track);
    for (i, (name, t, st)) in live.iter().enumerate() {
        let tip: SharedString = format!("{name}: {} left", format_countdown(*t)).into();
        chart = chart.child(
            div()
                .id(SharedString::from(format!("strip-tick-{i}")))
                .absolute()
                .top(px(0.))
                .bottom(px(0.))
                .left(gpui::relative(at(*t)))
                .ml(px(-5.))
                .w(px(10.))
                .flex()
                .justify_center()
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .child(div().mt(px(3.)).w(px(2.)).h(px(18.)).rounded(px(1.)).bg(state_color(p, *st))),
        );
    }
    let mut axis = div().relative().h(px(14.));
    for h in (0..=hours).step_by(step as usize) {
        let label = if h == 0 { "now".to_string() } else { format!("{h}h") };
        let x = h as f32 / hours as f32;
        let tick = div().absolute().top(px(0.)).text_size(px(11.)).text_color(p.text_faint).child(label);
        axis = axis.child(if h == 0 {
            tick.left(px(0.))
        } else if h == hours {
            tick.right(px(0.))
        } else {
            tick.left(gpui::relative(x)).ml(px(-8.))
        });
    }

    let mut counts = if live.len() == 1 { "1 active".to_string() } else { format!("{} active", live.len()) };
    if expiring > 0 {
        counts.push_str(&format!(" · {expiring} expiring soon"));
    }
    let summary = format!("Next expiry: {next_name} in {}. {counts}.", format_countdown(next_t));
    let detail = live.iter().map(|(name, t, _)| format!("{name} in {}", format_countdown(*t))).collect::<Vec<_>>().join(", ");
    let next_color = if next_state == CredState::Expiring { p.warning } else { p.text };

    Some(
        div()
            .px(px(24.))
            .pb(px(12.))
            .child(
                div()
                    .id("session-strip")
                    .relative()
                    .role(Role::Group)
                    .aria_label("Sessions")
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px(px(16.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.panel)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.))
                            .text_size(px(size::TEXT_SM))
                            .child(div().flex_shrink_0().text_color(p.text_muted).child("Next expiry"))
                            .child(div().min_w(px(0.)).font_weight(FontWeight::MEDIUM).text_color(next_color).truncate().child(next_name.clone()))
                            .child(div().flex_shrink_0().text_color(next_color).child(format!("in {}", format_countdown(next_t))))
                            .child(div().flex_1())
                            .child(div().flex_shrink_0().text_size(px(size::TEXT_XS)).text_color(p.text_muted).child(counts)),
                    )
                    .child(div().flex().flex_col().gap(px(2.)).child(chart).child(axis))
                    // For screen readers: the summary, then each session (the strip itself is a
                    // picture of the same thing).
                    .child(read_text("session-summary", summary).absolute().size(px(1.)).overflow_hidden().opacity(0.))
                    .child(read_text("session-list", format!("Sessions by expiry: {detail}.")).absolute().size(px(1.)).overflow_hidden().opacity(0.)),
            )
            .into_any_element(),
    )
}

/// 1.x PRESET_ICON_COLORS, the folder and profile icon palette.
const FOLDER_COLORS: [&str; 20] = crate::profile_icons::PRESET_COLORS;

fn parse_hex(color: &str) -> Option<gpui::Rgba> {
    let h = color.strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten().map(gpui::rgb)
}

/// 1.x delete confirmation for a profile.
fn confirm_delete(m: &mut AppModel, id: String, name: &str, cx: &mut Context<AppModel>) {
    m.open_dialog(
        DialogKind::Confirm {
            title: "Delete profile".into(),
            body: format!("Delete \"{name}\"? This will also remove its credentials from the credentials file."),
            confirm: "Delete".into(),
            action: ConfirmAction::DeleteProfile { id },
        },
        cx,
    )
}

impl Render for ProfilesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (form, seq) = {
            let m = self.model.read(cx);
            (m.form.clone(), m.form_seq)
        };
        match form {
            Some(req) => {
                if self.form.as_ref().map(|(s, _)| *s) != Some(seq) {
                    let model = self.model.clone();
                    let view = cx.new(|cx| FormView::new(model, req.profile, req.is_new, req.names, req.sso_defaults, window, cx));
                    self.form = Some((seq, view));
                }
                return div().size_full().child(self.form.as_ref().unwrap().1.clone()).into_any_element();
            }
            None => self.form = None,
        }
        // The tour's "Import accounts in bulk" step opens the add menu (1.x openAddMenu / closeAddMenu).
        let tour_step = self.model.read(cx).tour.as_ref().map(|t| crate::onboarding::TOURS[t.tour].steps[t.steps[t.index]].id);
        let add_open = self.menu.as_ref().is_some_and(|(k, _, _)| k == ADD_MENU);
        if tour_step == Some("bulk-import") {
            if !add_open {
                self.toggle_add_menu(false, window, cx);
            }
        } else if self.tour_opened_menu {
            self.tour_opened_menu = false;
            if add_open {
                self.menu = None;
            }
        }
        let m = self.model.read(cx).snapshot();
        let p = m.palette;
        let query = m.search.to_lowercase();
        let cards = self.model.read(cx).cards_layout();
        // The width the list gets: card columns of at least 296 px fill it (1.x had 1 to 3), and
        // the compact rows show the middle columns that fit.
        let cols = {
            let sidebar = if self.model.read(cx).sidebar_collapsed { size::RAIL } else { size::SIDEBAR };
            let avail = f32::from(window.viewport_size().width) - sidebar - 32.;
            let panel = if m.selected.is_some() && !cards { size::SIDE_PANEL } else { 0. };
            self.list_width = avail - panel;
            ((avail - 8. + 12.) / (296. + 12.)).floor().clamp(1., 6.) as u16
        };

        // A focus handle per drag handle, made before the rows borrow the view.
        for key in m.profiles.iter().map(|v| format!("p:{}", s(v, "id"))).chain(m.folders.iter().map(|v| format!("f:{}", s(v, "id")))) {
            self.handles.entry(key).or_insert_with(|| cx.focus_handle().tab_stop(true));
        }

        let total = m.profiles.len();
        // Sessions and the next expiry are in the strip under the header.
        let counts = if total == 1 { "1 profile".to_string() } else { format!("{total} profiles") };

        // Focus left the handle (Tab, a click elsewhere): the keyboard drag is cancelled.
        if let Some(k) = &self.kb_drag {
            let key = match &k.item {
                DragItem::Profile(id) => format!("p:{id}"),
                DragItem::Folder(id) => format!("f:{id}"),
            };
            if !self.handles.get(&key).is_some_and(|h| h.is_focused(window)) {
                let name = self.item_name(&k.item.clone(), cx);
                self.kb_drag = None;
                self.announce = Some(format!("Cancelled. {name} was returned to where it was.").into());
            }
        }
        // Folder sections, then the profiles with no (existing) folder; a keyboard drag shows the
        // move it would make.
        let mut sections = arrange(&m);
        if let Some(k) = &self.kb_drag {
            match &k.item {
                DragItem::Profile(id) => dnd::preview_profile(&mut sections, id, &k.slots[k.slot]),
                DragItem::Folder(id) => dnd::preview_folder(&mut sections, id, k.slot),
            }
        }
        let searching = !query.is_empty() || m.filter != Filter::All;
        // While dragging, every folder and the no-folder section are drop targets.
        let dragging = self.kb_drag.is_some() || cx.has_active_drag();

        let mut list = div().id("profile-list").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(16.)).pb(px(16.)).flex().flex_col().gap(px(10.));
        for (i, sec) in sections.iter().enumerate() {
            let rows: Vec<&Value> = sec.rows.iter().collect();
            if sec.folder.is_none() && rows.is_empty() && !(dragging && total > 0) {
                continue;
            }
            if sec.folder.is_some() && rows.is_empty() && searching && !dragging {
                continue;
            }
            list = list.child(self.render_section(sec.folder.as_ref(), i, rows, searching, cards.then_some(cols), &m, &p, cx));
        }
        if m.loaded && total == 0 {
            list = list.child(
                div()
                    .mt(px(48.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .mb(px(4.))
                            .size(px(40.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.panel)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("users", 20., p.text_muted)),
                    )
                    .child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).child("No profiles yet"))
                    .child(div().text_color(p.text_muted).child("A profile signs in to one AWS role and keeps its credentials file entry fresh"))
                    .child(
                        div().mt(px(16.)).child(
                            secondary_button("add-first-profile", "Add your first profile", Some("plus"), &p)
                                .on_press(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.edit_profile(None, cx)))),
                        ),
                    ),
            );
        }

        let filters = [(Filter::All, "All"), (Filter::Expiring, "Expiring"), (Filter::NeedsSignIn, "Needs sign-in")];
        let mut segmented =
            div().id("filter").role(Role::Group).aria_label("Show").flex().p(px(2.)).rounded(px(size::RADIUS)).border_1().border_color(p.border).bg(p.track());
        for (f, label) in filters {
            let on = m.filter == f;
            segmented = segmented.child(
                div()
                    .id(label)
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                    .focusable()
                    .tab_stop(true)
                    .focus_ring(&p)
                    .h(px(26.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .text_size(px(size::TEXT_SM))
                    .cursor_pointer()
                    .when(on, |d| chip(d, true, &p).text_color(p.text))
                    .when(!on, |d| d.text_color(p.text_muted).hover(|d| d.text_color(p.text)))
                    .on_press(cx.listener(move |this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.filter = f;
                            cx.notify();
                        })
                    }))
                    .child(label),
            );
        }

        // 1.x view toggle, kept in step with Settings > Appearance > Profile list.
        let view_button = |id: &'static str, label: &'static str, icon_name: &'static str, on: bool, value: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
                .focusable()
                .tab_stop(true)
                .focus_ring(&p)
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| chip(d, true, &p))
                .when(!on, |d| d.hover(|d| d.bg(p.hover)))
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
                .on_press(cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.set_setting("profileListLayout", serde_json::json!(value), cx))))
                .child(icon(icon_name, 16., if on { p.text } else { p.text_muted }))
        };
        let view_toggle = div()
            .id("view-toggle")
            .role(Role::Group)
            .aria_label("View")
            .flex()
            .p(px(2.))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(p.border)
            .bg(p.track())
            .child(view_button("view-list", "List view", "list", !cards, "compact", cx))
            .child(view_button("view-grid", "Grid view", "layout-grid", cards, "cards", cx));

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
                    .child(
                        div()
                            .id("profiles-heading")
                            .role(Role::Heading)
                            .aria_label("Profiles")
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Profiles"),
                    )
                    .child(div().text_size(px(size::TEXT_SM)).text_color(p.text_muted).child(counts))
                    .child(div().flex_1())
                    .child(icon_button("new-folder", "folder-plus", "New folder", &p).on_press(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.run("folders:save", vec![serde_json::json!({ "name": "New folder" })], cx))
                    })))
                    // 1.x split button: adding one profile stays a single click; bulk import is a
                    // once-at-setup action, so it lives in the menu.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                primary_button("add-profile", "Add profile", Some("plus"), &p)
                                    .rounded_r(px(0.))
                                    .on_press(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.edit_profile(None, cx)))),
                            )
                            .child(div().w(px(1.)).h(px(size::BUTTON)).bg(p.primary).child(div().size_full().bg(gpui::Rgba { a: 0.15, ..p.on_primary })))
                            .child(self.add_menu(&p, cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().w(px(320.)).child(Input::new(&self.search).cleanable(true).role(Role::SearchInput).aria_label("Search profiles")))
                    .child(segmented)
                    .child(div().flex_1())
                    .child(view_toggle),
            );

        // 1.x: a notice while auto refresh is paused.
        let (paused, due_to_failures) = self.model.read(cx).refresh_paused;
        let pause_notice = paused.then(|| {
            let text = if due_to_failures {
                "Auto refresh is paused after repeated sign-in failures. Check your IdP credentials, then resume it in Settings."
            } else {
                "Auto refresh is paused. Resume it in Settings."
            };
            div().px(px(24.)).pb(px(12.)).child(
                div()
                    .id("pause-notice")
                    .role(Role::Status)
                    .aria_label(text)
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(size::RADIUS_LG))
                    .border_1()
                    .border_color(p.warning)
                    .bg(p.panel)
                    .child(div().mt(px(5.)).child(status_dot(Some(p.warning), p.warning, 8.)))
                    .child(div().text_size(px(size::TEXT_SM)).child(text)),
            )
        });
        let strip = if total > 0 { session_strip(&m, &p) } else { None };
        let main = div().flex_1().min_w(px(0.)).flex().flex_col().child(header).children(pause_notice).children(strip).child(list);

        let mut root = div().size_full().relative().flex().bg(p.bg).text_color(p.text).child(main);
        if let Some(id) = m.selected.clone().filter(|_| !cards) {
            root = root.child(self.render_side_panel(&id, &m, &p, cx));
        }
        // Drag announcements for screen readers (1.x dnd-kit's live region), out of sight.
        if let Some(text) = self.announce.clone() {
            root = root.child(
                div()
                    .id("drag-announcement")
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .size(px(1.))
                    .overflow_hidden()
                    .opacity(0.)
                    .child(text)
                    .a11y_live(),
            );
        }
        root.into_any_element()
    }
}

impl ProfilesView {
    /// A folder section (or "No folder"); `cards` holds the column count in the cards layout.
    /// `index` is the folder's position, for dropping a dragged folder here.
    #[allow(clippy::too_many_arguments)]
    fn render_section(
        &self,
        folder: Option<&Value>,
        index: usize,
        rows: Vec<&Value>,
        searching: bool,
        cards: Option<u16>,
        m: &Snap,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (fid, name, color) = match folder {
            Some(f) => (s(f, "id").to_string(), s(f, "name").to_string(), parse_hex(s(f, "color")).unwrap_or(p.text_faint)),
            None => (String::new(), "No folder".to_string(), p.text_faint),
        };
        let collapsed = folder.is_some() && m.collapsed_folders.contains(&fid);
        let count = rows.len();
        let kb_item = self.kb_drag.as_ref().map(|k| k.item.clone());
        let kb_folder = kb_item.as_ref() == Some(&DragItem::Folder(fid.clone())) && folder.is_some();

        let mut header =
            div().h(px(32.)).flex().items_center().gap(px(8.)).px(px(8.)).text_size(px(size::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(p.text_muted);
        if folder.is_some() {
            let drag = DraggedFolder { id: fid.clone(), name: name.clone(), palette: *p };
            header =
                header.ml(px(-4.)).child(
                    self.drag_handle(format!("f:{fid}"), DragItem::Folder(fid.clone()), format!("Reorder {name} folder"), "Drag to reorder folder", p, cx)
                        .on_drag(drag, {
                            let view = cx.entity().downgrade();
                            move |d: &DraggedFolder, _, _, cx| {
                                let _ = view.update(cx, |this, cx| this.mouse_drag_started(cx));
                                cx.new(|_| DragPreview::new(d.name.clone(), "folder", d.palette))
                            }
                        }),
                );
            let fid2 = fid.clone();
            let label: SharedString = if collapsed { format!("Expand {name}") } else { format!("Collapse {name}") }.into();
            header = header.child(
                div()
                    .id(SharedString::from(format!("fold-{fid}")))
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_expanded(!collapsed)
                    .focusable()
                    .tab_stop(true)
                    .focus_ring(p)
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .hover(|d| d.bg(p.hover))
                    .cursor_pointer()
                    .on_press(cx.listener(move |this, _, _, cx| this.model.update(cx, |m, cx| m.toggle_folder(&fid2, cx))))
                    .on_a11y_action(gpui::AccessibleAction::Expand, {
                        let (model, fid) = (self.model.clone(), fid.clone());
                        move |_, _, cx| {
                            model.update(cx, |m, cx| {
                                if m.collapsed_folders.contains(&fid) {
                                    m.toggle_folder(&fid, cx)
                                }
                            })
                        }
                    })
                    .on_a11y_action(gpui::AccessibleAction::Collapse, {
                        let (model, fid) = (self.model.clone(), fid.clone());
                        move |_, _, cx| {
                            model.update(cx, |m, cx| {
                                if !m.collapsed_folders.contains(&fid) {
                                    m.toggle_folder(&fid, cx)
                                }
                            })
                        }
                    })
                    .child(crate::motion::chevron(icon("chevron-right", 14., color), SharedString::from(format!("fold-chevron-{fid}")), !collapsed)),
            );
        }
        if folder.is_none() {
            // Lines the folder icon up with the folders' (no grip or chevron here).
            header = header.ml(px(-4.)).child(div().w(px(46.)).flex_shrink_0());
        }
        header = header.child(icon("folder", 14., color));
        let renaming = self.renaming.as_ref().filter(|(rid, _, _)| folder.is_some() && *rid == fid).map(|(_, input, _)| input.clone());
        header = match renaming {
            Some(input) => header.child(div().w(px(240.)).child(Input::new(&input).aria_label("Folder name"))),
            None => header.child(div().child(name.clone())),
        };
        header = header.child(div().text_size(px(size::TEXT_XS)).font_weight(FontWeight::NORMAL).text_color(p.text_faint).child(count.to_string()));
        if let Some(f) = folder {
            header = header.child(div().flex_1()).child(self.folder_menu(f, rows.len(), cx));
        }

        // Drop targets: a profile dropped on the section (its header, a collapsed or empty folder)
        // goes to its end; a folder dropped on a folder takes its place (1.x containerFolderIdOf).
        let tint = gpui::Rgba { a: 0.06, ..p.accent };
        let ring = gpui::Rgba { a: 0.6, ..p.accent };
        let target = (!fid.is_empty()).then(|| fid.clone());
        let mut section = div()
            .id(SharedString::from(format!("section-{}", if fid.is_empty() { "none".into() } else { fid.clone() })))
            .role(Role::Group)
            .aria_label(if folder.is_some() { format!("{name} folder") } else { "No folder".into() })
            // Each folder is a quiet container, as in 1.x, so where one group ends is plain
            // without boxing every row.
            .rounded(px(size::RADIUS_LG))
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .px(px(6.))
            .pt(px(4.))
            .pb(px(6.))
            .when(kb_folder, |d| d.bg(tint).border_color(ring))
            .drag_over::<DraggedProfile>(move |st, _, _, _| st.bg(tint).border_color(ring))
            .on_drop(cx.listener(move |this, d: &DraggedProfile, _, cx| this.commit_profile(&d.id, target.clone(), Place::End, cx)))
            .child(header);
        if folder.is_some() {
            section = section
                .drag_over::<DraggedFolder>(move |st, _, _, _| st.bg(tint).border_color(ring))
                .on_drop(cx.listener(move |this, d: &DraggedFolder, _, cx| this.commit_folder(&d.id, index, cx)));
        }
        // A collapsed folder still shows the profile a keyboard drag is moving into it, so the
        // focused handle stays on screen.
        let rows: Vec<&Value> =
            if collapsed { rows.into_iter().filter(|r| kb_item.as_ref() == Some(&DragItem::Profile(s(r, "id").to_string()))).collect() } else { rows };
        if folder.is_some() && !collapsed && rows.is_empty() {
            let text = if searching { "No matches. Drop a profile here to move it in" } else { "Empty. Drag profiles here" };
            section = section.child(div().pl(px(52.)).pb(px(8.)).pt(px(2.)).text_size(px(size::TEXT_SM)).text_color(p.text_faint).child(text));
        }
        match cards {
            Some(cols) if !rows.is_empty() => {
                let mut grid = div().grid().grid_cols(cols).gap(px(12.)).px(px(4.)).pt(px(6.)).pb(px(6.));
                for prof in rows {
                    grid = grid.child(self.render_card(prof, m, p, cx));
                }
                section = section.child(grid);
            }
            _ => {
                for (i, prof) in rows.into_iter().enumerate() {
                    if i > 0 {
                        // A hairline between rows, inset to start under the name.
                        section = section.child(div().h(px(1.)).ml(px(72.)).mr(px(8.)).bg(p.border));
                    }
                    section = section.child(self.render_row(prof, m, p, cx));
                }
            }
        }
        section
    }

    fn render_row(&self, prof: &Value, m: &Snap, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let summary = m.summary(&id);
        let state = summary.map(CredState::of).unwrap_or(CredState::Never);
        let remaining = summary.and_then(|d| d.get("timeRemainingSeconds")).and_then(Value::as_i64);
        let selected = m.selected.as_deref() == Some(id.as_str());
        let refreshing = m.refreshing.contains(&id);
        let facts = Facts::of(prof, &self.model.read(cx).settings);
        // SAML is the usual case, so only Identity Center is tagged.
        let show_method = facts.method != "SAML" && self.list_width >= METHOD_TAG_MIN_WIDTH;

        let count_color = match state {
            CredState::Expiring => p.warning,
            CredState::Expired => p.danger,
            _ => p.text_muted,
        };
        let id_details = id.clone();
        let id_console = id.clone();
        let id_refresh = id.clone();
        // Always the console: when the profile isn't signed in, opening it signs in first.
        let (console_icon, console_label) = ("app-window", format!("Open AWS console for {name}"));

        let grip = self.profile_handle(prof, p, cx);
        self.drop_target(
            div()
                .id(SharedString::from(format!("row-{id}")))
                .role(Role::Group)
                .aria_label(name.clone())
                .relative()
                .flex()
                .items_center()
                .rounded(px(size::RADIUS_LG))
                .when(selected, |d| d.bg(p.selected))
                .hover(|d| d.bg(p.hover)),
            prof,
            p,
            cx,
        )
        // The selection marker: a short inset bar, so it sits cleanly inside the rounded row.
        .when(selected, |d| d.child(div().absolute().left(px(0.)).top(px(14.)).bottom(px(14.)).w(px(3.)).rounded(px(2.)).bg(p.accent)))
        .pl(px(4.))
        .child(grip)
        .child(
            div()
                .id(SharedString::from(format!("details-{id}")))
                .role(Role::Button)
                .aria_label(format!("Show details for {name}"))
                .focusable()
                .tab_stop(true)
                .focus_ring(p)
                .rounded(px(size::RADIUS))
                .flex_1()
                .min_w(px(0.))
                .h(px(ROW_HEIGHT))
                .pl(px(4.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .on_press(cx.listener(move |this, _, _, cx| {
                    let id = id_details.clone();
                    this.model.update(cx, |m, cx| {
                        m.selected = if m.selected.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                        cx.notify();
                    })
                }))
                .child(profile_avatar(&avatar_of(prof), 36., 18., p))
                // Two levels in one typeface, as 1.x: the name, then what it signs in to.
                .child(
                    div()
                        .flex_1()
                        .min_w(px(120.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone()))
                        .child(
                            read_text(SharedString::from(format!("subtitle-{id}")), facts.detail())
                                .text_size(px(size::TEXT_SM))
                                .text_color(p.text_muted)
                                .truncate(),
                        ),
                )
                .when(show_method, |d| d.child(div().flex_shrink_0().flex().child(method_tag(SharedString::from(format!("method-{id}")), facts.method, p))))
                .child(
                    div()
                        .w(px(140.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().id(SharedString::from(format!("dot-{id}"))).role(Role::Image).aria_label(state.name()).child(expiry_ring(
                            "ring-spin",
                            state,
                            session_fraction(Some(prof), remaining),
                            refreshing,
                            16.,
                            p,
                        )))
                        .child(
                            read_text(
                                SharedString::from(format!("status-{id}")),
                                if refreshing { "Refreshing…".to_string() } else { short_label(state, remaining) },
                            )
                            .text_size(px(size::TEXT_SM))
                            .when(state != CredState::Never, |d| d.font_weight(FontWeight::MEDIUM))
                            .text_color(count_color),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .pl(px(4.))
                .pr(px(8.))
                .child(
                    icon_button(SharedString::from(format!("console-{id}")), console_icon, console_label, p)
                        .border_1()
                        .border_color(p.border)
                        .bg(p.raised)
                        .on_press(cx.listener(move |this, _, _, cx| {
                            let id = id_console.clone();
                            this.model.update(cx, |m, cx| m.open_console(id, cx))
                        })),
                )
                .child(refresh_button(SharedString::from(format!("refresh-{id}")), format!("Refresh {name}"), refreshing, p).on_press(cx.listener(
                    move |this, _, _, cx| {
                        let id = id_refresh.clone();
                        this.model.update(cx, |m, cx| m.refresh(id, cx))
                    },
                )))
                .child(self.row_menu(prof, cx)),
        )
    }

    /// One card of the cards layout (1.x ProfileCardBody, grid view): avatar, name and account,
    /// the credential state with its expiry, then Console, Refresh, Move to folder, Edit, Delete.
    fn render_card(&self, prof: &Value, m: &Snap, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let summary = m.summary(&id);
        let state = summary.map(CredState::of).unwrap_or(CredState::Never);
        let remaining = summary.and_then(|d| d.get("timeRemainingSeconds")).and_then(Value::as_i64);
        let refreshing = m.refreshing.contains(&id);
        // The same sentence as a row: "platform-prod (412398765432) · PlatformAdmin".
        let account = Facts::of(prof, &self.model.read(cx).settings).detail();
        let label = if refreshing { "Refreshing…".to_string() } else { short_label(state, remaining) };
        let tone = match state {
            CredState::Expiring => p.warning,
            CredState::Expired => p.danger,
            _ => p.text_muted,
        };
        let expiry = summary
            .and_then(|d| d.get("expiresAtPst"))
            .and_then(Value::as_str)
            .filter(|_| state != CredState::Never)
            .map(|e| format!("{} {}", if state == CredState::Expired { "at" } else { "until" }, format_expiry(e)));

        let (id_console, id_refresh, id_edit, id_delete, name_delete) = (id.clone(), id.clone(), id.clone(), id.clone(), name.clone());
        // The console is the card's one strong action; the rest stay quiet.
        let actions = div()
            .mt_auto()
            .flex()
            .items_center()
            .gap(px(2.))
            .pl(px(16.))
            .pr(px(8.))
            .pb(px(12.))
            .pt(px(8.))
            .child(
                // Always the console; a profile that isn't signed in signs in first.
                primary_button(SharedString::from(format!("card-console-{id}")), "Console", Some("app-window"), p)
                    .aria_label(SharedString::from(format!("Open AWS console for {name}")))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .text_size(px(size::TEXT_SM))
                    .on_press(cx.listener(move |this, _, _, cx| {
                        let id = id_console.clone();
                        this.model.update(cx, |m, cx| m.open_console(id, cx))
                    })),
            )
            .child(div().flex_1())
            .child(
                refresh_button(SharedString::from(format!("card-refresh-{id}")), format!("Refresh {name}"), refreshing, p)
                    .when(refreshing, |d| d.cursor_default())
                    .on_press(cx.listener(move |this, _, _, cx| {
                        let id = id_refresh.clone();
                        if !this.model.read(cx).refreshing.contains(&id) {
                            this.model.update(cx, |m, cx| m.refresh(id, cx))
                        }
                    }))
                    .a11y_disabled(refreshing),
            )
            .child(self.move_menu(prof, cx))
            .child(icon_button(SharedString::from(format!("card-edit-{id}")), "pencil", format!("Edit {name}"), p).on_press(cx.listener(
                move |this, _, _, cx| {
                    let id = id_edit.clone();
                    this.model.update(cx, |m, cx| m.edit_profile(Some(id), cx))
                },
            )))
            .child(icon_button(SharedString::from(format!("card-delete-{id}")), "trash", format!("Delete {name}"), p).on_press(cx.listener(
                move |this, _, _, cx| {
                    let (id, name) = (id_delete.clone(), name_delete.clone());
                    this.model.update(cx, |m, cx| confirm_delete(m, id, &name, cx))
                },
            )));

        let grip = self.profile_handle(prof, p, cx);
        let avatar = avatar_of(prof);
        let card = div()
            .id(SharedString::from(format!("card-{id}")))
            .role(Role::Group)
            .aria_label(name.clone())
            .relative()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .rounded(px(size::RADIUS_LG))
            .overflow_hidden()
            .border_1()
            .border_color(p.border)
            .bg(p.panel)
            .hover(|d| d.border_color(p.border_strong));
        self.drop_target(card, prof, p, cx)
            // The profile's colour as a thin edge along the top, kept clear of the rounded corners.
            .child(div().absolute().top(px(0.)).left(px(16.)).right(px(16.)).h(px(3.)).rounded_b(px(2.)).bg(avatar.color()))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .p(px(16.))
                    .pl(px(8.))
                    .pb(px(12.))
                    .child(grip.mt(px(4.)))
                    .child(profile_avatar(&avatar, 40., 20., p))
                    .child(div().flex_1().min_w(px(0.)).flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).truncate().child(name.clone())).child(
                        read_text(SharedString::from(format!("card-subtitle-{id}")), account).text_size(px(size::TEXT_SM)).text_color(p.text_muted).truncate(),
                    )),
            )
            .child(
                div()
                    .px(px(16.))
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().id(SharedString::from(format!("card-dot-{id}"))).role(Role::Image).aria_label(state.name()).child(expiry_ring(
                                "ring-spin",
                                state,
                                session_fraction(Some(prof), remaining),
                                refreshing,
                                16.,
                                p,
                            )))
                            .child(
                                read_text(SharedString::from(format!("card-status-{id}")), label)
                                    .font_family(FONT_MONO)
                                    .text_size(px(size::TEXT_XS))
                                    .text_color(tone),
                            ),
                    )
                    .when_some(expiry, |d, e| {
                        d.child(div().pl(px(24.)).truncate().font_family(FONT_MONO).text_size(px(size::TEXT_XS)).text_color(p.text_faint).child(e))
                    }),
            )
            .child(actions)
    }

    /// "More ways to add profiles" beside Add profile (1.x add menu): "Add accounts…".
    fn add_menu(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.menu.as_ref().is_some_and(|(k, _, _)| k == ADD_MENU);
        let trigger = Button::new("add-more")
            .primary()
            .icon(Icon::empty().path(crate::assets::icon("chevron-down")).text_color(p.on_primary))
            .accessibility_label("More ways to add profiles")
            .h(px(size::BUTTON))
            .w(px(32.))
            .rounded_l(px(0.))
            .rounded_r(px(size::RADIUS))
            .bg(p.primary)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_add_menu(true, window, cx)));
        self.popover(ADD_MENU.to_string(), open, trigger, cx)
    }

    /// `focus`: false when the tour opens it (the tour card keeps the keyboard).
    fn toggle_add_menu(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.as_ref().is_some_and(|(k, _, _)| k == ADD_MENU) {
            self.menu = None;
            cx.notify();
            return;
        }
        let model = self.model.clone();
        let entries = vec![Entry::item("Add accounts…", move |_, cx| model.update(cx, |m, cx| m.open_import_wizard(cx)))
            .description("Import SAML or Identity Center accounts in bulk")];
        self.show_menu_with(ADD_MENU.to_string(), entries, focus, window, cx);
        self.tour_opened_menu = !focus;
    }

    // --- Drag and drop -----------------------------------------------------------------------------

    /// A grip: the drag handle for the mouse and the keyboard (1.x: focus it, Space to pick up,
    /// arrows to move, Space to drop, Escape to cancel).
    fn drag_handle(&self, key: String, item: DragItem, label: String, tip: &'static str, p: &Palette, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let handle = self.handles.get(&key).cloned().unwrap_or_else(|| cx.focus_handle());
        let active = self.kb_drag.as_ref().is_some_and(|k| k.item == item);
        let label: SharedString = label.into();
        div()
            .id(SharedString::from(format!("grip-{key}")))
            .role(Role::Button)
            .aria_label(label)
            .track_focus(&handle)
            .focus_ring(p)
            .w(px(24.))
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor(gpui::CursorStyle::OpenHand)
            .when(active, |d| d.bg(p.selected).border_1().border_color(p.accent))
            .hover(|d| d.bg(p.hover))
            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| this.drag_key(&item, ev, window, cx)))
            .child(icon("grip-vertical", 16., if active { p.text } else { p.text_faint }))
    }

    fn profile_handle(&self, prof: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let order = self.model.read(cx).profiles.iter().position(|v| s(v, "id") == id).unwrap_or(0);
        let drag = DraggedProfile { id: id.clone(), name: name.clone(), folder: s(prof, "folderId").to_string(), order, palette: *p };
        let view = cx.entity().downgrade();
        self.drag_handle(format!("p:{id}"), DragItem::Profile(id), format!("Reorder {name}"), "Drag to reorder or move into a folder", p, cx).on_drag(
            drag,
            move |d: &DraggedProfile, _, _, cx| {
                let _ = view.update(cx, |this, cx| this.mouse_drag_started(cx));
                cx.new(|_| DragPreview::new(d.name.clone(), "users", d.palette))
            },
        )
    }

    /// A row or card as a drop target: a profile dropped on it takes its place, with a line drawn
    /// on the side it will land.
    fn drop_target(&self, el: gpui::Stateful<gpui::Div>, prof: &Value, p: &Palette, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let id = s(prof, "id").to_string();
        let folder = s(prof, "folderId").to_string();
        let order = self.model.read(cx).profiles.iter().position(|v| s(v, "id") == id).unwrap_or(0);
        let accent = p.accent;
        let kb = self.kb_drag.as_ref().is_some_and(|k| k.item == DragItem::Profile(id.clone()));
        let target = id.clone();
        el.when(kb, |d| d.bg(p.selected))
            .drag_over::<DraggedProfile>(move |st, d, _, _| {
                if d.id == id {
                    st
                } else if d.folder == folder && d.order < order {
                    st.border_b_2().border_color(accent)
                } else {
                    st.border_t_2().border_color(accent)
                }
            })
            .on_drop(cx.listener(move |this, d: &DraggedProfile, _, cx| {
                if d.id == target {
                    return;
                }
                let (place, folder) = {
                    let m = this.model.read(cx);
                    let place = dnd::place_on_row(&m.profiles, &m.folders, &d.id, &target);
                    let folder = m
                        .profiles
                        .iter()
                        .find(|v| s(v, "id") == target)
                        .map(|v| s(v, "folderId").to_string())
                        .filter(|f| m.folders.iter().any(|x| s(x, "id") == f));
                    (place, folder)
                };
                this.commit_profile(&d.id, folder, place, cx);
            }))
    }

    /// A mouse drag began: it replaces any keyboard drag, and the list redraws with every drop
    /// target showing.
    fn mouse_drag_started(&mut self, cx: &mut Context<Self>) {
        self.kb_drag = None;
        cx.notify();
    }

    fn drag_key(&mut self, item: &DragItem, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.modifiers.modified() {
            return;
        }
        let active = self.kb_drag.as_ref().is_some_and(|d| d.item == *item);
        let handled = match (active, k.key.as_str()) {
            (false, "space") => {
                self.pick_up(item.clone(), cx);
                true
            }
            (true, "up" | "left") => {
                self.step(-1, cx);
                true
            }
            (true, "down" | "right") => {
                self.step(1, cx);
                true
            }
            (true, "space" | "enter") => {
                self.drop_kb(cx);
                true
            }
            (true, "escape") => {
                let name = self.item_name(item, cx);
                self.kb_drag = None;
                self.announce = Some(format!("Cancelled. {name} was returned to where it was.").into());
                true
            }
            _ => false,
        };
        if handled {
            // Keeps the keyboard click (Space on a focused element) from firing as well.
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn item_name(&self, item: &DragItem, cx: &Context<Self>) -> String {
        let m = self.model.read(cx);
        match item {
            DragItem::Profile(id) => m.profiles.iter().find(|v| s(v, "id") == id).map(|v| s(v, "name").to_string()).unwrap_or_default(),
            DragItem::Folder(id) => m.folders.iter().find(|v| s(v, "id") == id).map(|v| format!("{} folder", s(v, "name"))).unwrap_or_default(),
        }
    }

    fn pick_up(&mut self, item: DragItem, cx: &mut Context<Self>) {
        let m = self.model.read(cx).snapshot();
        let drag = match &item {
            DragItem::Profile(id) => {
                let sections = arrange(&m);
                let slots = dnd::profile_slots(&sections, &m.collapsed_folders, id);
                let Some(at) = dnd::slot_of(&sections, id).and_then(|cur| slots.iter().position(|x| *x == cur)) else { return };
                KbDrag { item: item.clone(), slots, slot: at, origin: at }
            }
            DragItem::Folder(id) => {
                let Some(at) = m.folders.iter().position(|f| s(f, "id") == id) else { return };
                KbDrag { item: item.clone(), slots: Vec::new(), slot: at, origin: at }
            }
        };
        self.kb_drag = Some(drag);
        let name = self.item_name(&item, cx);
        self.announce = Some(format!("Picked up {name}. Use the arrow keys to move it, Space to drop it, Escape to cancel.").into());
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let folders = self.model.read(cx).folders.len();
        let Some(k) = &mut self.kb_drag else { return };
        let count = if matches!(k.item, DragItem::Folder(_)) { folders } else { k.slots.len() };
        if count == 0 {
            return;
        }
        k.slot = (k.slot as isize + by).clamp(0, count as isize - 1) as usize;
        let item = k.item.clone();
        let text = self.where_text(&item, cx);
        self.announce = Some(text.into());
    }

    /// "Sandbox is in Team A, position 1 of 2." for the announcements.
    fn where_text(&self, item: &DragItem, cx: &Context<Self>) -> String {
        let name = self.item_name(item, cx);
        let Some(k) = &self.kb_drag else { return String::new() };
        let m = self.model.read(cx).snapshot();
        match item {
            DragItem::Folder(_) => format!("{name} is at position {} of {}.", k.slot + 1, m.folders.len()),
            DragItem::Profile(id) => {
                let mut sections = arrange(&m);
                dnd::preview_profile(&mut sections, id, &k.slots[k.slot]);
                for sec in &sections {
                    if let Some(i) = sec.rows.iter().position(|r| s(r, "id") == id) {
                        let place = sec.folder.as_ref().map(|f| s(f, "name").to_string()).unwrap_or_else(|| "No folder".into());
                        return format!("{name} is in {place}, position {} of {}.", i + 1, sec.rows.len());
                    }
                }
                String::new()
            }
        }
    }

    fn drop_kb(&mut self, cx: &mut Context<Self>) {
        let Some(k) = self.kb_drag.clone() else { return };
        let text = self.where_text(&k.item, cx).replacen(" is ", " was dropped ", 1);
        self.kb_drag = None;
        self.announce = Some(text.into());
        if k.slot == k.origin {
            return;
        }
        match &k.item {
            DragItem::Profile(id) => {
                let (folder, place) = k.slots[k.slot].clone();
                self.commit_profile(id, folder, place, cx);
            }
            DragItem::Folder(id) => self.commit_folder(id, k.slot, cx),
        }
    }

    /// Persist a profile move (1.x `persistLayout`): the whole order plus the moved profile's
    /// folder. The list shows the new order straight away; the reload confirms it.
    fn commit_profile(&mut self, id: &str, folder: Option<String>, place: Place, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.model.update(cx, |m, cx| {
            let ordered = dnd::apply_profile_move(&m.profiles, &m.folders, &id, folder.as_deref(), &place);
            let ids: Vec<Value> = ordered.iter().map(|v| v["id"].clone()).collect();
            m.profiles = ordered;
            let mut moved = serde_json::Map::new();
            moved.insert(id.clone(), folder.clone().map(Value::String).unwrap_or(Value::Null));
            m.run("profiles:applyLayout", vec![Value::Array(ids), Value::Object(moved)], cx);
            cx.notify();
        });
    }

    fn commit_folder(&mut self, id: &str, to: usize, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.model.update(cx, |m, cx| {
            if m.folders.iter().position(|f| s(f, "id") == id) == Some(to) {
                return;
            }
            m.folders = dnd::reorder_folders(&m.folders, &id, to);
            let ids: Vec<Value> = m.folders.iter().map(|f| f["id"].clone()).collect();
            m.run("folders:reorder", vec![Value::Array(ids)], cx);
            cx.notify();
        });
    }

    /// 1.x MoveToFolderMenu on a card: the folders (where it is now shown but not choosable), and
    /// "Remove from folder" when it is in one.
    fn move_menu(&self, prof: &Value, cx: &mut Context<Self>) -> impl IntoElement {
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let key = format!("move:{id}");
        let open = self.menu.as_ref().is_some_and(|(k, _, _)| *k == key);
        let prof_click = prof.clone();
        let trigger = Button::new(SharedString::from(format!("card-move-{id}")))
            .ghost()
            .icon(Icon::empty().path(crate::assets::icon("folder")))
            .accessibility_label(format!("Move {name} to folder"))
            .tooltip("Move to folder")
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_move_menu(&prof_click, window, cx)));
        self.popover(key, open, trigger, cx)
    }

    fn toggle_move_menu(&mut self, prof: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let id = s(prof, "id").to_string();
        let key = format!("move:{id}");
        if self.menu.as_ref().is_some_and(|(k, _, _)| *k == key) {
            self.menu = None;
            cx.notify();
            return;
        }
        let folders: Vec<(String, String)> = self.model.read(cx).folders.iter().map(|f| (s(f, "id").to_string(), s(f, "name").to_string())).collect();
        let current = Some(s(prof, "folderId").to_string()).filter(|c| folders.iter().any(|(fid, _)| fid == c));
        let model = self.model.clone();
        let mut entries = Vec::new();
        if folders.is_empty() {
            entries.push(Entry::Item {
                label: "No folders yet".into(),
                danger: false,
                disabled: true,
                tag: None,
                description: None,
                on_click: std::rc::Rc::new(|_, _| {}),
            });
        }
        for (fid, fname) in folders {
            let (model, pid) = (model.clone(), id.clone());
            let here = current.as_deref() == Some(fid.as_str());
            let item = Entry::item(fname, move |_, cx| model.update(cx, |m, cx| m.move_to_folder(&pid, Some(fid.clone()), cx)));
            entries.push(if here { item.disabled_with("current") } else { item });
        }
        if current.is_some() {
            let (model, pid) = (model.clone(), id.clone());
            entries.push(Entry::Separator);
            entries.push(Entry::item("Remove from folder", move |_, cx| model.update(cx, |m, cx| m.move_to_folder(&pid, None, cx))));
        }
        self.show_menu(key, entries, window, cx);
    }

    /// The row's "⋯" menu: Edit, Move to folder, Delete (1.x had Edit / Delete on the card and
    /// moving by drag; 2.0 keeps Move to folder here too, the quickest way across a long list).
    ///
    /// A controlled popover rather than `dropdown_menu`: the trigger is a plain button with a click
    /// handler, so UI Automation can Invoke it (the dropdown trigger opens on mouse down only).
    fn row_menu(&self, prof: &Value, cx: &mut Context<Self>) -> impl IntoElement {
        let id = s(prof, "id").to_string();
        let name = s(prof, "name").to_string();
        let open = self.menu.as_ref().is_some_and(|(mid, _, _)| *mid == id);
        let prof_click = prof.clone();
        let trigger = Button::new(SharedString::from(format!("more-{id}")))
            .ghost()
            .icon(Icon::empty().path(crate::assets::icon("ellipsis")))
            .accessibility_label(format!("More actions for {name}"))
            .tooltip(format!("More actions for {name}"))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_menu(&prof_click, window, cx)));
        self.popover(id, open, trigger, cx)
    }

    fn toggle_menu(&mut self, prof: &Value, window: &mut Window, cx: &mut Context<Self>) {
        let id = s(prof, "id").to_string();
        if self.menu.as_ref().is_some_and(|(mid, _, _)| *mid == id) {
            self.menu = None;
            cx.notify();
            return;
        }
        let name = s(prof, "name").to_string();
        let current_folder = s(prof, "folderId").to_string();
        let folders: Vec<(String, String)> = self.model.read(cx).folders.iter().map(|f| (s(f, "id").to_string(), s(f, "name").to_string())).collect();
        let model = self.model.clone();
        let mut entries = Vec::new();
        {
            let (model, id) = (model.clone(), id.clone());
            entries.push(Entry::item("Edit", move |_, cx| {
                let id = id.clone();
                model.update(cx, |m, cx| m.edit_profile(Some(id), cx));
            }));
        }
        if !folders.is_empty() || !current_folder.is_empty() {
            let options = std::iter::once((String::new(), "No folder".to_string())).chain(folders);
            let children = options
                .map(|(fid, fname)| {
                    let (model, pid, current) = (model.clone(), id.clone(), fid == current_folder);
                    let item = Entry::item(fname, move |_, cx| {
                        let target = (!fid.is_empty()).then(|| fid.clone());
                        model.update(cx, |m, cx| m.move_to_folder(&pid, target, cx));
                    });
                    // Where it already is: shown, not choosable (1.x context menu).
                    if current {
                        item.disabled_with("current")
                    } else {
                        item
                    }
                })
                .collect();
            entries.push(Entry::Group { label: "Move to folder".into(), children });
        }
        entries.push(Entry::Separator);
        {
            let (model, id) = (model.clone(), id.clone());
            entries.push(
                Entry::item("Delete", move |_, cx| {
                    let (id, name) = (id.clone(), name.clone());
                    model.update(cx, |m, cx| confirm_delete(m, id, &name, cx));
                })
                .danger(),
            );
        }
        self.show_menu(id, entries, window, cx);
    }

    /// Folder header "Folder options for X": Rename, Change color, Delete folder (1.x
    /// ProfileFolderSection).
    fn folder_menu(&self, folder: &Value, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let fid = s(folder, "id").to_string();
        let name = s(folder, "name").to_string();
        let key = format!("folder:{fid}");
        let open = self.menu.as_ref().is_some_and(|(k, _, _)| *k == key);
        let folder_click = folder.clone();
        let trigger = Button::new(SharedString::from(format!("folder-opts-{fid}")))
            .ghost()
            .icon(Icon::empty().path(crate::assets::icon("ellipsis")))
            .accessibility_label(format!("Folder options for {name}"))
            .tooltip(format!("Folder options for {name}"))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle_folder_menu(&folder_click, count, window, cx)));
        self.popover(key, open, trigger, cx)
    }

    fn popover(&self, key: String, open: bool, trigger: Button, cx: &mut Context<Self>) -> Popover {
        let mut pop = Popover::new(SharedString::from(format!("menu-{key}")))
            .appearance(false)
            .overlay_closable(false)
            .anchor(gpui::Anchor::TopRight)
            .trigger(trigger)
            .open(open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                if !*open {
                    this.menu = None;
                    cx.notify();
                }
            }));
        if let Some((_, menu, _)) = self.menu.as_ref().filter(|_| open) {
            let menu = menu.clone();
            pop = pop.content(move |_, _, _| menu.clone());
        }
        pop
    }

    /// Open a menu under `key`.
    fn show_menu(&mut self, key: String, entries: Vec<Entry>, window: &mut Window, cx: &mut Context<Self>) {
        self.show_menu_with(key, entries, true, window, cx)
    }

    fn show_menu_with(&mut self, key: String, entries: Vec<Entry>, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let palette = self.model.read(cx).palette;
        let menu = cx.new(|cx| MenuView::new(entries, palette, cx));
        let sub = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.menu = None;
            cx.notify();
        });
        if focus {
            menu.focus_handle(cx).focus(window, cx);
        }
        self.menu = Some((key, menu, sub));
        cx.notify();
    }

    fn toggle_folder_menu(&mut self, folder: &Value, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        let fid = s(folder, "id").to_string();
        let key = format!("folder:{fid}");
        if self.menu.as_ref().is_some_and(|(k, _, _)| *k == key) {
            self.menu = None;
            cx.notify();
            return;
        }
        let name = s(folder, "name").to_string();
        let current = s(folder, "color").to_lowercase();
        let model = self.model.clone();
        let view = cx.entity().downgrade();
        let mut entries = Vec::new();
        {
            let (fid, name, view) = (fid.clone(), name.clone(), view.clone());
            entries.push(Entry::item("Rename", move |window, cx| {
                let (fid, name) = (fid.clone(), name.clone());
                let _ = view.update(cx, |this, cx| this.start_rename(fid, name, window, cx));
            }));
        }
        let swatches = FOLDER_COLORS
            .iter()
            .map(|hex| {
                let (fid, model, hex) = (fid.clone(), model.clone(), *hex);
                Swatch {
                    label: format!("Set folder color {hex}").into(),
                    color: parse_hex(hex).unwrap_or(gpui::rgb(0x94a3b8)),
                    checked: current == hex,
                    on_click: std::rc::Rc::new(move |_: &mut Window, cx: &mut gpui::App| {
                        let fid = fid.clone();
                        model.update(cx, |m, cx| m.run("folders:save", vec![serde_json::json!({ "id": fid, "color": hex })], cx));
                    }),
                }
            })
            .collect();
        let colors = vec![Entry::Swatches(swatches)];
        entries.push(Entry::Group { label: "Change color".into(), children: colors });
        entries.push(Entry::Separator);
        {
            let (fid, model) = (fid.clone(), model.clone());
            entries.push(
                Entry::item("Delete folder", move |_, cx| {
                    let body = if count > 0 {
                        format!(
                            "Delete the folder \"{name}\"? The {count} profile{} inside will move to Ungrouped. No profile or credential is deleted.",
                            if count == 1 { "" } else { "s" }
                        )
                    } else {
                        format!("Delete the folder \"{name}\"? It is empty.")
                    };
                    let id = fid.clone();
                    model.update(cx, |m, cx| {
                        m.open_dialog(
                            DialogKind::Confirm {
                                title: "Delete folder".into(),
                                body,
                                confirm: "Delete folder".into(),
                                action: ConfirmAction::DeleteFolder { id },
                            },
                            cx,
                        )
                    });
                })
                .danger(),
            );
        }
        self.show_menu(key, entries, window, cx);
    }

    fn start_rename(&mut self, fid: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(name, window, cx);
            st
        });
        input.update(cx, |st, cx| st.focus(window, cx));
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_rename(cx),
            _ => {}
        });
        self.renaming = Some((fid, input, sub));
        cx.notify();
    }

    /// Enter or leaving the field saves; a blank or unchanged name keeps the old one (1.x).
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some((fid, input, _)) = self.renaming.take() else { return };
        let next = input.read(cx).value().trim().to_string();
        let old = self.model.read(cx).folders.iter().find(|f| s(f, "id") == fid).map(|f| s(f, "name").to_string()).unwrap_or_default();
        if !next.is_empty() && next != old {
            self.model.update(cx, |m, cx| m.run("folders:save", vec![serde_json::json!({ "id": fid, "name": next })], cx));
        }
        cx.notify();
    }

    fn render_side_panel(&self, id: &str, m: &Snap, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let prof = m.profile(id).cloned().unwrap_or(Value::Null);
        let summary = m.summary(id).cloned();
        let state = summary.as_ref().map(CredState::of).unwrap_or(CredState::Never);
        let remaining = summary.as_ref().and_then(|d| d.get("timeRemainingSeconds")).and_then(Value::as_i64);
        let name = s(&prof, "name").to_string();
        let section = {
            let c = s(&prof, "credentialProfileName");
            if c.is_empty() {
                name.clone()
            } else {
                c.to_string()
            }
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
            if mins % 60 == 0 {
                format!("Every {} h", mins / 60)
            } else {
                format!("Every {mins} min")
            }
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
                .child(
                    icon_button(SharedString::from(format!("copy-{label}")), "copy", format!("Copy {label} command"), p)
                        .on_press(move |_, _, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(code.clone()))),
                )
        };
        let fact = |k: &'static str, v: String, mono_value: bool| {
            div()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .text_size(px(size::TEXT_SM))
                .child(div().w(px(92.)).flex_shrink_0().text_color(p.text_muted).child(k))
                .child(
                    // Read as "Method: SAML / ADFS".
                    read_text(SharedString::from(format!("fact-{k}")), v.clone())
                        .aria_label(SharedString::from(format!("{k}: {v}")))
                        .min_w(px(0.))
                        .truncate()
                        .when(mono_value, |d| d.font_family(FONT_MONO).text_size(px(size::TEXT_XS))),
                )
        };
        let refreshing = m.refreshing.contains(id);

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
                    .items_center()
                    .gap(px(12.))
                    .child(profile_avatar(&avatar_of(&prof), 40., 20., p))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone()))
                            .child(read_text("panel-label", s(&prof, "label").to_string()).text_size(px(size::TEXT_SM)).text_color(p.text_muted)),
                    )
                    .child(icon_button("close-details", "x", "Close details", p).on_press(cx.listener(|this, _, _, cx| {
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
                            .child(div().id("panel-ring").role(Role::Image).aria_label(state.name()).child(expiry_ring(
                                "panel-ring-spin",
                                state,
                                session_fraction(Some(&prof), remaining),
                                refreshing,
                                22.,
                                p,
                            )))
                            .child(
                                read_text("panel-status", big).font_family(FONT_MONO).text_size(px(24.)).font_weight(FontWeight::MEDIUM).text_color(big_color),
                            ),
                    )
                    .child(read_text("panel-expiry", sub).text_size(px(size::TEXT_SM)).text_color(p.text_muted)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(refresh_secondary_button("panel-refresh", "Refresh", refreshing, p).on_press(cx.listener({
                        let id = id.to_string();
                        move |this, _, _, cx| this.model.update(cx, |m, cx| m.refresh(id.clone(), cx))
                    })))
                    .child(primary_button("panel-console", "Open console", Some("app-window"), p).on_press(cx.listener({
                        let id = id.to_string();
                        move |this, _, _, cx| this.model.update(cx, |m, cx| m.open_console(id.clone(), cx))
                    }))),
            )
            .child(
                div().flex().flex_col().gap(px(8.)).child(div().text_size(px(size::TEXT_SM)).font_weight(FontWeight::SEMIBOLD).child("Use in a shell")).child(
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
