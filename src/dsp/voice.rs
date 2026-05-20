/// Single synthesizer voice — the complete signal chain.
/// Signal flow:
///   [OSC1 + OSC2 + OSC3 + Noise]
///       -> filter_env modulates cutoff_hz
///       -> LFO modulates cutoff / pitch / amp
///       -> MoogFilter
///       -> amp_env scales output
///       -> out
///
/// The voice is monophonic, one MIDI note at a time.
/// The voice allocator will manage a pool of Voice objects for polyphony.
use crate::dsp::{
    envelope::Envelope,
    filter::MoogFilter,
    glide::Glide,
    lfo::{Lfo, LfoDest},
    noise::NoiseGen,
    oscillator::{Oscillator, Waveform, apply_pitch_offset, midi_to_hz},
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

    /// MIDI note currently playing (0 = none).
    pub midi_note: u8,

    // DSP units.
    osc1: Oscillator,
    osc2: Oscillator,
    osc3: Oscillator,
    #[allow(dead_code)] // I'll use it later
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
}

impl Voice {
    pub fn new(_patch: &Patch, sample_rate: f64) -> Self {
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
        }
    }

    /// Trigger a new note.
    /// `prev_held` should be true when another key is currently held (legato).
    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch, sample_rate: f64) {
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
            crate::dsp::glide::GlideMode::Always,
        );

        self.midi_note = note;
        self.velocity = velocity as f64 / 127.0;
        self.status = VoiceStatus::Active;
        self.sample_rate = sample_rate;
        self.base_hz = target_hz;

        // Trigger envelopes.
        self.amp_env.note_on();
        self.filter_env.note_on();

        // Sync LFO (optional — can make configurable later).
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

        // Placeholder patch values (replace by proper caching later).
        let osc1_hz = apply_pitch_offset(self.base_hz, 0, 0, 0.0);
        let osc2_hz = apply_pitch_offset(self.base_hz, 0, 0, 7.0);
        let osc3_hz = apply_pitch_offset(self.base_hz, -1, 0, 0.0);

        let mut mix_buf = [0.0_f64; 512];
        self.osc1
            .process(&mut mix_buf, n, osc1_hz, Waveform::Saw, self.sample_rate);
        self.osc2
            .process(&mut mix_buf, n, osc2_hz, Waveform::Saw, self.sample_rate);
        self.osc3
            .process(&mut mix_buf, n, osc3_hz, Waveform::Square, self.sample_rate);
        self.noise.process_white(&mut mix_buf, n, 0.0); // placeholder - off
        for s in mix_buf[..n].iter_mut() {
            *s *= 1.0 / 3.0; // placeholder - equal mix
        }

        let mut lfo_buf = [0.0_f64; 512];
        self.lfo.process(
            &mut lfo_buf,
            n,
            0.5,
            self.sample_rate,
            crate::dsp::lfo::LfoWaveform::Sine,
        );

        let mut fenv_buf = [0.0_f64; 512];
        let fenv_params = crate::dsp::envelope::AdsrParams {
            attack: 0.01,
            decay: 0.25,
            sustain: 0.4,
            release: 0.4,
        };
        self.filter_env.process_fill(&mut fenv_buf, n, &fenv_params);

        let base_cutoff = 800.0_f64;
        let filter_env_amount = 0.6_f64;
        let lfo_depth = 0.1_f64;
        let lfo_dest = LfoDest::Filter;

        let mut cutoff_buf = [0.0_f64; 512];
        for i in 0..n {
            let env_mod = fenv_buf[i] * filter_env_amount;

            let lfo_mod = if lfo_dest == LfoDest::Filter {
                lfo_buf[i] * lfo_depth
            } else {
                0.0
            };

            cutoff_buf[i] = (base_cutoff * 2.0_f64.powf(env_mod + lfo_mod))
                .clamp(20.0, self.sample_rate * 0.49);
        }

        let resonance = 0.3_f64;
        let mut filtered = [0.0_f64; 512];
        self.filter
            .process(&mix_buf, &cutoff_buf, resonance, &mut filtered, n);

        let amp_params = crate::dsp::envelope::AdsrParams {
            attack: 0.005,
            decay: 0.1,
            sustain: 0.8,
            release: 0.3,
        };
        self.amp_env.process_fill(&mut filtered, n, &amp_params);

        for i in 0..n {
            let s = filtered[i] * self.velocity;
            out_buf[i] += soft_clip(s);
        }

        if self.is_finished() {
            self.status = VoiceStatus::Idle;
        }
    }
}

/// Soft clipper (tanh-based) to prevent hard clipping on loud patches.
#[inline]
fn soft_clip(x: f64) -> f64 {
    x.tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_patch() -> Patch {
        Patch::default()
    }

    #[test]
    fn voice_produces_audio() {
        let sr = 48_000.0;
        let patch = default_patch();
        let mut voice = Voice::new(&patch, sr);
        voice.note_on(60, 100, &patch, sr);

        let mut buf = vec![0.0_f64; 128];
        voice.process(&mut buf, 128);

        let peak: f64 = buf.iter().cloned().map(f64::abs).fold(0.0, f64::max);
        assert!(peak > 1e-6, "voice produced silence");
    }

    #[test]
    fn voice_releases() {
        let sr = 48_000.0;
        let patch = default_patch();
        let mut voice = Voice::new(&patch, sr);
        voice.note_on(60, 100, &patch, sr);

        let mut buf = [0.0_f64; 512];
        for _ in 0..200 {
            buf.fill(0.0);
            voice.process(&mut buf, 128);
        }

        voice.note_off(&patch);

        for _ in 0..200 {
            buf.fill(0.0);
            voice.process(&mut buf, 128);
        }

        assert!(
            voice.is_finished() || voice.status == VoiceStatus::Idle,
            "voice did not finish after release"
        );
    }
}
