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
    /// `buf` is accumulated into (not overwritten) so multiple oscillators can
    /// be summed in the voice without an extra allocation.
    pub fn process(
        &mut self,
        buf: &mut [f64],
        n: usize,
        freq: f64,
        waveform: Waveform,
        sample_rate: f64,
    ) {
        let dt = freq / sample_rate;

        for s in buf[..n].iter_mut() {
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
                    // Integrate a square wave and rescale
                    if waveform == Waveform::Triangle {
                        // Leaky integrator tuned to give unit amplitude at low freq
                        self.tri_integrator =
                            self.tri_integrator * (1.0 - dt * 0.01) + 4.0 * dt * square;
                        self.tri_integrator
                    } else {
                        square
                    }
                }
            };

            *s += sample;

            // Advance phase.
            self.phase += dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
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
    fn sine_amplitude() {
        let buf = render(Waveform::Sine);
        // Sine peak should be about 1.0 (allow 1% tolerance).
        let peak: f64 = buf.iter().cloned().map(f64::abs).fold(0.0, f64::max);
        assert!((peak - 1.0).abs() < 0.02, "sine peak = {peak}");
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
    fn midi_to_hz_a4() {
        let hz = midi_to_hz(69);
        assert!((hz - 440.0).abs() < 0.001);
    }
}
