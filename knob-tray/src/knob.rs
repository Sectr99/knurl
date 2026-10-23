//! Layer state machine. The knob sends F13 (counter-clockwise), F15 (clockwise) and F14 (press);
//! the firmware reports every press as an instant tap, so layers switch on double-tap.

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Input { Cw, Ccw, Press }

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Output { Turn(i32), Press, Layer(usize) }

#[derive(Default)]
pub struct Knob {
    pub layer: usize,
    first_tap: Option<u64>, // ms timestamp of a tap still waiting for a possible second tap
}

impl Knob {
    /// A tap returns None and the caller must call `timeout()` after `double_ms` to fire the single press.
    pub fn feed(&mut self, input: Input, now_ms: u64, layers: usize, double_ms: u64) -> Option<Output> {
        match input {
            Input::Cw => Some(Output::Turn(1)),
            Input::Ccw => Some(Output::Turn(-1)),
            Input::Press => match self.first_tap.take() {
                Some(t) if now_ms - t <= double_ms => {
                    self.layer = (self.layer + 1) % layers.max(1);
                    Some(Output::Layer(self.layer))
                }
                _ => { self.first_tap = Some(now_ms); None }
            },
        }
    }

    pub fn timeout(&mut self) -> Option<Output> {
        self.first_tap.take().map(|_| Output::Press)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_turns_and_layers() {
        let mut k = Knob::default();
        assert_eq!(k.feed(Input::Cw, 0, 3, 350), Some(Output::Turn(1)));
        assert_eq!(k.feed(Input::Ccw, 10, 3, 350), Some(Output::Turn(-1)));
        assert_eq!(k.feed(Input::Press, 1000, 3, 350), None);          // waits for a second tap
        assert_eq!(k.timeout(), Some(Output::Press));                   // single tap
        assert_eq!(k.timeout(), None);
        assert_eq!(k.feed(Input::Press, 2000, 3, 350), None);
        assert_eq!(k.feed(Input::Press, 2200, 3, 350), Some(Output::Layer(1))); // double tap
        assert_eq!(k.timeout(), None);                                  // nothing left pending
        assert_eq!(k.feed(Input::Press, 3000, 3, 350), None);
        assert_eq!(k.feed(Input::Press, 3500, 3, 350), None);           // too slow: second tap starts a new wait
        assert_eq!(k.timeout(), Some(Output::Press));
        for i in 0..2 { k.feed(Input::Press, 5000 + i * 1000, 3, 350); k.feed(Input::Press, 5100 + i * 1000, 3, 350); }
        assert_eq!(k.layer, 0);                                         // wraps around
    }
}
