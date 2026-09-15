use crate::dsp::patch::Patch;
use crate::engine::{allocator::VoiceAllocator, control::Control};

pub struct Channel {
    patch: Patch,
    allocator: VoiceAllocator,
    pitch_bend: f64,
    pressure: f64,
    sustain: bool,
    presses: [u16; 128],
    deferred: [bool; 128],
}

impl Channel {
    pub fn new(patch: Patch, sample_rate: f64) -> Self {
        let allocator = VoiceAllocator::new(&patch, sample_rate);
        Self {
            patch,
            allocator,
            pitch_bend: 0.0,
            pressure: 0.0,
            sustain: false,
            presses: [0; 128],
            deferred: [false; 128],
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let note = note as usize;
        self.presses[note] = self.presses[note].saturating_add(1);
        self.deferred[note] = false;
        self.allocator.note_on(note as u8, velocity, &self.patch);
        self.allocator.pitch_bend(self.pitch_bend);
        self.allocator.channel_pressure(self.pressure);
    }

    pub fn note_off(&mut self, note: u8) {
        let note = note as usize;
        if self.presses[note] == 0 {
            return;
        }

        self.presses[note] -= 1;
        if self.presses[note] > 0 {
            return;
        }

        if self.sustain {
            self.deferred[note] = true;
        } else {
            self.allocator.note_off(note as u8);
        }
    }

    pub fn all_notes_off(&mut self) {
        self.presses.fill(0);
        self.deferred.fill(false);
        self.allocator.all_notes_off();
    }

    pub fn all_sound_off(&mut self) {
        self.presses.fill(0);
        self.deferred.fill(false);
        self.allocator.all_sound_off();
    }

    pub fn set_sustain(&mut self, down: bool) {
        if self.sustain && !down {
            for (note, deferred) in self.deferred.iter_mut().enumerate() {
                if *deferred {
                    self.allocator.note_off(note as u8);
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

    pub fn control(&mut self, control: Control, value: f64) {
        match control {
            Control::FilterCutoff => self.patch.filter_cutoff_hz = value,
            Control::FilterResonance => self.patch.filter_resonance = value,
            Control::FilterEnvAmount => self.patch.filter_env_amount = value,
            Control::FilterKeyTrack => self.patch.filter_key_track = value,
            Control::AmpAttack => self.patch.amp_env.attack = value,
            Control::AmpDecay => self.patch.amp_env.decay = value,
            Control::AmpSustain => self.patch.amp_env.sustain = value,
            Control::AmpRelease => self.patch.amp_env.release = value,
            Control::FilterAttack => self.patch.filter_env.attack = value,
            Control::FilterDecay => self.patch.filter_env.decay = value,
            Control::FilterSustain => self.patch.filter_env.sustain = value,
            Control::FilterRelease => self.patch.filter_env.release = value,
            Control::LfoRate => self.patch.lfo_rate_hz = value,
            Control::LfoDepth => self.patch.lfo_depth = value,
            Control::Osc1Level => self.patch.osc1_level = value,
            Control::Osc2Level => self.patch.osc2_level = value,
            Control::Osc3Level => self.patch.osc3_level = value,
            Control::NoiseLevel => self.patch.noise_level = value,
            Control::GlideTime => self.patch.glide_time = value,
            Control::PitchBend => {
                self.pitch_bend(value);
                return;
            }
            Control::ChannelPressure => {
                self.channel_pressure(value);
                return;
            }
        }

        self.allocator.apply_patch(&self.patch);
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
    fn repeated_note_waits_for_every_note_off() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.set_sustain(true);
        ch.note_on(60, 100);
        ch.note_on(60, 100);

        ch.note_off(60);
        assert_eq!(ch.presses[60], 1);
        assert!(!ch.deferred[60]);

        ch.note_off(60);
        assert_eq!(ch.presses[60], 0);
        assert!(ch.deferred[60]);
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
}
