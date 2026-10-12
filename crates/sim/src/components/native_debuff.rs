use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EtherealEffectProfile {
    pub magic_damage_per_10k: u16,
    pub healing_per_10k: u16,
    pub allied_damage_per_10k: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeDebuffProfile {
    pub ability: AbilityId,
    pub buff: u32,
    pub targets: AttackTargetMask,
    pub organic_only: bool,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub damage_bonus_per_10k: i16,
    pub ethereal: Option<EtherealEffectProfile>,
}

impl NativeDebuffProfile {
    pub(crate) fn canonical_words(self) -> [u64; 13] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff),
            u64::from(self.targets.bits()),
            u64::from(self.organic_only),
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            self.movement_percent_delta as u64,
            self.attack_speed_percent_delta as u64,
            self.damage_bonus_per_10k as u64,
            u64::from(self.ethereal.is_some()),
            u64::from(self.ethereal.map_or(0, |e| e.magic_damage_per_10k)),
            u64::from(self.ethereal.map_or(0, |e| e.healing_per_10k)),
            u64::from(self.ethereal.map_or(0, |e| e.allied_damage_per_10k)),
        ]
    }
}
