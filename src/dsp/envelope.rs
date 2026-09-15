use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AdsrParams {
    /// Attack time in seconds (>= 0.0001).
    pub attack: f64,
    /// Decay time in seconds.
    pub decay: f64,
    /// Sustain level (0.0 / 1.0).
    pub sustain: f64,
    /// Release time in seconds.
    pub release: f64,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack: 0.005,
            decay: 0.1,
            sustain: 0.8,
            release: 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Envelope {
    stage: Stage,
    /// Current envelope output value (0.0 / 1.0).
    value: f64,
    /// Value at which the release stage started.
    release_start: f64,
    /// Elapsed samples in the current stage.
    stage_samples: u64,
    sample_rate: f64,
}

impl Envelope {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            stage: Stage::Idle,
            value: 0.0,
            release_start: 0.0,
            stage_samples: 0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.value = 0.0;
        self.release_start = 0.0;
        self.stage_samples = 0;
    }

    /// Trigger a note without discontinuity when already releasing.
    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.stage_samples = 0;
        // Keep current value so we don't click when retriggering mid-release.
    }

    /// Begin the release stage.
    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.release_start = self.value;
            self.stage = Stage::Release;
            self.stage_samples = 0;
        }
    }

    /// Returns `true` when the voice can be stolen.
    pub fn is_finished(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline]
    pub fn tick(&mut self, params: &AdsrParams) -> f64 {
        let sr = self.sample_rate;
        match self.stage {
            Stage::Idle => {
                self.value = 0.0;
            }

            Stage::Attack => {
                self.stage_samples += 1;
                let dur = (params.attack * sr).max(1.0);
                self.value = self.stage_samples as f64 / dur;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                    self.stage_samples = 0;
                }
            }

            Stage::Decay => {
                let dur = (params.decay * sr).max(1.0);
                let t = self.stage_samples as f64 / dur;
                // Exponential decay
                self.value = 1.0 - (1.0 - params.sustain) * t;
                if t >= 1.0 {
                    self.value = params.sustain;
                    self.stage = Stage::Sustain;
                    self.stage_samples = 0;
                } else {
                    self.stage_samples += 1;
                }
            }

            Stage::Sustain => {
                self.value = params.sustain;
            }

            Stage::Release => {
                let dur = (params.release * sr).max(1.0);
                let t = self.stage_samples as f64 / dur;
                self.value = self.release_start * (1.0 - t);
                if t >= 1.0 || self.value < 1e-6 {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                } else {
                    self.stage_samples += 1;
                }
            }
        }

        self.value.clamp(0.0, 1.0)
    }

    /// Fill `buf[..n]` with envelope values and multiply for amplitude shaping.
    pub fn process_mul(&mut self, buf: &mut [f64], n: usize, params: &AdsrParams) {
        for s in buf[..n].iter_mut() {
            *s *= self.tick(params);
        }
    }

    /// Fill `buf[..n]` with envelope values.
    pub fn process_fill(&mut self, buf: &mut [f64], n: usize, params: &AdsrParams) {
        for s in buf[..n].iter_mut() {
            *s = self.tick(params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> AdsrParams {
        AdsrParams {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.2,
        }
    }

    #[test]
    fn envelope_follows_adsr_shape() {
        let sr = 48_000.0;
        let mut env = Envelope::new(sr);
        let p = default_params();
        env.note_on();

        let attack = (p.attack * sr) as usize;
        let decay = (p.decay * sr) as usize;
        let mut values = Vec::with_capacity(attack + decay + 2);
        for _ in 0..attack + decay + 2 {
            values.push(env.tick(&p));
        }

        assert!((values[attack / 2 - 1] - 0.5).abs() < 1e-12);
        assert!((values[attack - 1] - 1.0).abs() < 1e-12);
        assert!((values[attack + decay / 2] - 0.85).abs() < 1e-4);
        assert!((values[attack + decay + 1] - p.sustain).abs() < 1e-12);
    }

    #[test]
    fn envelope_finishes_after_release() {
        let mut env = Envelope::new(48_000.0);
        let p = default_params();
        env.note_on();

        let pre = ((p.attack + p.decay) * 48_000.0) as usize + 200;
        for _ in 0..pre {
            env.tick(&p);
        }

        env.note_off();

        let rel = (p.release * 48_000.0) as usize;
        let mut midpoint = 0.0;
        for _ in 0..=rel / 2 {
            midpoint = env.tick(&p);
        }
        assert!((midpoint - p.sustain * 0.5).abs() < 1e-12);

        for _ in 0..=rel / 2 {
            env.tick(&p);
        }
        assert!(env.is_finished(), "envelope should finish on time");
    }
}
