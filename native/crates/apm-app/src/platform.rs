//! OS integration that is not a window: single instance, launch at startup, shortcut icons,
//! window icons, taskbar colour and monitor geometry. Windows implementations; other platforms get
//! no-ops until the macOS port.

#![cfg_attr(not(windows), allow(unused_variables, dead_code))]

use std::path::{Path, PathBuf};

/// FNV-1a: a stable short hash (std's hasher is not stable across releases, and two builds must
/// agree on the single-instance name).
pub fn stable_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// 1.x's login item: Electron's `setLoginItemSettings` on Windows writes HKCU\...\Run under the
/// app's AppUserModelID, which for this app (no `setAppUserModelId`) is "electron.app." + the
/// product name. 2.0 writes the same value name, so updating replaces the 1.x entry.
pub const RUN_VALUE_NAME: &str = "electron.app.AWS Profile Manager";

/// What `Run` should hold for this executable.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod fallback {
    use super::*;
    pub enum Acquire {
        First,
        Second,
    }
    pub fn acquire_single_instance(_key: &str) -> Acquire {
        Acquire::First
    }
    pub fn signal_first_instance(_key: &str) -> bool {
        false
    }
    pub fn listen_for_activation(_key: &str, _on: impl Fn() + Send + 'static) {}
    pub fn set_run_entry(_name: &str, _command: Option<&str>) -> Result<bool, String> {
        Ok(false)
    }
    pub fn run_entry(_name: &str) -> Option<String> {
        None
    }
    pub fn taskbar_is_dark() -> bool {
        true
    }
    pub fn tray_icon_px() -> u32 {
        16
    }
    pub fn set_window_icon(_hwnd: isize, _ico: &Path) {}
    pub fn set_no_activate(_hwnd: isize) {}
    pub fn shortcut_dirs() -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn sync_shortcut_icons_in(_dirs: &[PathBuf], _exe: &Path, _icon: &Path) -> usize {
        0
    }
    pub struct MonitorInfo {
        pub monitor: (i32, i32, i32, i32),
        pub work: (i32, i32, i32, i32),
        pub scale: f32,
    }
    pub fn monitor_at(_x: i32, _y: i32) -> Option<MonitorInfo> {
        None
    }
}
#[cfg(not(windows))]
pub use fallback::*;

/// The shortcut icon for a design: the design's .ico copied to app data under a per-design name
/// (the shell cannot read into the install dir reliably after updates, and a new path makes its
/// icon cache miss), or the exe itself for Classic. 1.x `materializeIcon`.
pub fn shortcut_icon_for(app_data: &Path, key: &str, design_ico: Option<&Path>, exe: &Path) -> PathBuf {
    let Some(source) = design_ico else { return exe.to_path_buf() };
    let dir = app_data.join("shortcut-icons");
    let dest = dir.join(format!("{key}.ico"));
    let Ok(data) = std::fs::read(source) else { return exe.to_path_buf() };
    if std::fs::read(&dest).ok().as_deref() != Some(&data[..]) {
        let _ = std::fs::create_dir_all(&dir);
        if std::fs::write(&dest, &data).is_err() {
            return exe.to_path_buf();
        }
    }
    dest
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // ---- single instance ---------------------------------------------------------------------

    /// The first instance holds this until it exits.
    pub struct Instance(HANDLE);
    unsafe impl Send for Instance {}
    impl Drop for Instance {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub enum Acquire {
        First(Instance),
        Second,
    }

    fn class_name(key: &str) -> Vec<u16> {
        wide(&format!("AWSProfileManager2.Activate.{key}"))
    }

    /// 1.x `requestSingleInstanceLock`: a named mutex per user session (and per app-data dir, so
    /// test runs with their own sandbox do not collide with each other or with a real install).
    pub fn acquire_single_instance(key: &str) -> Acquire {
        let name = wide(&format!("Local\\AWSProfileManager2.{key}"));
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if h.is_null() {
                return Acquire::First(Instance(h));
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                return Acquire::Second;
            }
            Acquire::First(Instance(h))
        }
    }

    const WM_ACTIVATE_APP: u32 = WM_APP + 1;

    /// A second launch: ask the first instance to show its window. Retries briefly in case the
    /// first instance is still starting. `true` if it was reached.
    pub fn signal_first_instance(key: &str) -> bool {
        let class = class_name(key);
        for _ in 0..40 {
            unsafe {
                let hwnd = FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
                if !hwnd.is_null() {
                    // We are the foreground process (just launched); let the first instance take it.
                    AllowSetForegroundWindow(ASFW_ANY);
                    PostMessageW(hwnd, WM_ACTIVATE_APP, 0, 0);
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    static ON_ACTIVATE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

    unsafe extern "system" fn activate_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_ACTIVATE_APP {
            if let Some(f) = ON_ACTIVATE.get() {
                f();
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// The first instance: a message-only window on its own thread that a second launch finds.
    pub fn listen_for_activation(key: &str, on_activate: impl Fn() + Send + Sync + 'static) {
        let _ = ON_ACTIVATE.set(Box::new(on_activate));
        let class = class_name(key);
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || unsafe {
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: Some(activate_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: GetModuleHandleW(std::ptr::null()),
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class.as_ptr(),
                    hIconSm: std::ptr::null_mut(),
                };
                RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return;
                }
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .ok();
    }

    // ---- launch at startup -------------------------------------------------------------------

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const APPROVED_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";

    fn open_key(path: &str, write: bool) -> Option<HKEY> {
        let p = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        let access = if write { KEY_READ | KEY_SET_VALUE } else { KEY_READ };
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, access, &mut key) };
        (rc == 0).then_some(key)
    }

    fn read_value(key: HKEY, name: &str) -> Option<Vec<u8>> {
        let n = wide(name);
        let mut len: u32 = 0;
        unsafe {
            if RegQueryValueExW(key, n.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut len) != 0 {
                return None;
            }
            let mut buf = vec![0u8; len as usize];
            if RegQueryValueExW(key, n.as_ptr(), std::ptr::null(), std::ptr::null_mut(), buf.as_mut_ptr(), &mut len) != 0 {
                return None;
            }
            buf.truncate(len as usize);
            Some(buf)
        }
    }

    fn utf16_string(bytes: &[u8]) -> String {
        let w: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
    }

    /// The `Run` command registered under `name`, if any.
    pub fn run_entry(name: &str) -> Option<String> {
        let key = open_key(RUN_KEY, false)?;
        let v = read_value(key, name).map(|b| utf16_string(&b));
        unsafe { RegCloseKey(key) };
        v
    }

    /// Register (`Some(command)`) or remove (`None`) the login item `name`, touching the registry
    /// only when it would change (1.x). Removing also clears Task Manager's StartupApproved entry;
    /// registering re-enables an entry Task Manager had disabled (the user just asked for it).
    /// Returns whether anything changed.
    pub fn set_run_entry(name: &str, command: Option<&str>) -> Result<bool, String> {
        let current = run_entry(name);
        let key = open_key(RUN_KEY, true).ok_or("cannot open the Run key")?;
        let n = wide(name);
        let mut changed = false;
        unsafe {
            match command {
                Some(cmd) if current.as_deref() != Some(cmd) => {
                    let data = wide(cmd);
                    let rc = RegSetValueExW(key, n.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, (data.len() * 2) as u32);
                    if rc != 0 {
                        RegCloseKey(key);
                        return Err(format!("RegSetValueEx failed ({rc})"));
                    }
                    changed = true;
                }
                None if current.is_some() => {
                    RegDeleteValueW(key, n.as_ptr());
                    changed = true;
                }
                _ => {}
            }
            RegCloseKey(key);
            if let Some(approved) = open_key(APPROVED_KEY, true) {
                match command {
                    None => {
                        RegDeleteValueW(approved, n.as_ptr());
                    }
                    Some(_) => {
                        if let Some(v) = read_value(approved, name) {
                            if v.first().is_some_and(|b| b & 1 == 1) {
                                let enabled = [2u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                                RegSetValueExW(approved, n.as_ptr(), 0, REG_BINARY, enabled.as_ptr(), enabled.len() as u32);
                                changed = true;
                            }
                        }
                    }
                }
                RegCloseKey(approved);
            }
        }
        Ok(changed)
    }

    // ---- taskbar, DPI, monitors --------------------------------------------------------------

    /// The taskbar colour decides a monochrome tray glyph's colour (Electron's
    /// `shouldUseDarkColorsForSystemIntegratedUI`: Personalize\SystemUsesLightTheme).
    pub fn taskbar_is_dark() -> bool {
        let Some(key) = open_key("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", false) else { return true };
        let v = read_value(key, "SystemUsesLightTheme");
        unsafe { RegCloseKey(key) };
        match v {
            Some(b) if b.len() >= 4 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) == 0,
            _ => true,
        }
    }

    /// Tray icon size in pixels: 16 per 100% scaling.
    pub fn tray_icon_px() -> u32 {
        let dpi = unsafe { GetDpiForSystem() };
        (16 * dpi.max(96)) / 96
    }

    pub struct MonitorInfo {
        /// (left, top, right, bottom), physical pixels.
        pub monitor: (i32, i32, i32, i32),
        pub work: (i32, i32, i32, i32),
        pub scale: f32,
    }

    /// The monitor nearest a physical point.
    pub fn monitor_at(x: i32, y: i32) -> Option<MonitorInfo> {
        unsafe {
            let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            if m.is_null() {
                return None;
            }
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(m, &mut mi) == 0 {
                return None;
            }
            let (mut dx, mut dy) = (96u32, 96u32);
            GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let r = |r: windows_sys::Win32::Foundation::RECT| (r.left, r.top, r.right, r.bottom);
            Some(MonitorInfo { monitor: r(mi.rcMonitor), work: r(mi.rcWork), scale: dx.max(96) as f32 / 96. })
        }
    }

    // ---- window icons ------------------------------------------------------------------------

    /// HICONs by (path, size), loaded once and kept for the process (windows reference them).
    static ICONS: std::sync::Mutex<Vec<(PathBuf, i32, isize)>> = std::sync::Mutex::new(Vec::new());

    fn load_icon(ico: &Path, size: i32) -> Option<isize> {
        let mut cache = ICONS.lock().unwrap();
        if let Some((_, _, h)) = cache.iter().find(|(p, s, _)| p == ico && *s == size) {
            return Some(*h);
        }
        let p = wide(&ico.to_string_lossy());
        let h = unsafe { LoadImageW(std::ptr::null_mut(), p.as_ptr(), IMAGE_ICON, size, size, LR_LOADFROMFILE) };
        if h.is_null() {
            return None;
        }
        cache.push((ico.to_path_buf(), size, h as isize));
        Some(h as isize)
    }

    /// Title bar and taskbar-button icon of a window (`BrowserWindow.setIcon`).
    pub fn set_window_icon(hwnd: isize, ico: &Path) {
        let (big, small) = unsafe { (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CXSMICON)) };
        for (which, size) in [(ICON_BIG, big.max(32)), (ICON_SMALL, small.max(16))] {
            if let Some(h) = load_icon(ico, size) {
                unsafe { SendMessageW(hwnd as HWND, WM_SETICON, which as WPARAM, h as LPARAM) };
            }
        }
    }

    /// The HUD is informational: clicking it must not activate it.
    pub fn set_no_activate(hwnd: isize) {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE as isize);
        }
    }

    // ---- shortcut icons (1.x shortcutIcon.ts) ------------------------------------------------

    /// Start menu (user and machine), desktop and pinned taskbar folders.
    pub fn shortcut_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(app_data.join("Microsoft\\Windows\\Start Menu\\Programs"));
            dirs.push(app_data.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"));
        }
        let program_data = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        dirs.push(program_data.join("Microsoft\\Windows\\Start Menu\\Programs"));
        if let Some(desktop) = desktop_dir() {
            dirs.push(desktop);
        }
        dirs
    }

    fn desktop_dir() -> Option<PathBuf> {
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        unsafe {
            let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
            s.map(PathBuf::from)
        }
    }

    fn same_path(a: &str, b: &Path) -> bool {
        let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
        !a.is_empty() && norm(a) == norm(&b.to_string_lossy())
    }

    /// Point every .lnk in `dirs` that targets `exe` at `icon` (index 0), writing only shortcuts
    /// whose icon is wrong. Returns how many were rewritten. Call on a thread of its own (COM).
    pub fn sync_shortcut_icons_in(dirs: &[PathBuf], exe: &Path, icon: &Path) -> usize {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE};
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        let mut changed = 0;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            for dir in dirs {
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                        continue;
                    }
                    let result: windows::core::Result<bool> = (|| {
                        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                        let file: IPersistFile = link.cast()?;
                        file.Load(&HSTRING::from(path.as_os_str()), STGM_READWRITE)?;
                        let mut target = [0u16; 1024];
                        link.GetPath(&mut target, std::ptr::null_mut(), 0)?;
                        let target = String::from_utf16_lossy(&target[..target.iter().position(|c| *c == 0).unwrap_or(0)]);
                        if !same_path(&target, exe) {
                            return Ok(false);
                        }
                        let mut current = [0u16; 1024];
                        let mut index = 0i32;
                        link.GetIconLocation(&mut current, &mut index)?;
                        let current = String::from_utf16_lossy(&current[..current.iter().position(|c| *c == 0).unwrap_or(0)]);
                        // An empty icon means "the target's own icon".
                        let shown = if current.is_empty() { target.clone() } else { current };
                        if same_path(&shown, icon) && index == 0 {
                            return Ok(false);
                        }
                        link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;
                        file.Save(None, true)?;
                        Ok(true)
                    })();
                    if result.unwrap_or(false) {
                        changed += 1;
                    }
                }
            }
        }
        changed
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Only a test-only value name is ever written, and it is removed again.
        #[test]
        fn run_entry_round_trip() {
            let name = format!("AWSProfileManager2-test-{}", std::process::id());
            let cmd = "\"C:\\nowhere\\apm-test.exe\"";
            assert_eq!(run_entry(&name), None);
            assert_eq!(set_run_entry(&name, Some(cmd)), Ok(true));
            assert_eq!(run_entry(&name).as_deref(), Some(cmd));
            // Unchanged: no write.
            assert_eq!(set_run_entry(&name, Some(cmd)), Ok(false));
            assert_eq!(set_run_entry(&name, None), Ok(true));
            assert_eq!(run_entry(&name), None);
            assert_eq!(set_run_entry(&name, None), Ok(false));
        }

        #[test]
        fn single_instance_mutex() {
            let key = format!("test-{}", std::process::id());
            let first = acquire_single_instance(&key);
            assert!(matches!(first, Acquire::First(_)));
            assert!(matches!(acquire_single_instance(&key), Acquire::Second));
            drop(first);
            assert!(matches!(acquire_single_instance(&key), Acquire::First(_)));
        }

        #[test]
        fn shortcut_icons_repointed() {
            use windows::core::{Interface, HSTRING};
            use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ};
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
            let dir = std::env::temp_dir().join(format!("apm-lnk-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let exe = std::env::current_exe().unwrap();
            let other = std::path::PathBuf::from("C:\\Windows\\notepad.exe");
            let icon = dir.join("design.ico");
            std::fs::write(&icon, b"x").unwrap();
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                for (name, target) in [("ours.lnk", &exe), ("other.lnk", &other)] {
                    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                    link.SetPath(&HSTRING::from(target.as_os_str())).unwrap();
                    let file: IPersistFile = link.cast().unwrap();
                    file.Save(&HSTRING::from(dir.join(name).as_os_str()), true).unwrap();
                }
            }
            assert_eq!(sync_shortcut_icons_in(&[dir.clone()], &exe, &icon), 1);
            // Already right: nothing to write.
            assert_eq!(sync_shortcut_icons_in(&[dir.clone()], &exe, &icon), 0);
            unsafe {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                let file: IPersistFile = link.cast().unwrap();
                file.Load(&HSTRING::from(dir.join("ours.lnk").as_os_str()), STGM_READ).unwrap();
                let mut buf = [0u16; 1024];
                let mut idx = 0;
                link.GetIconLocation(&mut buf, &mut idx).unwrap();
                let s = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap()]);
                assert!(same_path(&s, &icon));
            }
            // Back to Classic: the exe's own icon.
            assert_eq!(sync_shortcut_icons_in(&[dir.clone()], &exe, &exe), 1);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_case_insensitive() {
        assert_eq!(stable_hash("C:\\Users\\A\\AppData"), stable_hash("c:\\users\\a\\appdata"));
        assert_eq!(stable_hash(""), "cbf29ce484222325");
    }

    #[test]
    fn run_command_is_quoted() {
        assert_eq!(run_command(Path::new("C:\\Program Files\\APM\\app.exe")), "\"C:\\Program Files\\APM\\app.exe\"");
    }
}
