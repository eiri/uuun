use crate::dsp::{
    patch::Patch,
    voice::{RenderParams, Voice, VoiceStatus},
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
    pub fn new(sample_rate: f64) -> Self {
        let voices = std::array::from_fn(|_| Voice::new(sample_rate));
        Self {
            voices,
            timestamps: [0; MAX_VOICES],
            counter: 0,
        }
    }

    // Find or steal a voice for `note` and trigger it.
    pub fn note_on(&mut self, note: u8, velocity: u8, patch: &Patch) {
        // Retrigger an existing voice instead of layering the same note.
        if let Some(i) = self.find_playing(note) {
            self.counter += 1;
            self.timestamps[i] = self.counter;
            self.voices[i].note_on(note, velocity, patch);
            return;
        }

        let i = self.steal_voice();
        self.counter += 1;
        self.timestamps[i] = self.counter;
        self.voices[i].note_on(note, velocity, patch);
    }

    // Send note-off to whichever voice is playing `note` (if any).
    pub fn note_off(&mut self, note: u8) {
        if let Some(i) = self.find_playing(note) {
            self.voices[i].note_off();
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in self.voices.iter_mut() {
            if v.status != VoiceStatus::Idle {
                v.note_off();
            }
        }
    }

    pub fn all_sound_off(&mut self) {
        for voice in self.voices.iter_mut() {
            voice.stop();
        }
    }

    pub fn process(&mut self, out_buf: &mut [f64], params: RenderParams<'_>) {
        for v in self.voices.iter_mut() {
            if v.status != VoiceStatus::Idle {
                v.process(out_buf, params);
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

    #[test]
    fn retrigger_reuses_slot() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        alloc.note_on(60, 100, &p);
        alloc.note_on(61, 100, &p);
        alloc.note_on(60, 80, &p);
        assert_eq!(alloc.find_playing(60), Some(0));
        assert_eq!(alloc.timestamps[0], 3);
        assert_eq!(alloc.voices[2].status, VoiceStatus::Idle);
    }

    #[test]
    fn stealing_priority_is_exact() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        for note in 60..68 {
            alloc.note_on(note, 100, &p);
        }
        alloc.note_on(80, 100, &p);
        assert_eq!(alloc.find_playing(80), Some(0));
        assert_eq!(alloc.find_playing(60), None);

        alloc.note_off(64);
        alloc.note_off(62);
        alloc.note_on(81, 100, &p);
        assert_eq!(alloc.find_playing(81), Some(2));
        assert_eq!(alloc.voices[4].status, VoiceStatus::Releasing);

        alloc.voices[6].stop();
        alloc.note_on(82, 100, &p);
        assert_eq!(alloc.find_playing(82), Some(6));
        alloc.note_on(83, 100, &p);
        assert_eq!(alloc.find_playing(83), Some(4));
        alloc.note_on(84, 100, &p);
        assert_eq!(alloc.find_playing(84), Some(1));
    }

    #[test]
    fn basic_note_on_off() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        alloc.note_on(60, 100, &p);
        let active = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Active)
            .count();
        assert_eq!(active, 1);
        alloc.note_off(60);
        let releasing = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Releasing)
            .count();
        assert_eq!(releasing, 1);
    }

    #[test]
    fn polyphony_up_to_max() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        for note in 60..60 + MAX_VOICES as u8 {
            alloc.note_on(note, 100, &p);
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
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        // Fill all 8 voices.
        for note in 60..60 + MAX_VOICES as u8 {
            alloc.note_on(note, 100, &p);
        }
        // One more — should steal without panic.
        alloc.note_on(80, 100, &p);
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
    fn all_sound_off_stops_immediately() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        alloc.note_on(60, 100, &p);
        alloc.all_sound_off();

        let mut buf = [0.0_f64; 128];
        alloc.process(
            &mut buf,
            RenderParams {
                patch: &p,
                pitch_bend: 0.0,
                pressure: 0.0,
            },
        );

        assert!(buf.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn all_notes_off_silences_everything() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        for note in 60..68_u8 {
            alloc.note_on(note, 100, &p);
        }
        alloc.all_notes_off();
        let still_active = alloc
            .voices
            .iter()
            .filter(|v| v.status == VoiceStatus::Active)
            .count();
        assert_eq!(still_active, 0);
    }

    #[test]
    fn allocator_produces_audio() {
        let p = Patch::default();
        let mut alloc = VoiceAllocator::new(48_000.0);
        alloc.note_on(60, 100, &p);
        let mut buf = [0.0_f64; 128];
        alloc.process(
            &mut buf,
            RenderParams {
                patch: &p,
                pitch_bend: 0.0,
                pressure: 0.0,
            },
        );
        let peak = buf.iter().cloned().map(f64::abs).fold(0.0_f64, f64::max);
        assert!(peak > 1e-6, "allocator produced silence");
    }
}
