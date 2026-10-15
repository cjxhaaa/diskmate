//! Picking a window on the desktop with the mouse, and telling which program it belongs to.
//!
//! While picking, the window under the pointer gets a frame and a label (drawn by a few small
//! click-through, always-on-top windows of our own), and a click chooses it. The click is caught by
//! a low-level mouse hook and swallowed, so it never presses anything in the window being picked.
//! Esc or a right click cancels, and so does a minute without a choice — the hooks hold the whole
//! desktop's mouse, so picking must never be able to outlive its purpose.

use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{w, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_PICK_CLICK: u32 = WM_APP + 1;
const WM_PICK_RELEASED: u32 = WM_APP + 2;
const WM_PICK_CANCEL: u32 = WM_APP + 3;
const GIVE_UP_AFTER: Duration = Duration::from_secs(60);

/// The thread running a pick, or 0. The hooks only act while it is set.
static PICK_THREAD: AtomicU32 = AtomicU32::new(0);
/// When the hooks stop swallowing input no matter what (GetTickCount64 milliseconds). A second
/// line of defence: should the picking loop ever get stuck, the mouse and Esc still work.
static HOOKS_EXPIRE_AT: AtomicU64 = AtomicU64::new(0);
/// What the label says.
static LABEL: Mutex<String> = Mutex::new(String::new());
static FONT: Mutex<isize> = Mutex::new(0);

/// The accent colour (#5b6cff) as a COLORREF, which is 0x00BBGGRR.
const ACCENT: COLORREF = COLORREF(0x00FF_6C5B);
const LABEL_BG: COLORREF = COLORREF(0x0035_211B);
const KIND_FILL: isize = 1;
const KIND_LABEL: isize = 2;

/// Whether the hooks may still act on input.
fn hooks_live() -> u32 {
    let tid = PICK_THREAD.load(Ordering::Relaxed);
    if tid != 0 && unsafe { GetTickCount64() } <= HOOKS_EXPIRE_AT.load(Ordering::Relaxed) {
        tid
    } else {
        0
    }
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    let tid = hooks_live();
    if code >= 0 && tid != 0 {
        let post = |msg: u32, l: &LPARAM| {
            let info = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
            let _ = unsafe {
                PostThreadMessageW(
                    tid,
                    msg,
                    WPARAM(info.pt.x as u32 as usize),
                    LPARAM(info.pt.y as isize),
                )
            };
        };
        match w.0 as u32 {
            WM_LBUTTONDOWN => {
                post(WM_PICK_CLICK, &l);
                return LRESULT(1);
            }
            WM_LBUTTONUP => {
                post(WM_PICK_RELEASED, &l);
                return LRESULT(1);
            }
            WM_RBUTTONDOWN => {
                post(WM_PICK_CANCEL, &l);
                return LRESULT(1);
            }
            WM_RBUTTONUP => return LRESULT(1),
            _ => {}
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    let tid = hooks_live();
    if code >= 0 && tid != 0 && matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let info = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        if info.vkCode == VK_ESCAPE.0 as u32 {
            let _ = unsafe { PostThreadMessageW(tid, WM_PICK_CANCEL, WPARAM(0), LPARAM(0)) };
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let kind = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                let brush = CreateSolidBrush(if kind == KIND_LABEL { LABEL_BG } else { ACCENT });
                FillRect(hdc, &rc, brush);
                let _ = DeleteObject(brush.into());
                if kind == KIND_LABEL {
                    let font = *FONT.lock().unwrap();
                    let old = SelectObject(hdc, HGDIOBJ(font as _));
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, COLORREF(0x00FF_FFFF));
                    let mut text: Vec<u16> = LABEL.lock().unwrap().encode_utf16().collect();
                    let pad = scale(10);
                    let mut inner = RECT {
                        left: rc.left + pad,
                        top: rc.top,
                        right: rc.right - pad,
                        bottom: rc.bottom,
                    };
                    DrawTextW(
                        hdc,
                        &mut text,
                        &mut inner,
                        DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                    SelectObject(hdc, old);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn per_monitor_dpi() {
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

fn scale(px: i32) -> i32 {
    px * unsafe { GetDpiForSystem() } as i32 / 96
}

/// The window's visible bounds: without the invisible resize borders Windows 10+ adds around it.
fn bounds(hwnd: HWND) -> RECT {
    let mut rc = RECT::default();
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rc as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();
    if !ok {
        let _ = unsafe { GetWindowRect(hwnd, &mut rc) };
    }
    rc
}

fn window_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// A Store app's window belongs to ApplicationFrameHost; the app itself owns a child window
/// inside it. Returns the app's process when there is one.
fn hosted_app_pid(frame: HWND, frame_pid: u32) -> Option<u32> {
    struct Search {
        frame_pid: u32,
        found: u32,
    }
    unsafe extern "system" fn each(child: HWND, l: LPARAM) -> BOOL {
        let s = unsafe { &mut *(l.0 as *mut Search) };
        let pid = pid_of(child);
        if pid != 0 && pid != s.frame_pid {
            s.found = pid;
            return FALSE;
        }
        TRUE
    }
    let mut s = Search {
        frame_pid,
        found: 0,
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(frame),
            Some(each),
            LPARAM(&mut s as *mut Search as isize),
        );
    }
    (s.found != 0).then_some(s.found)
}

/// The process a window really belongs to (see [`hosted_app_pid`]), and whether it was hosted.
fn owner(hwnd: HWND) -> (u32, bool) {
    let pid = pid_of(hwnd);
    let host = image_path(pid)
        .map(|p| file_name(&p).eq_ignore_ascii_case("ApplicationFrameHost.exe"))
        .unwrap_or(false);
    match host.then(|| hosted_app_pid(hwnd, pid)).flatten() {
        Some(app) => (app, true),
        None => (pid, false),
    }
}

struct Overlay {
    fill: HWND,
    edges: [HWND; 4],
    label: HWND,
}

impl Overlay {
    fn create() -> windows::core::Result<Self> {
        unsafe {
            let inst: HINSTANCE = GetModuleHandleW(None)?.into();
            let class = w!("DiskMatePickOverlay");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: inst,
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                ..Default::default()
            };
            // Registering twice fails harmlessly; the class from the first pick is reused.
            RegisterClassExW(&wc);
            let make = |kind: isize, alpha: u8| -> windows::core::Result<HWND> {
                let h = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class,
                    PCWSTR::null(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(inst),
                    None,
                )?;
                SetWindowLongPtrW(h, GWLP_USERDATA, kind);
                SetLayeredWindowAttributes(h, COLORREF(0), alpha, LWA_ALPHA)?;
                Ok(h)
            };
            Ok(Overlay {
                fill: make(KIND_FILL, 46)?,
                edges: [
                    make(KIND_FILL, 255)?,
                    make(KIND_FILL, 255)?,
                    make(KIND_FILL, 255)?,
                    make(KIND_FILL, 255)?,
                ],
                label: make(KIND_LABEL, 240)?,
            })
        }
    }

    fn is_ours(&self, h: HWND) -> bool {
        h == self.fill || h == self.label || self.edges.contains(&h)
    }

    fn place(h: HWND, x: i32, y: i32, w: i32, hgt: i32) {
        unsafe {
            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                x,
                y,
                w.max(1),
                hgt.max(1),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(h), None, true);
        }
    }

    fn show(&self, rc: RECT, text: &str) {
        let t = scale(3);
        let (x, y, w, h) = (rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top);
        Self::place(self.fill, x, y, w, h);
        Self::place(self.edges[0], x, y, w, t);
        Self::place(self.edges[1], x, y + h - t, w, t);
        Self::place(self.edges[2], x, y, t, h);
        Self::place(self.edges[3], x + w - t, y, t, h);

        *LABEL.lock().unwrap() = text.to_string();
        let (lw, lh) = self.measure(text);
        let lw = lw.min(w.max(scale(240)));
        // Above the window when there is room, otherwise just inside its top edge.
        let top_of_screens = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let ly = if y - lh - scale(6) >= top_of_screens {
            y - lh - scale(6)
        } else {
            y + scale(8)
        };
        let lx = if ly > y { x + scale(8) } else { x };
        Self::place(self.label, lx, ly, lw, lh);
    }

    fn measure(&self, text: &str) -> (i32, i32) {
        unsafe {
            let hdc = GetDC(Some(self.label));
            let font = *FONT.lock().unwrap();
            let old = SelectObject(hdc, HGDIOBJ(font as _));
            let mut buf: Vec<u16> = text.encode_utf16().collect();
            let mut rc = RECT::default();
            DrawTextW(
                hdc,
                &mut buf,
                &mut rc,
                DT_SINGLELINE | DT_CALCRECT | DT_NOPREFIX,
            );
            SelectObject(hdc, old);
            ReleaseDC(Some(self.label), hdc);
            (
                rc.right - rc.left + scale(20),
                rc.bottom - rc.top + scale(12),
            )
        }
    }

    fn hide(&self) {
        for h in [self.fill, self.label].iter().chain(self.edges.iter()) {
            unsafe {
                let _ = ShowWindow(*h, SW_HIDE);
            }
        }
    }

    fn destroy(&self) {
        for h in [self.fill, self.label].iter().chain(self.edges.iter()) {
            unsafe {
                let _ = DestroyWindow(*h);
            }
        }
    }
}

/// The top-level window under the screen point, if it is not one of ours.
fn target_at(pt: POINT, overlay: &Overlay) -> Option<HWND> {
    let h = unsafe { WindowFromPoint(pt) };
    if h.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(h, GA_ROOT) };
    let root = if root.is_invalid() { h } else { root };
    (!overlay.is_ours(root)).then_some(root)
}

fn label_for(hwnd: HWND) -> String {
    let (pid, _) = owner(hwnd);
    let exe = image_path(pid)
        .map(|p| file_name(&p))
        .unwrap_or_else(|| format!("进程 {pid}"));
    let title: String = window_text(hwnd).chars().take(40).collect();
    let what = if title.is_empty() {
        exe
    } else {
        format!("{exe} · {title}")
    };
    format!("{what}    单击选定 · Esc 取消")
}

/// Lets the user pick a window. Blocks until a window is clicked (its handle), or the pick is
/// cancelled or times out (None). Runs its own thread: hooks need a thread with a message loop.
pub fn pick() -> Result<Option<isize>, String> {
    if PICK_THREAD.load(Ordering::Relaxed) != 0 {
        return Err("已经在选择窗口了".into());
    }
    std::thread::spawn(pick_on_this_thread)
        .join()
        .map_err(|_| "选择窗口时出错".to_string())?
}

fn pick_on_this_thread() -> Result<Option<isize>, String> {
    unsafe {
        // Hook coordinates, window bounds and our frame all in physical pixels, on every monitor.
        per_monitor_dpi();
        let font = CreateFontW(
            -scale(14),
            0,
            0,
            0,
            500,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Microsoft YaHei UI"),
        );
        *FONT.lock().unwrap() = font.0 as isize;
        let overlay = Overlay::create().map_err(|e| e.message())?;
        let inst: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.message())?.into();
        HOOKS_EXPIRE_AT.store(
            GetTickCount64() + GIVE_UP_AFTER.as_millis() as u64 + 2000,
            Ordering::Relaxed,
        );
        PICK_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        let hooks = (
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(inst), 0),
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(inst), 0),
        );
        // Hover follows the pointer by polling: it keeps working over windows whose input our hook
        // may not see (an elevated Task Manager).
        let timer = SetTimer(None, 0, 30, None);
        let started = Instant::now();
        let mut current: Option<HWND> = None;
        // What is on screen now; the frame is only moved when the window does.
        let mut shown_rect = RECT::default();
        let mut shown_label = String::new();
        let mut chosen: Option<HWND> = None;
        let mut clicked_at: Option<Instant> = None;
        let mut msg = MSG::default();
        loop {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                break;
            }
            match msg.message {
                WM_TIMER => {
                    if let Some(at) = clicked_at {
                        // The button never came back up as far as the hook saw: stop waiting.
                        if at.elapsed() > Duration::from_millis(600) {
                            break;
                        }
                        continue;
                    }
                    if started.elapsed() > GIVE_UP_AFTER
                        || GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0
                    {
                        break;
                    }
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_err() {
                        continue;
                    }
                    let target = target_at(pt, &overlay);
                    if target != current {
                        current = target;
                        match target {
                            Some(h) => {
                                shown_label = label_for(h);
                                shown_rect = bounds(h);
                                overlay.show(shown_rect, &shown_label);
                            }
                            None => overlay.hide(),
                        }
                    } else if let Some(h) = target {
                        // Same window: follow it only if it moved or was resized.
                        let rc = bounds(h);
                        if rc != shown_rect {
                            shown_rect = rc;
                            overlay.show(shown_rect, &shown_label);
                        }
                    }
                }
                WM_PICK_CLICK => {
                    let pt = POINT {
                        x: msg.wParam.0 as u32 as i32,
                        y: msg.lParam.0 as i32,
                    };
                    chosen = target_at(pt, &overlay);
                    overlay.hide();
                    // Hold the hooks until the button is released, so the window under the
                    // pointer does not get a stray button-up.
                    clicked_at = Some(Instant::now());
                }
                WM_PICK_RELEASED if clicked_at.is_some() => break,
                WM_PICK_CANCEL => {
                    chosen = None;
                    break;
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        PICK_THREAD.store(0, Ordering::Relaxed);
        if let Ok(h) = hooks.0 {
            let _ = UnhookWindowsHookEx(h);
        }
        if let Ok(h) = hooks.1 {
            let _ = UnhookWindowsHookEx(h);
        }
        let _ = KillTimer(None, timer);
        overlay.destroy();
        let _ = DeleteObject(font.into());
        Ok(chosen.map(|h| h.0 as isize))
    }
}

// ---------------------------------------------------------------------------------------------
// What the picked window is
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowPick {
    pub hwnd: i64,
    pub title: String,
    pub class_name: String,
    pub pid: u32,
    /// `chrome.exe`
    pub name: String,
    pub path: Option<String>,
    pub command_line: Option<String>,
    /// Unix seconds.
    pub started_at: Option<u64>,
    /// Private bytes, as Task Manager's "Memory" column in the Details tab.
    pub memory: Option<u64>,
    pub threads: u32,
    pub parent_pid: Option<u32>,
    pub parent_name: Option<String>,
    /// Running as administrator. None when Windows will not say.
    pub elevated: Option<bool>,
    /// A Store app shown through ApplicationFrameHost; `pid` is the app's own process.
    pub hosted: bool,
    pub description: Option<String>,
    pub company: Option<String>,
    pub icon: Option<String>,
}

fn command_line(h: HANDLE) -> Option<String> {
    use windows::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows::Win32::Foundation::UNICODE_STRING;
    unsafe {
        let mut len = 0u32;
        let _ = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut len,
        );
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let status = NtQueryInformationProcess(
            h,
            ProcessCommandLineInformation,
            buf.as_mut_ptr() as *mut _,
            len,
            &mut len,
        );
        if status.is_err() {
            return None;
        }
        let us = &*(buf.as_ptr() as *const UNICODE_STRING);
        if us.Buffer.is_null() || us.Length == 0 {
            return None;
        }
        let chars = std::slice::from_raw_parts(us.Buffer.0, us.Length as usize / 2);
        Some(String::from_utf16_lossy(chars))
    }
}

fn filetime_secs(ft: FILETIME) -> Option<u64> {
    let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    (ticks > 0).then(|| (ticks / 10_000_000).saturating_sub(11_644_473_600))
}

fn elevated(h: HANDLE) -> Option<bool> {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(h, TOKEN_QUERY, &mut token).ok()?;
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let r = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut e as *mut TOKEN_ELEVATION as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        r.ok()?;
        Some(e.TokenIsElevated != 0)
    }
}

/// Everything worth showing about the picked window and its process.
pub fn describe(hwnd_raw: isize) -> WindowPick {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    let hwnd = HWND(hwnd_raw as _);
    let (pid, hosted) = owner(hwnd);
    let path = image_path(pid);
    let mut out = WindowPick {
        hwnd: hwnd_raw as i64,
        title: window_text(hwnd),
        class_name: class_name(hwnd),
        pid,
        name: path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| format!("进程 {pid}")),
        path: path.clone(),
        hosted,
        ..Default::default()
    };
    unsafe {
        if let Ok(h) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
        .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid))
        {
            out.command_line = command_line(h);
            let (mut c, mut e, mut k, mut u) = Default::default();
            if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() {
                out.started_at = filetime_secs(c);
            }
            let mut m = PROCESS_MEMORY_COUNTERS_EX {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                ..Default::default()
            };
            if GetProcessMemoryInfo(h, &mut m as *mut _ as *mut PROCESS_MEMORY_COUNTERS, m.cb)
                .is_ok()
            {
                out.memory = Some(m.PrivateUsage as u64);
            }
            out.elevated = elevated(h);
            let _ = CloseHandle(h);
        }
    }
    let procs = crate::util::process_tree();
    if let Some(me) = procs.iter().find(|p| p.pid == pid) {
        out.threads = me.threads;
        if let Some(parent) = procs.iter().find(|p| p.pid == me.parent) {
            out.parent_pid = Some(parent.pid);
            out.parent_name = Some(parent.name.clone());
        }
    }
    if let Some(p) = &path {
        if let Some(d) = crate::filemeta::details(std::slice::from_ref(p)).remove(p) {
            out.description = d.description;
            out.company = d.company;
            out.icon = d.icon;
        }
    }
    out
}

/// Brings the window to the front, restoring it if minimized.
pub fn focus(hwnd_raw: i64) -> Result<(), String> {
    let hwnd = HWND(hwnd_raw as isize as _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err("这个窗口已经关闭了".into());
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            return Err("Windows 没有允许切换到这个窗口".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod real {
    use super::*;

    fn wait_for_pick_thread() -> u32 {
        for _ in 0..100 {
            let tid = PICK_THREAD.load(Ordering::Relaxed);
            if tid != 0 {
                return tid;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("picking never started");
    }

    /// Picks the taskbar without touching the real mouse: the click the hook would report is posted
    /// to the picking thread instead. `cargo test --release real_pick -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_pick_simulated_click_and_cancel() {
        per_monitor_dpi();
        let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.unwrap();
        let rc = bounds(taskbar);
        let (x, y) = ((rc.left + rc.right) / 2, (rc.top + rc.bottom) / 2);

        // Whatever happens, this test process ends within 15 s, and its windows and hooks with it.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(15));
            eprintln!("watchdog: test stuck, exiting");
            std::process::exit(2);
        });
        let picking = std::thread::spawn(pick_on_this_thread);
        let tid = wait_for_pick_thread();
        // Long enough for several timer ticks on an unchanged window (where it used to hang).
        std::thread::sleep(Duration::from_millis(600));
        unsafe {
            PostThreadMessageW(
                tid,
                WM_PICK_CLICK,
                WPARAM(x as u32 as usize),
                LPARAM(y as isize),
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(50));
            PostThreadMessageW(tid, WM_PICK_RELEASED, WPARAM(0), LPARAM(0)).unwrap();
        }
        let picked = picking.join().unwrap().unwrap();
        assert_eq!(picked, Some(taskbar.0 as isize), "the taskbar was picked");
        assert_eq!(PICK_THREAD.load(Ordering::Relaxed), 0, "hooks released");

        let d = describe(picked.unwrap());
        println!(
            "pid {} {} | {:?} | {:?} | elevated {:?} | parent {:?} {:?} | mem {:?} | threads {}\n  cmd {:?}\n  class {} title {:?} icon {}",
            d.pid, d.name, d.path, d.description, d.elevated, d.parent_pid, d.parent_name,
            d.memory, d.threads, d.command_line, d.class_name, d.title, d.icon.is_some()
        );
        assert_eq!(d.name.to_lowercase(), "explorer.exe");
        assert!(d.path.is_some() && d.command_line.is_some() && d.started_at.is_some());

        // Esc / right click / timeout all end the same way.
        let picking = std::thread::spawn(pick_on_this_thread);
        let tid = wait_for_pick_thread();
        unsafe { PostThreadMessageW(tid, WM_PICK_CANCEL, WPARAM(0), LPARAM(0)).unwrap() };
        assert_eq!(picking.join().unwrap().unwrap(), None, "cancelled");
        assert_eq!(PICK_THREAD.load(Ordering::Relaxed), 0);
        println!("pick ok");
    }
}
