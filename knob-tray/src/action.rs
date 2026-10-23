//! Action strings typed in the settings window, e.g. "wheel:-120", "ctrl+wheel:120", "next", "ctrl+shift+z",
//! "click:middle", "run:notepad", "type:hello". Steps chain with ';'. Empty or "none" does nothing.
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::*,
    Shell::ShellExecuteW,
    WindowsAndMessaging::SW_SHOWNORMAL,
};

pub(crate) const CTRL: u8 = 0x11;
pub(crate) const SHIFT: u8 = 0x10;
pub(crate) const ALT: u8 = 0x12;
pub(crate) const WIN: u8 = 0x5B;

#[derive(Debug, PartialEq, Clone)]
pub enum Kind {
    Key(u8),
    Wheel { delta: i32, horizontal: bool },
    Click { down: u32, up: u32 },
    Run(String),
    Type(String),
}

#[derive(Debug, PartialEq, Clone)]
pub struct Step { pub(crate) mods: Vec<u8>, pub(crate) kind: Kind }

/// A sequence of steps; empty does nothing.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct Action(pub(crate) Vec<Step>);

impl Action { pub fn is_empty(&self) -> bool { self.0.is_empty() } }

pub(crate) const NAMED: &[(&str, u8)] = &[
    ("up", 0x26), ("down", 0x28), ("left", 0x25), ("right", 0x27), ("pageup", 0x21), ("pagedown", 0x22),
    ("home", 0x24), ("end", 0x23), ("insert", 0x2D), ("delete", 0x2E), ("backspace", 0x08), ("tab", 0x09),
    ("enter", 0x0D), ("esc", 0x1B), ("space", 0x20), ("plus", 0xBB), ("next", 0xB0), ("prev", 0xB1),
    ("stop", 0xB2), ("playpause", 0xB3), ("mute", 0xAD), ("voldown", 0xAE), ("volup", 0xAF),
];
const PUNCT: &[(u8, u8)] = &[(b';', 0xBA), (b'=', 0xBB), (b',', 0xBC), (b'-', 0xBD), (b'.', 0xBE), (b'/', 0xBF),
                             (b'`', 0xC0), (b'[', 0xDB), (b'\\', 0xDC), (b']', 0xDD), (b'\'', 0xDE)];

pub(crate) fn key_code(name: &str) -> Option<u8> {
    let b = name.as_bytes();
    if b.len() == 1 {
        if b[0].is_ascii_lowercase() || b[0].is_ascii_digit() { return Some(b[0].to_ascii_uppercase()); }
        if let Some(&(_, vk)) = PUNCT.iter().find(|p| p.0 == b[0]) { return Some(vk); }   // '+' is the separator: use "plus"
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) { return Some(0x6F + n); }
    }
    NAMED.iter().find(|n| n.0 == name).map(|n| n.1)
}

/// Inverse of `key_code`, for the key recorder. None for keys the syntax can't express.
pub fn key_name(vk: u8) -> Option<String> {
    match vk {
        b'A'..=b'Z' | b'0'..=b'9' => Some((vk as char).to_ascii_lowercase().to_string()),
        0x70..=0x87 => Some(format!("f{}", vk - 0x6F)),
        _ => NAMED.iter().find(|n| n.1 == vk).map(|n| n.0.to_string())
             .or_else(|| PUNCT.iter().find(|p| p.1 == vk && p.0 != b'=').map(|p| (p.0 as char).to_string())),
    }
}

fn parse_step(text: &str) -> Result<Step, String> {
    let lower = text.trim().to_ascii_lowercase();
    if let Some(target) = lower.strip_prefix("run:") {
        let original = text.trim()[4..].trim();          // keep the path's own case
        if target.trim().is_empty() { return Err("run: needs a program, file or URL".into()); }
        return Ok(Step { mods: vec![], kind: Kind::Run(original.to_string()) });
    }
    let parts: Vec<&str> = lower.split('+').map(str::trim).collect();
    let (last, mod_names) = parts.split_last().unwrap();
    let mut mods = vec![];
    for m in mod_names {
        mods.push(match *m { "ctrl" | "control" => CTRL, "shift" => SHIFT, "alt" => ALT, "win" => WIN,
                             _ => return Err(format!("'{m}' is not a modifier (ctrl, shift, alt, win)")) });
    }
    let kind = if let Some(n) = last.strip_prefix("wheel:").or(last.strip_prefix("hwheel:")) {
        let delta = n.parse().map_err(|_| format!("'{n}' is not a number"))?;
        Kind::Wheel { delta, horizontal: last.starts_with('h') }
    } else if let Some(b) = last.strip_prefix("click:") {
        let (down, up) = match b { "left" => (0x02, 0x04), "right" => (0x08, 0x10), "middle" => (0x20, 0x40),
                                   _ => return Err(format!("'{b}' should be left, middle or right")) };
        Kind::Click { down, up }
    } else {
        Kind::Key(key_code(last).ok_or_else(|| format!("unknown key '{last}'"))?)
    };
    Ok(Step { mods, kind })
}

pub fn parse(text: &str) -> Result<Action, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") { return Ok(Action::default()); }
    let mut steps = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        let seg = rest.trim_start();
        if seg.len() >= 5 && seg[..5].eq_ignore_ascii_case("type:") {   // type: takes the rest, ';' included
            steps.push(Step { mods: vec![], kind: Kind::Type(seg[5..].to_string()) });
            break;
        }
        let (head, tail) = seg.split_once(';').unwrap_or((seg, ""));
        if !head.trim().is_empty() { steps.push(parse_step(head)?); }
        rest = tail;
    }
    Ok(Action(steps))
}

fn key(vk: u8, up: bool) {
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0xAD..=0xB3) { flags |= KEYEVENTF_EXTENDEDKEY; }
    unsafe { keybd_event(vk, 0, flags, 0) };
}

fn type_text(s: &str) {
    for unit in s.encode_utf16() {
        for up in [false, true] {
            let ki = KEYBDINPUT { wVk: 0, wScan: unit, dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 };
            let input = INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } };
            unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
        }
    }
}

fn run(target: &str) {
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    unsafe { ShellExecuteW(0, w("open").as_ptr(), w(target).as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

/// `mult` scales scrolling and repeats keys/clicks (speed and acceleration); run/type happen once.
pub fn perform(a: &Action, mult: f32) {
    let repeats = (mult.round() as usize).max(1);
    for s in &a.0 {
        match &s.kind {
            Kind::Run(t) => { run(t); continue; }
            Kind::Type(t) => { type_text(t); continue; }
            _ => {}
        }
        for &m in &s.mods { key(m, false); }
        match s.kind {
            Kind::Key(vk) => for _ in 0..repeats { key(vk, false); key(vk, true); },
            Kind::Wheel { delta, horizontal } =>
                unsafe { mouse_event(if horizontal { 0x1000 } else { 0x0800 }, 0, 0, (delta as f32 * mult).round() as i32, 0) },
            Kind::Click { down, up } => for _ in 0..repeats { unsafe { mouse_event(down, 0, 0, 0, 0); mouse_event(up, 0, 0, 0, 0) } },
            Kind::Run(_) | Kind::Type(_) => {}
        }
        for &m in s.mods.iter().rev() { key(m, true); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn one(s: &str) -> Step { let a = parse(s).unwrap(); assert_eq!(a.0.len(), 1, "{s}"); a.0[0].clone() }

    #[test]
    fn parses() {
        assert!(parse(" ").unwrap().is_empty());
        assert_eq!(one("wheel:-120"), Step { mods: vec![], kind: Kind::Wheel { delta: -120, horizontal: false } });
        assert_eq!(one("Ctrl+wheel:120"), Step { mods: vec![CTRL], kind: Kind::Wheel { delta: 120, horizontal: false } });
        assert_eq!(one("hwheel:60").kind, Kind::Wheel { delta: 60, horizontal: true });
        assert_eq!(one("ctrl+shift+z"), Step { mods: vec![CTRL, SHIFT], kind: Kind::Key(b'Z') });
        assert_eq!(one("f13").kind, Kind::Key(0x7C));
        assert_eq!(one("next").kind, Kind::Key(0xB0));
        assert_eq!(one("click:middle").kind, Kind::Click { down: 0x20, up: 0x40 });
        assert_eq!(one("ctrl+[").kind, Kind::Key(0xDB));
        assert_eq!(one("run:C:\\Tools\\My App.exe").kind, Kind::Run("C:\\Tools\\My App.exe".into()));
        assert!(parse("ctrl+banana").is_err());
        assert!(parse("hyper+a").is_err());
        assert!(parse("wheel:lots").is_err());
        assert!(parse("run:").is_err());
    }

    #[test]
    fn sequences_and_type() {
        assert_eq!(parse("ctrl+c; alt+tab ;ctrl+v").unwrap().0.len(), 3);
        let a = parse("ctrl+a; type:Hi; there").unwrap();
        assert_eq!(a.0.len(), 2);
        assert_eq!(a.0[1].kind, Kind::Type("Hi; there".into()));     // type: keeps the rest, ';' included
        assert_eq!(parse("type:  spaced ").unwrap().0[0].kind, Kind::Type("  spaced".into()));   // box is trimmed, inner spaces kept
    }

    #[test]
    fn key_names_round_trip() {
        for name in ["a", "7", "f13", "pageup", "playpause", "[", "/", "plus"] {
            assert_eq!(key_name(key_code(name).unwrap()).as_deref(), Some(name));
        }
        assert_eq!(key_name(0x10), None);                            // bare Shift isn't an action
    }
}
