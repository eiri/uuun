// FIXME! Can't make it work on polysynth atm,
// remove from midi wiring and figure out later
pub struct LegatoTracker {
    held: [bool; 128],
    count: usize,
}

impl LegatoTracker {
    pub fn new() -> Self {
        Self {
            held: [false; 128],
            count: 0,
        }
    }

    #[allow(dead_code)]
    pub fn press(&mut self, note: u8) -> bool {
        let legato = self.count > 0;
        let idx = note as usize;
        if !self.held[idx] {
            self.held[idx] = true;
            self.count += 1;
        }
        legato
    }

    #[allow(dead_code)]
    pub fn release(&mut self, note: u8) -> bool {
        let idx = note as usize;
        if self.held[idx] {
            self.held[idx] = false;
            self.count = self.count.saturating_sub(1);
        }
        self.count > 0
    }

    #[allow(dead_code)]
    pub fn held_count(&self) -> usize {
        self.count
    }
}

impl Default for LegatoTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_note_not_legato() {
        let mut t = LegatoTracker::new();
        assert!(!t.press(60), "first note should not be legato");
    }

    #[test]
    fn second_note_is_legato() {
        let mut t = LegatoTracker::new();
        t.press(60);
        assert!(t.press(64), "second overlapping note should be legato");
    }

    #[test]
    fn release_while_other_held() {
        let mut t = LegatoTracker::new();
        t.press(60);
        t.press(64);
        assert!(t.release(60), "still holding note 64");
    }

    #[test]
    fn release_last_note() {
        let mut t = LegatoTracker::new();
        t.press(60);
        assert!(!t.release(60), "no notes remaining");
    }
}
