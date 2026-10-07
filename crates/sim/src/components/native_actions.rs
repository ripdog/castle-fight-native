use super::*;

/// Native bounce effects retain their own target history, independently of weapon/projectile state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HealingWaveProfile {
    pub ability: AbilityId,
    pub healing: i32,
    /// Native trigger Heal still resolves alongside its scripted proxy; not part of bounce falloff.
    pub trigger_healing: i32,
    pub maximum_targets: u8,
    pub jump_radius: i32,
    pub retention_per_10k: u16,
    pub recovery_ticks: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HealingWaveState {
    pub source: SimId,
    pub team: Team,
    pub profile: HealingWaveProfile,
    pub started_tick: u64,
    pub jump_index: u8,
    pub current_target: SimId,
    pub last_position: SimPoint,
    pub next_healing: i32,
    pub hit_targets: [SimId; MAX_BOUNCE_HITS],
    pub hit_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeBoltProfile {
    pub ability: AbilityId,
    pub damage: i32,
    pub stun_ticks: u16,
    pub hero_stun_ticks: u16,
    pub damage_per_second: i32,
    pub duration_ticks: u16,
    pub speed_per_tick: i32,
    pub cleanse: bool,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NativeBoltState {
    pub source: SimId,
    pub team: Team,
    pub target: SimId,
    pub profile: NativeBoltProfile,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub position: SimPoint,
    pub position_tick: u64,
    pub impact_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HailstoneState {
    pub source: SimId,
    pub team: Team,
    pub target: SimId,
    pub profile: crate::building_mechanics::HailstoneProfile,
    pub origin: SimPoint,
    pub destination: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NativeAction {
    HealingWave(HealingWaveState),
    Bolt(NativeBoltState),
    Hailstone(HailstoneState),
}
