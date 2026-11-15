//! The tray: status badge, image and menu (1.x `tray.ts` and the tray half of `appIcon.ts`).
//!
//! Pure parts (status rules, badge drawing, menu model) are here and unit tested; `main.rs` turns
//! the menu model into a `muda` menu and owns the `tray_icon::TrayIcon`.

use std::collections::HashSet;
use std::path::Path;

use apm_core::core::Core;
use apm_core::extras::icons::CLASSIC_APP_ICON;
use serde_json::Value;

/// The credential status the tray image shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayStatus {
    Ok,
    /// An auto-refresh profile expires within 5 minutes (amber).
    Warn,
    /// Auto refresh stopped after repeated failures (red).
    Err,
    /// Auto refresh paused by the user (gray).
    Paused,
}

/// A profile counts as "expiring" for the badge only this close to expiry. The scheduler already
/// refreshes at 15 minutes out, so a wider window would flash the badge before every routine refresh.
pub const EXPIRING_BADGE_MS: i64 = 5 * 60_000;

/// 1.x `computeTrayStatus`: failures beat a user pause, which beats expiry.
pub fn compute_status(paused: bool, paused_due_to_failures: bool, profiles: &[serde_json::Map<String, Value>], now_ms: i64) -> TrayStatus {
    if paused_due_to_failures {
        return TrayStatus::Err;
    }
    if paused {
        return TrayStatus::Paused;
    }
    let soon = now_ms + EXPIRING_BADGE_MS;
    let expiring = profiles.iter().any(|p| {
        let auto = match p.get("autoRefresh") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Null) | None => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            Some(_) => true,
        };
        let exp = p.get("expiration").and_then(Value::as_str).filter(|s| !s.is_empty()).and_then(apm_core::env::parse_js_date_ms);
        auto && exp.is_some_and(|t| t <= soon)
    });
    if expiring {
        TrayStatus::Warn
    } else {
        TrayStatus::Ok
    }
}

pub fn tooltip(status: TrayStatus) -> &'static str {
    match status {
        TrayStatus::Ok => "AWS Profile Manager",
        TrayStatus::Warn => "AWS Profile Manager — credentials expiring",
        TrayStatus::Err => "AWS Profile Manager — auto refresh stopped after failures",
        TrayStatus::Paused => "AWS Profile Manager — auto refresh paused",
    }
}

/// Badge colours; `Ok` never draws one.
pub fn badge_rgb(status: TrayStatus) -> Option<[u8; 3]> {
    match status {
        TrayStatus::Ok => None,
        TrayStatus::Warn => Some([0xff, 0xb0, 0x20]),
        TrayStatus::Err => Some([0xff, 0x4d, 0x4f]),
        TrayStatus::Paused => Some([0x8a, 0x8f, 0xa3]),
    }
}

/// A status dot in the bottom-right corner of a square straight-alpha RGBA image, with a
/// transparent ring knocked out around it so it reads against any icon and taskbar colour.
/// Antialiased by distance from the edge (1.x `drawBadge`, which worked on premultiplied BGRA).
pub fn draw_badge(px: &mut [u8], size: u32, rgb: [u8; 3]) {
    let n = size as f32;
    let c = n * 0.78;
    let r = n * 0.22;
    let gap = (n * 0.07).max(1.);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            let hole = (r + gap + 0.5 - d).clamp(0., 1.);
            if hole == 0. {
                continue;
            }
            let dot = (r + 0.5 - d).clamp(0., 1.);
            let i = ((y * size + x) * 4) as usize;
            let keep = 1. - hole;
            let old_a = px[i + 3] as f32 / 255.;
            let under = old_a * keep * (1. - dot);
            let out_a = dot + under;
            for k in 0..3 {
                let v = if out_a > 0. { (rgb[k] as f32 * dot + px[i + k] as f32 * under) / out_a } else { 0. };
                px[i + k] = v.round().clamp(0., 255.) as u8;
            }
            px[i + 3] = (out_a * 255.).round().clamp(0., 255.) as u8;
        }
    }
}

/// Monochrome sources are white; a light taskbar needs black.
pub fn to_black(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        p[0] = 0;
        p[1] = 0;
        p[2] = 0;
    }
}

/// Tray settings that decide the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayLook {
    pub key: String,
    pub mono: bool,
    pub badge: bool,
    /// Windows: the taskbar is dark (a monochrome glyph stays white).
    pub taskbar_dark: bool,
    /// Tray icon pixel size (16 at 100% scaling, 32 at 200%).
    pub size: u32,
}

/// The tray image as straight-alpha RGBA (`(pixels, width, height)`).
///
/// Classic keeps the tray image it always had: icon.png resized (to 32 px in 1.x), plus the badge.
/// Designs use their pre-rendered `tray-<n>.png` / `tray-mono-<n>.png`.
pub fn tray_image(resources: &Path, look: &TrayLook, status: TrayStatus) -> Option<(Vec<u8>, u32, u32)> {
    let badge = if look.badge { badge_rgb(status) } else { None };
    let mut img = if look.key == CLASSIC_APP_ICON {
        let src = image::open(resources.join("icon.png")).ok()?;
        src.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8()
    } else {
        let size = if look.size > 16 { 32 } else { 16 };
        let file = resources.join("app-icons").join(&look.key).join(if look.mono { format!("tray-mono-{size}.png") } else { format!("tray-{size}.png") });
        match image::open(&file) {
            Ok(i) => i.to_rgba8(),
            // A design without a tray set falls back to Classic's.
            Err(_) => image::open(resources.join("icon.png")).ok()?.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8(),
        }
    };
    let (w, h) = img.dimensions();
    let px: &mut [u8] = &mut img;
    if look.mono && look.key != CLASSIC_APP_ICON && !look.taskbar_dark {
        to_black(px);
    }
    if let Some(rgb) = badge {
        if w == h {
            draw_badge(px, w, rgb);
        }
    }
    Some((img.into_raw(), w, h))
}

/// Read the look from the settings.
pub fn look(core: &Core, taskbar_dark: bool, size: u32) -> TrayLook {
    let settings = core.settings().get();
    TrayLook {
        key: core.current_app_icon_key(),
        mono: settings.get("trayIconStyle").and_then(Value::as_str) == Some("monochrome"),
        badge: settings.get("trayStatusBadge") != Some(&Value::Bool(false)),
        taskbar_dark,
        size,
    }
}

pub fn status(core: &Core) -> TrayStatus {
    let pause = core.refresh_pause_state();
    compute_status(pause.paused, pause.paused_due_to_failures, &core.profiles().all(), core.cfg.clock.now_ms())
}

// ---- menu --------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Open,
    /// Open the AWS console for (profile id, menu label).
    Console(String, String),
    Pause,
    Resume,
    RefreshAll,
    Refresh(String),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Action { label: String, enabled: bool, action: Option<Action> },
    Submenu { label: String, enabled: bool, items: Vec<Item> },
    Separator,
}

fn item(label: impl Into<String>, enabled: bool, action: Action) -> Item {
    Item::Action { label: label.into(), enabled, action: Some(action) }
}

fn note(label: impl Into<String>) -> Item {
    Item::Action { label: label.into(), enabled: false, action: None }
}

/// 1.x `buildContextMenu`. `profiles` are (id, menu label) in display order.
///
/// The tray is reachable while the window is on the unlock screen, so everything that touches a
/// profile is disabled while locked; only "Open App" (how you reach the unlock prompt) and "Exit"
/// stay live. Items are shown-but-disabled rather than hidden, so the menu does not look broken.
pub fn build_menu(profiles: &[(String, String)], locked: bool, consoles_in_flight: &HashSet<String>) -> Vec<Item> {
    let mut refresh = vec![item("All", true, Action::RefreshAll)];
    refresh.extend(profiles.iter().map(|(id, label)| item(label.clone(), true, Action::Refresh(id.clone()))));

    let console: Vec<Item> = if profiles.is_empty() {
        vec![note("No profiles yet")]
    } else {
        profiles
            .iter()
            .map(|(id, label)| {
                let busy = consoles_in_flight.contains(id);
                let shown = if busy { format!("{label}  (opening…)") } else { label.clone() };
                item(shown, !busy, Action::Console(id.clone(), label.clone()))
            })
            .collect()
    };

    let mut menu = vec![item("Open App", true, Action::Open), Item::Separator];
    if locked {
        menu.push(note("Locked — open the app to unlock"));
    }
    menu.push(Item::Submenu { label: "AWS Console".into(), enabled: !locked, items: console });
    menu.push(item("Pause Auto Refresh", !locked, Action::Pause));
    menu.push(item("Resume Auto Refresh", !locked, Action::Resume));
    menu.push(Item::Submenu { label: "Refresh profile".into(), enabled: !locked, items: refresh });
    menu.push(Item::Separator);
    menu.push(item("Exit", true, Action::Exit));
    menu
}

/// (id, label) for every profile, with 1.x `profileMenuLabel`.
pub fn menu_profiles(core: &Core) -> Vec<(String, String)> {
    let names = match core.settings().get().get("accountDisplayNames") {
        Some(Value::Object(m)) => m.clone(),
        _ => serde_json::Map::new(),
    };
    core.profiles()
        .all()
        .iter()
        .filter_map(|p| Some((p.get("id")?.as_str()?.to_string(), apm_core::dashboard::profile_menu_label(p, &names))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn status_rules() {
        let now = 1_700_000_000_000i64;
        let iso = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let soon = profile(json!({ "autoRefresh": true, "expiration": iso(now + 4 * 60_000) }));
        let later = profile(json!({ "autoRefresh": true, "expiration": iso(now + 10 * 60_000) }));
        let manual = profile(json!({ "autoRefresh": false, "expiration": iso(now + 60_000) }));
        let expired = profile(json!({ "autoRefresh": true, "expiration": iso(now - 60_000) }));
        assert_eq!(compute_status(false, false, &[later.clone()], now), TrayStatus::Ok);
        assert_eq!(compute_status(false, false, &[manual.clone()], now), TrayStatus::Ok);
        assert_eq!(compute_status(false, false, &[later.clone(), soon.clone()], now), TrayStatus::Warn);
        assert_eq!(compute_status(false, false, &[expired], now), TrayStatus::Warn);
        assert_eq!(compute_status(true, false, &[soon.clone()], now), TrayStatus::Paused);
        assert_eq!(compute_status(true, true, &[soon], now), TrayStatus::Err);
        assert_eq!(compute_status(false, false, &[profile(json!({ "autoRefresh": true }))], now), TrayStatus::Ok);
    }

    #[test]
    fn badge_colours() {
        assert_eq!(badge_rgb(TrayStatus::Ok), None);
        assert_eq!(badge_rgb(TrayStatus::Warn), Some([0xff, 0xb0, 0x20]));
        assert_eq!(badge_rgb(TrayStatus::Err), Some([0xff, 0x4d, 0x4f]));
        assert_eq!(badge_rgb(TrayStatus::Paused), Some([0x8a, 0x8f, 0xa3]));
    }

    #[test]
    fn badge_draws_dot_and_ring() {
        let size = 32u32;
        let mut px = vec![255u8; (size * size * 4) as usize];
        draw_badge(&mut px, size, [0xff, 0x4d, 0x4f]);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // Centre of the dot (0.78 of the size): the badge colour, opaque.
        assert_eq!(at(25, 25), [0xff, 0x4d, 0x4f, 255]);
        // Just outside the dot, inside the knocked-out ring: transparent.
        let ring_x = 16; // 8.5 px left of the centre: past the dot, inside the gap
        assert_eq!(at(ring_x, 25)[3], 0);
        // Far corner untouched.
        assert_eq!(at(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn menu_matches_1x() {
        let profiles = vec![("p1".to_string(), "Dev - Admin".to_string()), ("p2".to_string(), "Prod - Read".to_string())];
        let mut busy = HashSet::new();
        busy.insert("p2".to_string());
        let menu = build_menu(&profiles, false, &busy);
        let labels: Vec<String> = menu
            .iter()
            .map(|i| match i {
                Item::Action { label, .. } | Item::Submenu { label, .. } => label.clone(),
                Item::Separator => "-".into(),
            })
            .collect();
        assert_eq!(labels, ["Open App", "-", "AWS Console", "Pause Auto Refresh", "Resume Auto Refresh", "Refresh profile", "-", "Exit"]);
        let Item::Submenu { items, .. } = &menu[2] else { panic!() };
        assert_eq!(items[0], item("Dev - Admin", true, Action::Console("p1".into(), "Dev - Admin".into())));
        assert_eq!(items[1], item("Prod - Read  (opening…)", false, Action::Console("p2".into(), "Prod - Read".into())));
        let Item::Submenu { items, .. } = &menu[5] else { panic!() };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], item("All", true, Action::RefreshAll));
    }

    #[test]
    fn locked_menu_disables_profile_actions() {
        let menu = build_menu(&[], true, &HashSet::new());
        assert_eq!(menu[2], note("Locked — open the app to unlock"));
        let enabled: Vec<bool> = menu
            .iter()
            .filter_map(|i| match i {
                Item::Action { enabled, action: Some(_), .. } | Item::Submenu { enabled, .. } => Some(*enabled),
                _ => None,
            })
            .collect();
        // Open App, AWS Console, Pause, Resume, Refresh profile, Exit
        assert_eq!(enabled, [true, false, false, false, false, true]);
        let Item::Submenu { items, .. } = &menu[3] else { panic!() };
        assert_eq!(items, &vec![note("No profiles yet")]);
    }

    #[test]
    fn classic_and_mono_images() {
        let res = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources");
        let classic = TrayLook { key: CLASSIC_APP_ICON.into(), mono: false, badge: true, taskbar_dark: true, size: 16 };
        let (_, w, h) = tray_image(&res, &classic, TrayStatus::Warn).unwrap();
        assert_eq!((w, h), (32, 32));
        let design = std::fs::read_dir(res.join("app-icons")).unwrap().filter_map(|e| e.ok()).find(|e| e.path().is_dir()).unwrap();
        let key = design.file_name().to_string_lossy().into_owned();
        let mono_light = TrayLook { key: key.clone(), mono: true, badge: false, taskbar_dark: false, size: 16 };
        let (px, w, _) = tray_image(&res, &mono_light, TrayStatus::Ok).unwrap();
        assert_eq!(w, 16);
        // Black glyph on a light taskbar.
        assert!(px.chunks_exact(4).filter(|p| p[3] > 0).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
        let mono_dark = TrayLook { taskbar_dark: true, size: 32, ..mono_light };
        let (px, w, _) = tray_image(&res, &mono_dark, TrayStatus::Ok).unwrap();
        assert_eq!(w, 32);
        assert!(px.chunks_exact(4).any(|p| p[3] > 128 && p[0] > 200));
    }
}

// ---- HUD placement (1.x trayHud.ts computeCardRect) ---------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Which edge the taskbar occupies, from the gap between the display and its work area. An
/// auto-hidden taskbar (no gap) counts as bottom.
fn taskbar_edge(display: Rect, area: Rect) -> &'static str {
    if area.y > display.y {
        "top"
    } else if area.x > display.x {
        "left"
    } else if area.x + area.w < display.x + display.w {
        "right"
    } else {
        "bottom"
    }
}

/// Resting rect of the HUD card: parked against the taskbar edge, aligned with the tray icon only
/// when the icon is actually on the taskbar (an icon in the overflow flyout reports a position
/// inside the work area, which would strand the card mid-screen). All in one pixel space.
pub fn compute_card_rect(tray: Rect, display: Rect, area: Rect, card_w: f64, card_h: f64, margin: f64) -> Rect {
    let edge = taskbar_edge(display, area);
    let cx = tray.x + tray.w / 2.;
    let cy = tray.y + tray.h / 2.;
    let icon_on_taskbar = cx < area.x || cx >= area.x + area.w || cy < area.y || cy >= area.y + area.h;
    let right_most = area.x + area.w - card_w - margin;
    let bottom_most = area.y + area.h - card_h - margin;
    let (x, y) = if edge == "bottom" || edge == "top" {
        let y = if edge == "bottom" { bottom_most } else { area.y + margin };
        (if icon_on_taskbar { (cx - card_w / 2.).round() } else { right_most }, y)
    } else {
        let x = if edge == "right" { right_most } else { area.x + margin };
        (x, if icon_on_taskbar { (cy - card_h / 2.).round() } else { bottom_most })
    };
    // max < min when the work area is narrower than the card: prefer the top-left corner.
    let clamp = |v: f64, lo: f64, hi: f64| v.min(hi).max(lo);
    Rect { x: clamp(x, area.x + margin, right_most), y: clamp(y, area.y + margin, bottom_most), w: card_w, h: card_h }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    const DISPLAY: Rect = Rect { x: 0., y: 0., w: 1920., h: 1080. };

    #[test]
    fn bottom_taskbar_icon_on_taskbar() {
        let area = Rect { x: 0., y: 0., w: 1920., h: 1040. };
        let tray = Rect { x: 1700., y: 1050., w: 24., h: 24. };
        let r = compute_card_rect(tray, DISPLAY, area, 300., 64., 4.);
        assert_eq!(r, Rect { x: 1562., y: 972., w: 300., h: 64. });
    }

    #[test]
    fn overflow_icon_goes_to_the_corner() {
        let area = Rect { x: 0., y: 0., w: 1920., h: 1040. };
        let tray = Rect { x: 1500., y: 900., w: 24., h: 24. };
        let r = compute_card_rect(tray, DISPLAY, area, 300., 64., 4.);
        assert_eq!((r.x, r.y), (1616., 972.));
    }

    #[test]
    fn left_and_top_taskbars() {
        let left = Rect { x: 60., y: 0., w: 1860., h: 1080. };
        let tray = Rect { x: 10., y: 1000., w: 24., h: 24. };
        let r = compute_card_rect(tray, DISPLAY, left, 300., 64., 4.);
        assert_eq!((r.x, r.y), (64., 980.));
        let top = Rect { x: 0., y: 40., w: 1920., h: 1040. };
        let tray = Rect { x: 1800., y: 8., w: 24., h: 24. };
        let r = compute_card_rect(tray, DISPLAY, top, 300., 64., 4.);
        assert_eq!((r.x, r.y), (1616., 44.));
    }
}
