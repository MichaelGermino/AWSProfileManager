//! Embedded web windows (wry on WebView2): the Identity Center sign-in window and the in-app AWS
//! console (docs/native-rewrite-plan.md §3.4, Phase 0 spike d).
//!
//! Each one is a plain top-level Win32 window on a thread of its own, with its own message loop,
//! hosting one wry webview. Not a GPUI window: the core asks for these from its tokio threads, a
//! sign-in window has to exist hidden and be revealed later, and the test runner's "first GPUI
//! window is the main window" stays true.
//!
//! Memory: the webview and its `WebContext` are dropped when the window is destroyed and the thread
//! then ends, which releases the WebView2 environment, so its browser processes exit a few seconds
//! later. Nothing of WebView2 stays alive after the last web window closes.

#![cfg_attr(not(feature = "test-hooks"), allow(dead_code))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use serde_json::{json, Value};

/// What to open.
pub struct Spec {
    pub kind: &'static str,
    pub url: String,
    pub title: String,
    /// WebView2 user data folder (cookies, IdP or console session).
    pub data_dir: PathBuf,
    /// A WebView2 profile inside `data_dir`: separate cookies and storage, same browser processes.
    pub profile_name: Option<String>,
    /// Logical size (scaled to the monitor's DPI).
    pub size: (i32, i32),
    /// Show straight away (the console) or start hidden until revealed (a sign-in).
    pub visible: bool,
    pub icon: Option<PathBuf>,
    /// The user closed the window (not a [`Handle::close`]). Also called if the webview could not
    /// be created, so a waiting sign-in ends instead of hanging.
    pub on_user_close: Option<Box<dyn FnOnce() + Send>>,
    /// The webview could not be created (no WebView2 runtime): the reason.
    pub on_failed: Option<Box<dyn FnOnce(String) + Send>>,
}

/// How windows are placed and shown. Test mode: on the APM_WINDOW_DISPLAY monitor, never activated,
/// and the fake services' test certificate accepted.
#[derive(Clone, Default)]
pub struct Options {
    pub test_mode: bool,
    pub display: Option<String>,
}

enum Cmd {
    Reveal,
    Close,
    /// Test control: as if the user clicked the close button.
    UserClose,
    Eval(String),
    Navigate(String),
}

struct Inner {
    kind: &'static str,
    title: String,
    tx: Mutex<mpsc::Sender<Cmd>>,
    /// 0 until the window exists, -1 once it is gone.
    hwnd: AtomicIsize,
    url: Mutex<String>,
    visible: std::sync::atomic::AtomicBool,
}

/// A running web window. Cheap to clone; every method is safe to call from any thread, before the
/// window exists and after it is gone.
#[derive(Clone)]
pub struct Handle(Arc<Inner>);

impl Handle {
    fn send(&self, cmd: Cmd) {
        let _ = self.0.tx.lock().unwrap().send(cmd);
        #[cfg(windows)]
        imp::wake(self.0.hwnd.load(Ordering::SeqCst));
    }
    pub fn reveal(&self) {
        self.send(Cmd::Reveal);
    }
    pub fn close(&self) {
        self.send(Cmd::Close);
    }
    pub fn user_close(&self) {
        self.send(Cmd::UserClose);
    }
    pub fn eval(&self, js: &str) {
        self.send(Cmd::Eval(js.to_string()));
    }
    pub fn navigate(&self, url: &str) {
        self.send(Cmd::Navigate(url.to_string()));
    }
    pub fn is_open(&self) -> bool {
        self.0.hwnd.load(Ordering::SeqCst) != -1
    }
    pub fn describe(&self) -> Value {
        json!({
            "kind": self.0.kind,
            "title": self.0.title,
            "url": *self.0.url.lock().unwrap(),
            "visible": self.0.visible.load(Ordering::SeqCst),
        })
    }
}

/// Open web windows, for the test control channel.
static OPEN: Mutex<Vec<Handle>> = Mutex::new(Vec::new());

pub fn open_windows() -> Vec<Handle> {
    let mut all = OPEN.lock().unwrap();
    all.retain(Handle::is_open);
    all.clone()
}

/// Open a web window on a thread of its own.
pub fn open(spec: Spec, options: &Options) -> Handle {
    let (tx, rx) = mpsc::channel();
    let handle = Handle(Arc::new(Inner {
        kind: spec.kind,
        title: spec.title.clone(),
        tx: Mutex::new(tx),
        hwnd: AtomicIsize::new(0),
        url: Mutex::new(spec.url.clone()),
        visible: std::sync::atomic::AtomicBool::new(false),
    }));
    {
        let mut all = OPEN.lock().unwrap();
        all.retain(Handle::is_open);
        all.push(handle.clone());
    }
    let inner = handle.0.clone();
    let options = options.clone();
    #[cfg(windows)]
    {
        let spawned = std::thread::Builder::new().name(format!("web-{}", spec.kind)).spawn(move || imp::run(spec, rx, inner, options));
        if let Err(e) = spawned {
            eprintln!("[webwin] cannot start window thread: {e}");
            handle.0.hwnd.store(-1, Ordering::SeqCst);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (rx, inner, options);
        handle.0.hwnd.store(-1, Ordering::SeqCst);
        if let Some(f) = spec.on_failed {
            f("embedded web windows are not available on this platform yet".into());
        }
        if let Some(f) = spec.on_user_close {
            f();
        }
    }
    handle
}

/// A WebView2 profile name for an id: letters, digits, '-' and '_' only, at most 64 characters.
pub fn profile_name(prefix: &str, id: &str) -> String {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let mut name = format!("{prefix}{clean}");
    name.truncate(64);
    name
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::num::NonZeroIsize;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use wry::raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
    use wry::{WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

    /// Posted to the window when a command is queued.
    const WM_APP_CMD: u32 = WM_APP + 1;
    const CLASS: &str = "APMWebWindow";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn wake(hwnd: isize) {
        if hwnd > 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_APP_CMD, 0, 0);
            }
        }
    }

    struct Hwnd(isize);

    impl HasWindowHandle for Hwnd {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let h = Win32WindowHandle::new(NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?);
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
        }
    }

    /// Per-window state, owned by the thread and reached from the window procedure through
    /// GWLP_USERDATA.
    struct State {
        webview: Option<WebView>,
        rx: mpsc::Receiver<Cmd>,
        inner: Arc<Inner>,
        on_user_close: Option<Box<dyn FnOnce() + Send>>,
        options: Options,
        /// The webview exists; commands can run.
        ready: bool,
        /// The user clicked close while the webview was still being created.
        pending_user_close: bool,
    }

    impl State {
        fn show(&mut self, hwnd: HWND) {
            unsafe {
                if self.options.test_mode {
                    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                } else {
                    ShowWindow(hwnd, SW_SHOW);
                    SetForegroundWindow(hwnd);
                }
            }
            self.inner.visible.store(true, Ordering::SeqCst);
        }

        fn user_closed(&mut self, hwnd: HWND) {
            if let Some(f) = self.on_user_close.take() {
                f();
            }
            unsafe {
                DestroyWindow(hwnd);
            }
        }

        fn drain(&mut self, hwnd: HWND) {
            if !self.ready {
                return;
            }
            if self.pending_user_close {
                self.pending_user_close = false;
                self.user_closed(hwnd);
                return;
            }
            while let Ok(cmd) = self.rx.try_recv() {
                match cmd {
                    Cmd::Reveal => {
                        if !self.inner.visible.load(Ordering::SeqCst) {
                            self.show(hwnd);
                        }
                    }
                    Cmd::Close => {
                        // Ours, not the user's: no on_user_close.
                        self.on_user_close = None;
                        unsafe {
                            DestroyWindow(hwnd);
                        }
                        return;
                    }
                    Cmd::UserClose => {
                        self.user_closed(hwnd);
                        return;
                    }
                    Cmd::Eval(js) => {
                        if let Some(w) = &self.webview {
                            let _ = w.evaluate_script(&js);
                        }
                    }
                    Cmd::Navigate(url) => {
                        if let Some(w) = &self.webview {
                            let _ = w.load_url(&url);
                        }
                    }
                }
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if !state.is_null() {
            let state = &mut *state;
            match msg {
                WM_APP_CMD => {
                    state.drain(hwnd);
                    return 0;
                }
                WM_CLOSE => {
                    if state.ready {
                        state.user_closed(hwnd);
                    } else {
                        state.pending_user_close = true;
                    }
                    return 0;
                }
                WM_DESTROY => {
                    // Drop the webview with its window (wry closes the WebView2 controller).
                    state.webview.take();
                    state.inner.visible.store(false, Ordering::SeqCst);
                    PostQuitMessage(0);
                    return 0;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn register_class() -> *const u16 {
        static CLASS_NAME: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        let name = CLASS_NAME.get_or_init(|| {
            let name = wide(CLASS);
            unsafe {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: GetModuleHandleW(std::ptr::null()),
                    hIcon: std::ptr::null_mut(),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: name.as_ptr(),
                    hIconSm: std::ptr::null_mut(),
                };
                RegisterClassExW(&wc);
            }
            name
        });
        name.as_ptr()
    }

    unsafe extern "system" fn collect_monitor(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        let list = &mut *(data as *mut Vec<(HMONITOR, i32)>);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) != 0 {
            list.push((m, mi.rcMonitor.left));
        }
        1
    }

    /// Test mode: the leftmost or rightmost monitor, like the GPUI windows (APM_WINDOW_DISPLAY).
    fn test_monitor(display: &str) -> Option<HMONITOR> {
        let mut list: Vec<(HMONITOR, i32)> = Vec::new();
        unsafe {
            EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(collect_monitor), &mut list as *mut _ as LPARAM);
        }
        list.sort_by_key(|(_, left)| *left);
        if display == "left" { list.first() } else { list.last() }.map(|(m, _)| *m)
    }

    /// Size for the monitor's DPI and centre in its work area.
    fn place(hwnd: HWND, size: (i32, i32), options: &Options) {
        unsafe {
            let monitor = match (options.test_mode, options.display.as_deref()) {
                (true, Some(d)) => test_monitor(d),
                _ => None,
            }
            .unwrap_or_else(|| MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY));
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut mi) == 0 {
                return;
            }
            let (mut dx, mut dy) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let scale = dx.max(96) as f32 / 96.0;
            let work = mi.rcWork;
            let (ww, wh) = (work.right - work.left, work.bottom - work.top);
            let w = ((size.0 as f32 * scale) as i32).min(ww);
            let h = ((size.1 as f32 * scale) as i32).min(wh);
            let x = work.left + (ww - w) / 2;
            let y = work.top + (wh - h) / 2;
            SetWindowPos(hwnd, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    fn set_icon(hwnd: HWND, path: &std::path::Path) {
        let p = wide(&path.to_string_lossy());
        unsafe {
            for (which, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
                let icon = LoadImageW(std::ptr::null_mut(), p.as_ptr(), IMAGE_ICON, size, size, LR_LOADFROMFILE);
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, which as WPARAM, icon as LPARAM);
                }
            }
        }
    }

    pub fn run(spec: Spec, rx: mpsc::Receiver<Cmd>, inner: Arc<Inner>, options: Options) {
        let Spec { url, title, data_dir, profile_name, size, visible, icon, on_user_close, on_failed, .. } = spec;
        let class = register_class();
        let title_w = wide(&title);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class,
                title_w.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                size.0,
                size.1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            inner.hwnd.store(-1, Ordering::SeqCst);
            if let Some(f) = on_failed {
                f("could not create the window".into());
            }
            if let Some(f) = on_user_close {
                f();
            }
            return;
        }
        place(hwnd, size, &options);
        if let Some(icon) = icon.filter(|p| p.exists()) {
            set_icon(hwnd, &icon);
        }
        let state = Box::into_raw(Box::new(State {
            webview: None,
            rx,
            inner: inner.clone(),
            on_user_close,
            options: options.clone(),
            ready: false,
            pending_user_close: false,
        }));
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        }
        inner.hwnd.store(hwnd as isize, Ordering::SeqCst);
        if visible {
            unsafe { (*state).show(hwnd) };
        }

        let _ = std::fs::create_dir_all(&data_dir);
        let mut context = WebContext::new(Some(data_dir));
        // wry's defaults (no mini menu, no SmartScreen), plus the fake services' certificate in
        // test mode (their CA is not in the OS store).
        let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
        if options.test_mode {
            args.push_str(" --ignore-certificate-errors");
        }
        let url_inner = inner.clone();
        let mut builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(url)
            .with_focused(visible && !options.test_mode)
            .with_additional_browser_args(args)
            .with_on_page_load_handler(move |_, u| *url_inner.url.lock().unwrap() = u);
        if let Some(p) = profile_name {
            builder = builder.with_profile_name(p);
        }
        match builder.build(&Hwnd(hwnd as isize)) {
            Ok(webview) => unsafe {
                (*state).webview = Some(webview);
            },
            Err(e) => {
                eprintln!("[webwin] cannot create the webview: {e}");
                if let Some(f) = on_failed {
                    f(e.to_string());
                }
                unsafe {
                    (*state).pending_user_close = true;
                }
            }
        }
        unsafe {
            (*state).ready = true;
            (*state).drain(hwnd);
        }
        // Commands queued while the webview was being created.
        wake(hwnd as isize);

        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            inner.hwnd.store(-1, Ordering::SeqCst);
            inner.visible.store(false, Ordering::SeqCst);
            let state = Box::from_raw(state);
            drop(state);
        }
        // Last: the environment goes with the context, and WebView2's processes with it.
        drop(context);
    }
}
