use crate::parameter::Control;

pub const SUSTAIN_CC: u8 = 64;
pub const ALL_SOUND_OFF_CC: u8 = 120;
pub const RESET_CONTROLLERS_CC: u8 = 121;
pub const ALL_NOTES_OFF_CC: u8 = 123;

/// Map assigned CC numbers to patch controls.
pub fn cc_target(cc: u8) -> Option<Control> {
    Some(match cc {
        5 => Control::GlideTime,
        74 => Control::FilterCutoff,
        71 => Control::FilterResonance,
        86 => Control::FilterEnvAmount,
        85 => Control::FilterKeyTrack,
        73 => Control::AmpAttack,
        75 => Control::AmpDecay,
        79 => Control::AmpSustain,
        72 => Control::AmpRelease,
        102 => Control::FilterAttack,
        103 => Control::FilterDecay,
        104 => Control::FilterSustain,
        105 => Control::FilterRelease,
        76 => Control::LfoRate,
        77 => Control::LfoDepth,
        106 => Control::Osc1Level,
        107 => Control::Osc2Level,
        108 => Control::Osc3Level,
        109 => Control::NoiseLevel,
        _ => return None,
    })
}

/// Scale normalized CC values; runtime controls already arrive decoded.
pub fn cc_value(control: Control, v: f64) -> Option<f64> {
    Some(match control {
        Control::FilterCutoff => 20.0 * 1000.0_f64.powf(v),
        Control::FilterResonance => v.clamp(0.0, 0.99),
        Control::AmpAttack
        | Control::AmpDecay
        | Control::AmpRelease
        | Control::FilterAttack
        | Control::FilterDecay
        | Control::FilterRelease => 0.001 * 10_000.0_f64.powf(v),
        Control::LfoRate => 0.01 * 3000.0_f64.powf(v),
        Control::GlideTime => 4.0 * v,
        Control::PitchBend | Control::ChannelPressure => return None,
        _ => v,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_assigned_controllers() {
        let mappings = [
            (5, Control::GlideTime),
            (71, Control::FilterResonance),
            (72, Control::AmpRelease),
            (73, Control::AmpAttack),
            (74, Control::FilterCutoff),
            (75, Control::AmpDecay),
            (76, Control::LfoRate),
            (77, Control::LfoDepth),
            (79, Control::AmpSustain),
            (85, Control::FilterKeyTrack),
            (86, Control::FilterEnvAmount),
            (102, Control::FilterAttack),
            (103, Control::FilterDecay),
            (104, Control::FilterSustain),
            (105, Control::FilterRelease),
            (106, Control::Osc1Level),
            (107, Control::Osc2Level),
            (108, Control::Osc3Level),
            (109, Control::NoiseLevel),
        ];

        for (cc, target) in mappings {
            assert_eq!(cc_target(cc), Some(target), "CC {cc}");
        }
        assert_eq!(cc_target(0), None);
        assert_eq!(cc_target(127), None);
        assert_eq!(cc_value(Control::PitchBend, 1.0), None);
        assert_eq!(cc_value(Control::ChannelPressure, 1.0), None);
    }
}
