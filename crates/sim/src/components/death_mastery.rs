use super::*;

/// Native Death and Decay (`AUdd`) released by a script at a damaged target's position: every
/// hostile body inside the area loses a share of its maximum life once per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeathAndDecayProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub life_per_10k: u16,
    pub duration_ticks: u16,
    pub targets: AttackTargetMask,
}
impl DeathAndDecayProfile {
    pub(crate) fn canonical_words(self) -> [u64; 5] {
        [
            u64::from(self.ability.0),
            self.radius as u64,
            u64::from(self.life_per_10k),
            u64::from(self.duration_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}

/// The Lich King's damage listener (`devourAttackProc`): one roll per landed attack either devours
/// an eligible target or leaves a Death and Decay area at its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeathMasteryProfile {
    pub ability: AbilityId,
    /// A roll below this devours an eligible target.
    pub devour_chance_per_10k: u16,
    /// A roll below this (and not devouring) releases Death and Decay.
    pub decay_chance_per_10k: u16,
    pub devour_damage: i32,
    /// Share of the target's current life restored to the bearer.
    pub devour_heal_per_10k: u16,
    /// Bodies carrying this ability can never be devoured.
    pub devour_excluded_ability: AbilityId,
    pub decay: DeathAndDecayProfile,
}
impl DeathMasteryProfile {
    pub(crate) fn canonical_words(self) -> [u64; 11] {
        let decay = self.decay.canonical_words();
        [
            u64::from(self.ability.0),
            u64::from(self.devour_chance_per_10k),
            u64::from(self.decay_chance_per_10k),
            self.devour_damage as u64,
            u64::from(self.devour_heal_per_10k),
            u64::from(self.devour_excluded_ability.0),
            decay[0],
            decay[1],
            decay[2],
            decay[3],
            decay[4],
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeathAndDecayState {
    pub source: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub profile: DeathAndDecayProfile,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
}
