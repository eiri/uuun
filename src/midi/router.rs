use std::sync::mpsc;

use crate::{
    dsp::patch::Patch,
    engine::{audio::NUM_CHANNELS, message::EngineMessage},
    midi::{
        cc_map::{CcTarget, cc_target},
        message::MidiEvent,
    },
};

/// Maximum pitch-bend range in semitones, +2/-2
const PITCH_BEND_SEMITONES: f64 = 2.0;

/// Routes `MidiEvent` values (from the MIDI thread) into `EngineMessage`
/// values (to the audio engine thread).
pub struct MidiRouter {
    // Per-channel patch cache
    patches: Vec<Patch>,
    engine_tx: mpsc::SyncSender<EngineMessage>,
}

impl MidiRouter {
    pub fn new(initial_patch: Patch, engine_tx: mpsc::SyncSender<EngineMessage>) -> Self {
        Self {
            patches: vec![initial_patch; NUM_CHANNELS],
            engine_tx,
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
                self.send(EngineMessage::NoteOff { channel: ch, note });
            }

            MidiEvent::ControlChange { channel, cc, value } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
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
                self.send(EngineMessage::PitchBend {
                    channel: ch,
                    semitones,
                });
            }

            MidiEvent::ChannelPressure { channel, value } => {
                let ch = channel as usize;
                if ch >= NUM_CHANNELS {
                    return;
                }
                self.send(EngineMessage::ChannelPressure { channel: ch, value });
            }
        }
    }

    fn send(&self, msg: EngineMessage) {
        // Non-blocking: if the engine queue is full - drop the message
        let _ = self.engine_tx.try_send(msg);
    }

    fn apply_cc(&mut self, ch: usize, target: CcTarget, v: f64) {
        let p = &mut self.patches[ch];

        match target {
            CcTarget::FilterCutoff => {
                // Map 0..1 exponentially to 20 Hz .. 20 kHz.
                p.filter_cutoff_hz = 20.0 * 1000.0_f64.powf(v);
            }
            CcTarget::FilterResonance => {
                p.filter_resonance = v.clamp(0.0, 0.99);
            }
            CcTarget::FilterEnvAmount => {
                p.filter_env_amount = v;
            }
            CcTarget::FilterKeyTrack => {
                p.filter_key_track = v;
            }
            CcTarget::AmpAttack => {
                p.amp_env.attack = map_adsr_time(v);
            }
            CcTarget::AmpDecay => {
                p.amp_env.decay = map_adsr_time(v);
            }
            CcTarget::AmpSustain => {
                p.amp_env.sustain = v;
            }
            CcTarget::AmpRelease => {
                p.amp_env.release = map_adsr_time(v);
            }
            CcTarget::FilterAttack => {
                p.filter_env.attack = map_adsr_time(v);
            }
            CcTarget::FilterDecay => {
                p.filter_env.decay = map_adsr_time(v);
            }
            CcTarget::FilterSustain => {
                p.filter_env.sustain = v;
            }
            CcTarget::FilterRelease => {
                p.filter_env.release = map_adsr_time(v);
            }
            CcTarget::LfoRate => {
                // 0..1 -> 0.01..30 Hz (exponential feels natural for rate).
                p.lfo_rate_hz = 0.01 * 3000.0_f64.powf(v);
            }
            CcTarget::LfoDepth => {
                p.lfo_depth = v;
            }
            CcTarget::Osc1Level => {
                p.osc1_level = v;
            }
            CcTarget::Osc2Level => {
                p.osc2_level = v;
            }
            CcTarget::Osc3Level => {
                p.osc3_level = v;
            }
            CcTarget::NoiseLevel => {
                p.noise_level = v;
            }
            CcTarget::GlideTime => {
                // up to 4 seconds
                p.glide_time = v * 4.0;
            }
            CcTarget::Unassigned => {}
        }

        let patch_copy = *p;
        self.send(EngineMessage::SetPatch {
            channel: ch,
            patch: Box::new(patch_copy),
        });
    }
}

fn map_adsr_time(v: f64) -> f64 {
    0.001 * 10_000.0_f64.powf(v)
}
