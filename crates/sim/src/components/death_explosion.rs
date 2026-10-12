use super::*;

/// Native Amnx/Adda death AOE. Duration is the post-death delay, not a buff lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathExplosionProfile {
    pub ability: AbilityId,
    pub full_radius: i32,
    pub full_damage: i32,
    pub partial_radius: i32,
    pub partial_damage: i32,
    pub delay_ticks: u16,
    pub targets: AttackTargetMask,
}
impl DeathExplosionProfile {
    pub(crate) fn canonical_words(self) -> [u64; 7] {
        [
            u64::from(self.ability.0),
            self.full_radius as u64,
            self.full_damage as u64,
            self.partial_radius as u64,
            self.partial_damage as u64,
            u64::from(self.delay_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeathExplosionState {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub profile: DeathExplosionProfile,
    pub position: SimPoint,
    pub due_tick: u64,
}
