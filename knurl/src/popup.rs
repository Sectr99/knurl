//! Dark rounded overlay that slides down from the top centre of the monitor under the cursor (fading in), holds,
//! and slides back up (macOS-style). Title, subtitle, one pip per layer. Drawn with Direct2D into a per-pixel-alpha
//! layered window, so the card, its shadow, text and pips are all anti-aliased. Never takes focus; clicks pass through.
use std::{cell::RefCell, time::{Duration, Instant}};
use windows::{core::{w, PCWSTR}, Win32::Graphics::{Direct2D::{Common::*, *}, DirectWrite::*, Dxgi::Common::*, Gdi::HDC as D2dHdc}};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::HiDpi::GetDpiForSystem,
    UI::WindowsAndMessaging::*,
};

// Sizes in px at 96 dpi; colours 0xRRGGBB.
const WIDTH: f32 = 280.0;
const RADIUS: f32 = 16.0;  // Material "large" shape
const SHADOW: f32 = 32.0;  // transparent margin around the card for its shadow
const CARD: u32 = 0x282A2F;   // Material 3 dark surface-container-high (same scheme as the settings page)
const ACCENT: u32 = 0x9ECAFF; // Material 3 dark primary
const ON_SURFACE: u32 = 0xE2E2E9;
const ON_SURFACE_VAR: u32 = 0xC3C6CF;

const SLIDE_IN: Duration = Duration::from_millis(220);
const HOLD: Duration = Duration::from_millis(1300);
const SLIDE_OUT: Duration = Duration::from_millis(180);
const TOP_GAP: f32 = 12.0; // resting distance below the top of the work area
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
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
    static FACTORIES: (ID2D1Factory, IDWriteFactory) = unsafe {
        (D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("Direct2D"),
         DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DirectWrite"))
    };
}

/// The rendered popup: a 32-bit premultiplied bitmap selected into a memory DC, card plus shadow margin.
struct Surface { dc: HDC, bmp: HBITMAP, w: i32, h: i32 }

impl Drop for Surface {
    fn drop(&mut self) { unsafe { DeleteDC(self.dc); DeleteObject(self.bmp); } }
}

pub struct Popup { hwnd: HWND, scale: f32 }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn rgba(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: (c >> 16 & 0xFF) as f32 / 255.0, g: (c >> 8 & 0xFF) as f32 / 255.0, b: (c & 0xFF) as f32 / 255.0, a }
}
fn rounded(left: f32, top: f32, right: f32, bottom: f32, r: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left, top, right, bottom }, radiusX: r, radiusY: r }
}
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
            let class = wide("KnurlPopup");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                                       class.as_ptr(), class.as_ptr(), WS_POPUP, 0, 0, 10, 10, 0, 0, inst, std::ptr::null());
            Popup { hwnd, scale: GetDpiForSystem() as f32 / 96.0 }
        }
    }

    /// `count` > 0 draws one dot per layer with `index` highlighted. While already showing, the text updates in
    /// place and the hold restarts; otherwise it slides in. The popup hides itself.
    pub fn show(&self, title: &str, subtitle: &str, index: usize, count: usize) {
        let content = Content { title: title.into(), subtitle: subtitle.into(), index, count };
        let s = self.scale;
        let Ok(surface) = render(&content, s) else { return };
        let (w, h, m) = (surface.w, surface.h, px(SHADOW, s));
        CONTENT.with(|c| *c.borrow_mut() = content);
        SURFACE.with(|c| *c.borrow_mut() = Some(surface));
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
                a.y_shown = mi.rcWork.top + px(TOP_GAP, s) - m;
                // ponytail: starts just above the monitor's top edge; a monitor stacked above would briefly show it
                a.y_hidden = mi.rcMonitor.top - h;
                a.y = a.y_hidden;
                Phase::In { t0: now, from: a.y_hidden }
            }
            Phase::Out { .. } => Phase::In { t0: now, from: a.y },  // reverse from wherever it is
            Phase::In { .. } => a.phase,                             // keep sliding in; hold starts after
            Phase::Hold { .. } => Phase::Hold { until: now + HOLD },
        };
        ANIM.with(|c| *c.borrow_mut() = a);
        present(self.hwnd, a);
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(self.hwnd, FRAME_TIMER, 10, None);   // ~64 fps in practice (system timer resolution)
        }
    }
}

fn on_frame(hwnd: HWND) {
    let a = ANIM.with(|c| { let a = step(*c.borrow(), Instant::now()); *c.borrow_mut() = a; a });
    if let Phase::Hidden = a.phase {
        unsafe { KillTimer(hwnd, FRAME_TIMER); ShowWindow(hwnd, SW_HIDE); }
    } else {
        present(hwnd, a);
    }
}

/// Puts the rendered popup on screen at the animation's position, fading with the slide.
fn present(hwnd: HWND, a: Anim) {
    let travel = (a.y - a.y_hidden) as f32 / (a.y_shown - a.y_hidden).max(1) as f32;
    let alpha = (travel / 0.6).clamp(0.0, 1.0);    // fully opaque once 60% of the way down
    SURFACE.with(|c| if let Some(sf) = c.borrow().as_ref() { unsafe {
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: (alpha * 255.0) as u8, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(hwnd, 0, &POINT { x: a.x, y: a.y }, &SIZE { cx: sf.w, cy: sf.h }, sf.dc, &POINT { x: 0, y: 0 }, 0, &blend, ULW_ALPHA);
    } });
}

fn text_format(dw: &IDWriteFactory, family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size: f32) -> windows::core::Result<IDWriteTextFormat> { unsafe {
    let f = dw.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))?;
    f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
    let ellipsis = dw.CreateEllipsisTrimmingSign(&f)?;
    f.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 }, &ellipsis)?;
    Ok(f)
} }

/// Draws the card: soft shadow, rounded fill with a hairline edge, title, subtitle and layer pips (the current one
/// a wider accent pill). Windows 11 fonts; Windows 10 falls back to Segoe UI.
fn render(c: &Content, s: f32) -> windows::core::Result<Surface> {
    let m = px(SHADOW, s) as f32;
    let (cw, ch) = (WIDTH * s, (if c.count > 0 { 96.0 } else { 76.0 }) * s);
    let (w, h) = ((cw + 2.0 * m).ceil() as i32, (ch + 2.0 * m).ceil() as i32);
    let surface = unsafe {
        let dc = CreateCompatibleDC(0);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h,
                                          biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, 0, 0);
        SelectObject(dc, bmp);
        Surface { dc, bmp, w, h }
    };
    FACTORIES.with(|(d2d, dw)| unsafe {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            ..Default::default()
        };
        let rt = d2d.CreateDCRenderTarget(&props)?;
        rt.BindDC(D2dHdc(surface.dc as _), &windows::Win32::Foundation::RECT { left: 0, top: 0, right: w, bottom: h })?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);   // ClearType can't blend onto transparency
        rt.BeginDraw();
        rt.Clear(Some(&rgba(0, 0.0)));
        let brush = rt.CreateSolidColorBrush(&rgba(0, 0.0), None)?;
        let (x0, y0, x1, y1, r) = (m, m, m + cw, m + ch, RADIUS * s);

        brush.SetColor(&rgba(0, 0.022));   // shadow: stacked, growing translucent cards, a little below
        for i in (1..=12).rev() {
            let (k, dy) = (i as f32 * 2.0 * s, 6.0 * s);
            rt.FillRoundedRectangle(&rounded(x0 - k, y0 - k + dy, x1 + k, y1 + k + dy, r + k), &brush);
        }
        brush.SetColor(&rgba(CARD, 1.0));
        rt.FillRoundedRectangle(&rounded(x0, y0, x1, y1, r), &brush);
        brush.SetColor(&rgba(0xFFFFFF, 0.09));
        rt.DrawRoundedRectangle(&rounded(x0 + 0.5, y0 + 0.5, x1 - 0.5, y1 - 0.5, r), &brush, 1.0, None);

        let line = |top: f32, height: f32| D2D_RECT_F { left: x0 + 16.0 * s, right: x1 - 16.0 * s, top: y0 + top * s, bottom: y0 + (top + height) * s };
        let title: Vec<u16> = c.title.encode_utf16().collect();
        brush.SetColor(&rgba(ON_SURFACE, 1.0));
        rt.DrawText(&title, &text_format(dw, w!("Segoe UI Variable Display"), DWRITE_FONT_WEIGHT_SEMI_BOLD, 21.0 * s)?,
                    &line(12.0, 32.0), &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        if !c.subtitle.is_empty() {
            let sub: Vec<u16> = c.subtitle.encode_utf16().collect();
            brush.SetColor(&rgba(ON_SURFACE_VAR, 1.0));
            rt.DrawText(&sub, &text_format(dw, w!("Segoe UI Variable Text"), DWRITE_FONT_WEIGHT_NORMAL, 13.0 * s)?,
                        &line(44.0, 20.0), &brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
        if c.count > 0 {
            let (dot, pill, gap) = (6.0 * s, 18.0 * s, 6.0 * s);
            let mut x = x0 + (cw - (pill + (c.count - 1) as f32 * (dot + gap))) / 2.0;
            let y = y0 + 76.0 * s;
            for i in 0..c.count {
                let current = i == c.index;
                let len = if current { pill } else { dot };
                brush.SetColor(&if current { rgba(ACCENT, 1.0) } else { rgba(ON_SURFACE_VAR, 0.35) });
                rt.FillRoundedRectangle(&rounded(x, y - dot / 2.0, x + len, y + dot / 2.0, dot / 2.0), &brush);
                x += len + gap;
            }
        }
        rt.EndDraw(None, None)
    })?;
    Ok(surface)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
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
