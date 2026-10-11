use super::{NUM_CHANNELS, channel::Channel, message::EngineMessage};
use crate::{dsp::patch::Patch, parameter::Control};

pub const BLOCK_SIZE: usize = 256;
const CHANNEL_GAIN: f64 = 0.5;
const MASTER_GAIN: f64 = 0.23;

/// Device-independent mono synthesis with bounded scratch storage.
pub struct Synth {
    channels: [Channel; NUM_CHANNELS],
    channel_mix: [f64; BLOCK_SIZE],
}

impl Synth {
    /// Requires a valid patch and a finite audio sample rate above 41 Hz.
    pub fn new(patch: Patch, sample_rate: f64) -> Self {
        Self {
            channels: std::array::from_fn(|_| Channel::new(patch, sample_rate)),
            channel_mix: [0.0; BLOCK_SIZE],
        }
    }

    pub fn handle(&mut self, message: EngineMessage) {
        if let Some(channel) = self.channels.get_mut(message.channel) {
            channel.handle(message.event);
        }
    }

    pub fn set_control(&mut self, channel: usize, control: Control, value: f64) {
        if let Some(channel) = self.channels.get_mut(channel) {
            channel.control(control, value);
        }
    }

    /// Overwrite mono output with the gained, clipped channel mix.
    pub fn render(&mut self, output: &mut [f64]) {
        for block in output.chunks_mut(BLOCK_SIZE) {
            block.fill(0.0);
            let channel_mix = &mut self.channel_mix[..block.len()];
            for channel in &mut self.channels {
                channel_mix.fill(0.0);
                channel.process(channel_mix);
                for (mix, sample) in block.iter_mut().zip(channel_mix.iter()) {
                    *mix += sample * CHANNEL_GAIN;
                }
            }
            for sample in block {
                *sample = (*sample * MASTER_GAIN).clamp(-1.0, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{allocator::MAX_VOICES, message::EngineEvent};

    #[test]
    fn ignores_unsupported_events() {
        let mut synth = Synth::new(Patch::default(), 48_000.0);
        for event in [
            EngineEvent::NoteOn {
                note: 60,
                velocity: 100,
            },
            EngineEvent::NoteOff { note: 60 },
            EngineEvent::AllNotesOff,
            EngineEvent::AllSoundOff,
            EngineEvent::Sustain { down: true },
            EngineEvent::ResetControllers,
        ] {
            synth.handle(EngineMessage {
                channel: usize::MAX,
                event,
            });
        }
        let mut output = [1.0; 256];
        synth.render(&mut output);
        assert!(output.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn lengths_and_channels() {
        for length in [0, 1, 64, 256, 513, 1024] {
            let mut synth = Synth::new(Patch::default(), 48_000.0);
            synth.handle(EngineMessage {
                channel: NUM_CHANNELS,
                event: EngineEvent::NoteOn {
                    note: 60,
                    velocity: 100,
                },
            });
            synth.set_control(NUM_CHANNELS, Control::PitchBend, 2.0);
            let mut output = vec![f64::NAN; length];
            synth.render(&mut output);
            assert!(output.iter().all(|s| *s == 0.0));
            synth.handle(EngineMessage {
                channel: 0,
                event: EngineEvent::NoteOn {
                    note: 60,
                    velocity: 100,
                },
            });
            synth.render(&mut output);
            assert!(output.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn gain_stays_clean_under_load() {
        for voices in [1, 8, 32] {
            let patch = Patch {
                filter_cutoff_hz: 10_000.0,
                ..Default::default()
            };
            let mut synth = Synth::new(patch, 48_000.0);
            for voice in 0..voices {
                synth.handle(EngineMessage {
                    channel: voice / MAX_VOICES,
                    event: EngineEvent::NoteOn {
                        note: 48 + (voice % MAX_VOICES) as u8,
                        velocity: 127,
                    },
                });
            }
            let mut output = vec![0.0; 24_000];
            synth.render(&mut output);
            let samples = &output[2_048..];
            let peak = samples.iter().copied().map(f64::abs).fold(0.0, f64::max);
            let rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
            let clipped = samples.iter().filter(|s| s.abs() >= 1.0).count();
            assert!(rms > 0.001, "{voices} voices produced silence");
            assert!(rms < 0.35, "{voices} voices reached {rms:.3} RMS");
            assert!(peak < 0.9, "{voices} voices peaked at {peak:.3}");
            assert_eq!(clipped, 0);
        }
    }
}
