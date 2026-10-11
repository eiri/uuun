use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

use crate::{
    engine::{
        NUM_CHANNELS,
        control::Controls,
        message::{EngineEvent, EngineMessage},
    },
    midi::{
        cc_map::{
            ALL_NOTES_OFF_CC, ALL_SOUND_OFF_CC, RESET_CONTROLLERS_CC, SUSTAIN_CC, cc_target,
            cc_value,
        },
        message::MidiEvent,
    },
    parameter::Control,
};

/// Maximum pitch-bend range in semitones, +2/-2
const PITCH_BEND_SEMITONES: f64 = 2.0;

/// Counts note-ons rejected by a full engine queue.
#[derive(Clone, Default)]
pub struct MidiDiagnostics {
    dropped_note_ons: Arc<AtomicU64>,
}

impl MidiDiagnostics {
    pub fn dropped_note_ons(&self) -> u64 {
        self.dropped_note_ons.load(Ordering::Relaxed)
    }
}

/// Routes MIDI events into the engine event queue and control mailboxes.
pub struct MidiRouter {
    engine_tx: mpsc::SyncSender<EngineMessage>,
    controls: Arc<Controls>,
    diagnostics: MidiDiagnostics,
}

impl MidiRouter {
    pub fn new(engine_tx: mpsc::SyncSender<EngineMessage>, controls: Arc<Controls>) -> Self {
        Self {
            engine_tx,
            controls,
            diagnostics: MidiDiagnostics::default(),
        }
    }

    pub fn diagnostics(&self) -> MidiDiagnostics {
        self.diagnostics.clone()
    }

    pub fn route(&self, event: MidiEvent) {
        let channel = usize::from(event.channel());
        if channel >= NUM_CHANNELS {
            return;
        }

        match event {
            MidiEvent::NoteOn { note, velocity, .. } => {
                self.send_note_on(EngineMessage {
                    channel,
                    event: EngineEvent::NoteOn { note, velocity },
                });
            }
            MidiEvent::NoteOff { note, .. } => {
                self.send_critical(EngineMessage {
                    channel,
                    event: EngineEvent::NoteOff { note },
                });
            }
            MidiEvent::ControlChange { cc, value, .. } => {
                let event = match cc {
                    SUSTAIN_CC => Some(EngineEvent::Sustain { down: value >= 64 }),
                    ALL_SOUND_OFF_CC => Some(EngineEvent::AllSoundOff),
                    RESET_CONTROLLERS_CC => {
                        // Clear pending runtime controls before submitting reset.
                        self.controls
                            .clear(channel, &[Control::PitchBend, Control::ChannelPressure]);
                        Some(EngineEvent::ResetControllers)
                    }
                    ALL_NOTES_OFF_CC => Some(EngineEvent::AllNotesOff),
                    _ => None,
                };
                if let Some(event) = event {
                    self.send_critical(EngineMessage { channel, event });
                } else if let Some(control) = cc_target(cc)
                    && let Some(value) = cc_value(control, value as f64 / 127.0)
                {
                    self.controls.set(channel, control, value);
                }
            }
            MidiEvent::PitchBend { value, .. } => {
                self.controls
                    .set(channel, Control::PitchBend, value * PITCH_BEND_SEMITONES);
            }
            MidiEvent::ChannelPressure { value, .. } => {
                self.controls.set(channel, Control::ChannelPressure, value);
            }
        }
    }

    fn send_note_on(&self, msg: EngineMessage) {
        if matches!(
            self.engine_tx.try_send(msg),
            Err(mpsc::TrySendError::Full(_))
        ) {
            self.diagnostics
                .dropped_note_ons
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn send_critical(&self, msg: EngineMessage) {
        // Releases must survive a full queue to prevent stuck notes.
        let _ = self.engine_tx.send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_preserves_patch_mailboxes() {
        let (tx, rx) = mpsc::sync_channel(2);
        let controls = Arc::new(Controls::new());
        let router = MidiRouter::new(tx, Arc::clone(&controls));
        router.route(MidiEvent::PitchBend {
            channel: 0,
            value: 1.0,
        });
        router.route(MidiEvent::ChannelPressure {
            channel: 0,
            value: 0.8,
        });
        router.route(MidiEvent::ControlChange {
            channel: 0,
            cc: 74,
            value: 127,
        });
        router.route(MidiEvent::ControlChange {
            channel: 0,
            cc: 121,
            value: 0,
        });
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 0,
                event: EngineEvent::ResetControllers
            }
        ));
        let mut applied = Vec::new();
        controls.drain(|ch, control, value| applied.push((ch, control, value)));
        assert_eq!(applied, [(0, Control::FilterCutoff, 20_000.0)]);

        router.route(MidiEvent::ControlChange {
            channel: 0,
            cc: 121,
            value: 0,
        });
        router.route(MidiEvent::PitchBend {
            channel: 0,
            value: -0.5,
        });
        applied.clear();
        controls.drain(|ch, control, value| applied.push((ch, control, value)));
        assert_eq!(applied, [(0, Control::PitchBend, -1.0)]);
    }

    #[test]
    fn ignores_unsupported_channels_and_ccs() {
        let (tx, rx) = mpsc::sync_channel(8);
        let controls = Arc::new(Controls::new());
        let router = MidiRouter::new(tx, Arc::clone(&controls));
        for channel in [4, 15] {
            for event in [
                MidiEvent::NoteOn {
                    channel,
                    note: 60,
                    velocity: 100,
                },
                MidiEvent::NoteOff { channel, note: 60 },
                MidiEvent::PitchBend {
                    channel,
                    value: 1.0,
                },
                MidiEvent::ChannelPressure {
                    channel,
                    value: 1.0,
                },
                MidiEvent::ControlChange {
                    channel,
                    cc: 121,
                    value: 0,
                },
                MidiEvent::ControlChange {
                    channel,
                    cc: 74,
                    value: 127,
                },
            ] {
                router.route(event);
            }
        }
        router.route(MidiEvent::ControlChange {
            channel: 0,
            cc: 3,
            value: 127,
        });
        assert!(rx.try_recv().is_err());
        controls.drain(|_, _, _| panic!("unexpected control"));
    }

    #[test]
    fn note_off_survives_full_queue() {
        let (tx, rx) = mpsc::sync_channel(1);
        let router = MidiRouter::new(tx, Arc::new(Controls::new()));
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

        assert_eq!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 0,
                event: EngineEvent::NoteOn {
                    note: 60,
                    velocity: 100
                }
            }
        );
        assert_eq!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 0,
                event: EngineEvent::NoteOff { note: 60 }
            }
        );
        handle.join().unwrap();
    }

    #[test]
    fn note_on_drops_are_counted() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let router = MidiRouter::new(tx, Arc::new(Controls::new()));
        let diagnostics = router.diagnostics();

        for note in [60, 61] {
            router.route(MidiEvent::NoteOn {
                channel: 0,
                note,
                velocity: 100,
            });
        }

        assert_eq!(diagnostics.dropped_note_ons(), 1);
    }

    #[test]
    fn controls_keep_latest_when_queue_is_full() {
        let (tx, rx) = mpsc::sync_channel(1);
        let controls = Arc::new(Controls::new());
        let router = MidiRouter::new(tx, Arc::clone(&controls));
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

        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                event: EngineEvent::NoteOn { .. },
                ..
            }
        ));
        let mut applied = Vec::new();
        controls.drain(|channel, control, value| applied.push((channel, control, value)));
        assert_eq!(applied, [(0, Control::FilterCutoff, 20_000.0)]);
    }

    #[test]
    fn mapped_cc_endpoints_are_applied() {
        let endpoints = [
            (5, Control::GlideTime, 0.0, 4.0),
            (71, Control::FilterResonance, 0.0, 0.99),
            (72, Control::AmpRelease, 0.001, 10.0),
            (73, Control::AmpAttack, 0.001, 10.0),
            (74, Control::FilterCutoff, 20.0, 20_000.0),
            (75, Control::AmpDecay, 0.001, 10.0),
            (76, Control::LfoRate, 0.01, 30.0),
            (77, Control::LfoDepth, 0.0, 1.0),
            (79, Control::AmpSustain, 0.0, 1.0),
            (85, Control::FilterKeyTrack, 0.0, 1.0),
            (86, Control::FilterEnvAmount, 0.0, 1.0),
            (102, Control::FilterAttack, 0.001, 10.0),
            (103, Control::FilterDecay, 0.001, 10.0),
            (104, Control::FilterSustain, 0.0, 1.0),
            (105, Control::FilterRelease, 0.001, 10.0),
            (106, Control::Osc1Level, 0.0, 1.0),
            (107, Control::Osc2Level, 0.0, 1.0),
            (108, Control::Osc3Level, 0.0, 1.0),
            (109, Control::NoiseLevel, 0.0, 1.0),
        ];

        for (cc, control, low, high) in endpoints {
            assert_cc_value(cc, 0, control, low);
            assert_cc_value(cc, 127, control, high);
        }
    }

    fn assert_cc_value(cc: u8, input: u8, expected_control: Control, expected_value: f64) {
        let (tx, _rx) = mpsc::sync_channel(1);
        let controls = Arc::new(Controls::new());
        let router = MidiRouter::new(tx, Arc::clone(&controls));
        router.route(MidiEvent::ControlChange {
            channel: 0,
            cc,
            value: input,
        });

        let mut applied = Vec::new();
        controls.drain(|channel, control, value| applied.push((channel, control, value)));
        assert_eq!(applied.len(), 1, "CC {cc} value {input}");

        let (channel, control, value) = applied[0];
        assert_eq!(channel, 0);
        assert_eq!(control, expected_control, "CC {cc}");
        assert!(
            (value - expected_value).abs() < 1e-9,
            "CC {cc} value {input}: {value}, expected {expected_value}"
        );
    }

    #[test]
    fn panic_controls_are_forwarded() {
        let (tx, rx) = mpsc::sync_channel(2);
        let router = MidiRouter::new(tx, Arc::new(Controls::new()));

        for cc in [ALL_SOUND_OFF_CC, ALL_NOTES_OFF_CC] {
            router.route(MidiEvent::ControlChange {
                channel: 1,
                cc,
                value: 0,
            });
        }

        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 1,
                event: EngineEvent::AllSoundOff
            }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 1,
                event: EngineEvent::AllNotesOff
            }
        ));
    }

    #[test]
    fn sustain_and_reset_are_forwarded() {
        let (tx, rx) = mpsc::sync_channel(3);
        let controls = Arc::new(Controls::new());
        let router = MidiRouter::new(tx, Arc::clone(&controls));

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
            EngineMessage {
                channel: 2,
                event: EngineEvent::Sustain { down: true }
            }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 2,
                event: EngineEvent::Sustain { down: false }
            }
        ));
        assert!(matches!(
            rx.recv().unwrap(),
            EngineMessage {
                channel: 2,
                event: EngineEvent::ResetControllers
            }
        ));

        let mut applied = 0;
        controls.drain(|_, _, _| applied += 1);
        assert_eq!(applied, 0);
    }
}
