//! Phase 0 spike (d): embedded sign-in page = wry webview inside a GPUI window, with a per-org
//! persistent data directory (the 1.x `persist:sso-<hash>` partition equivalent).
//! Env: SPIKE_URL (default https://example.com), SPIKE_DATA_DIR (default %TEMP%\apm-sso-spike),
//! SPIKE_CLOSE_AFTER_MS closes the window after that delay (to measure teardown).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui::{div, prelude::*, px, size, App, Bounds, Context, Window, WindowBounds, WindowOptions};
use std::time::Duration;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebContext, WebView, WebViewBuilder};

struct SignIn {
    _webview: WebView,
    _context: WebContext,
}

impl Render for SignIn {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn main() {
    let url = std::env::var("SPIKE_URL").unwrap_or_else(|_| "https://example.com".into());
    let data_dir = std::env::var("SPIKE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("apm-sso-spike"));
    let close_after = std::env::var("SPIKE_CLOSE_AFTER_MS").ok().and_then(|v| v.parse::<u64>().ok());

    gpui_platform::application()
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
            let win = cx
                .open_window(
                    WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
                    |window, cx| {
                        let mut context = WebContext::new(Some(data_dir.clone()));
                        let vp = window.viewport_size();
                        let webview = WebViewBuilder::new_with_web_context(&mut context)
                            .with_url(url.clone())
                            .with_bounds(Rect {
                                position: LogicalPosition::new(0, 0).into(),
                                size: LogicalSize::new(f32::from(vp.width), f32::from(vp.height)).into(),
                            })
                            // The real app watches for the loopback redirect here; the spike just logs.
                            .with_navigation_handler(|u| {
                                println!("navigate: {}", u.split('?').next().unwrap_or(""));
                                true
                            })
                            .build_as_child(window)
                            .expect("webview");
                        cx.new(|_| SignIn { _webview: webview, _context: context })
                    },
                )
                .expect("window");
            cx.activate(true);
            cx.on_window_closed(|_, _| println!("window closed; webview dropped")).detach();
            if let Some(ms) = close_after {
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(ms)).await;
                    let _ = win.update(cx, |_, window, _| window.remove_window());
                    cx.background_executor().timer(Duration::from_millis(8000)).await;
                    let _ = cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
}
