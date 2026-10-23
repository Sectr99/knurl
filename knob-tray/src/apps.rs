//! Per-app layers: which program has focus, and which layer it should get.
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows_sys::Win32::{
    Foundation::{CloseHandle, BOOL, HWND, LPARAM},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
    System::Threading::{GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
    UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK},
    UI::WindowsAndMessaging::*,
};

/// "Chrome.EXE " -> "chrome"; entries may be typed with or without ".exe".
pub fn normalize(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.strip_suffix(".exe").map(str::to_string).unwrap_or(n)
}

/// "chrome, photoshop.exe" -> ["chrome", "photoshop"]
#[cfg(test)]
pub fn parse_list(s: &str) -> Vec<String> {
    s.split(',').map(normalize).filter(|a| !a.is_empty()).collect()
}

/// First layer listing `exe` wins; programs listed nowhere fall back to the layer the user picked (`manual`).
pub fn pick_layer(apps: &[Vec<String>], exe: &str, manual: usize) -> (usize, bool) {
    let exe = normalize(exe);
    match apps.iter().position(|list| list.iter().any(|a| *a == exe)) {
        Some(i) => (i, true),
        None => (manual, false),
    }
}

/// An app listed on two layers, as (app, first layer index, second layer index).
pub fn duplicate(apps: &[Vec<String>]) -> Option<(String, usize, usize)> {
    for (i, list) in apps.iter().enumerate() {
        for a in list {
            if let Some(j) = apps.iter().skip(i + 1).position(|l| l.contains(a)) { return Some((a.clone(), i, i + 1 + j)); }
        }
    }
    None
}

fn exe_of_window(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == GetCurrentProcessId() { return None; }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h == 0 { return None; }               // e.g. elevated apps when we aren't
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 { return None; }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(normalize(path.rsplit('\\').next().unwrap_or(&path)))
    }
}

/// None when our own window (settings) has focus or the program can't be read; callers keep the current layer then.
/// ponytail: Store/UWP apps all show up as "applicationframehost"
pub fn foreground_exe() -> Option<String> {
    exe_of_window(unsafe { GetForegroundWindow() })
}

/// Programs that currently have a normal top-level window, for the settings "Add running app" list.
pub fn running_apps() -> Vec<String> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let mut cloaked = 0u32;   // hidden system/UWP windows report "visible" but are cloaked
            DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4);
            if IsWindowVisible(hwnd) != 0 && cloaked == 0 && GetWindowTextLengthW(hwnd) > 0 && GetWindow(hwnd, GW_OWNER) == 0
                && ex & WS_EX_TOOLWINDOW == 0 {
                if let Some(exe) = exe_of_window(hwnd) { (*(out as *mut Vec<String>)).push(exe); }
            }
        }
        1
    }
    let mut v: Vec<String> = vec![];
    unsafe { EnumWindows(Some(each), &mut v as *mut _ as LPARAM) };
    v.sort();
    v.dedup();
    v
}

static NOTIFY_HWND: AtomicIsize = AtomicIsize::new(0);
static NOTIFY_MSG: AtomicU32 = AtomicU32::new(0);

/// Post `msg` to `hwnd` whenever the foreground window changes (callback runs on this thread's message loop).
pub fn watch_foreground(hwnd: HWND, msg: u32) {
    unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
        unsafe { PostMessageW(NOTIFY_HWND.load(Ordering::Relaxed), NOTIFY_MSG.load(Ordering::Relaxed), 0, 0) };
    }
    NOTIFY_HWND.store(hwnd, Ordering::Relaxed);
    NOTIFY_MSG.store(msg, Ordering::Relaxed);
    unsafe { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, 0, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        assert_eq!(parse_list(" Chrome.EXE, photoshop ,,"), vec!["chrome", "photoshop"]);
        let apps = vec![vec![], vec!["photoshop".to_string()], vec!["chrome".to_string(), "firefox".to_string()]];
        assert_eq!(pick_layer(&apps, "Photoshop.exe", 0), (1, true));
        assert_eq!(pick_layer(&apps, "firefox", 1), (2, true));
        assert_eq!(pick_layer(&apps, "notepad.exe", 2), (2, false));     // unlisted -> back to the manual layer
        assert_eq!(duplicate(&apps), None);
        let dup = vec![vec!["chrome".to_string()], vec![], vec!["edge".to_string(), "chrome".to_string()]];
        assert_eq!(duplicate(&dup), Some(("chrome".to_string(), 0, 2)));
    }
}
