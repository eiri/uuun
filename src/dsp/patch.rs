use serde::{Deserialize, Serialize};

use crate::parameter::Control;

use super::{
    envelope::AdsrParams,
    glide::GlideType,
    lfo::{LfoDest, LfoWaveform},
    oscillator::Waveform,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OscParams {
    pub waveform: Waveform,
    /// Octave offset relative to the played MIDI note (-2 / +2).
    pub octave: i8,
    /// Semitone offset (-12 / +12).
    pub semitone: i8,
    /// Fine detune in cents (-100.0 / +100.0).
    pub detune_ct: f64,
    /// Linear mix level (0.0 / 1.0).
    pub level: f64,
}

impl Default for OscParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            octave: 0,
            semitone: 0,
            detune_ct: 0.0,
            level: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Patch {
    pub oscillators: [OscParams; 3],

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
    #[serde(default)]
    pub glide_type: GlideType,
}

impl Patch {
    /// The startup sound, distinct from the default single-oscillator patch.
    pub fn startup() -> Self {
        let default = Self::default();
        Self {
            oscillators: [
                default.oscillators[0],
                OscParams {
                    detune_ct: 7.0,
                    level: 0.7,
                    ..OscParams::default()
                },
                OscParams {
                    waveform: Waveform::Square,
                    octave: -1,
                    level: 0.5,
                    ..OscParams::default()
                },
            ],
            filter_cutoff_hz: 3_500.0,
            filter_resonance: 0.30,
            filter_env_amount: 0.6,
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
            lfo_rate_hz: 0.5,
            lfo_depth: 0.2,
            ..default
        }
    }

    /// Set a patch control; return false for channel-owned runtime controls.
    pub fn set_control(&mut self, control: Control, value: f64) -> bool {
        match control {
            Control::FilterCutoff => self.filter_cutoff_hz = value,
            Control::FilterResonance => self.filter_resonance = value,
            Control::FilterEnvAmount => self.filter_env_amount = value,
            Control::FilterKeyTrack => self.filter_key_track = value,
            Control::AmpAttack => self.amp_env.attack = value,
            Control::AmpDecay => self.amp_env.decay = value,
            Control::AmpSustain => self.amp_env.sustain = value,
            Control::AmpRelease => self.amp_env.release = value,
            Control::FilterAttack => self.filter_env.attack = value,
            Control::FilterDecay => self.filter_env.decay = value,
            Control::FilterSustain => self.filter_env.sustain = value,
            Control::FilterRelease => self.filter_env.release = value,
            Control::LfoRate => self.lfo_rate_hz = value,
            Control::LfoDepth => self.lfo_depth = value,
            Control::Osc1Level => self.oscillators[0].level = value,
            Control::Osc2Level => self.oscillators[1].level = value,
            Control::Osc3Level => self.oscillators[2].level = value,
            Control::NoiseLevel => self.noise_level = value,
            Control::GlideTime => self.glide_time = value,
            Control::PitchBend | Control::ChannelPressure => return false,
        }
        true
    }

    pub fn validate(&self) -> Result<(), String> {
        // Preserve oscillator error names and floating-point validation order.
        for (index, osc) in self.oscillators.iter().enumerate() {
            check_osc(index, "detune_ct", osc.detune_ct, -100.0, 100.0)?;
            check_osc(index, "level", osc.level, 0.0, 1.0)?;
        }
        for (name, value, min, max) in [
            ("noise_level", self.noise_level, 0.0, 1.0),
            ("filter_cutoff_hz", self.filter_cutoff_hz, 20.0, 20_000.0),
            ("filter_resonance", self.filter_resonance, 0.0, 1.0),
            ("filter_env_amount", self.filter_env_amount, 0.0, 1.0),
            ("filter_key_track", self.filter_key_track, 0.0, 1.0),
            ("filter_env.sustain", self.filter_env.sustain, 0.0, 1.0),
            ("amp_env.sustain", self.amp_env.sustain, 0.0, 1.0),
            ("lfo_rate_hz", self.lfo_rate_hz, 0.01, 30.0),
            ("lfo_depth", self.lfo_depth, 0.0, 1.0),
        ] {
            check_range(name, value, min, max)?;
        }

        for (name, value, max) in [
            ("filter_env.attack", self.filter_env.attack, 10.0),
            ("filter_env.decay", self.filter_env.decay, 10.0),
            ("filter_env.release", self.filter_env.release, 10.0),
            ("amp_env.attack", self.amp_env.attack, 10.0),
            ("amp_env.decay", self.amp_env.decay, 10.0),
            ("amp_env.release", self.amp_env.release, 10.0),
            ("glide_time", self.glide_time, 4.0),
        ] {
            check_range(name, value, 0.0, max)?;
        }

        // All octaves precede semitones in the existing validation contract.
        for (index, osc) in self.oscillators.iter().enumerate() {
            check_osc(index, "octave", f64::from(osc.octave), -2.0, 2.0)?;
        }
        for (index, osc) in self.oscillators.iter().enumerate() {
            check_osc(index, "semitone", f64::from(osc.semitone), -12.0, 12.0)?;
        }

        Ok(())
    }
}

fn check_osc(index: usize, field: &str, value: f64, min: f64, max: f64) -> Result<(), String> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "osc{}_{field} must be within {min}..={max}",
            index + 1
        ))
    }
}

fn check_range(name: &str, value: f64, min: f64, max: f64) -> Result<(), String> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{name} must be within {min}..={max}"))
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            oscillators: [
                OscParams {
                    level: 1.0,
                    ..Default::default()
                },
                OscParams::default(),
                OscParams::default(),
            ],

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
            amp_env: AdsrParams::default(),

            lfo_waveform: LfoWaveform::Sine,
            lfo_rate_hz: 1.0,
            lfo_depth: 0.0,
            lfo_destination: LfoDest::Filter,

            glide_time: 0.0,
            glide_type: GlideType::Lcr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_explicit_values() {
        for (patch, osc_expected, expected) in [
            (
                Patch::default(),
                [
                    (Waveform::Saw, 0, 0, 0.0, 1.0),
                    (Waveform::Saw, 0, 0, 0.0, 0.0),
                    (Waveform::Saw, 0, 0, 0.0, 0.0),
                ],
                [
                    0.0, 2_000.0, 0.2, 0.0, 0.0, 0.01, 0.3, 0.5, 0.3, 0.005, 0.1, 0.8, 0.2, 1.0,
                    0.0, 0.0,
                ],
            ),
            (
                Patch::startup(),
                [
                    (Waveform::Saw, 0, 0, 0.0, 1.0),
                    (Waveform::Saw, 0, 0, 7.0, 0.7),
                    (Waveform::Square, -1, 0, 0.0, 0.5),
                ],
                [
                    0.0, 3_500.0, 0.3, 0.6, 0.0, 0.02, 0.4, 0.5, 0.5, 0.01, 0.15, 0.8, 0.35, 0.5,
                    0.2, 0.0,
                ],
            ),
        ] {
            assert!(patch.validate().is_ok());
            for (osc, expected) in patch.oscillators.into_iter().zip(osc_expected) {
                assert_eq!(
                    (
                        osc.waveform,
                        osc.octave,
                        osc.semitone,
                        osc.detune_ct,
                        osc.level
                    ),
                    expected
                );
            }
            assert_eq!(
                [
                    patch.noise_level,
                    patch.filter_cutoff_hz,
                    patch.filter_resonance,
                    patch.filter_env_amount,
                    patch.filter_key_track,
                    patch.filter_env.attack,
                    patch.filter_env.decay,
                    patch.filter_env.sustain,
                    patch.filter_env.release,
                    patch.amp_env.attack,
                    patch.amp_env.decay,
                    patch.amp_env.sustain,
                    patch.amp_env.release,
                    patch.lfo_rate_hz,
                    patch.lfo_depth,
                    patch.glide_time,
                ],
                expected
            );
            assert_eq!(patch.lfo_waveform, LfoWaveform::Sine);
            assert_eq!(patch.lfo_destination, LfoDest::Filter);
            assert_eq!(patch.glide_type, GlideType::Lcr);
        }
    }

    #[test]
    fn oscillator_bounds_and_order() {
        for index in 0..3 {
            for (field, value, min, max) in [
                ("detune_ct", f64::NAN, -100, 100),
                ("level", 1.1, 0, 1),
                ("octave", 3.0, -2, 2),
                ("semitone", 13.0, -12, 12),
            ] {
                let mut patch = Patch::default();
                let osc = &mut patch.oscillators[index];
                match field {
                    "detune_ct" => osc.detune_ct = value,
                    "level" => osc.level = value,
                    "octave" => osc.octave = value as i8,
                    "semitone" => osc.semitone = value as i8,
                    _ => unreachable!(),
                }
                assert_eq!(
                    patch.validate().unwrap_err(),
                    format!("osc{}_{field} must be within {min}..={max}", index + 1)
                );
            }
        }
        let mut patch = Patch::default();
        patch.oscillators[0].semitone = 13;
        patch.oscillators[2].octave = 3;
        patch.amp_env.release = -1.0;
        patch.oscillators[1].level = 2.0;
        assert_eq!(
            patch.validate().unwrap_err(),
            "osc2_level must be within 0..=1"
        );
        patch.oscillators[1].level = 0.0;
        assert_eq!(
            patch.validate().unwrap_err(),
            "amp_env.release must be within 0..=10"
        );
        patch.amp_env.release = 0.2;
        assert_eq!(
            patch.validate().unwrap_err(),
            "osc3_octave must be within -2..=2"
        );
        patch.oscillators[2].octave = 0;
        assert_eq!(
            patch.validate().unwrap_err(),
            "osc1_semitone must be within -12..=12"
        );
    }

    #[test]
    fn rejects_runtime_controls() {
        let mut patch = Patch::default();
        assert!(!patch.set_control(Control::PitchBend, 1.0));
        assert!(!patch.set_control(Control::ChannelPressure, 1.0));
        assert_eq!(patch.filter_cutoff_hz, 2_000.0);
    }

    #[test]
    fn default_patch_is_valid() {
        assert!(Patch::default().validate().is_ok());
    }

    #[test]
    fn rejects_invalid_values() {
        let invalid_cutoff = Patch {
            filter_cutoff_hz: f64::NAN,
            ..Default::default()
        };
        assert_eq!(
            invalid_cutoff.validate().unwrap_err(),
            "filter_cutoff_hz must be within 20..=20000"
        );

        let invalid_release = Patch {
            amp_env: AdsrParams {
                release: -1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            invalid_release.validate().unwrap_err(),
            "amp_env.release must be within 0..=10"
        );
    }
}
