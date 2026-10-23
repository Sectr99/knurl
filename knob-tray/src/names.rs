//! Action names as written in settings: keys as they appear on the keyboard joined with " + "
//! ("Ctrl + Shift + Z", "Page Up", "F5"), function names ("Zoom +", "Scroll Down", "Volume -", "Copy"),
//! "Open …" and "Type …". Steps are separated with ", ". Matching ignores case and spaces, and the older
//! syntax (ctrl+z, wheel:-120, next) is still accepted.
use crate::action::{self, Action};

/// Function names and what they do. Modifiers may go in front: "Shift + Scroll Up".
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("Scroll Up", "wheel:120"), ("Scroll Down", "wheel:-120"), ("Scroll Left", "hwheel:-120"), ("Scroll Right", "hwheel:120"),
    ("Zoom +", "ctrl+wheel:120"), ("Zoom -", "ctrl+wheel:-120"), ("Zoom Reset", "ctrl+0"),
    ("Volume +", "volup"), ("Volume -", "voldown"), ("Mute", "mute"),
    ("Next Track", "next"), ("Previous Track", "prev"), ("Play/Pause", "playpause"), ("Stop", "stop"),
    ("Left Click", "click:left"), ("Right Click", "click:right"), ("Middle Click", "click:middle"),
    ("Double Click", "click:left; click:left"),
    ("Copy", "ctrl+c"), ("Cut", "ctrl+x"), ("Paste", "ctrl+v"), ("Undo", "ctrl+z"), ("Redo", "ctrl+y"),
    ("Select All", "ctrl+a"), ("Save", "ctrl+s"),
];

/// Key names as written on keys -> the syntax's key name. Compared without spaces, case-insensitively.
const KEY_NAMES: &[(&str, &str)] = &[
    ("Page Up", "pageup"), ("Page Down", "pagedown"), ("PgUp", "pageup"), ("PgDn", "pagedown"),
    ("Up Arrow", "up"), ("Down Arrow", "down"), ("Left Arrow", "left"), ("Right Arrow", "right"),
    ("Arrow Up", "up"), ("Arrow Down", "down"), ("Arrow Left", "left"), ("Arrow Right", "right"),
    ("Escape", "esc"), ("Return", "enter"), ("Del", "delete"), ("Ins", "insert"), ("Spacebar", "space"),
    ("Control", "ctrl"), ("Windows", "win"), ("Minus", "-"), ("Comma", ","), ("Period", "."),
];

/// How each key is written back (Record button, suggestions). Media keys use their function names.
fn key_display(vk: u8) -> Option<String> {
    Some(match vk {
        b'A'..=b'Z' | b'0'..=b'9' => (vk as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x21 => "Page Up".into(), 0x22 => "Page Down".into(),
        0x26 => "Up Arrow".into(), 0x28 => "Down Arrow".into(), 0x25 => "Left Arrow".into(), 0x27 => "Right Arrow".into(),
        0x24 => "Home".into(), 0x23 => "End".into(), 0x2D => "Insert".into(), 0x2E => "Delete".into(),
        0x08 => "Backspace".into(), 0x09 => "Tab".into(), 0x0D => "Enter".into(), 0x1B => "Esc".into(), 0x20 => "Space".into(),
        0xBB => "Plus".into(),
        0xB0 => "Next Track".into(), 0xB1 => "Previous Track".into(), 0xB2 => "Stop".into(), 0xB3 => "Play/Pause".into(),
        0xAD => "Mute".into(), 0xAE => "Volume -".into(), 0xAF => "Volume +".into(),
        _ => action::key_name(vk)?,                       // punctuation: [ ] , . ; / - ' ` \
    })
}

/// "Ctrl + Shift + Z" for a recorded key and the modifiers held with it.
pub fn combo_display(vk: u8, ctrl: bool, shift: bool, alt: bool, win: bool) -> Option<String> {
    let mut s = String::new();
    for (on, m) in [(ctrl, "Ctrl + "), (shift, "Shift + "), (alt, "Alt + "), (win, "Win + ")] { if on { s.push_str(m); } }
    Some(s + &key_display(vk)?)
}

fn compact(s: &str) -> String { s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase() }

fn is_modifier(m: &str) -> Option<&'static str> {
    match m { "ctrl" | "control" => Some("ctrl"), "shift" => Some("shift"), "alt" => Some("alt"), "win" | "windows" => Some("win"), _ => None }
}

/// "Zoom +", "Shift + Scroll Up" -> syntax.
fn function(c: &str) -> Option<String> {
    FUNCTIONS.iter().find_map(|(name, syntax)| {
        let name = compact(name);
        let prefix = c.strip_suffix(&name)?;
        if prefix.is_empty() { return Some(syntax.to_string()); }
        if syntax.contains(';') { return None; }          // no modifiers on multi-step functions
        let mods: Option<Vec<&str>> = prefix.strip_suffix('+')?.split('+').map(is_modifier).collect();
        Some(mods?.iter().map(|m| format!("{m}+")).collect::<String>() + syntax)
    })
}

/// "Ctrl + Shift + Z", "Page Up", "Ctrl + -" -> syntax.
fn keys(c: &str) -> Option<String> {
    let parts: Vec<&str> = match c.strip_suffix("++") {        // "Ctrl + +": the key is '+' itself
        Some(head) => head.split('+').chain(["plus"]).collect(),
        None => c.split('+').collect(),
    };
    let (key, mods) = parts.split_last()?;
    let key = KEY_NAMES.iter().find(|k| compact(k.0) == *key).map_or(*key, |k| k.1);
    let mods: Option<Vec<&str>> = mods.iter().map(|m| is_modifier(m)).collect();
    let syntax = mods?.iter().map(|m| format!("{m}+")).collect::<String>() + key;
    action::parse(&syntax).ok().map(|_| syntax)
}

fn open_or_type(step: &str) -> Option<String> {
    let lower = step.to_lowercase();
    if let Some(text) = lower.strip_prefix("type ").map(|_| &step[5..]) { return Some(format!("type:{text}")); }
    let target = lower.strip_prefix("open ").map(|_| step[5..].trim())?;
    if target.is_empty() { return None; }
    let site = target.contains('.') && !target.contains([' ', '\\']) && !target.to_lowercase().ends_with(".exe") && !target.contains("://");
    Some(format!("run:{}", if site { format!("https://{target}") } else { target.to_string() }))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + (ca != b[j]) as usize).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn unknown(step: &str) -> String {
    let c = compact(step);
    let known = FUNCTIONS.iter().map(|f| f.0.to_string())
        .chain(KEY_NAMES.iter().map(|k| k.0.to_string()))
        .chain(["Home", "End", "Insert", "Delete", "Backspace", "Tab", "Enter", "Esc", "Space", "Plus"].map(String::from));
    match known.map(|k| (edit_distance(&c, &compact(&k)), k)).min() {
        Some((d, k)) if d <= (c.len() / 4).max(2) => format!("Unknown: \u{2018}{step}\u{2019}. Did you mean \u{2018}{k}\u{2019}?"),
        _ => format!("Unknown: \u{2018}{step}\u{2019}. Use key names like Ctrl + Z or functions like Zoom +."),
    }
}

fn step_syntax(step: &str) -> Result<String, String> {
    let step = step.trim();
    if step.is_empty() { return Ok(String::new()); }
    let c = compact(step);
    open_or_type(step)
        .or_else(|| function(&c))
        .or_else(|| keys(&c))
        .or_else(|| action::parse(step).ok().map(|_| step.to_string()))   // older syntax
        .ok_or_else(|| unknown(step))
}

/// Settings text -> action syntax. Steps are separated by ", " or ';'; "Type …" takes the rest of the box.
pub fn to_syntax(input: &str) -> Result<String, String> {
    let mut out = vec![];
    let mut rest = input.trim();
    while !rest.is_empty() {
        if rest.to_lowercase().starts_with("type ") || rest.to_lowercase().starts_with("type:") {
            out.push(if rest.as_bytes()[4] == b':' { rest.to_string() } else { step_syntax(rest)? });
            break;
        }
        let cut = [", ", ";"].iter().filter_map(|sep| rest.find(sep).map(|i| (i, sep.len()))).min();
        let (head, tail) = match cut { Some((i, n)) => (&rest[..i], &rest[i + n..]), None => (rest, "") };
        let s = step_syntax(head)?;
        if !s.is_empty() { out.push(s); }
        rest = tail.trim_start();
    }
    Ok(out.join("; "))
}

/// Settings text -> runnable action.
pub fn parse(input: &str) -> Result<Action, String> { action::parse(&to_syntax(input)?) }

/// True when two boxes do the same thing, however they're written.
pub fn same(a: &str, b: &str) -> bool { matches!((parse(a), parse(b)), (Ok(x), Ok(y)) if x == y) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_to_actions() {
        for (input, want) in [
            ("Zoom +", "ctrl+wheel:120"), ("zoom -", "ctrl+wheel:-120"), ("Zoom Reset", "ctrl+0"), ("ZoomReset", "ctrl+0"),
            ("Scroll Down", "wheel:-120"), ("scroll up", "wheel:120"), ("Scroll Right", "hwheel:120"),
            ("Volume +", "volup"), ("Volume -", "voldown"), ("Mute", "mute"), ("Next Track", "next"), ("Play/Pause", "playpause"),
            ("Middle Click", "click:middle"), ("Double Click", "click:left; click:left"), ("Copy", "ctrl+c"), ("Select All", "ctrl+a"),
            ("Shift + Scroll Up", "shift+wheel:120"), ("Ctrl + Middle Click", "ctrl+click:middle"), ("Alt + Zoom +", "alt+ctrl+wheel:120"),
            ("Ctrl + Shift + Z", "ctrl+shift+z"), ("Alt + Tab", "alt+tab"), ("F5", "f5"), ("Page Up", "pageup"), ("page down", "pagedown"),
            ("Up Arrow", "up"), ("Enter", "enter"), ("Escape", "esc"), ("Ctrl + -", "ctrl+-"), ("Ctrl + Plus", "ctrl+plus"),
            ("Ctrl + +", "ctrl+plus"), ("[", "["), ("Win + D", "win+d"),
            ("Open notepad", "run:notepad"), ("open youtube.com", "run:https://youtube.com"), ("Type Hello, World", "type:Hello, World"),
            ("wheel:-120", "wheel:-120"), ("ctrl+z", "ctrl+z"), ("next", "next"), ("", ""),
        ] {
            assert_eq!(to_syntax(input).as_deref(), Ok(want), "{input}");
        }
    }

    #[test]
    fn sequences() {
        assert_eq!(to_syntax("Copy, Alt + Tab, Paste").unwrap(), "ctrl+c; alt+tab; ctrl+v");
        assert_eq!(to_syntax("Ctrl + T, Type hello, world").unwrap(), "ctrl+t; type:hello, world");
        assert_eq!(to_syntax("Ctrl + ,").unwrap(), "ctrl+,");                  // a bare comma key isn't a separator
    }

    #[test]
    fn unknown_names() {
        assert!(to_syntax("Zom +").unwrap_err().contains("Did you mean \u{2018}Zoom +\u{2019}"));
        assert!(to_syntax("Scrol Down").unwrap_err().contains("Scroll Down"));
        assert!(to_syntax("Hyper + Z").is_err());
        assert!(to_syntax("Ctrl").is_err());                                   // a modifier alone isn't an action
        assert!(to_syntax("Ctrl + Double Click").is_err());
        assert!(to_syntax("make a sandwich").unwrap_err().contains("key names"));
    }

    #[test]
    fn display_round_trips() {
        assert_eq!(combo_display(b'K', true, true, false, false).as_deref(), Some("Ctrl + Shift + K"));
        for vk in [b'A', b'7', 0x7C, 0x21, 0x26, 0x0D, 0x1B, 0x20, 0xBB, 0xDB, 0xB0, 0xAF] {
            let shown = combo_display(vk, true, false, false, false).unwrap();
            assert!(parse(&shown).is_ok(), "{shown} should parse back");
        }
        assert!(same("Zoom +", "ctrl+wheel:120") && same("Next Track", "next") && !same("Scroll Up", "Scroll Down"));
    }
}
