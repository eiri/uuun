use std::{env, thread, time::Duration};

use uuun::{
    dsp::patch::Patch,
    engine::audio::Engine,
    midi::{MidiManager, router::MidiRouter},
};

fn main() {
    let patch = Patch::startup();

    let engine = Engine::start(patch).unwrap_or_else(|e| {
        eprintln!("Failed to start audio engine: {e}");
        std::process::exit(1);
    });

    let port_index: Option<usize> = env::args().nth(1).and_then(|s| s.parse().ok());

    let router = MidiRouter::new(engine.sender(), engine.controls());
    let midi_manager = MidiManager::start(router, port_index).unwrap_or_else(|e| {
        eprintln!("MIDI initialisation failed: {e}");
        eprintln!("No MIDI input will be available.");
        std::process::exit(1);
    });

    let diagnostics = midi_manager.diagnostics();
    thread::spawn(move || {
        let mut reported = 0;

        loop {
            thread::sleep(Duration::from_secs(1));
            let dropped = diagnostics.dropped_note_ons();
            if dropped != reported {
                eprintln!("MIDI note-ons dropped: {dropped}");
                reported = dropped;
            }
        }
    });

    println!("synthesizer running. Press Ctrl-C to quit.");
    thread::park();
}
