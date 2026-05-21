use crate::dsp::patch::Patch;

#[derive(Debug, Clone)]
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
    SetPatch {
        channel: usize,
        patch: Box<Patch>,
    },
    AllNotesOff,
}
