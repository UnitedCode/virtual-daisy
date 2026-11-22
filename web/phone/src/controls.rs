//! Preserve real switch edges when browser clicks arrive in one message batch.
use std::collections::VecDeque;
const MIN_EDGE_GAP: u64 = 16_000_000;
#[derive(Default)]
pub struct Switch {
    requested: bool,
    last_edge: Option<u64>,
    edges: VecDeque<(u64, bool)>,
}
impl Switch {
    pub fn set(&mut self, pressed: bool, now: u64) {
        if pressed == self.requested {
            return;
        }
        let at = self
            .last_edge
            .map_or(now, |last| now.max(last.saturating_add(MIN_EDGE_GAP)));
        self.edges.push_back((at, pressed));
        self.last_edge = Some(at);
        self.requested = pressed;
    }
    pub fn due(&mut self, now: u64) -> Option<bool> {
        if self.edges.front().is_some_and(|edge| edge.0 <= now) {
            self.edges.pop_front().map(|edge| edge.1)
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_browser_double_click_keeps_two_distinct_presses() {
        let mut switch = Switch::default();
        for pressed in [true, false, true, false] {
            switch.set(pressed, 0);
        }
        assert_eq!(switch.due(0), Some(true));
        assert_eq!(switch.due(15_999_999), None);
        assert_eq!(switch.due(16_000_000), Some(false));
        assert_eq!(switch.due(32_000_000), Some(true));
        assert_eq!(switch.due(48_000_000), Some(false));
        assert_eq!(switch.due(64_000_000), None);
    }
    #[test]
    fn holding_and_releasing_a_key_keeps_the_performers_duration() {
        let mut switch = Switch::default();
        switch.set(true, 100_000_000);
        switch.set(true, 120_000_000);
        assert_eq!(switch.due(100_000_000), Some(true));
        assert_eq!(switch.due(500_000_000), None);
        switch.set(false, 600_000_000);
        switch.set(false, 600_000_000);
        assert_eq!(switch.due(600_000_000), Some(false));
        assert_eq!(switch.due(600_000_000), None);
    }
}
