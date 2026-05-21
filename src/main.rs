mod dsp;
mod engine;

use std::{thread, time::Duration};

use dsp::{
    envelope::AdsrParams,
    lfo::{LfoDest, LfoWaveform},
    oscillator::Waveform,
    patch::Patch,
};
use engine::{audio::Engine, message::EngineMessage};

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

    println!("Playing demo sequence. Ctrl-C to quit.\n");

    let play = |msgs: &[(usize, u8, u64, u64)]| {
        let mut events: Vec<(u64, EngineMessage)> = Vec::new();
        for &(ch, note, on_ms, off_ms) in msgs {
            events.push((
                on_ms,
                EngineMessage::NoteOn {
                    channel: ch,
                    note,
                    velocity: 30,
                },
            ));
            events.push((off_ms, EngineMessage::NoteOff { channel: ch, note }));
        }
        events.sort_by_key(|(t, _)| *t);

        let start = std::time::Instant::now();
        let mut idx = 0;
        while idx < events.len() {
            let elapsed = start.elapsed().as_millis() as u64;
            while idx < events.len() && events[idx].0 <= elapsed {
                engine.send(events[idx].1.clone());
                idx += 1;
            }
            thread::sleep(Duration::from_millis(2));
        }
        events.last().map(|(t, _)| *t).unwrap_or(0)
    };

    // C major arpeggio on channel 0
    println!("Part 1: C major arpeggio");
    let step = 350_u64;
    let dur = 700_u64;
    let arp_notes = [48u8, 52, 55, 60, 64, 67, 72]; // C3 E3 G3 C4 E4 G4 C5
    let arp_seq: Vec<(usize, u8, u64, u64)> = arp_notes
        .iter()
        .enumerate()
        .map(|(i, &n)| (0, n, i as u64 * step, i as u64 * step + dur))
        .collect();

    let total_ms = play(&arp_seq);
    thread::sleep(Duration::from_millis(total_ms + 800)); // let release ring

    // Chord voicing, channels 0-2
    println!("Part 2: C major chord spread across 3 channels");
    let chord: &[(usize, u8, u64, u64)] = &[
        (0, 36, 0, 1_800),   // C2  bass
        (1, 52, 80, 1_800),  // E3  mid
        (1, 55, 80, 1_800),  // G3  mid
        (2, 64, 160, 1_800), // E4  high
        (2, 67, 160, 1_800), // G4  high
        (2, 72, 160, 1_800), // C5  high
    ];
    play(chord);
    thread::sleep(Duration::from_millis(2_400));

    // Bright lead on channel 0 for patch swap
    println!("Part 3: patch swap for square-wave lead");
    let lead_patch = Patch {
        osc1_waveform: Waveform::Square,
        osc1_octave: 0,
        osc1_semitone: 0,
        osc1_detune_ct: 0.0,
        osc1_level: 1.0,
        osc2_waveform: Waveform::Square,
        osc2_octave: 0,
        osc2_semitone: 0,
        osc2_detune_ct: -5.0,
        osc2_level: 0.5,
        osc3_waveform: Waveform::Saw,
        osc3_octave: -1,
        osc3_semitone: 0,
        osc3_detune_ct: 0.0,
        osc3_level: 0.3,
        noise_level: 0.0,
        filter_cutoff_hz: 5_000.0,
        filter_resonance: 0.55,
        filter_env_amount: 0.5,
        filter_key_track: 0.0,
        filter_env: AdsrParams {
            attack: 0.005,
            decay: 0.2,
            sustain: 0.6,
            release: 0.3,
        },
        amp_env: AdsrParams {
            attack: 0.004,
            decay: 0.08,
            sustain: 0.9,
            release: 0.2,
        },
        lfo_waveform: LfoWaveform::Triangle,
        lfo_rate_hz: 5.0,
        lfo_depth: 0.05,
        lfo_destination: LfoDest::Amp,
        glide_time: 0.0,
    };

    engine.send(EngineMessage::SetPatch {
        channel: 0,
        patch: Box::new(lead_patch),
    });

    // Descending run: C5 B4 A4 G4 F4 E4 D4 C4
    let lead_notes = [72u8, 71, 69, 67, 65, 64, 62, 60];
    let lead_seq: Vec<(usize, u8, u64, u64)> = lead_notes
        .iter()
        .enumerate()
        .map(|(i, &n)| (0, n, i as u64 * 220, i as u64 * 220 + 400))
        .collect();

    let total_ms = play(&lead_seq);
    thread::sleep(Duration::from_millis(total_ms + 600));

    // all notes off and exit
    engine.send(EngineMessage::AllNotesOff);
    thread::sleep(Duration::from_millis(600));
    println!("\ndemo complete.");
}
