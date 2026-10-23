//! Follow Windows' dark app theme: dark title bar, backgrounds and control themes in the settings window,
//! and dark tray menus. Falls back to the normal light look wherever Windows doesn't offer the dark parts.
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Dwm::DwmSetWindowAttribute,
    Graphics::Gdi::*,
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
    UI::Controls::SetWindowTheme,
    UI::WindowsAndMessaging::*,
};

const BG: u32 = 0x00202020;      // COLORREF 0x00BBGGRR
const FIELD: u32 = 0x002B2B2B;
const TEXT: u32 = 0x00F0F0F0;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

thread_local! {
    static DARK: Cell<bool> = const { Cell::new(false) };
    static BRUSHES: (HBRUSH, HBRUSH) = unsafe { (CreateSolidBrush(BG), CreateSolidBrush(FIELD)) };
}

fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

/// Windows "Choose your app mode: Dark".
pub fn system_dark() -> bool {
    let mut v = 1u32;
    let mut n = 4u32;
    let key = w(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), w("AppsUseLightTheme").as_ptr(), RRF_RT_REG_DWORD,
                          std::ptr::null_mut(), (&mut v as *mut u32).cast(), &mut n) == 0 && v == 0 }
}

/// Dark context menus (tray). ponytail: uxtheme ordinals 135/136 are undocumented; skipped if absent.
pub fn allow_dark_menus() {
    unsafe {
        let ux = LoadLibraryW(w("uxtheme.dll").as_ptr());
        if ux == 0 { return; }
        if let Some(set_mode) = GetProcAddress(ux, 135 as *const u8) {
            let set_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            set_mode(1);                                   // AllowDark: follow the system setting
        }
        if let Some(flush) = GetProcAddress(ux, 136 as *const u8) {
            let flush: extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

/// Theme `win` and its controls for the current Windows setting. Call each time the window opens.
pub fn apply(win: HWND) {
    let dark = system_dark();
    DARK.with(|d| d.set(dark));
    unsafe extern "system" fn each(h: HWND, dark: LPARAM) -> BOOL {
        let mut cls = [0u16; 32];
        let n = unsafe { GetClassNameW(h, cls.as_mut_ptr(), 32) } as usize;
        let theme = match (dark != 0, &*String::from_utf16_lossy(&cls[..n])) {
            (false, _) => None,                                                // back to the default theme
            (true, "ComboBox" | "Edit") => Some(w("DarkMode_CFD")),
            (true, _) => Some(w("DarkMode_Explorer")),                         // buttons, list, scrollbars
        };
        unsafe { SetWindowTheme(h, theme.as_ref().map_or(std::ptr::null(), |t| t.as_ptr()), std::ptr::null()) };
        1
    }
    unsafe {
        let on: BOOL = dark as BOOL;
        DwmSetWindowAttribute(win, DWMWA_USE_IMMERSIVE_DARK_MODE, (&on as *const BOOL).cast(), 4);
        EnumChildWindows(win, Some(each), dark as LPARAM);
        RedrawWindow(win, std::ptr::null(), 0, RDW_ERASE | RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME);
    }
}

/// For the settings window's message handler: colours for its background and controls when dark.
pub fn handle(hwnd: HWND, msg: u32, wp: WPARAM) -> Option<LRESULT> {
    if !DARK.with(|d| d.get()) { return None; }
    let (bg, field) = BRUSHES.with(|b| *b);
    unsafe {
        match msg {
            WM_ERASEBKGND => {
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                FillRect(wp as HDC, &rc, bg);
                Some(1)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
                SetTextColor(wp as HDC, TEXT);
                SetBkColor(wp as HDC, BG);
                Some(bg as LRESULT)
            }
            WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                SetTextColor(wp as HDC, TEXT);
                SetBkColor(wp as HDC, FIELD);
                Some(field as LRESULT)
            }
            _ => None,
        }
    }
}
