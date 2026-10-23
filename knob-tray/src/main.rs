//! Rotary knob tray app: F13/F15 (turn) and F14 (press) from the remapped knob drive per-layer actions.
//! Layers switch by tap gesture or follow the focused app. Tray menu: enable, next layer, per-app switching,
//! settings, knob setup, install, start with Windows, quit.
#![windows_subsystem = "windows"]

mod action;
mod apps;
mod audio;
mod config;
mod knob;
mod names;
mod popup;
mod system;
mod theme;
mod webui;

use config::{Config, Layer, Parsed};
use native_windows_gui as nwg;
use std::{cell::RefCell, rc::Rc, sync::atomic::{AtomicIsize, Ordering}, time::{Instant, SystemTime}};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND},
    System::Threading::CreateMutexW,
    UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
    UI::WindowsAndMessaging::*,
};

const VK_F13: u32 = 0x7C; // counter-clockwise
const VK_F14: u32 = 0x7D; // press
const VK_F15: u32 = 0x7E; // clockwise
const TIMER_TAP: usize = 1;
const TIMER_FOCUS: usize = 3;
const TIMER_BATTERY: usize = 4;
const TIMER_RELOAD: usize = 5;
const FOCUS_SETTLE_MS: u32 = 150;   // ignore focus that only passes through (alt-tab, taskbar clicks)
const BATTERY_POLL_MS: u32 = 60_000;
const RELOAD_POLL_MS: u32 = 2_000;
const LOW_BATTERY: u8 = 15;
const WM_FOREGROUND: u32 = WM_APP + 1;
const WM_RECORDED: u32 = WM_APP + 2;
const WM_WEB: u32 = WM_APP + 3;     // the settings page sent a message
const WM_SETTINGS_CLOSED: u32 = WM_APP + 4;
const WM_OPEN_SETTINGS: u32 = WM_APP + 5;    // sent by a second launch
const ICON_ON: &[u8] = include_bytes!("../assets/knob_on.ico");
const ICON_OFF: &[u8] = include_bytes!("../assets/knob_off.ico");

static INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);

/// Mutable state; borrowed briefly so nested message loops (menus, dialogs) can't double-borrow.
struct State {
    config: Config,
    parsed: Vec<Parsed>,
    knob: knob::Knob,
    pace: knob::Pace,
    manual_layer: usize,              // last layer picked by gesture / tray; unlisted apps return here
    enabled: bool,
    config_mtime: Option<SystemTime>, // to notice edits made outside the app
    battery: Option<system::Battery>,
    battery_warned: bool,
    factory_warned: bool,
    recording: Option<usize>,         // action box waiting for a recorded key
}

#[derive(Default)]
struct Ui {
    msg: nwg::MessageWindow,
    icon_on: nwg::Icon, icon_off: nwg::Icon, icon_win: nwg::Icon,
    tray: nwg::TrayNotification,
    menu: nwg::Menu,
    m_enabled: nwg::MenuItem, m_next: nwg::MenuItem, m_auto: nwg::MenuItem, m_settings: nwg::MenuItem,
    m_sep1: nwg::MenuSeparator,
    m_setup: nwg::MenuItem, m_install: nwg::MenuItem, m_startup: nwg::MenuItem,
    m_sep2: nwg::MenuSeparator,
    m_quit: nwg::MenuItem,
    web: webui::Host,
}

struct App { ui: Ui, popup: popup::Popup, web: webui::Web, state: RefCell<State>, start: Instant }

impl App {
    fn hwnd(h: &nwg::ControlHandle) -> HWND { h.hwnd().unwrap() as HWND }
    fn msg_hwnd(&self) -> HWND { Self::hwnd(&self.ui.msg.handle) }
    fn now(&self) -> u64 { self.start.elapsed().as_millis() as u64 }
    fn balloon(&self, title: &str, text: &str) { self.ui.tray.show(text, Some(title), None, None); }

    /// `app`: the program that switched the layer automatically, shown in the subtitle.
    fn flash_layer(&self, app: Option<&str>) {
        let (name, i, n) = { let s = self.state.borrow(); (s.config.layers[s.knob.layer].name.clone(), s.knob.layer, s.config.layers.len()) };
        let sub = match app { Some(a) => format!("Layer {} of {n}  \u{00B7}  {a}", i + 1), None => format!("Layer {} of {n}", i + 1) };
        self.popup.show(&name, &sub, i, n);
    }

    fn set_enabled(&self, on: bool, announce: bool) {
        let h = self.msg_hwnd();
        unsafe {
            if on {
                for (id, vk) in [(1, VK_F13), (2, VK_F14), (3, VK_F15)] {
                    if RegisterHotKey(h, id, MOD_NOREPEAT, vk) == 0 {
                        self.balloon("Rotary knob", "Another app has claimed F13-F15; the knob won't work until it's closed.");
                    }
                }
            } else {
                for id in 1..=3 { UnregisterHotKey(h, id); }
            }
        }
        self.state.borrow_mut().enabled = on;
        self.refresh_tray();
        if !announce { return; }
        if on { self.flash_layer(None) } else { self.popup.show("Knob paused", "Re-enable it from the tray icon", 0, 0) }
    }

    fn refresh_tray(&self) {
        let s = self.state.borrow();
        let name = &s.config.layers[s.knob.layer].name;
        let battery = match s.battery {
            Some(b) if b.connected => format!("  \u{00B7}  {}% battery", b.percent),
            _ => String::new(),
        };
        self.ui.tray.set_icon(if s.enabled { &self.ui.icon_on } else { &self.ui.icon_off });
        self.ui.tray.set_tip(&if s.enabled { format!("Rotary knob \u{2014} {name}{battery}") } else { format!("Rotary knob \u{2014} paused{battery}") });
        self.ui.m_enabled.set_checked(s.enabled);
        self.ui.m_auto.set_checked(s.config.auto_switch);
        self.ui.m_next.set_enabled(s.config.layers.iter().filter(|l| l.enabled).count() > 1);
        self.ui.m_startup.set_checked(system::startup_enabled());
        let installed = system::running_installed();
        self.ui.m_install.set_checked(installed);
        self.ui.m_install.set_enabled(!installed);
    }

    /// The next enabled layer after `from` (or `from` itself if it's the only one).
    fn next_enabled(layers: &[Layer], from: usize) -> usize {
        (1..=layers.len()).map(|i| (from + i) % layers.len()).find(|&i| layers[i].enabled).unwrap_or(from)
    }

    fn switch_to_next_layer(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.knob.layer = Self::next_enabled(&s.config.layers, s.knob.layer);
            s.manual_layer = s.knob.layer;
        }
        self.flash_layer(None);
        self.refresh_tray();
    }

    fn run(&self, out: knob::Output) {
        use knob::Output as O;
        let (act, mult) = {
            let mut s = self.state.borrow_mut();
            let now = self.now();
            let (i, s) = (s.knob.layer, &mut *s);
            let (l, p) = (&s.config.layers[i], &s.parsed[i]);
            match out {
                O::NextLayer => (None, 0.0),
                O::Press => (Some(p.press.clone()), 1.0),
                O::Turn(d) | O::ShiftTurn(d) => match s.pace.step(now, l.accelerate, l.max_per_sec) {
                    None => return,                                         // rate-limited
                    Some(m) => {
                        let a = match (out, d > 0) {
                            (O::Turn(_), true) => &p.cw, (O::Turn(_), false) => &p.ccw,
                            (_, true) => &p.shift_cw, (_, false) => &p.shift_ccw,
                        };
                        (Some(a.clone()), m * l.speed)
                    }
                },
            }
        };
        match act {
            Some(a) => if let Some((title, sub)) = action::perform(&a, mult) { self.popup.show(&title, &sub, 0, 0) },
            None => self.switch_to_next_layer(),
        }
    }

    fn on_hotkey(&self, vk: u32) {
        let input = match vk { VK_F15 => knob::Input::Cw, VK_F13 => knob::Input::Ccw, _ => knob::Input::Press };
        let now = self.now();
        let (outs, window) = {
            let mut s = self.state.borrow_mut();
            let c = knob::Ctx { window_ms: s.config.tap_window_ms, gesture: s.config.gesture, has_shift: s.parsed[s.knob.layer].has_shift() };
            (s.knob.feed(input, now, &c), c.window_ms)
        };
        let h = self.msg_hwnd();
        if input == knob::Input::Press && outs.is_empty() {
            unsafe { SetTimer(h, TIMER_TAP, window as u32, None) };         // wait for more taps (or a turn)
        } else if outs.iter().any(|o| matches!(o, knob::Output::NextLayer | knob::Output::ShiftTurn(_))) {
            unsafe { KillTimer(h, TIMER_TAP) };
        }
        for o in outs { self.run(o); }
    }

    fn on_tap_timeout(&self) {
        unsafe { KillTimer(self.msg_hwnd(), TIMER_TAP) };
        let outs = self.state.borrow_mut().knob.timeout(); // own statement: a borrow in `for` would be held into run()
        for o in outs { self.run(o); }
    }

    /// Focus has stayed on one window for FOCUS_SETTLE_MS: give it its layer, or the manual layer if unlisted.
    fn on_focus_settled(&self) {
        unsafe { KillTimer(self.msg_hwnd(), TIMER_FOCUS) };
        let Some((exe, title)) = apps::foreground_app() else { return };     // our own settings window, or unreadable
        let (changed, matched) = {
            let mut s = self.state.borrow_mut();
            if !s.enabled || !s.config.auto_switch { return; }
            let lists: Vec<Vec<String>> = s.config.layers.iter().map(|l| if l.enabled { l.apps.clone() } else { vec![] }).collect();
            let (target, matched) = apps::pick_layer(&lists, &exe, &title, s.manual_layer);
            let changed = target != s.knob.layer;
            s.knob.layer = target;
            (changed, matched)
        };
        if changed { self.flash_layer(matched.as_deref()); self.refresh_tray(); }
    }

    fn toggle_auto_switch(&self) {
        let cfg = { let mut s = self.state.borrow_mut(); s.config.auto_switch = !s.config.auto_switch; s.config.clone() };
        self.save(&cfg);
        self.refresh_tray();
        if cfg.auto_switch { self.on_focus_settled(); }
    }

    /// Make `cfg` live (already validated).
    fn apply_config(&self, cfg: Config, parsed: Vec<Parsed>) {
        let mut s = self.state.borrow_mut();
        let last = cfg.layers.len() - 1;
        s.knob.layer = s.knob.layer.min(last);
        s.manual_layer = s.manual_layer.min(last);
        if !cfg.layers[s.knob.layer].enabled { s.knob.layer = Self::next_enabled(&cfg.layers, s.knob.layer); }
        if !cfg.layers[s.manual_layer].enabled { s.manual_layer = s.knob.layer; }
        s.config = cfg;
        s.parsed = parsed;
    }

    fn save(&self, cfg: &Config) -> bool {
        match config::save_to(cfg, &config::path()) {
            Ok(()) => { self.state.borrow_mut().config_mtime = config::modified(); true }
            Err(e) => { self.balloon("Couldn't save settings", &format!("{}: {e}", config::path().display())); false }
        }
    }

    /// Pick up edits made to config.json outside the app.
    fn check_reload(&self) {
        let m = config::modified();
        if m == self.state.borrow().config_mtime { return; }
        self.state.borrow_mut().config_mtime = m;                   // handle each change once
        let Ok(text) = std::fs::read_to_string(config::path()) else { return };
        match config::from_text(&text) {
            Ok(cfg) => {
                let parsed = config::validate(&cfg).expect("validated by from_text");
                self.apply_config(cfg, parsed);
                self.refresh_tray();
                self.popup.show("Settings reloaded", "config.json changed on disk", 0, 0);
            }
            Err(e) => self.balloon("config.json has an error", &format!("{e}\nKeeping the previous settings.")),
        }
    }

    fn check_battery(&self) {
        let b = system::battery();
        let warn = {
            let mut s = self.state.borrow_mut();
            s.battery = b;
            match b {
                Some(b) if b.connected && b.percent <= LOW_BATTERY && !s.battery_warned => { s.battery_warned = true; Some(b.percent) }
                Some(b) if b.percent > LOW_BATTERY + 5 => { s.battery_warned = false; None }
                _ => None,
            }
        };
        if let Some(p) = warn { self.balloon("Knob battery low", &format!("{p}% left. Plug it in to charge.")); }
        self.refresh_tray();
    }

    fn on_raw_input(&self, lparam: isize) {
        if self.state.borrow().factory_warned || !system::is_factory_knob_report(lparam) { return; }
        self.state.borrow_mut().factory_warned = true;
        self.balloon("Knob needs setting up", "It's sending volume keys. Right-click the tray icon and choose \u{201C}Set up knob\u{201D}.");
    }

    fn setup_knob(&self) {
        match system::setup_knob() {
            Ok(()) => self.balloon("Knob set up", "Turns now send F13/F15 and the press sends F14."),
            Err(e) => self.balloon("Couldn't set up the knob", &e),
        }
    }

    fn install(&self) {
        match system::install() {
            Err(e) => self.balloon("Install failed", &e),
            Ok(dest) => {
                // hand over to the installed copy: release hotkeys and the single-instance lock first
                self.set_enabled(false, false);
                unsafe { CloseHandle(INSTANCE_MUTEX.swap(0, Ordering::Relaxed)) };
                match std::process::Command::new(&dest).spawn() {
                    Ok(_) => nwg::stop_thread_dispatch(),
                    Err(e) => { self.balloon("Installed, but couldn't start it", &e.to_string()); self.set_enabled(true, false); }
                }
            }
        }
    }
}

fn build_ui(ui: &mut Ui) -> Result<(), nwg::NwgError> {
    let small = unsafe { GetSystemMetrics(SM_CXSMICON) } as u32;
    nwg::Icon::builder().source_bin(Some(ICON_ON)).size(Some((small, small))).build(&mut ui.icon_on)?;
    nwg::Icon::builder().source_bin(Some(ICON_OFF)).size(Some((small, small))).build(&mut ui.icon_off)?;
    nwg::Icon::builder().source_bin(Some(ICON_ON)).size(Some((32, 32))).build(&mut ui.icon_win)?;
    let mut global = nwg::Font::default();
    nwg::Font::builder().family("Segoe UI").size(16).build(&mut global)?;
    nwg::Font::set_global_default(Some(global));

    nwg::MessageWindow::builder().build(&mut ui.msg)?;
    nwg::TrayNotification::builder().parent(&ui.msg).icon(Some(&ui.icon_on)).tip(Some("Rotary knob")).build(&mut ui.tray)?;
    nwg::Menu::builder().popup(true).parent(&ui.msg).build(&mut ui.menu)?;
    let item = |m: &mut nwg::MenuItem, t: &str| nwg::MenuItem::builder().text(t).parent(&ui.menu).build(m);
    item(&mut ui.m_enabled, "Enabled")?;
    item(&mut ui.m_next, "Next layer")?;
    item(&mut ui.m_auto, "Switch layers per app")?;
    item(&mut ui.m_settings, "Settings\u{2026}")?;
    nwg::MenuSeparator::builder().parent(&ui.menu).build(&mut ui.m_sep1)?;
    item(&mut ui.m_setup, "Set up knob (map to F13\u{2013}F15)")?;
    item(&mut ui.m_install, "Install to this PC")?;
    item(&mut ui.m_startup, "Start with Windows")?;
    nwg::MenuSeparator::builder().parent(&ui.menu).build(&mut ui.m_sep2)?;
    item(&mut ui.m_quit, "Quit")?;
    webui::build(&mut ui.web, &ui.icon_win)
}

fn main() {
    // No console (windows subsystem) and panics inside Win32 callbacks abort, so leave a note behind.
    std::panic::set_hook(Box::new(|info| {
        let p = config::path().with_file_name("crash.log");
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(&p, format!("{info}\n"));
    }));
    unsafe {
        INSTANCE_MUTEX.store(CreateMutexW(std::ptr::null(), 0, system::wide("RotaryKnobTraySingleInstance").as_ptr()), Ordering::Relaxed);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // already running: launching it again (e.g. from the Start menu) opens that copy's settings
            let host = FindWindowW(std::ptr::null(), system::wide("Rotary knob \u{2014} settings").as_ptr());
            if host != 0 { PostMessageW(host, WM_OPEN_SETTINGS, 0, 0); }
            return;
        }
    }
    audio::init();
    nwg::init().expect("init GUI");
    theme::allow_dark_menus();

    let (config, load_error) = config::load();
    if let Some(e) = &load_error {
        nwg::simple_message("Rotary knob", &format!("Couldn't read {}:\n{e}\n\nUsing default layers for now. \
            The file is left as it is until you press Save in Settings.", config::path().display()));
    }
    let parsed = config::validate(&config).expect("defaults and loaded configs are valid");

    let mut ui = Ui::default();
    build_ui(&mut ui).expect("build UI");
    let app = Rc::new(App {
        ui,
        popup: popup::Popup::new(),
        web: Default::default(),
        state: RefCell::new(State {
            config, parsed, knob: Default::default(), pace: Default::default(), manual_layer: 0, enabled: false,
            config_mtime: if load_error.is_some() { None } else { config::modified() },
            battery: None, battery_warned: false, factory_warned: false, recording: None,
        }),
        start: Instant::now(),
    });
    { let (c, p) = { let s = app.state.borrow(); (s.config.clone(), config::validate(&s.config).unwrap()) }; app.apply_config(c, p); }
    if load_error.is_some() { app.state.borrow_mut().config_mtime = config::modified(); }   // don't "reload" the broken file

    let a = app.clone();
    let _raw = nwg::bind_raw_event_handler(&app.ui.msg.handle, 0x10000, move |_, msg, wp, lp| match msg {
        WM_HOTKEY => { a.on_hotkey(((lp as usize) >> 16) as u32); Some(0) }
        WM_TIMER if wp == TIMER_TAP => { a.on_tap_timeout(); Some(0) }
        WM_TIMER if wp == TIMER_FOCUS => { a.on_focus_settled(); Some(0) }
        WM_TIMER if wp == TIMER_BATTERY => { a.check_battery(); Some(0) }
        WM_TIMER if wp == TIMER_RELOAD => { a.check_reload(); Some(0) }
        WM_FOREGROUND => { unsafe { SetTimer(a.msg_hwnd(), TIMER_FOCUS, FOCUS_SETTLE_MS, None) }; Some(0) }
        WM_RECORDED => { a.on_recorded(wp as u8, lp as u32); Some(0) }
        WM_WEB => { a.on_web_message(); Some(0) }
        WM_SETTINGS_CLOSED => { a.close_settings(); Some(0) }
        WM_INPUT => { a.on_raw_input(lp); None }                    // None: default processing frees the input
        _ => None,
    }).expect("raw handler");
    let h = app.msg_hwnd();
    apps::watch_foreground(h, WM_FOREGROUND);
    system::watch_factory_mapping(h);
    unsafe {
        SetTimer(h, TIMER_BATTERY, BATTERY_POLL_MS, None);
        SetTimer(h, TIMER_RELOAD, RELOAD_POLL_MS, None);
    }

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
            E::OnMenuItemSelected if h == ui.m_enabled.handle => { let on = !a.state.borrow().enabled; a.set_enabled(on, true) }
            E::OnMenuItemSelected if h == ui.m_next.handle => a.switch_to_next_layer(),
            E::OnMenuItemSelected if h == ui.m_auto.handle => a.toggle_auto_switch(),
            E::OnMenuItemSelected if h == ui.m_settings.handle => a.open_settings(),
            E::OnMenuItemSelected if h == ui.m_setup.handle => a.setup_knob(),
            E::OnMenuItemSelected if h == ui.m_install.handle => a.install(),
            E::OnMenuItemSelected if h == ui.m_startup.handle => { system::set_startup(!system::startup_enabled()); a.refresh_tray() }
            E::OnMenuItemSelected if h == ui.m_quit.handle => nwg::stop_thread_dispatch(),
            _ => {}
        }
    });

    let a = app.clone();
    // Closing the settings window: free the web view from the app's own loop, not inside the window's close handling.
    let _win = nwg::bind_raw_event_handler(&app.ui.web.win.handle, 0x10001, move |_, msg, _, _| match msg {
        WM_CLOSE => { unsafe { PostMessageW(a.msg_hwnd(), WM_SETTINGS_CLOSED, 0, 0) }; Some(0) }
        WM_OPEN_SETTINGS => { a.open_settings(); Some(0) }
        _ => None,
    }).expect("settings window handler");

    app.check_battery();
    app.set_enabled(true, true);
    app.on_focus_settled();
    if std::env::args().any(|a| a == "--settings") { app.open_settings(); }
    nwg::dispatch_thread_events();
    app.set_enabled(false, false);
}
