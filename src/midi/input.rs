use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::midi::{
    message::MidiEvent,
    router::{MidiDiagnostics, MidiRouter},
};

/// Owns the MIDI input connection.  Drop to close the port.
pub struct MidiManager {
    /// Keeping the connection alive for the lifetime of this struct.
    _connection: MidiInputConnection<()>,
    diagnostics: MidiDiagnostics,
}

impl MidiManager {
    pub fn start(router: MidiRouter, port_index: Option<usize>) -> Result<Self, String> {
        let mut midi_in = MidiInput::new("uuun-midi-in").map_err(|e| e.to_string())?;
        // Do not filter SysEx, timing, or active-sensing bytes so we see
        // everything, ignore it at wmidi level.
        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();

        if ports.is_empty() {
            return Err("no MIDI input ports found".to_string());
        }

        println!("Available MIDI input ports:");
        for (i, port) in ports.iter().enumerate() {
            let name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| "<unknown>".to_string());
            println!("  [{i}] {name}");
        }

        let idx = port_index.unwrap_or(0);
        if idx >= ports.len() {
            return Err(format!(
                "port index {idx} out of range (0..{})",
                ports.len()
            ));
        }

        let port = &ports[idx];
        let port_name = midi_in
            .port_name(port)
            .unwrap_or_else(|_| "<unknown>".to_string());

        println!("Connecting to MIDI port [{idx}]: {port_name}");

        let diagnostics = router.diagnostics();
        let connection = midi_in
            .connect(
                port,
                "uuun-conn",
                move |_timestamp_us, bytes, _| {
                    // Ignore unknown or malformed messages.
                    if let Ok(msg) = wmidi::MidiMessage::try_from(bytes)
                        && let Some(event) = decode(msg)
                    {
                        router.route(event);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;

        Ok(Self {
            _connection: connection,
            diagnostics,
        })
    }

    pub fn diagnostics(&self) -> MidiDiagnostics {
        self.diagnostics.clone()
    }
}

fn decode(msg: wmidi::MidiMessage<'_>) -> Option<MidiEvent> {
    use wmidi::MidiMessage as M;

    match msg {
        M::NoteOn(ch, note, vel) => {
            let channel = ch as u8;
            let note = u8::from(note);
            let velocity = u8::from(vel);
            // MIDI spec: NoteOn with velocity 0 is equivalent to NoteOff.
            if velocity == 0 {
                Some(MidiEvent::NoteOff { channel, note })
            } else {
                Some(MidiEvent::NoteOn {
                    channel,
                    note,
                    velocity,
                })
            }
        }
        M::NoteOff(ch, note, _vel) => Some(MidiEvent::NoteOff {
            channel: ch as u8,
            note: u8::from(note),
        }),
        M::ControlChange(ch, cc, val) => {
            // ControlFunction wraps U7 (a newtype over u8).
            // The public API exposes Into<U7> for ControlFunction, and U7: Into<u8>.
            let cc_u7: wmidi::U7 = cc.into();
            Some(MidiEvent::ControlChange {
                channel: ch as u8,
                cc: u8::from(cc_u7),
                value: u8::from(val),
            })
        }
        M::PitchBendChange(ch, bend) => {
            // PitchBend wraps a U14 (14-bit, centre = 8192, range 0..=16383).
            let raw = u16::from(bend);
            let normalised = (raw as f64 - 8192.0) / 8192.0;
            Some(MidiEvent::PitchBend {
                channel: ch as u8,
                value: normalised.clamp(-1.0, 1.0),
            })
        }
        M::ChannelPressure(ch, pressure) => Some(MidiEvent::ChannelPressure {
            channel: ch as u8,
            value: u8::from(pressure) as f64 / 127.0,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Option<MidiEvent> {
        wmidi::MidiMessage::try_from(bytes).ok().and_then(decode)
    }

    #[test]
    fn decodes_notes_and_controls() {
        assert!(matches!(
            parse(&[0x93, 60, 100]),
            Some(MidiEvent::NoteOn {
                channel: 3,
                note: 60,
                velocity: 100
            })
        ));
        for bytes in [[0x83, 60, 64], [0x93, 60, 0]] {
            assert!(matches!(
                parse(&bytes),
                Some(MidiEvent::NoteOff {
                    channel: 3,
                    note: 60
                })
            ));
        }
        assert!(matches!(
            parse(&[0xb2, 74, 127]),
            Some(MidiEvent::ControlChange {
                channel: 2,
                cc: 74,
                value: 127
            })
        ));
    }

    #[test]
    fn decodes_bend_and_pressure() {
        for (bytes, expected) in [
            ([0xe1, 0, 0], -1.0),
            ([0xe1, 0, 64], 0.0),
            ([0xe1, 127, 127], 8191.0 / 8192.0),
        ] {
            let Some(MidiEvent::PitchBend { channel, value }) = parse(&bytes) else {
                panic!("missing bend")
            };
            assert_eq!(channel, 1);
            assert_eq!(value, expected);
        }
        for raw in [0, 64, 127] {
            let Some(MidiEvent::ChannelPressure { channel, value }) = parse(&[0xd2, raw]) else {
                panic!("missing pressure")
            };
            assert_eq!(channel, 2);
            assert_eq!(value, f64::from(raw) / 127.0);
        }
    }

    #[test]
    fn ignores_other_and_malformed_messages() {
        for bytes in [
            &[0xc0, 10][..],
            &[0xa0, 60, 10],
            &[0xf8],
            &[0xfe],
            &[0xf0, 1, 2, 0xf7],
            &[],
            &[0x90],
            &[0x90, 60],
            &[0x90, 128, 10],
            &[60, 100],
        ] {
            assert!(parse(bytes).is_none(), "{bytes:?}");
        }
    }
}
