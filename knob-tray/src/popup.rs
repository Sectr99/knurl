//! Dark rounded overlay that slides down from the top centre of the monitor under the cursor, holds, and
//! slides back up (macOS-style). Big title, small subtitle, one dot per layer. GDI-drawn, never takes focus.
use std::{cell::RefCell, time::{Duration, Instant}};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::HiDpi::GetDpiForSystem,
    UI::WindowsAndMessaging::*,
};

const BG: u32 = 0x00202020; // COLORREF is 0x00BBGGRR
const TITLE: u32 = 0x00FFFFFF;
const SUB: u32 = 0x00A0A0A0;
const DOT_ON: u32 = 0x00FFC24C; // light accent blue
const DOT_OFF: u32 = 0x00505050;

const SLIDE_IN: Duration = Duration::from_millis(220);
const HOLD: Duration = Duration::from_millis(1300);
const SLIDE_OUT: Duration = Duration::from_millis(180);
const TOP_GAP: f32 = 12.0; // resting distance below the top of the work area, at 96 dpi
const FRAME_TIMER: usize = 1;

#[derive(Default, Clone)]
struct Content { title: String, subtitle: String, index: usize, count: usize }

#[derive(Clone, Copy)]
enum Phase { Hidden, In { t0: Instant, from: i32 }, Hold { until: Instant }, Out { t0: Instant, from: i32 } }

/// Where the popup slides between; fixed for one appearance.
#[derive(Clone, Copy)]
struct Anim { phase: Phase, x: i32, y_shown: i32, y_hidden: i32, y: i32 }

thread_local! {
    static CONTENT: RefCell<Content> = RefCell::new(Content::default());
    static ANIM: RefCell<Anim> = RefCell::new(Anim { phase: Phase::Hidden, x: 0, y_shown: 0, y_hidden: 0, y: 0 });
}

pub struct Popup { hwnd: HWND, scale: f32 }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn px(v: f32, s: f32) -> i32 { (v * s).round() as i32 }
fn lerp(a: i32, b: i32, t: f32) -> i32 { a + ((b - a) as f32 * t).round() as i32 }
fn progress(t0: Instant, d: Duration, now: Instant) -> f32 { (now.duration_since(t0).as_secs_f32() / d.as_secs_f32()).min(1.0) }

/// Advance the animation one frame; returns the new phase and y.
fn step(a: Anim, now: Instant) -> Anim {
    match a.phase {
        Phase::In { t0, from } => {
            let t = progress(t0, SLIDE_IN, now);
            let y = lerp(from, a.y_shown, 1.0 - (1.0 - t).powi(3));         // ease-out
            Anim { y, phase: if t >= 1.0 { Phase::Hold { until: now + HOLD } } else { a.phase }, ..a }
        }
        Phase::Hold { until } => if now >= until { Anim { phase: Phase::Out { t0: now, from: a.y_shown }, ..a } } else { a },
        Phase::Out { t0, from } => {
            let t = progress(t0, SLIDE_OUT, now);
            Anim { y: lerp(from, a.y_hidden, t * t * t), phase: if t >= 1.0 { Phase::Hidden } else { a.phase }, ..a } // ease-in
        }
        Phase::Hidden => a,
    }
}

impl Popup {
    pub fn new() -> Popup {
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let class = wide("RotaryKnobPopup");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst, lpszClassName: class.as_ptr(),
                                 style: CS_DROPSHADOW, hCursor: LoadCursorW(0, IDC_ARROW), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.as_ptr(), class.as_ptr(),
                                       WS_POPUP, 0, 0, 10, 10, 0, 0, inst, std::ptr::null());
            let round = DWMWCP_ROUND;   // Windows 11 rounded corners; ignored on Windows 10
            DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&round as *const i32).cast(), 4);
            Popup { hwnd, scale: GetDpiForSystem() as f32 / 96.0 }
        }
    }

    /// `count` > 0 draws one dot per layer with `index` highlighted. While already showing, the text updates in
    /// place and the hold restarts; otherwise it slides in. The popup hides itself.
    pub fn show(&self, title: &str, subtitle: &str, index: usize, count: usize) {
        CONTENT.with(|c| *c.borrow_mut() = Content { title: title.into(), subtitle: subtitle.into(), index, count });
        let s = self.scale;
        let (w, h) = (px(340.0, s), px(if count > 0 { 116.0 } else { 92.0 }, s));
        let now = Instant::now();
        let mut a = ANIM.with(|a| *a.borrow());
        a.phase = match a.phase {
            Phase::Hidden => {
                let mi = unsafe {
                    let mut pt = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut pt);
                    let mut mi: MONITORINFO = std::mem::zeroed();
                    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi);
                    mi
                };
                a.x = (mi.rcWork.left + mi.rcWork.right - w) / 2;
                a.y_shown = mi.rcWork.top + px(TOP_GAP, s);
                // ponytail: starts just above the monitor's top edge; a monitor stacked above would briefly show it
                a.y_hidden = mi.rcMonitor.top - h - px(8.0, s);
                a.y = a.y_hidden;
                Phase::In { t0: now, from: a.y_hidden }
            }
            Phase::Out { .. } => Phase::In { t0: now, from: a.y },  // reverse from wherever it is
            Phase::In { .. } => a.phase,                             // keep sliding in; hold starts after
            Phase::Hold { .. } => Phase::Hold { until: now + HOLD },
        };
        ANIM.with(|c| *c.borrow_mut() = a);
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, a.x, a.y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            InvalidateRect(self.hwnd, std::ptr::null(), 1);
            SetTimer(self.hwnd, FRAME_TIMER, 10, None);   // ~64 fps in practice (system timer resolution)
        }
    }
}

fn on_frame(hwnd: HWND) {
    let a = ANIM.with(|c| { let a = step(*c.borrow(), Instant::now()); *c.borrow_mut() = a; a });
    unsafe {
        if let Phase::Hidden = a.phase {
            KillTimer(hwnd, FRAME_TIMER);
            ShowWindow(hwnd, SW_HIDE);
        } else {
            SetWindowPos(hwnd, HWND_TOPMOST, a.x, a.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn font(height: i32, weight: i32) -> HFONT {
    unsafe { CreateFontW(-height, 0, 0, 0, weight, 0, 0, 0, 0, 0, 0, CLEARTYPE_QUALITY as u32, 0, wide("Segoe UI").as_ptr()) }
}

/// Draws one centred line in `r` and frees the font.
fn text(dc: HDC, s: &str, f: HFONT, color: u32, mut r: RECT) {
    let w: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let old = SelectObject(dc, f);
        SetTextColor(dc, color);
        DrawTextW(dc, w.as_ptr(), w.len() as i32, &mut r, DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
        SelectObject(dc, old);
        DeleteObject(f);
    }
}

fn paint(hwnd: HWND) { unsafe {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let s = GetDpiForSystem() as f32 / 96.0;
    let bg = CreateSolidBrush(BG);
    FillRect(dc, &rc, bg);
    DeleteObject(bg);
    SetBkMode(dc, TRANSPARENT as i32);
    let c = CONTENT.with(|c| c.borrow().clone());
    let line = |top: f32, height: f32| RECT { left: px(16.0, s), right: rc.right - px(16.0, s), top: px(top, s), bottom: px(top + height, s) };
    text(dc, &c.title, font(px(28.0, s), 600), TITLE, line(14.0, 40.0));
    if !c.subtitle.is_empty() { text(dc, &c.subtitle, font(px(14.0, s), 400), SUB, line(54.0, 22.0)); }
    if c.count > 0 {
        let (r, gap) = (px(4.0, s), px(16.0, s));
        let y = px(96.0, s);
        let x0 = (rc.right - gap * (c.count as i32 - 1)) / 2;
        SelectObject(dc, GetStockObject(NULL_PEN));
        for i in 0..c.count {
            let b = CreateSolidBrush(if i == c.index { DOT_ON } else { DOT_OFF });
            let old = SelectObject(dc, b);
            let x = x0 + gap * i as i32;
            Ellipse(dc, x - r, y - r, x + r + 1, y + r + 1);
            SelectObject(dc, old);
            DeleteObject(b);
        }
    }
    EndPaint(hwnd, &ps);
} }

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => { paint(hwnd); 0 }
        WM_TIMER if wp == FRAME_TIMER => { on_frame(hwnd); 0 }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_in_holds_and_slides_out() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut a = Anim { phase: Phase::In { t0, from: -120 }, x: 0, y_shown: 12, y_hidden: -120, y: -120 };
        a = step(a, ms(110));
        assert!(a.y > -54 && a.y < 12, "ease-out is past halfway at half time: {}", a.y);
        a = step(a, ms(220));
        assert_eq!(a.y, 12);
        assert!(matches!(a.phase, Phase::Hold { .. }));
        a = step(a, ms(220 + 1300));                 // hold ends
        assert!(matches!(a.phase, Phase::Out { .. }));
        let out_start = 220 + 1300;
        a = step(a, ms(out_start + 90));
        assert!(a.y < 12 && a.y > -54, "ease-in has barely moved at half time: {}", a.y);
        a = step(a, ms(out_start + 180));
        assert_eq!(a.y, -120);
        assert!(matches!(a.phase, Phase::Hidden));
    }
}
