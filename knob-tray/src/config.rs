//! Layers and settings, stored as JSON in %APPDATA%\RotaryKnob\config.json.
use crate::{action, apps, knob::Gesture, names};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::SystemTime};

pub const APP_NAME: &str = "RotaryKnob";

/// "Turning does" choices: name, clockwise, counter-clockwise, suggested press. Index 0 is "Custom".
pub const PAIRS: &[(&str, &str, &str, &str)] = &[
    ("Custom", "", "", ""),
    ("Scroll up / down", "Scroll Down", "Scroll Up", "Middle Click"),
    ("Scroll left / right", "Scroll Right", "Scroll Left", ""),
    ("Zoom", "Zoom +", "Zoom -", "Zoom Reset"),
    ("Volume", "Volume +", "Volume -", "Mute"),
    ("Tracks", "Next Track", "Previous Track", "Play/Pause"),
    ("Undo / redo", "Redo", "Undo", ""),
    ("Arrow up / down", "Down Arrow", "Up Arrow", "Enter"),
    ("Arrow left / right", "Right Arrow", "Left Arrow", "Enter"),
    ("Page up / down", "Page Down", "Page Up", "Home"),
    ("Browser tabs", "Ctrl + Tab", "Ctrl + Shift + Tab", "Ctrl + T"),
    ("Brush size", "]", "[", ""),
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Layer {
    pub name: String,
    pub enabled: bool,
    pub clockwise: String,
    pub counter_clockwise: String,
    pub press: String,
    pub shift_clockwise: String,         // tap, then turn
    pub shift_counter_clockwise: String,
    pub speed: f32,                      // scales scrolling, repeats keys
    pub accelerate: bool,
    pub max_per_sec: u32,                // 0 = unlimited
    pub apps: Vec<String>,               // programs that switch to this layer
}

impl Default for Layer {
    fn default() -> Self {
        Layer { name: String::new(), enabled: true, clockwise: String::new(), counter_clockwise: String::new(),
                press: String::new(), shift_clockwise: String::new(), shift_counter_clockwise: String::new(),
                speed: 1.0, accelerate: false, max_per_sec: 0, apps: vec![] }
    }
}

/// Ready-made layers offered by "New layer": what it does, and the layer.
pub fn templates() -> Vec<(&'static str, Layer)> {
    let l = |name: &str, cw: &str, ccw: &str, press: &str, scw: &str, sccw: &str, app: &str| Layer {
        name: name.into(), clockwise: cw.into(), counter_clockwise: ccw.into(), press: press.into(),
        shift_clockwise: scw.into(), shift_counter_clockwise: sccw.into(), apps: vec![app.into()], ..Default::default()
    };
    vec![
        ("Turn: Discord's volume in the Windows mixer \u{00B7} Press: mute Discord. Active when Discord is focused.",
         l("Discord", "Discord Volume +", "Discord Volume -", "Discord Mute", "", "", "discord")),
        ("Turn: YouTube volume \u{00B7} Tap + turn: skip 10 s \u{00B7} Press: play / pause. Active on YouTube tabs in any browser.",
         l("YouTube", "Up Arrow", "Down Arrow", "K", "L", "J", "title:youtube")),
    ]
}

impl Layer {
    fn from_pair(name: &str, i: usize) -> Layer {
        let p = PAIRS[i];
        Layer { name: name.into(), clockwise: p.1.into(), counter_clockwise: p.2.into(), press: p.3.into(), ..Default::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    #[serde(rename = "double_tap_ms")]   // name kept so older config files still load
    pub tap_window_ms: u64,
    pub gesture: Gesture,
    pub auto_switch: bool,
    pub layers: Vec<Layer>,
}

impl Default for Config {
    fn default() -> Self {
        Config { tap_window_ms: 350, gesture: Gesture::DoubleTap, auto_switch: true,
                 layers: vec![Layer::from_pair("Scroll", 1), Layer::from_pair("Zoom", 3), Layer::from_pair("Media", 5)] }
    }
}

/// A layer's actions, parsed once.
#[derive(Debug)]
pub struct Parsed { pub cw: action::Action, pub ccw: action::Action, pub press: action::Action,
                    pub shift_cw: action::Action, pub shift_ccw: action::Action }

impl Parsed { pub fn has_shift(&self) -> bool { !self.shift_cw.is_empty() || !self.shift_ccw.is_empty() } }

/// Everything Save and loading must reject, with a message naming the layer and field.
pub fn validate(c: &Config) -> Result<Vec<Parsed>, String> {
    if c.layers.is_empty() { return Err("Add at least one layer.".into()); }
    if !c.layers.iter().any(|l| l.enabled) { return Err("At least one layer must be enabled.".into()); }
    if !(100..=1000).contains(&c.tap_window_ms) { return Err("The tap window must be between 100 and 1000 ms.".into()); }
    let lists: Vec<Vec<String>> = c.layers.iter().map(|l| l.apps.clone()).collect();
    if let Some((app, a, b)) = apps::duplicate(&lists) {
        return Err(format!("'{app}' is listed on both '{}' and '{}'. An app can only switch to one layer.", c.layers[a].name, c.layers[b].name));
    }
    c.layers.iter().map(|l| {
        if l.name.trim().is_empty() { return Err("Every layer needs a name.".to_string()); }
        if !(0.1..=10.0).contains(&l.speed) { return Err(format!("Layer '{}': speed must be between 0.1 and 10.", l.name)); }
        if l.max_per_sec > 50 { return Err(format!("Layer '{}': max per second must be between 0 (no limit) and 50.", l.name)); }
        let p = |what: &str, s: &str| names::parse(s).map_err(|e| format!("Layer '{}', {what}: {e}", l.name));
        Ok(Parsed { cw: p("turn clockwise", &l.clockwise)?, ccw: p("turn counter-clockwise", &l.counter_clockwise)?,
                    press: p("press", &l.press)?, shift_cw: p("tap + turn clockwise", &l.shift_clockwise)?,
                    shift_ccw: p("tap + turn counter-clockwise", &l.shift_counter_clockwise)? })
    }).collect()
}

pub fn path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join(APP_NAME).join("config.json")
}

pub fn from_text(text: &str) -> Result<Config, String> {
    let mut c: Config = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;  // editors may add a BOM
    for l in &mut c.layers { l.apps = l.apps.iter().map(|a| apps::normalize(a)).filter(|a| !a.is_empty()).collect(); }
    validate(&c)?;
    Ok(c)
}

/// Missing file -> defaults. An unreadable file -> defaults plus the reason, so it isn't silently replaced.
pub fn load() -> (Config, Option<String>) {
    match std::fs::read_to_string(path()) {
        Err(_) => (Config::default(), None),
        Ok(text) => match from_text(&text) { Ok(c) => (c, None), Err(e) => (Config::default(), Some(e)) },
    }
}

pub fn save_to(c: &Config, p: &std::path::Path) -> std::io::Result<()> {
    if let Some(dir) = p.parent() { std::fs::create_dir_all(dir)?; }
    std::fs::write(p, serde_json::to_string_pretty(c).unwrap())
}

pub fn modified() -> Option<SystemTime> { std::fs::metadata(path()).and_then(|m| m.modified()).ok() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_files_still_load() {
        // written by the previous version: no gesture, enabled, speed or tap+turn fields
        let old = "\u{feff}{\"double_tap_ms\":300,\"auto_switch\":false,\"layers\":[{\"name\":\"S\",\"clockwise\":\"wheel:-120\",\
                   \"counter_clockwise\":\"wheel:120\",\"press\":\"\",\"apps\":[\"Chrome.EXE\"]}]}";
        let c = from_text(old).unwrap();
        assert_eq!((c.tap_window_ms, c.gesture, c.auto_switch), (300, Gesture::DoubleTap, false));
        let l = &c.layers[0];
        assert!(l.enabled && l.speed == 1.0 && !l.accelerate && l.shift_clockwise.is_empty());
        assert_eq!(l.apps, vec!["chrome"]);
        assert_eq!(from_text(&serde_json::to_string(&c).unwrap()).unwrap(), c);   // round-trips
    }

    #[test]
    fn templates_are_valid() {
        let layers: Vec<Layer> = templates().into_iter().map(|t| t.1).collect();
        assert!(validate(&Config { layers, ..Config::default() }).is_ok());
    }

    #[test]
    fn rejects_bad_settings() {
        let mut c = Config::default();
        assert!(validate(&c).is_ok());
        for l in &mut c.layers { l.enabled = false; }
        assert!(validate(&c).unwrap_err().contains("enabled"));
        let mut c = Config::default();
        c.layers[0].speed = 0.0;
        assert!(validate(&c).unwrap_err().contains("speed"));
        let mut c = Config::default();
        c.layers[1].shift_clockwise = "ctrl+banana".into();
        assert!(validate(&c).unwrap_err().contains("tap + turn clockwise"));
        assert!(from_text("{\"layers\": [ oops").is_err());
    }
}
