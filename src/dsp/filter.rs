// A 4-pole (24 dB/oct) lowpass with resonance feedback and tanh saturation inside each stage
pub struct MoogFilter {
    // Stage outputs (4 poles)
    y: [f64; 4],
    // Previous stage outputs (for the thermal noise model / double-sample).
    yp: [f64; 4],
    // Filter state for the 2* oversampled pass.
    last_in: f64,
    sample_rate: f64,
}

impl MoogFilter {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            y: [0.0; 4],
            yp: [0.0; 4],
            last_in: 0.0,
            sample_rate,
        }
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.y = [0.0; 4];
        self.yp = [0.0; 4];
        self.last_in = 0.0;
    }

    // redo of https://github.com/eiri/gregory filter
    #[inline]
    pub fn tick(&mut self, input: f64, cutoff_hz: f64, resonance: f64) -> f64 {
        let cutoff = cutoff_hz.clamp(20.0, self.sample_rate * 0.49);
        let res = resonance.clamp(0.0, 1.0);

        // Huovilainen coefficient derivation.
        let sr2 = self.sample_rate * 2.0;
        let f = 2.0 * cutoff / sr2;
        let k = 3.6 * f - 1.6 * f * f - 1.0;
        let p = (k + 1.0) * 0.5;
        let scale = (-1.6 * p - 0.5).exp() + 0.35013 * (k * k).powi(2);
        let r = res * (scale + 0.5) * 0.94 * (2.0 - 4.0 * f) * 0.5;

        // 2* oversampled processing.
        let out = self.pass(self.last_in, input, p, k, r);
        self.last_in = input;
        self.pass(input, input, p, k, r);
        out
    }

    #[inline]
    fn pass(&mut self, x0: f64, x1: f64, p: f64, k: f64, r: f64) -> f64 {
        let input = (x0 + x1) * 0.5;

        let feedback = r * self.y[3];
        let x = tanh_approx(input - feedback);

        // Four cascaded first-order stages with nonlinear tanh.
        let new_y0 = tanh_approx(p * (x + self.yp[0]) - k * self.y[0]);
        let new_y1 = tanh_approx(p * (new_y0 + self.yp[1]) - k * self.y[1]);
        let new_y2 = tanh_approx(p * (new_y1 + self.yp[2]) - k * self.y[2]);
        let new_y3 = tanh_approx(p * (new_y2 + self.yp[3]) - k * self.y[3]);

        self.yp = self.y;
        self.y = [new_y0, new_y1, new_y2, new_y3];

        new_y3
    }

    pub fn process(
        &mut self,
        input_buf: &[f64],
        cutoff_buf: &[f64],
        resonance: f64,
        out_buf: &mut [f64],
        n: usize,
    ) {
        for i in 0..n {
            out_buf[i] += self.tick(input_buf[i], cutoff_buf[i], resonance);
        }
    }
}

/// Fast tanh (hyperbolic tangent) approximation (Pade 3/3 rational)
// lifted from https://github.com/eiri/gregory filter
#[inline(always)]
fn tanh_approx(x: f64) -> f64 {
    // Clamp to avoid overflow in the polynomial at extreme inputs.
    let x = x.clamp(-4.5, 4.5);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_produces_output() {
        let mut f = MoogFilter::new(48_000.0);
        let mut out = 0.0;
        for i in 0..4800 {
            let x = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48_000.0).sin();
            out = f.tick(x, 1_000.0, 0.5);
        }
        assert!(out.is_finite(), "filter output is NaN/Inf");
        assert!(out.abs() > 1e-6, "filter output is zero");
    }

    #[test]
    fn filter_attenuates_above_cutoff() {
        let sr = 48_000.0;
        let mut f_low = MoogFilter::new(sr);
        let mut f_high = MoogFilter::new(sr);

        let n = 48_000_usize;
        let mut rms_low = 0.0_f64;
        let mut rms_high = 0.0_f64;

        for i in 0..n {
            // 8 kHz test tone.
            let x = (2.0 * std::f64::consts::PI * 8_000.0 * i as f64 / sr).sin();
            let o_low = f_low.tick(x, 500.0, 0.0); // cutoff well below 8k
            let o_high = f_high.tick(x, 16_000.0, 0.0); // cutoff well above 8k
            rms_low += o_low * o_low;
            rms_high += o_high * o_high;
        }
        rms_low = (rms_low / n as f64).sqrt();
        rms_high = (rms_high / n as f64).sqrt();

        // Low-cutoff filter should attenuate significantly.
        assert!(
            rms_low < rms_high * 0.1,
            "filter not attenuating: rms_low={rms_low:.4}, rms_high={rms_high:.4}"
        );
    }

    #[test]
    fn test_tanh_approx_accuracy() {
        for i in -30..=30 {
            let x = i as f64 * 0.1;
            let approx = tanh_approx(x);
            let exact = x.tanh();
            assert!(
                (approx - exact).abs() < 0.05, // Pade 3/3 is about 3% max error within +3.0/-3.0
                "tanh_approx({x}) = {approx}, expected ~{exact}"
            );
        }
    }
}
