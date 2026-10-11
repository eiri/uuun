#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineMessage {
    pub channel: usize,
    pub event: EngineEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEvent {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    AllNotesOff,
    AllSoundOff,
    Sustain { down: bool },
    ResetControllers,
}
