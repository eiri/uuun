use std::sync::{Arc, mpsc};

use crate::{
    engine::{
        audio::NUM_CHANNELS,
        control::{Control, Controls},
        message::EngineMessage,
    },
    midi::{
        cc_map::{
            ALL_NOTES_OFF_CC, ALL_SOUND_OFF_CC, CcTarget, RESET_CONTROLLERS_CC, SUSTAIN_CC,
            cc_target,
        },
        message::MidiEvent,
    },
};

/// Maximum pitch-bend range in semitones, +2/-2
const PITCH_BEND_SEMITONES: f64 = 2.0;

/// Routes MIDI events into the engine event queue and control mailboxes.
pub struct MidiRouter {
    engine_tx: mpsc::SyncSender<EngineMessage>,
    controls: Arc<Controls>,
}

impl MidiRouter {
    pub fn new(engine_tx: mpsc::SyncSender<EngineMessage>, controls: Arc<Controls>) -> Self {
        Self {
            engine_tx,
            controls,
        }
    }

    pub fn route(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }

                self.send(EngineMessage::NoteOn {
                    channel: ch,
                    note,
                    velocity,
                });
            }

            MidiEvent::NoteOff { channel, note } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }
                self.send_critical(EngineMessage::NoteOff { channel: ch, note });
            }

            MidiEvent::ControlChange { channel, cc, value } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }
                match cc {
                    SUSTAIN_CC => {
                        self.send_critical(EngineMessage::Sustain {
                            channel: ch,
                            down: value >= 64,
                        });
                        return;
                    }
                    ALL_SOUND_OFF_CC => {
                        self.send_critical(EngineMessage::AllSoundOff { channel: ch });
                        return;
                    }
                    RESET_CONTROLLERS_CC => {
                        self.controls
                            .clear(ch, &[Control::PitchBend, Control::ChannelPressure]);
                        self.send_critical(EngineMessage::ResetControllers { channel: ch });
                        return;
                    }
                    ALL_NOTES_OFF_CC => {
                        self.send_critical(EngineMessage::AllNotesOff { channel: ch });
                        return;
                    }
                    _ => {}
                }

                let target = cc_target(cc);
                if target == CcTarget::Unassigned {
                    return;
                }
                let normalised = value as f64 / 127.0; // 0.0..1.0
                self.apply_cc(ch, target, normalised);
            }

            MidiEvent::PitchBend { channel, value } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }
                let semitones = value * PITCH_BEND_SEMITONES;
                self.controls.set(ch, Control::PitchBend, semitones);
            }

            MidiEvent::ChannelPressure { channel, value } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }
                self.controls.set(ch, Control::ChannelPressure, value);
            }
        }
    }

    fn send(&self, msg: EngineMessage) {
        let _ = self.engine_tx.try_send(msg);
    }

    fn send_critical(&self, msg: EngineMessage) {
        // Releases must survive a full queue to prevent stuck notes.
        let _ = self.engine_tx.send(msg);
    }

    fn apply_cc(&self, ch: usize, target: CcTarget, v: f64) {
        let (control, value) = match target {
            CcTarget::FilterCutoff => (Control::FilterCutoff, 20.0 * 1000.0_f64.powf(v)),
            CcTarget::FilterResonance => (Control::FilterResonance, v.clamp(0.0, 0.99)),
            CcTarget::FilterEnvAmount => (Control::FilterEnvAmount, v),
            CcTarget::FilterKeyTrack => (Control::FilterKeyTrack, v),
            CcTarget::AmpAttack => (Control::AmpAttack, map_adsr_time(v)),
            CcTarget::AmpDecay => (Control::AmpDecay, map_adsr_time(v)),
            CcTarget::AmpSustain => (Control::AmpSustain, v),
            CcTarget::AmpRelease => (Control::AmpRelease, map_adsr_time(v)),
            CcTarget::FilterAttack => (Control::FilterAttack, map_adsr_time(v)),
            CcTarget::FilterDecay => (Control::FilterDecay, map_adsr_time(v)),
            CcTarget::FilterSustain => (Control::FilterSustain, v),
            CcTarget::FilterRelease => (Control::FilterRelease, map_adsr_time(v)),
            CcTarget::LfoRate => (Control::LfoRate, 0.01 * 3000.0_f64.powf(v)),
            CcTarget::LfoDepth => (Control::LfoDepth, v),
            CcTarget::Osc1Level => (Control::Osc1Level, v),
            CcTarget::Osc2Level => (Control::Osc2Level, v),
            CcTarget::Osc3Level => (Control::Osc3Level, v),
            CcTarget::NoiseLevel => (Control::NoiseLevel, v),
            CcTarget::GlideTime => (Control::GlideTime, v * 4.0),
            CcTarget::Unassigned => return,
        };

        self.controls.set(ch, control, value);
    }
}

fn map_adsr_time(v: f64) -> f64 {
    0.001 * 10_000.0_f64.powf(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_off_survives_full_queue() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut router = MidiRouter::new(tx, Arc::new(Controls::new()));
        router.route(MidiEvent::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        });

        let handle = std::thread::spawn(move || {
            router.route(MidiEvent::NoteOff {
                channel: 0,
                note: 60,
            });
        });

        assert!(matches!(rx.recv().unwrap(), EngineMessage::NoteOn { .. }));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::NoteOff {
                channel: 0,
                note: 60
            }
        ));
        handle.join().unwrap();
    }

    #[test]
    fn controls_keep_latest_when_queue_is_full() {
        let (tx, rx) = mpsc::sync_channel(1);
        let controls = Arc::new(Controls::new());
        let mut router = MidiRouter::new(tx, Arc::clone(&controls));
        router.route(MidiEvent::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        });

        for value in [1, 127] {
            router.route(MidiEvent::ControlChange {
                channel: 0,
                cc: 74,
                value,
            });
        }

        assert!(matches!(rx.recv().unwrap(), EngineMessage::NoteOn { .. }));
        let mut applied = Vec::new();
        controls.drain(|channel, control, value| applied.push((channel, control, value)));
        assert_eq!(applied, [(0, Control::FilterCutoff, 20_000.0)]);
    }

    #[test]
    fn panic_controls_are_forwarded() {
        let (tx, rx) = mpsc::sync_channel(2);
        let mut router = MidiRouter::new(tx, Arc::new(Controls::new()));

        for cc in [ALL_SOUND_OFF_CC, ALL_NOTES_OFF_CC] {
            router.route(MidiEvent::ControlChange {
                channel: 1,
                cc,
                value: 0,
            });
        }

        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::AllSoundOff { channel: 1 }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::AllNotesOff { channel: 1 }
        ));
    }

    #[test]
    fn sustain_and_reset_are_forwarded() {
        let (tx, rx) = mpsc::sync_channel(3);
        let controls = Arc::new(Controls::new());
        let mut router = MidiRouter::new(tx, Arc::clone(&controls));

        router.route(MidiEvent::PitchBend {
            channel: 2,
            value: 1.0,
        });
        for value in [127, 0] {
            router.route(MidiEvent::ControlChange {
                channel: 2,
                cc: SUSTAIN_CC,
                value,
            });
        }
        router.route(MidiEvent::ControlChange {
            channel: 2,
            cc: RESET_CONTROLLERS_CC,
            value: 0,
        });

        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::Sustain {
                channel: 2,
                down: true
            }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::Sustain {
                channel: 2,
                down: false
            }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage::ResetControllers { channel: 2 }
        ));

        let mut applied = 0;
        controls.drain(|_, _, _| applied += 1);
        assert_eq!(applied, 0);
    }
}
