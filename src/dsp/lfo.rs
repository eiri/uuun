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
    fn sine_lfo_follows_one_cycle() {
        let mut lfo = Lfo::new();
        let sr = 48_000.0;
        let mut values = Vec::with_capacity(sr as usize);
        for _ in 0..sr as usize {
            values.push(lfo.tick(1.0, sr, LfoWaveform::Sine));
        }

        for (index, expected) in [(0, 0.0), (12_000, 1.0), (24_000, 0.0), (36_000, -1.0)] {
            assert!((values[index] - expected).abs() < 1e-9);
        }
        assert!(lfo.tick(1.0, sr, LfoWaveform::Sine).abs() < 1e-9);
    }
}
