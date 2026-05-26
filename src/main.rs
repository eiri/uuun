mod dsp;
mod engine;
mod midi;

use std::{env, sync::mpsc, thread};

use dsp::{
    envelope::AdsrParams,
    lfo::{LfoDest, LfoWaveform},
    oscillator::Waveform,
    patch::Patch,
};
use engine::audio::Engine;
use midi::{MidiEvent, MidiManager, router::MidiRouter};

fn main() {
    let patch = Patch {
        osc1_waveform: Waveform::Saw,
        osc1_octave: 0,
        osc1_semitone: 0,
        osc1_detune_ct: 0.0,
        osc1_level: 1.0,

        osc2_waveform: Waveform::Saw,
        osc2_octave: 0,
        osc2_semitone: 0,
        osc2_detune_ct: 7.0,
        osc2_level: 0.7,

        osc3_waveform: Waveform::Square,
        osc3_octave: -1,
        osc3_semitone: 0,
        osc3_detune_ct: 0.0,
        osc3_level: 0.5,

        noise_level: 0.0,

        filter_cutoff_hz: 3_500.0,
        filter_resonance: 0.30,
        filter_env_amount: 0.6,
        filter_key_track: 0.0,

        filter_env: AdsrParams {
            attack: 0.02,
            decay: 0.4,
            sustain: 0.5,
            release: 0.5,
        },
        amp_env: AdsrParams {
            attack: 0.01,
            decay: 0.15,
            sustain: 0.80,
            release: 0.35,
        },

        lfo_waveform: LfoWaveform::Sine,
        lfo_rate_hz: 0.5,
        lfo_depth: 0.2,
        lfo_destination: LfoDest::Filter,

        glide_time: 0.0,
    };

    let engine = Engine::start(patch).unwrap_or_else(|e| {
        eprintln!("Failed to start audio engine: {e}");
        std::process::exit(1);
    });

    let port_index: Option<usize> = env::args().nth(1).and_then(|s| s.parse().ok());

    let (midi_tx, midi_rx) = mpsc::sync_channel::<MidiEvent>(1024);

    let _midi_manager = MidiManager::start(midi_tx, port_index).unwrap_or_else(|e| {
        eprintln!("MIDI initialisation failed: {e}");
        eprintln!("No MIDI input will be available.");
        std::process::exit(1);
    });

    let engine_tx = engine.sender();
    let mut router = MidiRouter::new(patch, engine_tx);

    thread::spawn(move || {
        loop {
            match midi_rx.recv() {
                Ok(ev) => router.route(ev),
                Err(_) => break,
            }
        }
    });

    println!("synthesizer running. Press Ctrl-C to quit.");
    thread::park();
}
