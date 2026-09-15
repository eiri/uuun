#[derive(Debug, Clone, Copy)]
pub enum EngineMessage {
    NoteOn {
        channel: usize,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: usize,
        note: u8,
    },
    AllNotesOff {
        channel: usize,
    },
    AllSoundOff {
        channel: usize,
    },
    Sustain {
        channel: usize,
        down: bool,
    },
    ResetControllers {
        channel: usize,
    },
}
