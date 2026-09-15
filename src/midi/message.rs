/// Raw MIDI event received from the hardware port.
///
/// This is the MIDI domain type. The router maps it to engine events or
/// coalesced controls.
#[derive(Debug, Clone)]
pub enum MidiEvent {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    ControlChange { channel: u8, cc: u8, value: u8 },
    PitchBend { channel: u8, value: f64 },
    ChannelPressure { channel: u8, value: f64 },
}
