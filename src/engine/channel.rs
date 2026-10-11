use crate::dsp::{patch::Patch, voice::RenderParams};
use crate::{
    engine::{allocator::VoiceAllocator, message::EngineEvent},
    parameter::Control,
};

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
        let allocator = VoiceAllocator::new(sample_rate);
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

    pub fn handle(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::NoteOn { note, velocity } => self.note_on(note, velocity),
            EngineEvent::NoteOff { note } => self.note_off(note),
            EngineEvent::AllNotesOff => self.all_notes_off(),
            EngineEvent::AllSoundOff => self.all_sound_off(),
            EngineEvent::Sustain { down } => self.set_sustain(down),
            EngineEvent::ResetControllers => self.reset_controllers(),
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let note = note as usize;
        self.presses[note] = self.presses[note].saturating_add(1);
        self.deferred[note] = false;
        self.allocator.note_on(note as u8, velocity, &self.patch);
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
            Control::PitchBend => self.pitch_bend(value),
            Control::ChannelPressure => self.channel_pressure(value),
            _ => {
                self.patch.set_control(control, value);
            }
        }
    }

    /// Set the channel pitch-bend offset in semitones.
    pub fn pitch_bend(&mut self, semitones: f64) {
        self.pitch_bend = semitones;
    }

    /// Set channel aftertouch (0.0..1.0).
    pub fn channel_pressure(&mut self, value: f64) {
        self.pressure = value;
    }

    pub fn process(&mut self, out_buf: &mut [f64]) {
        let params = RenderParams {
            patch: &self.patch,
            pitch_bend: self.pitch_bend,
            pressure: self.pressure,
        };
        self.allocator.process(out_buf, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_all_events() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.handle(EngineEvent::NoteOn {
            note: 60,
            velocity: 100,
        });
        assert_eq!(ch.presses[60], 1);
        ch.handle(EngineEvent::Sustain { down: true });
        ch.handle(EngineEvent::NoteOff { note: 60 });
        assert!(ch.deferred[60]);
        ch.pitch_bend(2.0);
        ch.channel_pressure(1.0);
        ch.handle(EngineEvent::ResetControllers);
        assert!(!ch.sustain);
        assert!(!ch.deferred[60]);
        assert_eq!(ch.pitch_bend, 0.0);
        assert_eq!(ch.pressure, 0.0);
        ch.handle(EngineEvent::NoteOn {
            note: 62,
            velocity: 100,
        });
        ch.handle(EngineEvent::AllNotesOff);
        assert!(ch.presses.iter().all(|n| *n == 0));
        ch.handle(EngineEvent::AllSoundOff);
        let mut output = [0.0; 256];
        ch.process(&mut output);
        assert!(output.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn controls_reach_current_notes() {
        for releasing in [false, true] {
            let mut changed = Channel::new(Patch::default(), 48_000.0);
            let mut plain = Channel::new(Patch::default(), 48_000.0);
            for ch in [&mut changed, &mut plain] {
                ch.note_on(60, 100);
                ch.process(&mut [0.0; 256]);
                if releasing {
                    ch.note_off(60);
                }
            }
            changed.control(Control::FilterCutoff, 200.0);
            changed.control(Control::PitchBend, 2.0);
            changed.control(Control::ChannelPressure, 0.5);
            let mut a = [0.0; 256];
            let mut b = [0.0; 256];
            changed.process(&mut a);
            plain.process(&mut b);
            assert_ne!(a, b, "releasing {releasing}");
        }
    }

    #[test]
    fn patch_controls_reach_new_notes() {
        let mut changed = Channel::new(Patch::default(), 48_000.0);
        changed.control(Control::FilterCutoff, 200.0);
        let mut expected = Channel::new(
            Patch {
                filter_cutoff_hz: 200.0,
                ..Default::default()
            },
            48_000.0,
        );
        changed.note_on(60, 100);
        expected.note_on(60, 100);
        let mut a = [0.0; 256];
        let mut b = [0.0; 256];
        changed.process(&mut a);
        expected.process(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn channels_are_independent() {
        let mut changed = Channel::new(Patch::default(), 48_000.0);
        let mut untouched = Channel::new(Patch::default(), 48_000.0);
        let mut reference = Channel::new(Patch::default(), 48_000.0);
        changed.control(Control::FilterCutoff, 200.0);
        changed.pitch_bend(2.0);
        changed.channel_pressure(1.0);
        for ch in [&mut changed, &mut untouched, &mut reference] {
            ch.note_on(60, 100);
        }
        let mut outputs = [[0.0; 256]; 3];
        changed.process(&mut outputs[0]);
        untouched.process(&mut outputs[1]);
        reference.process(&mut outputs[2]);
        assert_eq!(outputs[1], outputs[2]);
        assert_ne!(outputs[0], outputs[1]);
    }

    #[test]
    fn sustain_release_and_panic() {
        let patch = Patch {
            amp_env: crate::dsp::envelope::AdsrParams {
                release: 0.001,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut ch = Channel::new(patch, 48_000.0);
        ch.note_on(60, 100);
        ch.process(&mut [0.0; 256]);
        ch.set_sustain(true);
        ch.note_off(60);
        ch.set_sustain(false);
        ch.process(&mut [0.0; 256]);
        let mut output = [0.0; 256];
        ch.process(&mut output);
        assert!(output.iter().all(|s| *s == 0.0));

        ch.note_on(62, 100);
        ch.process(&mut [0.0; 256]);
        ch.all_sound_off();
        ch.process(&mut output);
        assert!(output.iter().all(|s| *s == 0.0));
        assert!(ch.presses.iter().all(|n| *n == 0));
    }

    #[test]
    fn channel_note_on_produces_audio() {
        let mut ch = Channel::new(Patch::default(), 48_000.0);
        ch.note_on(60, 100);
        let mut buf = [0.0_f64; 128];
        ch.process(&mut buf);
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
        plain.process(&mut plain_buf);
        bent.process(&mut bent_buf);

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
        plain.process(&mut plain_buf);
        pressed.process(&mut pressed_buf);

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
