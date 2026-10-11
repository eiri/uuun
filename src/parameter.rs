/// Patch and runtime controls, independent of their transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Control {
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
    PitchBend,
    ChannelPressure,
}

impl Control {
    /// Mailbox order must match enum indices.
    pub const ALL: [Self; 21] = [
        Self::FilterCutoff,
        Self::FilterResonance,
        Self::FilterEnvAmount,
        Self::FilterKeyTrack,
        Self::AmpAttack,
        Self::AmpDecay,
        Self::AmpSustain,
        Self::AmpRelease,
        Self::FilterAttack,
        Self::FilterDecay,
        Self::FilterSustain,
        Self::FilterRelease,
        Self::LfoRate,
        Self::LfoDepth,
        Self::Osc1Level,
        Self::Osc2Level,
        Self::Osc3Level,
        Self::NoiseLevel,
        Self::GlideTime,
        Self::PitchBend,
        Self::ChannelPressure,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_indices_are_ordered() {
        assert!(Control::ALL.len() <= u32::BITS as usize);
        for (index, control) in Control::ALL.into_iter().enumerate() {
            assert_eq!(control as usize, index);
        }
    }
}
