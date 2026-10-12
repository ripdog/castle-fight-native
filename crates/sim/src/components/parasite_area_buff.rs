use super::*;

/// A native targeted Parasite releases an independent caster-centered friendly buff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParasiteAreaBuffProfile {
    pub parent: NativeParasiteProfile,
    pub parent_nonhero_only: bool,
    pub ability: AbilityId,
    pub buff: NativeBuffIdentity,
    pub radius: i32,
    pub targets: AttackTargetMask,
    pub armor_bonus_per_100: i16,
    pub damage_bonus_per_10k: i16,
    pub regeneration_per_second_per_10k: u32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
}

impl ParasiteAreaBuffProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.parent_nonhero_only),
            self.ability.0.into(),
            self.buff.rawcode.into(),
            u64::from(self.buff.positive),
            u64::from(self.buff.stealable),
            u64::from(self.buff.organic_only),
            self.radius as u64,
            self.targets.bits().into(),
            self.armor_bonus_per_100 as u64,
            self.damage_bonus_per_10k as u64,
            self.regeneration_per_second_per_10k.into(),
            self.duration_ticks.into(),
            self.hero_duration_ticks.into(),
        ]
        .into_iter()
        .chain(self.parent.canonical_words())
    }

    pub(crate) fn buff_effect(self) -> AbilityEffect {
        AbilityEffect::StatBuff {
            modifier: ModifierId(self.ability.0),
            buff: self.buff,
            armor_bonus_per_100: self.armor_bonus_per_100,
            damage_bonus_per_10k: self.damage_bonus_per_10k,
            regeneration_per_second_per_10k: self.regeneration_per_second_per_10k,
            duration_ticks: self.duration_ticks,
            hero_duration_ticks: self.hero_duration_ticks,
            autocast_range: 0,
        }
    }
}
