//! Phase 0 spike (a): GPUI window with gpui-component title bar, tray icon via tray-icon,
//! destroy-on-hide (closing the window keeps the process alive in the tray).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui::{div, prelude::*, px, size, App, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions};
use gpui_component::{Root, TitleBar};
use std::sync::mpsc;
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

enum TrayCmd {
    Open,
    Quit,
}

struct Shell {
    opened: SharedString,
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(0x18181b))
            .text_color(gpui::rgb(0xe4e4e7))
            .child(TitleBar::new().child(div().pl_3().child("AWS Profile Manager — GPUI spike")))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(div().w(px(240.)).h_full().bg(gpui::rgb(0x111113)).p_4().child("Profiles"))
                    .child(div().flex_1().p_6().child(self.opened.clone())),
            )
    }
}

fn open_main(cx: &mut App) {
    if let Some(handle) = cx.windows().first().cloned() {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
    let opened: SharedString = format!("Window opened at {:?}", std::time::SystemTime::now()).into();
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..TitleBar::window_options()
        },
        |window, cx| {
            let view = cx.new(|_| Shell { opened });
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
    cx.activate(true);
}

fn tray_icon_image() -> tray_icon::Icon {
    // Plain 32x32 square; the real app loads resources/icon.ico.
    let rgba: Vec<u8> = (0..32 * 32).flat_map(|_| [0xf5, 0x9e, 0x0b, 0xff]).collect();
    tray_icon::Icon::from_rgba(rgba, 32, 32).expect("icon")
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32EmptyWorkingSet(process: isize) -> i32;
}

/// After the last window closes, hand untouched pages back to the OS.
fn trim_working_set() {
    #[cfg(windows)]
    unsafe {
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

fn main() {
    let app = gpui_platform::application().with_quit_mode(gpui::QuitMode::Explicit);
    app.run(|cx: &mut App| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.spawn(async move |cx| {
                    // Let GPUI finish tearing the window down first.
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    trim_working_set();
                })
                .detach();
            }
        })
        .detach();

        let (tx, rx) = mpsc::channel::<TrayCmd>();
        let menu = Menu::new();
        let open_item = MenuItem::new("Open App", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        menu.append_items(&[&open_item, &quit_item]).expect("menu");
        let (open_id, quit_id) = (open_item.id().clone(), quit_item.id().clone());
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("AWS Profile Manager spike")
            .with_icon(tray_icon_image())
            .build()
            .expect("tray");
        // The tray must outlive the closure; leak it for the spike.
        Box::leak(Box::new(tray));

        let tx_menu = tx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let cmd = if e.id == open_id { Some(TrayCmd::Open) } else if e.id == quit_id { Some(TrayCmd::Quit) } else { None };
            if let Some(c) = cmd {
                let _ = tx_menu.send(c);
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                let _ = tx.send(TrayCmd::Open);
            }
        }));

        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            while let Ok(cmd) = rx.try_recv() {
                let _ = cx.update(|cx| match cmd {
                    TrayCmd::Open => open_main(cx),
                    TrayCmd::Quit => cx.quit(),
                });
            }
        })
        .detach();

        open_main(cx);
    });
}
