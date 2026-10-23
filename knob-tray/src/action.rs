//! Action strings typed in the settings window, e.g. "wheel:-120", "ctrl+wheel:120", "next",
//! "ctrl+shift+z", "click:middle". Empty or "none" does nothing.
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, mouse_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP};

const CTRL: u8 = 0x11;
const SHIFT: u8 = 0x10;
const ALT: u8 = 0x12;
const WIN: u8 = 0x5B;

#[derive(Debug, PartialEq, Clone)]
pub enum Kind {
    Nothing,
    Key(u8),
    Wheel { delta: i32, horizontal: bool },
    Click { down: u32, up: u32 },
}

#[derive(Debug, PartialEq, Clone)]
pub struct Action { mods: Vec<u8>, kind: Kind }

pub const HELP: &str = "wheel:N, hwheel:N \u{2014} scroll (120 = one notch; negative = down / left)\r\n\
    click:left, click:middle, click:right\r\n\
    Keys: a\u{2013}z 0\u{2013}9 f1\u{2013}f24 up down left right pageup pagedown home end space enter tab esc \
    backspace delete insert plus [ ] , . ; / -\r\n\
    Media: next prev playpause stop volup voldown mute\r\n\
    Combine with ctrl, shift, alt, win using +   e.g. ctrl+wheel:120, ctrl+shift+z\r\n\
    Leave a box empty to do nothing.";

fn key_code(name: &str) -> Option<u8> {
    let b = name.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return Some(b[0].to_ascii_uppercase());
    }
    if b.len() == 1 {   // US-layout punctuation ('+' is the separator, so use "plus")
        let vk = match b[0] { b';' => 0xBA, b'=' => 0xBB, b',' => 0xBC, b'-' => 0xBD, b'.' => 0xBE, b'/' => 0xBF,
                              b'`' => 0xC0, b'[' => 0xDB, b'\\' => 0xDC, b']' => 0xDD, b'\'' => 0xDE, _ => 0 };
        if vk != 0 { return Some(vk); }
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) { return Some(0x6F + n); }
    }
    Some(match name {
        "up" => 0x26, "down" => 0x28, "left" => 0x25, "right" => 0x27,
        "pageup" => 0x21, "pagedown" => 0x22, "home" => 0x24, "end" => 0x23,
        "insert" => 0x2D, "delete" => 0x2E, "backspace" => 0x08, "tab" => 0x09,
        "enter" => 0x0D, "esc" => 0x1B, "space" => 0x20, "plus" => 0xBB, "minus" => 0xBD,
        "next" => 0xB0, "prev" => 0xB1, "stop" => 0xB2, "playpause" => 0xB3,
        "mute" => 0xAD, "voldown" => 0xAE, "volup" => 0xAF,
        _ => return None,
    })
}

fn extended(vk: u8) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0xAD..=0xB3)
}

pub fn parse(text: &str) -> Result<Action, String> {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() || text == "none" { return Ok(Action { mods: vec![], kind: Kind::Nothing }); }
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
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
    Ok(Action { mods, kind })
}

fn key(vk: u8, up: bool) {
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if extended(vk) { flags |= KEYEVENTF_EXTENDEDKEY; }
    unsafe { keybd_event(vk, 0, flags, 0) };
}

pub fn perform(a: &Action) {
    for &m in &a.mods { key(m, false); }
    match a.kind {
        Kind::Nothing => {}
        Kind::Key(vk) => { key(vk, false); key(vk, true); }
        Kind::Wheel { delta, horizontal } => unsafe { mouse_event(if horizontal { 0x1000 } else { 0x0800 }, 0, 0, delta, 0) },
        Kind::Click { down, up } => unsafe { mouse_event(down, 0, 0, 0, 0); mouse_event(up, 0, 0, 0, 0) },
    }
    for &m in a.mods.iter().rev() { key(m, true); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse(" ").unwrap().kind, Kind::Nothing);
        assert_eq!(parse("wheel:-120").unwrap(), Action { mods: vec![], kind: Kind::Wheel { delta: -120, horizontal: false } });
        assert_eq!(parse("Ctrl+wheel:120").unwrap(), Action { mods: vec![CTRL], kind: Kind::Wheel { delta: 120, horizontal: false } });
        assert_eq!(parse("hwheel:60").unwrap().kind, Kind::Wheel { delta: 60, horizontal: true });
        assert_eq!(parse("ctrl+shift+z").unwrap(), Action { mods: vec![CTRL, SHIFT], kind: Kind::Key(b'Z') });
        assert_eq!(parse("f13").unwrap().kind, Kind::Key(0x7C));
        assert_eq!(parse("next").unwrap().kind, Kind::Key(0xB0));
        assert_eq!(parse("ctrl+0").unwrap().kind, Kind::Key(b'0'));
        assert_eq!(parse("click:middle").unwrap().kind, Kind::Click { down: 0x20, up: 0x40 });
        assert_eq!(parse("]").unwrap().kind, Kind::Key(0xDD));
        assert_eq!(parse("ctrl+[").unwrap(), Action { mods: vec![CTRL], kind: Kind::Key(0xDB) });
        assert!(parse("ctrl+banana").is_err());
        assert!(parse("hyper+a").is_err());
        assert!(parse("wheel:lots").is_err());
    }
}
