/// Minimal 64-bit xorshift PRNG.
struct Xorshift {
    state: u64,
}

impl Xorshift {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0xDEAD_BEEF_CAFE_1234 // for lulz
            } else {
                seed
            },
        }
    }

    #[inline]
    fn next_f64(&mut self) -> f64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        // Map u64 to (-1, 1).
        (self.state as i64 as f64) / (i64::MAX as f64)
    }
}

pub struct NoiseGen {
    rng: Xorshift,
    // Three IIR coefficients for pink noise (Paul Kellett's method).
    b0: f64,
    b1: f64,
    b2: f64,
}

impl NoiseGen {
    pub fn new() -> Self {
        Self {
            rng: Xorshift::new(0x1234_5678_9ABC_DEF0),
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
        }
    }

    /// One sample of white noise in [-1, 1].
    #[inline]
    pub fn white(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// One sample of pink noise.
    #[inline]
    pub fn pink(&mut self) -> f64 {
        let w = self.rng.next_f64();
        self.b0 = 0.99886 * self.b0 + w * 0.0555179;
        self.b1 = 0.99332 * self.b1 + w * 0.0750759;
        self.b2 = 0.96900 * self.b2 + w * 0.1538520;
        let pink = self.b0 + self.b1 + self.b2 + w * 0.5362;
        pink * 0.11 // approximate to unity RMS
    }

    /// Fill `buf[..n]` by accumulating white noise scaled by `level`.
    #[allow(dead_code)]
    pub fn process_white(&mut self, buf: &mut [f64], n: usize, level: f64) {
        for s in buf[..n].iter_mut() {
            *s += self.white() * level;
        }
    }

    /// Fill `buf[..n]` by accumulating pink noise scaled by `level`.
    #[allow(dead_code)]
    pub fn process_pink(&mut self, buf: &mut [f64], n: usize, level: f64) {
        for s in buf[..n].iter_mut() {
            *s += self.pink() * level;
        }
    }
}

impl Default for NoiseGen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_noise_bounded() {
        let mut ng = NoiseGen::new();
        for _ in 0..100_000 {
            let v = ng.white();
            assert!(v > -1.1 && v < 1.1, "white noise out of range: {v}");
        }
    }

    #[test]
    fn pink_noise_finite() {
        let mut ng = NoiseGen::new();
        for _ in 0..10_000 {
            assert!(ng.pink().is_finite());
        }
    }

    #[test]
    fn white_noise_nonzero_rms() {
        let mut ng = NoiseGen::new();
        let n = 48_000_usize;
        let rms: f64 = (0..n).map(|_| ng.white().powi(2)).sum::<f64>() / n as f64;
        let rms = rms.sqrt();
        assert!(
            rms > 0.5 && rms < 0.7,
            "white RMS = {rms} (expected ≈ 0.577)"
        );
    }
}
