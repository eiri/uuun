mod dsp;

use dsp::{envelope::AdsrParams, lfo::LfoWaveform, patch::Patch, voice::Voice};

fn main() {
    // a test patch - trigger C2, render for 1 sec and report peak
    let patch = Patch {
        osc1_waveform: dsp::oscillator::Waveform::Saw,
        osc1_octave: 0,
        osc1_semitone: 0,
        osc1_detune_ct: 0.0,
        osc1_level: 1.0,

        osc2_waveform: dsp::oscillator::Waveform::Saw,
        osc2_octave: 0,
        osc2_semitone: 0,
        osc2_detune_ct: 7.0,
        osc2_level: 0.7,

        osc3_waveform: dsp::oscillator::Waveform::Square,
        osc3_octave: -1,
        osc3_semitone: 0,
        osc3_detune_ct: 0.0,
        osc3_level: 0.5,

        noise_level: 0.0,

        filter_cutoff_hz: 800.0,
        filter_resonance: 0.3,
        filter_env_amount: 0.6,
        filter_key_track: 0.5,

        filter_env: AdsrParams {
            attack: 0.01,
            decay: 0.25,
            sustain: 0.4,
            release: 0.4,
        },

        amp_env: AdsrParams {
            attack: 0.005,
            decay: 0.1,
            sustain: 0.8,
            release: 0.3,
        },

        lfo_waveform: LfoWaveform::Sine,
        lfo_rate_hz: 0.5,
        lfo_depth: 0.1,
        lfo_destination: dsp::lfo::LfoDest::Filter,

        glide_time: 0.0,
    };

    const SAMPLE_RATE: f64 = 48_000.0;
    const BLOCK_SIZE: usize = 128;

    let mut voice = Voice::new(&patch, SAMPLE_RATE);
    let midi_note: u8 = 36; // C2
    voice.note_on(midi_note, 100, &patch, SAMPLE_RATE);

    let total_samples = SAMPLE_RATE as usize;
    let num_blocks = total_samples / BLOCK_SIZE;
    let mut peak: f64 = 0.0;

    for block in 0..num_blocks {
        // Release at 0.7 s to get the release part
        if block == (0.7 * SAMPLE_RATE / BLOCK_SIZE as f64) as usize {
            voice.note_off(&patch);
        }

        let mut buf = vec![0.0_f64; BLOCK_SIZE];
        voice.process(&mut buf, BLOCK_SIZE);

        for &s in &buf {
            if s.abs() > peak {
                peak = s.abs();
            }
        }
    }

    println!("sample rate : {SAMPLE_RATE} Hz");
    println!("block size  : {BLOCK_SIZE} frames");
    println!(
        "rendered    : {} blocks ({total_samples} samples)",
        num_blocks
    );
    println!("peak level  : {peak:.6}");

    if peak > 1e-6 {
        println!("ok, got audio");
    } else {
        println!("hm, peak amost zero, something went wrong");
        std::process::exit(1);
    }
}
