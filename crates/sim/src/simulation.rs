use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use bevy_ecs::{entity::Entity, prelude::World};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use serde::{Deserialize, Serialize};

const RANDOM_PURPOSE_BOUNCE_TARGET: u64 = 0x424f_554e_4345_0001;
const RANDOM_PURPOSE_ABILITY_TARGET: u64 = 0x4142_494c_4954_0001;
const RANDOM_PURPOSE_UPHILL_MISS: u64 = 0x5550_4849_4c4c_0001;
const RANDOM_PURPOSE_ATTACK_PROC: u64 = 0x4154_4b50_524f_4301;
const RANDOM_PURPOSE_DEFEND_DEFLECT: u64 = 0x4445_4645_4e44_0001;
const RANDOM_PURPOSE_HEROIC_SHRINE: u64 = 0x5348_5249_4e45_0001;
const RANDOM_PURPOSE_ARTILLERY_POINT: u64 = 0x4152_5450_4f49_0001;
const RANDOM_PURPOSE_ARTILLERY_DAMAGE: u64 = 0x4152_5444_4d47_0001;
pub const UPHILL_MISS_CHANCE_SCALE: u16 = 10_000;
/// Logical checksum encoding revision. Bump when the canonical projection changes incompatibly.
pub const CANONICAL_CHECKSUM_SCHEMA_VERSION: u32 = 95;

const VISUAL_ID_BIT: u64 = 1 << 63;
const ATTACK_PROC_CHANCE_SCALE: u16 = 10_000;
const DIRECT_RETALIATION_RANGE_MULTIPLIER: i32 = 3;
const AVOIDANCE_CLEAR_TICKS: u8 = 8;

mod abilities;
mod automatic_abilities;
mod builder;
mod building_bolts;
mod death_healing;
mod scripted_roots;
mod support_orders;
pub use death_healing::DeathHealVisualEvent;
mod building_damage_procs;
mod enchantments;
mod impale;
mod naga;
mod native_building_auras;
mod native_immolation;
mod night_elf;
mod parasite_area_buff;
mod permanent_grants;
mod starfall;
mod training_variants;
mod travelling_mana_clap;
mod weather;
mod world_freezer;
use world_freezer::WorldFreezerState;
mod attack_proxy;
mod building_spells;
mod eraser;
mod flame_strike;
mod frost_launcher;
mod magic_ruin;
mod snowveil;
mod speed_auras;
mod spheres;
use snowveil::SnowveilState;
#[cfg(test)]
mod building_spells_source_tests;
#[cfg(test)]
mod building_spells_tests;
mod vision;
use building_spells::{BuildingSpellControl, BuildingSpellTargetState};
pub use building_spells::{BuildingSpellVisualEvent, BuildingSpellVisualKind, HexState};
#[cfg(test)]
mod action_animation_tests;
mod canonical;
mod combat;
mod construction;
#[cfg(test)]
mod content_lifecycle_tests;
mod damage_charge;
mod damage_retaliation;
mod damage_thresholds;
mod death_spawns;
mod economy;
mod forked_lightning;
mod homing_splash;
mod lava_spawns;
mod line_projectiles;
mod mana_improvement;
mod master_projectiles;
mod match_rules;
#[cfg(test)]
mod match_rules_tests;
mod mirror_images;
mod native_summons;
mod source_damage_runtime;
mod spiked_shell;
use match_rules::{PendingRescueStrike, PlayerStrikeState};
pub use match_rules::{PlayerRoundScore, RescueStrikeVisualEvent, RoundScoreboard};
mod movement;
mod native_actions;
pub use native_actions::aura_carrier::AuraCarrierView;
#[cfg(test)]
mod native_actions_tests;
mod native_buff_transfer;
mod native_carriers;
mod native_kill_effects;
#[cfg(test)]
mod native_target_effects;
#[cfg(test)]
mod production_abilities;
mod projectiles;
mod shrine;
mod snapshot;
pub use shrine::ShrineRevivalEvent;
mod status;
mod targeting;

use canonical::{CanonicalMatchState, canonical_checksum, canonical_configuration_identity};
use combat::AttackResolutionContext;
use projectiles::{BallisticImpactContext, ProjectileWorldChanges, TargetProjectileContext};

pub use snapshot::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, SimulationSnapshot, SnapshotRestoreError,
    SnapshotWireError,
};
use status::{
    apply_ability_effect_to_unit, apply_corruption, apply_melee_reactive_armor_effects,
    apply_timed_armor_modifier, apply_timed_attack_speed_modifier, apply_timed_damage_over_time,
    apply_timed_movement_modifier, effective_armor_points_per_100, effective_attack_cooldown_ticks,
    purge_expired_status_modifiers, resolve_periodic_unit_statuses,
};

use crate::components::{DelayedShrineRevival, ShrineRevivalState};
use crate::{
    components::{
        AbilityConfigurationError, AbilityEffect, AbilityId, AbilityTargetPolicy,
        ActionAnimationKind, ActionTimingProfile, AdditionalAutomaticAbilities,
        AdditionalAutomaticAbilityDefinitions, AreaDamageOrigin, AttackCooldown, AttackDelivery,
        AttackProfile, AttackSequence, AttackTargetMask, AutomaticAbilityInstance,
        AutomaticAbilityProfile, AutomaticAbilityState, BallisticProjectile, BounceProjectile,
        BuildTimeTicks, Builder, BuilderBuildOrder, BuilderConfiguration, BuilderLocomotion,
        BuilderOrder, BuilderOrderQueue, BuilderProfile, BuilderSpawn, BuilderState,
        BuildingConstruction, BuildingFootprint, BuildingGameplayProperties, BuildingRuntimeState,
        BuildingSpawn, BuildingUpgradeSource, BurningOilZone, ChainLightningState, CollisionRadius,
        ContentIdentity, Corpse, CorpseDefinitionId, CorpseProducer, CorpseProfile,
        DefendEffectProfile, GameplayBundleIdentity, GuaranteedHitProjectile, Health,
        HealthRegeneration, LineProjectile, MAX_BOUNCE_HITS, MAX_TIMED_ARMOR_MODIFIERS,
        MAX_TIMED_ATTACK_SPEED_MODIFIERS, MAX_TIMED_DAMAGE_OVER_TIME, MAX_TIMED_MOVEMENT_MODIFIERS,
        ManaState, MechanicalUnit, ModifierId, MovementClass, MovementProfile, NavigationGoal,
        NavigationState, Owner, PassiveUnitEffect, PassiveUnitEffects, PendingAttackEffects,
        PendingCastState, PendingCastTarget, PlayerId, Position, ProductionActionTiming,
        ProductionAdditionalAutomaticAbilities, ProductionArmorProfile, ProductionAttackTargets,
        ProductionCollisionRadius, ProductionContentIdentity, ProductionCorpseProfile,
        ProductionDamageType, ProductionHealthRegeneration, ProductionMovementClass,
        ProductionPassiveEffects, ProductionProfile, ProductionSecondaryAttack,
        ProductionSpellcastingProfile, ProductionState, ProductionUnitClassifications,
        ProductionUnitRepairMetadata, ReflectedProjectile, RepairTimeTicks, ResolvedUnitDefinition,
        ResurrectionProfile, RetaliationState, SecondaryAttackProfile, SecondaryResurrectionState,
        SimId, SpawnTick, SpellcastingProfile, StatusState, TargetState, Team, TimedArmorModifier,
        TimedAttackSpeedModifier, TimedDamageOverTime, TriggeredAttackEffect, UnitClassifications,
        UnitGameplayProperties, UnitSpawn,
    },
    content::CASTLE_FIGHT_SIMULATION_HZ,
    damage::{ArmorProfile, ArmorType, DamageRules, DamageType},
    economy::{
        BuildingEconomyProfile, EconomyRules, PlayerEconomyView, PlayerResources,
        RESOURCE_FIXED_SCALE, ResourcePurchaseError, taxed_income_from_fixed,
    },
    math::{SUBUNITS_PER_WORLD_UNIT, SimPoint},
    spatial::{SpatialGrid, SpatialPartition, SpatialReservationGrid},
    terrain::TerrainElevationMap,
    topology::{NavCell, PursuitStep, TopologyGrid},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetlessLane {
    /// Inclusive lower/upper world-space bounds of the strategic lane corridor in simulation
    /// subunits. A unit whose full collision circle fits between these bounds is considered lane
    /// aligned and resumes the normal horizontal objective march from its current `y`.
    pub min_y: i32,
    pub max_y: i32,
}

impl TargetlessLane {
    #[must_use]
    pub const fn new(min_y: i32, max_y: i32) -> Self {
        Self { min_y, max_y }
    }
}

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub fog: Option<crate::FogRules>,
    pub match_seed: u64,
    pub spatial_cell_size: i32,
    pub navigation_cell_size: i32,
    pub navigation_min: NavCell,
    pub navigation_max: NavCell,
    pub target_pursuit_extra_range: i32,
    pub unit_separation_distance: i32,
    pub max_separation_per_tick: i32,
    pub static_blockers: Vec<BuildingFootprint>,
    /// Static cells that block flying-unit movement without making ordinary ground obstacles
    /// impassable to air units.
    pub air_static_blockers: Vec<BuildingFootprint>,
    /// Additional static placement-only blockers. These do not affect unit navigation.
    pub build_static_blockers: Vec<BuildingFootprint>,
    /// Canonical buildable regions for each team. An empty region list leaves that team
    /// unrestricted for generic/test maps that do not author build regions.
    pub team_build_regions: [Vec<BuildingFootprint>; 2],
    /// Optional authored castle rectangle for each team. Castle Fight uses this for builder Blink
    /// clamping and Human Artillery targeting; generic/test maps can leave it unset and fall back
    /// to the team's build region.
    pub team_castle_regions: [Option<BuildingFootprint>; 2],
    /// Optional standard-lane ingress guidance for targetless units. Generic maps leave this unset.
    pub targetless_lane: Option<TargetlessLane>,
    pub team_objective: [SimPoint; 2],
    pub economy: EconomyRules,
    pub match_rules: crate::MatchRules,
    pub rescue_strike_rules: Option<crate::RescueStrikeRules>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CombatRules {
    pub terrain_elevation: Option<TerrainElevationMap>,
    pub uphill_miss_chance_per_10k: u16,
    pub damage_rules: DamageRules,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            fog: None,
            match_seed: 0,
            spatial_cell_size: 8 * SUBUNITS_PER_WORLD_UNIT,
            navigation_cell_size: SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, -64),
            navigation_max: NavCell::new(120, 64),
            target_pursuit_extra_range: 3 * SUBUNITS_PER_WORLD_UNIT,
            unit_separation_distance: 3 * SUBUNITS_PER_WORLD_UNIT / 4,
            max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT / 16,
            static_blockers: Vec::new(),
            air_static_blockers: Vec::new(),
            build_static_blockers: Vec::new(),
            team_build_regions: [Vec::new(), Vec::new()],
            team_castle_regions: [None, None],
            targetless_lane: None,
            team_objective: [
                SimPoint::new(120 * SUBUNITS_PER_WORLD_UNIT, 0),
                SimPoint::new(0, 0),
            ],
            economy: EconomyRules::default(),
            match_rules: crate::MatchRules::default(),
            rescue_strike_rules: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerConfig {
    pub id: PlayerId,
    pub team: Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerConnectionStatus {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchOutcome {
    Victory(Team),
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchLifecycle {
    Running,
    PausedForDisconnect {
        disconnected_teams_mask: u8,
    },
    RoundReview {
        outcome: MatchOutcome,
        ended_tick: u64,
        next_round_tick: u64,
    },
    Finished {
        outcome: MatchOutcome,
        finished_tick: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamObjectiveError {
    UnsupportedTeam,
    ObjectiveNotFound,
    TeamMismatch,
    ObjectiveAlreadyRegistered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerView {
    pub id: PlayerId,
    pub team: Team,
    pub resources: PlayerResources,
    pub connection: PlayerConnectionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct PlayerState {
    id: PlayerId,
    team: Team,
    resources: PlayerResources,
    connection: PlayerConnectionStatus,
}

const DEFAULT_PLAYER_CONFIGS: [PlayerConfig; 2] = [
    PlayerConfig {
        id: PlayerId(0),
        team: Team(0),
    },
    PlayerConfig {
        id: PlayerId(1),
        team: Team(1),
    },
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickTimings {
    pub topology: Duration,
    pub timers: Duration,
    pub production: Duration,
    pub snapshot_and_spatial: Duration,
    pub abilities: Duration,
    pub targeting: Duration,
    pub combat: Duration,
    pub movement_intent: Duration,
    pub crowd_separation: Duration,
    pub hard_collision: Duration,
    pub collision_fallback_search: Duration,
    pub crowd_and_collision: Duration,
    pub ballistic_impact: Duration,
    pub structural_commit: Duration,
    pub checksum: Duration,
    pub total: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickResult {
    pub completed_tick: u64,
    pub units_alive: usize,
    pub buildings_alive: usize,
    pub corpses_alive: usize,
    pub attacks_resolved: usize,
    pub deaths: usize,
    pub corpses_spawned: usize,
    pub corpses_expired: usize,
    pub units_spawned: usize,
    pub spawn_failures: usize,
    pub topology_rebuilds: usize,
    pub pursuit_steps: usize,
    pub navigation_route_steps: usize,
    pub movement_intents: usize,
    pub movement_blocked: usize,
    pub collision_fallback_searches: usize,
    pub collision_fallback_candidate_checks: usize,
    pub collision_fallback_max_ring: u32,
    pub objective_move_intents: usize,
    pub a_star_fallbacks: usize,
    pub a_star_cache_hits: usize,
    pub a_star_expanded_nodes: usize,
    pub projectiles_alive: usize,
    pub projectiles_launched: usize,
    pub projectile_impacts: usize,
    pub projectile_effects: usize,
    pub projectile_invalidations: usize,
    pub ballistic_candidate_checks: usize,
    pub bounce_jumps: usize,
    pub bounce_candidate_checks: usize,
    pub ability_evaluations: usize,
    pub ability_casts: usize,
    pub ability_candidate_checks: usize,
    pub ability_effects: usize,
    pub stunned_units: usize,
    pub timed_movement_modifiers: usize,
    pub retained_targets: usize,
    pub target_changes: usize,
    pub ally_defense_queries: usize,
    pub ally_defense_victim_candidates: usize,
    pub ally_defense_attacker_candidates: usize,
    pub checksum: u64,
    pub timings: TickTimings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackEvent {
    pub source: SimId,
    pub target: SimId,
    pub source_position: SimPoint,
    pub target_position: SimPoint,
    pub delivery: AttackDelivery,
    pub missed: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AbilityCastTarget {
    Unit(SimId),
    AllEnemyUnits,
    AllFriendlyUnits,
    Point(SimPoint),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityCastEvent {
    pub source: SimId,
    pub ability: AbilityId,
    pub target: AbilityCastTarget,
    pub target_position: Option<SimPoint>,
    pub effect: AbilityEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainLightningEvent {
    pub source: SimId,
    pub ability: AbilityId,
    pub bounce_index: u8,
    points: [SimPoint; MAX_BOUNCE_HITS + 1],
    point_count: u8,
}

impl ChainLightningEvent {
    #[must_use]
    pub fn points(&self) -> &[SimPoint] {
        &self.points[..usize::from(self.point_count)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileViewKind {
    SphereFlight {
        destination: SimPoint,
        source_movement: MovementClass,
        target_movement: MovementClass,
    },
    NativeMover {
        rawcode: u32,
        owner: PlayerId,
        facing_degrees: i16,
        height_world_units: i32,
    },
    NativeCarrierBolt {
        target: SimId,
        ability: AbilityId,
    },
    GuaranteedHit {
        target: SimId,
    },
    Line {
        source_rawcode: Option<u32>,
        destination: SimPoint,
        spill_origin: Option<SimPoint>,
        primary_target: SimId,
        primary_impact_tick: u64,
    },
    Ballistic {
        source_rawcode: Option<u32>,
        destination: SimPoint,
        impact_radius: i32,
    },
    Bounce {
        source_rawcode: Option<u32>,
        target: SimId,
        bounce_index: u8,
        remaining_bounces: u8,
    },
    Reflected {
        target: SimId,
        reflector: SimId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileView {
    pub ability: Option<AbilityId>,
    pub id: SimId,
    pub source: SimId,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
    pub kind: ProjectileViewKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurningOilView {
    pub id: SimId,
    pub ability: AbilityId,
    pub center: SimPoint,
    pub recipients: Vec<SimId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpseView {
    pub vertex_rgba: Option<[u8; 4]>,
    pub id: SimId,
    pub position: SimPoint,
    pub source_unit: SimId,
    pub source_owner: PlayerId,
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
    pub decay_start_tick: u64,
    pub expires_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub content: Option<ContentIdentity>,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub collision_radius: i32,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub classifications: UnitClassifications,
    pub health: i32,
    pub health_max: i32,
    pub attack: AttackProfile,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub attack_delivery: AttackDelivery,
    pub attack_targets: AttackTargetMask,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub target: Option<SimId>,
    pub direct_retaliation_lock: bool,
    pub ally_defense_lock: bool,
    pub last_attacker: Option<SimId>,
    pub last_attacked_tick: Option<u64>,
    pub cooldown_remaining: u16,
    pub stunned_until_tick: u64,
    pub status: StatusState,
    pub mana_current: Option<i32>,
    pub mana_maximum: Option<i32>,
    pub ability_ready_tick: Option<u64>,
    pub ability_cast_sequence: Option<u64>,
    /// Active auto-maintained Warcraft Defend ability, if any.
    pub active_defend_ability: Option<AbilityId>,
    pub hex: Option<HexState>,
    pub negative_building_shield_level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderView {
    pub id: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub profile: BuilderProfile,
    pub configuration: BuilderConfiguration,
    pub destination: Option<SimPoint>,
    pub follow_target: Option<SimId>,
    pub repair_target: Option<SimId>,
    pub build_footprint: Option<BuildingFootprint>,
    pub build_content: Option<ContentIdentity>,
    pub orders: Vec<BuilderOrderView>,
    pub repair_autocast_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderOrderView {
    Build {
        content: Option<ContentIdentity>,
        footprint: BuildingFootprint,
        economy: Option<BuildingEconomyProfile>,
    },
    Move(SimPoint),
    Follow(SimId),
    Repair(SimId),
    Blink(SimPoint),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingView {
    pub id: SimId,
    pub classifications: UnitClassifications,
    pub content: Option<ContentIdentity>,
    pub owner: Option<PlayerId>,
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub health_max: i32,
    pub construction_started_tick: Option<u64>,
    pub construction_complete_tick: Option<u64>,
    pub production: Option<ProductionProfile>,
    pub production_movement_class: Option<MovementClass>,
    pub production_attack_targets: Option<AttackTargetMask>,
    pub next_spawn_tick: Option<u64>,
    pub production_queue: Option<u8>,
    pub attack: Option<AttackProfile>,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub attack_delivery: Option<AttackDelivery>,
    pub attack_targets: Option<AttackTargetMask>,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub target: Option<SimId>,
    pub cooldown_remaining: Option<u16>,
    pub mana_current: Option<i32>,
    pub mana_maximum: Option<i32>,
    pub ability_ready_tick: Option<u64>,
    pub ability_cast_sequence: Option<u64>,
    pub ability_autocast_enabled: Option<bool>,
    pub stunned_until_tick: Option<u64>,
    /// Presentation readback of authoritative status, including passive structure buffs.
    pub status: StatusState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingPlacementError {
    OutsideNavigation,
    OutsideBuildRegion,
    StaticObstacle,
    BuildingOverlap,
    BuildingReserved,
    UnitOccupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderSpawnError {
    UnsupportedPlayer,
    UnsupportedTeam,
    PlayerAlreadyHasBuilder,
    PlayerTeamMismatch,
    OutsideBuildRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderCommandError {
    OrderQueueFull,
    BuilderNotFound,
    NotAuthorized,
    OutsideBuildRegion,
    BlinkOutOfRange,
    FollowTargetNotFound,
    RepairTargetNotFound,
    NotFriendlyRepairTarget,
    RepairTargetNotRepairable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderBuildError {
    TypeLimitReached,
    Builder(BuilderCommandError),
    TeamMismatch,
    MissingBuildingIdentity,
    MissingEconomyProfile,
    BuildingNotInCatalog,
    Resources(ResourcePurchaseError),
    Placement(BuildingPlacementError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingConstructionCancelError {
    ConstructionNotFound,
    NotOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingConstructionCancelOutcome {
    RemovedNewBuilding,
    RevertedUpgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingUpgradeError {
    TypeLimitReached,
    SourceNotFound,
    NotOwner,
    SourceUnderConstruction,
    SourceDisabled,
    ProductionQueueNotEmpty,
    SourceDefinitionMismatch,
    TeamMismatch,
    FootprintMismatch,
    MissingEconomyProfile,
    Resources(ResourcePurchaseError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingCommandError {
    SourceNotFound,
    NotAuthorized,
    SourceCannotAttack,
    SourceCannotProduce,
    SourceCannotCast,
    InsufficientMana,
    AbilityOnCooldown,
    ProductionQueueEmpty,
    ProductionQueueFull,
    TargetNotFound,
    FriendlyTarget,
    InvalidTargetType,
    TargetOutOfRange,
}

const PURSUIT_CACHE_CAPACITY: usize = 65_536;

pub struct Simulation {
    world: World,
    config: SimulationConfig,
    combat_rules: CombatRules,
    pool: ThreadPool,
    topology: TopologyGrid,
    air_topology: TopologyGrid,
    topology_dirty: bool,
    pursuit_cache: BTreeMap<(NavCell, NavCell, Option<i32>, i8), NavCell>,
    radius_objective_fields: BTreeMap<(u8, i32), Vec<u32>>,
    defense_alerts: Vec<DefenseAlert>,
    last_attacks: Vec<AttackEvent>,
    last_ability_casts: Vec<AbilityCastEvent>,
    last_death_heals: Vec<DeathHealVisualEvent>,
    last_shrine_revivals: Vec<ShrineRevivalEvent>,
    shrine_death_generation: u64,
    retired_damage_sources: BTreeMap<SimId, source_damage_runtime::RetiredDamageSource>,
    last_building_spell_visuals: Vec<BuildingSpellVisualEvent>,
    last_chain_lightnings: Vec<ChainLightningEvent>,
    last_rescue_strike_visuals: Vec<RescueStrikeVisualEvent>,
    pending_rescue_strikes: Vec<PendingRescueStrike>,
    player_strikes: Vec<PlayerStrikeState>,
    team_rescue_ready_ticks: [u64; 2],
    round_index: u16,
    round_wins: [u8; 2],
    round_start_tick: u64,
    review_until_tick: Option<u64>,
    last_round_scoreboard: Option<RoundScoreboard>,
    initial_round: Option<Box<SimulationSnapshot>>,
    players: Vec<PlayerState>,
    lifecycle: MatchLifecycle,
    team_objectives: [Option<SimId>; 2],
    gjallarhorn_constructed_count: [u32; 2],
    next_tick: u64,
    next_id: u64,
    next_visual_id: u64,
    configuration_identity: u64,
    debug_buildings_invulnerable: bool,
    fog: Option<crate::FogOfWar>,
}

impl Simulation {
    pub fn new(config: SimulationConfig, workers: usize) -> Self {
        Self::new_with_combat_rules(config, workers, CombatRules::default())
    }

    pub fn new_with_combat_rules(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
    ) -> Self {
        Self::new_internal(config, workers, combat_rules, None, &DEFAULT_PLAYER_CONFIGS)
    }

    pub fn new_with_gameplay_bundle(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
        gameplay_bundle: GameplayBundleIdentity,
    ) -> Self {
        Self::new_internal(
            config,
            workers,
            combat_rules,
            Some(gameplay_bundle),
            &DEFAULT_PLAYER_CONFIGS,
        )
    }

    pub fn new_with_gameplay_bundle_and_players(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
        gameplay_bundle: GameplayBundleIdentity,
        players: &[PlayerConfig],
    ) -> Self {
        Self::new_internal(
            config,
            workers,
            combat_rules,
            Some(gameplay_bundle),
            players,
        )
    }

    fn new_internal(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
        gameplay_bundle: Option<GameplayBundleIdentity>,
        player_configs: &[PlayerConfig],
    ) -> Self {
        assert!(workers > 0, "simulation requires at least one worker");
        assert!(config.spatial_cell_size > 0);
        assert!(config.navigation_cell_size > 0);
        assert!(config.target_pursuit_extra_range >= 0);
        assert!(config.unit_separation_distance >= 0);
        assert_eq!(
            config.unit_separation_distance % 2,
            0,
            "fallback unit separation must be an even collision diameter"
        );
        assert!(config.max_separation_per_tick >= 0);
        if let Some(lane) = config.targetless_lane {
            assert!(
                lane.min_y <= lane.max_y,
                "targetless lane y bounds are inverted"
            );
        }
        validate_combat_rules(&config, &combat_rules);
        let player_configs = validate_player_configs(player_configs);
        let configuration_identity = canonical_configuration_identity(
            &config,
            &combat_rules,
            gameplay_bundle,
            &player_configs,
        );
        let starting_resources = PlayerResources {
            gold: config.economy.starting_gold,
            lumber: config.economy.starting_lumber,
            legendary_points_used: 0,
            legendary_points_cap: config.economy.starting_legendary_points,
        };

        let rescue_strikes_enabled = config.match_rules.rescue_strikes_enabled;
        let players = player_configs
            .iter()
            .map(|player| PlayerState {
                id: player.id,
                team: player.team,
                resources: starting_resources,
                connection: PlayerConnectionStatus::Connected,
            })
            .collect();

        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("castle-sim-{index}"))
            .build()
            .expect("failed to create simulation worker pool");
        let topology = TopologyGrid::build(
            config.navigation_cell_size,
            config.navigation_min,
            config.navigation_max,
            config.static_blockers.iter().copied(),
            config.team_objective,
        );
        let air_topology = TopologyGrid::build(
            config.navigation_cell_size,
            config.navigation_min,
            config.navigation_max,
            config.air_static_blockers.iter().copied(),
            config.team_objective,
        );

        let fog = config.fog.as_ref().map(|rules| {
            crate::FogOfWar::new(
                SimPoint::new(
                    config.navigation_min.x * config.navigation_cell_size,
                    config.navigation_min.y * config.navigation_cell_size,
                ),
                SimPoint::new(
                    (config.navigation_max.x + 1) * config.navigation_cell_size,
                    (config.navigation_max.y + 1) * config.navigation_cell_size,
                ),
                rules,
            )
        });
        Self {
            fog,
            world: World::new(),
            config,
            combat_rules,
            pool,
            topology,
            air_topology,
            topology_dirty: false,
            pursuit_cache: BTreeMap::new(),
            radius_objective_fields: BTreeMap::new(),
            defense_alerts: Vec::new(),
            last_attacks: Vec::new(),
            last_ability_casts: Vec::new(),
            last_death_heals: Vec::new(),
            last_shrine_revivals: Vec::new(),
            shrine_death_generation: 1,
            retired_damage_sources: BTreeMap::new(),
            last_building_spell_visuals: Vec::new(),
            last_chain_lightnings: Vec::new(),
            last_rescue_strike_visuals: Vec::new(),
            pending_rescue_strikes: Vec::new(),
            player_strikes: player_configs
                .iter()
                .map(|player| PlayerStrikeState {
                    player: player.id,
                    available: rescue_strikes_enabled,
                    cooldown_until_tick: 0,
                })
                .collect(),
            team_rescue_ready_ticks: [0; 2],
            round_index: 1,
            round_wins: [0; 2],
            round_start_tick: 0,
            review_until_tick: None,
            last_round_scoreboard: None,
            initial_round: None,
            players,
            lifecycle: MatchLifecycle::Running,
            team_objectives: [None, None],
            gjallarhorn_constructed_count: [0, 0],
            next_tick: 0,
            next_id: 1,
            next_visual_id: 1,
            configuration_identity,
            debug_buildings_invulnerable: false,
        }
    }

    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.next_tick
    }

    #[must_use]
    pub const fn damage_rules(&self) -> DamageRules {
        self.combat_rules.damage_rules
    }

    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.pool.current_num_threads()
    }

    #[must_use]
    pub const fn lifecycle(&self) -> MatchLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub fn team_objective(&self, team: Team) -> Option<SimId> {
        self.team_objectives
            .get(usize::from(team.0))
            .copied()
            .flatten()
    }

    #[must_use]
    pub fn team_build_regions(&self, team: Team) -> &[BuildingFootprint] {
        self.config
            .team_build_regions
            .get(usize::from(team.0))
            .map_or(&[], Vec::as_slice)
    }

    pub fn register_team_objective(
        &mut self,
        team: Team,
        objective: SimId,
    ) -> Result<(), TeamObjectiveError> {
        let Some(slot) = self.team_objectives.get(usize::from(team.0)) else {
            return Err(TeamObjectiveError::UnsupportedTeam);
        };
        if slot.is_some() {
            return Err(TeamObjectiveError::ObjectiveAlreadyRegistered);
        }
        let Some(entity) = self.world.iter_entities().find(|entity| {
            entity.get::<SimId>().copied() == Some(objective)
                && entity.get::<BuildingFootprint>().is_some()
        }) else {
            return Err(TeamObjectiveError::ObjectiveNotFound);
        };
        if entity.get::<Team>().copied() != Some(team) {
            return Err(TeamObjectiveError::TeamMismatch);
        }
        self.team_objectives[usize::from(team.0)] = Some(objective);
        Ok(())
    }

    pub(crate) fn set_player_connection_status(
        &mut self,
        player: PlayerId,
        connection: PlayerConnectionStatus,
    ) -> bool {
        if matches!(self.lifecycle, MatchLifecycle::Finished { .. }) {
            return false;
        }
        let Some(state) = self.player_state_mut(player) else {
            return false;
        };
        state.connection = connection;
        self.refresh_disconnect_pause();
        true
    }

    pub(crate) fn finish_match_from_control(&mut self, outcome: MatchOutcome) -> bool {
        if matches!(self.lifecycle, MatchLifecycle::Finished { .. }) {
            return false;
        }
        if self.review_until_tick.is_none() {
            self.capture_round_scoreboard(outcome, self.next_tick.saturating_sub(1));
        }
        self.review_until_tick = None;
        self.invalidate_pending_golden_shrine_revivals();
        self.clear_snow();
        self.clear_world_freezer();
        self.lifecycle = MatchLifecycle::Finished {
            outcome,
            finished_tick: self.next_tick.saturating_sub(1),
        };
        true
    }

    #[must_use]
    pub fn config(&self) -> &SimulationConfig {
        &self.config
    }

    pub fn players(&self) -> Vec<PlayerView> {
        self.players
            .iter()
            .map(|player| PlayerView {
                id: player.id,
                team: player.team,
                resources: player.resources,
                connection: player.connection,
            })
            .collect()
    }

    #[must_use]
    pub fn player(&self, id: PlayerId) -> Option<PlayerView> {
        let player = self.player_state(id)?;
        Some(PlayerView {
            id: player.id,
            team: player.team,
            resources: player.resources,
            connection: player.connection,
        })
    }

    /// Enables or disables developer-tool building damage immunity.
    ///
    /// This runtime flag is canonical and snapshot-persisted for host debugging. It suppresses
    /// health loss only: buildings remain targetable, attacks still resolve, and projectile/on-hit
    /// behavior continues normally.
    pub fn debug_set_buildings_invulnerable(&mut self, enabled: bool) {
        self.debug_buildings_invulnerable = enabled;
    }

    #[must_use]
    pub const fn debug_buildings_invulnerable(&self) -> bool {
        self.debug_buildings_invulnerable
    }

    /// Applies direct developer-tool damage to every live combat unit.
    ///
    /// The query intentionally matches the normal unit archetype via `MovementProfile`. Buildings
    /// and builders therefore do not need special-case exclusions. Fatal damage follows the normal
    /// unit despawn/corpse lifecycle, and fatalities are ordered by `SimId` so corpse allocation is
    /// deterministic.
    pub fn debug_damage_all_units(&mut self, damage: i32) -> usize {
        assert!(damage >= 0, "debug unit damage must be non-negative");

        let retained_sources = self
            .snapshot_units()
            .iter()
            .filter_map(|unit| {
                source_damage_runtime::RetiredDamageSource::capture(unit)
                    .map(|source| (unit.id, source))
            })
            .collect::<BTreeMap<_, _>>();
        let mut affected = 0usize;
        let mut fatalities = Vec::new();
        {
            let mut query = self.world.query::<(
                Entity,
                &SimId,
                &Owner,
                &Team,
                &Position,
                &mut Health,
                Option<&CorpseProducer>,
                Option<&ResurrectionProfile>,
                &MovementProfile,
            )>();
            for (entity, id, owner, team, position, mut health, corpse, resurrection, _) in
                query.iter_mut(&mut self.world)
            {
                if health.current <= 0 {
                    continue;
                }
                affected += 1;
                health.current = health
                    .current
                    .checked_sub(damage)
                    .expect("debug unit damage overflowed validated bounds");
                if health.current <= 0 {
                    fatalities.push((
                        entity,
                        *id,
                        owner.0,
                        *team,
                        position.0,
                        corpse.map(|corpse| corpse.0),
                        resurrection.map(|profile| profile.0),
                    ));
                }
            }
        }

        fatalities.sort_unstable_by_key(|(_, id, ..)| *id);
        for (entity, source_unit, source_owner, source_team, position, corpse, resurrection) in
            fatalities
        {
            let shrine_state = self
                .world
                .get::<ShrineRevivalState>(entity)
                .copied()
                .unwrap_or_default();
            let vertex_rgba = self
                .world
                .get::<StatusState>(entity)
                .and_then(|status| status.damage_threshold.vertex_rgba);
            if let Some(source) = retained_sources.get(&source_unit) {
                self.retired_damage_sources.insert(source_unit, *source);
            }
            self.spawn_death_children(entity, position);
            self.world.despawn(entity);
            let Some(profile) = corpse else {
                continue;
            };
            let id = self.allocate_id();
            let decay_start_tick = self
                .next_tick
                .checked_add(u64::from(profile.decay_start_ticks))
                .expect("corpse decay-start tick overflow");
            let expires_tick = profile.lifetime_ticks.map(|lifetime_ticks| {
                self.next_tick
                    .checked_add(u64::from(lifetime_ticks))
                    .expect("corpse expiry tick overflow")
            });
            self.world.spawn((
                id,
                Position(position),
                Corpse {
                    vertex_rgba,
                    source_unit,
                    source_owner,
                    source_team,
                    definition: profile.definition,
                    created_tick: self.next_tick,
                    decay_start_tick,
                    expires_tick,
                    resurrection,
                    shrine_state,
                },
            ));
        }

        affected
    }

    fn player_state(&self, player: PlayerId) -> Option<&PlayerState> {
        self.players
            .binary_search_by_key(&player, |state| state.id)
            .ok()
            .map(|index| &self.players[index])
    }

    fn player_state_mut(&mut self, player: PlayerId) -> Option<&mut PlayerState> {
        let index = self
            .players
            .binary_search_by_key(&player, |state| state.id)
            .ok()?;
        Some(&mut self.players[index])
    }

    fn unique_player_for_team(&self, team: Team) -> Option<PlayerId> {
        let mut players = self
            .players
            .iter()
            .filter(|player| player.team == team)
            .map(|player| player.id);
        let player = players.next()?;
        players.next().is_none().then_some(player)
    }

    fn inferred_owner_for_team(&self, team: Team) -> PlayerId {
        self.unique_player_for_team(team).unwrap_or_else(|| {
            panic!(
                "team {} does not have exactly one player; use an explicit PlayerId ownership API",
                team.0
            )
        })
    }

    fn refresh_disconnect_pause(&mut self) {
        if matches!(self.lifecycle, MatchLifecycle::Finished { .. }) {
            return;
        }
        let mut disconnected_teams_mask = 0u8;
        for team in [Team(0), Team(1)] {
            let mut team_players = self.players.iter().filter(|player| player.team == team);
            let Some(first) = team_players.next() else {
                continue;
            };
            let any_connected = first.connection == PlayerConnectionStatus::Connected
                || team_players
                    .any(|player| player.connection == PlayerConnectionStatus::Connected);
            if !any_connected {
                disconnected_teams_mask |= 1 << team.0;
            }
        }
        self.lifecycle = if disconnected_teams_mask == 0 {
            if let Some(next_round_tick) = self.review_until_tick {
                let score = self
                    .last_round_scoreboard
                    .as_ref()
                    .expect("review requires scoreboard");
                MatchLifecycle::RoundReview {
                    outcome: score.outcome,
                    ended_tick: score.ended_tick,
                    next_round_tick,
                }
            } else {
                MatchLifecycle::Running
            }
        } else {
            MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask,
            }
        };
    }

    fn evaluate_objective_outcome(&mut self, completed_tick: u64) {
        let [Some(team_0_objective), Some(team_1_objective)] = self.team_objectives else {
            self.evaluate_match_time_limit(completed_tick);
            return;
        };
        let team_0_alive = self.world.iter_entities().any(|entity| {
            entity.get::<SimId>().copied() == Some(team_0_objective)
                && entity
                    .get::<Health>()
                    .is_some_and(|health| health.current > 0)
        });
        let team_1_alive = self.world.iter_entities().any(|entity| {
            entity.get::<SimId>().copied() == Some(team_1_objective)
                && entity
                    .get::<Health>()
                    .is_some_and(|health| health.current > 0)
        });
        let outcome = match (team_0_alive, team_1_alive) {
            (true, true) => {
                self.evaluate_match_time_limit(completed_tick);
                return;
            }
            (false, false) => MatchOutcome::Draw,
            (false, true) => MatchOutcome::Victory(Team(1)),
            (true, false) => MatchOutcome::Victory(Team(0)),
        };
        self.finish_round(outcome, completed_tick);
    }

    fn assert_player_team(&self, player: PlayerId, team: Team) {
        let state = self
            .player_state(player)
            .unwrap_or_else(|| panic!("unknown player {}", player.0));
        assert_eq!(
            state.team, team,
            "player {} belongs to team {}, not team {}",
            player.0, state.team.0, team.0
        );
    }

    pub(crate) fn order_building_attack_target(
        &mut self,
        source: SimId,
        target: SimId,
    ) -> Result<(), BuildingCommandError> {
        let (source_entity, source_team, source_footprint, attack, attack_targets, secondary) =
            self.world
                .iter_entities()
                .find_map(|entity| {
                    (entity.get::<SimId>().copied() == Some(source)
                        && entity.get::<BuildingFootprint>().is_some())
                    .then(|| {
                        (
                            entity.id(),
                            *entity.get::<Team>().expect("building missing team"),
                            *entity
                                .get::<BuildingFootprint>()
                                .expect("building missing footprint"),
                            entity.get::<AttackProfile>().copied(),
                            entity.get::<AttackTargetMask>().copied(),
                            entity.get::<SecondaryAttackProfile>().copied(),
                        )
                    })
                })
                .ok_or(BuildingCommandError::SourceNotFound)?;
        let mut attack = attack.ok_or(BuildingCommandError::SourceCannotAttack)?;
        let attack_targets = attack_targets.expect("attack building missing target mask");

        let target_entity = self
            .world
            .iter_entities()
            .find(|entity| {
                entity.get::<SimId>().copied() == Some(target)
                    && entity
                        .get::<Health>()
                        .is_some_and(|health| health.current > 0)
            })
            .ok_or(BuildingCommandError::TargetNotFound)?;
        let target_team = *target_entity
            .get::<Team>()
            .expect("attack target missing team");
        if target_team == source_team {
            return Err(BuildingCommandError::FriendlyTarget);
        }

        let distance_sq = if let Some(position) = target_entity.get::<Position>() {
            let movement_class = *target_entity
                .get::<MovementClass>()
                .expect("unit attack target missing movement class");
            let movement_class = if target_entity.get::<StatusState>().is_some_and(|status| {
                status
                    .ensnare
                    .is_some_and(|net| self.next_tick < net.expires_tick)
            }) {
                MovementClass::Ground
            } else {
                movement_class
            };
            if !attack_targets.can_target_unit(movement_class) {
                return Err(BuildingCommandError::InvalidTargetType);
            }
            if let Some(second) = secondary
                && !second.primary_targets.can_target_unit(movement_class)
            {
                attack = second.attack;
            }
            point_to_footprint_distance_sq(
                position.0,
                source_footprint,
                self.config.navigation_cell_size,
            )
        } else if let Some(target_footprint) = target_entity.get::<BuildingFootprint>() {
            if !attack_targets.can_target_buildings() {
                return Err(BuildingCommandError::InvalidTargetType);
            }
            if let Some(second) = secondary
                && !second.primary_targets.can_target_buildings()
            {
                attack = second.attack;
            }
            footprint_to_footprint_distance_sq(
                source_footprint,
                *target_footprint,
                self.config.navigation_cell_size,
            )
        } else {
            return Err(BuildingCommandError::TargetNotFound);
        };
        if distance_sq > attack.acquisition_range_sq() {
            return Err(BuildingCommandError::TargetOutOfRange);
        }

        let mut source = self.world.entity_mut(source_entity);
        let mut state = source
            .get_mut::<TargetState>()
            .expect("attack building missing target state");
        state.current = Some(target);
        state.direct_retaliation_lock = false;
        state.ally_defense_lock = false;
        Ok(())
    }

    pub fn spawn_unit(&mut self, unit: UnitSpawn) -> SimId {
        self.spawn_unit_with_properties(unit, UnitGameplayProperties::default())
    }

    pub fn spawn_resolved_unit(
        &mut self,
        team: Team,
        position: SimPoint,
        definition: ResolvedUnitDefinition,
    ) -> SimId {
        let owner = self.inferred_owner_for_team(team);
        self.spawn_resolved_unit_for_player(owner, team, position, definition)
    }

    pub fn spawn_resolved_unit_for_player(
        &mut self,
        owner: PlayerId,
        team: Team,
        position: SimPoint,
        definition: ResolvedUnitDefinition,
    ) -> SimId {
        self.assert_player_team(owner, team);
        let unit = UnitSpawn::from_template(team, position, definition.template);
        validate_unit_spawn(unit);
        self.validate_unit_gameplay_properties(position, definition.properties);
        if let Some(spellcasting) = definition.spellcasting {
            validate_spellcasting_profile(spellcasting);
        }
        self.spawn_resolved_unit_unchecked(Some(owner), unit, definition)
    }

    pub fn spawn_unit_with_spellcasting(
        &mut self,
        unit: UnitSpawn,
        spellcasting: SpellcastingProfile,
    ) -> SimId {
        self.spawn_unit_with_properties_and_spellcasting(
            unit,
            UnitGameplayProperties::default(),
            spellcasting,
        )
    }

    pub fn spawn_unit_with_properties_and_spellcasting(
        &mut self,
        unit: UnitSpawn,
        properties: UnitGameplayProperties,
        spellcasting: SpellcastingProfile,
    ) -> SimId {
        validate_unit_spawn(unit);
        self.validate_unit_gameplay_properties(unit.position, properties);
        validate_spellcasting_profile(spellcasting);
        let owner = self.inferred_owner_for_team(unit.team);
        self.spawn_unit_unchecked(Some(owner), unit, properties, Some(spellcasting))
    }

    pub fn spawn_unit_with_corpse(&mut self, unit: UnitSpawn, corpse: CorpseProfile) -> SimId {
        self.spawn_unit_with_properties(
            unit,
            UnitGameplayProperties {
                corpse: Some(corpse),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_unit_with_collision_radius(
        &mut self,
        unit: UnitSpawn,
        collision_radius: CollisionRadius,
    ) -> SimId {
        self.spawn_unit_with_properties(
            unit,
            UnitGameplayProperties {
                collision_radius: Some(collision_radius),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_unit_with_properties(
        &mut self,
        unit: UnitSpawn,
        properties: UnitGameplayProperties,
    ) -> SimId {
        validate_unit_spawn(unit);
        self.validate_unit_gameplay_properties(unit.position, properties);
        let owner = self.inferred_owner_for_team(unit.team);
        self.spawn_unit_unchecked(Some(owner), unit, properties, None)
    }

    fn validate_unit_gameplay_properties(
        &mut self,
        position: SimPoint,
        properties: UnitGameplayProperties,
    ) {
        assert!(
            properties
                .passive_effects
                .iter()
                .filter(|effect| matches!(effect, PassiveUnitEffect::DamageThreshold(_)))
                .count()
                <= 1,
            "a unit supports one persistent damage-threshold controller"
        );
        if let Some(corpse) = properties.corpse {
            validate_corpse_profile(corpse);
        }
        if let Some(collision_radius) = properties.collision_radius {
            validate_collision_radius(collision_radius);
        }
        if properties.mechanical {
            assert!(
                properties.build_time_ticks.is_some_and(|ticks| ticks > 0),
                "mechanical units require positive build-time metadata for repair"
            );
        }
        let collision_radius = properties
            .collision_radius
            .map_or(self.default_collision_radius(), |radius| radius.0);
        let legal = match properties.movement_class {
            MovementClass::Ground if properties.collision_radius.is_some() => {
                self.refresh_topology_if_dirty();
                let source_cell = self.topology.cell_of_point(position);
                self.topology
                    .component_id(source_cell)
                    .is_some_and(|component| {
                        self.topology.circle_is_traversable_in_component(
                            position,
                            collision_radius,
                            component,
                        )
                    })
            }
            MovementClass::Ground => self
                .topology
                .contains(self.topology.cell_of_point(position)),
            MovementClass::Air => {
                let source_cell = self.air_topology.cell_of_point(position);
                self.air_position_is_traversable_from(source_cell, position, collision_radius)
            }
        };
        assert!(
            legal,
            "authored collision unit footprint is outside legal movement space"
        );
    }

    fn footprint_overlaps_pending_build_order(
        &self,
        footprint: BuildingFootprint,
        ignore_builder_entity: Option<Entity>,
    ) -> bool {
        self.world.iter_entities().any(|entity| {
            if ignore_builder_entity == Some(entity.id()) {
                return false;
            }
            entity.get::<BuilderOrderQueue>().is_some_and(|queue| {
                queue
                    .0
                    .iter()
                    .filter_map(|order| order.build())
                    .any(|order| footprints_overlap(order.building.footprint, footprint))
            })
        })
    }

    fn footprint_inside_team_build_region(&self, team: Team, footprint: BuildingFootprint) -> bool {
        let Some(regions) = self.config.team_build_regions.get(usize::from(team.0)) else {
            return false;
        };
        regions.is_empty()
            || regions
                .iter()
                .copied()
                .any(|region| footprint_contains_footprint(region, footprint))
    }

    fn cell_inside_team_build_region(&self, team: Team, cell: NavCell) -> bool {
        let Some(regions) = self.config.team_build_regions.get(usize::from(team.0)) else {
            return false;
        };
        if regions.is_empty() {
            self.topology.contains(cell)
        } else {
            regions
                .iter()
                .copied()
                .any(|region| footprint_contains_cell(region, cell))
        }
    }

    fn point_inside_team_build_region(&self, team: Team, point: SimPoint) -> bool {
        self.cell_inside_team_build_region(team, self.topology.cell_of_point(point))
    }

    fn clamp_builder_blink_destination(
        &self,
        team: Team,
        destination: SimPoint,
        inset: i32,
    ) -> Option<SimPoint> {
        let team_index = usize::from(team.0);
        if let Some(region) = *self.config.team_castle_regions.get(team_index)? {
            return clamp_point_to_cell_rect(
                destination,
                NavCell::new(region.min_x, region.min_y),
                NavCell::new(region.max_x(), region.max_y()),
                self.config.navigation_cell_size,
                inset,
            );
        }

        let regions = self.config.team_build_regions.get(team_index)?;
        if regions.is_empty() {
            return clamp_point_to_cell_rect(
                destination,
                self.config.navigation_min,
                self.config.navigation_max,
                self.config.navigation_cell_size,
                inset,
            );
        }
        regions
            .iter()
            .copied()
            .filter_map(|region| {
                let min = NavCell::new(region.min_x, region.min_y);
                let max = NavCell::new(region.max_x(), region.max_y());
                clamp_point_to_cell_rect(
                    destination,
                    min,
                    max,
                    self.config.navigation_cell_size,
                    inset,
                )
            })
            .min_by_key(|point| (destination.distance_sq(*point), point.x, point.y))
    }

    fn footprint_contains_live_unit(&self, footprint: BuildingFootprint) -> bool {
        self.world.iter_entities().any(|entity| {
            let Some(position) = entity.get::<Position>() else {
                return false;
            };
            let Some(health) = entity.get::<Health>() else {
                return false;
            };
            if health.current <= 0
                || entity.get::<BuildingFootprint>().is_some()
                || entity.get::<MovementClass>() == Some(&MovementClass::Air)
            {
                return false;
            }
            if let Some(radius) = entity.get::<CollisionRadius>() {
                let distance_sq = point_to_footprint_distance_sq(
                    position.0,
                    footprint,
                    self.config.navigation_cell_size,
                );
                distance_sq == 0 || distance_sq < square_i32(radius.0)
            } else {
                footprint_contains_cell(footprint, self.topology.cell_of_point(position.0))
            }
        })
    }

    fn footprint_inside_navigation(&self, footprint: BuildingFootprint) -> bool {
        (footprint.min_y..=footprint.max_y()).all(|y| {
            (footprint.min_x..=footprint.max_x())
                .all(|x| self.topology.contains(NavCell::new(x, y)))
        })
    }

    pub fn step(&mut self) -> TickResult {
        if let MatchLifecycle::RoundReview {
            next_round_tick, ..
        } = self.lifecycle
        {
            self.clear_presentation_events();
            let completed_tick = self.next_tick;
            self.next_tick += 1;
            if self.next_tick >= next_round_tick {
                self.reset_for_next_round(next_round_tick);
            }
            return TickResult {
                completed_tick,
                units_alive: self.unit_count(),
                buildings_alive: self.building_count(),
                corpses_alive: self.corpse_count(),
                projectiles_alive: self.projectile_count(),
                checksum: self.checksum(),
                ..Default::default()
            };
        }
        if self.lifecycle != MatchLifecycle::Running {
            // Presentation events describe one completed gameplay tick. A paused/terminal step has
            // no gameplay tick, so do not keep replaying the previous tick's cosmetic events while
            // the authoritative state is frozen.
            self.clear_presentation_events();
            return self.frozen_tick_result();
        }
        self.clear_presentation_events();
        let tick_start = Instant::now();
        let completed_tick = self.next_tick;

        let phase_start = Instant::now();
        let mut topology_rebuilt = self.refresh_topology_if_dirty();
        let mut topology = phase_start.elapsed();

        let phase_start = Instant::now();
        self.refresh_mana_improvement_links();
        self.refresh_carrier_auras_for_timers();
        self.refresh_native_mana_auras_for_timers();
        self.refresh_native_building_auras_for_timers();
        self.advance_cooldowns();
        self.advance_rescue_strikes(completed_tick);
        self.resolve_shrine_revivals();
        self.advance_building_spell_controls();
        let mirror_spawns = self.advance_mirror_images() + self.advance_lava_spawns();
        let corpses_expired = self.expire_corpses();
        self.pause_frozen_building_activities();
        self.advance_builders();
        self.advance_building_construction();
        let timers = phase_start.elapsed();

        // Builder construction can add an occupied footprint during this tick. Rebuild before
        // target selection/movement so the site blocks ground navigation on its first live tick.
        let phase_start = Instant::now();
        topology_rebuilt |= self.refresh_topology_if_dirty();
        topology += phase_start.elapsed();

        let phase_start = Instant::now();
        let (units_spawned, spawn_failures) = self.advance_production();
        let production = phase_start.elapsed();

        let phase_start = Instant::now();
        self.refresh_vision();
        let mut units = self.snapshot_units();
        let mut buildings = self.snapshot_buildings();
        self.refresh_snow_protection(&mut units, &mut buildings);
        death_healing::refresh_support(&mut units, &buildings, None);
        resolve_periodic_unit_statuses(
            &mut units,
            completed_tick,
            self.combat_rules.damage_rules,
            &self.retired_damage_sources,
        );
        native_actions::resolve_native_building_damage_over_time(
            &mut buildings,
            completed_tick,
            self.combat_rules.damage_rules,
        );
        death_healing::refresh_support(&mut units, &buildings, None);
        self.resolve_burning_oil_zones(&mut units, &mut buildings, completed_tick);
        let mut grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            units
                .iter()
                .enumerate()
                .filter(|(_, unit)| unit.health > 0)
                .flat_map(|(index, unit)| {
                    let component_entry = (unit.movement_class == MovementClass::Ground)
                        .then(|| {
                            self.topology
                                .component_id(self.topology.cell_of_point(unit.position))
                        })
                        .flatten()
                        .map(|component| {
                            (
                                SpatialPartition::new(unit.team.0, component),
                                index,
                                unit.position,
                            )
                        });
                    [
                        component_entry,
                        Some((SpatialPartition::global(unit.team.0), index, unit.position)),
                    ]
                    .into_iter()
                    .flatten()
                }),
        );
        let mut snapshot_and_spatial = phase_start.elapsed();

        let phase_start = Instant::now();
        self.resolve_support_orders(&mut units);
        self.resolve_world_freezer(&mut units, &mut buildings);
        self.resolve_native_actions(&mut units, &mut buildings);
        self.resolve_permanent_immolation(&mut units);
        self.apply_passive_auras(&mut units, &buildings);
        self.apply_speed_auras(&mut units);
        self.resolve_damage_charge(&mut units, &mut buildings);
        self.refresh_native_building_auras(&mut units, &mut buildings);
        let ability_metrics = self.resolve_automatic_abilities(&mut buildings, &mut units, &grid);
        self.refresh_native_building_auras(&mut units, &mut buildings);
        if self
            .last_ability_casts
            .iter()
            .any(|c| matches!(c.effect, AbilityEffect::NagaCharm(_)))
        {
            // Ownership changes during spell resolution invalidate team partitions immediately.
            grid = SpatialGrid::build(
                self.config.spatial_cell_size,
                units
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| u.health > 0)
                    .flat_map(|(i, u)| {
                        let component = (u.movement_class == MovementClass::Ground)
                            .then(|| {
                                self.topology
                                    .component_id(self.topology.cell_of_point(u.position))
                            })
                            .flatten();
                        [
                            component.map(|c| (SpatialPartition::new(u.team.0, c), i, u.position)),
                            Some((SpatialPartition::global(u.team.0), i, u.position)),
                        ]
                        .into_iter()
                        .flatten()
                    }),
            );
        }
        let abilities = phase_start.elapsed();

        let phase_start = Instant::now();
        let needs_ally_defense_index = !self.defense_alerts.is_empty()
            && self.pool.install(|| {
                units
                    .par_iter()
                    .any(|unit| self.unit_will_query_ally_defense(unit, &units, &buildings))
            });
        let defense_victims = if needs_ally_defense_index {
            grouped_defense_victims(&self.defense_alerts, &units, &buildings)
        } else {
            Vec::new()
        };
        let alert_grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            defense_victims.iter().enumerate().map(|(index, victim)| {
                (
                    SpatialPartition::global(victim.victim_team.0),
                    index,
                    victim.victim_position,
                )
            }),
        );
        let unit_snapshots = units.as_slice();
        let defense_attacker_grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            defense_victims
                .iter()
                .enumerate()
                .flat_map(|(victim_index, victim)| {
                    let partition = defense_attacker_partition(victim_index);
                    victim
                        .unit_attackers
                        .iter()
                        .copied()
                        .map(move |unit_index| {
                            (partition, unit_index, unit_snapshots[unit_index].position)
                        })
                }),
        );
        snapshot_and_spatial += phase_start.elapsed();

        let phase_start = Instant::now();
        let target_selection = self.select_targets(
            &units,
            &buildings,
            &grid,
            &defense_victims,
            &alert_grid,
            &defense_attacker_grid,
        );
        for (unit, decision) in units.iter_mut().zip(&target_selection.decisions) {
            unit.target = decision.target;
            unit.direct_retaliation_lock = decision.direct_retaliation_lock;
            unit.ally_defense_lock = decision.ally_defense_lock;
        }
        let building_target_selection = self.select_building_targets(&buildings, &units, &grid);
        for (building, target) in buildings
            .iter_mut()
            .zip(&building_target_selection.decisions)
        {
            if building.attack.is_some() {
                building.target = *target;
            }
        }
        let targeting = phase_start.elapsed();

        let mut unit_health: Vec<i32> = units.iter().map(|unit| unit.health).collect();
        let mut building_health: Vec<i32> =
            buildings.iter().map(|building| building.health).collect();
        let mut cooldowns: Vec<u16> = units.iter().map(|unit| unit.cooldown_remaining).collect();
        let mut attack_sequences: Vec<u64> =
            units.iter().map(|unit| unit.attack_sequence).collect();
        let mut building_cooldowns: Vec<u16> = buildings
            .iter()
            .map(|building| building.cooldown_remaining.unwrap_or(0))
            .collect();
        let mut positions: Vec<SimPoint> = units.iter().map(|unit| unit.position).collect();
        let mut navigation_states: Vec<NavigationState> =
            units.iter().map(|unit| unit.navigation).collect();
        let mut attackers_this_tick = vec![None; units.len()];
        let mut next_defense_alerts = Vec::new();

        let chain_lightning_hops = self.resolve_due_chain_lightning_hops(
            &mut units,
            &mut unit_health,
            &positions,
            completed_tick,
        );
        let chain_lightning_updates = chain_lightning_hops.updates;
        let chain_lightning_entities_to_remove = chain_lightning_hops.removals;

        let phase_start = Instant::now();
        let native_carriers = self.resolve_native_carriers(TargetProjectileContext {
            units: &mut units,
            buildings: &mut buildings,
            grid: &grid,
            unit_health: &mut unit_health,
            building_health: &mut building_health,
            positions: &positions,
            attackers_this_tick: &mut attackers_this_tick,
            next_defense_alerts: &mut next_defense_alerts,
            completed_tick,
        });
        let target_projectiles = self.resolve_due_target_projectiles(TargetProjectileContext {
            units: &mut units,
            buildings: &mut buildings,
            grid: &grid,
            unit_health: &mut unit_health,
            building_health: &mut building_health,
            positions: &positions,
            attackers_this_tick: &mut attackers_this_tick,
            next_defense_alerts: &mut next_defense_alerts,
            completed_tick,
        });
        let due_ballistic_projectiles = target_projectiles.due_ballistic_projectiles;
        let mut projectile_entities_to_remove = target_projectiles.projectile_entities_to_remove;
        let bounce_projectile_updates = target_projectiles.bounce_projectile_updates;
        let mut projectile_impacts =
            target_projectiles.projectile_impacts + native_carriers.impacts;
        let mut projectile_effects =
            target_projectiles.projectile_effects + native_carriers.effects;
        let mut projectile_invalidations =
            target_projectiles.projectile_invalidations + native_carriers.invalidations;
        let bounce_jumps = target_projectiles.bounce_jumps;
        let bounce_candidate_checks = target_projectiles.bounce_candidate_checks;
        let mut chain_lightning_launches = target_projectiles.chain_lightning_launches;
        let reflected_projectile_launches = target_projectiles.reflected_projectile_launches;

        let attack_resolution = self.resolve_attacks(AttackResolutionContext {
            units: &mut units,
            buildings: &mut buildings,
            unit_health: &mut unit_health,
            building_health: &mut building_health,
            cooldowns: &mut cooldowns,
            attack_sequences: &mut attack_sequences,
            building_cooldowns: &mut building_cooldowns,
            positions: &positions,
            attackers_this_tick: &mut attackers_this_tick,
            next_defense_alerts: &mut next_defense_alerts,
            completed_tick,
        });
        let attacks_resolved = attack_resolution.attacks_resolved;
        let projectile_launches = attack_resolution.projectile_launches;
        let line_projectile_launches = attack_resolution.line_projectile_launches;
        let ballistic_projectile_launches = attack_resolution.ballistic_projectile_launches;
        let bounce_projectile_launches = attack_resolution.bounce_projectile_launches;
        chain_lightning_launches.extend(attack_resolution.chain_lightning_launches);
        let projectiles_launched = projectile_launches.len()
            + line_projectile_launches.len()
            + reflected_projectile_launches.len()
            + ballistic_projectile_launches.len()
            + bounce_projectile_launches.len()
            + native_carriers.launches
            + attack_resolution.native_barrage_launches;
        let combat = phase_start.elapsed();

        let movement = self.resolve_movement(
            &units,
            &buildings,
            &unit_health,
            &building_health,
            &mut positions,
            &mut navigation_states,
        );

        for (unit, position) in units.iter_mut().zip(&positions) {
            unit.death_position = *position;
        }
        let phase_start = Instant::now();
        self.refresh_snow_protection_at(&mut units, &mut buildings, Some(&positions));
        let ballistic_impacts = self.resolve_due_ballistic_impacts(BallisticImpactContext {
            due_projectiles: due_ballistic_projectiles,
            units: &mut units,
            buildings: &buildings,
            positions: &positions,
            unit_health: &mut unit_health,
            building_health: &mut building_health,
            attackers_this_tick: &mut attackers_this_tick,
            next_defense_alerts: &mut next_defense_alerts,
            completed_tick,
        });
        let (hailstone_impacts, hailstone_effects) = self.resolve_due_hailstone_impacts(
            &mut units,
            &mut buildings,
            &positions,
            &mut unit_health,
            &mut building_health,
        );
        let (proxy_impacts, proxy_effects) =
            self.resolve_due_attack_proxy_impacts(&mut buildings, &mut building_health);
        projectile_impacts += proxy_impacts;
        projectile_effects += proxy_effects;
        if proxy_impacts > 0 {
            death_healing::refresh_support(&mut units, &buildings, None);
        }
        projectile_impacts += hailstone_impacts;
        projectile_effects += hailstone_effects;
        projectile_entities_to_remove.extend(ballistic_impacts.projectile_entities_to_remove);
        projectile_impacts += ballistic_impacts.projectile_impacts;
        projectile_effects += ballistic_impacts.projectile_effects;
        let ballistic_candidate_checks = ballistic_impacts.candidate_checks;
        let burning_oil_zone_launches = ballistic_impacts.burning_oil_zone_launches;
        let (line_impacts, line_effects, line_invalidations) =
            self.resolve_line_projectiles(TargetProjectileContext {
                units: &mut units,
                buildings: &mut buildings,
                grid: &grid,
                positions: &positions,
                unit_health: &mut unit_health,
                building_health: &mut building_health,
                attackers_this_tick: &mut attackers_this_tick,
                next_defense_alerts: &mut next_defense_alerts,
                completed_tick,
            });
        projectile_impacts += line_impacts;
        projectile_effects += line_effects;
        projectile_invalidations += line_invalidations;
        let ballistic_impact = phase_start.elapsed();

        let phase_start = Instant::now();
        self.commit_projectile_world_changes(ProjectileWorldChanges {
            chain_lightning_removals: chain_lightning_entities_to_remove,
            chain_lightning_updates,
            projectile_removals: projectile_entities_to_remove,
            bounce_updates: bounce_projectile_updates,
            burning_oil_zone_launches,
            chain_lightning_launches,
            projectile_launches,
            line_projectile_launches,
            reflected_projectile_launches,
            ballistic_projectile_launches,
            bounce_projectile_launches,
            completed_tick,
        });

        self.resolve_overheat_deaths(
            &mut units,
            &buildings,
            &positions,
            &mut unit_health,
            &mut building_health,
        );
        self.release_spheres(&mut units);
        self.observe_master_parasite_deaths(&units, &unit_health, &positions);
        self.observe_native_parasite_deaths(
            &units,
            &unit_health,
            &positions,
            &buildings,
            &building_health,
        );
        self.emit_death_heal_visuals(&units, &positions);
        let mut deaths = 0;
        let mut corpse_spawns = Vec::new();
        let mut death_spawns = 0;
        // Team contributions are phase-local derived state, never a stale persistent counter.
        // Scan buildings twice per resolution phase rather than once per individual fatality.
        let shrine_support = [
            self.golden_shrine_support(Team(0)),
            self.golden_shrine_support(Team(1)),
        ];
        for (index, unit) in units.iter().enumerate() {
            if unit.removed_without_death {
                if let Some(source) = source_damage_runtime::RetiredDamageSource::capture(unit) {
                    self.retired_damage_sources.insert(unit.id, source);
                }
                self.world.despawn(unit.entity);
                continue;
            }
            if unit_health[index] <= 0 {
                let shrine_state = if self
                    .world
                    .get::<Health>(unit.entity)
                    .is_some_and(|health| health.current > 0)
                {
                    // A newly fatal combat phase has a damage source; an already-dead external
                    // handle has no killer and cannot enter fJ's shrine branch.
                    self.schedule_shrine_revival(
                        unit.entity,
                        unit.owner,
                        unit.team,
                        positions[index],
                        unit_health[index],
                        shrine_support[usize::from(unit.team.0)],
                    )
                } else {
                    self.world
                        .get::<ShrineRevivalState>(unit.entity)
                        .copied()
                        .unwrap_or_default()
                };
                if let Some(profile) = unit.corpse {
                    let resurrection = self
                        .world
                        .get::<ResurrectionProfile>(unit.entity)
                        .map(|profile| profile.0);
                    corpse_spawns.push((
                        unit.id,
                        unit.owner,
                        unit.team,
                        positions[index],
                        profile,
                        resurrection,
                        shrine_state,
                        unit.status.damage_threshold.vertex_rgba,
                    ));
                }
                if let Some(source) = source_damage_runtime::RetiredDamageSource::capture(unit) {
                    self.retired_damage_sources.insert(unit.id, source);
                }
                death_spawns += self.spawn_death_children(unit.entity, positions[index]);
                self.world.despawn(unit.entity);
                deaths += 1;
                continue;
            }

            let live_target = unit.target.filter(|target| {
                target_is_alive(*target, &units, &buildings, &unit_health, &building_health)
            });

            let mut entity = self.world.entity_mut(unit.entity);
            entity
                .get_mut::<Health>()
                .expect("unit health missing")
                .current = unit_health[index];
            entity.get_mut::<Health>().expect("unit health missing").max = unit.health_max;
            entity
                .get_mut::<AttackCooldown>()
                .expect("unit cooldown missing")
                .remaining = cooldowns[index];
            entity
                .get_mut::<AttackSequence>()
                .expect("unit attack sequence missing")
                .0 = attack_sequences[index];
            entity
                .get_mut::<Position>()
                .expect("unit position missing")
                .0 = positions[index];
            *entity
                .get_mut::<NavigationState>()
                .expect("unit navigation state missing") = navigation_states[index];
            let mut target_state = entity
                .get_mut::<TargetState>()
                .expect("unit target missing");
            target_state.current = live_target;
            target_state.direct_retaliation_lock =
                live_target.is_some() && unit.direct_retaliation_lock;
            target_state.ally_defense_lock = live_target.is_some() && unit.ally_defense_lock;
            *entity
                .get_mut::<RetaliationState>()
                .expect("unit retaliation state missing") = match attackers_this_tick[index] {
                Some(attacker) => RetaliationState {
                    attacker: Some(attacker),
                    attacked_tick: Some(completed_tick),
                },
                None => RetaliationState::default(),
            };
            let mut status = unit.status;
            if positions[index] != unit.position {
                status.facing_direction = SimPoint::new(
                    positions[index].x - unit.position.x,
                    positions[index].y - unit.position.y,
                );
            }
            *entity
                .get_mut::<StatusState>()
                .expect("unit status state missing") = status;
            if let Some(mut mana) = entity.get_mut::<ManaState>() {
                mana.current = unit
                    .mana_current
                    .expect("mana-bearing unit snapshot mana missing");
            }
            if unit.spellcasting.is_some() {
                *entity
                    .get_mut::<AutomaticAbilityState>()
                    .expect("spellcasting unit ability state missing") = unit
                    .ability_state
                    .expect("spellcasting unit snapshot ability state missing");
            }
        }

        let corpses_spawned = corpse_spawns.len();
        for (
            source_unit,
            source_owner,
            source_team,
            position,
            profile,
            resurrection,
            shrine_state,
            vertex_rgba,
        ) in corpse_spawns
        {
            let id = self.allocate_id();
            let decay_start_tick = completed_tick
                .checked_add(u64::from(profile.decay_start_ticks))
                .expect("corpse decay-start tick overflow");
            let expires_tick = profile.lifetime_ticks.map(|lifetime_ticks| {
                completed_tick
                    .checked_add(u64::from(lifetime_ticks))
                    .expect("corpse expiry tick overflow")
            });
            self.world.spawn((
                id,
                Position(position),
                Corpse {
                    vertex_rgba,
                    source_unit,
                    source_owner,
                    source_team,
                    definition: profile.definition,
                    created_tick: completed_tick,
                    decay_start_tick,
                    expires_tick,
                    resurrection,
                    shrine_state,
                },
            ));
        }

        let mut building_deaths = Vec::new();
        for (index, building) in buildings.iter().enumerate() {
            if building_health[index] <= 0 {
                building_deaths.push(building.entity);
                deaths += 1;
            } else {
                let live_target = building.target.filter(|target| {
                    target_is_alive(*target, &units, &buildings, &unit_health, &building_health)
                });
                let mut entity = self.world.entity_mut(building.entity);
                entity
                    .get_mut::<Health>()
                    .expect("building health missing")
                    .current = building_health[index];
                if building.attack.is_some() {
                    entity
                        .get_mut::<AttackCooldown>()
                        .expect("attack building cooldown missing")
                        .remaining = building_cooldowns[index];
                    let mut target_state = entity
                        .get_mut::<TargetState>()
                        .expect("attack building target missing");
                    target_state.current = live_target;
                    target_state.direct_retaliation_lock = false;
                    target_state.ally_defense_lock = false;
                }
                if let Some(status) = building.status {
                    entity.insert(status);
                }
                if building.spellcasting.is_some() {
                    entity
                        .get_mut::<ManaState>()
                        .expect("spellcasting building mana missing")
                        .current = building
                        .mana_current
                        .expect("spellcasting building snapshot mana missing");
                    *entity
                        .get_mut::<AutomaticAbilityState>()
                        .expect("spellcasting building ability state missing") = building
                        .ability_state
                        .expect("spellcasting building snapshot ability state missing");
                }
            }
        }
        if !building_deaths.is_empty() {
            for entity in building_deaths {
                let id = *self.world.get::<SimId>(entity).expect("dead building id");
                self.stop_native_carrier(id);
                self.release_building_legendary_points(entity);
                self.world.despawn(entity);
            }
            self.topology_dirty = true;
        }

        self.prune_retired_air_bonus_sources();
        self.refresh_mana_improvement_links();
        self.evaluate_objective_outcome(completed_tick);
        if self.lifecycle == MatchLifecycle::Running {
            self.advance_economy_income();
        }
        self.defense_alerts = next_defense_alerts;
        let projectiles_alive = self.projectile_count();
        let corpses_alive = self.corpse_count();
        self.reveal_fogged_attack_sources(&units, &buildings);
        self.next_tick = self
            .next_tick
            .checked_add(1)
            .expect("tick counter exhausted");
        self.refresh_vision();
        let structural_commit = phase_start.elapsed();

        let phase_start = Instant::now();
        let checksum = canonical_checksum(
            &self.world,
            CanonicalMatchState {
                shrine_death_generation: self.shrine_death_generation,
                retired_damage_sources: &self.retired_damage_sources,
                debug_buildings_invulnerable: self.debug_buildings_invulnerable,
                fog: self.fog.as_ref(),
                next_tick: self.next_tick,
                next_id: self.next_id,
                next_visual_id: self.next_visual_id,
                configuration_identity: self.configuration_identity,
                defense_alerts: &self.defense_alerts,
                players: &self.players,
                lifecycle: self.lifecycle,
                team_objectives: self.team_objectives,
                gjallarhorn_constructed_count: self.gjallarhorn_constructed_count,
                round_index: self.round_index,
                round_start_tick: self.round_start_tick,
                round_wins: self.round_wins,
                player_strikes: &self.player_strikes,
                pending_rescue_strikes: &self.pending_rescue_strikes,
                team_rescue_ready_ticks: self.team_rescue_ready_ticks,
                review_until_tick: self.review_until_tick,
                last_round_scoreboard: self.last_round_scoreboard.as_ref(),
            },
        );
        let checksum_time = phase_start.elapsed();
        let timings = TickTimings {
            topology,
            timers,
            production,
            snapshot_and_spatial,
            abilities,
            targeting,
            combat,
            movement_intent: movement.intent,
            crowd_separation: movement.crowd_separation,
            hard_collision: movement.hard_collision,
            collision_fallback_search: movement.collision_fallback_search,
            crowd_and_collision: movement.crowd_and_collision,
            ballistic_impact,
            structural_commit,
            checksum: checksum_time,
            total: tick_start.elapsed(),
        };

        TickResult {
            completed_tick,
            units_alive: units.len() - unit_health.iter().filter(|health| **health <= 0).count()
                + death_spawns,
            buildings_alive: buildings.len()
                - building_health
                    .iter()
                    .filter(|health| **health <= 0)
                    .count(),
            corpses_alive,
            attacks_resolved,
            deaths,
            corpses_spawned,
            corpses_expired,
            units_spawned: units_spawned + mirror_spawns + death_spawns,
            spawn_failures,
            topology_rebuilds: usize::from(topology_rebuilt),
            pursuit_steps: movement.pursuit_steps,
            navigation_route_steps: movement.navigation_route_steps,
            movement_intents: movement.movement_intents,
            movement_blocked: movement.movement_blocked,
            collision_fallback_searches: movement.collision_fallback_searches,
            collision_fallback_candidate_checks: movement.collision_fallback_candidate_checks,
            collision_fallback_max_ring: movement.collision_fallback_max_ring,
            objective_move_intents: movement.objective_move_intents,
            a_star_fallbacks: movement.a_star_fallbacks,
            a_star_cache_hits: movement.a_star_cache_hits,
            a_star_expanded_nodes: movement.a_star_expanded_nodes,
            projectiles_alive,
            projectiles_launched,
            projectile_impacts,
            projectile_effects,
            projectile_invalidations,
            ballistic_candidate_checks,
            bounce_jumps,
            bounce_candidate_checks,
            ability_evaluations: ability_metrics.evaluations,
            ability_casts: ability_metrics.casts,
            ability_candidate_checks: ability_metrics.candidate_checks,
            ability_effects: ability_metrics.effects,
            stunned_units: units
                .iter()
                .enumerate()
                .filter(|(index, unit)| {
                    unit_health[*index] > 0 && completed_tick < unit.status.stunned_until_tick
                })
                .count(),
            timed_movement_modifiers: units
                .iter()
                .enumerate()
                .filter(|(index, _)| unit_health[*index] > 0)
                .map(|(_, unit)| usize::from(unit.status.movement_modifier_count))
                .sum(),
            retained_targets: target_selection.retained_targets
                + building_target_selection.retained_targets,
            target_changes: target_selection.target_changes
                + building_target_selection.target_changes,
            ally_defense_queries: target_selection.ally_defense_queries,
            ally_defense_victim_candidates: target_selection.ally_defense_victim_candidates,
            ally_defense_attacker_candidates: target_selection.ally_defense_attacker_candidates,
            checksum,
            timings,
        }
    }

    fn frozen_tick_result(&self) -> TickResult {
        TickResult {
            completed_tick: self.next_tick.saturating_sub(1),
            units_alive: self.unit_count(),
            buildings_alive: self.building_count(),
            corpses_alive: self.corpse_count(),
            projectiles_alive: self.projectile_count(),
            checksum: self.checksum(),
            ..TickResult::default()
        }
    }

    #[must_use]
    pub fn checksum(&self) -> u64 {
        canonical_checksum(
            &self.world,
            CanonicalMatchState {
                shrine_death_generation: self.shrine_death_generation,
                retired_damage_sources: &self.retired_damage_sources,
                debug_buildings_invulnerable: self.debug_buildings_invulnerable,
                fog: self.fog.as_ref(),
                next_tick: self.next_tick,
                next_id: self.next_id,
                next_visual_id: self.next_visual_id,
                configuration_identity: self.configuration_identity,
                defense_alerts: &self.defense_alerts,
                players: &self.players,
                lifecycle: self.lifecycle,
                team_objectives: self.team_objectives,
                gjallarhorn_constructed_count: self.gjallarhorn_constructed_count,
                round_index: self.round_index,
                round_start_tick: self.round_start_tick,
                round_wins: self.round_wins,
                player_strikes: &self.player_strikes,
                pending_rescue_strikes: &self.pending_rescue_strikes,
                team_rescue_ready_ticks: self.team_rescue_ready_ticks,
                review_until_tick: self.review_until_tick,
                last_round_scoreboard: self.last_round_scoreboard.as_ref(),
            },
        )
    }

    /// Clears presentation-only events from the most recently simulated tick. This does not alter
    /// authoritative state and is used after restore/catch-up so historical cosmetic effects are
    /// not emitted as if they had just happened live.
    pub fn clear_presentation_events(&mut self) {
        self.last_attacks.clear();
        self.last_ability_casts.clear();
        self.last_death_heals.clear();
        self.last_shrine_revivals.clear();
        self.last_building_spell_visuals.clear();
        self.last_chain_lightnings.clear();
        self.last_rescue_strike_visuals.clear();
    }

    #[must_use]
    pub fn attacks_last_tick(&self) -> &[AttackEvent] {
        &self.last_attacks
    }

    #[must_use]
    pub fn building_spell_visuals_last_tick(&self) -> &[BuildingSpellVisualEvent] {
        &self.last_building_spell_visuals
    }

    #[must_use]
    pub fn ability_casts_last_tick(&self) -> &[AbilityCastEvent] {
        &self.last_ability_casts
    }

    #[must_use]
    pub fn chain_lightnings_last_tick(&self) -> &[ChainLightningEvent] {
        &self.last_chain_lightnings
    }

    #[must_use]
    pub fn unit_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| {
                entity.get::<Position>().is_some()
                    && entity.get::<Health>().is_some()
                    && entity.get::<BuildingFootprint>().is_none()
            })
            .count()
    }

    #[must_use]
    pub fn corpse_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| entity.get::<Corpse>().is_some())
            .count()
    }

    #[must_use]
    pub fn building_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| entity.get::<BuildingFootprint>().is_some())
            .count()
    }

    #[must_use]
    pub fn projectile_count(&self) -> usize {
        self.world
            .iter_entities()
            .map(|entity| {
                let ordinary = matches!(
                    entity.get::<crate::components::NativeAction>(),
                    Some(
                        crate::components::NativeAction::AcidBomb(_)
                            | crate::components::NativeAction::Bolt(_)
                            | crate::components::NativeAction::ConeWave(_)
                            | crate::components::NativeAction::Ensnare(_)
                            | crate::components::NativeAction::BuildingAttackProxy(_)
                            | crate::components::NativeAction::Hailstone(_)
                    )
                ) || entity.get::<GuaranteedHitProjectile>().is_some()
                    || entity.get::<ReflectedProjectile>().is_some()
                    || entity.get::<BallisticProjectile>().is_some()
                    || entity.get::<LineProjectile>().is_some()
                    || entity.get::<BounceProjectile>().is_some()
                    || matches!(
                        entity.get::<crate::native_carriers::NativeCarrierState>(),
                        Some(crate::native_carriers::NativeCarrierState::Bolt(_))
                    );
                usize::from(ordinary)
                    + entity
                        .get::<crate::components::NativeAction>()
                        .map_or(0, |action| match action {
                            crate::components::NativeAction::MasterProjectiles(state) => {
                                state.lightning.len() + state.frost.len() + state.minions.len()
                            }
                            _ => 0,
                        })
                    + entity
                        .get::<WorldFreezerState>()
                        .map_or(0, |state| state.orbs.len())
            })
            .sum()
    }

    #[must_use]
    pub fn corpses(&self) -> Vec<CorpseView> {
        let mut corpses: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(corpse_view_from_entity)
            .collect();
        corpses.sort_unstable_by_key(|corpse| corpse.id);
        corpses
    }

    #[must_use]
    pub fn burning_oil_zones(&self) -> Vec<BurningOilView> {
        if !self
            .world
            .iter_entities()
            .any(|entity| entity.contains::<BurningOilZone>())
        {
            return Vec::new();
        }
        let units = self.units();
        let buildings = self.buildings();
        let mut zones: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let zone = entity.get::<BurningOilZone>()?;
                let radius_sq = square_i32(zone.profile.radius);
                let mut recipients: Vec<_> = units
                    .iter()
                    .filter(|unit| {
                        zone.profile.target_ground_units
                            && unit.health > 0
                            && unit.team != zone.source_team
                            && !unit.classifications.spell_immune
                            && !unit.classifications.invulnerable
                            && (unit.movement_class == MovementClass::Ground
                                || unit
                                    .status
                                    .ensnare
                                    .is_some_and(|net| self.tick() < net.expires_tick))
                            && zone.center.distance_sq(unit.position) <= radius_sq
                    })
                    .map(|unit| unit.id)
                    .collect();
                recipients.extend(
                    buildings
                        .iter()
                        .filter(|building| {
                            zone.profile.target_buildings
                                && !self.debug_buildings_invulnerable
                                && building.health > 0
                                && building.team != zone.source_team
                                && !building.classifications.spell_immune
                                && !building.classifications.invulnerable
                                && point_to_footprint_distance_sq(
                                    zone.center,
                                    building.footprint,
                                    self.config.navigation_cell_size,
                                ) <= radius_sq
                        })
                        .map(|building| building.id),
                );
                recipients.sort_unstable();
                Some(BurningOilView {
                    id: *entity.get::<SimId>()?,
                    ability: zone.profile.ability,
                    center: zone.center,
                    recipients,
                })
            })
            .collect();
        zones.sort_unstable_by_key(|zone| zone.id);
        zones
    }

    #[must_use]
    pub fn corpse(&self, id: SimId) -> Option<CorpseView> {
        self.world
            .iter_entities()
            .filter_map(corpse_view_from_entity)
            .find(|corpse| corpse.id == id)
    }

    #[must_use]
    pub fn projectiles(&self) -> Vec<ProjectileView> {
        let mut projectiles: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(projectile_view_from_entity)
            .collect();
        for entity in self.world.iter_entities() {
            if let Some(state) = entity.get::<WorldFreezerState>() {
                projectiles.extend(state.orbs.iter().map(|orb| {
                    ProjectileView {
                        id: orb.id,
                        source: orb.source,
                        ability: Some(state.profile.parent),
                        launch_position: orb.position(),
                        launch_tick: self.next_tick.saturating_sub(1),
                        impact_tick: self.next_tick,
                        kind: ProjectileViewKind::NativeMover {
                            rawcode: state.profile.dummy_rawcode,
                            owner: orb.owner,
                            facing_degrees: orb.facing_degrees,
                            height_world_units:
                                crate::building_mechanics::world_freezer_visual_timing_for_version(
                                    state.profile.map_version,
                                )
                                .orb_height_world_units,
                        },
                    }
                }));
            }
        }
        self.append_master_projectile_views(&mut projectiles);
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    #[must_use]
    pub fn builders(&self) -> Vec<BuilderView> {
        let mut builders: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(builder_view_from_entity)
            .collect();
        builders.sort_unstable_by_key(|builder| builder.id);
        builders
    }

    #[must_use]
    pub fn builder(&self, id: SimId) -> Option<BuilderView> {
        self.world
            .iter_entities()
            .filter_map(builder_view_from_entity)
            .find(|builder| builder.id == id)
    }

    #[must_use]
    pub fn builder_for_player(&self, player: PlayerId) -> Option<BuilderView> {
        self.world
            .iter_entities()
            .filter_map(builder_view_from_entity)
            .find(|builder| builder.owner == player)
    }

    #[must_use]
    pub fn builder_for_team(&self, team: Team) -> Option<BuilderView> {
        let player = self.unique_player_for_team(team)?;
        self.builder_for_player(player)
    }

    #[must_use]
    pub fn units(&self) -> Vec<UnitView> {
        let default_collision_radius = self.default_collision_radius();
        let mut units: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                unit_view_from_entity(entity, default_collision_radius, self.next_tick)
            })
            .collect();
        units.sort_unstable_by_key(|unit| unit.id);
        units
    }

    #[must_use]
    pub fn buildings(&self) -> Vec<BuildingView> {
        let mut buildings: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(building_view_from_entity)
            .collect();
        buildings.sort_unstable_by_key(|building| building.id);
        buildings
    }

    #[must_use]
    pub fn unit(&self, id: SimId) -> Option<UnitView> {
        let default_collision_radius = self.default_collision_radius();
        self.world
            .iter_entities()
            .filter_map(|entity| {
                unit_view_from_entity(entity, default_collision_radius, self.next_tick)
            })
            .find(|unit| unit.id == id)
    }

    fn default_collision_radius(&self) -> i32 {
        self.config.unit_separation_distance / 2
    }

    #[must_use]
    pub fn building(&self, id: SimId) -> Option<BuildingView> {
        self.world
            .iter_entities()
            .filter_map(building_view_from_entity)
            .find(|building| building.id == id)
    }

    fn allocate_id(&mut self) -> SimId {
        let id = SimId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|next| *next < VISUAL_ID_BIT)
            .expect("gameplay SimId space exhausted");
        id
    }

    fn spawn_unit_unchecked(
        &mut self,
        owner: Option<PlayerId>,
        unit: UnitSpawn,
        properties: UnitGameplayProperties,
        spellcasting: Option<SpellcastingProfile>,
    ) -> SimId {
        self.spawn_resolved_unit_unchecked(
            owner,
            unit,
            ResolvedUnitDefinition {
                template: crate::components::UnitTemplate {
                    health: unit.health,
                    attack: unit.attack,
                    movement: unit.movement,
                },
                properties,
                spellcasting,
                additional_abilities: None,
            },
        )
    }

    fn spawn_resolved_unit_unchecked(
        &mut self,
        owner: Option<PlayerId>,
        unit: UnitSpawn,
        definition: ResolvedUnitDefinition,
    ) -> SimId {
        self.spawn_resolved_unit_with_provenance(owner, unit, definition, None)
    }

    fn spawn_resolved_unit_with_provenance(
        &mut self,
        owner: Option<PlayerId>,
        unit: UnitSpawn,
        definition: ResolvedUnitDefinition,
        spawn_building: Option<SimId>,
    ) -> SimId {
        let properties = definition.properties;
        let spellcasting = definition.spellcasting;
        automatic_abilities::validate_additional_automatic_definitions(
            spellcasting,
            definition.additional_abilities,
        )
        .expect("validated resolved automatic ability definitions");
        let additional = definition
            .additional_abilities
            .filter(|definitions| definitions.iter().next().is_some())
            .map(|definitions| {
                AdditionalAutomaticAbilities::from_definitions(definitions, self.next_tick)
            });
        let id = self.allocate_id();
        let mut entity = self.world.spawn((
            id,
            unit.team,
            Position(unit.position),
            Health {
                current: unit.health,
                max: unit.health,
            },
            unit.attack,
            properties.attack_targets,
            properties.movement_class,
            AttackCooldown::default(),
            AttackSequence::default(),
            TargetState::default(),
            RetaliationState::default(),
            StatusState {
                spawn_building,
                facing_direction: SimPoint::new(if unit.team.0 == 0 { 1 } else { -1 }, 0),
                summon_order_refresh_tick: lava_spawns::initial_state(properties.passive_effects)
                    .map(|state| self.next_tick + u64::from(state.profile.order_refresh_ticks)),
                lava_spawn: lava_spawns::initial_state(properties.passive_effects),
                damage_charge: damage_charge::initial_state(properties.passive_effects),
                mirror_image: mirror_images::initial_state(properties.passive_effects),
                ..Default::default()
            },
            NavigationState::default(),
            unit.movement,
            SpawnTick(self.next_tick),
        ));
        entity.insert((ResurrectionProfile(definition), properties.action_timing));
        if let Some(additional) = additional {
            entity.insert(additional);
        }
        if let Some(owner) = owner {
            entity.insert(Owner(owner));
        }
        entity.insert(HealthRegeneration {
            per_second_per_10k: properties.health_regen_per_second_per_10k,
            remainder_per_10k_hz: 0,
        });
        if let Some(content) = properties.content {
            entity.insert(content);
        }
        if let Some(corpse) = properties.corpse {
            entity.insert(CorpseProducer(corpse));
        }
        if let Some(collision_radius) = properties.collision_radius {
            entity.insert(collision_radius);
        }
        if properties.mechanical {
            entity.insert(MechanicalUnit);
        }
        if properties.classifications != UnitClassifications::default() {
            entity.insert(properties.classifications);
        }
        if let Some(build_time_ticks) = properties.build_time_ticks {
            entity.insert(BuildTimeTicks(build_time_ticks));
        }
        if let Some(repair_time_ticks) = properties.repair_time_ticks {
            entity.insert(RepairTimeTicks(repair_time_ticks));
        }
        entity.insert((
            properties.damage_type,
            properties.armor,
            properties.passive_effects,
        ));
        if let Some(secondary_attack) = properties.secondary_attack {
            entity.insert(secondary_attack);
        }
        if let Some(spellcasting) = spellcasting {
            entity.insert((
                spellcasting,
                ManaState {
                    current: spellcasting.mana.starting,
                    regen_remainder_per_10k: 0,
                },
                AutomaticAbilityState {
                    ready_tick: self.next_tick,
                    cast_sequence: 0,
                    autocast_enabled: true,
                    manual_cast_requested: false,
                },
            ));
        } else if let Some(mana) = properties
            .content
            .and_then(|c| crate::content::native_mana_profile_for_version(c.map_version, c.rawcode))
        {
            entity.insert(ManaState {
                current: mana.starting,
                regen_remainder_per_10k: 0,
            });
        }
        id
    }

    fn refresh_topology_if_dirty(&mut self) -> bool {
        if !self.topology_dirty {
            return false;
        }
        let mut query = self.world.query::<&BuildingFootprint>();
        let building_footprints: Vec<_> = query.iter(&self.world).copied().collect();
        let blockers = self
            .config
            .static_blockers
            .iter()
            .copied()
            .chain(building_footprints);
        self.topology = TopologyGrid::build(
            self.config.navigation_cell_size,
            self.config.navigation_min,
            self.config.navigation_max,
            blockers,
            self.config.team_objective,
        );
        self.topology_dirty = false;
        self.pursuit_cache.clear();
        self.radius_objective_fields.clear();
        true
    }

    fn apply_passive_auras(&self, units: &mut [UnitSnapshot], buildings: &[BuildingSnapshot]) {
        let mut sources = units
            .iter()
            .filter(|unit| unit.health > 0)
            .flat_map(|unit| {
                unit.passive_effects.iter().filter_map(move |effect| {
                    let PassiveUnitEffect::Aura(profile) = effect else {
                        return None;
                    };
                    if (profile.melee_damage_return_per_10k > 0
                        || profile.melee_life_steal_per_10k > 0)
                        && (unit.abilities_disabled || unit.classifications.invisible)
                    {
                        return None;
                    }
                    if Self::passive_aura_suspended(&profile, &unit.status, self.next_tick) {
                        return None;
                    }
                    Some((unit.id, unit.team, unit.position, profile))
                })
            })
            .chain(
                buildings
                    .iter()
                    .filter(|building| building.health > 0)
                    .flat_map(|building| {
                        building.passive_effects.iter().filter_map(move |effect| {
                            let PassiveUnitEffect::Aura(profile) = effect else {
                                return None;
                            };
                            if building.status.is_some_and(|status| {
                                Self::passive_aura_suspended(&profile, &status, self.next_tick)
                            }) {
                                return None;
                            }
                            Some((
                                building.id,
                                building.team,
                                footprint_center_point(
                                    building.footprint,
                                    self.config.navigation_cell_size,
                                ),
                                profile,
                            ))
                        })
                    }),
            )
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|source| source.0);
        for (source_id, team, position, profile) in sources {
            for target in units.iter_mut() {
                if target.health <= 0
                    || target.classifications.locust
                    || target.team != team
                    || (target.attack.delivery == AttackDelivery::Melee && !profile.affects_melee)
                    || (target.attack.delivery != AttackDelivery::Melee && !profile.affects_ranged)
                    || ((profile.self_only || profile.radius == 0) && target.id != source_id)
                    || position.distance_sq(target.position) > square_i32(profile.radius)
                {
                    continue;
                }
                apply_timed_armor_modifier(
                    &mut target.status,
                    TimedArmorModifier {
                        ethereal: None,
                        melee_life_steal_per_10k: profile.melee_life_steal_per_10k,
                        attack_miss_chance_per_10k: 0,
                        melee_damage_return_per_10k: profile.melee_damage_return_per_10k,
                        undispellable: false,
                        aura_membership: true,
                        id: ModifierId(profile.ability.0),
                        native_buff: None,
                        armor_bonus_per_100: profile.armor_bonus_per_100,
                        regeneration_per_second_per_10k: 0,
                        mana_regeneration_per_second_per_10k: profile
                            .mana_regeneration_per_second_per_10k,
                        damage_bonus_per_10k: profile.damage_bonus_per_10k,
                        damage_taken_bonus_per_10k: 0,
                        expires_tick: self.next_tick + u64::from(profile.linger_ticks.max(1)),
                        reactive_slow_duration_ticks: 0,
                        reactive_movement_percent_delta: 0,
                        reactive_attack_speed_percent_delta: 0,
                        revealed_to: None,
                    },
                );
            }
        }
    }

    fn passive_aura_suspended(
        profile: &crate::components::AuraEffectProfile,
        status: &StatusState,
        tick: u64,
    ) -> bool {
        profile.suspend_during_spell_cooldown
            && status.ability_retreat_end_tick > 0
            && tick
                < status
                    .ability_retreat_end_tick
                    .checked_add(
                        u64::try_from(66 * CASTLE_FIGHT_SIMULATION_HZ / 10)
                            .expect("simulation Hz must be positive"),
                    )
                    .expect("Warlock aura recovery tick overflow")
    }

    // A zero-duration intrinsic mana aura must be sampled before regeneration, rather than
    // relying on a modifier from the previous combat phase to survive an extra tick.
    fn refresh_native_mana_auras_for_timers(&mut self) {
        let tick = self.next_tick;
        let cell_size = self.config.navigation_cell_size;
        let mut query = self.world.query_filtered::<(
            &SimId,
            &Team,
            Option<&Position>,
            Option<&BuildingFootprint>,
            &Health,
            &PassiveUnitEffects,
            Option<&StatusState>,
        ), bevy_ecs::query::Without<BuildingConstruction>>();
        let mut sources: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, health, _, _)| health.current > 0)
            .flat_map(|(id, team, position, footprint, _, effects, status)| {
                let origin = position
                    .map(|p| p.0)
                    .or_else(|| footprint.map(|f| footprint_center_point(*f, cell_size)));
                effects.iter().filter_map(move |effect| {
                    let PassiveUnitEffect::Aura(profile) = effect else {
                        return None;
                    };
                    if profile.mana_regeneration_per_second_per_10k == 0
                        || status.is_some_and(|status| {
                            Self::passive_aura_suspended(&profile, status, tick)
                        })
                    {
                        return None;
                    }
                    Some((*id, *team, origin?, profile))
                })
            })
            .collect();
        sources.sort_unstable_by_key(|source| source.0);
        if sources.is_empty() {
            return;
        }
        let mut targets = self.world.query::<(
            &SimId,
            &Team,
            &Position,
            &Health,
            &AttackProfile,
            &mut StatusState,
            &ManaState,
            Option<&UnitClassifications>,
        )>();
        for (id, team, position, health, attack, mut status, _, flags) in
            targets.iter_mut(&mut self.world)
        {
            purge_expired_status_modifiers(&mut status, tick);
            for (source_id, source_team, source_position, profile) in &sources {
                if health.current <= 0
                    || flags.is_some_and(|flags| flags.locust)
                    || team != source_team
                    || (profile.radius == 0 && id != source_id)
                    || position.0.distance_sq(*source_position) > square_i32(profile.radius)
                    || (attack.delivery == AttackDelivery::Melee && !profile.affects_melee)
                    || (attack.delivery != AttackDelivery::Melee && !profile.affects_ranged)
                {
                    continue;
                }
                apply_timed_armor_modifier(
                    &mut status,
                    TimedArmorModifier {
                        undispellable: false,
                        aura_membership: true,
                        id: ModifierId(profile.ability.0),
                        melee_damage_return_per_10k: profile.melee_damage_return_per_10k,
                        armor_bonus_per_100: profile.armor_bonus_per_100,
                        damage_bonus_per_10k: profile.damage_bonus_per_10k,
                        mana_regeneration_per_second_per_10k: profile
                            .mana_regeneration_per_second_per_10k,
                        expires_tick: tick + u64::from(profile.linger_ticks.max(1)),
                        ..TimedArmorModifier::default()
                    },
                );
            }
        }
    }

    fn advance_cooldowns(&mut self) {
        let mut query = self.world.query::<&mut AttackCooldown>();
        for mut cooldown in query.iter_mut(&mut self.world) {
            cooldown.remaining = cooldown.remaining.saturating_sub(1);
        }

        let mut health_regen_query =
            self.world
                .query::<(&mut Health, &mut HealthRegeneration, Option<&StatusState>)>();
        for (mut health, mut regeneration, status) in health_regen_query.iter_mut(&mut self.world) {
            // Native regeneration never raises a dead handle, including shrine buildings.
            if health.current <= 0 {
                continue;
            }
            let bonus_per_second_per_10k = status.map_or(0, |status| {
                status.armor_modifiers[..usize::from(status.armor_modifier_count)]
                    .iter()
                    .filter(|modifier| self.next_tick < modifier.expires_tick)
                    .map(|modifier| modifier.regeneration_per_second_per_10k)
                    .sum::<u32>()
            });
            let rate = regeneration
                .per_second_per_10k
                .saturating_add(bonus_per_second_per_10k);
            if health.current >= health.max || rate == 0 {
                health.current = health.current.min(health.max);
                regeneration.remainder_per_10k_hz = 0;
                continue;
            }
            let denominator = 10_000_u64
                .checked_mul(
                    u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz is positive"),
                )
                .expect("health regeneration fixed-point denominator overflow");
            let accumulated = u64::from(regeneration.remainder_per_10k_hz) + u64::from(rate);
            let whole_health = accumulated / denominator;
            let remainder = accumulated % denominator;
            let regenerated = i64::from(health.current)
                + i64::try_from(whole_health).expect("health regeneration exceeds i64");
            if regenerated >= i64::from(health.max) {
                health.current = health.max;
                regeneration.remainder_per_10k_hz = 0;
            } else {
                health.current = i32::try_from(regenerated)
                    .expect("health regeneration overflowed validated bounds");
                regeneration.remainder_per_10k_hz =
                    u32::try_from(remainder).expect("health regeneration remainder fits u32");
            }
        }

        let mut mana_query = self.world.query::<(
            Option<&SpellcastingProfile>,
            &mut ManaState,
            Option<&StatusState>,
            Option<&ContentIdentity>,
        )>();
        for (casting, mut mana, status, content) in mana_query.iter_mut(&mut self.world) {
            let profile = casting.map(|p| p.mana).or_else(|| {
                content.and_then(|c| {
                    crate::content::native_mana_profile_for_version(c.map_version, c.rawcode)
                })
            });
            let Some(profile) = profile else {
                continue;
            };
            if mana.current >= profile.maximum {
                mana.current = profile.maximum;
                mana.regen_remainder_per_10k = 0;
                continue;
            }
            let aura_per_tick = status.map_or(0, |status| {
                let permanent = crate::components::per_second_increment(
                    status.native_grants.mana_regeneration_per_second_per_10k(),
                    self.next_tick,
                    CASTLE_FIGHT_SIMULATION_HZ as u32,
                );
                permanent
                    + status.armor_modifiers[..usize::from(status.armor_modifier_count)]
                        .iter()
                        .filter(|modifier| self.next_tick < modifier.expires_tick)
                        .map(|modifier| {
                            crate::components::per_second_increment(
                                modifier.mana_regeneration_per_second_per_10k,
                                self.next_tick,
                                CASTLE_FIGHT_SIMULATION_HZ as u32,
                            )
                        })
                        .sum::<u32>()
            });
            let accumulated = u64::from(mana.regen_remainder_per_10k)
                + u64::from(
                    profile.regeneration_at_tick(self.next_tick, CASTLE_FIGHT_SIMULATION_HZ as u32),
                )
                + u64::from(aura_per_tick);
            let whole_mana = accumulated / 10_000;
            let remainder = accumulated % 10_000;
            let regenerated = i64::from(mana.current)
                + i64::try_from(whole_mana).expect("mana regeneration exceeds i64");
            if regenerated >= i64::from(profile.maximum) {
                mana.current = profile.maximum;
                mana.regen_remainder_per_10k = 0;
            } else {
                mana.current = i32::try_from(regenerated)
                    .expect("mana regeneration overflowed validated bounds");
                mana.regen_remainder_per_10k =
                    u16::try_from(remainder).expect("mana remainder fits 1/10,000 scale");
            }
        }

        let mut charge_query = self.world.query::<(&Health, &mut StatusState)>();
        for (health, mut status) in charge_query.iter_mut(&mut self.world) {
            if health.current <= 0 {
                continue;
            }
            let bonus = status.armor_modifiers[..usize::from(status.armor_modifier_count)]
                .iter()
                .filter(|modifier| self.next_tick < modifier.expires_tick)
                .map(|modifier| modifier.mana_regeneration_per_second_per_10k)
                .sum::<u32>();
            if let Some(charge) = status.damage_charge.as_mut() {
                damage_charge::advance_regeneration(charge, bonus);
            }
        }
        let mut status_query = self.world.query::<&mut StatusState>();
        for mut status in status_query.iter_mut(&mut self.world) {
            purge_expired_status_modifiers(&mut status, self.next_tick);
        }
    }

    fn expire_corpses(&mut self) -> usize {
        let mut query = self.world.query::<(Entity, &SimId, &Corpse)>();
        let mut expired: Vec<_> = query
            .iter(&self.world)
            .filter_map(|(entity, id, corpse)| {
                corpse
                    .expires_tick
                    .is_some_and(|expires_tick| self.next_tick >= expires_tick)
                    .then_some((*id, entity))
            })
            .collect();
        expired.sort_unstable_by_key(|(id, _)| *id);
        for (_, entity) in &expired {
            let original = self
                .world
                .get::<Corpse>(*entity)
                .expect("expired corpse")
                .source_unit;
            self.retired_damage_sources.remove(&original);
            self.world.despawn(*entity);
        }
        expired.len()
    }

    fn advance_production(&mut self) -> (usize, usize) {
        let mut query = self.world.query::<(
            Entity,
            &SimId,
            &Team,
            &BuildingFootprint,
            &ProductionProfile,
            &ProductionState,
            Option<&ProductionContentIdentity>,
            Option<&ProductionCorpseProfile>,
            Option<&ProductionCollisionRadius>,
            &ProductionMovementClass,
            &ProductionAttackTargets,
            &ProductionDamageType,
            &ProductionArmorProfile,
            &ProductionPassiveEffects,
            Option<&ProductionSpellcastingProfile>,
        )>();
        let mut attempts: Vec<_> = query
            .iter(&self.world)
            .filter(|(entity, _, _, _, _, state, _, _, _, _, _, _, _, _, _)| {
                !self
                    .world
                    .get::<StatusState>(*entity)
                    .is_some_and(|s| self.next_tick < s.frozen_until_tick)
                    && state.queued != 0
                    && state.next_spawn_tick <= self.next_tick
            })
            .map(
                |(
                    entity,
                    id,
                    team,
                    footprint,
                    profile,
                    state,
                    content,
                    corpse,
                    collision_radius,
                    movement_class,
                    attack_targets,
                    damage_type,
                    armor,
                    passive_effects,
                    spellcasting,
                )| {
                    let entity_ref = self.world.entity(entity);
                    let owner = entity_ref
                        .get::<Owner>()
                        .copied()
                        .expect("production building missing owner")
                        .0;
                    let repair_metadata = entity_ref
                        .get::<ProductionUnitRepairMetadata>()
                        .copied()
                        .expect("production building missing repair metadata");
                    let health_regeneration = entity_ref
                        .get::<ProductionHealthRegeneration>()
                        .copied()
                        .expect("production building missing health regeneration metadata");
                    ProductionAttempt {
                        entity,
                        id: *id,
                        owner,
                        team: *team,
                        footprint: *footprint,
                        profile: *profile,
                        content: content.map(|content| content.0),
                        corpse: corpse.map(|corpse| corpse.0),
                        collision_radius: collision_radius.map(|radius| radius.0),
                        movement_class: movement_class.0,
                        mechanical: repair_metadata.mechanical,
                        classifications: entity_ref
                            .get::<ProductionUnitClassifications>()
                            .map_or(UnitClassifications::default(), |flags| flags.0),
                        build_time_ticks: repair_metadata.build_time_ticks,
                        repair_time_ticks: repair_metadata.repair_time_ticks,
                        attack_targets: attack_targets.0,
                        action_timing: entity_ref
                            .get::<ProductionActionTiming>()
                            .expect("production action timing missing")
                            .0,
                        secondary_attack: entity_ref
                            .get::<ProductionSecondaryAttack>()
                            .map(|profile| profile.0),
                        health_regen_per_second_per_10k: health_regeneration.0,
                        damage_type: damage_type.0,
                        armor: armor.0,
                        passive_effects: passive_effects.0,
                        spellcasting: spellcasting.map(|profile| profile.0),
                        additional_abilities: entity_ref
                            .get::<ProductionAdditionalAutomaticAbilities>()
                            .map(|profiles| profiles.0),
                        next_spawn_tick: state.next_spawn_tick,
                    }
                },
            )
            .collect();
        attempts.sort_unstable_by_key(|attempt| attempt.id);

        if attempts.is_empty() {
            return (0, 0);
        }

        let mut shrines = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let content = *entity.get::<ContentIdentity>()?;
                let replication = crate::building_mechanics::production_replication_for_version(
                    content.map_version,
                );
                (content.rawcode == replication.shrine_rawcode
                    && entity.get::<BuildingEconomyProfile>().is_some()
                    && entity.get::<BuildingConstruction>().is_none()
                    && entity
                        .get::<Health>()
                        .is_some_and(|health| health.current > 0))
                .then_some((
                    *entity.get::<SimId>()?,
                    entity.get::<Owner>()?.0,
                    *entity.get::<Team>()?,
                ))
            })
            .collect::<Vec<_>>();
        shrines.sort_unstable_by_key(|(id, ..)| *id);

        let units = self.units();
        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let reservation_capacity = units
            .len()
            .checked_add(attempts.len())
            .expect("production reservation capacity overflow");
        let max_existing_radius = units
            .iter()
            .map(|unit| unit.collision_radius)
            .max()
            .unwrap_or_else(|| self.default_collision_radius());
        let max_spawn_radius = attempts
            .iter()
            .map(|attempt| {
                attempt
                    .collision_radius
                    .map_or(self.default_collision_radius(), |radius| radius.0)
            })
            .max()
            .unwrap_or_else(|| self.default_collision_radius());
        let reservation_cell_size = max_existing_radius
            .max(max_spawn_radius)
            .saturating_mul(2)
            .max(1);
        let mut ground_reservations = SpatialReservationGrid::build_with_radii(
            reservation_cell_size,
            bounds_min,
            bounds_max,
            reservation_capacity,
            units.iter().enumerate().filter_map(|(index, unit)| {
                (unit.movement_class == MovementClass::Ground && !unit.classifications.locust)
                    .then_some((index, unit.position, unit.collision_radius))
            }),
        );
        let mut air_reservations = SpatialReservationGrid::build_with_radii(
            reservation_cell_size,
            bounds_min,
            bounds_max,
            reservation_capacity,
            units.iter().enumerate().filter_map(|(index, unit)| {
                (unit.movement_class == MovementClass::Air && !unit.classifications.locust)
                    .then_some((index, unit.position, unit.collision_radius))
            }),
        );
        let mut next_reservation_index = units.len();
        let mut spawned = 0;
        let mut failed = 0;

        for attempt in attempts {
            let preferred = preferred_spawn_cell(attempt.team, attempt.footprint);
            let collision_radius = attempt
                .collision_radius
                .map_or(self.default_collision_radius(), |radius| radius.0);
            let reservations = match attempt.movement_class {
                MovementClass::Ground => &mut ground_reservations,
                MovementClass::Air => &mut air_reservations,
            };
            let spawn =
                spiral_cells(preferred, attempt.profile.search_radius_cells).find_map(|cell| {
                    if !self.topology.contains(cell) {
                        return None;
                    }
                    let position = self.topology.center_of_cell(cell);
                    let movement_legal = attempt.classifications.locust
                        || match attempt.movement_class {
                            MovementClass::Ground => {
                                let component = self.topology.component_id(cell)?;
                                attempt.collision_radius.is_none()
                                    || self.topology.circle_is_traversable_in_component(
                                        position,
                                        collision_radius,
                                        component,
                                    )
                            }
                            MovementClass::Air => {
                                let component = self.air_topology.component_id(cell)?;
                                self.air_topology.circle_is_traversable_in_component(
                                    position,
                                    collision_radius,
                                    component,
                                )
                            }
                        };
                    (movement_legal
                        && (attempt.classifications.locust
                            || reservations.is_clear_with_radius(position, collision_radius)))
                    .then_some((cell, position))
                });

            if let Some((_cell, position)) = spawn {
                let properties = UnitGameplayProperties {
                    content: attempt.content,
                    health_regen_per_second_per_10k: attempt.health_regen_per_second_per_10k,
                    corpse: attempt.corpse,
                    collision_radius: attempt.collision_radius,
                    movement_class: attempt.movement_class,
                    mechanical: attempt.mechanical,
                    classifications: attempt.classifications,
                    build_time_ticks: attempt.build_time_ticks,
                    repair_time_ticks: attempt.repair_time_ticks,
                    attack_targets: attempt.attack_targets,
                    secondary_attack: attempt.secondary_attack,
                    action_timing: attempt.action_timing,
                    damage_type: attempt.damage_type,
                    armor: attempt.armor,
                    passive_effects: attempt.passive_effects,
                };
                let definition = ResolvedUnitDefinition {
                    template: attempt.profile.unit,
                    properties,
                    spellcasting: attempt.spellcasting,
                    additional_abilities: attempt.additional_abilities,
                };
                let replication = crate::building_mechanics::production_replication_for_version(
                    attempt
                        .content
                        .map_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION, |c| c.map_version),
                );
                let marked = attempt.content.is_some_and(|content| {
                    crate::content::unit_has_ability_for_version(
                        content.rawcode,
                        AbilityId(replication.marker),
                        content.map_version,
                    )
                });
                let mut shrine_definition = definition;
                for _ in 0..replication.local_copies(marked) {
                    let original = self.spawn_resolved_unit_with_provenance(
                        Some(attempt.owner),
                        UnitSpawn::from_template(attempt.team, position, attempt.profile.unit),
                        definition,
                        Some(attempt.id),
                    );
                    if let Some(content) = attempt.content
                        && let Some(cold) = self.apply_training_variant(
                            original,
                            training_variants::TrainingVariantSpawn {
                                owner: attempt.owner,
                                team: attempt.team,
                                position,
                                producer: attempt.id,
                                version: content.map_version,
                                root_rawcode: content.rawcode,
                            },
                        )
                    {
                        shrine_definition = cold;
                    }
                    spawned += 1;
                }
                if !marked
                    && attempt.content.is_some_and(|content| {
                        crate::content::unit_has_ability_for_version(
                            content.rawcode,
                            AbilityId(replication.twins_marker),
                            content.map_version,
                        )
                    })
                {
                    let content = attempt.content.expect("marked native production");
                    let bundle = crate::castle_fight_content_bundle(content.map_version)
                        .expect("supported companion production version");
                    for rawcode in replication.twins_children {
                        let child = bundle
                            .unit_definitions()
                            .find(|definition| definition.rawcode == rawcode)
                            .expect(
                                "promoted training requires both complete companion definitions",
                            )
                            .resolved();
                        self.spawn_resolved_unit_with_provenance(
                            Some(attempt.owner),
                            UnitSpawn::from_template(attempt.team, position, child.template),
                            child,
                            Some(attempt.id),
                        );
                        spawned += 1;
                    }
                }
                let legendary = attempt.content.is_some_and(|content| {
                    crate::content::unit_has_ability_for_version(
                        content.rawcode,
                        AbilityId(replication.legendary_exclusion),
                        content.map_version,
                    )
                });
                for (shrine_id, shrine_owner, _) in shrines
                    .iter()
                    .filter(|(_, _, team)| *team == attempt.team)
                    .take(if legendary {
                        0
                    } else {
                        replication.maximum_shrines
                    })
                {
                    let roll = deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        attempt.id,
                        RANDOM_PURPOSE_HEROIC_SHRINE,
                        shrine_id.0,
                    ) % 100;
                    if roll < replication.chance_per_100 {
                        for _ in 0..replication.shrine_copies(marked) {
                            self.spawn_resolved_unit_with_provenance(
                                Some(*shrine_owner),
                                UnitSpawn::from_template(
                                    attempt.team,
                                    position,
                                    shrine_definition.template,
                                ),
                                shrine_definition,
                                Some(attempt.id),
                            );
                            spawned += 1;
                        }
                    }
                }
                if !attempt.classifications.locust {
                    reservations.insert_with_radius(
                        next_reservation_index,
                        position,
                        collision_radius,
                    );
                    next_reservation_index += 1;
                }
            } else {
                failed += 1;
            }

            let next = attempt
                .next_spawn_tick
                .checked_add(u64::from(attempt.profile.interval_ticks))
                .expect("production tick overflow");
            let mut entity = self.world.entity_mut(attempt.entity);
            let mut state = entity
                .get_mut::<ProductionState>()
                .expect("production state missing");
            state.next_spawn_tick = next;
            state.queued = 2;
        }

        (spawned, failed)
    }

    fn snapshot_units(&mut self) -> Vec<UnitSnapshot> {
        self.snapshot_units_with_dead(false)
    }

    fn snapshot_units_with_dead(&mut self, include_dead: bool) -> Vec<UnitSnapshot> {
        let mut query = self.world.query::<(
            Entity,
            &SimId,
            &Owner,
            &Team,
            &Position,
            &Health,
            &AttackProfile,
            &AttackCooldown,
            &TargetState,
            &RetaliationState,
            &StatusState,
            &NavigationState,
            &MovementProfile,
            &SpawnTick,
        )>();
        let default_collision_radius = self.default_collision_radius();
        let mut units: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, _, health, _, _, _, _, _, _, _, _)| {
                include_dead || health.current > 0
            })
            .map(
                |(
                    entity,
                    id,
                    owner,
                    team,
                    position,
                    health,
                    attack,
                    cooldown,
                    target,
                    retaliation,
                    status,
                    navigation,
                    movement,
                    spawn_tick,
                )| {
                    let entity_ref = self.world.entity(entity);
                    let corpse = entity_ref.get::<CorpseProducer>().map(|corpse| corpse.0);
                    let attack_sequence = entity_ref
                        .get::<AttackSequence>()
                        .expect("unit attack sequence missing")
                        .0;
                    let collision_radius = entity_ref.get::<CollisionRadius>().copied();
                    let movement_class = *entity_ref
                        .get::<MovementClass>()
                        .expect("unit movement class missing");
                    let mechanical = entity_ref.get::<MechanicalUnit>().is_some();
                    let attack_targets = *entity_ref
                        .get::<AttackTargetMask>()
                        .expect("unit attack target mask missing");
                    let secondary_attack = entity_ref.get::<SecondaryAttackProfile>().copied();
                    let damage_type = *entity_ref
                        .get::<DamageType>()
                        .expect("unit damage type missing");
                    let armor = *entity_ref
                        .get::<ArmorProfile>()
                        .expect("unit armor profile missing");
                    let passive_effects = *entity_ref
                        .get::<PassiveUnitEffects>()
                        .expect("unit passive effects missing");
                    let spellcasting = entity_ref.get::<SpellcastingProfile>().copied();
                    let mana_current = entity_ref.get::<ManaState>().map(|mana| mana.current);
                    let ability_state = entity_ref.get::<AutomaticAbilityState>().copied();
                    debug_assert_eq!(
                        spellcasting.is_some()
                            || entity_ref.get::<ContentIdentity>().is_some_and(|c| {
                                crate::content::native_mana_profile_for_version(
                                    c.map_version,
                                    c.rawcode,
                                )
                                .is_some()
                            }),
                        mana_current.is_some()
                    );
                    debug_assert_eq!(spellcasting.is_some(), ability_state.is_some());
                    let content = entity_ref.get::<ContentIdentity>();
                    let death_heal = content.and_then(|c| {
                        let p = crate::building_mechanics::death_heal_for_version(c.map_version)?;
                        crate::content::unit_has_ability_for_version(
                            c.rawcode,
                            AbilityId(p.marker),
                            c.map_version,
                        )
                        .then_some(death_healing::DeathHealSupport {
                            version: c.map_version,
                            active: false,
                            healing: 0,
                            radius: p.radius,
                            other_type_per_10k: p.other_type_per_10k,
                        })
                    });
                    UnitSnapshot {
                        removed_without_death: false,
                        air_attack_bonus_per_10k: content.map_or(0, |c| {
                            crate::content::native_air_attack_bonus_for_version(
                                c.map_version,
                                c.rawcode,
                            )
                        }),
                        damage_seed: self.config.match_seed,
                        death_rule: content.and_then(|c| {
                            crate::native_effects::unit_death_rule_for_version(
                                c.map_version,
                                c.rawcode,
                            )
                        }),
                        death_rules: self.combat_rules.damage_rules,
                        death_tick: self.next_tick,
                        rawcode: content.map_or(0, |c| c.rawcode),
                        death_heal,
                        death_position: position.0,
                        death_heal_visuals: 0,
                        death_heal_visual_version: None,
                        snow_damage_taken_per_10k: 10_000,
                        detection_teams: self.fog.as_ref().map_or(0, |fog| {
                            u8::from(fog.detects_invisible(Team(0), position.0))
                                | (u8::from(fog.detects_invisible(Team(1), position.0)) << 1)
                        }),
                        map_version: entity_ref
                            .get::<ContentIdentity>()
                            .map(|content| content.map_version),
                        visible_teams: self.visible_teams(position.0),
                        entity,
                        id: *id,
                        owner: owner.0,
                        team: *team,
                        position: position.0,
                        health: health.current,
                        health_max: health.max,
                        attack: native_actions::ensnare::ensnared_attack(
                            *attack,
                            movement_class,
                            *status,
                        ),
                        action_timing: *entity_ref
                            .get::<ActionTimingProfile>()
                            .expect("unit action timing missing"),
                        cooldown_remaining: cooldown.remaining,
                        attack_sequence,
                        target: target.current,
                        direct_retaliation_lock: target.direct_retaliation_lock,
                        ally_defense_lock: target.ally_defense_lock,
                        retaliation: *retaliation,
                        status: *status,
                        navigation: *navigation,
                        movement: *movement,
                        spawn_tick: spawn_tick.0,
                        corpse,
                        collision_radius: collision_radius
                            .map_or(default_collision_radius, |radius| radius.0),
                        collision_radius_override: collision_radius.map(|radius| radius.0),
                        movement_class,
                        mechanical,
                        classifications: entity_ref
                            .get::<UnitClassifications>()
                            .copied()
                            .unwrap_or_default(),
                        attack_targets,
                        secondary_attack,
                        damage_type,
                        armor,
                        passive_effects,
                        attacks_disabled: false,
                        abilities_disabled: false,
                        orders_suspended: false,
                        spellcasting,
                        mana_current,
                        ability_state,
                    }
                },
            )
            .collect();
        units.sort_unstable_by_key(|unit| unit.id);
        for unit in &mut units {
            self.project_building_spell_control(unit);
            unit.classifications.invulnerable |= unit
                .status
                .impale
                .is_some_and(|p| self.next_tick < p.landed_tick);
        }
        units
    }

    fn snapshot_buildings(&mut self) -> Vec<BuildingSnapshot> {
        let mut query = self.world.query::<(
            Entity,
            &SimId,
            &Team,
            &BuildingFootprint,
            &Health,
            Option<&AttackProfile>,
            Option<&AttackCooldown>,
            Option<&TargetState>,
            Option<&SpawnTick>,
            Option<&SpellcastingProfile>,
            Option<&ManaState>,
            Option<&AutomaticAbilityState>,
            Option<&StatusState>,
        )>();
        let mut buildings: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, health, _, _, _, _, _, _, _, _)| health.current > 0)
            .map(
                |(
                    entity,
                    id,
                    team,
                    footprint,
                    health,
                    attack,
                    cooldown,
                    target,
                    spawn_tick,
                    spellcasting,
                    mana,
                    ability_state,
                    status,
                )| {
                    let entity_ref = self.world.entity(entity);
                    let attack_targets = entity_ref.get::<AttackTargetMask>().copied();
                    let damage_type = *entity_ref
                        .get::<DamageType>()
                        .expect("building damage type missing");
                    let armor = *entity_ref
                        .get::<ArmorProfile>()
                        .expect("building armor profile missing");
                    debug_assert_eq!(attack.is_some(), cooldown.is_some());
                    debug_assert_eq!(attack.is_some(), target.is_some());
                    debug_assert_eq!(attack.is_some(), spawn_tick.is_some());
                    debug_assert_eq!(attack.is_some(), attack_targets.is_some());
                    debug_assert_eq!(
                        spellcasting.is_some()
                            || (entity_ref.get::<BuildingConstruction>().is_none()
                                && entity_ref.get::<ContentIdentity>().is_some_and(|c| {
                                    crate::content::building_native_mana_profile(
                                        c.map_version,
                                        c.rawcode,
                                    )
                                    .is_some()
                                })),
                        mana.is_some()
                    );
                    debug_assert_eq!(spellcasting.is_some(), ability_state.is_some());
                    debug_assert!(
                        !(attack.is_some() || spellcasting.is_some()) || status.is_some()
                    );
                    BuildingSnapshot {
                        death_heal_contribution: death_healing::building_contribution(&entity_ref),
                        action_timing: entity_ref
                            .get::<ActionTimingProfile>()
                            .copied()
                            .unwrap_or_default(),
                        secondary_attack: entity_ref.get::<SecondaryAttackProfile>().copied(),
                        passive_effects: entity_ref
                            .get::<PassiveUnitEffects>()
                            .copied()
                            .unwrap_or_default(),
                        snow_damage_taken_per_10k: 10_000,
                        map_version: entity_ref
                            .get::<ContentIdentity>()
                            .map(|content| content.map_version),
                        visible_teams: self.visible_teams(footprint_center_point(
                            *footprint,
                            self.config.navigation_cell_size,
                        )),
                        entity,
                        id: *id,
                        team: *team,
                        footprint: *footprint,
                        health: health.current,
                        attack: attack.copied(),
                        attack_targets,
                        damage_type,
                        armor,
                        classifications: entity_ref
                            .get::<UnitClassifications>()
                            .copied()
                            .unwrap_or_default(),
                        cooldown_remaining: cooldown.map(|cooldown| cooldown.remaining),
                        target: target.and_then(|target| target.current),
                        spawn_tick: spawn_tick.map(|spawn_tick| spawn_tick.0),
                        spellcasting: spellcasting.copied(),
                        mana_current: mana.map(|mana| mana.current),
                        ability_state: ability_state.copied(),
                        status: status.copied(),
                    }
                },
            )
            .collect();
        buildings.sort_unstable_by_key(|building| building.id);
        buildings
    }
}

#[derive(Debug, Clone, Copy)]
struct UnitSnapshot {
    air_attack_bonus_per_10k: u16,
    death_rule: Option<crate::native_effects::NativeUnitDeathRule>,
    damage_seed: u64,
    death_rules: DamageRules,
    death_tick: u64,
    rawcode: u32,
    death_position: SimPoint,
    death_heal: Option<death_healing::DeathHealSupport>,
    death_heal_visuals: u16,
    death_heal_visual_version: Option<crate::MapVersion>,
    snow_damage_taken_per_10k: u16,
    detection_teams: u8,
    map_version: Option<crate::MapVersion>,
    visible_teams: u8,
    entity: Entity,
    id: SimId,
    owner: PlayerId,
    team: Team,
    position: SimPoint,
    /// Phase-local RemoveUnit directive. Never survives a completed step.
    removed_without_death: bool,
    health: i32,
    health_max: i32,
    attack: AttackProfile,
    action_timing: ActionTimingProfile,
    cooldown_remaining: u16,
    attack_sequence: u64,
    target: Option<SimId>,
    direct_retaliation_lock: bool,
    ally_defense_lock: bool,
    retaliation: RetaliationState,
    status: StatusState,
    navigation: NavigationState,
    movement: MovementProfile,
    spawn_tick: u64,
    corpse: Option<CorpseProfile>,
    collision_radius: i32,
    collision_radius_override: Option<i32>,
    movement_class: MovementClass,
    mechanical: bool,
    classifications: UnitClassifications,
    attack_targets: AttackTargetMask,
    secondary_attack: Option<SecondaryAttackProfile>,
    damage_type: DamageType,
    armor: ArmorProfile,
    passive_effects: PassiveUnitEffects,
    attacks_disabled: bool,
    abilities_disabled: bool,
    orders_suspended: bool,
    spellcasting: Option<SpellcastingProfile>,
    mana_current: Option<i32>,
    ability_state: Option<AutomaticAbilityState>,
}

// A zero-range melee weapon still reaches physical contact. Use the same
// derived envelope for pursuit, reachability and release; never rewrite content.
// Two coordinate quanta cover truncation of both axes at a circular contact goal.
fn melee_contact_attack(mut attack: AttackProfile, contact_radius: i32) -> AttackProfile {
    if attack.range == 0 && attack.delivery == AttackDelivery::Melee {
        attack.range = contact_radius.saturating_add(2);
    }
    attack
}

impl UnitSnapshot {
    fn effective_passive_effects(&self) -> impl Iterator<Item = PassiveUnitEffect> + '_ {
        self.passive_effects.iter().chain(
            self.status
                .native_grants
                .passive_effects()
                .filter(|_| !self.abilities_disabled),
        )
    }

    fn target_movement_class(&self) -> MovementClass {
        if self.status.ensnare.is_some() {
            MovementClass::Ground
        } else {
            self.movement_class
        }
    }

    fn is_invisible(&self, tick: u64) -> bool {
        self.classifications.invisible || self.status.wind_walk_invisible(tick)
    }

    fn visible_to(&self, team: Team, tick: u64) -> bool {
        !self.classifications.locust
            && (self.team == team
                || self.status.is_revealed_to(team, tick)
                || self.detection_teams & (1 << team.0) != 0
                || (!self.is_invisible(tick) && self.visible_teams & (1 << team.0) != 0))
    }

    fn attack_for_unit_target(
        self,
        target: &UnitSnapshot,
    ) -> Option<(AttackProfile, AttackTargetMask, DamageType)> {
        let (attack, mask, damage) = self.attack_for_unit(target.target_movement_class())?;
        Some((
            melee_contact_attack(attack, self.collision_radius + target.collision_radius),
            mask,
            damage,
        ))
    }

    fn attack_for_unit(
        self,
        movement_class: MovementClass,
    ) -> Option<(AttackProfile, AttackTargetMask, DamageType)> {
        if self.attacks_disabled || self.orders_suspended {
            return None;
        }
        if let Some(secondary) = self.secondary_attack
            && !self
                .primary_attack_targets()
                .can_target_unit(movement_class)
            && secondary.targets.can_target_unit(movement_class)
        {
            return Some((secondary.attack, secondary.targets, secondary.damage_type));
        }
        self.primary_attack_targets()
            .can_target_unit(movement_class)
            .then_some((self.attack, self.primary_attack_targets(), self.damage_type))
    }

    fn attack_for_building(self) -> Option<(AttackProfile, AttackTargetMask, DamageType)> {
        if self.attacks_disabled || self.orders_suspended {
            return None;
        }
        if let Some(secondary) = self.secondary_attack
            && !self.primary_attack_targets().can_target_buildings()
            && secondary.targets.can_target_buildings()
        {
            return Some((
                melee_contact_attack(secondary.attack, self.collision_radius),
                secondary.targets,
                secondary.damage_type,
            ));
        }
        self.primary_attack_targets()
            .can_target_buildings()
            .then_some((
                melee_contact_attack(self.attack, self.collision_radius),
                self.primary_attack_targets(),
                self.damage_type,
            ))
    }

    fn primary_attack_targets(self) -> AttackTargetMask {
        if let Some(secondary) = self.secondary_attack {
            secondary.primary_targets
        } else {
            self.attack_targets
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BuildingSnapshot {
    death_heal_contribution: Option<death_healing::DeathHealContribution>,
    action_timing: ActionTimingProfile,
    secondary_attack: Option<SecondaryAttackProfile>,
    passive_effects: PassiveUnitEffects,
    snow_damage_taken_per_10k: u16,
    map_version: Option<crate::MapVersion>,
    visible_teams: u8,
    entity: Entity,
    id: SimId,
    team: Team,
    footprint: BuildingFootprint,
    health: i32,
    classifications: UnitClassifications,
    attack: Option<AttackProfile>,
    attack_targets: Option<AttackTargetMask>,
    damage_type: DamageType,
    armor: ArmorProfile,
    cooldown_remaining: Option<u16>,
    target: Option<SimId>,
    spawn_tick: Option<u64>,
    spellcasting: Option<SpellcastingProfile>,
    mana_current: Option<i32>,
    ability_state: Option<AutomaticAbilityState>,
    status: Option<StatusState>,
}

impl BuildingSnapshot {
    fn primary_attack_targets(self) -> AttackTargetMask {
        self.secondary_attack.map_or_else(
            || self.attack_targets.unwrap_or_default(),
            |attack| attack.primary_targets,
        )
    }

    fn attack_for_unit(
        self,
        movement: MovementClass,
    ) -> Option<(AttackProfile, AttackTargetMask, DamageType)> {
        let primary = self.attack?;
        let mask = self.primary_attack_targets();
        if mask.can_target_unit(movement) {
            return Some((primary, mask, self.damage_type));
        }
        let secondary = self.secondary_attack?;
        secondary.targets.can_target_unit(movement).then_some((
            secondary.attack,
            secondary.targets,
            secondary.damage_type,
        ))
    }

    fn attack_for_building(self) -> Option<(AttackProfile, AttackTargetMask, DamageType)> {
        let primary = self.attack?;
        let mask = self.primary_attack_targets();
        if mask.can_target_buildings() {
            return Some((primary, mask, self.damage_type));
        }
        let secondary = self.secondary_attack?;
        secondary.targets.can_target_buildings().then_some((
            secondary.attack,
            secondary.targets,
            secondary.damage_type,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
struct ProductionAttempt {
    entity: Entity,
    id: SimId,
    owner: PlayerId,
    team: Team,
    footprint: BuildingFootprint,
    profile: ProductionProfile,
    content: Option<ContentIdentity>,
    corpse: Option<CorpseProfile>,
    collision_radius: Option<CollisionRadius>,
    movement_class: MovementClass,
    mechanical: bool,
    classifications: UnitClassifications,
    build_time_ticks: Option<u32>,
    repair_time_ticks: Option<u32>,
    attack_targets: AttackTargetMask,
    secondary_attack: Option<SecondaryAttackProfile>,
    action_timing: ActionTimingProfile,
    health_regen_per_second_per_10k: u32,
    damage_type: DamageType,
    armor: ArmorProfile,
    passive_effects: PassiveUnitEffects,
    spellcasting: Option<SpellcastingProfile>,
    additional_abilities: Option<AdditionalAutomaticAbilityDefinitions>,
    next_spawn_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetIndex {
    Unit(usize),
    Building(usize),
}

#[derive(Debug, Clone, Copy)]
enum AttackSourceIndex {
    Unit(usize),
    Building(usize),
}

#[derive(Debug, Clone, Copy)]
enum AbilitySourceIndex {
    Unit(usize),
    Building(usize),
}

#[derive(Debug, Clone, Copy)]
enum AbilitySourceOrigin {
    Unit(SimPoint),
    Building(BuildingFootprint),
}

#[derive(Debug, Clone, Copy)]
struct AbilitySourceSnapshot {
    map_version: Option<crate::MapVersion>,
    source: AbilitySourceIndex,
    id: SimId,
    team: Team,
    origin: AbilitySourceOrigin,
    health: i32,
    stunned_until_tick: u64,
    spellcasting: Option<SpellcastingProfile>,
    mana_current: Option<i32>,
    ability_state: Option<AutomaticAbilityState>,
}

#[derive(Debug, Clone, Copy)]
struct AttackIntent {
    source: AttackSourceIndex,
    target: TargetIndex,
    source_id: SimId,
    source_team: Team,
    source_position: SimPoint,
    target_id: SimId,
    attack: AttackProfile,
    attack_targets: AttackTargetMask,
    damage_type: DamageType,
    passive_effects: PassiveUnitEffects,
    attack_sequence: u64,
    distance_sq: u64,
    completing_windup: bool,
}

#[derive(Debug, Clone, Copy)]
enum AbilityIntentTarget {
    Unit { index: usize, id: SimId },
    Building { id: SimId, position: SimPoint },
    AllEnemyUnits,
    AllFriendlyUnits,
    Corpse { id: SimId, position: SimPoint },
    Point { position: SimPoint },
}

impl AbilityIntentTarget {
    const fn pending_cast_target(self) -> PendingCastTarget {
        match self {
            Self::Unit { id, .. } => PendingCastTarget::Unit(id),
            Self::Building { id, position } => PendingCastTarget::Building { id, position },
            Self::AllEnemyUnits => PendingCastTarget::AllEnemyUnits,
            Self::AllFriendlyUnits => PendingCastTarget::AllFriendlyUnits,
            Self::Corpse { id, position } => PendingCastTarget::Corpse { id, position },
            Self::Point { position } => PendingCastTarget::Point(position),
        }
    }

    const fn sort_key(self) -> (u8, SimId) {
        match self {
            Self::Unit { id, .. } => (0, id),
            Self::AllEnemyUnits => (1, SimId(0)),
            Self::AllFriendlyUnits => (2, SimId(0)),
            Self::Corpse { id, .. } => (3, id),
            Self::Point { .. } => (4, SimId(0)),
            Self::Building { id, .. } => (5, id),
        }
    }

    const fn cast_target(self) -> AbilityCastTarget {
        match self {
            Self::Unit { id, .. } => AbilityCastTarget::Unit(id),
            Self::AllEnemyUnits => AbilityCastTarget::AllEnemyUnits,
            Self::AllFriendlyUnits => AbilityCastTarget::AllFriendlyUnits,
            Self::Corpse { position, .. } => AbilityCastTarget::Point(position),
            Self::Point { position } => AbilityCastTarget::Point(position),
            Self::Building { position, .. } => AbilityCastTarget::Point(position),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AbilityIntent {
    source: AbilitySourceIndex,
    source_id: SimId,
    target: AbilityIntentTarget,
    ability: AutomaticAbilityProfile,
    cast_sequence: u64,
    completing_windup: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct AbilityMetrics {
    evaluations: usize,
    casts: usize,
    candidate_checks: usize,
    effects: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct AbilityEvaluation {
    intent: Option<AbilityIntent>,
    candidate_checks: usize,
}

#[derive(Debug, Clone, Copy)]
struct ProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: GuaranteedHitProjectile,
}

#[derive(Debug, Clone, Copy)]
struct ReflectedProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: ReflectedProjectile,
}

#[derive(Debug, Clone, Copy)]
struct BallisticProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: BallisticProjectile,
}

#[derive(Debug, Clone, Copy)]
struct BounceProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: BounceProjectile,
}

#[derive(Debug, Clone, Copy)]
enum DueTargetProjectileSnapshot {
    GuaranteedHit(ProjectileSnapshot),
    Reflected(ReflectedProjectileSnapshot),
    Bounce(BounceProjectileSnapshot),
}

impl DueTargetProjectileSnapshot {
    const fn id(&self) -> SimId {
        match self {
            Self::GuaranteedHit(snapshot) => snapshot.id,
            Self::Reflected(snapshot) => snapshot.id,
            Self::Bounce(snapshot) => snapshot.id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ProjectileLaunch {
    source: SimId,
    source_team: Team,
    source_is_building: bool,
    target: SimId,
    damage: i32,
    on_hit: PendingAttackEffects,
    damage_type: DamageType,
    speed_per_tick: i32,
    launch_position: SimPoint,
    launch_tick: u64,
    impact_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct ReflectedProjectileLaunch {
    original_source: SimId,
    reflector: SimId,
    reflector_team: Team,
    target: SimId,
    damage: i32,
    damage_type: DamageType,
    launch_position: SimPoint,
    launch_tick: u64,
    impact_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct BallisticProjectileLaunch {
    source: SimId,
    source_team: Team,
    target_mask: AttackTargetMask,
    damage: i32,
    burning_oil: Option<crate::components::BurningOilEffectProfile>,
    splash_falloff: Option<crate::components::SplashFalloffProfile>,
    frost: Option<crate::components::FrostAttackEffectProfile>,
    damage_type: DamageType,
    launch_position: SimPoint,
    destination: SimPoint,
    impact_radius: i32,
    launch_tick: u64,
    impact_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct BounceProjectileLaunch {
    source: SimId,
    source_rawcode: Option<u32>,
    source_team: Team,
    source_is_building: bool,
    target_mask: AttackTargetMask,
    target: SimId,
    damage: i32,
    damage_type: DamageType,
    launch_position: SimPoint,
    launch_tick: u64,
    impact_tick: u64,
    speed_per_tick: i32,
    bounce_range: i32,
    max_bounces: u8,
    damage_percent_per_bounce: u16,
    allow_repeat_targets: bool,
}

#[derive(Debug, Clone, Copy)]
struct BounceProjectileUpdate {
    entity: Entity,
    projectile: BounceProjectile,
}

struct BounceSearchContext<'a> {
    completed_tick: u64,
    units: &'a [UnitSnapshot],
    unit_health: &'a [i32],
    grid: &'a SpatialGrid,
}

struct DamageTargetState<'a> {
    match_seed: u64,
    damage_rules: DamageRules,
    units: &'a mut [UnitSnapshot],
    buildings: &'a [BuildingSnapshot],
    unit_positions: &'a [SimPoint],
    unit_health: &'a mut [i32],
    building_health: &'a mut [i32],
    attackers_this_tick: &'a mut [Option<SimId>],
    next_defense_alerts: &'a mut Vec<DefenseAlert>,
    navigation_cell_size: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct DefenseAlert {
    victim_id: SimId,
    victim_team: Team,
    victim_position: SimPoint,
    attacker_id: SimId,
    attacked_tick: u64,
}

#[derive(Debug)]
struct DefenseVictim {
    victim_id: SimId,
    victim_team: Team,
    victim_position: SimPoint,
    attacked_tick: u64,
    unit_attackers: Vec<usize>,
    building_attackers: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default)]
struct DefenseTargetSearch {
    target: Option<SimId>,
    queried: bool,
    victim_candidates: usize,
    attacker_candidates: usize,
}

struct DefenseSearchContext<'a> {
    units: &'a [UnitSnapshot],
    buildings: &'a [BuildingSnapshot],
    attacker_grid: &'a SpatialGrid,
}

#[derive(Debug, Clone, Copy)]
struct TargetDecision {
    target: Option<SimId>,
    direct_retaliation_lock: bool,
    ally_defense_lock: bool,
    defense_query: DefenseTargetSearch,
}

impl TargetDecision {
    const fn without_defense(
        target: Option<SimId>,
        direct_retaliation_lock: bool,
        ally_defense_lock: bool,
    ) -> Self {
        Self {
            target,
            direct_retaliation_lock,
            ally_defense_lock,
            defense_query: DefenseTargetSearch {
                target: None,
                queried: false,
                victim_candidates: 0,
                attacker_candidates: 0,
            },
        }
    }

    const fn with_defense(
        target: Option<SimId>,
        direct_retaliation_lock: bool,
        ally_defense_lock: bool,
        defense_query: DefenseTargetSearch,
    ) -> Self {
        Self {
            target,
            direct_retaliation_lock,
            ally_defense_lock,
            defense_query,
        }
    }
}

#[derive(Debug)]
struct TargetSelectionResult {
    decisions: Vec<TargetDecision>,
    retained_targets: usize,
    target_changes: usize,
    ally_defense_queries: usize,
    ally_defense_victim_candidates: usize,
    ally_defense_attacker_candidates: usize,
}

#[derive(Debug)]
struct BuildingTargetSelectionResult {
    decisions: Vec<Option<SimId>>,
    retained_targets: usize,
    target_changes: usize,
}

fn defense_attacker_partition(victim_index: usize) -> SpatialPartition {
    let component = u32::try_from(victim_index).expect("too many defense-victim groups");
    assert_ne!(
        component,
        SpatialPartition::GLOBAL_COMPONENT,
        "defense-victim index exhausted reserved spatial partition"
    );
    SpatialPartition::new(0, component)
}

fn grouped_defense_victims(
    alerts: &[DefenseAlert],
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
) -> Vec<DefenseVictim> {
    let mut alerts = alerts.to_vec();
    alerts.sort_unstable_by_key(|alert| {
        (
            alert.attacked_tick,
            alert.victim_team.0,
            alert.victim_id,
            alert.victim_position.x,
            alert.victim_position.y,
            alert.attacker_id,
        )
    });

    let mut victims: Vec<DefenseVictim> = Vec::new();
    for alert in alerts {
        let can_merge = victims.last().is_some_and(|victim| {
            victim.attacked_tick == alert.attacked_tick
                && victim.victim_team == alert.victim_team
                && victim.victim_id == alert.victim_id
                && victim.victim_position == alert.victim_position
        });
        let attacker_unit_index = find_unit_index(units, alert.attacker_id);
        let attacker_building_index = find_building_index(buildings, alert.attacker_id);
        if attacker_unit_index.is_none() && attacker_building_index.is_none() {
            continue;
        }

        if can_merge {
            let victim = victims.last_mut().expect("checked defense victim missing");
            if let Some(index) = attacker_unit_index {
                if victim.unit_attackers.last().copied() != Some(index) {
                    victim.unit_attackers.push(index);
                }
            } else if let Some(index) = attacker_building_index
                && victim.building_attackers.last().copied() != Some(index)
            {
                victim.building_attackers.push(index);
            }
        } else {
            victims.push(DefenseVictim {
                victim_id: alert.victim_id,
                victim_team: alert.victim_team,
                victim_position: alert.victim_position,
                attacked_tick: alert.attacked_tick,
                unit_attackers: attacker_unit_index.into_iter().collect(),
                building_attackers: attacker_building_index.into_iter().collect(),
            });
        }
    }
    victims
}

fn validate_player_configs(players: &[PlayerConfig]) -> Vec<PlayerConfig> {
    assert!(
        !players.is_empty(),
        "simulation requires at least one player"
    );
    let mut players = players.to_vec();
    players.sort_unstable_by_key(|player| player.id);
    for player in &players {
        assert!(
            player.team.0 < 2,
            "verification slice supports teams 0 and 1 only"
        );
    }
    for pair in players.windows(2) {
        assert_ne!(
            pair[0].id, pair[1].id,
            "duplicate PlayerId {}",
            pair[0].id.0
        );
    }
    players
}

fn validate_combat_rules(config: &SimulationConfig, rules: &CombatRules) {
    assert!(
        rules.uphill_miss_chance_per_10k <= UPHILL_MISS_CHANCE_SCALE,
        "uphill miss chance must be between 0 and {UPHILL_MISS_CHANCE_SCALE} per 10k"
    );
    if rules.uphill_miss_chance_per_10k == 0 {
        return;
    }

    let terrain = rules
        .terrain_elevation
        .as_ref()
        .expect("non-zero uphill miss chance requires authoritative terrain elevation");
    let min_point = SimPoint::new(
        navigation_boundary_coordinate(config.navigation_min.x, config.navigation_cell_size),
        navigation_boundary_coordinate(config.navigation_min.y, config.navigation_cell_size),
    );
    let max_point = SimPoint::new(
        navigation_boundary_coordinate(
            config
                .navigation_max
                .x
                .checked_add(1)
                .expect("navigation x boundary overflow"),
            config.navigation_cell_size,
        ),
        navigation_boundary_coordinate(
            config
                .navigation_max
                .y
                .checked_add(1)
                .expect("navigation y boundary overflow"),
            config.navigation_cell_size,
        ),
    );
    assert!(
        terrain.contains(min_point) && terrain.contains(max_point),
        "authoritative terrain elevation must cover the full navigation bounds when uphill miss is enabled"
    );
}

fn navigation_boundary_coordinate(cell: i32, cell_size: i32) -> i32 {
    let coordinate = i64::from(cell)
        .checked_mul(i64::from(cell_size))
        .expect("navigation boundary coordinate overflowed i64");
    i32::try_from(coordinate).expect("navigation boundary coordinate overflowed i32")
}

fn validate_attack_profile(attack: AttackProfile) {
    assert!(attack.damage >= 0);
    assert!(attack.range >= 0);
    assert!(attack.acquisition_range >= 0);
    match attack.delivery {
        AttackDelivery::Melee | AttackDelivery::RangedInstant => {}
        AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
            assert!(speed_per_tick >= 0);
        }
        AttackDelivery::RangedBallistic {
            speed_per_tick,
            impact_radius,
        } => {
            assert!(speed_per_tick >= 0);
            assert!(impact_radius >= 0);
        }
        AttackDelivery::Line {
            speed_per_tick,
            minimum_range,
            spill_distance,
            spill_radius,
            damage_retention_per_10k,
            ..
        } => {
            assert!(speed_per_tick > 0);
            assert!(minimum_range >= 0 && minimum_range <= attack.range);
            assert!(spill_distance >= 0 && spill_radius >= 0);
            assert!(damage_retention_per_10k <= 10_000);
        }
        AttackDelivery::Bounce {
            speed_per_tick,
            bounce_range,
            max_bounces,
            damage_percent_per_bounce,
            allow_repeat_targets: _,
        } => {
            assert!(speed_per_tick >= 0);
            assert!(bounce_range >= 0);
            assert!(usize::from(max_bounces) < MAX_BOUNCE_HITS);
            assert!((1..=100).contains(&damage_percent_per_bounce));
        }
    }
}

fn validate_spellcasting_profile(spellcasting: SpellcastingProfile) {
    assert!(spellcasting.mana.maximum >= 0);
    assert!(spellcasting.mana.starting >= 0);
    assert!(spellcasting.mana.starting <= spellcasting.mana.maximum);
    assert!(match spellcasting.mana.regeneration() {
        crate::components::ManaRegeneration::PerTickPer10k(rate) => rate <= 10_000_000,
        crate::components::ManaRegeneration::PerSecondPer10k(rate) => rate <= 300_000_000,
    });
    assert!(spellcasting.ability.mana_cost >= 0);
    assert!(spellcasting.ability.mana_cost <= spellcasting.mana.maximum);
    assert!(spellcasting.ability.range >= 0);
    match spellcasting.ability.target_policy {
        AbilityTargetPolicy::ScriptedController
        | AbilityTargetPolicy::RandomEnemyUnit
        | AbilityTargetPolicy::NearestEnemyInCombat
        | AbilityTargetPolicy::FlyingEnemyUnit
        | AbilityTargetPolicy::NativeBuildingSpellTrigger
        | AbilityTargetPolicy::RandomEnemyUnitOrBuilding
        | AbilityTargetPolicy::RandomEnemyDebuff
        | AbilityTargetPolicy::FriendlyUnitInCombat
        | AbilityTargetPolicy::NativeBuffDonor
        | AbilityTargetPolicy::RandomGroundEnemyUnit
        | AbilityTargetPolicy::RecentlyAttackedFriendlyUnit
        | AbilityTargetPolicy::WoundedFriendlyUnit
        | AbilityTargetPolicy::AllFriendlyUnits
        | AbilityTargetPolicy::RandomCorpse => {}
        AbilityTargetPolicy::AllEnemyUnits
        | AbilityTargetPolicy::RandomEnemyUnitGlobal
        | AbilityTargetPolicy::RandomEnemyBasePoint => {
            assert_eq!(spellcasting.ability.range, 0);
        }
    }
    match spellcasting.ability.effect {
        AbilityEffect::NagaTidalWave(_) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::NagaCharm(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            assert!(p.child_range > 0 && p.child.0 != 0 && p.parent.buff_rawcode != 0);
        }
        AbilityEffect::NagaEarthquake(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            assert!(
                p.radius > 0 && p.damage >= 0 && p.duration_ticks > 0 && p.parent.buff_rawcode != 0
            );
        }
        AbilityEffect::Starfall(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            assert!(p.dummy_lifetime_ticks > 0);
            assert!(p.options.iter().all(|w| w.ability.0 != 0
                && w.radius > 0
                && w.damage >= 0
                && w.interval_ticks > 0
                && w.duration_ticks > 0
                && w.mana_cost >= 0
                && w.building_damage_per_10k <= 10000));
        }
        AbilityEffect::Wilderness(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            assert!(p.grants.iter().all(|g| g.ability.0 != 0));
        }
        AbilityEffect::SilverGlade(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            assert!(p.radius >= 0 && p.health >= 0 && p.mana >= 0 && p.duration_ticks > 0);
        }
        AbilityEffect::Heal(p) => {
            assert!(p.amount >= 0 && p.resume_attack_ticks > 0);
        }
        AbilityEffect::WindWalk(p) => {
            assert!(
                p.duration_ticks > p.fade_ticks
                    && p.rules.budget > 0
                    && p.rules.interval_ticks > 1
                    && p.rules.cache_ticks > 0
            );
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::ScriptedController
            );
        }
        AbilityEffect::ScriptedRoots(p) => {
            assert!(p.search_radius >= 0 && p.child_range >= 0 && p.maximum_targets > 0);
            assert!(p.roots.duration_ticks > 0 && p.roots.hero_duration_ticks > 0);
            assert!(p.roots.damage_per_second >= 0 && p.vision_radius >= 0);
        }
        AbilityEffect::Ensnare {
            profile,
            search_radius,
            maximum_targets,
        } => {
            assert!(profile.duration_ticks > 0 && profile.hero_duration_ticks > 0);
            assert!(profile.range >= 0 && profile.speed_per_tick > 0);
            assert!(profile.air_height >= 0 && profile.melee_range >= 0);
            assert!(search_radius >= 0 && maximum_targets > 0);
        }
        AbilityEffect::SpellSteal { recipient_radius } => {
            assert!(recipient_radius >= 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuffDonor
            );
        }
        AbilityEffect::StatBuff {
            buff,
            duration_ticks,
            hero_duration_ticks,
            autocast_range,
            ..
        } => {
            assert!(
                buff.positive
                    && duration_ticks > 0
                    && hero_duration_ticks > 0
                    && autocast_range >= 0
            );
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::FriendlyUnitInCombat
            );
        }
        AbilityEffect::BuildingAura(p) => {
            assert!(p.radius >= 0 && p.lifetime_ticks > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::BuildingStasis(p) => {
            assert!(p.detection_radius >= 0 && p.detonation_radius >= 0 && p.lifetime_ticks > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::NativeWeatherVisual { .. } | AbilityEffect::ManaBurnVisual => {
            panic!("weather art is a presentation event, not a cast profile")
        }
        AbilityEffect::BuildingWeather(p) => {
            assert!((1..=2).contains(&p.option_count) && p.dummy_lifetime_ticks > 0);
            for wave in p.options.into_iter().flatten() {
                assert!(
                    wave.radius >= 0
                        && wave.damage >= 0
                        && wave.maximum_damage > 0
                        && wave.waves > 0
                        && wave.interval_ticks > 0
                );
            }
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::BuildingEnchantment(p) => {
            assert!(p.heal_if_missing_hp_gt >= 0 && p.heal_amount > 0);
            assert!(usize::from(p.dispatcher_option_index) < p.options.len());
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::BuildingPermanentGrant(p) => {
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
            let mut identities = std::collections::BTreeSet::new();
            for grant in p.always.into_iter().chain(p.options.into_iter().flatten()) {
                assert!(identities.insert(grant.ability));
                match grant.effect {
                    crate::NativeAbilityGrantEffect::CriticalStrike {
                        chance_per_10k,
                        multiplier_per_10k,
                        ..
                    }
                    | crate::NativeAbilityGrantEffect::Demolish {
                        chance_per_10k,
                        multiplier_per_10k,
                        ..
                    } => {
                        assert!(chance_per_10k <= 10_000 && multiplier_per_10k >= 10_000);
                    }
                    crate::NativeAbilityGrantEffect::Evasion { chance_per_10k } => {
                        assert!(chance_per_10k <= 10_000)
                    }
                    _ => {}
                }
            }
        }
        AbilityEffect::SirenPurge(p) => {
            assert!(p.child.range > 0 && p.child.duration_ticks > 0);
        }
        AbilityEffect::ImpaleFan(p) => {
            assert!(
                p.child.wave_ticks > 0
                    && p.child.distance > 0
                    && p.child.radius >= 0
                    && p.child.damage >= 0
            );
            assert!(p.dummy_lifetime_ticks >= p.child.wave_ticks && p.cast_point_distance > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomGroundEnemyUnit
            );
        }
        AbilityEffect::NativeDebuff(p) => {
            assert!(p.duration_ticks > 0 && p.hero_duration_ticks > 0);
        }
        AbilityEffect::Curse(p) => {
            assert!(p.chance_to_miss_per_10k <= 10_000 && p.duration_ticks > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NearestEnemyInCombat
            );
        }
        AbilityEffect::ParasiteAreaBuff(p) => {
            assert!(p.radius >= 0 && p.duration_ticks > 0);
        }
        AbilityEffect::TravellingManaClap(p) => {
            assert!(p.callback_speed_per_second > 0 && p.mover_duration_ticks > 0);
            assert!(p.maximum_mana_burn >= 0 && p.burn_radius >= 0);
        }
        AbilityEffect::NativeSummon(p) => {
            assert!(p.count > 0 && p.duration_ticks > 0);
        }
        AbilityEffect::BuildingAcidBomb(p) => {
            assert!(p.child.speed_per_tick > 0 && p.child.range > 0 && p.child.radius >= 0);
            assert!(
                p.child.interval_ticks > 0
                    && p.child.duration_ticks > 0
                    && p.child.hero_duration_ticks > 0
            );
            assert!(p.child.primary_damage >= 0 && p.child.secondary_damage >= 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::BuildingBolt(p) => {
            assert!(p.bolt.speed_per_tick > 0 && p.child_range > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::BuildingEraser(p) => assert!(p.damage > 0 && p.shield_minimum_level > 0),
        AbilityEffect::BuildingMagicRuin(p) => {
            assert!(p.outcome_count == 10 && p.control_dummy_ticks.iter().all(|&t| t > 0));
        }
        AbilityEffect::BuildingFlameStrike(p) => {
            for option in p.options {
                assert!(
                    option.radius > 0
                        && option.full_interval_millis > 0
                        && option.half_interval_millis > 0
                        && option.maximum_damage > 0
                );
            }
        }
        AbilityEffect::BuildingAttackProxy(p) => {
            assert!(p.damage >= 0 && p.speed_per_tick > 0 && p.range > 0 && p.dummy_ticks > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::Hailstone(p) => {
            assert!(p.damage >= 0 && p.speed_per_tick > 0 && p.full_radius >= 0 && p.range > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::WorldFreezer(p) => {
            assert_eq!(p.map_version, crate::MapVersion::CASTLE_FIGHT_9_27);
            assert!(p.interval_millis > 0 && p.step > 0 && p.fire_cooldown_millis > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NativeBuildingSpellTrigger
            );
        }
        AbilityEffect::Snowfall { map_version } => {
            assert_eq!(map_version, crate::MapVersion::CASTLE_FIGHT_9_27);
        }
        AbilityEffect::SolarStrike {
            profile,
            radius,
            maximum_targets,
        } => {
            assert!(profile.speed_per_tick > 0 && profile.damage >= 0 && profile.stun_ticks > 0);
            assert!(radius > 0 && maximum_targets > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::FlyingEnemyUnit
            );
        }
        AbilityEffect::PhoenixFire(profile) => {
            assert!(
                profile.speed_per_tick > 0 && profile.damage >= 0 && profile.damage_per_second >= 0
            );
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomEnemyUnitOrBuilding
            );
        }
        AbilityEffect::HealingWave(profile) => {
            assert!(
                profile.healing > 0
                    && profile.trigger_healing >= 0
                    && profile.jump_radius >= 0
                    && profile.recovery_ticks > 0
            );
            assert!((1..=MAX_BOUNCE_HITS as u8).contains(&profile.maximum_targets));
            assert!(profile.retention_per_10k <= 10_000);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::WoundedFriendlyUnit
            );
        }
        AbilityEffect::Hex { profile } => {
            assert!(profile.duration_ticks > 0 && profile.hero_duration_ticks > 0);
            assert!(profile.initial_reengage_ticks > 0);
            for form in [profile.ground, profile.air] {
                assert!(form.speed_per_tick >= 0 && form.collision_radius > 0);
            }
        }
        AbilityEffect::MasterProjectiles(version) => {
            crate::native_effects::master_projectile_profile_for_version(version).validate();
        }
        AbilityEffect::Damage { amount } => assert!(amount >= 0),
        AbilityEffect::Stun { duration_ticks } => assert!(duration_ticks > 0),
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier: _,
            percent_delta,
            duration_ticks,
        } => {
            assert!((-100..=1_000).contains(&percent_delta));
            assert_ne!(percent_delta, 0);
            assert!(duration_ticks > 0);
        }
        AbilityEffect::FrostNova {
            radius,
            primary_damage,
            area_damage,
            duration_ticks,
            hero_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
            targets,
            ..
        } => {
            assert!(
                radius > 0
                    && primary_damage >= 0
                    && area_damage >= 0
                    && duration_ticks > 0
                    && hero_duration_ticks > 0
            );
            assert!(
                (-100..=0).contains(&movement_percent_delta)
                    && (-100..=0).contains(&attack_speed_percent_delta)
            );
            assert_eq!(targets, AttackTargetMask::AIR_AND_GROUND);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomGroundEnemyUnit
            );
        }
        AbilityEffect::AreaDebuff {
            radius,
            armor_delta_per_100,
            damage_delta_per_10k,
            duration_ticks,
            hero_duration_ticks,
            targets,
            ..
        } => {
            assert!(
                radius > 0
                    && armor_delta_per_100 <= 0
                    && (-10_000..=0).contains(&damage_delta_per_10k)
                    && duration_ticks > 0
                    && hero_duration_ticks > 0
            );
            assert_eq!(targets, AttackTargetMask::AIR_AND_GROUND);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomGroundEnemyUnit
            );
        }
        AbilityEffect::AreaStun {
            damage,
            radius,
            stun_ticks,
            hero_stun_ticks,
            targets,
            ..
        } => {
            assert!(damage >= 0 && radius > 0 && stun_ticks > 0 && hero_stun_ticks > 0);
            assert_eq!(targets, AttackTargetMask::GROUND_UNITS);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomGroundEnemyUnit
            );
        }
        AbilityEffect::AreaDamage { amount, radius, .. } => {
            assert!(amount >= 0);
            assert!(radius >= 0);
            assert_ne!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::AllEnemyUnits,
                "area damage requires a selected enemy unit to trigger the cast"
            );
        }
        AbilityEffect::FaerieFire {
            armor_reduction_per_100,
            duration_ticks,
            hero_duration_ticks,
            ..
        } => {
            assert!(armor_reduction_per_100 > 0 && duration_ticks > 0 && hero_duration_ticks > 0);
            assert!(matches!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::NearestEnemyInCombat | AbilityTargetPolicy::RandomEnemyDebuff
            ));
        }
        AbilityEffect::FrostArmor {
            native_buff: _,
            modifier: _,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            assert!(armor_bonus_per_100 > 0);
            assert!(armor_duration_ticks > 0);
            assert!(slow_duration_ticks > 0);
            assert!((-100..0).contains(&movement_percent_delta));
            assert!((-100..0).contains(&attack_speed_percent_delta));
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RecentlyAttackedFriendlyUnit
            );
        }
        AbilityEffect::HolyAid {
            native_buff: _,
            healing,
            armor_bonus_per_100,
            regeneration_per_second_per_10k: _,
            duration_ticks,
            permanent_max_health_bonus,
            resurrection_count,
            resurrection_radius,
            resurrection_mana_cost,
            resurrection_cooldown_ticks,
            resurrection_delay_ticks,
            modifier: _,
        } => {
            assert!(healing >= 0);
            assert!(armor_bonus_per_100 >= 0);
            assert!(duration_ticks > 0);
            assert!(permanent_max_health_bonus >= 0);
            assert!(
                resurrection_count == 0
                    || (resurrection_radius > 0
                        && resurrection_mana_cost > 0
                        && resurrection_cooldown_ticks > 0
                        && resurrection_delay_ticks > 0)
            );
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::WoundedFriendlyUnit
            );
        }
        AbilityEffect::Prayer {
            native_buff: _,
            healing,
            mana_restored,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            duration_ticks,
            radius,
            resurrection_count,
            resurrection_radius,
            modifier: _,
        } => {
            assert!(healing >= 0 && mana_restored >= 0);
            assert!(armor_bonus_per_100 >= 0);
            assert!(damage_bonus_per_10k <= 10_000);
            assert!(duration_ticks > 0 && radius > 0);
            assert!(resurrection_count == 0 || resurrection_radius > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::WoundedFriendlyUnit
            );
        }
        AbilityEffect::HolyFervour {
            radius,
            duration_ticks,
            ..
        } => {
            assert!(radius > 0 && duration_ticks > 0);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::AllFriendlyUnits
            );
        }
        AbilityEffect::Purification {
            damage,
            radius,
            consume_radius,
            reveal_radius,
            reveal_duration_ticks,
        } => {
            assert!(
                damage > 0
                    && radius > 0
                    && consume_radius > 0
                    && reveal_radius > 0
                    && reveal_duration_ticks > 0
            );
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomCorpse
            );
        }
        AbilityEffect::ArtilleryBombardment {
            min_damage,
            max_damage,
            speed_per_tick,
            splash,
            ..
        } => {
            assert!(min_damage > 0 && max_damage >= min_damage && speed_per_tick > 0);
            assert!(splash.full_radius <= splash.medium_radius);
            assert!(splash.medium_radius <= splash.outer_radius);
            assert_eq!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::RandomEnemyBasePoint
            );
        }
    }
}

fn validate_unit_template(unit: crate::components::UnitTemplate) {
    assert!(unit.health > 0);
    validate_attack_profile(unit.attack);
    assert!(unit.movement.speed_per_tick >= 0);
}

fn validate_collision_radius(collision_radius: CollisionRadius) {
    assert!(collision_radius.0 >= 0);
}

fn validate_unit_spawn(unit: UnitSpawn) {
    validate_unit_template(crate::components::UnitTemplate {
        health: unit.health,
        attack: unit.attack,
        movement: unit.movement,
    });
    assert!(unit.team.0 < 2, "verification slice supports two teams");
}

fn validate_corpse_profile(corpse: CorpseProfile) {
    assert!(
        corpse.decay_start_ticks > 0,
        "corpse decay-start delay must be positive"
    );
    if let Some(lifetime_ticks) = corpse.lifetime_ticks {
        assert!(lifetime_ticks > 0, "corpse lifetime must be positive");
        assert!(
            corpse.decay_start_ticks < lifetime_ticks,
            "corpse decay-start delay must precede expiry"
        );
    }
}

fn preferred_spawn_cell(team: Team, footprint: BuildingFootprint) -> NavCell {
    let y = footprint.min_y + i32::from(footprint.height / 2);
    if team.0 == 0 {
        NavCell::new(footprint.max_x() + 1, y)
    } else {
        NavCell::new(footprint.min_x - 1, y)
    }
}

fn spiral_cells(center: NavCell, radius: u16) -> impl Iterator<Item = NavCell> {
    let radius = i32::from(radius);
    std::iter::once(center).chain((1..=radius).flat_map(move |r| {
        let min_x = center.x - r;
        let max_x = center.x + r;
        let min_y = center.y - r;
        let max_y = center.y + r;
        let top = (min_x..=max_x).map(move |x| NavCell::new(x, min_y));
        let right = (min_y + 1..=max_y).map(move |y| NavCell::new(max_x, y));
        let bottom = (min_x..max_x).rev().map(move |x| NavCell::new(x, max_y));
        let left = (min_y + 1..max_y)
            .rev()
            .map(move |y| NavCell::new(min_x, y));
        top.chain(right).chain(bottom).chain(left)
    }))
}

fn corpse_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<CorpseView> {
    let corpse = *entity.get::<Corpse>()?;
    Some(CorpseView {
        vertex_rgba: corpse.vertex_rgba,
        id: *entity.get::<SimId>()?,
        position: entity.get::<Position>()?.0,
        source_unit: corpse.source_unit,
        source_owner: corpse.source_owner,
        source_team: corpse.source_team,
        definition: corpse.definition,
        created_tick: corpse.created_tick,
        decay_start_tick: corpse.decay_start_tick,
        expires_tick: corpse.expires_tick,
    })
}

fn projectile_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<ProjectileView> {
    if let Some(crate::components::NativeAction::Parasite(state)) =
        entity.get::<crate::components::NativeAction>()
        && state.buff_until_tick.is_none()
    {
        let ability = state.ability?;
        return Some(ProjectileView {
            id: *entity.get::<SimId>()?,
            source: state.source,
            ability: Some(ability),
            launch_position: state.position,
            launch_tick: state.position_tick,
            impact_tick: state.position_tick + 1,
            kind: ProjectileViewKind::NativeCarrierBolt {
                target: state.target,
                ability,
            },
        });
    }

    if let Some(crate::components::NativeAction::TravellingManaClap(state)) =
        entity.get::<crate::components::NativeAction>()
        && state.profile.mover_has_visible_model
        && state.position_tick < state.created_tick + u64::from(state.profile.mover_duration_ticks)
    {
        let dx = state.destination.x - state.origin.x;
        let dy = state.destination.y - state.origin.y;
        return Some(ProjectileView {
            id: *entity.get::<SimId>()?,
            source: state.source,
            ability: Some(state.profile.clap.ability),
            launch_position: state.position,
            launch_tick: state.position_tick,
            impact_tick: state.position_tick + 1,
            kind: ProjectileViewKind::NativeMover {
                rawcode: state.profile.mover_rawcode,
                owner: state.owner,
                facing_degrees: (dy as f32).atan2(dx as f32).to_degrees() as i16,
                height_world_units: state.profile.mover_height_world_units,
            },
        });
    }

    if let Some(crate::native_carriers::NativeCarrierState::Bolt(bolt)) =
        entity.get::<crate::native_carriers::NativeCarrierState>()
    {
        return Some(ProjectileView {
            id: *entity.get::<SimId>()?,
            source: bolt.visual_source,
            ability: Some(bolt.ability),
            launch_position: bolt.position,
            launch_tick: bolt.position_tick,
            impact_tick: bolt.impact_tick,
            kind: ProjectileViewKind::NativeCarrierBolt {
                target: bolt.target,
                ability: bolt.ability,
            },
        });
    }
    let id = *entity.get::<SimId>()?;
    if let Some(crate::components::NativeAction::AcidBomb(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        let destination = if projectile.profile.homing {
            return Some(ProjectileView {
                id,
                source: projectile.source,
                ability: Some(projectile.profile.ability),
                launch_position: projectile.position,
                launch_tick: projectile.position_tick,
                impact_tick: projectile.impact_tick,
                kind: ProjectileViewKind::GuaranteedHit {
                    target: projectile.target,
                },
            });
        } else {
            projectile.destination
        };
        return Some(ProjectileView {
            id,
            source: projectile.source,
            ability: Some(projectile.profile.ability),
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Ballistic {
                source_rawcode: None,
                destination,
                impact_radius: 0,
            },
        });
    }
    if let Some(crate::components::NativeAction::BuildingAttackProxy(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            id,
            source: projectile.source,
            ability: None,
            launch_position: projectile.origin,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Ballistic {
                source_rawcode: Some(projectile.profile.dummy_rawcode),
                destination: projectile.destination,
                impact_radius: 0,
            },
        });
    }
    if let Some(crate::components::NativeAction::Hailstone(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            ability: Some(projectile.profile.ability),
            id,
            source: projectile.source,
            launch_position: projectile.origin,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Ballistic {
                source_rawcode: Some(projectile.profile.dummy_rawcode),
                destination: projectile.destination,
                impact_radius: projectile.profile.full_radius,
            },
        });
    }
    if let Some(crate::components::NativeAction::ConeWave(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            ability: Some(projectile.profile.ability),
            id,
            source: projectile.source,
            launch_position: projectile.origin,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.launch_tick
                + (projectile.profile.distance as u64)
                    .div_ceil(projectile.profile.speed_per_tick as u64)
                    .max(1),
            kind: ProjectileViewKind::Ballistic {
                source_rawcode: None,
                destination: projectile.destination,
                impact_radius: 0,
            },
        });
    }
    if let Some(crate::components::NativeAction::SphereFlight(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            ability: Some(projectile.ability),
            id,
            source: projectile.source,
            launch_position: projectile.origin,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::SphereFlight {
                destination: projectile.destination,
                source_movement: projectile.source_movement,
                target_movement: projectile.target_movement,
            },
        });
    }
    if let Some(crate::components::NativeAction::Ensnare(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            ability: Some(projectile.profile.ability),
            id,
            source: projectile.source,
            launch_position: projectile.position,
            launch_tick: projectile.position_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::GuaranteedHit {
                target: projectile.target,
            },
        });
    }
    if let Some(crate::components::NativeAction::Bolt(projectile)) =
        entity.get::<crate::components::NativeAction>()
    {
        return Some(ProjectileView {
            ability: Some(projectile.profile.ability),
            id,
            source: projectile.source,
            launch_position: projectile.position,
            launch_tick: projectile.position_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::GuaranteedHit {
                target: projectile.target,
            },
        });
    }
    if let Some(projectile) = entity.get::<GuaranteedHitProjectile>() {
        return Some(ProjectileView {
            ability: None,
            id,
            source: projectile.source,
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::GuaranteedHit {
                target: projectile.target,
            },
        });
    }
    if let Some(projectile) = entity.get::<ReflectedProjectile>() {
        return Some(ProjectileView {
            ability: None,
            id,
            source: projectile.original_source,
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Reflected {
                target: projectile.target,
                reflector: projectile.reflector,
            },
        });
    }
    if let Some(projectile) = entity.get::<LineProjectile>() {
        return Some(ProjectileView {
            ability: None,
            id,
            source: projectile.source,
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Line {
                source_rawcode: projectile.source_rawcode,
                destination: projectile.destination,
                spill_origin: projectile.spill_origin,
                primary_target: projectile.target,
                primary_impact_tick: projectile.primary_impact_tick,
            },
        });
    }
    if let Some(projectile) = entity.get::<BallisticProjectile>() {
        return Some(ProjectileView {
            ability: None,
            id,
            source: projectile.source,
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Ballistic {
                source_rawcode: None,
                destination: projectile.destination,
                impact_radius: projectile.impact_radius,
            },
        });
    }
    let projectile = *entity.get::<BounceProjectile>()?;
    Some(ProjectileView {
        ability: None,
        id,
        source: projectile.source,
        launch_position: projectile.launch_position,
        launch_tick: projectile.launch_tick,
        impact_tick: projectile.impact_tick,
        kind: ProjectileViewKind::Bounce {
            source_rawcode: projectile.source_rawcode,
            target: projectile.target,
            bounce_index: projectile.bounce_index,
            remaining_bounces: projectile.remaining_bounces,
        },
    })
}

fn builder_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<BuilderView> {
    entity.get::<Builder>()?;
    let state = *entity.get::<BuilderState>()?;
    let queue = entity.get::<BuilderOrderQueue>();
    let build = queue
        .and_then(|queue| queue.0.front())
        .and_then(|order| order.build());
    Some(BuilderView {
        id: *entity.get::<SimId>()?,
        owner: entity.get::<Owner>()?.0,
        team: *entity.get::<Team>()?,
        position: entity.get::<Position>()?.0,
        profile: *entity.get::<BuilderProfile>()?,
        configuration: entity.get::<BuilderConfiguration>()?.clone(),
        destination: state.destination,
        follow_target: state.follow_target,
        repair_target: state.repair_target,
        build_footprint: build.map(|order| order.building.footprint),
        build_content: build.and_then(|order| order.properties.content),
        orders: queue
            .map(|queue| {
                queue
                    .0
                    .iter()
                    .map(|order| match order {
                        BuilderOrder::Build(build) => BuilderOrderView::Build {
                            content: build.properties.content,
                            footprint: build.building.footprint,
                            economy: build.properties.economy,
                        },
                        BuilderOrder::Move(point) => BuilderOrderView::Move(*point),
                        BuilderOrder::Follow(target) => BuilderOrderView::Follow(*target),
                        BuilderOrder::Repair(target) => BuilderOrderView::Repair(*target),
                        BuilderOrder::Blink(point) => BuilderOrderView::Blink(*point),
                        BuilderOrder::Stop => BuilderOrderView::Stop,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        repair_autocast_enabled: state.repair_autocast_enabled,
    })
}

fn unit_view_from_entity(
    entity: bevy_ecs::world::EntityRef<'_>,
    default_collision_radius: i32,
    current_tick: u64,
) -> Option<UnitView> {
    if entity.get::<BuildingFootprint>().is_some() {
        return None;
    }
    let spellcasting = entity.get::<SpellcastingProfile>().copied();
    let ability_state = entity.get::<AutomaticAbilityState>().copied();
    let passive_effects = *entity.get::<PassiveUnitEffects>()?;
    let spawn_tick = entity.get::<SpawnTick>()?.0;
    let control = entity
        .get::<BuildingSpellControl>()
        .copied()
        .unwrap_or_default();
    let hex = control.hex.filter(|hex| current_tick < hex.expires_tick);
    let active_defend_ability = (!control.defend_disabled && hex.is_none())
        .then(|| {
            active_defend_profile(passive_effects, spawn_tick, current_tick)
                .map(|profile| profile.ability)
        })
        .flatten();
    let status = *entity.get::<StatusState>()?;
    let mut attack = native_actions::ensnare::ensnared_attack(
        *entity.get::<AttackProfile>()?,
        *entity.get::<MovementClass>()?,
        status,
    );
    attack.damage = attack
        .damage
        .saturating_add(status.damage_threshold.attack_bonus)
        .saturating_add(status.native_grants.attack_damage())
        .saturating_add(passive_effects.native_attack_damage_bonus());
    Some(UnitView {
        id: *entity.get::<SimId>()?,
        content: entity.get::<ContentIdentity>().copied(),
        owner: entity.get::<Owner>()?.0,
        team: *entity.get::<Team>()?,
        position: entity.get::<Position>()?.0,
        collision_radius: hex.map_or_else(
            || {
                entity
                    .get::<CollisionRadius>()
                    .map_or(default_collision_radius, |radius| radius.0)
            },
            |hex| hex.form.collision_radius,
        ),
        movement_class: *entity.get::<MovementClass>()?,
        mechanical: entity.get::<MechanicalUnit>().is_some(),
        classifications: {
            let mut flags = entity
                .get::<UnitClassifications>()
                .copied()
                .unwrap_or_default();
            flags.invisible |= status.wind_walk_invisible(current_tick);
            flags.invulnerable |= status.impale.is_some_and(|p| current_tick < p.landed_tick);
            flags
        },
        health: entity.get::<Health>()?.current,
        health_max: entity.get::<Health>()?.max,
        attack,
        secondary_attack: entity
            .get::<SecondaryAttackProfile>()
            .copied()
            .map(|mut secondary| {
                secondary.attack.damage = secondary
                    .attack
                    .damage
                    .saturating_add(status.damage_threshold.attack_bonus)
                    .saturating_add(status.native_grants.attack_damage())
                    .saturating_add(passive_effects.native_attack_damage_bonus());
                secondary
            }),
        attack_delivery: attack.delivery,
        attack_targets: *entity.get::<AttackTargetMask>()?,
        damage_type: *entity.get::<DamageType>()?,
        armor: hex.map_or_else(
            || {
                let mut armor = *entity.get::<ArmorProfile>().expect("unit armor");
                armor.armor_points += crate::building_mechanics::shield_armor_for_version(
                    control.shield_level,
                    crate::MapVersion::CASTLE_FIGHT_9_27,
                );
                armor
            },
            |hex| hex.form.armor,
        ),
        target: entity.get::<TargetState>()?.current,
        direct_retaliation_lock: entity.get::<TargetState>()?.direct_retaliation_lock,
        ally_defense_lock: entity.get::<TargetState>()?.ally_defense_lock,
        last_attacker: entity.get::<RetaliationState>()?.attacker,
        last_attacked_tick: entity.get::<RetaliationState>()?.attacked_tick,
        cooldown_remaining: entity.get::<AttackCooldown>()?.remaining,
        stunned_until_tick: status.stunned_until_tick,
        status,
        mana_current: status
            .damage_charge
            .map(damage_charge::mana_current)
            .or_else(|| entity.get::<ManaState>().map(|mana| mana.current)),
        mana_maximum: status
            .damage_charge
            .map(|charge| charge.profile.mana_maximum)
            .or_else(|| spellcasting.map(|profile| profile.mana.maximum))
            .or_else(|| {
                entity.get::<ContentIdentity>().and_then(|c| {
                    crate::content::native_mana_profile_for_version(c.map_version, c.rawcode)
                        .map(|p| p.maximum)
                })
            }),
        ability_ready_tick: ability_state.map(|state| state.ready_tick),
        ability_cast_sequence: ability_state.map(|state| state.cast_sequence),
        active_defend_ability,
        hex,
        negative_building_shield_level: control.shield_level,
    })
}

fn building_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<BuildingView> {
    let production = entity.get::<ProductionProfile>().copied();
    let attack = entity.get::<AttackProfile>().copied();
    let spellcasting = entity.get::<SpellcastingProfile>().copied();
    let ability_state = entity.get::<AutomaticAbilityState>().copied();
    let construction = entity.get::<BuildingConstruction>().copied();
    Some(BuildingView {
        classifications: entity
            .get::<UnitClassifications>()
            .copied()
            .unwrap_or_default(),
        id: *entity.get::<SimId>()?,
        content: entity.get::<ContentIdentity>().copied(),
        owner: entity.get::<Owner>().map(|owner| owner.0),
        team: *entity.get::<Team>()?,
        footprint: *entity.get::<BuildingFootprint>()?,
        health: entity.get::<Health>()?.current,
        health_max: entity.get::<Health>()?.max,
        status: entity.get::<StatusState>().copied().unwrap_or_default(),
        construction_started_tick: construction.map(|state| state.started_tick),
        construction_complete_tick: construction.map(|state| state.complete_tick),
        production,
        production_movement_class: entity.get::<ProductionMovementClass>().map(|class| class.0),
        production_attack_targets: entity
            .get::<ProductionAttackTargets>()
            .map(|targets| targets.0),
        next_spawn_tick: entity
            .get::<ProductionState>()
            .map(|state| state.next_spawn_tick),
        production_queue: entity.get::<ProductionState>().map(|state| state.queued),
        attack,
        secondary_attack: entity.get::<SecondaryAttackProfile>().copied(),
        attack_delivery: attack.map(|attack| attack.delivery),
        attack_targets: entity.get::<AttackTargetMask>().copied(),
        damage_type: *entity.get::<DamageType>()?,
        armor: *entity.get::<ArmorProfile>()?,
        target: entity
            .get::<TargetState>()
            .and_then(|target| target.current),
        cooldown_remaining: entity
            .get::<AttackCooldown>()
            .map(|cooldown| cooldown.remaining),
        mana_current: entity.get::<ManaState>().map(|mana| mana.current),
        mana_maximum: spellcasting
            .map(|profile| profile.mana.maximum)
            .or_else(|| {
                entity.get::<ContentIdentity>().and_then(|c| {
                    crate::content::building_native_mana_profile(c.map_version, c.rawcode)
                        .map(|p| p.maximum)
                })
            }),
        ability_ready_tick: ability_state.map(|state| state.ready_tick),
        ability_cast_sequence: ability_state.map(|state| state.cast_sequence),
        ability_autocast_enabled: ability_state.map(|state| state.autocast_enabled),
        stunned_until_tick: entity
            .get::<StatusState>()
            .map(|state| state.stunned_until_tick),
    })
}

fn find_target_index(
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
    id: SimId,
) -> Option<TargetIndex> {
    find_unit_index(units, id)
        .map(TargetIndex::Unit)
        .or_else(|| find_building_index(buildings, id).map(TargetIndex::Building))
}

fn target_sim_id(
    target: TargetIndex,
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
) -> SimId {
    match target {
        TargetIndex::Unit(index) => units[index].id,
        TargetIndex::Building(index) => buildings[index].id,
    }
}

fn live_target_position(
    target: TargetIndex,
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
    unit_health: &[i32],
    building_health: &[i32],
    navigation_cell_size: i32,
) -> Option<SimPoint> {
    match target {
        TargetIndex::Unit(index) => (unit_health[index] > 0).then_some(units[index].position),
        TargetIndex::Building(index) => (building_health[index] > 0)
            .then(|| footprint_center_point(buildings[index].footprint, navigation_cell_size)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectileDefenseResolution {
    damage: i32,
    reflected: bool,
}

fn active_defend_profile(
    passive_effects: PassiveUnitEffects,
    spawn_tick: u64,
    current_tick: u64,
) -> Option<DefendEffectProfile> {
    passive_effects.iter().find_map(|effect| {
        let PassiveUnitEffect::Defend(profile) = effect else {
            return None;
        };
        let activation_tick = spawn_tick.checked_add(u64::from(profile.activation_delay_ticks))?;
        (current_tick >= activation_tick).then_some(profile)
    })
}

fn scale_damage_per_10k(damage: i32, factor_per_10k: u16) -> i32 {
    if damage <= 0 || factor_per_10k == 0 {
        return 0;
    }
    let scaled = (i64::from(damage) * i64::from(factor_per_10k) + 5_000) / 10_000;
    i32::try_from(scaled.max(1)).expect("scaled damage exceeds i32")
}

fn ranged_projectile_damage_after_defend(
    target: TargetIndex,
    damage: i32,
    completed_tick: u64,
    units: &[UnitSnapshot],
) -> i32 {
    let TargetIndex::Unit(index) = target else {
        return damage;
    };
    let unit = units[index];
    active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
        .map_or(damage, |profile| {
            scale_damage_per_10k(damage, profile.ranged_damage_taken_per_10k)
        })
}

fn resolve_directed_projectile_defense(
    target: TargetIndex,
    projectile_id: SimId,
    damage: i32,
    damage_type: DamageType,
    completed_tick: u64,
    match_seed: u64,
    units: &[UnitSnapshot],
) -> ProjectileDefenseResolution {
    let TargetIndex::Unit(index) = target else {
        return ProjectileDefenseResolution {
            damage,
            reflected: false,
        };
    };
    let unit = units[index];
    let Some(profile) =
        active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
    else {
        return ProjectileDefenseResolution {
            damage,
            reflected: false,
        };
    };

    let reflected = damage_type == DamageType::Pierce
        && profile.deflect_chance_per_10k > 0
        && deterministic_random(
            match_seed,
            completed_tick,
            projectile_id,
            RANDOM_PURPOSE_DEFEND_DEFLECT
                ^ unit.id.0.rotate_left(17)
                ^ u64::from(profile.ability.0),
            0,
        ) % u64::from(ATTACK_PROC_CHANCE_SCALE)
            < u64::from(profile.deflect_chance_per_10k);
    let factor = if reflected {
        profile.deflected_pierce_damage_taken_per_10k
    } else {
        profile.ranged_damage_taken_per_10k
    };
    ProjectileDefenseResolution {
        damage: scale_damage_per_10k(damage, factor),
        reflected,
    }
}

fn native_healing_amount(status: &StatusState, amount: i32, tick: u64) -> i32 {
    status
        .ethereal_at(tick)
        .map_or(amount, |e| scale_damage_per_10k(amount, e.healing_per_10k))
}

fn spell_damage_after_defend(unit: UnitSnapshot, damage: i32, completed_tick: u64) -> i32 {
    let damage = unit.status.ethereal_at(completed_tick).map_or(damage, |e| {
        scale_damage_per_10k(damage, e.magic_damage_per_10k)
    });
    let damage = active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
        .map_or(damage, |profile| {
            scale_damage_per_10k(damage, profile.spell_damage_taken_per_10k)
        });
    let damage = native_kill_effects::damage_after_native_incoming(&unit, damage, completed_tick);
    let damage = scale_damage_per_10k(damage, unit.snow_damage_taken_per_10k);
    unit.effective_passive_effects()
        .fold(damage, |damage, effect| {
            if let PassiveUnitEffect::SpellResistance(profile) = effect {
                scale_damage_per_10k(damage, profile.damage_taken_per_10k)
            } else {
                damage
            }
        })
}

#[derive(Debug, Clone, Copy)]
enum DamageArmorRule {
    Standard,
    IgnorePoints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeaponDamageKind {
    Melee,
    Ranged,
    Cleave,
    Defensive,
    Thorns,
}

#[derive(Debug, Clone, Copy)]
struct WeaponDamageEvent {
    source_id: SimId,
    damage: i32,
    damage_type: DamageType,
    completed_tick: u64,
    kind: WeaponDamageKind,
}

impl Simulation {
    fn apply_damage_to_target(
        &mut self,
        target: TargetIndex,
        event: WeaponDamageEvent,
        buildings_invulnerable: bool,
        state: DamageTargetState<'_>,
    ) -> Option<SimPoint> {
        self.apply_damage_to_target_with_armor_rule(
            target,
            event,
            buildings_invulnerable,
            state,
            DamageArmorRule::Standard,
        )
    }

    fn apply_damage_to_target_with_armor_rule(
        &mut self,
        target: TargetIndex,
        event: WeaponDamageEvent,
        buildings_invulnerable: bool,
        state: DamageTargetState<'_>,
        armor_rule: DamageArmorRule,
    ) -> Option<SimPoint> {
        let WeaponDamageEvent {
            source_id,
            damage,
            damage_type,
            completed_tick,
            kind,
        } = event;
        match target {
            TargetIndex::Unit(index) => {
                if state.unit_health[index] <= 0
                    || state.units[index].classifications.invulnerable
                    || (kind == WeaponDamageKind::Thorns
                        && state.units[index].classifications.spell_immune)
                {
                    return None;
                }
                let damage = if matches!(
                    kind,
                    WeaponDamageKind::Melee | WeaponDamageKind::Ranged | WeaponDamageKind::Cleave
                ) && let Some(ethereal) =
                    state.units[index].status.ethereal_at(completed_tick)
                {
                    if damage_type == DamageType::Magic {
                        scale_damage_per_10k(damage, ethereal.magic_damage_per_10k)
                    } else {
                        0
                    }
                } else {
                    damage
                };
                let adjusted_damage = state.damage_rules.apply_attack_with_armor_per_100(
                    damage,
                    damage_type,
                    state.units[index].armor.armor_type,
                    match armor_rule {
                        DamageArmorRule::Standard => {
                            effective_armor_points_per_100(&state.units[index])
                        }
                        DamageArmorRule::IgnorePoints => 0,
                    },
                );
                let shell = spiked_shell::damage(
                    &state.units[index],
                    adjusted_damage,
                    kind == WeaponDamageKind::Melee,
                );
                let adjusted_damage =
                    if matches!(kind, WeaponDamageKind::Melee | WeaponDamageKind::Ranged) {
                        night_elf::hardened_skin_after_armor(
                            &mut state.units[index],
                            shell.incoming,
                            source_id,
                            completed_tick,
                        )
                    } else {
                        shell.incoming
                    };
                let event_damage =
                    if matches!(kind, WeaponDamageKind::Defensive | WeaponDamageKind::Thorns) {
                        spell_damage_after_defend(
                            state.units[index],
                            adjusted_damage,
                            completed_tick,
                        )
                    } else {
                        scale_damage_per_10k(
                            native_kill_effects::damage_after_native_incoming(
                                &state.units[index],
                                adjusted_damage,
                                completed_tick,
                            ),
                            state.units[index].snow_damage_taken_per_10k,
                        )
                    };
                let event_damage = source_damage_runtime::attack_damage_for_recipient(
                    source_id,
                    &state.units[index],
                    event_damage,
                    damage_type,
                    state.units,
                    &self.retired_damage_sources,
                );
                self.observe_building_damage_proc(
                    source_id,
                    &state.units[index],
                    state.unit_positions[index],
                    completed_tick,
                );
                let dummy_killed = source_damage_runtime::dispatch(
                    source_id,
                    index,
                    event_damage,
                    source_damage_runtime::SourceDamageState {
                        units: state.units,
                        health: state.unit_health,
                        rules: state.damage_rules,
                        tick: completed_tick,
                    },
                    &self.retired_damage_sources,
                );
                if damage_type != DamageType::Spells {
                    self.observe_damage_retaliation(
                        source_id,
                        index,
                        completed_tick,
                        state.units,
                        state.unit_health,
                        state.unit_positions,
                    );
                }
                damage_thresholds::observe_damage_event(
                    &mut state.units[index],
                    state.unit_health[index],
                    event_damage,
                );
                if state.units[index].classifications.combat_sapper
                    && let Some(source_index) = find_unit_index(state.units, source_id)
                    && state.unit_health[source_index] > 0
                {
                    mirror_images::observe_source_damage(
                        &mut state.units[source_index],
                        completed_tick,
                        state.match_seed,
                    );
                }
                let after = state.unit_health[index]
                    .checked_sub(event_damage)
                    .expect("unit damage arithmetic overflowed validated bounds");
                death_healing::commit_split_health_from(
                    state.units,
                    state.unit_health,
                    Some(state.unit_positions),
                    index,
                    after,
                    Some(source_id),
                );
                let source_index = find_unit_index(state.units, source_id);
                if state.unit_health[index] <= 0
                    && !dummy_killed
                    && state.units[index].classifications.combat_sapper
                    && let Some(source_index) = source_index
                    && state.unit_health[source_index] > 0
                {
                    native_kill_effects::activate_kill_berserk(
                        &mut state.units[source_index],
                        completed_tick,
                    );
                }
                let source_is_unit = source_index.is_some();
                let recorded_attacker_is_building =
                    state.attackers_this_tick[index].is_some_and(|attacker| {
                        find_building_index(state.buildings, attacker).is_some()
                    });
                if state.attackers_this_tick[index].is_none()
                    || (source_is_unit && recorded_attacker_is_building)
                {
                    state.attackers_this_tick[index] = Some(source_id);
                }
                state.next_defense_alerts.push(DefenseAlert {
                    victim_id: state.units[index].id,
                    victim_team: state.units[index].team,
                    victim_position: state.unit_positions[index],
                    attacker_id: source_id,
                    attacked_tick: completed_tick,
                });
                if shell.count > 0
                    && let Some(source_index) = find_unit_index(state.units, source_id)
                {
                    let shell_source = state.units[index].id;
                    for returned in shell.returned[..shell.count].iter().copied() {
                        self.apply_damage_to_target_with_armor_rule(
                            TargetIndex::Unit(source_index),
                            WeaponDamageEvent {
                                source_id: shell_source,
                                damage: returned.damage,
                                damage_type: DamageType::Spells,
                                completed_tick,
                                kind: if returned.thorns {
                                    WeaponDamageKind::Thorns
                                } else {
                                    WeaponDamageKind::Defensive
                                },
                            },
                            buildings_invulnerable,
                            DamageTargetState {
                                match_seed: state.match_seed,
                                damage_rules: state.damage_rules,
                                units: state.units,
                                buildings: state.buildings,
                                unit_positions: state.unit_positions,
                                unit_health: state.unit_health,
                                building_health: state.building_health,
                                attackers_this_tick: state.attackers_this_tick,
                                next_defense_alerts: state.next_defense_alerts,
                                navigation_cell_size: state.navigation_cell_size,
                            },
                            if returned.thorns {
                                DamageArmorRule::IgnorePoints
                            } else {
                                DamageArmorRule::Standard
                            },
                        );
                    }
                }
                if kind == WeaponDamageKind::Melee
                    && !state.units[index].classifications.spell_immune
                    && let Some(source_index) = source_index
                    && state.unit_health[source_index] > 0
                {
                    let factor = state.units[source_index].status.armor_modifiers
                        [..usize::from(state.units[source_index].status.armor_modifier_count)]
                        .iter()
                        .filter(|m| completed_tick < m.expires_tick)
                        .map(|m| m.melee_life_steal_per_10k)
                        .max()
                        .unwrap_or(0);
                    let healing = (i64::from(event_damage.max(0)) * i64::from(factor) / 10_000)
                        .min(i64::from(
                            state.units[source_index].health_max - state.unit_health[source_index],
                        ));
                    let health = state.unit_health[source_index]
                        + i32::try_from(healing).expect("life stolen fits missing health");
                    state.unit_health[source_index] = health;
                    state.units[source_index].health = health;
                }
                Some(state.unit_positions[index])
            }
            TargetIndex::Building(index) => {
                if state.building_health[index] <= 0 {
                    return None;
                }
                if !buildings_invulnerable {
                    let adjusted_damage = state.damage_rules.apply_attack(
                        damage,
                        damage_type,
                        match armor_rule {
                            DamageArmorRule::Standard => state.buildings[index].armor,
                            DamageArmorRule::IgnorePoints => {
                                ArmorProfile::new(state.buildings[index].armor.armor_type, 0)
                            }
                        },
                    );
                    state.building_health[index] = state.building_health[index]
                        .checked_sub(scale_damage_per_10k(
                            adjusted_damage,
                            state.buildings[index].snow_damage_taken_per_10k,
                        ))
                        .expect("building damage arithmetic overflowed validated bounds");
                }
                death_healing::refresh_support(
                    state.units,
                    state.buildings,
                    Some(state.building_health),
                );
                Some(footprint_center_point(
                    state.buildings[index].footprint,
                    state.navigation_cell_size,
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingAttackEffectSource {
    id: SimId,
    position: SimPoint,
    team: Team,
}

struct PendingAttackEffectState<'a> {
    navigation_cell_size: i32,
    completed_tick: u64,
    units: &'a mut [UnitSnapshot],
    unit_health: &'a mut [i32],
    buildings: &'a mut [BuildingSnapshot],
    building_health: &'a mut [i32],
    retired_sources: &'a BTreeMap<SimId, source_damage_runtime::RetiredDamageSource>,
    damage_rules: DamageRules,
}

#[derive(Debug, Clone, Default)]
struct PendingAttackEffectResult {
    cone_wave: Option<crate::components::ConeWaveState>,
    poison: Option<crate::PoisonProfile>,
    fork_events: Vec<ChainLightningEvent>,
    native_debuff_casts: [Option<AbilityCastEvent>; 2],
    chain_event: Option<ChainLightningEvent>,
    chain_state: Option<ChainLightningState>,
}

fn apply_pending_attack_effects(
    target: TargetIndex,
    effects: PendingAttackEffects,
    source: PendingAttackEffectSource,
    state: PendingAttackEffectState<'_>,
) -> PendingAttackEffectResult {
    let PendingAttackEffectState {
        navigation_cell_size,
        completed_tick,
        units,
        unit_health,
        buildings,
        building_health,
        damage_rules,
        retired_sources,
    } = state;
    lava_spawns::observe_landed_attack(source.id, target, completed_tick, units, buildings);
    let TargetIndex::Unit(index) = target else {
        let TargetIndex::Building(index) = target else {
            unreachable!()
        };
        if let Some(TriggeredAttackEffect::ForkedLightning(profile)) = effects.triggered_spell {
            let fork_events = forked_lightning::resolve(
                target,
                profile,
                source,
                &mut PendingAttackEffectState {
                    navigation_cell_size,
                    completed_tick,
                    units,
                    unit_health,
                    buildings,
                    building_health,
                    damage_rules,
                    retired_sources,
                },
            );
            return PendingAttackEffectResult {
                fork_events,
                ..Default::default()
            };
        }
        if let Some(TriggeredAttackEffect::EntanglingRoots(profile)) = effects.triggered_spell
            && profile.targets.can_target_buildings()
            && building_health[index] > 0
            && !buildings[index].classifications.spell_immune
            && !buildings[index].classifications.invulnerable
            && !(profile.nonhero_only && buildings[index].classifications.uses_hero_spell_rules())
        {
            let status = buildings[index].status.get_or_insert_default();
            apply_roots_status(
                status,
                profile,
                completed_tick,
                buildings[index].classifications.uses_hero_spell_rules(),
                false,
            );
        }
        return PendingAttackEffectResult::default();
    };
    let mut result = PendingAttackEffectResult::default();
    if let Some(TriggeredAttackEffect::ConeWave(profile)) = effects.triggered_spell {
        let origin = units
            .iter()
            .find(|unit| unit.id == source.id)
            .map_or(source.position, |unit| unit.position);
        result.cone_wave = Some(native_actions::cone_wave::new_state(
            source.id,
            source.team,
            origin,
            units[index].position,
            profile,
            completed_tick,
        ));
    }
    if unit_health[index] <= 0 {
        return result;
    }
    if let Some(profile) = effects.frost
        && !units[index].classifications.spell_immune
        && !units[index].classifications.invulnerable
        && profile
            .targets
            .can_target_unit(units[index].target_movement_class())
    {
        apply_native_frost_attack(&mut units[index], profile, completed_tick);
    }
    if let Some(profile) = effects.feedback
        && !units[index].classifications.spell_immune
        && profile
            .targets
            .can_target_unit(units[index].target_movement_class())
    {
        let drained = if let Some(mana) = units[index].mana_current.as_mut() {
            let drained = (*mana).min(profile.maximum_mana_drained).max(0);
            *mana -= drained;
            drained
        } else if let Some(charge) = units[index].status.damage_charge.as_mut() {
            let drained = damage_charge::mana_current(*charge)
                .min(profile.maximum_mana_drained)
                .max(0);
            damage_charge::restore_mana(charge, -drained);
            drained
        } else {
            0
        };
        // Feedback is separate mana combustion, never part of critical weapon damage.
        let damage = i64::from(drained) * i64::from(profile.damage_per_mana_per_10k) / 10_000
            + if units[index].classifications.summoned {
                i64::from(profile.summoned_damage)
            } else {
                0
            };
        let damage = damage_rules.apply_spell(
            i32::try_from(damage).expect("Feedback damage overflow"),
            units[index].armor.armor_type,
        );
        let damage = spell_damage_after_defend(units[index], damage, completed_tick);
        let event_damage = damage;
        source_damage_runtime::dispatch(
            source.id,
            index,
            event_damage,
            source_damage_runtime::SourceDamageState {
                units,
                health: unit_health,
                rules: damage_rules,
                tick: completed_tick,
            },
            retired_sources,
        );
        damage_thresholds::observe_damage_event(
            &mut units[index],
            unit_health[index],
            event_damage,
        );
        let after = unit_health[index]
            .checked_sub(event_damage)
            .expect("Feedback health overflow");
        death_healing::commit_split_health_from(
            units,
            unit_health,
            None,
            index,
            after,
            Some(source.id),
        );
    }
    if unit_health[index] <= 0 {
        return PendingAttackEffectResult::default();
    }
    if effects.stun_duration_ticks > 0 {
        let stunned_until_tick = completed_tick
            .checked_add(u64::from(effects.stun_duration_ticks))
            .expect("passive stun expiry tick overflow");
        units[index].status.stunned_until_tick = units[index]
            .status
            .stunned_until_tick
            .max(stunned_until_tick);
    }
    if let Some(effect) = effects.triggered_spell {
        match effect {
            TriggeredAttackEffect::NativeDebuff { parent, profile } => {
                let cast = AbilityCastEvent {
                    source: source.id,
                    ability: parent,
                    target: AbilityCastTarget::Unit(units[index].id),
                    target_position: Some(units[index].position),
                    effect: AbilityEffect::NativeDebuff(profile),
                };
                result.native_debuff_casts[0] = Some(cast);
                if apply_ability_effect_to_unit(
                    &mut units[index],
                    AbilityEffect::NativeDebuff(profile),
                    source.team,
                    completed_tick,
                    damage_rules,
                ) {
                    result.native_debuff_casts[1] = Some(AbilityCastEvent {
                        ability: profile.ability,
                        ..cast
                    });
                }
            }
            TriggeredAttackEffect::ForkedLightning(profile) => {
                result.fork_events = forked_lightning::resolve(
                    target,
                    profile,
                    source,
                    &mut PendingAttackEffectState {
                        navigation_cell_size,
                        completed_tick,
                        units,
                        unit_health,
                        buildings,
                        building_health,
                        damage_rules,
                        retired_sources,
                    },
                );
            }
            TriggeredAttackEffect::ConeWave(_) => {}
            TriggeredAttackEffect::ChainLightning(profile) => {
                let max_targets = usize::from(profile.maximum_targets).min(MAX_BOUNCE_HITS);
                if max_targets > 0 {
                    let adjusted = damage_rules
                        .apply_spell(profile.initial_damage, units[index].armor.armor_type);
                    let adjusted =
                        spell_damage_after_defend(units[index], adjusted, completed_tick);
                    let event_damage = adjusted;
                    source_damage_runtime::dispatch(
                        source.id,
                        index,
                        event_damage,
                        source_damage_runtime::SourceDamageState {
                            units,
                            health: unit_health,
                            rules: damage_rules,
                            tick: completed_tick,
                        },
                        retired_sources,
                    );
                    damage_thresholds::observe_damage_event(
                        &mut units[index],
                        unit_health[index],
                        event_damage,
                    );
                    let after = unit_health[index]
                        .checked_sub(event_damage)
                        .expect("Chain Lightning damage overflow");
                    death_healing::commit_split_health_from(
                        units,
                        unit_health,
                        None,
                        index,
                        after,
                        Some(source.id),
                    );

                    let mut points = [SimPoint::default(); MAX_BOUNCE_HITS + 1];
                    points[0] = source.position;
                    points[1] = units[index].position;
                    result.chain_event = Some(ChainLightningEvent {
                        source: source.id,
                        ability: profile.ability,
                        bounce_index: 0,
                        points,
                        point_count: 2,
                    });

                    if max_targets > 1 {
                        let next_damage = scaled_chain_lightning_damage(
                            profile.initial_damage,
                            profile.damage_reduction_per_10k,
                        );
                        if next_damage > 0 {
                            let mut hit_targets = [SimId(0); MAX_BOUNCE_HITS];
                            hit_targets[0] = units[index].id;
                            result.chain_state = Some(ChainLightningState {
                                source: source.id,
                                source_team: source.team,
                                profile,
                                started_tick: completed_tick,
                                next_jump_index: 1,
                                current_target: units[index].id,
                                last_position: units[index].position,
                                next_damage,
                                hit_targets,
                                hit_count: 1,
                            });
                        }
                    }
                }
            }
            TriggeredAttackEffect::EntanglingRoots(profile) => {
                if !units[index].classifications.spell_immune
                    && !units[index].classifications.invulnerable
                    && profile
                        .targets
                        .can_target_unit(units[index].target_movement_class())
                    && !(profile.nonhero_only
                        && units[index].classifications.uses_hero_spell_rules())
                {
                    let hero = units[index].classifications.uses_hero_spell_rules();
                    apply_roots_status(
                        &mut units[index].status,
                        profile,
                        completed_tick,
                        hero,
                        true,
                    );
                }
            }
        }
    }
    if let Some(profile) = effects.poison
        && unit_health[index] > 0
        && native_actions::poison::eligible(&units[index], profile)
    {
        result.poison = Some(profile);
    }
    result
}

fn apply_roots_status(
    status: &mut StatusState,
    profile: crate::EntanglingRootsEffectProfile,
    tick: u64,
    hero: bool,
    movement: bool,
) {
    let duration = if hero {
        profile.hero_duration_ticks
    } else {
        profile.duration_ticks
    };
    let expiry = tick
        .checked_add(u64::from(duration))
        .expect("roots expiry overflow");
    if movement {
        apply_timed_movement_modifier(status, ModifierId(profile.ability.0), -100, expiry);
    }
    status.rooted_until_tick = status.rooted_until_tick.max(expiry);
    apply_timed_damage_over_time(
        status,
        ModifierId(profile.ability.0),
        profile.damage_per_second,
        u16::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation frequency fits"),
        tick,
        expiry.checked_add(1).expect("roots DOT expiry overflow"),
    );
}

fn apply_frost_attack(
    unit: &mut UnitSnapshot,
    profile: crate::components::FrostAttackEffectProfile,
    tick: u64,
) {
    let duration = if unit.classifications.uses_hero_spell_rules() {
        profile.hero_duration_ticks
    } else {
        profile.duration_ticks
    };
    let until = tick
        .checked_add(u64::from(duration))
        .expect("Frost expiry overflow");
    apply_timed_movement_modifier(
        &mut unit.status,
        ModifierId(profile.ability.0),
        profile.movement_percent_delta,
        until,
    );
    apply_timed_attack_speed_modifier(
        &mut unit.status,
        ModifierId(profile.ability.0),
        profile.attack_speed_percent_delta,
        until,
    );
}

fn ceil_millis_to_ticks(millis: u64) -> u64 {
    millis
        .checked_mul(u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz is positive"))
        .expect("millisecond-to-tick conversion overflow")
        .div_ceil(1_000)
}

fn burning_oil_pulse(
    profile: crate::components::BurningOilEffectProfile,
    pulse_index: u16,
) -> Option<(u32, i32)> {
    if pulse_index == 0 {
        return None;
    }
    let full_count = profile.full_duration_millis / profile.full_interval_millis;
    if pulse_index <= full_count {
        return Some((
            u32::from(pulse_index) * u32::from(profile.full_interval_millis),
            profile.full_damage,
        ));
    }
    let half_index = pulse_index - full_count;
    let half_window = profile
        .total_duration_millis
        .saturating_sub(profile.full_duration_millis);
    let half_count = half_window / profile.half_interval_millis;
    if half_index <= half_count {
        return Some((
            u32::from(profile.full_duration_millis)
                + u32::from(half_index) * u32::from(profile.half_interval_millis),
            profile.half_damage,
        ));
    }
    None
}

fn projectile_travel_ticks(distance_sq: u64, speed_per_tick: i32) -> u64 {
    debug_assert!(speed_per_tick > 0);
    let floor_distance = distance_sq.isqrt();
    let distance = floor_distance + u64::from(floor_distance * floor_distance < distance_sq);
    distance
        .div_ceil(
            u64::try_from(speed_per_tick).expect("validated projectile speed must be positive"),
        )
        .max(1)
}

fn scale_building_health(current: i32, source_max: i32, target_max: i32) -> i32 {
    assert!(source_max > 0 && target_max > 0);
    if current <= 0 {
        return 0;
    }
    let scaled = (i64::from(current) * i64::from(target_max) + i64::from(source_max) - 1)
        / i64::from(source_max);
    i32::try_from(scaled)
        .expect("scaled building health exceeds i32")
        .clamp(1, target_max)
}

fn scaled_bounce_damage(damage: i32, percent: u16) -> i32 {
    debug_assert!((1..=100).contains(&percent));
    i32::try_from(i64::from(damage) * i64::from(percent) / 100)
        .expect("bounce damage scaling overflowed validated bounds")
}

fn scaled_chain_lightning_damage(damage: i32, reduction_per_10k: u16) -> i32 {
    debug_assert!(reduction_per_10k <= ATTACK_PROC_CHANCE_SCALE);
    i32::try_from(
        i64::from(damage) * i64::from(ATTACK_PROC_CHANCE_SCALE - reduction_per_10k)
            / i64::from(ATTACK_PROC_CHANCE_SCALE),
    )
    .expect("Chain Lightning damage scaling overflow")
}

fn chain_lightning_jump_due_tick(started_tick: u64, jump_index: u8) -> u64 {
    debug_assert!(jump_index > 0);
    let elapsed_ticks = u64::from(jump_index)
        .checked_mul(
            u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz must be positive"),
        )
        .expect("Chain Lightning jump timing overflow")
        .div_ceil(4);
    started_tick
        .checked_add(elapsed_ticks)
        .expect("Chain Lightning due tick overflow")
}

fn deterministic_random(seed: u64, tick: u64, entity: SimId, purpose: u64, index: u64) -> u64 {
    let state = splitmix64(seed ^ tick.rotate_left(17));
    let state = splitmix64(state ^ entity.0.rotate_left(31));
    let state = splitmix64(state ^ purpose);
    splitmix64(state ^ index)
}

fn deterministic_ability_target_rank(
    seed: u64,
    caster: SimId,
    ability: AbilityId,
    cast_sequence: u64,
    candidate: SimId,
) -> u64 {
    let state = splitmix64(seed ^ caster.0.rotate_left(31));
    let state = splitmix64(state ^ u64::from(ability.0).rotate_left(17));
    let state = splitmix64(state ^ RANDOM_PURPOSE_ABILITY_TARGET);
    let state = splitmix64(state ^ cast_sequence);
    splitmix64(state ^ candidate.0)
}

fn splitmix64(input: u64) -> u64 {
    let mut value = input.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn target_is_alive(
    id: SimId,
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
    unit_health: &[i32],
    building_health: &[i32],
) -> bool {
    find_unit_index(units, id).is_some_and(|index| unit_health[index] > 0)
        || find_building_index(buildings, id).is_some_and(|index| building_health[index] > 0)
}

fn find_unit_index(units: &[UnitSnapshot], id: SimId) -> Option<usize> {
    units.binary_search_by_key(&id, |unit| unit.id).ok()
}

fn find_building_index(buildings: &[BuildingSnapshot], id: SimId) -> Option<usize> {
    buildings
        .binary_search_by_key(&id, |building| building.id)
        .ok()
}

fn square_i32(value: i32) -> u64 {
    let value = i64::from(value);
    (value * value) as u64
}

fn clamp_point_to_cell_rect(
    point: SimPoint,
    min: NavCell,
    max: NavCell,
    cell_size: i32,
    inset: i32,
) -> Option<SimPoint> {
    debug_assert!(cell_size > 0);
    debug_assert!(inset >= 0);
    let min_x = i64::from(min.x)
        .checked_mul(i64::from(cell_size))?
        .checked_add(i64::from(inset))?;
    let min_y = i64::from(min.y)
        .checked_mul(i64::from(cell_size))?
        .checked_add(i64::from(inset))?;
    let max_x = i64::from(max.x + 1)
        .checked_mul(i64::from(cell_size))?
        .checked_sub(i64::from(inset))?
        .checked_sub(1)?;
    let max_y = i64::from(max.y + 1)
        .checked_mul(i64::from(cell_size))?
        .checked_sub(i64::from(inset))?
        .checked_sub(1)?;
    if min_x > max_x || min_y > max_y {
        return None;
    }
    Some(SimPoint::new(
        i32::try_from(i64::from(point.x).clamp(min_x, max_x)).ok()?,
        i32::try_from(i64::from(point.y).clamp(min_y, max_y)).ok()?,
    ))
}

fn footprints_overlap(a: BuildingFootprint, b: BuildingFootprint) -> bool {
    a.min_x <= b.max_x() && a.max_x() >= b.min_x && a.min_y <= b.max_y() && a.max_y() >= b.min_y
}

fn footprint_contains_footprint(
    container: BuildingFootprint,
    footprint: BuildingFootprint,
) -> bool {
    footprint.min_x >= container.min_x
        && footprint.max_x() <= container.max_x()
        && footprint.min_y >= container.min_y
        && footprint.max_y() <= container.max_y()
}

fn footprint_contains_cell(footprint: BuildingFootprint, cell: NavCell) -> bool {
    cell.x >= footprint.min_x
        && cell.x <= footprint.max_x()
        && cell.y >= footprint.min_y
        && cell.y <= footprint.max_y()
}

fn footprint_center_point(footprint: BuildingFootprint, cell_size: i32) -> SimPoint {
    let min_x = i64::from(footprint.min_x) * i64::from(cell_size);
    let min_y = i64::from(footprint.min_y) * i64::from(cell_size);
    let max_x = i64::from(footprint.max_x() + 1) * i64::from(cell_size);
    let max_y = i64::from(footprint.max_y() + 1) * i64::from(cell_size);
    SimPoint::new(
        i32::try_from((min_x + max_x) / 2).expect("building center x overflow"),
        i32::try_from((min_y + max_y) / 2).expect("building center y overflow"),
    )
}

fn point_attack_envelope_goal(source: SimPoint, target: SimPoint, max_range: i32) -> SimPoint {
    debug_assert!(max_range >= 0);
    let distance_sq = source.distance_sq(target);
    if distance_sq <= square_i32(max_range) {
        return source;
    }
    if max_range == 0 {
        return target;
    }
    let floor_distance = distance_sq.isqrt();
    let distance = floor_distance + u64::from(floor_distance * floor_distance < distance_sq);
    let distance = i64::try_from(distance).expect("attack-envelope distance overflow");
    let dx = i64::from(source.x) - i64::from(target.x);
    let dy = i64::from(source.y) - i64::from(target.y);
    let range = i64::from(max_range);
    SimPoint::new(
        i32::try_from(i64::from(target.x) + dx * range / distance)
            .expect("attack-envelope x overflow"),
        i32::try_from(i64::from(target.y) + dy * range / distance)
            .expect("attack-envelope y overflow"),
    )
}

fn closest_point_on_footprint(
    point: SimPoint,
    footprint: BuildingFootprint,
    cell_size: i32,
) -> SimPoint {
    let min_x = footprint.min_x * cell_size;
    let min_y = footprint.min_y * cell_size;
    let max_x = (footprint.max_x() + 1) * cell_size;
    let max_y = (footprint.max_y() + 1) * cell_size;
    SimPoint::new(point.x.clamp(min_x, max_x), point.y.clamp(min_y, max_y))
}

fn point_to_footprint_distance_sq(
    point: SimPoint,
    footprint: BuildingFootprint,
    cell_size: i32,
) -> u64 {
    let min_x = footprint.min_x * cell_size;
    let min_y = footprint.min_y * cell_size;
    let max_x = (footprint.max_x() + 1) * cell_size;
    let max_y = (footprint.max_y() + 1) * cell_size;
    let closest = SimPoint::new(point.x.clamp(min_x, max_x), point.y.clamp(min_y, max_y));
    point.distance_sq(closest)
}

fn footprint_to_footprint_distance_sq(
    a: BuildingFootprint,
    b: BuildingFootprint,
    cell_size: i32,
) -> u64 {
    let a_min_x = i64::from(a.min_x) * i64::from(cell_size);
    let a_min_y = i64::from(a.min_y) * i64::from(cell_size);
    let a_max_x = i64::from(a.max_x() + 1) * i64::from(cell_size);
    let a_max_y = i64::from(a.max_y() + 1) * i64::from(cell_size);
    let b_min_x = i64::from(b.min_x) * i64::from(cell_size);
    let b_min_y = i64::from(b.min_y) * i64::from(cell_size);
    let b_max_x = i64::from(b.max_x() + 1) * i64::from(cell_size);
    let b_max_y = i64::from(b.max_y() + 1) * i64::from(cell_size);
    let dx = if a_max_x < b_min_x {
        b_min_x - a_max_x
    } else if b_max_x < a_min_x {
        a_min_x - b_max_x
    } else {
        0
    };
    let dy = if a_max_y < b_min_y {
        b_min_y - a_max_y
    } else if b_max_y < a_min_y {
        a_min_y - b_max_y
    } else {
        0
    };
    (dx * dx + dy * dy) as u64
}

fn building_source_query_radius(
    footprint: BuildingFootprint,
    acquisition_range: i32,
    cell_size: i32,
) -> i32 {
    let footprint_extent_cells = i32::from(footprint.width.max(footprint.height));
    acquisition_range
        .checked_add(
            footprint_extent_cells
                .checked_mul(cell_size)
                .expect("building query footprint extent overflow"),
        )
        .expect("building acquisition query radius overflow")
}

fn apply_native_frost_attack(
    unit: &mut UnitSnapshot,
    profile: crate::FrostAttackEffectProfile,
    tick: u64,
) {
    apply_frost_attack(unit, profile, tick);
    status::mark_modifier_undispellable(&mut unit.status, ModifierId(profile.ability.0));
}
