use crate::dsp::{
    patch::Patch,
    voice::{Voice, VoiceStatus},
};

pub const MAX_VOICES: usize = 8;

pub struct VoiceAllocator {
    voices: [Voice; MAX_VOICES],
    /// Allocation timestamp for each voice slot (higher = more recent).
    timestamps: [u64; MAX_VOICES],
    /// Global counter, incremented on every note-on.
    counter: u64,
}

impl VoiceAllocator {
    pub fn new(patch: &Patch, sample_rate: f64) -> Self {
        let voices = std::array::from_fn(|_| Voice::new(patch, sample_rate));
        Self {
            voices,
            timestamps: [0; MAX_VOICES],
            counter: 0,
        }
    }

    // Find or steal a voice for `note` and trigger it.
    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch, sample_rate: f64) {
        // If this note is already playing, retrigger its voice for legato or mono layer unison.
        if let Some(i) = self.find_playing(note) {
            self.counter += 1;
            self.timestamps[i] = self.counter;
            self.voices[i].note_on(note, velocity, patch, sample_rate);
            return;
        }

        let i = self.steal_voice();
        self.counter += 1;
        self.timestamps[i] = self.counter;
        self.voices[i].note_on(note, velocity, patch, sample_rate);
    }

    // Send note-off to whichever voice is playing `note` (if any).
    pub fn note_off(&mut self, note: u8, patch: &Patch) {
        if let Some(i) = self.find_playing(note) {
            self.voices[i].note_off(patch);
        }
    }

    pub fn all_notes_off(&mut self, patch: &Patch) {
        for v in self.voices.iter_mut() {
            if v.status != VoiceStatus::Idle {
                v.note_off(patch);
            }
        }
    }

    pub fn set_patch(&mut self, patch: &Patch) {
        for v in self.voices.iter_mut() {
            v.set_patch(patch);
        }
    }

    pub fn process(&mut self, out_buf: &mut [f64], n: usize) {
        for v in self.voices.iter_mut() {
            if v.status != VoiceStatus::Idle {
                v.process(out_buf, n);
            }
        }
    }

    fn find_playing(&self, note: u8) -> Option<usize> {
        self.voices.iter().enumerate().find_map(|(i, v)| {
            if v.midi_note == note && v.status == VoiceStatus::Active {
                Some(i)
            } else {
                None
            }
        })
    }

    fn steal_voice(&self) -> usize {
        // Do we have idles?
        if let Some(i) = self
            .voices
            .iter()
            .position(|v| v.status == VoiceStatus::Idle)
        {
            return i;
        }
        // Otherwise steal the oldest releasing
        if let Some((i, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.status == VoiceStatus::Releasing)
            .min_by_key(|(i, _)| self.timestamps[*i])
        {
            return i;
        }
        // Otherwise steal the oldest active
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(i, _)| self.timestamps[*i])
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_patch() -> Patch {
        Patch::default()
    }

    #[test]
    fn basic_note_on_off() {
        let p = default_patch();
        let mut alloc = VoiceAllocator::new(&p, 48_000.0);
        alloc.note_on(60, 100, &p, 48_000.0);
        let active = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Active)
            .count();
        assert_eq!(active, 1);
        alloc.note_off(60, &p);
        let releasing = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Releasing)
            .count();
        assert_eq!(releasing, 1);
    }

    #[test]
    fn polyphony_up_to_max() {
        let p = default_patch();
        let mut alloc = VoiceAllocator::new(&p, 48_000.0);
        for note in 60..60 + MAX_VOICES as u8 {
            alloc.note_on(note, 100, &p, 48_000.0);
        }
        let active = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Active)
            .count();
        assert_eq!(active, MAX_VOICES);
    }

    #[test]
    fn stealing_beyond_max_voices() {
        let p = default_patch();
        let mut alloc = VoiceAllocator::new(&p, 48_000.0);
        // Fill all 8 voices.
        for note in 60..60 + MAX_VOICES as u8 {
            alloc.note_on(note, 100, &p, 48_000.0);
        }
        // One more — should steal without panic.
        alloc.note_on(80, 100, &p, 48_000.0);
        let non_idle = alloc
            .voices
            .iter()
            .filter(|v| v.status != VoiceStatus::Idle)
            .count();
        assert_eq!(
            non_idle, MAX_VOICES,
            "voice count should stay at MAX after steal"
        );
    }

    #[test]
    fn all_notes_off_silences_everything() {
        let p = default_patch();
        let mut alloc = VoiceAllocator::new(&p, 48_000.0);
        for note in 60..68_u8 {
            alloc.note_on(note, 100, &p, 48_000.0);
        }
        alloc.all_notes_off(&p);
        let still_active = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Active)
            .count();
        assert_eq!(still_active, 0);
    }

    #[test]
    fn allocator_produces_audio() {
        let p = default_patch();
        let mut alloc = VoiceAllocator::new(&p, 48_000.0);
        alloc.note_on(60, 100, &p, 48_000.0);
        let mut buf = [0.0_f64; 128];
        alloc.process(&mut buf, 128);
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6, "allocator produced silence");
    }
}
