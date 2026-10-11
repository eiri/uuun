use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

/// PolyBLEP correction for the discontinuity at phase = 0 (or 1).
/// `t` is the phase normalised to [0,1), `dt` = freq/sample_rate.
// lifted from https://github.com/eiri/gregory oscillator
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

pub struct Oscillator {
    phase: f64,
    /// Integrated value used for triangle generation.
    tri_integrator: f64,
}

impl Oscillator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            tri_integrator: 0.0,
        }
    }

    /// Reset phase to 0 (used on note-on for hard sync if desired).
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.tri_integrator = 0.0;
    }

    /// Render `n` samples of the chosen waveform at `freq` Hz into `buf`.
    /// Frequencies at or above Nyquist are muted to prevent aliasing.
    /// `buf` is accumulated into (not overwritten) so multiple oscillators can
    /// be summed in the voice without an extra allocation.
    #[cfg(test)]
    pub fn process(
        &mut self,
        buf: &mut [f64],
        n: usize,
        freq: f64,
        waveform: Waveform,
        sample_rate: f64,
    ) {
        for s in buf[..n].iter_mut() {
            *s += self.tick(freq, waveform, sample_rate);
        }
    }

    /// Sample once; muted pitches advance phase at the capped frequency.
    #[inline]
    pub fn tick(&mut self, freq: f64, waveform: Waveform, sample_rate: f64) -> f64 {
        if !freq.is_finite() || !sample_rate.is_finite() || sample_rate <= 0.0 {
            return 0.0;
        }

        let nyquist = sample_rate * 0.5;
        let muted = freq >= nyquist || freq < 0.0;
        let dt = freq.clamp(0.0, nyquist) / sample_rate;
        if muted {
            self.phase = (self.phase + dt).rem_euclid(1.0);
            return 0.0;
        }

        let sample = match waveform {
            Waveform::Sine => {
                use std::f64::consts::TAU;
                (self.phase * TAU).sin()
            }
            Waveform::Saw => {
                let naive = 2.0 * self.phase - 1.0;
                naive - poly_blep(self.phase, dt)
            }
            Waveform::Square | Waveform::Triangle => {
                let naive: f64 = if self.phase < 0.5 { 1.0 } else { -1.0 };
                let half = (self.phase + 0.5).fract();
                let square = naive + poly_blep(self.phase, dt) - poly_blep(half, dt);
                // Integrate a square wave and rescale with the existing leak.
                if waveform == Waveform::Triangle {
                    self.tri_integrator =
                        self.tri_integrator * (1.0 - dt * 0.01) + 4.0 * dt * square;
                    self.tri_integrator
                } else {
                    square
                }
            }
        };
        self.phase = (self.phase + dt).rem_euclid(1.0);
        sample
    }
}

impl Default for Oscillator {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a MIDI note number (0–127) to frequency in Hz.
/// A4 = MIDI 69 = 440 Hz.
// lifted from https://github.com/eiri/gregory engine
#[inline]
pub fn midi_to_hz(note: u8) -> f64 {
    440.0 * 2.0_f64.powf((note as f64 - 69.0) / 12.0)
}

/// Apply octave, semitone, and cent offsets to a base frequency.
#[inline]
pub fn apply_pitch_offset(base_hz: f64, octave: i8, semitone: i8, detune_ct: f64) -> f64 {
    let semitones = (octave as f64) * 12.0 + (semitone as f64) + detune_ct / 100.0;
    base_hz * 2.0_f64.powf(semitones / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;
    const N: usize = 4800; // 100 ms

    fn render(waveform: Waveform) -> Vec<f64> {
        let mut osc = Oscillator::new();
        let mut buf = vec![0.0_f64; N];
        osc.process(&mut buf, N, 440.0, waveform, SR);
        buf
    }

    #[test]
    fn scalar_matches_wrapper() {
        for waveform in [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ] {
            for freq in [
                0.0,
                -1.0,
                f64::NAN,
                f64::INFINITY,
                440.0,
                SR * 0.499,
                SR * 0.5,
                SR * 2.0,
            ] {
                let mut scalar = Oscillator::new();
                let mut block = Oscillator::new();
                let mut samples = [0.25; 257];
                block.process(&mut samples, 257, freq, waveform, SR);
                for sample in samples {
                    assert_eq!(sample, 0.25 + scalar.tick(freq, waveform, SR));
                }
                assert_eq!(scalar.phase, block.phase);
                assert_eq!(scalar.tri_integrator, block.tri_integrator);
            }
        }
    }

    #[test]
    fn silent_phase_policy() {
        let mut osc = Oscillator::new();
        osc.tick(440.0, Waveform::Sine, SR);
        let phase = osc.phase;
        for freq in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(osc.tick(freq, Waveform::Triangle, SR), 0.0);
            assert_eq!(osc.phase, phase);
            assert_eq!(osc.tri_integrator, 0.0);
        }
        assert_eq!(osc.tick(0.0, Waveform::Saw, SR), 2.0 * phase - 1.0);
        assert_eq!(osc.phase, phase);
        assert_eq!(osc.tick(SR, Waveform::Triangle, SR), 0.0);
        assert_eq!(osc.phase, (phase + 0.5).rem_euclid(1.0));
        assert_eq!(osc.tri_integrator, 0.0);
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let phase = osc.phase;
            assert_eq!(osc.tick(440.0, Waveform::Saw, rate), 0.0);
            assert_eq!(osc.phase, phase);
        }
    }

    #[test]
    fn sine_amplitude() {
        let buf = render(Waveform::Sine);
        // Sine peak should be about 1.0 (allow 1% tolerance).
        let peak: f64 = buf.iter().cloned().map(f64::abs).fold(0.0, f64::max);
        assert!((peak - 1.0).abs() < 0.02, "sine peak = {peak}");
    }

    #[test]
    fn sine_frequency_matches_request() {
        let buf = render(Waveform::Sine);
        let crossings: Vec<usize> = buf
            .windows(2)
            .enumerate()
            .filter_map(|(i, pair)| (pair[0] <= 0.0 && pair[1] > 0.0).then_some(i + 1))
            .collect();
        let periods = (crossings.len() - 1) as f64;
        let samples = (*crossings.last().unwrap() - *crossings.first().unwrap()) as f64;
        let measured = SR * periods / samples;

        assert!((measured - 440.0).abs() < 0.1, "frequency = {measured}");
    }

    #[test]
    fn no_nan_or_inf() {
        for wf in [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ] {
            let buf = render(wf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "{wf:?} produced NaN or Inf"
            );
        }
    }

    #[test]
    fn near_nyquist_stays_finite() {
        for waveform in [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ] {
            let mut osc = Oscillator::new();
            let mut buf = [0.0; 257];
            osc.process(&mut buf, 257, SR * 0.499, waveform, SR);

            assert!(buf.iter().all(|sample| sample.is_finite()));
            assert!((0.0..1.0).contains(&osc.phase));
        }
    }

    #[test]
    fn extreme_pitch_is_muted() {
        let freq = apply_pitch_offset(midi_to_hz(127), 2, 12, 100.0);
        assert!(freq > SR * 0.5);

        let mut osc = Oscillator::new();
        let mut buf = [0.0; 257];
        osc.process(&mut buf, 257, freq, Waveform::Saw, SR);

        assert!(buf.iter().all(|sample| *sample == 0.0));
        assert!((0.0..1.0).contains(&osc.phase));
    }

    #[test]
    fn midi_to_hz_a4() {
        let hz = midi_to_hz(69);
        assert!((hz - 440.0).abs() < 0.001);
    }
}
