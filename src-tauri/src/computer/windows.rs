//! Windows driver: Win32 for windows and input, UI Automation for elements,
//! xcap for display capture. Coordinates are physical pixels (the app is
//! per-monitor DPI aware).

use std::mem::{size_of, ManuallyDrop};
use std::thread::sleep;
use std::time::Duration;

use image::RgbaImage;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, VARIANT_TRUE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_I4};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetAncestor, GetForegroundWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SetCursorPos, SetForegroundWindow, ShowWindow, WindowFromPoint, GA_ROOT, GWL_EXSTYLE, SW_RESTORE, WS_EX_TOOLWINDOW,
};

use super::{Display, Driver, FieldInfo, Key, MouseButton, Rect, UiElement, WindowInfo};

pub struct WindowsDriver;

impl WindowsDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsDriver {
    fn default() -> Self {
        Self::new()
    }
}

fn rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

fn hwnd(id: u64) -> HWND {
    HWND(id as usize as *mut core::ffi::c_void)
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
    }
}

fn window_info(h: HWND) -> Option<WindowInfo> {
    unsafe {
        let len = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; len.max(0) as usize + 1];
        let n = GetWindowTextW(h, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let mut r = RECT::default();
        let _ = DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, size_of::<RECT>() as u32);
        Some(WindowInfo { id: h.0 as usize as u64, title, process: process_name(pid), pid, rect: rect(r), minimized: IsIconic(h).as_bool() })
    }
}

/// A window a user would recognise as an application window.
fn is_app_window(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
            return false;
        }
        if GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, size_of::<u32>() as u32);
        if cloaked != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        pid != GetCurrentProcessId() // never IGRIS's own windows
    }
}

unsafe extern "system" fn collect_window(h: HWND, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<HWND>);
    if is_app_window(h) {
        list.push(h);
    }
    BOOL(1)
}

unsafe extern "system" fn collect_monitor(m: HMONITOR, _hdc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<Display>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(m, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let name_len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        list.push(Display {
            id: list.len() as u32 + 1,
            name: String::from_utf16_lossy(&info.szDevice[..name_len]).trim_start_matches("\\\\.\\").to_string(),
            rect: rect(info.monitorInfo.rcMonitor),
            primary: info.monitorInfo.dwFlags & 1 != 0,
        });
    }
    BOOL(1)
}

// --- input -----------------------------------------------------------------

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // UIPI blocks input to windows of elevated (administrator) apps.
        return Err("Windows blocked the input (the target app may be running as administrator).".into());
    }
    Ok(())
}

fn mouse(flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

fn vk(k: Key) -> Result<(VIRTUAL_KEY, bool), String> {
    // (virtual key, extended)
    Ok(match k {
        Key::Ctrl => (VK_CONTROL, false),
        Key::Shift => (VK_SHIFT, false),
        Key::Alt => (VK_MENU, false),
        Key::Win => (VK_LWIN, true),
        Key::Enter => (VK_RETURN, false),
        Key::Tab => (VK_TAB, false),
        Key::Escape => (VK_ESCAPE, false),
        Key::Backspace => (VK_BACK, false),
        Key::Delete => (VK_DELETE, true),
        Key::Insert => (VK_INSERT, true),
        Key::Space => (VK_SPACE, false),
        Key::Up => (VK_UP, true),
        Key::Down => (VK_DOWN, true),
        Key::Left => (VK_LEFT, true),
        Key::Right => (VK_RIGHT, true),
        Key::Home => (VK_HOME, true),
        Key::End => (VK_END, true),
        Key::PageUp => (VK_PRIOR, true),
        Key::PageDown => (VK_NEXT, true),
        Key::F(n) => (VIRTUAL_KEY(VK_F1.0 + (n as u16 - 1)), false),
        Key::Char(c) if c.is_ascii_alphanumeric() => (VIRTUAL_KEY(c.to_ascii_uppercase() as u16), false),
        Key::Char(c) => {
            let code = match c {
                ',' => VK_OEM_COMMA,
                '.' => VK_OEM_PERIOD,
                '/' => VK_OEM_2,
                ';' => VK_OEM_1,
                '\'' => VK_OEM_7,
                '[' => VK_OEM_4,
                ']' => VK_OEM_6,
                '\\' => VK_OEM_5,
                '-' => VK_OEM_MINUS,
                '=' => VK_OEM_PLUS,
                '`' => VK_OEM_3,
                _ => return Err(format!("Unsupported key '{c}'.")),
            };
            (code, false)
        }
    })
}

fn key_flags(extended: bool, up: bool) -> KEYBD_EVENT_FLAGS {
    let mut f = KEYBD_EVENT_FLAGS(0);
    if extended {
        f |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        f |= KEYEVENTF_KEYUP;
    }
    f
}

// --- UI Automation -----------------------------------------------------------

fn uia() -> Result<IUIAutomation, String> {
    unsafe {
        // Each blocking worker thread joins the multithreaded apartment once.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("UI Automation is unavailable: {e}"))
    }
}

fn variant_i4(v: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 { vt: VT_I4, wReserved1: 0, wReserved2: 0, wReserved3: 0, Anonymous: VARIANT_0_0_0 { lVal: v } }),
        },
    }
}

fn variant_bool(v: bool) -> VARIANT {
    let b = if v { VARIANT_TRUE } else { windows::Win32::Foundation::VARIANT_FALSE };
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 { vt: VT_BOOL, wReserved1: 0, wReserved2: 0, wReserved3: 0, Anonymous: VARIANT_0_0_0 { boolVal: b } }),
        },
    }
}

const ROLES: &[(UIA_CONTROLTYPE_ID, &str)] = &[
    (UIA_ButtonControlTypeId, "button"),
    (UIA_SplitButtonControlTypeId, "split button"),
    (UIA_EditControlTypeId, "edit"),
    (UIA_DocumentControlTypeId, "document"),
    (UIA_HyperlinkControlTypeId, "hyperlink"),
    (UIA_MenuItemControlTypeId, "menu item"),
    (UIA_ListItemControlTypeId, "list item"),
    (UIA_TabItemControlTypeId, "tab"),
    (UIA_CheckBoxControlTypeId, "checkbox"),
    (UIA_RadioButtonControlTypeId, "radio button"),
    (UIA_ComboBoxControlTypeId, "combo box"),
    (UIA_TreeItemControlTypeId, "tree item"),
    (UIA_DataItemControlTypeId, "data item"),
    (UIA_MenuBarControlTypeId, "menu bar"),
    (UIA_TextControlTypeId, "text"),
    (UIA_ImageControlTypeId, "image"),
    (UIA_GroupControlTypeId, "group"),
    (UIA_WindowControlTypeId, "window"),
    (UIA_PaneControlTypeId, "pane"),
    (UIA_ToolBarControlTypeId, "toolbar"),
    (UIA_CustomControlTypeId, "custom"),
];

fn role(t: UIA_CONTROLTYPE_ID) -> &'static str {
    ROLES.iter().find(|(id, _)| *id == t).map(|(_, r)| *r).unwrap_or("other")
}

const INTERACTIVE: &[UIA_CONTROLTYPE_ID] = &[
    UIA_ButtonControlTypeId,
    UIA_SplitButtonControlTypeId,
    UIA_EditControlTypeId,
    UIA_DocumentControlTypeId,
    UIA_HyperlinkControlTypeId,
    UIA_MenuItemControlTypeId,
    UIA_ListItemControlTypeId,
    UIA_TabItemControlTypeId,
    UIA_CheckBoxControlTypeId,
    UIA_RadioButtonControlTypeId,
    UIA_ComboBoxControlTypeId,
    UIA_TreeItemControlTypeId,
    UIA_DataItemControlTypeId,
];

fn element(e: &IUIAutomationElement, cached: bool) -> Option<UiElement> {
    unsafe {
        let (name, ty, r, enabled, focused) = if cached {
            (e.CachedName().ok()?, e.CachedControlType().ok()?, e.CachedBoundingRectangle().ok()?, e.CachedIsEnabled().ok()?, e.CachedHasKeyboardFocus().ok()?)
        } else {
            (
                e.CurrentName().ok()?,
                e.CurrentControlType().ok()?,
                e.CurrentBoundingRectangle().ok()?,
                e.CurrentIsEnabled().ok()?,
                e.CurrentHasKeyboardFocus().ok()?,
            )
        };
        Some(UiElement {
            name: name.to_string().chars().take(120).collect(),
            role: role(ty).to_string(),
            rect: rect(r),
            enabled: enabled.as_bool(),
            focused: focused.as_bool(),
        })
    }
}

impl Driver for WindowsDriver {
    fn displays(&self) -> Result<Vec<Display>, String> {
        let mut list: Vec<Display> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut list as *mut Vec<Display> as isize));
        }
        if list.is_empty() {
            return Err("No display found.".into());
        }
        Ok(list)
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        let mut handles: Vec<HWND> = Vec::new();
        unsafe { EnumWindows(Some(collect_window), LPARAM(&mut handles as *mut Vec<HWND> as isize)) }.map_err(|e| format!("Couldn't list windows: {e}"))?;
        Ok(handles.into_iter().filter_map(window_info).collect())
    }

    fn foreground(&self) -> Result<Option<WindowInfo>, String> {
        let h = unsafe { GetForegroundWindow() };
        if h.0.is_null() {
            return Ok(None);
        }
        Ok(window_info(h))
    }

    fn focus_window(&self, id: u64) -> Result<(), String> {
        let h = hwnd(id);
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return Err("That window no longer exists.".into());
            }
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            // Windows only lets the foreground process change the foreground
            // window; briefly attaching to its input queue is the documented workaround.
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
            let _ = BringWindowToTop(h);
            let _ = SetForegroundWindow(h);
            if attached {
                let _ = AttachThreadInput(me, fg_thread, false);
            }
            for _ in 0..10 {
                if GetForegroundWindow() == h {
                    return Ok(());
                }
                sleep(Duration::from_millis(50));
            }
        }
        Err("Windows didn't bring that window to the front.".into())
    }

    fn capture(&self, display: &Display) -> Result<RgbaImage, String> {
        let (cx, cy) = display.rect.center();
        let m = xcap::Monitor::from_point(cx, cy).map_err(|e| format!("Couldn't find that display: {e}"))?;
        let img = m.capture_image().map_err(|e| format!("Couldn't capture the screen: {e}"))?;
        RgbaImage::from_raw(img.width(), img.height(), img.into_raw()).ok_or_else(|| "The captured image was malformed.".to_string())
    }

    fn ui_elements(&self, window: u64, max: usize) -> Result<Vec<UiElement>, String> {
        let auto = uia()?;
        unsafe {
            let root = auto.ElementFromHandle(hwnd(window)).map_err(|e| format!("Couldn't read that window's controls: {e}"))?;
            let cache = auto.CreateCacheRequest().map_err(|e| e.to_string())?;
            for p in [UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_BoundingRectanglePropertyId, UIA_IsEnabledPropertyId, UIA_HasKeyboardFocusPropertyId] {
                cache.AddProperty(p).map_err(|e| e.to_string())?;
            }
            let mut types: Option<IUIAutomationCondition> = None;
            for t in INTERACTIVE {
                let c = auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &variant_i4(t.0)).map_err(|e| e.to_string())?;
                types = Some(match types {
                    None => c,
                    Some(prev) => auto.CreateOrCondition(&prev, &c).map_err(|e| e.to_string())?,
                });
            }
            let onscreen = auto.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &variant_bool(false)).map_err(|e| e.to_string())?;
            let types = types.ok_or("no control types")?;
            let cond = auto.CreateAndCondition(&types, &onscreen).map_err(|e| e.to_string())?;
            let found = root.FindAllBuildCache(TreeScope_Descendants, &cond, &cache).map_err(|e| format!("Couldn't read that window's controls: {e}"))?;
            let n = found.Length().unwrap_or(0);
            let mut out = Vec::new();
            for i in 0..n {
                if let Some(el) = found.GetElement(i).ok().and_then(|e| element(&e, true)) {
                    if !el.rect.is_empty() && (!el.name.trim().is_empty() || matches!(el.role.as_str(), "edit" | "document" | "combo box")) {
                        out.push(el);
                    }
                }
            }
            // Selection (fields first, top-to-bottom) happens in `select_elements`.
            out.truncate(max);
            Ok(out)
        }
    }

    fn element_at(&self, x: i32, y: i32) -> Result<Option<UiElement>, String> {
        let auto = uia()?;
        unsafe { Ok(auto.ElementFromPoint(POINT { x, y }).ok().and_then(|e| element(&e, false))) }
    }

    fn focused_field(&self) -> Option<FieldInfo> {
        let auto = uia().ok()?;
        unsafe {
            let e = auto.GetFocusedElement().ok()?;
            let password = e.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
            let pattern = e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok();
            let read_only = pattern.as_ref().and_then(|p| p.CurrentIsReadOnly().ok()).map(|b| b.as_bool());
            let value = if password { None } else { pattern.as_ref().and_then(|p| p.CurrentValue().ok()).map(|v| v.to_string().chars().take(2000).collect()) };
            Some(FieldInfo { value, read_only, password })
        }
    }

    fn focused_element(&self) -> Result<Option<UiElement>, String> {
        let auto = uia()?;
        unsafe { Ok(auto.GetFocusedElement().ok().and_then(|e| element(&e, false))) }
    }

    fn click(&self, x: i32, y: i32, button: MouseButton, count: u32) -> Result<(), String> {
        self.move_mouse(x, y)?;
        let (down, up) = match button {
            MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        };
        for i in 0..count.clamp(1, 3) {
            if i > 0 {
                sleep(Duration::from_millis(60));
            }
            send(&[mouse(down, 0), mouse(up, 0)])?;
        }
        Ok(())
    }

    fn move_mouse(&self, x: i32, y: i32) -> Result<(), String> {
        unsafe { SetCursorPos(x, y) }.map_err(|e| format!("Couldn't move the mouse: {e}"))?;
        sleep(Duration::from_millis(30));
        Ok(())
    }

    fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> Result<(), String> {
        self.move_mouse(x, y)?;
        if dy != 0 {
            // Positive = away from the user (up); callers pass "down" as positive clicks.
            send(&[mouse(MOUSEEVENTF_WHEEL, -dy * 120)])?;
        }
        if dx != 0 {
            send(&[mouse(MOUSEEVENTF_HWHEEL, dx * 120)])?;
        }
        Ok(())
    }

    fn drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<(), String> {
        self.move_mouse(from.0, from.1)?;
        send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0)])?;
        let steps = 12;
        for i in 1..=steps {
            let x = from.0 + (to.0 - from.0) * i / steps;
            let y = from.1 + (to.1 - from.1) * i / steps;
            if let Err(e) = self.move_mouse(x, y) {
                let _ = send(&[mouse(MOUSEEVENTF_LEFTUP, 0)]);
                return Err(e);
            }
        }
        send(&[mouse(MOUSEEVENTF_LEFTUP, 0)])
    }

    fn type_text(&self, text: &str, stop: &dyn Fn() -> bool) -> Result<(), String> {
        let units: Vec<u16> = text.replace("\r\n", "\n").encode_utf16().collect();
        for chunk in units.chunks(16) {
            if stop() {
                return Err("Stopped by the user.".into());
            }
            let mut inputs = Vec::with_capacity(chunk.len() * 2);
            for &u in chunk {
                match u {
                    0x0A => {
                        inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                        inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                    }
                    0x09 => {
                        inputs.push(key(VK_TAB, 0, KEYBD_EVENT_FLAGS(0)));
                        inputs.push(key(VK_TAB, 0, KEYEVENTF_KEYUP));
                    }
                    _ => {
                        inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE));
                        inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            }
            send(&inputs)?;
            sleep(Duration::from_millis(12));
        }
        Ok(())
    }

    fn press(&self, keys: &[Key]) -> Result<(), String> {
        let codes = keys.iter().map(|k| vk(*k)).collect::<Result<Vec<_>, _>>()?;
        let mut inputs: Vec<INPUT> = codes.iter().map(|(v, ext)| key(*v, 0, key_flags(*ext, false))).collect();
        inputs.extend(codes.iter().rev().map(|(v, ext)| key(*v, 0, key_flags(*ext, true))));
        send(&inputs)
    }

    fn owner_at(&self, x: i32, y: i32) -> Option<u32> {
        unsafe {
            let h = WindowFromPoint(POINT { x, y });
            if h.0.is_null() {
                return None;
            }
            let root = GetAncestor(h, GA_ROOT);
            let mut pid = 0u32;
            GetWindowThreadProcessId(if root.0.is_null() { h } else { root }, Some(&mut pid));
            (pid != 0).then_some(pid)
        }
    }

    fn idle_ms(&self) -> Option<u64> {
        unsafe {
            let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            if !GetLastInputInfo(&mut info).as_bool() {
                return None;
            }
            Some(GetTickCount().wrapping_sub(info.dwTime) as u64)
        }
    }
}
