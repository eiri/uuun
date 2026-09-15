use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlideType {
    #[default]
    Lcr,
    Lct,
    Exp,
}

/// Tracks pitch in octaves so linear ramps sound uniform across the keyboard.
pub struct Glide {
    current_log2: f64,
    target_log2: f64,
    step: f64,
    coeff: f64,
    kind: GlideType,
    initialized: bool,
}

impl Glide {
    pub fn new() -> Self {
        Self {
            current_log2: 0.0,
            target_log2: 0.0,
            step: 0.0,
            coeff: 0.0,
            kind: GlideType::Lcr,
            initialized: false,
        }
    }

    pub fn set_target(
        &mut self,
        target_hz: f64,
        glide_time: f64,
        sample_rate: f64,
        kind: GlideType,
    ) {
        self.target_log2 = target_hz.log2();
        self.kind = kind;
        self.step = 0.0;
        self.coeff = 0.0;

        // A voice's first note has no previous pitch to glide from.
        if !self.initialized || glide_time <= 1e-4 {
            self.current_log2 = self.target_log2;
            self.initialized = true;
            return;
        }

        let delta = self.target_log2 - self.current_log2;
        if delta.abs() < f64::EPSILON {
            return;
        }

        match kind {
            // Constant rate: glide_time is the duration per octave.
            GlideType::Lcr => {
                self.step = delta.signum() / (glide_time * sample_rate);
            }
            // Constant time: every interval reaches its target in glide_time.
            GlideType::Lct => {
                self.step = delta / (glide_time * sample_rate);
            }
            // Exponential: glide_time is one time constant.
            GlideType::Exp => {
                self.coeff = (-1.0 / (glide_time * sample_rate)).exp();
            }
        }
    }

    #[inline]
    pub fn tick(&mut self) -> f64 {
        if self.step != 0.0 {
            let remaining = self.target_log2 - self.current_log2;
            if remaining.abs() <= self.step.abs() {
                self.current_log2 = self.target_log2;
                self.step = 0.0;
            } else {
                self.current_log2 += self.step;
            }
        } else if self.coeff > 0.0 {
            self.current_log2 =
                self.coeff * self.current_log2 + (1.0 - self.coeff) * self.target_log2;

            if (self.target_log2 - self.current_log2).abs() < 1e-9 {
                self.current_log2 = self.target_log2;
                self.coeff = 0.0;
            }
        }

        2.0_f64.powf(self.current_log2)
    }

    pub fn process(&mut self, freq_buf: &mut [f64], n: usize) {
        for sample in freq_buf[..n].iter_mut() {
            *sample = self.tick();
        }
    }
}

impl Default for Glide {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 1_000.0;

    #[test]
    fn first_note_is_instant() {
        for kind in [GlideType::Lcr, GlideType::Lct, GlideType::Exp] {
            let mut glide = Glide::new();
            glide.set_target(440.0, 1.0, SR, kind);

            assert!((glide.tick() - 440.0).abs() < 0.1);
        }
    }

    #[test]
    fn zero_time_is_instant() {
        let mut glide = Glide::new();
        glide.set_target(220.0, 0.0, SR, GlideType::Lcr);
        glide.set_target(440.0, 0.0, SR, GlideType::Lcr);

        assert!((glide.tick() - 440.0).abs() < 0.1);
    }

    #[test]
    fn lcr_time_depends_on_distance() {
        let mut one_octave = Glide::new();
        one_octave.set_target(220.0, 0.0, SR, GlideType::Lcr);
        one_octave.set_target(440.0, 1.0, SR, GlideType::Lcr);

        let mut two_octaves = Glide::new();
        two_octaves.set_target(220.0, 0.0, SR, GlideType::Lcr);
        two_octaves.set_target(880.0, 1.0, SR, GlideType::Lcr);

        let mut one_hz = 220.0;
        let mut two_hz = 220.0;
        for _ in 0..1_000 {
            one_hz = one_octave.tick();
            two_hz = two_octaves.tick();
        }

        assert!((one_hz - 440.0).abs() < 0.1);
        assert!((two_hz - 440.0).abs() < 0.1);
    }

    #[test]
    fn lct_time_is_constant() {
        for target in [440.0, 880.0] {
            let mut glide = Glide::new();
            glide.set_target(220.0, 0.0, SR, GlideType::Lct);
            glide.set_target(target, 1.0, SR, GlideType::Lct);

            for _ in 0..1_000 {
                glide.tick();
            }

            assert!((glide.tick() - target).abs() < 0.1);
        }
    }

    #[test]
    fn exponential_glide_converges() {
        let mut glide = Glide::new();
        glide.set_target(220.0, 0.0, 48_000.0, GlideType::Exp);
        glide.set_target(440.0, 0.1, 48_000.0, GlideType::Exp);

        let mut hz = 220.0;
        for _ in 0..48_000 {
            hz = glide.tick();
        }

        assert!((hz - 440.0).abs() < 1.0, "glide not converged: {hz}");
    }
}
