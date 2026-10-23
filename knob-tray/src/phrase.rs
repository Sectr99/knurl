//! Plain-English actions: "scroll down 3", "zoom in", "next track", "press ctrl shift z", "open notepad",
//! "type hello", "copy then paste". Translated to the action syntax (which is still accepted as-is), and
//! described back in plain English for the settings preview.
use crate::action::{self, Action, Kind, ALT, CTRL, SHIFT, WIN};

/// Everyday phrases and the action they mean. The first phrase for an action is its name in previews.
const PHRASES: &[(&str, &str)] = &[
    ("copy", "ctrl+c"), ("cut", "ctrl+x"), ("paste", "ctrl+v"), ("undo", "ctrl+z"), ("redo", "ctrl+y"),
    ("select all", "ctrl+a"), ("save", "ctrl+s"), ("find", "ctrl+f"), ("search", "ctrl+f"), ("print", "ctrl+p"),
    ("new tab", "ctrl+t"), ("close tab", "ctrl+w"), ("reopen tab", "ctrl+shift+t"), ("reopen closed tab", "ctrl+shift+t"),
    ("next tab", "ctrl+tab"), ("previous tab", "ctrl+shift+tab"), ("prev tab", "ctrl+shift+tab"),
    ("new window", "ctrl+n"), ("close window", "alt+f4"), ("switch window", "alt+tab"), ("switch app", "alt+tab"),
    ("refresh", "f5"), ("reload", "f5"), ("go back", "alt+left"), ("back", "alt+left"), ("go forward", "alt+right"),
    ("forward", "alt+right"), ("full screen", "f11"), ("fullscreen", "f11"), ("show desktop", "win+d"),
    ("lock", "win+l"), ("lock screen", "win+l"), ("lock pc", "win+l"), ("screenshot", "win+shift+s"), ("snip", "win+shift+s"),
    ("task manager", "ctrl+shift+esc"), ("bigger brush", "]"), ("brush bigger", "]"), ("smaller brush", "["), ("brush smaller", "["),
    ("zoom in", "ctrl+wheel:120"), ("zoom out", "ctrl+wheel:-120"), ("reset zoom", "ctrl+0"), ("zoom reset", "ctrl+0"), ("actual size", "ctrl+0"),
    ("next track", "next"), ("next song", "next"), ("skip", "next"), ("skip track", "next"),
    ("previous track", "prev"), ("previous song", "prev"), ("prev track", "prev"), ("previous", "prev"),
    ("play/pause", "playpause"), ("play pause", "playpause"), ("play", "playpause"), ("pause", "playpause"),
    ("toggle playback", "playpause"), ("stop playback", "stop"),
    ("volume up", "volup"), ("louder", "volup"), ("volume down", "voldown"), ("quieter", "voldown"),
    ("mute", "mute"), ("unmute", "mute"), ("toggle mute", "mute"),
    ("left click", "click:left"), ("click", "click:left"), ("right click", "click:right"), ("middle click", "click:middle"),
    ("double click", "click:left; click:left"),
    ("up arrow", "up"), ("down arrow", "down"), ("left arrow", "left"), ("right arrow", "right"),
];

const NOTHING: &[&str] = &["nothing", "none", "do nothing", "no action"];
const FILLERS: &[&str] = &["press", "hit", "tap", "hold", "holding", "and", "while", "pressing", "the", "key", "keys", "with"];
const MULTI_WORD: &[(&str, &str)] = &[
    (" page up ", " pageup "), (" page down ", " pagedown "), (" back space ", " backspace "), (" space bar ", " space "),
    (" up arrow ", " up "), (" arrow up ", " up "), (" down arrow ", " down "), (" arrow down ", " down "),
    (" left arrow ", " left "), (" arrow left ", " left "), (" right arrow ", " right "), (" arrow right ", " right "),
    (" left bracket ", " [ "), (" open bracket ", " [ "), (" right bracket ", " ] "), (" close bracket ", " ] "),
];
const SYNONYMS: &[(&str, &str)] = &[
    ("control", "ctrl"), ("ctl", "ctrl"), ("option", "alt"), ("opt", "alt"), ("windows", "win"), ("super", "win"),
    ("cmd", "win"), ("command", "win"), ("meta", "win"), ("escape", "esc"), ("return", "enter"), ("del", "delete"),
    ("ins", "insert"), ("spacebar", "space"), ("comma", ","), ("period", "."), ("dot", "."), ("slash", "/"),
    ("semicolon", ";"), ("backslash", "\\"), ("quote", "'"), ("apostrophe", "'"), ("backtick", "`"), ("grave", "`"),
    ("equals", "="), ("minus", "-"), ("dash", "-"), ("hyphen", "-"),
];
const NUMBERS: &[&str] = &["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
const EXAMPLES: &str = "Try e.g. \u{2018}scroll down\u{2019}, \u{2018}press ctrl z\u{2019} or \u{2018}open notepad\u{2019}.";

fn normalize(s: &str) -> String {
    let s = s.trim().trim_end_matches(['.', '!']).to_lowercase();
    let s = s.strip_prefix("please ").unwrap_or(&s).to_string();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn number(w: &str) -> Option<i32> {
    w.parse().ok().or_else(|| NUMBERS.iter().position(|n| *n == w).map(|i| i as i32))
}

/// "scroll down", "scroll up 3", "scroll left by two notches"
fn scroll(n: &str) -> Option<String> {
    let w: Vec<&str> = n.split(' ').filter(|w| *w != "by").collect();
    if w.first() != Some(&"scroll") { return None; }
    let count = match w.get(2) { None => 1, Some(x) => number(x)? };
    if w.len() > 4 || (w.len() == 4 && !matches!(w[3], "times" | "notches" | "notch" | "lines" | "line" | "clicks" | "click" | "steps" | "step")) { return None; }
    if !(1..=20).contains(&count) { return None; }
    Some(match *w.get(1)? {
        "down" => format!("wheel:{}", -120 * count), "up" => format!("wheel:{}", 120 * count),
        "right" => format!("hwheel:{}", 120 * count), "left" => format!("hwheel:{}", -120 * count),
        _ => return None,
    })
}

/// "press ctrl shift z", "ctrl + z", "hold shift and press tab", "ctrl plus z"
fn combo(n: &str) -> Option<String> {
    let mut s = format!(" {n} ");
    for (from, to) in MULTI_WORD { s = s.replace(from, to); }
    let tokens: Vec<String> = s.split([' ', '+']).filter(|t| !t.is_empty() && !FILLERS.contains(t))
        .map(|t| SYNONYMS.iter().find(|p| p.0 == t).map_or(t, |p| p.1).to_string()).collect();
    let (key, mods) = tokens.split_last()?;
    let mods: Vec<&str> = mods.iter().map(String::as_str).filter(|m| *m != "plus").collect();   // "ctrl plus z"
    if !mods.iter().all(|m| matches!(*m, "ctrl" | "shift" | "alt" | "win")) || matches!(key.as_str(), "ctrl" | "shift" | "alt" | "win") {
        return None;
    }
    action::key_code(key)?;
    Some(mods.iter().map(|m| format!("{m}+")).collect::<String>() + key)
}

/// "open notepad", "launch spotify", "go to youtube.com"
fn open(orig: &str, n: &str) -> Option<String> {
    let verb = ["open ", "launch ", "start ", "run ", "go to ", "visit "].into_iter().find(|v| n.starts_with(v))?;
    let target = orig.trim()[verb.len()..].trim();
    if target.is_empty() { return None; }
    let looks_like_site = target.contains('.') && !target.contains([' ', '\\']) && !target.to_lowercase().ends_with(".exe") && !target.contains("://");
    Some(format!("run:{}", if looks_like_site { format!("https://{target}") } else { target.to_string() }))
}

/// One step in plain English (or syntax) -> syntax.
fn single(orig: &str) -> Result<String, String> {
    let orig = orig.trim();
    if action::parse(orig).is_ok() { return Ok(orig.to_string()); }
    let n = normalize(orig);
    if NOTHING.contains(&n.as_str()) { return Ok(String::new()); }
    if let Some(p) = PHRASES.iter().find(|p| p.0 == n) { return Ok(p.1.to_string()); }
    for verb in ["type ", "write "] {
        if n.starts_with(verb) { return Ok(format!("type:{}", &orig[verb.len()..])); }
    }
    if let Some(s) = open(orig, &n).or_else(|| scroll(&n)).or_else(|| combo(&n)) { return Ok(s); }
    Err(not_understood(orig))
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

fn not_understood(orig: &str) -> String {
    let n = normalize(orig);
    let candidates = PHRASES.iter().map(|p| p.0).chain(["scroll down", "scroll up", "scroll left", "scroll right"]);
    let best = candidates.map(|c| (edit_distance(&n, c), c)).min();
    match best {
        Some((d, c)) if d <= (n.len() / 4).max(2) => format!("Didn\u{2019}t understand \u{2018}{orig}\u{2019}. Did you mean \u{2018}{c}\u{2019}?"),
        _ => format!("Didn\u{2019}t understand \u{2018}{orig}\u{2019}. {EXAMPLES}"),
    }
}

/// Plain English (or syntax) -> action syntax. Steps chain with "then", commas or ';'; "type ..." takes the rest.
pub fn to_syntax(input: &str) -> Result<String, String> {
    let t = input.trim();
    if t.is_empty() || action::parse(t).is_ok() { return Ok(t.to_string()); }
    let mut steps = vec![];
    let mut rest = t;
    while !rest.trim().is_empty() {
        let seg = rest.trim_start();
        let lower = seg.to_lowercase();
        if lower.starts_with("type ") || lower.starts_with("write ") || lower.starts_with("type:") {
            steps.push(single(seg)?);                                   // the text keeps its commas and "then"s
            break;
        }
        let cut = [";", ", and then ", " and then ", ", then ", " then ", ","].iter()
            .filter_map(|sep| lower.find(sep).map(|i| (i, sep.len())))
            .min_by_key(|&(i, len)| (i, std::cmp::Reverse(len)));     // earliest, and longest there (", and then " over ",")
        let (head, tail) = match cut { Some((i, len)) => (&seg[..i], &seg[i + len..]), None => (seg, "") };
        // a step may itself be "copy and paste": only split on "and" if the whole step isn't understood
        match single(head) {
            Ok(s) => steps.push(s),
            Err(e) => {
                let parts: Vec<&str> = head.split(" and ").collect();
                if parts.len() < 2 { return Err(e); }
                for p in parts { steps.push(single(p).map_err(|_| e.clone())?); }
            }
        }
        rest = tail;
    }
    Ok(steps.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("; "))
}

/// Plain English (or syntax) -> runnable action. Used everywhere an action is loaded or saved.
pub fn parse(input: &str) -> Result<Action, String> {
    action::parse(&to_syntax(input)?)
}

// --- describing actions back in plain English ---

fn key_label(vk: u8) -> String {
    match vk {
        b'A'..=b'Z' | b'0'..=b'9' => (vk as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x21 => "Page Up".into(), 0x22 => "Page Down".into(), 0x08 => "Backspace".into(), 0x1B => "Esc".into(),
        0xBB => "+".into(),
        _ => action::key_name(vk).map(|n| {
            let mut c = n.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }).unwrap_or_else(|| format!("key {vk}")),
    }
}

fn mods_label(mods: &[u8]) -> String {
    mods.iter().map(|&m| match m { CTRL => "Ctrl + ", SHIFT => "Shift + ", ALT => "Alt + ", WIN => "Win + ", _ => "" }).collect()
}

fn notches(delta: i32) -> String {
    let n = delta.abs() as f32 / 120.0;
    if n == 1.0 { String::new() } else if n.fract() == 0.0 { format!(" {} notches", n as i32) } else { format!(" {n:.1} notches") }
}

fn describe_step(step: &action::Step) -> String {
    let m = mods_label(&step.mods);
    match &step.kind {
        Kind::Wheel { delta, horizontal: false } if step.mods == [CTRL] =>
            format!("zooms {}{}", if *delta > 0 { "in" } else { "out" }, notches(*delta)),
        Kind::Wheel { delta, horizontal } => {
            let dir = match (horizontal, *delta > 0) { (false, true) => "up", (false, false) => "down", (true, true) => "right", (true, false) => "left" };
            let held = if m.is_empty() { String::new() } else { format!(" with {} held", m.trim_end_matches(" + ")) };
            format!("scrolls {dir}{}{held}", notches(*delta))
        }
        Kind::Click { down, .. } => {
            let b = match down { 0x02 => "left", 0x08 => "right", _ => "middle" };
            if m.is_empty() { format!("{b}-clicks") } else { format!("{b}-clicks with {} held", m.trim_end_matches(" + ")) }
        }
        Kind::Key(vk) if step.mods.is_empty() && (0xAD..=0xB3).contains(vk) => match vk {
            0xB0 => "skips to the next track", 0xB1 => "goes to the previous track", 0xB2 => "stops playback",
            0xB3 => "plays / pauses", 0xAD => "mutes / unmutes", 0xAE => "turns the volume down", _ => "turns the volume up",
        }.to_string(),
        Kind::Key(vk) => {
            let syntax = action::combo_name(*vk, step.mods.contains(&CTRL), step.mods.contains(&SHIFT), step.mods.contains(&ALT), step.mods.contains(&WIN));
            let meaning = syntax.and_then(|s| PHRASES.iter().find(|p| p.1 == s).map(|p| format!(" ({})", p.0)));
            format!("presses {m}{}{}", key_label(*vk), meaning.unwrap_or_default())
        }
        Kind::Run(t) => format!("opens {t}"),
        Kind::Type(t) => format!("types \u{201C}{t}\u{201D}"),
    }
}

/// Settings preview: what an action box will do, or why it isn't understood.
pub fn describe(input: &str) -> Result<String, String> {
    let a = parse(input)?;
    if a.is_empty() { return Ok("Does nothing".into()); }
    let s = a.0.iter().map(describe_step).collect::<Vec<_>>().join(", then ");
    let mut c = s.chars();
    Ok(c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default())
}

/// True when two action boxes do the same thing, however they're worded.
pub fn same(a: &str, b: &str) -> bool {
    matches!((to_syntax(a), to_syntax(b)), (Ok(x), Ok(y)) if action::parse(&x) == action::parse(&y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_plain_english() {
        for (input, want) in [
            ("scroll down", "wheel:-120"), ("Scroll Up 3", "wheel:360"), ("scroll left by two notches", "hwheel:-240"),
            ("zoom in", "ctrl+wheel:120"), ("reset zoom", "ctrl+0"), ("next track", "next"), ("Volume up", "volup"),
            ("play/pause", "playpause"), ("copy", "ctrl+c"), ("undo.", "ctrl+z"), ("please paste", "ctrl+v"),
            ("press ctrl shift z", "ctrl+shift+z"), ("ctrl + z", "ctrl + z"), ("hold shift and press tab", "shift+tab"),
            ("control plus z", "ctrl+z"), ("press plus", "plus"), ("ctrl plus", "ctrl+plus"), ("press page down", "pagedown"),
            ("press escape", "esc"), ("cmd left bracket", "win+["), ("press down", "down"), ("up arrow", "up"),
            ("middle click", "click:middle"), ("open notepad", "run:notepad"), ("Open C:\\Tools\\My App.exe", "run:C:\\Tools\\My App.exe"),
            ("go to youtube.com", "run:https://youtube.com"), ("type Hello, World", "type:Hello, World"),
            ("do nothing", ""), ("", ""), ("wheel:-120", "wheel:-120"), ("ctrl+shift+k", "ctrl+shift+k"),
        ] {
            assert_eq!(to_syntax(input).as_deref(), Ok(want), "{input}");
        }
    }

    #[test]
    fn sequences() {
        assert_eq!(to_syntax("copy then paste").unwrap(), "ctrl+c; ctrl+v");
        assert_eq!(to_syntax("copy and paste").unwrap(), "ctrl+c; ctrl+v");
        assert_eq!(to_syntax("select all, copy, and then switch window").unwrap(), "ctrl+a; ctrl+c; alt+tab");
        assert_eq!(to_syntax("new tab then type hello, then world").unwrap(), "ctrl+t; type:hello, then world");
        assert_eq!(to_syntax("hold shift and press tab then enter").unwrap(), "shift+tab; enter");
    }

    #[test]
    fn explains_mistakes() {
        assert!(to_syntax("zoom innn").unwrap_err().contains("Did you mean \u{2018}zoom in\u{2019}"));
        assert!(to_syntax("make me a sandwich").unwrap_err().contains("Try e.g."));
        assert!(to_syntax("scroll down 500").is_err());
        assert!(to_syntax("press ctrl").is_err());          // a modifier alone isn't an action
    }

    #[test]
    fn describes() {
        for (input, want) in [
            ("scroll down", "Scrolls down"), ("scroll up 3", "Scrolls up 3 notches"), ("zoom out", "Zooms out"),
            ("ctrl+z", "Presses Ctrl + Z (undo)"), ("press ctrl shift k", "Presses Ctrl + Shift + K"),
            ("next track", "Skips to the next track"), ("middle click", "Middle-clicks"), ("open notepad", "Opens notepad"),
            ("copy then paste", "Presses Ctrl + C (copy), then presses Ctrl + V (paste)"), ("type hi", "Types \u{201C}hi\u{201D}"),
            ("", "Does nothing"), ("page up", "Presses Page Up"), ("f13", "Presses F13"),
        ] {
            assert_eq!(describe(input).as_deref(), Ok(want), "{input}");
        }
    }

    #[test]
    fn same_meaning() {
        assert!(same("scroll down", "wheel:-120"));
        assert!(same("next track", "next"));
        assert!(same("ctrl + z", "ctrl+z") && same("undo", "ctrl+z"));   // valid syntax is kept as typed, same meaning
        assert!(!same("scroll down", "scroll up"));
    }
}
