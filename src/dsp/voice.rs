/// Single synthesizer voice — the complete signal chain.
/// Signal flow:
///   [OSC1 + OSC2 + OSC3 + Noise]
///       -> filter_env modulates cutoff_hz
///       -> LFO pitch mod (when dest = Pitch)
///       -> MoogFilter
///             cutoff = base_cutoff
///                    + filter_env * env_amount
///                    + key_track * (note − 60 semitones)
///                    + LFO × depth (when dest = Filter)
///       -> amp_env * velocity * LFO (when dest = Amp)
///       -> soft-clip
///       -> out
use crate::dsp::{
    envelope::Envelope,
    filter::MoogFilter,
    glide::Glide,
    lfo::{Lfo, LfoDest},
    noise::NoiseGen,
    oscillator::{Oscillator, apply_pitch_offset, midi_to_hz},
    patch::Patch,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatus {
    Idle,
    Active,
    Releasing, // can be stolen
}

/// Shared channel settings borrowed for one render call.
#[derive(Clone, Copy)]
pub struct RenderParams<'a> {
    pub patch: &'a Patch,
    pub pitch_bend: f64,
    pub pressure: f64,
}

pub struct Voice {
    pub(crate) status: VoiceStatus,
    pub(crate) midi_note: u8,

    oscillators: [Oscillator; 3],
    noise: NoiseGen,
    filter: MoogFilter,
    amp_env: Envelope,
    filter_env: Envelope,
    lfo: Lfo,
    glide: Glide,

    sample_rate: f64,

    /// Velocity scaling (0.0 / 1.0).
    velocity: f64,
}

impl Voice {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            status: VoiceStatus::Idle,
            midi_note: 0,

            oscillators: std::array::from_fn(|_| Oscillator::new()),
            noise: NoiseGen::new(),
            filter: MoogFilter::new(sample_rate),
            amp_env: Envelope::new(sample_rate),
            filter_env: Envelope::new(sample_rate),
            lfo: Lfo::new(),
            glide: Glide::new(),

            sample_rate,
            velocity: 1.0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch) {
        let target_hz = midi_to_hz(note);
        self.glide.set_target(
            target_hz,
            patch.glide_time,
            self.sample_rate,
            patch.glide_type,
        );

        self.midi_note = note;
        self.velocity = velocity as f64 / 127.0;
        self.status = VoiceStatus::Active;

        // Trigger envelopes.
        self.amp_env.note_on();
        self.filter_env.note_on();

        // Sync LFO.
        self.lfo.reset();

        // Reset oscillator phases on new note.
        for oscillator in &mut self.oscillators {
            oscillator.reset();
        }
    }

    /// Begin note release.
    pub fn note_off(&mut self) {
        self.amp_env.note_off();
        self.filter_env.note_off();
        self.status = VoiceStatus::Releasing;
    }

    /// Stop immediately without running the release stage.
    pub fn stop(&mut self) {
        self.amp_env.reset();
        self.filter_env.reset();
        self.filter.reset();
        self.status = VoiceStatus::Idle;
    }

    /// `true` when the voice has fully decayed and can be reassigned.
    pub fn is_finished(&self) -> bool {
        self.amp_env.is_finished()
    }

    /// Accumulate a voice into output without allocation or a block-size limit.
    pub fn process(&mut self, out_buf: &mut [f64], params: RenderParams<'_>) {
        let p = params.patch;
        let tuning = p
            .oscillators
            .map(|osc| apply_pitch_offset(1.0, osc.octave, osc.semitone, osc.detune_ct));
        let source_level = p.oscillators.iter().map(|osc| osc.level).sum::<f64>() + p.noise_level;
        let source_gain = 1.0 / source_level.max(1.0);
        let noise_gain = p.noise_level * source_gain;
        let key_track_semitones = (self.midi_note as f64 - 60.0) * p.filter_key_track;
        let key_tracked_cutoff = p.filter_cutoff_hz * 2.0_f64.powf(key_track_semitones / 12.0);
        let pressure_mod = params.pressure * 3.0;
        let nyquist = self.sample_rate * 0.49;
        let pitch_lfo_semitones = if p.lfo_destination == LfoDest::Pitch {
            p.lfo_depth
        } else {
            0.0
        };
        let fixed_bend = if params.pitch_bend != 0.0 {
            2.0_f64.powf(params.pitch_bend / 12.0)
        } else {
            1.0
        };

        // Preserve source order and tick every component, even after release ends.
        for out in out_buf {
            let lfo = self
                .lfo
                .tick(p.lfo_rate_hz, self.sample_rate, p.lfo_waveform);
            let base = self.glide.tick();
            let pitch_factor = if pitch_lfo_semitones != 0.0 {
                let total_st = lfo * pitch_lfo_semitones + params.pitch_bend;
                if total_st != 0.0 {
                    2.0_f64.powf(total_st / 12.0)
                } else {
                    1.0
                }
            } else {
                fixed_bend
            };
            let bp = base * pitch_factor;
            let mut mix = 0.0;
            for ((oscillator, settings), tuning) in
                self.oscillators.iter_mut().zip(&p.oscillators).zip(tuning)
            {
                mix += oscillator.tick(bp * tuning, settings.waveform, self.sample_rate)
                    * settings.level;
            }
            mix *= source_gain;
            if p.noise_level > 1e-6 {
                mix += self.noise.white() * noise_gain;
            }

            let env_mod = self.filter_env.tick(&p.filter_env) * p.filter_env_amount;
            let lfo_mod = if p.lfo_destination == LfoDest::Filter {
                lfo * p.lfo_depth
            } else {
                0.0
            };
            let cutoff = (key_tracked_cutoff * 2.0_f64.powf(env_mod + lfo_mod + pressure_mod))
                .clamp(20.0, nyquist);
            let filtered =
                self.filter.tick(mix, cutoff, p.filter_resonance) * self.amp_env.tick(&p.amp_env);
            let amp_lfo = if p.lfo_destination == LfoDest::Amp {
                1.0 + lfo * p.lfo_depth
            } else {
                1.0
            };
            *out += (filtered * self.velocity * amp_lfo).tanh();
        }

        // Keep the existing block-end idle transition.
        if self.is_finished() {
            self.status = VoiceStatus::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{envelope::AdsrParams, oscillator::Waveform, patch::OscParams};

    fn render_params(patch: &Patch) -> RenderParams<'_> {
        RenderParams {
            patch,
            pitch_bend: 0.0,
            pressure: 0.0,
        }
    }

    fn bass_patch() -> Patch {
        Patch {
            oscillators: [
                OscParams {
                    level: 1.0,
                    ..Default::default()
                },
                OscParams {
                    detune_ct: 7.0,
                    level: 0.7,
                    ..Default::default()
                },
                OscParams {
                    waveform: Waveform::Square,
                    octave: -1,
                    level: 0.5,
                    ..Default::default()
                },
            ],
            filter_cutoff_hz: 800.0,
            filter_resonance: 0.3,
            filter_env_amount: 0.6,
            filter_key_track: 0.5,
            filter_env: AdsrParams {
                attack: 0.01,
                decay: 0.25,
                sustain: 0.4,
                release: 0.4,
            },
            amp_env: AdsrParams {
                release: 0.3,
                ..Default::default()
            },
            lfo_rate_hz: 0.5,
            lfo_depth: 0.1,
            ..Default::default()
        }
    }

    #[test]
    fn voice_produces_audio() {
        let sr = 48_000.0;
        let p = bass_patch();
        let mut v = Voice::new(sr);
        v.note_on(60, 100, &p);
        let mut buf = vec![0.0_f64; 128];
        v.process(&mut buf, render_params(&p));
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6, "voice produced silence");
    }

    #[test]
    fn voice_releases() {
        let sr = 48_000.0;
        let p = bass_patch();
        let mut v = Voice::new(sr);
        v.note_on(60, 100, &p);
        let mut buf = [0.0_f64; 512];
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf[..128], render_params(&p));
        }
        v.note_off();
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf[..128], render_params(&p));
        }
        assert!(v.is_finished() || v.status == VoiceStatus::Idle);
    }

    #[test]
    fn key_tracking_brightens_high_notes() {
        let sr = 48_000.0;
        let mut patch = bass_patch();
        patch.filter_cutoff_hz = 400.0;
        patch.filter_env_amount = 0.0;
        patch.amp_env = AdsrParams {
            attack: 0.001,
            decay: 0.0,
            sustain: 1.0,
            release: 0.1,
        };

        let measure_rms = |key_track: f64| {
            let patch = Patch {
                filter_key_track: key_track,
                ..patch
            };
            let mut voice = Voice::new(sr);
            let mut buf = [0.0_f64; 256];
            voice.note_on(84, 127, &patch);
            for _ in 0..20 {
                buf.fill(0.0);
                voice.process(&mut buf, render_params(&patch));
            }

            (buf.iter().map(|sample| sample * sample).sum::<f64>() / buf.len() as f64).sqrt()
        };
        let fixed = measure_rms(0.0);
        let tracked = measure_rms(1.0);

        assert!(
            tracked > fixed * 2.0,
            "key tracking had little effect: fixed={fixed:.4}, tracked={tracked:.4}"
        );
    }

    #[test]
    fn set_patch_updates_timbre() {
        let sr = 48_000.0;
        let saw = bass_patch();
        let mut square = saw;
        square.oscillators[0].waveform = Waveform::Square;
        let mut unchanged = Voice::new(sr);
        let mut changed = Voice::new(sr);
        unchanged.note_on(60, 100, &saw);
        changed.note_on(60, 100, &saw);

        let mut unchanged_buf = [0.0_f64; 128];
        let mut changed_buf = [0.0_f64; 128];
        for _ in 0..10 {
            unchanged_buf.fill(0.0);
            changed_buf.fill(0.0);
            unchanged.process(&mut unchanged_buf, render_params(&saw));
            changed.process(&mut changed_buf, render_params(&saw));
        }

        unchanged_buf.fill(0.0);
        changed_buf.fill(0.0);
        unchanged.process(&mut unchanged_buf, render_params(&saw));
        changed.process(&mut changed_buf, render_params(&square));
        let difference = unchanged_buf
            .iter()
            .zip(changed_buf)
            .map(|(unchanged, changed)| (unchanged - changed).powi(2))
            .sum::<f64>()
            .sqrt();

        assert!(
            difference > 0.1,
            "waveform change difference = {difference}"
        );
    }

    #[test]
    fn reused_voice_glides_from_last_pitch() {
        let patch = Patch {
            glide_time: 0.1,
            glide_type: crate::dsp::glide::GlideType::Lct,
            ..bass_patch()
        };
        let mut voice = Voice::new(48_000.0);
        voice.note_on(60, 100, &patch);
        assert!((voice.glide.tick() - midi_to_hz(60)).abs() < 0.1);

        voice.stop();
        voice.note_on(72, 100, &patch);
        let first = voice.glide.tick();

        assert!(first < midi_to_hz(72));
        assert!(first > midi_to_hz(60));
    }

    #[test]
    fn source_mix_keeps_headroom() {
        let mut single = bass_patch();
        single.oscillators[1].level = 0.0;
        single.oscillators[2].level = 0.0;

        let stacked = Patch {
            oscillators: [single.oscillators[0]; 3],
            ..single
        };

        let render = |patch: Patch| {
            let mut voice = Voice::new(48_000.0);
            let mut output = [0.0; 256];
            voice.note_on(60, 127, &patch);
            voice.process(&mut output, render_params(&patch));
            output
        };
        let difference = render(single)
            .iter()
            .zip(render(stacked))
            .map(|(single, stacked)| (single - stacked).abs())
            .fold(0.0_f64, f64::max);

        assert!(
            difference < 1e-12,
            "source normalization differs by {difference}"
        );
    }

    #[test]
    fn lengths_and_partitions() {
        let patch = bass_patch();
        for length in [0, 1, 64, 256, 512, 513, 1024] {
            let mut whole = Voice::new(48_000.0);
            let mut split = Voice::new(48_000.0);
            whole.note_on(60, 100, &patch);
            split.note_on(60, 100, &patch);
            let mut a = vec![0.25; length];
            let mut b = a.clone();
            whole.process(&mut a, render_params(&patch));
            for block in b.chunks_mut(63) {
                split.process(block, render_params(&patch));
            }
            assert!(a.iter().all(|s| s.is_finite()));
            for (a, b) in a.into_iter().zip(b) {
                assert!((a - b).abs() <= 1e-10);
            }
            assert_eq!(whole.status, VoiceStatus::Active);
        }
    }
}
