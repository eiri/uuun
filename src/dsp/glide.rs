#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GlideMode {
    Off,
    Always,
    Legato,
}

// Internally the module tracks the logarithm of frequency so that slides in semitone-space are perceptually linear.
pub struct Glide {
    /// Current log2(freq) used as the glide state variable.
    current_log2: f64,
    /// Target log2(freq).
    target_log2: f64,
    /// Coefficient per sample: 0 = instant, values near 1 = slow glide.
    coeff: f64,
    mode: GlideMode,
}

impl Glide {
    pub fn new() -> Self {
        Self {
            current_log2: 0.0,
            target_log2: 0.0,
            coeff: 0.0,
            mode: GlideMode::Off,
        }
    }

    pub fn set_target(
        &mut self,
        target_hz: f64,
        prev_note_held: bool,
        glide_time: f64,
        sample_rate: f64,
        mode: GlideMode,
    ) {
        self.mode = mode;
        self.target_log2 = target_hz.log2();

        let should_glide = glide_time > 1e-4
            && match mode {
                GlideMode::Off => false,
                GlideMode::Always => true,
                GlideMode::Legato => prev_note_held,
            };

        if should_glide {
            // Time constant: reach ~63 % of target in `glide_time` seconds.
            self.coeff = (-1.0 / (glide_time * sample_rate)).exp();
        } else {
            self.current_log2 = self.target_log2;
            self.coeff = 0.0;
        }
    }

    /// Return the current glided frequency and advance by one sample.
    #[inline]
    pub fn tick(&mut self) -> f64 {
        if self.coeff > 0.0 {
            self.current_log2 =
                self.coeff * self.current_log2 + (1.0 - self.coeff) * self.target_log2;
        }
        2.0_f64.powf(self.current_log2)
    }

    /// Fill `freq_buf[..n]` with per-sample glided frequencies.
    #[allow(dead_code)]
    pub fn process(&mut self, freq_buf: &mut [f64], n: usize) {
        for s in freq_buf[..n].iter_mut() {
            *s = self.tick();
        }
    }

    /// Instantaneously snap to a frequency.
    pub fn reset_to(&mut self, freq_hz: f64) {
        self.current_log2 = freq_hz.log2();
        self.target_log2 = self.current_log2;
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

    #[test]
    fn glide_off_instant() {
        let mut g = Glide::new();
        g.reset_to(220.0);
        g.set_target(440.0, false, 0.0, 48_000.0, GlideMode::Off);
        let hz = g.tick();
        assert!((hz - 440.0).abs() < 0.1, "expected instant jump, got {hz}");
    }

    #[test]
    fn glide_converges() {
        let mut g = Glide::new();
        g.reset_to(220.0);
        g.set_target(440.0, true, 0.1, 48_000.0, GlideMode::Always);

        // Run for 1 second - should be pretty close to target.
        let mut hz = 220.0;
        for _ in 0..48_000 {
            hz = g.tick();
        }
        assert!((hz - 440.0).abs() < 1.0, "glide not converged: {hz}");
    }
}
