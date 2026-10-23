//! Settings window: layer list on the left, the selected layer's editor and general options on the right.
//! Edits go to a draft; Save validates and applies it. Closing the window only hides it.
use crate::{action, apps, config::{self, Config, Layer, PRESETS}, knob::Gesture, App, WM_RECORDED};
use native_windows_gui as nwg;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::Input::KeyboardAndMouse::GetAsyncKeyState,
    UI::WindowsAndMessaging::*,
};

const GESTURES: [(&str, Gesture); 3] = [("Double-tap", Gesture::DoubleTap), ("Triple-tap", Gesture::TripleTap), ("Tray menu only", Gesture::TrayOnly)];
const ACTION_FIELDS: [&str; 5] = ["Turn clockwise", "Turn counter-clockwise", "Press", "Tap + turn clockwise", "Tap + turn counter-clockwise"];

#[derive(Default)]
pub struct SettingsUi {
    pub win: nwg::Window,
    font_head: nwg::Font, font_small: nwg::Font,
    h_layers: nwg::Label, h_layer: nwg::Label, h_general: nwg::Label, h_syntax: nwg::Label,
    list: nwg::ListBox<String>,
    b_add: nwg::Button, b_remove: nwg::Button, b_up: nwg::Button, b_down: nwg::Button,
    labels: [nwg::Label; 11],
    t_name: nwg::TextInput, c_enabled: nwg::CheckBox, preset: nwg::ComboBox<String>,
    actions: [nwg::TextInput; 5], rec: [nwg::Button; 5],
    t_speed: nwg::TextInput, c_accel: nwg::CheckBox, l_max: nwg::Label, t_max: nwg::TextInput,
    t_apps: nwg::TextInput, add_app: nwg::ComboBox<String>,
    gesture: nwg::ComboBox<String>, t_window: nwg::TextInput,
    help: nwg::Label, status: nwg::Label,
    b_import: nwg::Button, b_export: nwg::Button, b_save: nwg::Button, b_close: nwg::Button,
}

pub fn build(ui: &mut SettingsUi, icon: &nwg::Icon) -> Result<(), nwg::NwgError> {
    nwg::Font::builder().family("Segoe UI Semibold").size(18).build(&mut ui.font_head)?;
    nwg::Font::builder().family("Segoe UI").size(14).build(&mut ui.font_small)?;
    nwg::Window::builder().flags(nwg::WindowFlags::WINDOW).size((920, 676)).center(true)
        .icon(Some(icon)).title("Rotary knob \u{2014} settings").build(&mut ui.win)?;
    let w = &ui.win;
    let head = |l: &mut nwg::Label, t: &str, x: i32, y: i32, f: &nwg::Font| nwg::Label::builder().parent(w).text(t)
        .position((x, y)).size((300, 26)).font(Some(f)).build(l);
    let button = |b: &mut nwg::Button, t: &str, x: i32, y: i32, wd: i32| nwg::Button::builder().parent(w).text(t)
        .position((x, y)).size((wd, 30)).build(b);
    let input = |t: &mut nwg::TextInput, x: i32, y: i32, wd: i32| nwg::TextInput::builder().parent(w).position((x, y)).size((wd, 24)).build(t);

    head(&mut ui.h_layers, "Layers", 20, 16, &ui.font_head)?;
    nwg::ListBox::builder().parent(w).position((20, 48)).size((212, 516)).build(&mut ui.list)?;
    button(&mut ui.b_add, "Add", 20, 574, 104)?;
    button(&mut ui.b_remove, "Remove", 128, 574, 104)?;
    button(&mut ui.b_up, "Move up", 20, 610, 104)?;
    button(&mut ui.b_down, "Move down", 128, 610, 104)?;

    let (lx, ix, iw, row, y0) = (256, 446, 280, 36, 48);
    let ry = |r: i32| if r < 9 { y0 + r * row } else { y0 + r * row + 42 };   // rows 9+ sit under "General"
    head(&mut ui.h_layer, "Layer", lx, 16, &ui.font_head)?;
    let names = ["Name", "Preset", ACTION_FIELDS[0], ACTION_FIELDS[1], ACTION_FIELDS[2], ACTION_FIELDS[3], ACTION_FIELDS[4],
                 "Speed (\u{00D7})", "Apps (auto-switch)", "Layer switch gesture", "Tap window (ms)"];
    for (i, t) in names.iter().enumerate() {
        nwg::Label::builder().parent(w).text(t).position((lx, ry(i as i32) + 4)).size((185, 22)).build(&mut ui.labels[i])?;
    }
    input(&mut ui.t_name, ix, ry(0) + 1, iw)?;
    nwg::CheckBox::builder().parent(w).text("Enabled").position((ix + iw + 12, ry(0))).size((110, 26)).build(&mut ui.c_enabled)?;
    nwg::ComboBox::builder().parent(w).position((ix, ry(1))).size((iw, 26))
        .collection(PRESETS.iter().map(|p| p.0.to_string()).collect()).build(&mut ui.preset)?;
    for i in 0..5 {
        input(&mut ui.actions[i], ix, ry(2 + i as i32) + 1, iw)?;
        nwg::Button::builder().parent(w).text("Record").position((ix + iw + 8, ry(2 + i as i32))).size((76, 26)).build(&mut ui.rec[i])?;
    }
    input(&mut ui.t_speed, ix, ry(7) + 1, 60)?;
    nwg::CheckBox::builder().parent(w).text("Accelerate").position((ix + 74, ry(7))).size((110, 26)).build(&mut ui.c_accel)?;
    nwg::Label::builder().parent(w).text("Max per second").position((ix + 194, ry(7) + 4)).size((110, 22)).build(&mut ui.l_max)?;
    input(&mut ui.t_max, ix + 306, ry(7) + 1, 50)?;
    nwg::TextInput::builder().parent(w).position((ix, ry(8) + 1)).size((150, 24))
        .placeholder_text(Some("e.g. chrome, photoshop")).build(&mut ui.t_apps)?;
    nwg::ComboBox::builder().parent(w).position((ix + 158, ry(8))).size((iw - 158, 26)).build(&mut ui.add_app)?;

    head(&mut ui.h_general, "General", lx, ry(9) - 34, &ui.font_head)?;
    nwg::ComboBox::builder().parent(w).position((ix, ry(9))).size((200, 26))
        .collection(GESTURES.iter().map(|g| g.0.to_string()).collect()).build(&mut ui.gesture)?;
    input(&mut ui.t_window, ix, ry(10) + 1, 80)?;

    head(&mut ui.h_syntax, "Action syntax", lx, 494, &ui.font_head)?;
    nwg::Label::builder().parent(w).text(action::HELP).position((lx, 522)).size((650, 100)).font(Some(&ui.font_small)).build(&mut ui.help)?;
    button(&mut ui.b_import, "Import\u{2026}", lx, 630, 96)?;
    button(&mut ui.b_export, "Export\u{2026}", lx + 104, 630, 96)?;
    nwg::Label::builder().parent(w).text("").position((lx + 212, 637)).size((236, 22)).font(Some(&ui.font_small)).build(&mut ui.status)?;
    button(&mut ui.b_save, "Save", 712, 630, 96)?;
    button(&mut ui.b_close, "Close", 816, 630, 96)?;
    Ok(())
}

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
        let ui = &self.ui.s;
        {
            let mut s = self.state.borrow_mut();
            s.draft = s.config.layers.clone();
            s.draft_sel = Some(s.knob.layer);
            ui.gesture.set_selection(GESTURES.iter().position(|g| g.1 == s.config.gesture));
            ui.t_window.set_text(&s.config.tap_window_ms.to_string());
        }
        ui.status.set_text("");
        let mut running = vec!["Add running app\u{2026}".to_string()];
        running.extend(apps::running_apps());
        ui.add_app.set_collection(running);
        ui.add_app.set_selection(Some(0));
        self.refresh_list();
        self.load_fields();
        crate::theme::apply(App::hwnd(&ui.win.handle));
        ui.win.set_visible(true);
        ui.win.set_focus();
    }

    fn refresh_list(&self) {
        let s = self.state.borrow();
        let names = s.draft.iter().map(|l| if l.enabled { l.name.clone() } else { format!("{}  (off)", l.name) }).collect();
        self.ui.s.list.set_collection(names);
        self.ui.s.list.set_selection(s.draft_sel);
    }

    fn load_fields(&self) {
        let ui = &self.ui.s;
        let l = { let s = self.state.borrow(); s.draft_sel.and_then(|i| s.draft.get(i)).cloned().unwrap_or_default() };
        ui.t_name.set_text(&l.name);
        ui.c_enabled.set_check_state(if l.enabled { nwg::CheckBoxState::Checked } else { nwg::CheckBoxState::Unchecked });
        for (t, v) in ui.actions.iter().zip([&l.clockwise, &l.counter_clockwise, &l.press, &l.shift_clockwise, &l.shift_counter_clockwise]) {
            t.set_text(v);
        }
        ui.t_speed.set_text(&l.speed.to_string());
        ui.c_accel.set_check_state(if l.accelerate { nwg::CheckBoxState::Checked } else { nwg::CheckBoxState::Unchecked });
        ui.t_max.set_text(&l.max_per_sec.to_string());
        ui.t_apps.set_text(&l.apps.join(", "));
        let matching = PRESETS.iter().skip(1).position(|p| (p.1, p.2, p.3) == (&*l.clockwise, &*l.counter_clockwise, &*l.press));
        ui.preset.set_selection(Some(matching.map_or(0, |i| i + 1)));
    }

    fn commit_fields(&self) {
        let ui = &self.ui.s;
        let mut s = self.state.borrow_mut();
        let Some(i) = s.draft_sel else { return };
        let a: Vec<String> = ui.actions.iter().map(|t| t.text()).collect();
        let checked = |c: &nwg::CheckBox| c.check_state() == nwg::CheckBoxState::Checked;
        s.draft[i] = Layer {
            name: ui.t_name.text().trim().to_string(),
            enabled: checked(&ui.c_enabled),
            clockwise: a[0].clone(), counter_clockwise: a[1].clone(), press: a[2].clone(),
            shift_clockwise: a[3].clone(), shift_counter_clockwise: a[4].clone(),
            speed: ui.t_speed.text().trim().parse().unwrap_or(0.0),          // out of range -> Save explains
            accelerate: checked(&ui.c_accel),
            max_per_sec: ui.t_max.text().trim().parse().unwrap_or(u32::MAX),
            apps: apps::parse_list(&ui.t_apps.text()),
        };
    }

    fn apply_preset(&self) {
        let ui = &self.ui.s;
        let Some(i) = ui.preset.selection().filter(|&i| i > 0) else { return };
        let p = PRESETS[i];
        ui.actions[0].set_text(p.1);
        ui.actions[1].set_text(p.2);
        ui.actions[2].set_text(p.3);
        let name = ui.t_name.text();
        if name.trim().is_empty() || name == "New layer" || PRESETS.iter().any(|q| q.0 == name) { ui.t_name.set_text(p.0); }
        self.commit_fields();
        self.refresh_list();
    }

    fn add_running_app(&self) {
        let ui = &self.ui.s;
        let Some(exe) = ui.add_app.selection().filter(|&i| i > 0).and_then(|_| ui.add_app.selection_string()) else { return };
        let mut list = apps::parse_list(&ui.t_apps.text());
        if !list.contains(&exe) { list.push(exe); }
        ui.t_apps.set_text(&list.join(", "));
        ui.add_app.set_selection(Some(0));
        self.commit_fields();
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

    fn draft_config(&self) -> Config {
        let ui = &self.ui.s;
        let s = self.state.borrow();
        Config {
            tap_window_ms: ui.t_window.text().trim().parse().unwrap_or(0),
            gesture: ui.gesture.selection().map_or(Gesture::DoubleTap, |i| GESTURES[i].1),
            auto_switch: s.config.auto_switch,
            layers: s.draft.clone(),
        }
    }

    fn save_settings(&self) {
        self.commit_fields();
        let cfg = self.draft_config();
        match config::validate(&cfg) {
            Err(e) => { self.ui.s.status.set_text(""); nwg::modal_error_message(&self.ui.s.win, "Can't save", &e); }
            Ok(parsed) => {
                self.apply_config(cfg.clone(), parsed);
                if self.save(&cfg) { self.ui.s.status.set_text("\u{2713} Saved"); }
                self.refresh_list();
                self.refresh_tray();
            }
        }
    }

    fn start_record(&self, field: usize) {
        stop_hook();
        self.state.borrow_mut().recording = Some(field);
        NOTIFY.store(self.msg_hwnd(), Ordering::Relaxed);
        let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(record_proc), GetModuleHandleW(std::ptr::null()), 0) };
        HOOK.store(h, Ordering::Relaxed);
        self.ui.s.status.set_text(if h != 0 { "Press a key combination\u{2026} (Esc cancels)" } else { "Couldn't start recording" });
    }

    /// From the hook: `mods` bits are ctrl, shift, alt, win.
    pub fn on_recorded(&self, vk: u8, mods: u32) {
        stop_hook();
        let Some(field) = self.state.borrow_mut().recording.take() else { return };
        let ui = &self.ui.s;
        if vk == 0x1B && mods == 0 { ui.status.set_text("Recording cancelled"); return; }
        match action::combo_name(vk, mods & 1 != 0, mods & 2 != 0, mods & 4 != 0, mods & 8 != 0) {
            None => ui.status.set_text("That key can't be used in an action"),
            Some(combo) => {
                ui.actions[field].set_text(&combo);
                self.commit_fields();
                self.load_fields();                                  // preset box shows "Custom" if it no longer matches
                ui.status.set_text(&format!("Recorded {combo}"));
            }
        }
    }

    fn pick_file(&self, action: nwg::FileDialogAction, title: &str) -> Option<std::path::PathBuf> {
        let mut fd = nwg::FileDialog::default();
        nwg::FileDialog::builder().title(title).action(action).filters("Knob settings (*.json)|All files (*.*)").build(&mut fd).ok()?;
        if !fd.run(Some(&self.ui.s.win)) { return None; }
        fd.get_selected_item().ok().map(std::path::PathBuf::from)
    }

    fn import(&self) {
        let Some(path) = self.pick_file(nwg::FileDialogAction::Open, "Import knob settings") else { return };
        let result = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| config::from_text(&t));
        match result {
            Err(e) => { nwg::modal_error_message(&self.ui.s.win, "Can't import", &format!("{}:\n{e}", path.display())); }
            Ok(c) => {
                {
                    let mut s = self.state.borrow_mut();
                    s.draft = c.layers;
                    s.draft_sel = Some(0);
                }
                self.ui.s.gesture.set_selection(GESTURES.iter().position(|g| g.1 == c.gesture));
                self.ui.s.t_window.set_text(&c.tap_window_ms.to_string());
                self.refresh_list();
                self.load_fields();
                self.ui.s.status.set_text("Imported \u{2014} press Save to keep it");
            }
        }
    }

    fn export(&self) {
        self.commit_fields();
        let cfg = self.draft_config();
        if let Err(e) = config::validate(&cfg) { nwg::modal_error_message(&self.ui.s.win, "Can't export", &e); return; }
        let Some(mut path) = self.pick_file(nwg::FileDialogAction::Save, "Export knob settings") else { return };
        if path.extension().is_none() { path.set_extension("json"); }
        match config::save_to(&cfg, &path) {
            Ok(()) => self.ui.s.status.set_text("\u{2713} Exported"),
            Err(e) => { nwg::modal_error_message(&self.ui.s.win, "Can't export", &format!("{}: {e}", path.display())); }
        }
    }

    pub fn on_settings_event(&self, evt: nwg::Event, h: nwg::ControlHandle) {
        use nwg::Event as E;
        let ui = &self.ui.s;
        match evt {
            E::OnListBoxSelect if h == ui.list.handle => {
                self.commit_fields();
                self.state.borrow_mut().draft_sel = ui.list.selection();
                self.refresh_list();
                self.load_fields();
            }
            E::OnComboxBoxSelection if h == ui.preset.handle => self.apply_preset(),
            E::OnComboxBoxSelection if h == ui.add_app.handle => self.add_running_app(),
            E::OnButtonClick if h == ui.c_enabled.handle => { self.commit_fields(); self.refresh_list(); }
            E::OnButtonClick if h == ui.b_add.handle => self.edit_list(|d, _| { d.push(Layer { name: "New layer".into(), ..Default::default() }); d.len() - 1 }),
            E::OnButtonClick if h == ui.b_remove.handle => self.edit_list(|d, i| { if d.len() > 1 { d.remove(i); } i }),
            E::OnButtonClick if h == ui.b_up.handle => self.edit_list(|d, i| if i > 0 { d.swap(i, i - 1); i - 1 } else { i }),
            E::OnButtonClick if h == ui.b_down.handle => self.edit_list(|d, i| if i + 1 < d.len() { d.swap(i, i + 1); i + 1 } else { i }),
            E::OnButtonClick if h == ui.b_import.handle => self.import(),
            E::OnButtonClick if h == ui.b_export.handle => self.export(),
            E::OnButtonClick if h == ui.b_save.handle => self.save_settings(),
            E::OnButtonClick if h == ui.b_close.handle => { stop_hook(); ui.win.set_visible(false) }
            E::OnButtonClick => if let Some(i) = ui.rec.iter().position(|b| b.handle == h) { self.start_record(i) },
            E::OnWindowClose if h == ui.win.handle => stop_hook(),
            _ => {}
        }
    }
}
