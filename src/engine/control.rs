use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::{engine::audio::NUM_CHANNELS, parameter::Control};

const CONTROL_COUNT: usize = Control::ALL.len();

/// Fixed mailboxes that overwrite stale controls without blocking audio events.
pub struct Controls {
    values: [[AtomicU64; CONTROL_COUNT]; NUM_CHANNELS],
    dirty: [AtomicU32; NUM_CHANNELS],
}

impl Controls {
    pub fn new() -> Self {
        Self {
            values: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
            dirty: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    pub fn set(&self, channel: usize, control: Control, value: f64) {
        let Some(values) = self.values.get(channel) else {
            return;
        };
        let index = control as usize;

        values[index].store(value.to_bits(), Ordering::Relaxed);
        self.dirty[channel].fetch_or(1 << index, Ordering::Release);
    }

    pub fn clear(&self, channel: usize, controls: &[Control]) {
        let Some(dirty) = self.dirty.get(channel) else {
            return;
        };
        let mask = controls
            .iter()
            .fold(0, |mask, control| mask | 1 << (*control as usize));

        dirty.fetch_and(!mask, Ordering::AcqRel);
    }

    pub fn drain(&self, mut apply: impl FnMut(usize, Control, f64)) {
        for channel in 0..NUM_CHANNELS {
            let mut dirty = self.dirty[channel].swap(0, Ordering::Acquire);
            while dirty != 0 {
                let index = dirty.trailing_zeros() as usize;
                let control = Control::ALL[index];
                let value = f64::from_bits(self.values[channel][index].load(Ordering::Relaxed));

                apply(channel, control, value);
                dirty &= !(1 << index);
            }
        }
    }
}

impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_latest_value() {
        let controls = Controls::new();
        controls.set(1, Control::FilterCutoff, 100.0);
        controls.set(1, Control::FilterCutoff, 2_000.0);

        let mut applied = Vec::new();
        controls.drain(|channel, control, value| applied.push((channel, control, value)));

        assert_eq!(applied, [(1, Control::FilterCutoff, 2_000.0)]);
    }
}
