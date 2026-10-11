//! Deterministic render cases; references captured before refactoring.
use super::{
    glide::GlideType,
    lfo::{LfoDest, LfoWaveform},
    oscillator::Waveform,
    patch::Patch,
    voice::Voice,
};

fn render(case: usize, block_size: usize) -> [f64; 512] {
    let waveforms = [
        Waveform::Saw,
        Waveform::Square,
        Waveform::Triangle,
        Waveform::Sine,
    ];
    let patch = Patch {
        osc1_waveform: waveforms[case],
        osc2_waveform: waveforms[(case + 1) % 4],
        osc2_detune_ct: 7.0,
        osc2_level: 0.7,
        osc3_waveform: waveforms[(case + 2) % 4],
        osc3_octave: -1,
        osc3_level: 0.5,
        noise_level: 0.2,
        filter_env_amount: 0.6,
        filter_key_track: 0.5,
        lfo_waveform: [
            LfoWaveform::Sine,
            LfoWaveform::Square,
            LfoWaveform::Saw,
            LfoWaveform::Triangle,
        ][case],
        lfo_rate_hz: 17.0,
        lfo_depth: 0.4,
        lfo_destination: [
            LfoDest::Filter,
            LfoDest::Pitch,
            LfoDest::Amp,
            LfoDest::Filter,
        ][case],
        glide_time: 0.1,
        glide_type: GlideType::Lct,
        ..Patch::default()
    };
    let mut voice = Voice::new(&patch, 48_000.0);
    // Stop resets envelopes but preserves pitch history; avoid retrigger fixtures.
    voice.note_on(48, 100, &patch);
    voice.stop();
    voice.set_pitch_bend(1.5);
    voice.set_channel_pressure(0.3);
    voice.note_on(67, 110, &patch);
    let mut output = [0.0; 512];
    for block in output.chunks_mut(block_size) {
        let n = block.len();
        voice.process(block, n);
    }
    output
}

pub(crate) fn summary(samples: &[f64]) -> [f64; 6] {
    let peak = samples.iter().copied().map(f64::abs).fold(0.0, f64::max);
    let rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
    [
        samples[0],
        samples[31],
        samples[127],
        samples[samples.len() - 1],
        peak,
        rms,
    ]
}

#[test]
fn signal_references() {
    // Captured with a standalone probe, not calculated by these tests.
    let expected = [
        [
            -1.2808581290810313e-9,
            -0.0060575174372496715,
            0.09567926150200703,
            -0.028149070581164174,
            0.2425283287817796,
            0.14767163501702932,
        ],
        [
            -1.2808581290810313e-9,
            0.028477759147893376,
            0.13423462731819452,
            -0.12239265301909504,
            0.25532347881065953,
            0.1534448771611823,
        ],
        [
            -7.685148774486188e-10,
            -0.00014079991338535424,
            0.07960613526132089,
            0.18413358745432012,
            0.19652195733098993,
            0.1143470541294158,
        ],
        [
            -4.409098834390036e-10,
            0.006026570319837623,
            0.10281732996732706,
            -0.19232375908416344,
            0.19271223749237004,
            0.09001646155043384,
        ],
    ];
    for (case, expected) in expected.into_iter().enumerate() {
        let whole = render(case, 512);
        for (actual, expected) in summary(&whole).into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 1e-10,
                "case {case}: {actual} != {expected}"
            );
        }
        for size in [1, 64, 127, 256] {
            for (actual, expected) in render(case, size).into_iter().zip(whole) {
                assert!(
                    (actual - expected).abs() <= 1e-10,
                    "partition {size}, case {case}"
                );
            }
        }
    }
}
