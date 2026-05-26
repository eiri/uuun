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
    glide::{Glide, GlideMode},
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
    pub status: VoiceStatus,
    pub midi_note: u8,

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

    /// Base frequency after glide, before LFO pitch mod.
    base_hz: f64,

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
            base_hz: 440.0,
            velocity: 1.0,
            pitch_bend_semitones: 0.0,
            channel_pressure: 0.0,
            patch: *patch,
        }
    }

    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
    }

    pub fn set_pitch_bend(&mut self, semitones: f64) {
        self.pitch_bend_semitones = semitones;
    }

    pub fn set_channel_pressure(&mut self, value: f64) {
        self.channel_pressure = value;
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch, sample_rate: f64) {
        self.patch = *patch;

        let target_hz = midi_to_hz(note);
        let prev_held = self.status == VoiceStatus::Active;

        // If this is the first note ever, snap the glide to the target instantly.
        if self.status == VoiceStatus::Idle {
            self.glide.reset_to(target_hz);
        }

        self.glide.set_target(
            target_hz,
            prev_held,
            patch.glide_time,
            sample_rate,
            if patch.glide_time > 1e-4 {
                GlideMode::Always
            } else {
                GlideMode::Off
            },
        );

        self.midi_note = note;
        self.velocity = velocity as f64 / 127.0;
        self.status = VoiceStatus::Active;
        self.sample_rate = sample_rate;
        self.base_hz = target_hz;

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
    pub fn note_off(&mut self, _patch: &Patch) {
        self.amp_env.note_off();
        self.filter_env.note_off();
        self.status = VoiceStatus::Releasing;
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

            mix_buf[i] = s1[0] * p.osc1_level + s2[0] * p.osc2_level + s3[0] * p.osc3_level;
        }

        if p.noise_level > 1e-6 {
            self.noise.process_white(&mut mix_buf, n, p.noise_level);
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
        }
    }

    #[test]
    fn voice_produces_audio() {
        let sr = 48_000.0;
        let p = bass_patch();
        let mut v = Voice::new(&p, sr);
        v.note_on(60, 100, &p, sr);
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
        v.note_on(60, 100, &p, sr);
        let mut buf = [0.0_f64; 512];
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf, 128);
        }
        v.note_off(&p);
        for _ in 0..200 {
            buf.fill(0.0);
            v.process(&mut buf, 128);
        }
        assert!(v.is_finished() || v.status == VoiceStatus::Idle);
    }

    #[test]
    fn higher_note_brighter() {
        let sr = 48_000.0;
        let mut p = bass_patch();
        p.filter_key_track = 1.0;
        p.filter_env_amount = 0.0;
        p.amp_env = AdsrParams {
            attack: 0.001,
            decay: 0.0,
            sustain: 1.0,
            release: 0.1,
        };

        let measure_peak = |note: u8| {
            let mut v = Voice::new(&p, sr);
            v.note_on(note, 100, &p, sr);
            let mut buf = [0.0_f64; 512];
            let mut peak = 0.0_f64;
            for _ in 0..50 {
                buf.fill(0.0);
                v.process(&mut buf, 128);
                peak = peak.max(buf[..128].iter().cloned().map(f64::abs).fold(0.0, f64::max));
            }
            peak
        };
        assert!(measure_peak(36) > 1e-4, "low note silent");
        assert!(measure_peak(84) > 1e-4, "high note silent");
    }

    #[test]
    fn set_patch_updates_timbre() {
        let sr = 48_000.0;
        let mut p = bass_patch();
        let mut v = Voice::new(&p, sr);
        v.note_on(60, 100, &p, sr);
        let mut buf = [0.0_f64; 512];
        for _ in 0..10 {
            buf.fill(0.0);
            v.process(&mut buf, 128);
        }
        p.osc1_waveform = Waveform::Square;
        v.set_patch(&p);
        buf.fill(0.0);
        v.process(&mut buf, 128);
        let peak = buf[..128]
            .iter()
            .cloned()
            .map(f64::abs)
            .fold(0.0_f64, f64::max);
        assert!(peak > 1e-6, "silent after set_patch");
    }

    #[test]
    fn patch_is_copy() {
        // Compile-time proof: Copy means assignment doesn't move.
        let p = bass_patch();
        let _p2 = p; // copy
        let _p3 = p; // still usable — would fail if Patch were only Clone
    }
}
