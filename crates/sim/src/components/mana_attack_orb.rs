use super::*;

/// Default-active native attack orb. Mana and its released payload are authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaAttackOrbProfile {
    pub ability: AbilityId,
    pub mana_cost: i32,
    pub bonus_damage: i32,
    pub speed_per_tick: i32,
    pub primary_targets: AttackTargetMask,
    pub splash: SplashFalloffProfile,
}
impl ManaAttackOrbProfile {
    pub(crate) fn canonical_words(self) -> [u64; 11] {
        [
            u64::from(self.ability.0),
            self.mana_cost as u64,
            self.bonus_damage as u64,
            self.speed_per_tick as u64,
            self.splash.full_radius as u64,
            self.splash.medium_radius as u64,
            self.splash.outer_radius as u64,
            u64::from(self.splash.medium_damage_per_10k),
            u64::from(self.splash.outer_damage_per_10k),
            u64::from(self.splash.targets.bits()),
            u64::from(self.primary_targets.bits()),
        ]
    }
}
