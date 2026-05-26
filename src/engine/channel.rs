use crate::dsp::patch::Patch;
use crate::engine::allocator::VoiceAllocator;

pub struct Channel {
    pub patch: Patch,
    allocator: VoiceAllocator,
    pub sample_rate: f64,
}

impl Channel {
    pub fn new(patch: Patch, sample_rate: f64) -> Self {
        let allocator = VoiceAllocator::new(&patch, sample_rate);
        Self {
            patch,
            allocator,
            sample_rate,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.allocator
            .note_on(note, velocity, &self.patch, self.sample_rate);
    }

    pub fn note_off(&mut self, note: u8) {
        self.allocator.note_off(note, &self.patch);
    }

    pub fn all_notes_off(&mut self) {
        self.allocator.all_notes_off(&self.patch);
    }

    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
        self.allocator.set_patch(&self.patch);
    }

    /// Apply a pitch-bend offset (in semitones) to all active voices.
    pub fn pitch_bend(&mut self, semitones: f64) {
        self.allocator.pitch_bend(semitones);
    }

    /// Apply channel aftertouch (0.0..1.0) to all active voices.
    pub fn channel_pressure(&mut self, value: f64) {
        self.allocator.channel_pressure(value);
    }

    pub fn process(&mut self, out_buf: &mut [f64], n: usize) {
        self.allocator.process(out_buf, n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_note_on_produces_audio() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.note_on(60, 100);
        let mut buf = [0.0_f64; 128];
        ch.process(&mut buf, 128);
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6);
    }

    #[test]
    fn channel_patch_swap() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.note_on(60, 100);
        let mut buf = [0.0_f64; 128];
        ch.process(&mut buf, 128);
        // Swap patch mid-stream — must not panic.
        let new_patch = Patch {
            filter_cutoff_hz: 4_000.0,
            ..Default::default()
        };
        ch.set_patch(new_patch);
        buf.fill(0.0);
        ch.process(&mut buf, 128);
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6);
    }
}
