//! Knob gestures. The knob sends F13 (counter-clockwise), F15 (clockwise) and F14 (press), and F16/F17 when it's
//! turned while held down; the firmware reports every press as an instant tap, so a plain hold isn't seen. Taps
//! are counted within a window to switch layer; a tap followed by a turn, or a turn while held, gives the
//! "shifted" turn actions.
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture { #[default] DoubleTap, TripleTap, TrayOnly }

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Input { Cw, Ccw, Press, HeldCw, HeldCcw }

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Output { Turn(i32), ShiftTurn(i32), Press, NextLayer }

/// What the current layer and settings allow.
pub struct Ctx { pub window_ms: u64, pub gesture: Gesture, pub has_shift: bool }

#[derive(Default)]
pub struct Knob {
    pub layer: usize,
    taps: u32,                // taps waiting for the window to close
    last_tap: u64,
    shift_until: Option<u64>, // turns before this time stay shifted
}

impl Knob {
    /// An empty result after a press means "wait": call `timeout()` once `window_ms` passes without another tap.
    pub fn feed(&mut self, input: Input, now: u64, c: &Ctx) -> Vec<Output> {
        let dir = match input { Input::Cw | Input::HeldCw => 1, Input::Ccw | Input::HeldCcw => -1, Input::Press => 0 };
        if matches!(input, Input::HeldCw | Input::HeldCcw) {      // held: shifted if the layer has those, else a plain turn
            self.taps = 0;
            self.shift_until = None;
            return vec![if c.has_shift { Output::ShiftTurn(dir) } else { Output::Turn(dir) }];
        }
        if dir != 0 {
            let tapped = self.taps > 0 && now - self.last_tap <= c.window_ms;
            if c.has_shift && (tapped || self.shift_until.is_some_and(|u| now <= u)) {
                self.taps = 0;                          // the tap was a modifier, not a press
                self.shift_until = Some(now + c.window_ms);
                return vec![Output::ShiftTurn(dir)];
            }
            self.shift_until = None;
            return vec![Output::Turn(dir)];
        }
        self.shift_until = None;
        let needed = match c.gesture { Gesture::DoubleTap => 2, Gesture::TripleTap => 3, Gesture::TrayOnly => u32::MAX };
        if needed == u32::MAX && !c.has_shift { return vec![Output::Press]; }   // nothing to wait for: fire now
        self.taps = if self.taps > 0 && now - self.last_tap <= c.window_ms { self.taps + 1 } else { 1 };
        self.last_tap = now;
        if self.taps >= needed { self.taps = 0; return vec![Output::NextLayer]; }
        vec![]
    }

    /// The tap window closed: the counted taps were plain presses.
    pub fn timeout(&mut self) -> Vec<Output> {
        vec![Output::Press; std::mem::take(&mut self.taps) as usize]
    }
}

/// Turn pacing: acceleration from the gap between detents, and an optional rate limit.
#[derive(Default)]
pub struct Pace { last_turn: Option<u64>, last_fired: Option<u64> }

impl Pace {
    /// Multiplier for this detent (1.0 when not accelerating), or None when `max_per_sec` drops it.
    pub fn step(&mut self, now: u64, accelerate: bool, max_per_sec: u32) -> Option<f32> {
        let gap = self.last_turn.map(|t| now.saturating_sub(t)).unwrap_or(u64::MAX);
        self.last_turn = Some(now);
        if max_per_sec > 0 && self.last_fired.is_some_and(|f| now.saturating_sub(f) < 1000 / max_per_sec as u64) { return None; }
        self.last_fired = Some(now);
        // ponytail: simple curve - 1x at >=150 ms between detents, up to 5x when spinning fast
        Some(if accelerate { (150.0 / gap.max(1) as f32).clamp(1.0, 5.0) } else { 1.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: Ctx = Ctx { window_ms: 350, gesture: Gesture::DoubleTap, has_shift: false };

    #[test]
    fn held_turn_is_shifted_or_plain() {
        let mut k = Knob::default();
        assert_eq!(k.feed(Input::HeldCw, 0, &D), vec![Output::Turn(1)]);
        let s = Ctx { has_shift: true, ..D };
        assert_eq!(k.feed(Input::HeldCcw, 10, &s), vec![Output::ShiftTurn(-1)]);
        assert_eq!(k.feed(Input::Cw, 20, &s), vec![Output::Turn(1)]);   // letting go ends it
    }

    #[test]
    fn double_tap() {
        let mut k = Knob::default();
        assert_eq!(k.feed(Input::Cw, 0, &D), vec![Output::Turn(1)]);
        assert_eq!(k.feed(Input::Press, 1000, &D), vec![]);                // waits for a second tap
        assert_eq!(k.timeout(), vec![Output::Press]);
        assert_eq!(k.timeout(), vec![]);
        assert_eq!(k.feed(Input::Press, 2000, &D), vec![]);
        assert_eq!(k.feed(Input::Press, 2200, &D), vec![Output::NextLayer]);
        assert_eq!(k.timeout(), vec![]);
        assert_eq!(k.feed(Input::Press, 3000, &D), vec![]);
        assert_eq!(k.feed(Input::Press, 3500, &D), vec![]);                // too slow: starts a new count
        assert_eq!(k.timeout(), vec![Output::Press]);
    }

    #[test]
    fn triple_tap_and_tray_only() {
        let t = Ctx { gesture: Gesture::TripleTap, ..D };
        let mut k = Knob::default();
        k.feed(Input::Press, 0, &t);
        assert_eq!(k.feed(Input::Press, 200, &t), vec![]);
        assert_eq!(k.feed(Input::Press, 400, &t), vec![Output::NextLayer]);
        k.feed(Input::Press, 1000, &t); k.feed(Input::Press, 1200, &t);
        assert_eq!(k.timeout(), vec![Output::Press, Output::Press]);       // two taps = two presses
        let tray = Ctx { gesture: Gesture::TrayOnly, ..D };
        assert_eq!(k.feed(Input::Press, 2000, &tray), vec![Output::Press]); // instant, no waiting
    }

    #[test]
    fn tap_then_turn() {
        let s = Ctx { has_shift: true, ..D };
        let mut k = Knob::default();
        assert_eq!(k.feed(Input::Press, 0, &s), vec![]);
        assert_eq!(k.feed(Input::Cw, 200, &s), vec![Output::ShiftTurn(1)]);
        assert_eq!(k.feed(Input::Ccw, 400, &s), vec![Output::ShiftTurn(-1)]);  // keeps going while turning
        assert_eq!(k.timeout(), vec![]);                                       // the tap was not a press
        assert_eq!(k.feed(Input::Cw, 1000, &s), vec![Output::Turn(1)]);        // pause ended the shift
        assert_eq!(k.feed(Input::Press, 2000, &D), vec![]);
        assert_eq!(k.feed(Input::Cw, 2100, &D), vec![Output::Turn(1)]);        // layer without shift actions
        assert_eq!(k.timeout(), vec![Output::Press]);
    }

    #[test]
    fn pace() {
        let mut p = Pace::default();
        assert_eq!(p.step(0, true, 0), Some(1.0));
        assert_eq!(p.step(30, true, 0), Some(5.0));          // fast spin
        assert_eq!(p.step(105, true, 0), Some(2.0));
        assert_eq!(p.step(1000, false, 0), Some(1.0));
        let mut r = Pace::default();
        assert!(r.step(0, false, 4).is_some());
        assert!(r.step(100, false, 4).is_none());           // 4/s = one per 250 ms
        assert!(r.step(260, false, 4).is_some());
    }
}
