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

pub struct Voice {
    pub(crate) status: VoiceStatus,
    pub(crate) midi_note: u8,

    osc1: Oscillator,
    osc2: Oscillator,
    osc3: Oscillator,
    noise: NoiseGen,
    filter: MoogFilter,
    amp_env: Envelope,
    filter_env: Envelope,
    lfo: Lfo,
    glide: Glide,

    sample_rate: f64,

    /// Velocity scaling (0.0 / 1.0).
    velocity: f64,

    /// Pitch-bend offset in semitones, applied on top of glide output.
    pitch_bend_semitones: f64,

    /// Channel aftertouch value (0.0..1.0), modulates filter cutoff.
    channel_pressure: f64,

    patch: Patch,
}

impl Voice {
    pub fn new(patch: &Patch, sample_rate: f64) -> Self {
        Self {
            status: VoiceStatus::Idle,
            midi_note: 0,

            osc1: Oscillator::new(),
            osc2: Oscillator::new(),
            osc3: Oscillator::new(),
            noise: NoiseGen::new(),
            filter: MoogFilter::new(sample_rate),
            amp_env: Envelope::new(sample_rate),
            filter_env: Envelope::new(sample_rate),
            lfo: Lfo::new(),
            glide: Glide::new(),

            sample_rate,
            velocity: 1.0,
            pitch_bend_semitones: 0.0,
            channel_pressure: 0.0,
            patch: *patch,
        }
    }

    pub(crate) fn apply_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
    }

    pub fn set_pitch_bend(&mut self, semitones: f64) {
        self.pitch_bend_semitones = semitones;
    }

    pub fn set_channel_pressure(&mut self, value: f64) {
        self.channel_pressure = value;
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch) {
        self.patch = *patch;

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
        self.osc1.reset();
        self.osc2.reset();
        self.osc3.reset();
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

    /// Render `n` samples into `out_buf` (accumulated, not overwritten).
    pub fn process(&mut self, out_buf: &mut [f64], n: usize) {
        // Re-borrow fields to avoid borrowing `self` twice inside the loop.
        // We work through scratch buffers on the stack for small block sizes.
        // 512 samples covers the largest practical block size.
        debug_assert!(
            n <= 512,
            "block size > 512 not supported without heap alloc"
        );

        let p = self.patch;

        let mut lfo_buf = [0.0_f64; 512];
        self.lfo.process(
            &mut lfo_buf,
            n,
            p.lfo_rate_hz,
            self.sample_rate,
            p.lfo_waveform,
        );

        let mut freq_buf = [0.0_f64; 512];
        self.glide.process(&mut freq_buf, n);

        let pitch_lfo_semitones = if p.lfo_destination == LfoDest::Pitch {
            p.lfo_depth
        } else {
            0.0
        };

        let source_level = p.osc1_level + p.osc2_level + p.osc3_level + p.noise_level;
        let source_gain = 1.0 / source_level.max(1.0);

        let mut mix_buf = [0.0_f64; 512];
        for i in 0..n {
            let base = freq_buf[i];
            let pitch_factor = {
                let lfo_st = if pitch_lfo_semitones != 0.0 {
                    lfo_buf[i] * pitch_lfo_semitones
                } else {
                    0.0
                };
                let total_st = lfo_st + self.pitch_bend_semitones;
                if total_st != 0.0 {
                    2.0_f64.powf(total_st / 12.0)
                } else {
                    1.0
                }
            };
            let bp = base * pitch_factor;

            let mut s1 = [0.0_f64; 1];
            let mut s2 = [0.0_f64; 1];
            let mut s3 = [0.0_f64; 1];
            self.osc1.process(
                &mut s1,
                1,
                apply_pitch_offset(bp, p.osc1_octave, p.osc1_semitone, p.osc1_detune_ct),
                p.osc1_waveform,
                self.sample_rate,
            );
            self.osc2.process(
                &mut s2,
                1,
                apply_pitch_offset(bp, p.osc2_octave, p.osc2_semitone, p.osc2_detune_ct),
                p.osc2_waveform,
                self.sample_rate,
            );
            self.osc3.process(
                &mut s3,
                1,
                apply_pitch_offset(bp, p.osc3_octave, p.osc3_semitone, p.osc3_detune_ct),
                p.osc3_waveform,
                self.sample_rate,
            );

            mix_buf[i] =
                (s1[0] * p.osc1_level + s2[0] * p.osc2_level + s3[0] * p.osc3_level) * source_gain;
        }

        if p.noise_level > 1e-6 {
            self.noise
                .process_white(&mut mix_buf, n, p.noise_level * source_gain);
        }

        let mut fenv_buf = [0.0_f64; 512];
        self.filter_env
            .process_fill(&mut fenv_buf, n, &p.filter_env);

        let key_track_semitones = (self.midi_note as f64 - 60.0) * p.filter_key_track;
        let key_tracked_cutoff = p.filter_cutoff_hz * 2.0_f64.powf(key_track_semitones / 12.0);
        let lfo_to_filter = p.lfo_destination == LfoDest::Filter;
        let nyquist = self.sample_rate * 0.49;

        let mut cutoff_buf = [0.0_f64; 512];
        for i in 0..n {
            let env_mod = fenv_buf[i] * p.filter_env_amount;
            let lfo_mod = if lfo_to_filter {
                lfo_buf[i] * p.lfo_depth
            } else {
                0.0
            };
            // Channel aftertouch opens the filter by up to +3 octaves.
            let pressure_mod = self.channel_pressure * 3.0;
            cutoff_buf[i] = (key_tracked_cutoff * 2.0_f64.powf(env_mod + lfo_mod + pressure_mod))
                .clamp(20.0, nyquist);
        }

        let mut filtered = [0.0_f64; 512];
        self.filter
            .process(&mix_buf, &cutoff_buf, p.filter_resonance, &mut filtered, n);

        self.amp_env.process_mul(&mut filtered, n, &p.amp_env);

        let lfo_to_amp = p.lfo_destination == LfoDest::Amp;
        for i in 0..n {
            let amp_lfo = if lfo_to_amp {
                1.0 + lfo_buf[i] * p.lfo_depth
            } else {
                1.0
            };
            // Soft clipper (tanh-based)
            out_buf[i] += (filtered[i] * self.velocity * amp_lfo).tanh();
        }

        if self.is_finished() {
            self.status = VoiceStatus::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{
        envelope::AdsrParams,
        lfo::{LfoDest, LfoWaveform},
        oscillator::Waveform,
    };

    fn bass_patch() -> Patch {
        Patch {
            osc1_waveform: Waveform::Saw,
            osc1_octave: 0,
            osc1_semitone: 0,
            osc1_detune_ct: 0.0,
            osc1_level: 1.0,
            osc2_waveform: Waveform::Saw,
            osc2_octave: 0,
            osc2_semitone: 0,
            osc2_detune_ct: 7.0,
            osc2_level: 0.7,
            osc3_waveform: Waveform::Square,
            osc3_octave: -1,
            osc3_semitone: 0,
            osc3_detune_ct: 0.0,
            osc3_level: 0.5,
            noise_level: 0.0,
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
                attack: 0.005,
                decay: 0.1,
                sustain: 0.8,
                release: 0.3,
            },
            lfo_waveform: LfoWaveform::Sine,
            lfo_rate_hz: 0.5,
            lfo_depth: 0.1,
            lfo_destination: LfoDest::Filter,
            glide_time: 0.0,
            glide_type: crate::dsp::glide::GlideType::Lcr,
        }
    }

    #[test]
    fn voice_produces_audio() {
        let sr = 48_000.0;
        let p = bass_patch();
        let mut v = Voice::new(&p, sr);
        v.note_on(60, 100, &p);
        let mut buf = vec![0.0_f64; 128];
        v.process(&mut buf, 128);
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6, "voice produced silence");
    }

    #[test]
    fn voice_releases() {
        let sr = 48_000.0;
        let p = bass_patch();
        let mut v = Voice::new(&p, sr);
        v.note_on(60, 100, &p);
        let mut buf = [0.0_f64; 512];
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf, 128);
        }
        v.note_off();
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf, 128);
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
            let mut voice = Voice::new(&patch, sr);
            let mut buf = [0.0_f64; 256];
            voice.note_on(84, 127, &patch);
            for _ in 0..20 {
                buf.fill(0.0);
                voice.process(&mut buf, 256);
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
        let square = Patch {
            osc1_waveform: Waveform::Square,
            ..saw
        };
        let mut unchanged = Voice::new(&saw, sr);
        let mut changed = Voice::new(&saw, sr);
        unchanged.note_on(60, 100, &saw);
        changed.note_on(60, 100, &saw);

        let mut unchanged_buf = [0.0_f64; 128];
        let mut changed_buf = [0.0_f64; 128];
        for _ in 0..10 {
            unchanged_buf.fill(0.0);
            changed_buf.fill(0.0);
            unchanged.process(&mut unchanged_buf, 128);
            changed.process(&mut changed_buf, 128);
        }

        changed.apply_patch(&square);
        unchanged_buf.fill(0.0);
        changed_buf.fill(0.0);
        unchanged.process(&mut unchanged_buf, 128);
        changed.process(&mut changed_buf, 128);
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
        let mut voice = Voice::new(&patch, 48_000.0);
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
        single.osc2_level = 0.0;
        single.osc3_level = 0.0;

        let stacked = Patch {
            osc2_waveform: single.osc1_waveform,
            osc2_octave: single.osc1_octave,
            osc2_semitone: single.osc1_semitone,
            osc2_detune_ct: single.osc1_detune_ct,
            osc2_level: 1.0,
            osc3_waveform: single.osc1_waveform,
            osc3_octave: single.osc1_octave,
            osc3_semitone: single.osc1_semitone,
            osc3_detune_ct: single.osc1_detune_ct,
            osc3_level: 1.0,
            ..single
        };

        let render = |patch: Patch| {
            let mut voice = Voice::new(&patch, 48_000.0);
            let mut output = [0.0; 256];
            voice.note_on(60, 127, &patch);
            voice.process(&mut output, 256);
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
    fn patch_is_copy() {
        // Compile-time proof: Copy means assignment doesn't move.
        let p = bass_patch();
        let _p2 = p; // copy
        let _p3 = p; // still usable — would fail if Patch were only Clone
    }
}
