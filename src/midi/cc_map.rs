#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcTarget {
    FilterCutoff,
    FilterResonance,
    FilterEnvAmount,
    FilterKeyTrack,

    AmpAttack,
    AmpDecay,
    AmpSustain,
    AmpRelease,

    FilterAttack,
    FilterDecay,
    FilterSustain,
    FilterRelease,

    LfoRate,
    LfoDepth,

    Osc1Level,
    Osc2Level,
    Osc3Level,
    NoiseLevel,

    GlideTime,

    Unassigned,
}

/// Map a raw CC number to a `CcTarget`.
///
/// CC assignments (no two arms may share the same integer):
///
/// | CC | Parameter         |
/// |----|-------------------|
/// |  5 | Glide time        |
/// | 71 | Filter resonance  |
/// | 72 | Amp release       |
/// | 73 | Amp attack        |
/// | 74 | Filter cutoff     |
/// | 75 | Amp decay         |
/// | 76 | LFO rate          |
/// | 77 | LFO depth         |
/// | 79 | Amp sustain       |
/// | 85 | Filter key-track  |
/// | 86 | Filter env amount |
/// |102 | Filter attack     |
/// |103 | Filter decay      |
/// |104 | Filter sustain    |
/// |105 | Filter release    |
/// |106 | Osc 1 level       |
/// |107 | Osc 2 level       |
/// |108 | Osc 3 level       |
/// |109 | Noise level       |
pub fn cc_target(cc: u8) -> CcTarget {
    match cc {
        5 => CcTarget::GlideTime,

        // Filter shape
        74 => CcTarget::FilterCutoff,
        71 => CcTarget::FilterResonance,
        86 => CcTarget::FilterEnvAmount,
        85 => CcTarget::FilterKeyTrack,

        // Amp ADSR (GM-standard CC numbers)
        73 => CcTarget::AmpAttack,
        75 => CcTarget::AmpDecay,
        79 => CcTarget::AmpSustain,
        72 => CcTarget::AmpRelease,

        // Filter ADSR — use "undefined" range so they don't collide with amp
        102 => CcTarget::FilterAttack,
        103 => CcTarget::FilterDecay,
        104 => CcTarget::FilterSustain,
        105 => CcTarget::FilterRelease,

        // LFO
        76 => CcTarget::LfoRate,
        77 => CcTarget::LfoDepth,

        // Mixer levels
        106 => CcTarget::Osc1Level,
        107 => CcTarget::Osc2Level,
        108 => CcTarget::Osc3Level,
        109 => CcTarget::NoiseLevel,

        _ => CcTarget::Unassigned,
    }
}
