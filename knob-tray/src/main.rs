//! Rotary knob tray app: F13/F15 (turn) and F14 (press) from the remapped knob drive per-layer actions.
//! Double-tap the knob to switch layer. Tray menu: enable/disable, next layer, settings, start with Windows, quit.
#![windows_subsystem = "windows"]

mod action;
mod knob;
mod popup;

use native_windows_gui as nwg;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND},
    System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ},
    System::Threading::CreateMutexW,
    UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
    UI::WindowsAndMessaging::*,
};

const VK_F13: u32 = 0x7C; // counter-clockwise
const VK_F14: u32 = 0x7D; // press
const VK_F15: u32 = 0x7E; // clockwise
const TIMER_TAP: usize = 1;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "RotaryKnob";
const ICON_ON: &[u8] = include_bytes!("../assets/knob_on.ico");
const ICON_OFF: &[u8] = include_bytes!("../assets/knob_off.ico");

/// name, clockwise, counter-clockwise, press. Index 0 is the "nothing matched" entry.
const PRESETS: &[(&str, &str, &str, &str)] = &[
    ("Custom", "", "", ""),
    ("Scroll", "wheel:-120", "wheel:120", "click:middle"),
    ("Horizontal scroll", "hwheel:120", "hwheel:-120", ""),
    ("Zoom", "ctrl+wheel:120", "ctrl+wheel:-120", "ctrl+0"),
    ("Media", "next", "prev", "playpause"),
    ("Volume", "volup", "voldown", "mute"),
    ("Undo / Redo", "ctrl+y", "ctrl+z", ""),
    ("Arrow up / down", "down", "up", "enter"),
    ("Arrow left / right", "right", "left", "enter"),
    ("Page up / down", "pagedown", "pageup", "home"),
    ("Browser tabs", "ctrl+tab", "ctrl+shift+tab", "ctrl+t"),
    ("Brush size  [ ]", "]", "[", ""),
];

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Layer { name: String, clockwise: String, counter_clockwise: String, press: String }

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config { double_tap_ms: u64, layers: Vec<Layer> }

impl Default for Config {
    fn default() -> Self {
        let l = |i: usize| { let p = PRESETS[i]; Layer { name: p.0.into(), clockwise: p.1.into(), counter_clockwise: p.2.into(), press: p.3.into() } };
        Config { double_tap_ms: 350, layers: vec![l(1), l(3), l(4)] }
    }
}

fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join(APP_NAME).join("config.json")
}
fn load_config() -> Config {
    let c: Config = std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if c.layers.is_empty() { Config::default() } else { c }
}
fn save_config(c: &Config) -> std::io::Result<()> {
    let p = config_path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, serde_json::to_string_pretty(c).unwrap())
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn startup_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr(), RRF_RT_REG_SZ,
                          std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) == 0 }
}
fn set_startup(on: bool) {
    unsafe {
        if on {
            let exe = wide(&format!("\"{}\"", std::env::current_exe().unwrap().display()));
            RegSetKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr(), REG_SZ,
                            exe.as_ptr().cast(), (exe.len() * 2) as u32);
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr());
        }
    }
}

/// Mutable state; borrowed briefly so nested message loops (menus, dialogs) can't double-borrow.
struct State {
    config: Config,
    parsed: Vec<[action::Action; 3]>, // clockwise, counter-clockwise, press per layer
    knob: knob::Knob,
    enabled: bool,
    draft: Vec<Layer>,         // settings window working copy
    draft_sel: Option<usize>,
}

#[derive(Default)]
struct Ui {
    msg: nwg::MessageWindow,
    icon_on: nwg::Icon,
    icon_off: nwg::Icon,
    icon_win: nwg::Icon,
    tray: nwg::TrayNotification,
    menu: nwg::Menu,
    m_enabled: nwg::MenuItem,
    m_next: nwg::MenuItem,
    m_settings: nwg::MenuItem,
    m_startup: nwg::MenuItem,
    m_sep: nwg::MenuSeparator,
    m_quit: nwg::MenuItem,
    font: nwg::Font,
    font_head: nwg::Font,
    font_small: nwg::Font,
    win: nwg::Window,
    h_layers: nwg::Label, h_layer: nwg::Label, h_syntax: nwg::Label,
    list: nwg::ListBox<String>,
    b_add: nwg::Button, b_remove: nwg::Button, b_up: nwg::Button, b_down: nwg::Button,
    labels: [nwg::Label; 6],
    t_name: nwg::TextInput, preset: nwg::ComboBox<String>,
    t_cw: nwg::TextInput, t_ccw: nwg::TextInput, t_press: nwg::TextInput, t_double: nwg::TextInput,
    help: nwg::Label,
    status: nwg::Label,
    b_save: nwg::Button, b_close: nwg::Button,
}

struct App { ui: Ui, popup: popup::Popup, state: RefCell<State>, start: Instant }

fn parse_layers(layers: &[Layer]) -> Result<Vec<[action::Action; 3]>, String> {
    layers.iter().map(|l| {
        let p = |what: &str, s: &str| action::parse(s).map_err(|e| format!("Layer '{}', {what}: {e}", l.name));
        Ok([p("clockwise", &l.clockwise)?, p("counter-clockwise", &l.counter_clockwise)?, p("press", &l.press)?])
    }).collect()
}

impl App {
    fn hwnd(h: &nwg::ControlHandle) -> HWND { h.hwnd().unwrap() as HWND }

    fn flash(&self, title: &str, subtitle: &str, dots: Option<(usize, usize)>) {
        let (i, n) = dots.unwrap_or((0, 0));
        self.popup.show(title, subtitle, i, n);
    }

    fn flash_layer(&self) {
        let (name, i, n) = { let s = self.state.borrow(); (s.config.layers[s.knob.layer].name.clone(), s.knob.layer, s.config.layers.len()) };
        self.flash(&name, &format!("Layer {} of {n}", i + 1), Some((i, n)));
    }

    fn set_enabled(&self, on: bool) {
        let h = Self::hwnd(&self.ui.msg.handle);
        unsafe {
            if on {
                for (id, vk) in [(1, VK_F13), (2, VK_F14), (3, VK_F15)] {
                    if RegisterHotKey(h, id, MOD_NOREPEAT, vk) == 0 {
                        self.ui.tray.show("Another app has claimed F13-F15; the knob won't work until it's closed.",
                                          Some("Rotary knob"), None, None);
                    }
                }
            } else {
                for id in 1..=3 { UnregisterHotKey(h, id); }
            }
        }
        self.state.borrow_mut().enabled = on;
        self.refresh_tray();
        if on { self.flash_layer() } else { self.flash("Knob paused", "Double-tap does nothing until re-enabled", None) }
    }

    fn refresh_tray(&self) {
        let s = self.state.borrow();
        let name = &s.config.layers[s.knob.layer].name;
        self.ui.tray.set_icon(if s.enabled { &self.ui.icon_on } else { &self.ui.icon_off });
        self.ui.tray.set_tip(&if s.enabled { format!("Rotary knob \u{2014} {name}") } else { "Rotary knob \u{2014} paused".into() });
        self.ui.m_enabled.set_checked(s.enabled);
        self.ui.m_startup.set_checked(startup_enabled());
    }

    fn run(&self, out: knob::Output) {
        let act = {
            let s = self.state.borrow();
            let i = s.knob.layer;
            match out {
                knob::Output::Turn(1) => Some(s.parsed[i][0].clone()),
                knob::Output::Turn(_) => Some(s.parsed[i][1].clone()),
                knob::Output::Press => Some(s.parsed[i][2].clone()),
                knob::Output::Layer(_) => None,
            }
        };
        match act {
            Some(a) => action::perform(&a),
            None => { self.flash_layer(); self.refresh_tray(); }
        }
    }

    fn on_hotkey(&self, vk: u32) {
        let input = match vk { VK_F15 => knob::Input::Cw, VK_F13 => knob::Input::Ccw, _ => knob::Input::Press };
        let now = self.start.elapsed().as_millis() as u64;
        let (out, double_ms) = {
            let mut s = self.state.borrow_mut();
            let (n, d) = (s.config.layers.len(), s.config.double_tap_ms);
            (s.knob.feed(input, now, n, d), d)
        };
        match out {
            Some(o) => { unsafe { KillTimer(Self::hwnd(&self.ui.msg.handle), TIMER_TAP) }; self.run(o) }
            None => unsafe { SetTimer(Self::hwnd(&self.ui.msg.handle), TIMER_TAP, double_ms as u32, None); },
        }
    }

    fn on_tap_timeout(&self) {
        unsafe { KillTimer(Self::hwnd(&self.ui.msg.handle), TIMER_TAP) };
        let out = self.state.borrow_mut().knob.timeout(); // own statement: an `if let` would hold the borrow into run()
        if let Some(o) = out { self.run(o); }
    }

    // --- settings window ---
    fn open_settings(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.draft = s.config.layers.clone();
            s.draft_sel = Some(s.knob.layer);
            self.ui.t_double.set_text(&s.config.double_tap_ms.to_string());
        }
        self.ui.status.set_text("");
        self.refresh_list();
        self.load_fields();
        self.ui.win.set_visible(true);
        self.ui.win.set_focus();
    }

    fn refresh_list(&self) {
        let s = self.state.borrow();
        self.ui.list.set_collection(s.draft.iter().map(|l| l.name.clone()).collect());
        self.ui.list.set_selection(s.draft_sel);
    }

    fn load_fields(&self) {
        let l = { let s = self.state.borrow(); s.draft_sel.and_then(|i| s.draft.get(i)).cloned().unwrap_or_default() };
        self.ui.t_name.set_text(&l.name);
        self.ui.t_cw.set_text(&l.clockwise);
        self.ui.t_ccw.set_text(&l.counter_clockwise);
        self.ui.t_press.set_text(&l.press);
        let matching = PRESETS.iter().skip(1).position(|p| (p.1, p.2, p.3) == (&*l.clockwise, &*l.counter_clockwise, &*l.press));
        self.ui.preset.set_selection(Some(matching.map_or(0, |i| i + 1)));
    }

    fn apply_preset(&self) {
        let Some(i) = self.ui.preset.selection().filter(|&i| i > 0) else { return };
        let p = PRESETS[i];
        self.ui.t_cw.set_text(p.1);
        self.ui.t_ccw.set_text(p.2);
        self.ui.t_press.set_text(p.3);
        let name = self.ui.t_name.text();
        if name.trim().is_empty() || name == "New layer" || PRESETS.iter().any(|q| q.0 == name) { self.ui.t_name.set_text(p.0); }
        self.commit_fields();
        self.refresh_list();
    }

    fn commit_fields(&self) {
        let mut s = self.state.borrow_mut();
        if let Some(i) = s.draft_sel {
            s.draft[i] = Layer { name: self.ui.t_name.text(), clockwise: self.ui.t_cw.text(),
                                 counter_clockwise: self.ui.t_ccw.text(), press: self.ui.t_press.text() };
        }
    }

    fn edit_list(&self, f: impl FnOnce(&mut Vec<Layer>, usize) -> usize) {
        self.commit_fields();
        {
            let mut s = self.state.borrow_mut();
            let i = s.draft_sel.unwrap_or(0);
            let new = f(&mut s.draft, i);
            s.draft_sel = Some(new.min(s.draft.len() - 1));
        }
        self.refresh_list();
        self.load_fields();
    }

    fn save_settings(&self) {
        self.commit_fields();
        let draft = self.state.borrow().draft.clone();
        let result = parse_layers(&draft).and_then(|parsed| {
            let d: u64 = self.ui.t_double.text().trim().parse().map_err(|_| "Double-tap window must be a number".to_string())?;
            if !(100..=1000).contains(&d) { return Err("Double-tap window must be between 100 and 1000 ms".into()); }
            Ok((parsed, d))
        });
        match result {
            Err(e) => { self.ui.status.set_text(""); nwg::modal_error_message(&self.ui.win, "Can't save", &e); }
            Ok((parsed, d)) => {
                let cfg = {
                    let mut s = self.state.borrow_mut();
                    s.config = Config { double_tap_ms: d, layers: draft };
                    s.parsed = parsed;
                    s.knob.layer = s.knob.layer.min(s.config.layers.len() - 1);
                    s.config.clone()
                };
                match save_config(&cfg) {
                    Ok(()) => self.ui.status.set_text("\u{2713} Saved"),
                    Err(e) => { nwg::modal_error_message(&self.ui.win, "Can't save", &format!("{}: {e}", config_path().display())); }
                }
                self.refresh_list();
                self.refresh_tray();
            }
        }
    }
}

fn build_ui(ui: &mut Ui) -> Result<(), nwg::NwgError> {
    let small = unsafe { GetSystemMetrics(SM_CXSMICON) } as u32;
    nwg::Icon::builder().source_bin(Some(ICON_ON)).size(Some((small, small))).build(&mut ui.icon_on)?;
    nwg::Icon::builder().source_bin(Some(ICON_OFF)).size(Some((small, small))).build(&mut ui.icon_off)?;
    nwg::Icon::builder().source_bin(Some(ICON_ON)).size(Some((32, 32))).build(&mut ui.icon_win)?;
    nwg::Font::builder().family("Segoe UI").size(16).build(&mut ui.font)?;
    nwg::Font::builder().family("Segoe UI Semibold").size(18).build(&mut ui.font_head)?;
    nwg::Font::builder().family("Segoe UI").size(14).build(&mut ui.font_small)?;
    let mut global = nwg::Font::default();
    nwg::Font::builder().family("Segoe UI").size(16).build(&mut global)?;
    nwg::Font::set_global_default(Some(global));

    nwg::MessageWindow::builder().build(&mut ui.msg)?;
    nwg::TrayNotification::builder().parent(&ui.msg).icon(Some(&ui.icon_on)).tip(Some("Rotary knob")).build(&mut ui.tray)?;
    nwg::Menu::builder().popup(true).parent(&ui.msg).build(&mut ui.menu)?;
    nwg::MenuItem::builder().text("Enabled").parent(&ui.menu).build(&mut ui.m_enabled)?;
    nwg::MenuItem::builder().text("Next layer").parent(&ui.menu).build(&mut ui.m_next)?;
    nwg::MenuItem::builder().text("Settings\u{2026}").parent(&ui.menu).build(&mut ui.m_settings)?;
    nwg::MenuItem::builder().text("Start with Windows").parent(&ui.menu).build(&mut ui.m_startup)?;
    nwg::MenuSeparator::builder().parent(&ui.menu).build(&mut ui.m_sep)?;
    nwg::MenuItem::builder().text("Quit").parent(&ui.menu).build(&mut ui.m_quit)?;

    // Settings window (closing it only hides it). Layout: layer list on the left, editor on the right.
    nwg::Window::builder().flags(nwg::WindowFlags::WINDOW).size((740, 500)).center(true)
        .icon(Some(&ui.icon_win)).title("Rotary knob \u{2014} settings").build(&mut ui.win)?;
    let w = &ui.win;
    let head = |l: &mut nwg::Label, t: &str, x: i32, y: i32, f: &nwg::Font| nwg::Label::builder().parent(w).text(t)
        .position((x, y)).size((300, 26)).font(Some(f)).build(l);
    head(&mut ui.h_layers, "Layers", 20, 16, &ui.font_head)?;
    head(&mut ui.h_layer, "Layer", 256, 16, &ui.font_head)?;
    nwg::ListBox::builder().parent(w).position((20, 48)).size((212, 330)).build(&mut ui.list)?;
    for (b, t, x, y) in [(&mut ui.b_add, "Add", 20, 388), (&mut ui.b_remove, "Remove", 128, 388),
                         (&mut ui.b_up, "Move up", 20, 424), (&mut ui.b_down, "Move down", 128, 424)] {
        nwg::Button::builder().parent(w).text(t).position((x, y)).size((104, 30)).build(b)?;
    }

    let (lx, ix, iw, row) = (256, 440, 280, 38);
    for (i, t) in ["Name", "Preset", "Turn clockwise", "Turn counter-clockwise", "Press", "Double-tap window (ms)"].iter().enumerate() {
        nwg::Label::builder().parent(w).text(t).position((lx, 52 + i as i32 * row)).size((180, 24)).build(&mut ui.labels[i])?;
    }
    nwg::TextInput::builder().parent(w).position((ix, 49)).size((iw, 24)).build(&mut ui.t_name)?;
    nwg::ComboBox::builder().parent(w).position((ix, 48 + row)).size((iw, 26))
        .collection(PRESETS.iter().map(|p| p.0.to_string()).collect()).build(&mut ui.preset)?;
    for (i, t) in [&mut ui.t_cw, &mut ui.t_ccw, &mut ui.t_press].into_iter().enumerate() {
        nwg::TextInput::builder().parent(w).position((ix, 49 + (i as i32 + 2) * row)).size((iw, 24)).build(t)?;
    }
    nwg::TextInput::builder().parent(w).position((ix, 49 + 5 * row)).size((80, 24)).build(&mut ui.t_double)?;

    head(&mut ui.h_syntax, "Action syntax", lx, 290, &ui.font_head)?;
    nwg::Label::builder().parent(w).text(action::HELP).position((lx, 320)).size((464, 100)).font(Some(&ui.font_small)).build(&mut ui.help)?;
    nwg::Label::builder().parent(w).text("").position((lx, 430)).size((200, 24)).build(&mut ui.status)?;
    nwg::Button::builder().parent(w).text("Save").position((520, 424)).size((96, 30)).build(&mut ui.b_save)?;
    nwg::Button::builder().parent(w).text("Close").position((624, 424)).size((96, 30)).build(&mut ui.b_close)?;
    Ok(())
}

fn main() {
    // No console (windows subsystem) and panics inside Win32 callbacks abort, so leave a note behind.
    std::panic::set_hook(Box::new(|info| {
        let p = config_path().with_file_name("crash.log");
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(&p, format!("{info}\n"));
    }));
    unsafe {
        CreateMutexW(std::ptr::null(), 0, wide("RotaryKnobTraySingleInstance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS { return; }
    }
    nwg::init().expect("init GUI");

    let config = load_config();
    let parsed = parse_layers(&config.layers).unwrap_or_else(|e| {
        nwg::simple_message("Rotary knob", &format!("{e}\n\nUsing default layers."));
        parse_layers(&Config::default().layers).unwrap()
    });
    let config = if parsed.len() == config.layers.len() { config } else { Config::default() };

    let mut ui = Ui::default();
    build_ui(&mut ui).expect("build UI");
    let app = Rc::new(App {
        ui,
        popup: popup::Popup::new(),
        state: RefCell::new(State { config, parsed, knob: Default::default(), enabled: false, draft: vec![], draft_sel: None }),
        start: Instant::now(),
    });

    let a = app.clone();
    let _raw = nwg::bind_raw_event_handler(&app.ui.msg.handle, 0x10000, move |_, msg, wp, lp| match msg {
        WM_HOTKEY => { a.on_hotkey(((lp as usize) >> 16) as u32); Some(0) }
        WM_TIMER if wp == TIMER_TAP => { a.on_tap_timeout(); Some(0) }
        _ => None,
    }).expect("raw handler");

    let a = app.clone();
    let _tray = nwg::full_bind_event_handler(&app.ui.msg.handle, move |evt, _, h| {
        use nwg::Event as E;
        let ui = &a.ui;
        match evt {
            E::OnContextMenu if h == ui.tray.handle => {
                a.refresh_tray();
                let (x, y) = nwg::GlobalCursor::position();
                ui.menu.popup(x, y);
            }
            E::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) if h == ui.tray.handle => a.open_settings(),
            E::OnMenuItemSelected if h == ui.m_enabled.handle => { let on = !a.state.borrow().enabled; a.set_enabled(on) }
            E::OnMenuItemSelected if h == ui.m_next.handle => {
                { let mut s = a.state.borrow_mut(); s.knob.layer = (s.knob.layer + 1) % s.config.layers.len(); }
                a.run(knob::Output::Layer(0));
            }
            E::OnMenuItemSelected if h == ui.m_settings.handle => a.open_settings(),
            E::OnMenuItemSelected if h == ui.m_startup.handle => { set_startup(!startup_enabled()); a.refresh_tray() }
            E::OnMenuItemSelected if h == ui.m_quit.handle => nwg::stop_thread_dispatch(),
            _ => {}
        }
    });

    let a = app.clone();
    let _win = nwg::full_bind_event_handler(&app.ui.win.handle, move |evt, _, h| {
        use nwg::Event as E;
        let ui = &a.ui;
        match evt {
            E::OnListBoxSelect if h == ui.list.handle => {
                a.commit_fields();
                a.state.borrow_mut().draft_sel = ui.list.selection();
                a.refresh_list();
                a.load_fields();
            }
            E::OnComboxBoxSelection if h == ui.preset.handle => a.apply_preset(),
            E::OnButtonClick if h == ui.b_add.handle => a.edit_list(|d, _| { d.push(Layer { name: "New layer".into(), ..Default::default() }); d.len() - 1 }),
            E::OnButtonClick if h == ui.b_remove.handle => a.edit_list(|d, i| { if d.len() > 1 { d.remove(i); } i }),
            E::OnButtonClick if h == ui.b_up.handle => a.edit_list(|d, i| if i > 0 { d.swap(i, i - 1); i - 1 } else { i }),
            E::OnButtonClick if h == ui.b_down.handle => a.edit_list(|d, i| if i + 1 < d.len() { d.swap(i, i + 1); i + 1 } else { i }),
            E::OnButtonClick if h == ui.b_save.handle => a.save_settings(),
            E::OnButtonClick if h == ui.b_close.handle => ui.win.set_visible(false),
            _ => {}
        }
    });

    app.set_enabled(true);
    if std::env::args().any(|a| a == "--settings") { app.open_settings(); }
    nwg::dispatch_thread_events();
    app.set_enabled(false);
}
