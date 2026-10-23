//! Settings window: a native window hosting WebView2, showing ui/settings.html. The page draws everything;
//! Rust keeps the logic. The web view exists only while the window is open, so it costs nothing when closed.
//!
//! Page -> Rust: {"id": n, "cmd": "...", ...}. Messages are queued and handled from the app's own message loop
//! (not inside WebView2's callback, where file dialogs and other nested loops can deadlock).
//! Rust -> page: knob.reply(id, data) and knob.event(name, data).
use crate::{apps, config::{self, Config, PAIRS}, names, theme, App, WM_RECORDED, WM_WEB};
use native_windows_gui as nwg;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque, num::NonZeroIsize, rc::Rc, sync::atomic::{AtomicIsize, Ordering}};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::Input::KeyboardAndMouse::GetAsyncKeyState,
    UI::WindowsAndMessaging::*,
};
use wry::WebViewBuilderExtWindows;

const PAGE: &str = include_str!("../ui/settings.html");

#[derive(Default)]
pub struct Host { pub win: nwg::Window }

pub fn build(h: &mut Host, icon: &nwg::Icon) -> Result<(), nwg::NwgError> {
    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::RESIZABLE | nwg::WindowFlags::MAXIMIZE_BOX)
        .size((1080, 760)).center(true).icon(Some(icon)).title("Rotary knob \u{2014} settings").build(&mut h.win)
}

struct Hwnd(isize);
impl HasWindowHandle for Hwnd {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// The web view and its inbox; lives in App.
#[derive(Default)]
pub struct Web { view: RefCell<Option<wry::WebView>>, inbox: Rc<RefCell<VecDeque<String>>> }

// --- key recorder: a low-level keyboard hook swallows the next non-modifier key and reports it ---

static HOOK: AtomicIsize = AtomicIsize::new(0);
static NOTIFY: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn record_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && (wp == WM_KEYDOWN as usize || wp == WM_SYSKEYDOWN as usize) {
            let vk = (*(lp as *const KBDLLHOOKSTRUCT)).vkCode as u8;
            if !matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C) {   // wait past bare modifiers
                let down = |v: i32| GetAsyncKeyState(v) as u16 & 0x8000 != 0;
                let mods = down(0x11) as u32 | (down(0x10) as u32) << 1 | (down(0x12) as u32) << 2 | ((down(0x5B) || down(0x5C)) as u32) << 3;
                PostMessageW(NOTIFY.load(Ordering::Relaxed), WM_RECORDED, vk as usize, mods as isize);
                return 1;
            }
        }
        CallNextHookEx(0, code, wp, lp)
    }
}

fn stop_hook() {
    let h = HOOK.swap(0, Ordering::Relaxed);
    if h != 0 { unsafe { UnhookWindowsHookEx(h) }; }
}

impl App {
    pub fn open_settings(&self) {
        let win = &self.ui.web.win;
        let hwnd = App::hwnd(&win.handle);
        let dark = theme::system_dark();
        theme::frame(hwnd, dark);
        if self.web.view.borrow().is_none() {
            let inbox = self.web.inbox.clone();
            let notify = self.msg_hwnd();
            let built = wry::WebViewBuilder::new()
                .with_html(PAGE)
                .with_background_color(if dark { (32, 32, 32, 255) } else { (243, 243, 243, 255) })   // page colour: no white flash
                .with_theme(if dark { wry::Theme::Dark } else { wry::Theme::Light })
                .with_default_context_menus(false)
                .with_hotkeys_zoom(false)
                .with_devtools(cfg!(debug_assertions))
                .with_ipc_handler(move |req| {
                    inbox.borrow_mut().push_back(req.into_body());
                    unsafe { PostMessageW(notify, WM_WEB, 0, 0) };
                })
                .build(&Hwnd(hwnd));
            match built {
                Ok(v) => *self.web.view.borrow_mut() = Some(v),
                Err(e) => {
                    nwg::modal_error_message(&self.ui.msg, "Rotary knob", &format!("Couldn't open settings (WebView2): {e}"));
                    return;
                }
            }
        }
        win.set_visible(true);
        win.set_focus();
        if let Some(v) = self.web.view.borrow().as_ref() { let _ = v.focus(); }
    }

    /// Window closed: free the web view entirely.
    pub fn close_settings(&self) {
        stop_hook();
        self.state.borrow_mut().recording = None;
        self.web.inbox.borrow_mut().clear();
        self.web.view.borrow_mut().take();
        self.ui.web.win.set_visible(false);
    }

    fn send(&self, js: String) {
        if let Some(v) = self.web.view.borrow().as_ref() { let _ = v.evaluate_script(&js); }
    }
    fn reply(&self, id: &Value, data: Value) { self.send(format!("knob.reply({id},{data})")); }
    fn event(&self, name: &str, data: Value) { self.send(format!("knob.event({},{data})", json!(name))); }

    /// Drain messages from the page (posted to the message window as WM_WEB).
    pub fn on_web_message(&self) {
        loop {
            let Some(text) = self.web.inbox.borrow_mut().pop_front() else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            if let Some(data) = self.handle(&msg) { self.reply(&id, data); }
        }
    }

    fn handle(&self, m: &Value) -> Option<Value> {
        let str_of = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some(match m.get("cmd")?.as_str()? {
            "init" => {
                let s = self.state.borrow();
                json!({
                    "config": s.config, "current": s.knob.layer, "dark": theme::system_dark(), "accent": theme::accent(),
                    "pairs": PAIRS.iter().map(|p| json!({"name": p.0, "cw": p.1, "ccw": p.2, "press": p.3})).collect::<Vec<_>>(),
                    "functions": names::FUNCTIONS.iter().map(|f| f.0).collect::<Vec<_>>(),
                    "apps": apps::running_apps(),
                })
            }
            "apps" => json!(apps::running_apps()),
            "check" => match names::check(&str_of("text")) {
                Ok(steps) => json!({"ok": true, "steps": steps}),
                Err(p) => json!({"ok": false, "problem": p}),
            },
            "pair" => {
                let (cw, ccw) = (str_of("cw"), str_of("ccw"));
                let found = PAIRS.iter().enumerate().skip(1).find_map(|(i, p)| {
                    if names::same(p.1, &cw) && names::same(p.2, &ccw) { Some((i, false)) }
                    else if names::same(p.1, &ccw) && names::same(p.2, &cw) { Some((i, true)) } else { None }
                });
                json!({"index": found.map_or(0, |f| f.0), "reversed": found.is_some_and(|f| f.1)})
            }
            "save" => {
                let mut cfg: Config = match serde_json::from_value(m.get("config")?.clone()) {
                    Ok(c) => c, Err(e) => return Some(json!({"ok": false, "error": e.to_string()})),
                };
                cfg.auto_switch = self.state.borrow().config.auto_switch;    // owned by the tray menu
                for l in &mut cfg.layers { l.apps = l.apps.iter().map(|a| apps::normalize(a)).filter(|a| !a.is_empty()).collect(); }
                match config::validate(&cfg) {
                    Err(e) => json!({"ok": false, "error": e}),
                    Ok(parsed) => {
                        self.apply_config(cfg.clone(), parsed);
                        let saved = self.save(&cfg);
                        self.refresh_tray();
                        json!({"ok": saved, "error": if saved { Value::Null } else { json!("Couldn't write the settings file") }})
                    }
                }
            }
            "record" => {
                let field = m.get("field")?.as_u64()? as usize;
                stop_hook();
                self.state.borrow_mut().recording = Some(field);
                NOTIFY.store(self.msg_hwnd(), Ordering::Relaxed);
                let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(record_proc), GetModuleHandleW(std::ptr::null()), 0) };
                HOOK.store(h, Ordering::Relaxed);
                json!({"ok": h != 0})
            }
            "stop_record" => { stop_hook(); self.state.borrow_mut().recording = None; json!({"ok": true}) }
            "import" => match self.pick_file(nwg::FileDialogAction::Open, "Import knob settings") {
                None => json!({"cancelled": true}),
                Some(path) => match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| config::from_text(&t)) {
                    Ok(c) => json!({"config": c}),
                    Err(e) => json!({"error": format!("{}: {e}", path.display())}),
                },
            },
            "export" => {
                let cfg: Config = serde_json::from_value(m.get("config")?.clone()).ok()?;
                if let Err(e) = config::validate(&cfg) { return Some(json!({"error": e})); }
                match self.pick_file(nwg::FileDialogAction::Save, "Export knob settings") {
                    None => json!({"cancelled": true}),
                    Some(mut path) => {
                        if path.extension().is_none() { path.set_extension("json"); }
                        match config::save_to(&cfg, &path) { Ok(()) => json!({"ok": true}), Err(e) => json!({"error": e.to_string()}) }
                    }
                }
            }
            "close" => { self.close_settings(); return None; }
            _ => return None,
        })
    }

    fn pick_file(&self, action: nwg::FileDialogAction, title: &str) -> Option<std::path::PathBuf> {
        let mut fd = nwg::FileDialog::default();
        nwg::FileDialog::builder().title(title).action(action).filters("Knob settings (*.json)|All files (*.*)").build(&mut fd).ok()?;
        if !fd.run(Some(&self.ui.web.win)) { return None; }
        fd.get_selected_item().ok().map(std::path::PathBuf::from)
    }

    /// From the hook: `mods` bits are ctrl, shift, alt, win.
    pub fn on_recorded(&self, vk: u8, mods: u32) {
        stop_hook();
        let Some(field) = self.state.borrow_mut().recording.take() else { return };
        if vk == 0x1B && mods == 0 { self.event("record_cancel", json!({"field": field})); return; }
        match names::combo_display(vk, mods & 1 != 0, mods & 2 != 0, mods & 4 != 0, mods & 8 != 0) {
            Some(text) => self.event("recorded", json!({"field": field, "text": text})),
            None => self.event("record_cancel", json!({"field": field, "reason": "That key can't be used in an action"})),
        }
    }
}
