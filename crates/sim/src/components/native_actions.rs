use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AcidBombProfile {
    pub ability: AbilityId,
    pub range: i32,
    pub radius: i32,
    pub speed_per_tick: i32,
    pub homing: bool,
    pub armor_penalty_per_100: i16,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub primary_damage: i32,
    pub secondary_damage: i32,
    pub interval_ticks: u16,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub targets: AttackTargetMask,
}

impl AcidBombProfile {
    pub(crate) fn canonical_words(self) -> [u64; 14] {
        [
            u64::from(self.ability.0),
            self.range as u64,
            self.radius as u64,
            self.speed_per_tick as u64,
            u64::from(self.homing),
            self.armor_penalty_per_100 as u64,
            self.movement_percent_delta as u64,
            self.attack_speed_percent_delta as u64,
            self.primary_damage as u64,
            self.secondary_damage as u64,
            u64::from(self.interval_ticks),
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}

/// One native Acid Bomb family: separate custom buffs cannot multiply its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcidBombStatus {
    pub source: Option<SimId>,
    pub profile: AcidBombProfile,
    pub damage_per_pulse: i32,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AcidBombMissileState {
    pub source: SimId,
    pub team: Team,
    pub target: SimId,
    pub profile: AcidBombProfile,
    pub launch_position: SimPoint,
    pub destination: SimPoint,
    pub launch_tick: u64,
    pub position: SimPoint,
    pub position_tick: u64,
    pub impact_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnsnareProfile {
    pub ability: AbilityId,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub range: i32,
    pub speed_per_tick: i32,
    pub lowering_ticks: u16,
    pub air_height: i32,
    pub melee_range: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsnareState {
    pub profile: EnsnareProfile,
    pub applied_tick: u64,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EnsnareMissileState {
    pub source: SimId,
    pub team: Team,
    pub target: SimId,
    pub profile: EnsnareProfile,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub position: SimPoint,
    pub position_tick: u64,
    pub impact_tick: u64,
}

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
    pub damage_source: Option<SimId>,
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
    Weather(crate::building_mechanics::WeatherState),
    MasterProjectiles(MasterProjectileState),
    ConeWave(ConeWaveState),
    SphereFlight(SphereFlightState),
    AuraCarrier(AuraCarrierState),
    StasisTrap(StasisTrapState),
    AcidBomb(AcidBombMissileState),
    Poison(PoisonState),
    Ensnare(EnsnareMissileState),
    HealingWave(HealingWaveState),
    Bolt(NativeBoltState),
    Hailstone(HailstoneState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PoisonState {
    pub source: SimId,
    pub target: SimId,
    pub generation: SimId,
    pub profile: PoisonProfile,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
}

/// The source map hides this native summoned ward; its gameplay clocks outlive the dummy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StasisTrapState {
    pub source: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub profile: crate::building_mechanics::BuildingStasisProfile,
    pub armed_tick: u64,
    pub detonation_tick: Option<u64>,
    pub expires_tick: u64,
}

/// Invulnerable/locust carrier: no combat handle, collision, model, or selectable presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AuraCarrierState {
    pub source: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub profile: crate::building_mechanics::BuildingAuraProfile,
    pub created_tick: u64,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SphereFlightState {
    pub source: SimId,
    pub ability: AbilityId,
    pub source_movement: super::MovementClass,
    pub target_movement: super::MovementClass,
    pub origin: SimPoint,
    pub destination: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConeWaveState {
    pub source: SimId,
    pub team: Team,
    pub profile: ConeWaveProfile,
    pub origin: SimPoint,
    pub destination: SimPoint,
    pub launch_tick: u64,
    pub traveled: i32,
    pub remaining_damage: i32,
    pub hit_targets: Vec<SimId>,
}
