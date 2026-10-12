use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CurseProfile {
    pub ability: AbilityId,
    pub buff: NativeBuffIdentity,
    pub chance_to_miss_per_10k: u16,
    pub targets: AttackTargetMask,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
}

impl CurseProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            self.ability.0.into(),
            self.buff.rawcode.into(),
            u64::from(self.buff.positive),
            u64::from(self.buff.stealable),
            u64::from(self.buff.organic_only),
            self.chance_to_miss_per_10k.into(),
            self.targets.bits().into(),
            self.duration_ticks.into(),
            self.hero_duration_ticks.into(),
        ]
    }
}
