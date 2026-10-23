//! Windows look: dark mode and accent colour (for the settings page), a matching title bar for the settings
//! window, and dark tray menus. Each falls back quietly where Windows lacks it.
use windows_sys::Win32::{
    Foundation::{BOOL, HWND},
    Graphics::Dwm::DwmSetWindowAttribute,
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
};

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    let (mut v, mut n) = (0u32, 4u32);
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, w(key).as_ptr(), w(value).as_ptr(), RRF_RT_REG_DWORD,
                                   std::ptr::null_mut(), (&mut v as *mut u32).cast(), &mut n) } == 0;
    ok.then_some(v)
}

/// Windows "Choose your app mode: Dark".
pub fn system_dark() -> bool {
    reg_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") == Some(0)
}

/// The Windows accent colour as "#rrggbb" (blue if unknown).
pub fn accent() -> String {
    match reg_dword(r"Software\Microsoft\Windows\DWM", "AccentColor") {   // stored as 0xAABBGGRR
        Some(c) => format!("#{:02x}{:02x}{:02x}", c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF),
        None => "#0078d4".into(),
    }
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

/// Title bar matching dark/light.
pub fn frame(hwnd: HWND, dark: bool) {
    let on: BOOL = dark as BOOL;
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&on as *const BOOL).cast(), 4) };
}
