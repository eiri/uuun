use std::sync::mpsc;

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::midi::message::MidiEvent;

/// Owns the MIDI input connection.  Drop to close the port.
pub struct MidiManager {
    /// Keeping the connection alive for the lifetime of this struct.
    _connection: MidiInputConnection<()>,
}

impl MidiManager {
    // Unknown or malformed messages are silently ignored.
    pub fn start(
        tx: mpsc::SyncSender<MidiEvent>,
        port_index: Option<usize>,
    ) -> Result<Self, String> {
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

        let connection = midi_in
            .connect(
                port,
                "uuun-conn",
                move |_timestamp_us, bytes, _| {
                    if let Ok(msg) = wmidi::MidiMessage::try_from(bytes)
                        && let Some(event) = decode(msg)
                    {
                        forward(&tx, event);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;

        Ok(Self {
            _connection: connection,
        })
    }
}

fn forward(tx: &mpsc::SyncSender<MidiEvent>, event: MidiEvent) {
    if must_deliver(&event) {
        // Releases may block briefly, but are never dropped.
        let _ = tx.send(event);
    } else {
        let _ = tx.try_send(event);
    }
}

fn must_deliver(event: &MidiEvent) -> bool {
    matches!(
        event,
        MidiEvent::NoteOff { .. } | MidiEvent::ControlChange { cc: 120 | 123, .. }
    )
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

    #[test]
    fn note_off_survives_full_queue() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(MidiEvent::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        })
        .unwrap();

        let handle = std::thread::spawn(move || {
            forward(
                &tx,
                MidiEvent::NoteOff {
                    channel: 0,
                    note: 60,
                },
            );
        });

        assert!(matches!(rx.recv().unwrap(), MidiEvent::NoteOn { .. }));
        assert!(matches!(
            rx.recv().unwrap(),
            MidiEvent::NoteOff {
                channel: 0,
                note: 60
            }
        ));
        handle.join().unwrap();
    }

    #[test]
    fn releases_and_panics_must_be_delivered() {
        assert!(must_deliver(&MidiEvent::NoteOff {
            channel: 0,
            note: 60,
        }));

        for cc in [120, 123] {
            assert!(must_deliver(&MidiEvent::ControlChange {
                channel: 0,
                cc,
                value: 0,
            }));
        }

        assert!(!must_deliver(&MidiEvent::ControlChange {
            channel: 0,
            cc: 74,
            value: 64,
        }));
    }
}
