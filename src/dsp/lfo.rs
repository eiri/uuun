use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoWaveform {
    Sine,
    Square,
    Saw,
    Triangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoDest {
    Filter,
    Pitch,
    Amp,
}

pub struct Lfo {
    phase: f64,
}

impl Lfo {
    pub fn new() -> Self {
        Self { phase: 0.0 }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, rate_hz: f64, sample_rate: f64, waveform: LfoWaveform) -> f64 {
        use std::f64::consts::TAU;

        let value = match waveform {
            LfoWaveform::Sine => (self.phase * TAU).sin(),
            LfoWaveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWaveform::Saw => 2.0 * self.phase - 1.0,
            LfoWaveform::Triangle => {
                if self.phase < 0.5 {
                    4.0 * self.phase - 1.0
                } else {
                    3.0 - 4.0 * self.phase
                }
            }
        };

        self.phase += rate_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        value
    }

    /// Fill `out[..n]` with raw LFO values ([-1, 1]).
    pub fn process(
        &mut self,
        out: &mut [f64],
        n: usize,
        rate_hz: f64,
        sample_rate: f64,
        waveform: LfoWaveform,
    ) {
        for s in out[..n].iter_mut() {
            *s = self.tick(rate_hz, sample_rate, waveform);
        }
    }
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_bounded() {
        let mut lfo = Lfo::new();
        for wf in [
            LfoWaveform::Sine,
            LfoWaveform::Square,
            LfoWaveform::Saw,
            LfoWaveform::Triangle,
        ] {
            for _ in 0..4800 {
                let v = lfo.tick(2.0, 48_000.0, wf);
                assert!(
                    (-1.0 - 1e-9..=1.0 + 1e-9).contains(&v),
                    "{wf:?} out of range: {v}"
                );
            }
        }
    }

    #[test]
    fn sine_lfo_one_cycle() {
        let mut lfo = Lfo::new();
        let sr = 48_000.0;
        let rate = 1.0; // 1 Hz
        let total = sr as usize;
        let mut values = Vec::with_capacity(total);
        for _ in 0..total {
            values.push(lfo.tick(rate, sr, LfoWaveform::Sine));
        }
        // After exactly one cycle, phase should be near 0 again.
        let peak: f64 = values.iter().cloned().map(f64::abs).fold(0.0, f64::max);
        assert!(peak > 0.99, "sine peak = {peak}");
    }
}
