use crate::dsp::patch::Patch;
use crate::engine::allocator::VoiceAllocator;

pub struct Channel {
    pub patch: Patch,
    allocator: VoiceAllocator,
    pub sample_rate: f64,
    pitch_bend: f64,
    pressure: f64,
    sustain: bool,
    deferred: [bool; 128],
}

impl Channel {
    pub fn new(patch: Patch, sample_rate: f64) -> Self {
        let allocator = VoiceAllocator::new(&patch, sample_rate);
        Self {
            patch,
            allocator,
            sample_rate,
            pitch_bend: 0.0,
            pressure: 0.0,
            sustain: false,
            deferred: [false; 128],
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.deferred[note as usize] = false;
        self.allocator
            .note_on(note, velocity, &self.patch, self.sample_rate);
        self.allocator.pitch_bend(self.pitch_bend);
        self.allocator.channel_pressure(self.pressure);
    }

    pub fn note_off(&mut self, note: u8) {
        if self.sustain {
            self.deferred[note as usize] = true;
        } else {
            self.allocator.note_off(note, &self.patch);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.deferred.fill(false);
        self.allocator.all_notes_off(&self.patch);
    }

    pub fn all_sound_off(&mut self) {
        self.deferred.fill(false);
        self.allocator.all_sound_off();
    }

    pub fn set_sustain(&mut self, down: bool) {
        if self.sustain && !down {
            for (note, deferred) in self.deferred.iter_mut().enumerate() {
                if *deferred {
                    self.allocator.note_off(note as u8, &self.patch);
                    *deferred = false;
                }
            }
        }

        self.sustain = down;
    }

    pub fn reset_controllers(&mut self) {
        self.pitch_bend(0.0);
        self.channel_pressure(0.0);
        self.set_sustain(false);
    }

    pub fn set_patch(&mut self, patch: Patch) -> Result<(), String> {
        patch.validate()?;

        self.patch = patch;
        self.allocator.set_patch(&self.patch);
        Ok(())
    }

    /// Apply a pitch-bend offset (in semitones) to all active voices.
    pub fn pitch_bend(&mut self, semitones: f64) {
        self.pitch_bend = semitones;
        self.allocator.pitch_bend(semitones);
    }

    /// Apply channel aftertouch (0.0..1.0) to all active voices.
    pub fn channel_pressure(&mut self, value: f64) {
        self.pressure = value;
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
    fn bend_set_before_note_is_applied() {
        let patch = Patch {
            filter_cutoff_hz: 10_000.0,
            ..Default::default()
        };
        let mut plain = Channel::new(patch, 48_000.0);
        let mut bent = Channel::new(patch, 48_000.0);
        bent.pitch_bend(2.0);

        plain.note_on(60, 100);
        bent.note_on(60, 100);

        let mut plain_buf = [0.0_f64; 256];
        let mut bent_buf = [0.0_f64; 256];
        plain.process(&mut plain_buf, 256);
        bent.process(&mut bent_buf, 256);

        assert_ne!(plain_buf, bent_buf);
    }

    #[test]
    fn pressure_set_before_note_is_applied() {
        let patch = Patch {
            filter_cutoff_hz: 200.0,
            ..Default::default()
        };
        let mut plain = Channel::new(patch, 48_000.0);
        let mut pressed = Channel::new(patch, 48_000.0);
        pressed.channel_pressure(1.0);

        plain.note_on(60, 100);
        pressed.note_on(60, 100);

        let mut plain_buf = [0.0_f64; 256];
        let mut pressed_buf = [0.0_f64; 256];
        plain.process(&mut plain_buf, 256);
        pressed.process(&mut pressed_buf, 256);

        assert_ne!(plain_buf, pressed_buf);
    }

    #[test]
    fn sustain_defers_note_off() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.note_on(60, 100);
        ch.set_sustain(true);
        ch.note_off(60);

        assert!(ch.deferred[60]);

        ch.set_sustain(false);
        assert!(!ch.deferred[60]);
    }

    #[test]
    fn reset_clears_controller_state() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.note_on(60, 100);
        ch.pitch_bend(2.0);
        ch.channel_pressure(1.0);
        ch.set_sustain(true);
        ch.note_off(60);

        ch.reset_controllers();

        assert_eq!(ch.pitch_bend, 0.0);
        assert_eq!(ch.pressure, 0.0);
        assert!(!ch.sustain);
        assert!(!ch.deferred[60]);
    }

    #[test]
    fn channel_rejects_invalid_patch() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        let invalid = Patch {
            osc1_level: 2.0,
            ..Default::default()
        };

        assert!(ch.set_patch(invalid).is_err());
        assert_eq!(ch.patch.osc1_level, 1.0);
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
        ch.set_patch(new_patch).unwrap();
        buf.fill(0.0);
        ch.process(&mut buf, 128);
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6);
    }
}
