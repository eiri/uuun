use serde::{Deserialize, Serialize};

use super::{
    envelope::AdsrParams,
    lfo::{LfoDest, LfoWaveform},
    oscillator::Waveform,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub osc1_waveform: Waveform,
    /// Octave offset relative to the played MIDI note (-2 / +2).
    pub osc1_octave: i8,
    /// Semitone offset (-12 / +12).
    pub osc1_semitone: i8,
    /// Fine detune in cents (-100.0 / +100.0).
    pub osc1_detune_ct: f64,
    /// Linear mix level (0.0 / 1.0).
    pub osc1_level: f64,

    pub osc2_waveform: Waveform,
    pub osc2_octave: i8,
    pub osc2_semitone: i8,
    pub osc2_detune_ct: f64,
    pub osc2_level: f64,

    pub osc3_waveform: Waveform,
    pub osc3_octave: i8,
    pub osc3_semitone: i8,
    pub osc3_detune_ct: f64,
    pub osc3_level: f64,

    /// White-noise mix level (0.0 / 1.0).
    pub noise_level: f64,

    /// Base cutoff frequency in Hz (20.0 / 20000.0).
    pub filter_cutoff_hz: f64,
    /// Resonance / Q (0.0 / 1.0, where 1.0 is almost self-oscillation).
    pub filter_resonance: f64,
    /// How much the filter envelope modulates cutoff (0.0 / 1.0).
    pub filter_env_amount: f64,
    /// Key-tracking amount: 0 = none, 1 = full 1-oct/oct tracking.
    pub filter_key_track: f64,

    pub filter_env: AdsrParams,
    pub amp_env: AdsrParams,

    pub lfo_waveform: LfoWaveform,
    /// LFO rate in Hz (0.01 / 30.0).
    pub lfo_rate_hz: f64,
    /// Modulation depth (0.0 / 1.0).
    pub lfo_depth: f64,
    pub lfo_destination: LfoDest,

    /// Portamento time in seconds (0.0 = off).
    pub glide_time: f64,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            osc1_waveform: Waveform::Saw,
            osc1_octave: 0,
            osc1_semitone: 0,
            osc1_detune_ct: 0.0,
            osc1_level: 1.0,

            osc2_waveform: Waveform::Saw,
            osc2_octave: 0,
            osc2_semitone: 0,
            osc2_detune_ct: 0.0,
            osc2_level: 0.0,

            osc3_waveform: Waveform::Saw,
            osc3_octave: 0,
            osc3_semitone: 0,
            osc3_detune_ct: 0.0,
            osc3_level: 0.0,

            noise_level: 0.0,

            filter_cutoff_hz: 2_000.0,
            filter_resonance: 0.2,
            filter_env_amount: 0.0,
            filter_key_track: 0.0,

            filter_env: AdsrParams {
                attack: 0.01,
                decay: 0.3,
                sustain: 0.5,
                release: 0.3,
            },

            amp_env: AdsrParams {
                attack: 0.005,
                decay: 0.1,
                sustain: 0.8,
                release: 0.2,
            },

            lfo_waveform: LfoWaveform::Sine,
            lfo_rate_hz: 1.0,
            lfo_depth: 0.0,
            lfo_destination: LfoDest::Filter,

            glide_time: 0.0,
        }
    }
}
