use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use bevy_ecs::{entity::Entity, prelude::World};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

const RANDOM_PURPOSE_BOUNCE_TARGET: u64 = 0x424f_554e_4345_0001;
const RANDOM_PURPOSE_ABILITY_TARGET: u64 = 0x4142_494c_4954_0001;
const RANDOM_PURPOSE_UPHILL_MISS: u64 = 0x5550_4849_4c4c_0001;
const RANDOM_PURPOSE_ATTACK_PROC: u64 = 0x4154_4b50_524f_4301;
const RANDOM_PURPOSE_DEFEND_DEFLECT: u64 = 0x4445_4645_4e44_0001;
pub const UPHILL_MISS_CHANCE_SCALE: u16 = 10_000;
/// Logical checksum encoding revision. Bump when the canonical projection changes incompatibly.
pub const CANONICAL_CHECKSUM_SCHEMA_VERSION: u32 = 5;
const ATTACK_PROC_CHANCE_SCALE: u16 = 10_000;
const DIRECT_RETALIATION_RANGE_MULTIPLIER: i32 = 3;
const AVOIDANCE_CLEAR_TICKS: u8 = 8;

mod abilities;
mod builder;
mod canonical;
mod combat;
mod construction;
mod economy;
mod movement;
mod projectiles;
mod snapshot;
mod status;
mod targeting;

use canonical::{CanonicalMatchState, canonical_checksum, canonical_configuration_identity};
use combat::AttackResolutionContext;
use projectiles::{BallisticImpactContext, ProjectileWorldChanges, TargetProjectileContext};

pub use snapshot::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, SimulationSnapshot, SnapshotRestoreError,
};
use status::{
    apply_ability_effect_to_unit, apply_melee_reactive_armor_effects, apply_timed_damage_over_time,
    apply_timed_movement_modifier, effective_armor_points_per_100, effective_attack_cooldown_ticks,
    purge_expired_status_modifiers, resolve_periodic_unit_statuses,
};

use crate::{
    components::{
        AbilityEffect, AbilityId, AbilityTargetPolicy, AttackCooldown, AttackDelivery,
        AttackProfile, AttackSequence, AttackTargetMask, AutomaticAbilityProfile,
        AutomaticAbilityState, BallisticProjectile, BounceProjectile, BuildTimeTicks, Builder,
        BuilderBuildOrder, BuilderConfiguration, BuilderLocomotion, BuilderProfile, BuilderSpawn,
        BuilderState, BuildingConstruction, BuildingFootprint, BuildingGameplayProperties,
        BuildingRuntimeState, BuildingSpawn, BuildingUpgradeSource, BurningOilZone,
        ChainLightningState, CollisionRadius, ContentIdentity, Corpse, CorpseDefinitionId,
        CorpseProducer, CorpseProfile, DefendEffectProfile, GameplayBundleIdentity,
        GuaranteedHitProjectile, Health, HealthRegeneration, MAX_BOUNCE_HITS,
        MAX_TIMED_ARMOR_MODIFIERS, MAX_TIMED_ATTACK_SPEED_MODIFIERS, MAX_TIMED_DAMAGE_OVER_TIME,
        MAX_TIMED_MOVEMENT_MODIFIERS, ManaState, MechanicalUnit, ModifierId, MovementClass,
        MovementProfile, NavigationGoal, NavigationState, Owner, PassiveUnitEffect,
        PassiveUnitEffects, PendingAttackEffects, PlayerId, Position, ProductionArmorProfile,
        ProductionAttackTargets, ProductionCollisionRadius, ProductionContentIdentity,
        ProductionCorpseProfile, ProductionDamageType, ProductionHealthRegeneration,
        ProductionMovementClass, ProductionPassiveEffects, ProductionProfile,
        ProductionSpellcastingProfile, ProductionState, ProductionUnitRepairMetadata,
        ReflectedProjectile, RepairTimeTicks, ResolvedUnitDefinition, RetaliationState, SimId,
        SpawnTick, SpellcastingProfile, StatusState, TargetState, Team, TimedArmorModifier,
        TimedAttackSpeedModifier, TimedDamageOverTime, TriggeredAttackEffect,
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
    /// Optional standard-lane ingress guidance for targetless units. Generic maps leave this unset.
    pub targetless_lane: Option<TargetlessLane>,
    pub team_objective: [SimPoint; 2],
    pub economy: EconomyRules,
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
            targetless_lane: None,
            team_objective: [
                SimPoint::new(120 * SUBUNITS_PER_WORLD_UNIT, 0),
                SimPoint::new(0, 0),
            ],
            economy: EconomyRules::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerConfig {
    pub id: PlayerId,
    pub team: Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerConnectionStatus {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    Victory(Team),
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchLifecycle {
    Running,
    PausedForDisconnect {
        disconnected_teams_mask: u8,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AbilityCastTarget {
    Unit(SimId),
    AllEnemyUnits,
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
    GuaranteedHit {
        target: SimId,
    },
    Ballistic {
        destination: SimPoint,
        impact_radius: i32,
    },
    Bounce {
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
    pub id: SimId,
    pub source: SimId,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
    pub kind: ProjectileViewKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpseView {
    pub id: SimId,
    pub position: SimPoint,
    pub source_unit: SimId,
    pub source_owner: PlayerId,
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
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
    pub health: i32,
    pub health_max: i32,
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
    pub repair_autocast_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingView {
    pub id: SimId,
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
    pub stunned_until_tick: Option<u64>,
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
    SourceNotFound,
    NotOwner,
    SourceUnderConstruction,
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
    last_chain_lightnings: Vec<ChainLightningEvent>,
    players: Vec<PlayerState>,
    lifecycle: MatchLifecycle,
    team_objectives: [Option<SimId>; 2],
    next_tick: u64,
    next_id: u64,
    configuration_identity: u64,
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

        Self {
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
            last_chain_lightnings: Vec::new(),
            players,
            lifecycle: MatchLifecycle::Running,
            team_objectives: [None, None],
            next_tick: 0,
            next_id: 1,
            configuration_identity,
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
        self.lifecycle = MatchLifecycle::Finished {
            outcome,
            finished_tick: self.next_tick.saturating_sub(1),
        };
        true
    }

    #[must_use]
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

    /// Applies direct developer-tool damage to every live combat unit.
    ///
    /// The query intentionally matches the normal unit archetype via `MovementProfile`. Buildings
    /// and builders therefore do not need special-case exclusions. Fatal damage follows the normal
    /// unit despawn/corpse lifecycle, and fatalities are ordered by `SimId` so corpse allocation is
    /// deterministic.
    pub fn debug_damage_all_units(&mut self, damage: i32) -> usize {
        assert!(damage >= 0, "debug unit damage must be non-negative");

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
                &MovementProfile,
            )>();
            for (entity, id, owner, team, position, mut health, corpse, _) in
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
                    ));
                }
            }
        }

        fatalities.sort_unstable_by_key(|(_, id, _, _, _, _)| *id);
        for (entity, source_unit, source_owner, source_team, position, corpse) in fatalities {
            self.world.despawn(entity);
            let Some(profile) = corpse else {
                continue;
            };
            let id = self.allocate_id();
            let expires_tick = profile.lifetime_ticks.map(|lifetime_ticks| {
                self.next_tick
                    .checked_add(u64::from(lifetime_ticks))
                    .expect("corpse expiry tick overflow")
            });
            self.world.spawn((
                id,
                Position(position),
                Corpse {
                    source_unit,
                    source_owner,
                    source_team,
                    definition: profile.definition,
                    created_tick: self.next_tick,
                    expires_tick,
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
            MatchLifecycle::Running
        } else {
            MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask,
            }
        };
    }

    fn evaluate_objective_outcome(&mut self, completed_tick: u64) {
        let [Some(team_0_objective), Some(team_1_objective)] = self.team_objectives else {
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
            (true, true) => return,
            (false, false) => MatchOutcome::Draw,
            (false, true) => MatchOutcome::Victory(Team(1)),
            (true, false) => MatchOutcome::Victory(Team(0)),
        };
        self.lifecycle = MatchLifecycle::Finished {
            outcome,
            finished_tick: completed_tick,
        };
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
        let (source_entity, source_team, source_footprint, attack, attack_targets) = self
            .world
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
                    )
                })
            })
            .ok_or(BuildingCommandError::SourceNotFound)?;
        let attack = attack.ok_or(BuildingCommandError::SourceCannotAttack)?;
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
            if !attack_targets.can_target_unit(movement_class) {
                return Err(BuildingCommandError::InvalidTargetType);
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
        self.spawn_unit_unchecked(
            Some(owner),
            unit,
            definition.properties,
            definition.spellcasting,
        )
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
            entity
                .get::<BuilderBuildOrder>()
                .is_some_and(|order| footprints_overlap(order.building.footprint, footprint))
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
        let regions = self.config.team_build_regions.get(usize::from(team.0))?;
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
        self.advance_cooldowns();
        let corpses_expired = self.expire_corpses();
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
        let mut units = self.snapshot_units();
        let mut buildings = self.snapshot_buildings();
        resolve_periodic_unit_statuses(&mut units, completed_tick, self.combat_rules.damage_rules);
        self.resolve_burning_oil_zones(&mut units, &mut buildings, completed_tick);
        let grid = SpatialGrid::build(
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
        let ability_metrics = self.resolve_automatic_abilities(&mut buildings, &mut units, &grid);
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
            &units,
            &mut unit_health,
            &positions,
            completed_tick,
        );
        let chain_lightning_updates = chain_lightning_hops.updates;
        let chain_lightning_entities_to_remove = chain_lightning_hops.removals;

        let phase_start = Instant::now();
        let target_projectiles = self.resolve_due_target_projectiles(TargetProjectileContext {
            units: &mut units,
            buildings: &buildings,
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
        let mut projectile_impacts = target_projectiles.projectile_impacts;
        let mut projectile_effects = target_projectiles.projectile_effects;
        let projectile_invalidations = target_projectiles.projectile_invalidations;
        let bounce_jumps = target_projectiles.bounce_jumps;
        let bounce_candidate_checks = target_projectiles.bounce_candidate_checks;
        let mut chain_lightning_launches = target_projectiles.chain_lightning_launches;
        let reflected_projectile_launches = target_projectiles.reflected_projectile_launches;

        let attack_resolution = self.resolve_attacks(AttackResolutionContext {
            units: &mut units,
            buildings: &buildings,
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
        let ballistic_projectile_launches = attack_resolution.ballistic_projectile_launches;
        let bounce_projectile_launches = attack_resolution.bounce_projectile_launches;
        chain_lightning_launches.extend(attack_resolution.chain_lightning_launches);
        let projectiles_launched = projectile_launches.len()
            + reflected_projectile_launches.len()
            + ballistic_projectile_launches.len()
            + bounce_projectile_launches.len();
        let combat = phase_start.elapsed();

        let movement = self.resolve_movement(
            &units,
            &buildings,
            &unit_health,
            &building_health,
            &mut positions,
            &mut navigation_states,
        );

        let phase_start = Instant::now();
        let ballistic_impacts = self.resolve_due_ballistic_impacts(BallisticImpactContext {
            due_projectiles: due_ballistic_projectiles,
            units: &units,
            buildings: &buildings,
            positions: &positions,
            unit_health: &mut unit_health,
            building_health: &mut building_health,
            attackers_this_tick: &mut attackers_this_tick,
            next_defense_alerts: &mut next_defense_alerts,
            completed_tick,
        });
        projectile_entities_to_remove.extend(ballistic_impacts.projectile_entities_to_remove);
        projectile_impacts += ballistic_impacts.projectile_impacts;
        projectile_effects += ballistic_impacts.projectile_effects;
        let ballistic_candidate_checks = ballistic_impacts.candidate_checks;
        let burning_oil_zone_launches = ballistic_impacts.burning_oil_zone_launches;
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
            reflected_projectile_launches,
            ballistic_projectile_launches,
            bounce_projectile_launches,
            completed_tick,
        });

        let mut deaths = 0;
        let mut corpse_spawns = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            if unit_health[index] <= 0 {
                if let Some(profile) = unit.corpse {
                    corpse_spawns.push((unit.id, unit.owner, unit.team, positions[index], profile));
                }
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
            *entity
                .get_mut::<StatusState>()
                .expect("unit status state missing") = unit.status;
            if unit.spellcasting.is_some() {
                entity
                    .get_mut::<ManaState>()
                    .expect("spellcasting unit mana missing")
                    .current = unit
                    .mana_current
                    .expect("spellcasting unit snapshot mana missing");
                *entity
                    .get_mut::<AutomaticAbilityState>()
                    .expect("spellcasting unit ability state missing") = unit
                    .ability_state
                    .expect("spellcasting unit snapshot ability state missing");
            }
        }

        let corpses_spawned = corpse_spawns.len();
        for (source_unit, source_owner, source_team, position, profile) in corpse_spawns {
            let id = self.allocate_id();
            let expires_tick = profile.lifetime_ticks.map(|lifetime_ticks| {
                completed_tick
                    .checked_add(u64::from(lifetime_ticks))
                    .expect("corpse expiry tick overflow")
            });
            self.world.spawn((
                id,
                Position(position),
                Corpse {
                    source_unit,
                    source_owner,
                    source_team,
                    definition: profile.definition,
                    created_tick: completed_tick,
                    expires_tick,
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
                if building.attack.is_some() || building.spellcasting.is_some() {
                    *entity
                        .get_mut::<StatusState>()
                        .expect("active building status state missing") = building
                        .status
                        .expect("active building snapshot status state missing");
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
                self.world.despawn(entity);
            }
            self.topology_dirty = true;
        }

        self.evaluate_objective_outcome(completed_tick);
        if self.lifecycle == MatchLifecycle::Running {
            self.advance_economy_income();
        }
        self.defense_alerts = next_defense_alerts;
        let projectiles_alive = self.projectile_count();
        let corpses_alive = self.corpse_count();
        self.next_tick = self
            .next_tick
            .checked_add(1)
            .expect("tick counter exhausted");
        let structural_commit = phase_start.elapsed();

        let phase_start = Instant::now();
        let checksum = canonical_checksum(
            &self.world,
            CanonicalMatchState {
                next_tick: self.next_tick,
                next_id: self.next_id,
                configuration_identity: self.configuration_identity,
                defense_alerts: &self.defense_alerts,
                players: &self.players,
                lifecycle: self.lifecycle,
                team_objectives: self.team_objectives,
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
            crowd_and_collision: movement.crowd_and_collision,
            ballistic_impact,
            structural_commit,
            checksum: checksum_time,
            total: tick_start.elapsed(),
        };

        TickResult {
            completed_tick,
            units_alive: units.len() - unit_health.iter().filter(|health| **health <= 0).count(),
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
            units_spawned,
            spawn_failures,
            topology_rebuilds: usize::from(topology_rebuilt),
            pursuit_steps: movement.pursuit_steps,
            navigation_route_steps: movement.navigation_route_steps,
            movement_intents: movement.movement_intents,
            movement_blocked: movement.movement_blocked,
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
                next_tick: self.next_tick,
                next_id: self.next_id,
                configuration_identity: self.configuration_identity,
                defense_alerts: &self.defense_alerts,
                players: &self.players,
                lifecycle: self.lifecycle,
                team_objectives: self.team_objectives,
            },
        )
    }

    /// Clears presentation-only events from the most recently simulated tick. This does not alter
    /// authoritative state and is used after restore/catch-up so historical cosmetic effects are
    /// not emitted as if they had just happened live.
    pub fn clear_presentation_events(&mut self) {
        self.last_attacks.clear();
        self.last_ability_casts.clear();
        self.last_chain_lightnings.clear();
    }

    #[must_use]
    pub fn attacks_last_tick(&self) -> &[AttackEvent] {
        &self.last_attacks
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
            .filter(|entity| {
                entity.get::<GuaranteedHitProjectile>().is_some()
                    || entity.get::<ReflectedProjectile>().is_some()
                    || entity.get::<BallisticProjectile>().is_some()
                    || entity.get::<BounceProjectile>().is_some()
            })
            .count()
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
        self.next_id = self.next_id.checked_add(1).expect("SimId space exhausted");
        id
    }

    fn spawn_unit_unchecked(
        &mut self,
        owner: Option<PlayerId>,
        unit: UnitSpawn,
        properties: UnitGameplayProperties,
        spellcasting: Option<SpellcastingProfile>,
    ) -> SimId {
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
            StatusState::default(),
            NavigationState::default(),
            unit.movement,
            SpawnTick(self.next_tick),
        ));
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
                },
            ));
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

    fn advance_cooldowns(&mut self) {
        let mut query = self.world.query::<&mut AttackCooldown>();
        for mut cooldown in query.iter_mut(&mut self.world) {
            cooldown.remaining = cooldown.remaining.saturating_sub(1);
        }

        let mut health_regen_query = self.world.query::<(&mut Health, &mut HealthRegeneration)>();
        for (mut health, mut regeneration) in health_regen_query.iter_mut(&mut self.world) {
            if health.current >= health.max || regeneration.per_second_per_10k == 0 {
                health.current = health.current.min(health.max);
                regeneration.remainder_per_10k_hz = 0;
                continue;
            }
            let denominator = 10_000_u64
                .checked_mul(
                    u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz is positive"),
                )
                .expect("health regeneration fixed-point denominator overflow");
            let accumulated = u64::from(regeneration.remainder_per_10k_hz)
                + u64::from(regeneration.per_second_per_10k);
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

        let mut mana_query = self.world.query::<(&SpellcastingProfile, &mut ManaState)>();
        for (profile, mut mana) in mana_query.iter_mut(&mut self.world) {
            if mana.current >= profile.mana.maximum {
                mana.current = profile.mana.maximum;
                mana.regen_remainder_per_10k = 0;
                continue;
            }
            let accumulated = u64::from(mana.regen_remainder_per_10k)
                + u64::from(profile.mana.regen_per_tick_per_10k);
            let whole_mana = accumulated / 10_000;
            let remainder = accumulated % 10_000;
            let regenerated = i64::from(mana.current)
                + i64::try_from(whole_mana).expect("mana regeneration exceeds i64");
            if regenerated >= i64::from(profile.mana.maximum) {
                mana.current = profile.mana.maximum;
                mana.regen_remainder_per_10k = 0;
            } else {
                mana.current = i32::try_from(regenerated)
                    .expect("mana regeneration overflowed validated bounds");
                mana.regen_remainder_per_10k =
                    u16::try_from(remainder).expect("mana remainder fits 1/10,000 scale");
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
            .filter(|(_, _, _, _, _, state, _, _, _, _, _, _, _, _, _)| {
                state.next_spawn_tick <= self.next_tick
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
                        build_time_ticks: repair_metadata.build_time_ticks,
                        repair_time_ticks: repair_metadata.repair_time_ticks,
                        attack_targets: attack_targets.0,
                        health_regen_per_second_per_10k: health_regeneration.0,
                        damage_type: damage_type.0,
                        armor: armor.0,
                        passive_effects: passive_effects.0,
                        spellcasting: spellcasting.map(|profile| profile.0),
                        next_spawn_tick: state.next_spawn_tick,
                    }
                },
            )
            .collect();
        attempts.sort_unstable_by_key(|attempt| attempt.id);

        if attempts.is_empty() {
            return (0, 0);
        }

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
                (unit.movement_class == MovementClass::Ground).then_some((
                    index,
                    unit.position,
                    unit.collision_radius,
                ))
            }),
        );
        let mut air_reservations = SpatialReservationGrid::build_with_radii(
            reservation_cell_size,
            bounds_min,
            bounds_max,
            reservation_capacity,
            units.iter().enumerate().filter_map(|(index, unit)| {
                (unit.movement_class == MovementClass::Air).then_some((
                    index,
                    unit.position,
                    unit.collision_radius,
                ))
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
                    let movement_legal = match attempt.movement_class {
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
                        && reservations.is_clear_with_radius(position, collision_radius))
                    .then_some((cell, position))
                });

            if let Some((_cell, position)) = spawn {
                self.spawn_unit_unchecked(
                    Some(attempt.owner),
                    UnitSpawn::from_template(attempt.team, position, attempt.profile.unit),
                    UnitGameplayProperties {
                        content: attempt.content,
                        health_regen_per_second_per_10k: attempt.health_regen_per_second_per_10k,
                        corpse: attempt.corpse,
                        collision_radius: attempt.collision_radius,
                        movement_class: attempt.movement_class,
                        mechanical: attempt.mechanical,
                        build_time_ticks: attempt.build_time_ticks,
                        repair_time_ticks: attempt.repair_time_ticks,
                        attack_targets: attempt.attack_targets,
                        damage_type: attempt.damage_type,
                        armor: attempt.armor,
                        passive_effects: attempt.passive_effects,
                    },
                    attempt.spellcasting,
                );
                reservations.insert_with_radius(next_reservation_index, position, collision_radius);
                next_reservation_index += 1;
                spawned += 1;
            } else {
                failed += 1;
            }

            let next = attempt
                .next_spawn_tick
                .checked_add(u64::from(attempt.profile.interval_ticks))
                .expect("production tick overflow");
            self.world
                .entity_mut(attempt.entity)
                .get_mut::<ProductionState>()
                .expect("production state missing")
                .next_spawn_tick = next;
        }

        (spawned, failed)
    }

    fn snapshot_units(&mut self) -> Vec<UnitSnapshot> {
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
            .filter(|(_, _, _, _, _, health, _, _, _, _, _, _, _, _)| health.current > 0)
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
                    let attack_targets = *entity_ref
                        .get::<AttackTargetMask>()
                        .expect("unit attack target mask missing");
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
                    debug_assert_eq!(spellcasting.is_some(), mana_current.is_some());
                    debug_assert_eq!(spellcasting.is_some(), ability_state.is_some());
                    UnitSnapshot {
                        entity,
                        id: *id,
                        owner: owner.0,
                        team: *team,
                        position: position.0,
                        health: health.current,
                        attack: *attack,
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
                        attack_targets,
                        damage_type,
                        armor,
                        passive_effects,
                        spellcasting,
                        mana_current,
                        ability_state,
                    }
                },
            )
            .collect();
        units.sort_unstable_by_key(|unit| unit.id);
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
                    debug_assert_eq!(spellcasting.is_some(), mana.is_some());
                    debug_assert_eq!(spellcasting.is_some(), ability_state.is_some());
                    debug_assert_eq!(attack.is_some() || spellcasting.is_some(), status.is_some());
                    BuildingSnapshot {
                        entity,
                        id: *id,
                        team: *team,
                        footprint: *footprint,
                        health: health.current,
                        attack: attack.copied(),
                        attack_targets,
                        damage_type,
                        armor,
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

    fn apply_crowd_separation(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        decisions: &[MovementDecision],
        desired_positions: &[SimPoint],
    ) -> Vec<SimPoint> {
        let max_separation = self.config.max_separation_per_tick;
        let max_radius = units
            .iter()
            .enumerate()
            .filter(|(index, _)| unit_health[*index] > 0)
            .map(|(_, unit)| unit.collision_radius)
            .max()
            .unwrap_or(0);
        if max_radius == 0 || max_separation == 0 {
            return desired_positions.to_vec();
        }

        let max_pair_distance = max_radius.saturating_mul(2);
        let max_anticipation_distance = max_pair_distance.saturating_mul(2);
        let collision_grid = SpatialGrid::build(
            max_anticipation_distance.max(1),
            desired_positions
                .iter()
                .enumerate()
                .filter(|(index, _)| unit_health[*index] > 0)
                .map(|(index, position)| {
                    (
                        movement_collision_partition(units[index].movement_class),
                        index,
                        *position,
                    )
                }),
        );
        self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(index, unit)| {
                    if unit_health[index] <= 0 {
                        return desired_positions[index];
                    }
                    let desired = desired_positions[index];
                    let current_cell = self.topology.cell_of_point(unit.position);
                    if unit.movement_class == MovementClass::Ground
                        && self.topology.component_id(current_cell).is_none()
                    {
                        return desired;
                    }
                    let movement_x = i64::from(desired.x) - i64::from(unit.position.x);
                    let movement_y = i64::from(desired.y) - i64::from(unit.position.y);
                    let mut push_x = 0_i64;
                    let mut push_y = 0_i64;
                    let mut contributions = 0_i64;

                    let query_radius = unit
                        .collision_radius
                        .saturating_add(max_radius)
                        .saturating_mul(2);
                    collision_grid.for_each_candidate(
                        movement_collision_partition(unit.movement_class),
                        desired,
                        query_radius,
                        |other_index| {
                            if other_index == index || unit_health[other_index] <= 0 {
                                return;
                            }
                            let other = desired_positions[other_index];
                            let separation_distance = unit
                                .collision_radius
                                .saturating_add(units[other_index].collision_radius);
                            let anticipation_distance = separation_distance.saturating_mul(2);
                            let separation_sq = square_i32(separation_distance);
                            let anticipation_sq = square_i32(anticipation_distance);
                            let distance_sq = desired.distance_sq(other);
                            if distance_sq >= anticipation_sq {
                                return;
                            }

                            let mut contributed = false;
                            if distance_sq < separation_sq {
                                let dx = i64::from(desired.x) - i64::from(other.x);
                                let dy = i64::from(desired.y) - i64::from(other.y);
                                let (direction_x, direction_y, axis_distance) =
                                    if dx == 0 && dy == 0 {
                                        let (x, y) =
                                            exact_overlap_direction(unit.id, units[other_index].id);
                                        (i64::from(x), i64::from(y), 0_i64)
                                    } else {
                                        (dx.signum(), dy.signum(), dx.abs().max(dy.abs()))
                                    };
                                let penetration =
                                    (i64::from(separation_distance) - axis_distance).max(1);
                                push_x += direction_x * penetration;
                                push_y += direction_y * penetration;
                                contributed = true;
                            }

                            if (movement_x != 0 || movement_y != 0)
                                && unit.target != Some(units[other_index].id)
                            {
                                let other_move_x =
                                    i64::from(other.x) - i64::from(units[other_index].position.x);
                                let other_move_y =
                                    i64::from(other.y) - i64::from(units[other_index].position.y);
                                let to_other_x = i64::from(units[other_index].position.x)
                                    - i64::from(unit.position.x);
                                let to_other_y = i64::from(units[other_index].position.y)
                                    - i64::from(unit.position.y);
                                let other_is_ahead =
                                    movement_x * to_other_x + movement_y * to_other_y > 0;
                                let relative_move_x = movement_x - other_move_x;
                                let relative_move_y = movement_y - other_move_y;
                                let closing =
                                    relative_move_x * to_other_x + relative_move_y * to_other_y > 0;
                                if other_is_ahead && closing {
                                    let axis_distance = to_other_x.abs().max(to_other_y.abs());
                                    let pressure =
                                        (i64::from(anticipation_distance) - axis_distance).max(1);
                                    let goal = navigation_goal(unit, &decisions[index]);
                                    let remembered_side = if unit.navigation.avoidance_goal == goal
                                        && goal != NavigationGoal::None
                                        && unit.navigation.bypass_side != 0
                                    {
                                        i64::from(unit.navigation.bypass_side)
                                    } else {
                                        i64::from(sidestep_sign(unit.id))
                                    };
                                    let perpendicular_x = -movement_y.signum() * remembered_side;
                                    let perpendicular_y = movement_x.signum() * remembered_side;
                                    push_x += perpendicular_x * pressure;
                                    push_y += perpendicular_y * pressure;
                                    contributed = true;
                                }
                            }
                            contributions += i64::from(contributed);
                        },
                    );

                    if contributions == 0 {
                        return desired;
                    }
                    push_x /= contributions;
                    push_y /= contributions;
                    let raw_offset = SimPoint::new(
                        i32::try_from(push_x).expect("crowd x offset overflow"),
                        i32::try_from(push_y).expect("crowd y offset overflow"),
                    );
                    let offset = SimPoint::new(0, 0).step_towards(raw_offset, max_separation);
                    self.valid_separated_position(current_cell, desired, offset, unit)
                })
                .collect()
        })
    }

    fn enforce_hard_non_overlap(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        navigation_states: &mut [NavigationState],
        decisions: &[MovementDecision],
        desired_positions: &[SimPoint],
        separated_positions: &[SimPoint],
    ) -> Vec<SimPoint> {
        let max_radius = units
            .iter()
            .enumerate()
            .filter(|(index, _)| unit_health[*index] > 0)
            .map(|(_, unit)| unit.collision_radius)
            .max()
            .unwrap_or(0);
        if max_radius == 0 {
            return separated_positions.to_vec();
        }

        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let reservation_cell_size = max_radius.saturating_mul(2).max(1);
        let mut ground_reservations = SpatialReservationGrid::build_with_radii(
            reservation_cell_size,
            bounds_min,
            bounds_max,
            units.len(),
            units.iter().enumerate().filter_map(|(index, unit)| {
                (unit_health[index] > 0 && unit.movement_class == MovementClass::Ground)
                    .then_some((index, separated_positions[index], unit.collision_radius))
            }),
        );
        let mut air_reservations =
            SpatialReservationGrid::build_with_radii(
                reservation_cell_size,
                bounds_min,
                bounds_max,
                units.len(),
                units.iter().enumerate().filter_map(|(index, unit)| {
                    (unit_health[index] > 0 && unit.movement_class == MovementClass::Air)
                        .then_some((index, separated_positions[index], unit.collision_radius))
                }),
            );
        let mut result: Vec<_> = units.iter().map(|unit| unit.position).collect();
        let lateral = self.config.max_separation_per_tick.max(1);

        for index in (0..units.len()).rev() {
            let unit = &units[index];
            if unit_health[index] <= 0 {
                continue;
            }
            let original_cell = self.topology.cell_of_point(unit.position);
            if unit.movement_class == MovementClass::Ground
                && self.topology.component_id(original_cell).is_none()
            {
                continue;
            }
            let reservations = match unit.movement_class {
                MovementClass::Ground => &mut ground_reservations,
                MovementClass::Air => &mut air_reservations,
            };
            reservations.remove(index);

            let desired = desired_positions[index];
            let separated = separated_positions[index];
            let sidestep_distance = effective_movement_speed(unit).max(lateral).max(1);
            let goal = navigation_goal(unit, &decisions[index]);
            let navigating = goal != NavigationGoal::None;
            let navigation = &mut navigation_states[index];
            if !navigating || navigation.avoidance_goal != goal {
                *navigation = NavigationState::default();
            }

            let direct_clear = self.position_is_legal_for_unit(unit, original_cell, desired)
                && reservations.is_clear_with_radius(desired, unit.collision_radius);
            let mut avoiding = false;
            if navigating {
                if !direct_clear {
                    navigation.avoidance_goal = goal;
                    if navigation.bypass_side == 0 {
                        navigation.bypass_side =
                            i8::try_from(sidestep_sign(unit.id)).expect("sidestep sign fits i8");
                    }
                    navigation.clear_ticks = 0;
                    avoiding = true;
                } else if navigation.avoidance_goal == goal && navigation.bypass_side != 0 {
                    navigation.clear_ticks = navigation.clear_ticks.saturating_add(1);
                    if navigation.clear_ticks < AVOIDANCE_CLEAR_TICKS {
                        avoiding = true;
                    } else {
                        *navigation = NavigationState::default();
                    }
                }
            }

            let steer_toward = if desired != unit.position {
                desired
            } else {
                decisions[index].attack_goal.unwrap_or(desired)
            };
            let default_side = i8::try_from(sidestep_sign(unit.id)).expect("sidestep sign fits i8");
            let side = if avoiding {
                navigation.bypass_side
            } else {
                default_side
            };
            let preferred_tangent =
                perpendicular_step_with_side(side, unit.position, steer_toward, sidestep_distance);
            let opposite_tangent =
                perpendicular_step_with_side(-side, unit.position, steer_toward, sidestep_distance);
            let preferred_arc =
                pursuit_arc_step(side, unit.position, steer_toward, sidestep_distance, true);
            let preferred_back_arc =
                pursuit_arc_step(side, unit.position, steer_toward, sidestep_distance, false);
            let opposite_arc =
                pursuit_arc_step(-side, unit.position, steer_toward, sidestep_distance, true);
            let preferred_tangent =
                offset_point(unit.position, preferred_tangent.x, preferred_tangent.y);
            let opposite_tangent =
                offset_point(unit.position, opposite_tangent.x, opposite_tangent.y);
            let preferred_arc = offset_point(unit.position, preferred_arc.x, preferred_arc.y);
            let preferred_back_arc =
                offset_point(unit.position, preferred_back_arc.x, preferred_back_arc.y);
            let opposite_arc = offset_point(unit.position, opposite_arc.x, opposite_arc.y);

            let mut chosen = None;
            if avoiding {
                let candidates = [
                    (preferred_arc, side),
                    (preferred_tangent, side),
                    (preferred_back_arc, side),
                    (Some(separated), 0),
                    (opposite_arc, -side),
                    (opposite_tangent, -side),
                    (Some(desired), 0),
                    (Some(unit.position), side),
                ];
                for (candidate, candidate_side) in candidates {
                    let Some(candidate) = candidate else {
                        continue;
                    };
                    if self.position_is_legal_for_unit(unit, original_cell, candidate)
                        && reservations.is_clear_with_radius(candidate, unit.collision_radius)
                    {
                        if candidate_side != 0 && candidate_side != navigation.bypass_side {
                            navigation.bypass_side = candidate_side;
                            navigation.clear_ticks = 0;
                        }
                        chosen = Some(candidate);
                        break;
                    }
                }
            } else {
                for candidate in [
                    Some(separated),
                    Some(desired),
                    preferred_tangent,
                    opposite_tangent,
                    Some(unit.position),
                ]
                .into_iter()
                .flatten()
                {
                    if self.position_is_legal_for_unit(unit, original_cell, candidate)
                        && reservations.is_clear_with_radius(candidate, unit.collision_radius)
                    {
                        chosen = Some(candidate);
                        break;
                    }
                }
            }

            let chosen = chosen
                .or_else(|| {
                    self.find_local_non_overlap_position(
                        unit,
                        original_cell,
                        sidestep_sign(unit.id),
                        reservations,
                    )
                })
                .unwrap_or(unit.position);

            reservations.insert_with_radius(index, chosen, unit.collision_radius);
            result[index] = chosen;
        }

        result
    }

    fn find_local_non_overlap_position(
        &self,
        unit: &UnitSnapshot,
        original_cell: NavCell,
        search_bias: i32,
        reservations: &SpatialReservationGrid,
    ) -> Option<SimPoint> {
        debug_assert!(search_bias == -1 || search_bias == 1);
        let origin = unit.position;
        let collision_radius = unit.collision_radius;
        let step = collision_radius.max(1);
        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let max_radius = [
            origin.x.saturating_sub(bounds_min.x).abs(),
            bounds_max.x.saturating_sub(origin.x).abs(),
            origin.y.saturating_sub(bounds_min.y).abs(),
            bounds_max.y.saturating_sub(origin.y).abs(),
        ]
        .into_iter()
        .max()
        .unwrap_or(step)
        .max(step);
        let max_ring = (max_radius + step - 1) / step;

        let legal_position = |candidate| match unit.movement_class {
            MovementClass::Ground => self.position_is_traversable_from(
                original_cell,
                candidate,
                unit.collision_radius_override,
            ),
            MovementClass::Air => {
                let source_cell = self.air_topology.cell_of_point(origin);
                self.air_position_is_traversable_from(source_cell, candidate, collision_radius)
            }
        };
        let order_multiplier = -search_bias;
        for ring in 1..=max_ring {
            let distance = ring.checked_mul(step)?;
            for raw_x_step in -ring..=ring {
                let x_step = raw_x_step.checked_mul(order_multiplier)?;
                let x = x_step.checked_mul(step)?;
                for y in [
                    search_bias.checked_mul(distance)?,
                    (-search_bias).checked_mul(distance)?,
                ] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    if legal_position(candidate)
                        && reservations.is_clear_with_radius(candidate, collision_radius)
                    {
                        return Some(candidate);
                    }
                }
            }
            for raw_y_step in (-ring + 1)..=(ring - 1) {
                let y_step = raw_y_step.checked_mul(order_multiplier)?;
                let y = y_step.checked_mul(step)?;
                for x in [
                    search_bias.checked_mul(distance)?,
                    (-search_bias).checked_mul(distance)?,
                ] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    if legal_position(candidate)
                        && reservations.is_clear_with_radius(candidate, collision_radius)
                    {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    fn nearest_reachable_unit_attack_cell(
        &self,
        source_cell: NavCell,
        current: SimPoint,
        target: SimPoint,
        attack_range: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let cell_size = self.config.navigation_cell_size;
        let radius_cells = attack_range
            .saturating_add(cell_size - 1)
            .div_euclid(cell_size)
            .saturating_add(1);
        let target_cell = self.topology.cell_of_point(target);
        let range_sq = square_i32(attack_range);
        let mut best: Option<(u64, i32, i32, NavCell)> = None;

        for y in target_cell.y - radius_cells..=target_cell.y + radius_cells {
            for x in target_cell.x - radius_cells..=target_cell.x + radius_cells {
                let cell = NavCell::new(x, y);
                if !self.topology.contains(cell) {
                    continue;
                }
                let position = self.topology.center_of_cell(cell);
                if position.distance_sq(target) > range_sq
                    || !self.position_is_traversable_from(source_cell, position, collision_radius)
                {
                    continue;
                }
                let key = (current.distance_sq(position), y, x, cell);
                if best.is_none_or(|existing| key < existing) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, _, _, cell)| cell)
    }

    fn nearest_reachable_building_attack_cell(
        &self,
        source_cell: NavCell,
        current: SimPoint,
        footprint: BuildingFootprint,
        attack_range: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let cell_size = self.config.navigation_cell_size;
        let radius_cells = attack_range
            .saturating_add(cell_size - 1)
            .div_euclid(cell_size)
            .saturating_add(1);
        let range_sq = square_i32(attack_range);
        let mut best: Option<(u64, i32, i32, NavCell)> = None;
        let min_x = footprint.min_x.saturating_sub(radius_cells);
        let max_x = footprint.max_x().saturating_add(radius_cells);
        let min_y = footprint.min_y.saturating_sub(radius_cells);
        let max_y = footprint.max_y().saturating_add(radius_cells);

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let cell = NavCell::new(x, y);
                if !self.topology.contains(cell) {
                    continue;
                }
                let position = self.topology.center_of_cell(cell);
                if point_to_footprint_distance_sq(position, footprint, cell_size) > range_sq
                    || !self.position_is_traversable_from(source_cell, position, collision_radius)
                {
                    continue;
                }
                let key = (current.distance_sq(position), y, x, cell);
                if best.is_none_or(|existing| key < existing) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, _, _, cell)| cell)
    }

    fn navigation_world_bounds(&self) -> (SimPoint, SimPoint) {
        let cell_size = i64::from(self.config.navigation_cell_size);
        let min_x = i64::from(self.config.navigation_min.x) * cell_size;
        let min_y = i64::from(self.config.navigation_min.y) * cell_size;
        let max_x = (i64::from(self.config.navigation_max.x) + 1) * cell_size - 1;
        let max_y = (i64::from(self.config.navigation_max.y) + 1) * cell_size - 1;
        (
            SimPoint::new(
                i32::try_from(min_x).expect("navigation minimum x overflow"),
                i32::try_from(min_y).expect("navigation minimum y overflow"),
            ),
            SimPoint::new(
                i32::try_from(max_x).expect("navigation maximum x overflow"),
                i32::try_from(max_y).expect("navigation maximum y overflow"),
            ),
        )
    }

    fn position_is_legal_for_unit(
        &self,
        unit: &UnitSnapshot,
        original_cell: NavCell,
        candidate: SimPoint,
    ) -> bool {
        match unit.movement_class {
            MovementClass::Ground => self.position_is_traversable_from(
                original_cell,
                candidate,
                unit.collision_radius_override,
            ),
            MovementClass::Air => self.air_position_is_traversable_from(
                self.air_topology.cell_of_point(unit.position),
                candidate,
                unit.collision_radius,
            ),
        }
    }

    fn position_is_traversable_from(
        &self,
        original_cell: NavCell,
        candidate: SimPoint,
        collision_radius: Option<i32>,
    ) -> bool {
        let Some(component) = self.topology.component_id(original_cell) else {
            return false;
        };
        if let Some(collision_radius) = collision_radius {
            self.topology
                .circle_is_traversable_in_component(candidate, collision_radius, component)
        } else {
            self.topology
                .component_id(self.topology.cell_of_point(candidate))
                == Some(component)
        }
    }

    fn air_position_is_traversable_from(
        &self,
        original_cell: NavCell,
        candidate: SimPoint,
        collision_radius: i32,
    ) -> bool {
        let Some(component) = self.air_topology.component_id(original_cell) else {
            return false;
        };
        self.air_topology
            .circle_is_traversable_in_component(candidate, collision_radius, component)
    }

    fn valid_separated_position(
        &self,
        original_cell: NavCell,
        desired: SimPoint,
        offset: SimPoint,
        unit: &UnitSnapshot,
    ) -> SimPoint {
        let candidates = [
            SimPoint::new(
                desired
                    .x
                    .checked_add(offset.x)
                    .expect("separation x overflow"),
                desired
                    .y
                    .checked_add(offset.y)
                    .expect("separation y overflow"),
            ),
            SimPoint::new(
                desired
                    .x
                    .checked_add(offset.x)
                    .expect("separation x overflow"),
                desired.y,
            ),
            SimPoint::new(
                desired.x,
                desired
                    .y
                    .checked_add(offset.y)
                    .expect("separation y overflow"),
            ),
            desired,
        ];

        candidates
            .into_iter()
            .find(|candidate| self.position_is_legal_for_unit(unit, original_cell, *candidate))
            .unwrap_or(desired)
    }
}

#[derive(Debug, Clone, Copy)]
struct NavigationRoute {
    next_cell: Option<NavCell>,
    navigation_route_step: bool,
    used_a_star: bool,
    a_star_cache_hit: bool,
    a_star_expanded_nodes: usize,
    cache_insert: Option<PursuitCacheInsert>,
}

impl NavigationRoute {
    const fn at(cell: NavCell) -> Self {
        Self {
            next_cell: Some(cell),
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }

    const fn none() -> Self {
        Self {
            next_cell: None,
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PursuitCacheInsert {
    from: NavCell,
    target: NavCell,
    collision_radius: Option<i32>,
    route_bias: i8,
    next: NavCell,
}

#[derive(Debug, Clone, Copy)]
struct MovementDecision {
    position: SimPoint,
    pursuit_step: bool,
    pursuit_target: Option<SimId>,
    attack_goal: Option<SimPoint>,
    navigation_route_step: bool,
    used_a_star: bool,
    a_star_cache_hit: bool,
    a_star_expanded_nodes: usize,
    cache_insert: Option<PursuitCacheInsert>,
}

impl MovementDecision {
    const fn stationary(position: SimPoint) -> Self {
        Self {
            position,
            pursuit_step: false,
            pursuit_target: None,
            attack_goal: None,
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }
}

fn navigation_goal(unit: &UnitSnapshot, decision: &MovementDecision) -> NavigationGoal {
    if let Some(target) = decision.pursuit_target {
        NavigationGoal::Target(target)
    } else if decision.position != unit.position {
        NavigationGoal::Objective(unit.team)
    } else {
        NavigationGoal::None
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MovementMetrics {
    intent: Duration,
    crowd_and_collision: Duration,
    pursuit_steps: usize,
    navigation_route_steps: usize,
    movement_intents: usize,
    movement_blocked: usize,
    objective_move_intents: usize,
    a_star_fallbacks: usize,
    a_star_cache_hits: usize,
    a_star_expanded_nodes: usize,
}

#[derive(Debug, Clone, Copy)]
struct UnitSnapshot {
    entity: Entity,
    id: SimId,
    owner: PlayerId,
    team: Team,
    position: SimPoint,
    health: i32,
    attack: AttackProfile,
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
    attack_targets: AttackTargetMask,
    damage_type: DamageType,
    armor: ArmorProfile,
    passive_effects: PassiveUnitEffects,
    spellcasting: Option<SpellcastingProfile>,
    mana_current: Option<i32>,
    ability_state: Option<AutomaticAbilityState>,
}

#[derive(Debug, Clone, Copy)]
struct BuildingSnapshot {
    entity: Entity,
    id: SimId,
    team: Team,
    footprint: BuildingFootprint,
    health: i32,
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
    build_time_ticks: Option<u32>,
    repair_time_ticks: Option<u32>,
    attack_targets: AttackTargetMask,
    health_regen_per_second_per_10k: u32,
    damage_type: DamageType,
    armor: ArmorProfile,
    passive_effects: PassiveUnitEffects,
    spellcasting: Option<SpellcastingProfile>,
    next_spawn_tick: u64,
}

#[derive(Debug, Clone, Copy)]
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
}

#[derive(Debug, Clone, Copy)]
enum AbilityIntentTarget {
    Unit { index: usize, id: SimId },
    AllEnemyUnits,
}

impl AbilityIntentTarget {
    const fn sort_key(self) -> (u8, SimId) {
        match self {
            Self::Unit { id, .. } => (0, id),
            Self::AllEnemyUnits => (1, SimId(0)),
        }
    }

    const fn cast_target(self) -> AbilityCastTarget {
        match self {
            Self::Unit { id, .. } => AbilityCastTarget::Unit(id),
            Self::AllEnemyUnits => AbilityCastTarget::AllEnemyUnits,
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
    damage_rules: DamageRules,
    units: &'a [UnitSnapshot],
    buildings: &'a [BuildingSnapshot],
    unit_positions: &'a [SimPoint],
    unit_health: &'a mut [i32],
    building_health: &'a mut [i32],
    attackers_this_tick: &'a mut [Option<SimId>],
    next_defense_alerts: &'a mut Vec<DefenseAlert>,
    navigation_cell_size: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

fn movement_collision_partition(movement_class: MovementClass) -> SpatialPartition {
    let component = match movement_class {
        MovementClass::Ground => 0,
        MovementClass::Air => 1,
    };
    SpatialPartition::new(0, component)
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
    assert!(attack.acquisition_range >= attack.range);
    match attack.delivery {
        AttackDelivery::Melee => {}
        AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
            assert!(speed_per_tick > 0);
        }
        AttackDelivery::RangedBallistic {
            speed_per_tick,
            impact_radius,
        } => {
            assert!(speed_per_tick > 0);
            assert!(impact_radius >= 0);
        }
        AttackDelivery::Bounce {
            speed_per_tick,
            bounce_range,
            max_bounces,
            damage_percent_per_bounce,
            allow_repeat_targets: _,
        } => {
            assert!(speed_per_tick > 0);
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
    assert!(spellcasting.mana.regen_per_tick_per_10k <= 10_000_000);
    assert!(spellcasting.ability.mana_cost >= 0);
    assert!(spellcasting.ability.mana_cost <= spellcasting.mana.maximum);
    assert!(spellcasting.ability.range >= 0);
    match spellcasting.ability.target_policy {
        AbilityTargetPolicy::RandomEnemyUnit
        | AbilityTargetPolicy::RecentlyAttackedFriendlyUnit => {}
        AbilityTargetPolicy::AllEnemyUnits | AbilityTargetPolicy::RandomEnemyUnitGlobal => {
            assert_eq!(spellcasting.ability.range, 0);
        }
    }
    match spellcasting.ability.effect {
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
        AbilityEffect::AreaDamage { amount, radius } => {
            assert!(amount >= 0);
            assert!(radius >= 0);
            assert_ne!(
                spellcasting.ability.target_policy,
                AbilityTargetPolicy::AllEnemyUnits,
                "area damage requires a selected enemy unit as its center"
            );
        }
        AbilityEffect::FrostArmor {
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
    if let Some(lifetime_ticks) = corpse.lifetime_ticks {
        assert!(lifetime_ticks > 0, "corpse lifetime must be positive");
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
        id: *entity.get::<SimId>()?,
        position: entity.get::<Position>()?.0,
        source_unit: corpse.source_unit,
        source_owner: corpse.source_owner,
        source_team: corpse.source_team,
        definition: corpse.definition,
        created_tick: corpse.created_tick,
        expires_tick: corpse.expires_tick,
    })
}

fn projectile_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<ProjectileView> {
    let id = *entity.get::<SimId>()?;
    if let Some(projectile) = entity.get::<GuaranteedHitProjectile>() {
        return Some(ProjectileView {
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
    if let Some(projectile) = entity.get::<BallisticProjectile>() {
        return Some(ProjectileView {
            id,
            source: projectile.source,
            launch_position: projectile.launch_position,
            launch_tick: projectile.launch_tick,
            impact_tick: projectile.impact_tick,
            kind: ProjectileViewKind::Ballistic {
                destination: projectile.destination,
                impact_radius: projectile.impact_radius,
            },
        });
    }
    let projectile = *entity.get::<BounceProjectile>()?;
    Some(ProjectileView {
        id,
        source: projectile.source,
        launch_position: projectile.launch_position,
        launch_tick: projectile.launch_tick,
        impact_tick: projectile.impact_tick,
        kind: ProjectileViewKind::Bounce {
            target: projectile.target,
            bounce_index: projectile.bounce_index,
            remaining_bounces: projectile.remaining_bounces,
        },
    })
}

fn builder_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<BuilderView> {
    entity.get::<Builder>()?;
    let state = *entity.get::<BuilderState>()?;
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
        build_footprint: entity
            .get::<BuilderBuildOrder>()
            .map(|order| order.building.footprint),
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
    let active_defend_ability = active_defend_profile(passive_effects, spawn_tick, current_tick)
        .map(|profile| profile.ability);
    let status = *entity.get::<StatusState>()?;
    Some(UnitView {
        id: *entity.get::<SimId>()?,
        content: entity.get::<ContentIdentity>().copied(),
        owner: entity.get::<Owner>()?.0,
        team: *entity.get::<Team>()?,
        position: entity.get::<Position>()?.0,
        collision_radius: entity
            .get::<CollisionRadius>()
            .map_or(default_collision_radius, |radius| radius.0),
        movement_class: *entity.get::<MovementClass>()?,
        mechanical: entity.get::<MechanicalUnit>().is_some(),
        health: entity.get::<Health>()?.current,
        health_max: entity.get::<Health>()?.max,
        attack_delivery: entity.get::<AttackProfile>()?.delivery,
        attack_targets: *entity.get::<AttackTargetMask>()?,
        damage_type: *entity.get::<DamageType>()?,
        armor: *entity.get::<ArmorProfile>()?,
        target: entity.get::<TargetState>()?.current,
        direct_retaliation_lock: entity.get::<TargetState>()?.direct_retaliation_lock,
        ally_defense_lock: entity.get::<TargetState>()?.ally_defense_lock,
        last_attacker: entity.get::<RetaliationState>()?.attacker,
        last_attacked_tick: entity.get::<RetaliationState>()?.attacked_tick,
        cooldown_remaining: entity.get::<AttackCooldown>()?.remaining,
        stunned_until_tick: status.stunned_until_tick,
        status,
        mana_current: entity.get::<ManaState>().map(|mana| mana.current),
        mana_maximum: spellcasting.map(|profile| profile.mana.maximum),
        ability_ready_tick: ability_state.map(|state| state.ready_tick),
        ability_cast_sequence: ability_state.map(|state| state.cast_sequence),
        active_defend_ability,
    })
}

fn building_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<BuildingView> {
    let production = entity.get::<ProductionProfile>().copied();
    let attack = entity.get::<AttackProfile>().copied();
    let spellcasting = entity.get::<SpellcastingProfile>().copied();
    let ability_state = entity.get::<AutomaticAbilityState>().copied();
    let construction = entity.get::<BuildingConstruction>().copied();
    Some(BuildingView {
        id: *entity.get::<SimId>()?,
        content: entity.get::<ContentIdentity>().copied(),
        owner: entity.get::<Owner>().map(|owner| owner.0),
        team: *entity.get::<Team>()?,
        footprint: *entity.get::<BuildingFootprint>()?,
        health: entity.get::<Health>()?.current,
        health_max: entity.get::<Health>()?.max,
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
        mana_maximum: spellcasting.map(|profile| profile.mana.maximum),
        ability_ready_tick: ability_state.map(|state| state.ready_tick),
        ability_cast_sequence: ability_state.map(|state| state.cast_sequence),
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

fn spell_damage_after_defend(unit: UnitSnapshot, damage: i32, completed_tick: u64) -> i32 {
    active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
        .map_or(damage, |profile| {
            scale_damage_per_10k(damage, profile.spell_damage_taken_per_10k)
        })
}

fn apply_damage_to_target(
    target: TargetIndex,
    source_id: SimId,
    damage: i32,
    damage_type: DamageType,
    completed_tick: u64,
    state: DamageTargetState<'_>,
) -> Option<SimPoint> {
    match target {
        TargetIndex::Unit(index) => {
            if state.unit_health[index] <= 0 {
                return None;
            }
            let adjusted_damage = state.damage_rules.apply_attack_with_armor_per_100(
                damage,
                damage_type,
                state.units[index].armor.armor_type,
                effective_armor_points_per_100(&state.units[index]),
            );
            state.unit_health[index] = state.unit_health[index]
                .checked_sub(adjusted_damage)
                .expect("unit damage arithmetic overflowed validated bounds");
            let source_is_unit = find_unit_index(state.units, source_id).is_some();
            let recorded_attacker_is_building = state.attackers_this_tick[index]
                .is_some_and(|attacker| find_building_index(state.buildings, attacker).is_some());
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
            Some(state.unit_positions[index])
        }
        TargetIndex::Building(index) => {
            if state.building_health[index] <= 0 {
                return None;
            }
            let adjusted_damage =
                state
                    .damage_rules
                    .apply_attack(damage, damage_type, state.buildings[index].armor);
            state.building_health[index] = state.building_health[index]
                .checked_sub(adjusted_damage)
                .expect("building damage arithmetic overflowed validated bounds");
            Some(footprint_center_point(
                state.buildings[index].footprint,
                state.navigation_cell_size,
            ))
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
    completed_tick: u64,
    units: &'a mut [UnitSnapshot],
    unit_health: &'a mut [i32],
    damage_rules: DamageRules,
}

#[derive(Debug, Clone, Copy, Default)]
struct PendingAttackEffectResult {
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
        completed_tick,
        units,
        unit_health,
        damage_rules,
    } = state;
    let TargetIndex::Unit(index) = target else {
        return PendingAttackEffectResult::default();
    };
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
    let mut result = PendingAttackEffectResult::default();
    if let Some(effect) = effects.triggered_spell {
        match effect {
            TriggeredAttackEffect::ChainLightning(profile) => {
                let max_targets = usize::from(profile.maximum_targets).min(MAX_BOUNCE_HITS);
                if max_targets > 0 {
                    let adjusted = damage_rules
                        .apply_spell(profile.initial_damage, units[index].armor.armor_type);
                    let adjusted =
                        spell_damage_after_defend(units[index], adjusted, completed_tick);
                    unit_health[index] = unit_health[index]
                        .checked_sub(adjusted)
                        .expect("Chain Lightning damage overflow");

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
                if profile.targets.can_target_unit(units[index].movement_class) {
                    let expires_tick = completed_tick
                        .checked_add(u64::from(profile.duration_ticks))
                        .expect("Entangling Roots expiry overflow");
                    apply_timed_movement_modifier(
                        &mut units[index].status,
                        ModifierId(profile.ability.0),
                        -100,
                        expires_tick,
                    );
                    units[index].status.stunned_until_tick =
                        units[index].status.stunned_until_tick.max(expires_tick);
                    let dot_expires_tick = expires_tick
                        .checked_add(1)
                        .expect("Entangling Roots damage-over-time expiry overflow");
                    apply_timed_damage_over_time(
                        &mut units[index].status,
                        ModifierId(profile.ability.0),
                        profile.damage_per_second,
                        u16::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz fits u16"),
                        completed_tick,
                        dot_expires_tick,
                    );
                }
            }
        }
    }
    result
}

fn effective_movement_speed(unit: &UnitSnapshot) -> i32 {
    let count = usize::from(unit.status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let percent = unit.status.movement_modifiers[..count]
        .iter()
        .fold(100_i32, |total, modifier| {
            total
                .checked_add(i32::from(modifier.percent_delta))
                .expect("movement percentage overflow")
        })
        .clamp(0, 1_000);
    i32::try_from(i64::from(unit.movement.speed_per_tick) * i64::from(percent) / 100)
        .expect("effective movement speed overflowed validated bounds")
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

fn offset_point(point: SimPoint, x: i32, y: i32) -> Option<SimPoint> {
    Some(SimPoint::new(
        point.x.checked_add(x)?,
        point.y.checked_add(y)?,
    ))
}

fn exact_overlap_direction(a: SimId, b: SimId) -> (i32, i32) {
    debug_assert_ne!(a, b);
    let (low, high, sign) = if a < b { (a.0, b.0, -1) } else { (b.0, a.0, 1) };
    let axis = (low.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ high.rotate_left(17)) & 1;
    if axis == 0 { (sign, 0) } else { (0, sign) }
}

fn sidestep_sign(id: SimId) -> i32 {
    let mixed = id.0 ^ id.0.rotate_left(21) ^ 0x9e37_79b9_7f4a_7c15;
    if mixed & 1 == 0 { -1 } else { 1 }
}

fn perpendicular_step_with_side(
    side: i8,
    from: SimPoint,
    toward: SimPoint,
    distance: i32,
) -> SimPoint {
    debug_assert!(side == -1 || side == 1);
    let dx = i64::from(toward.x) - i64::from(from.x);
    let dy = i64::from(toward.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return SimPoint::new(0, i32::from(side) * distance);
    }
    let side = i64::from(side);
    let raw = SimPoint::new(
        i32::try_from(-dy * side).expect("sidestep x overflow"),
        i32::try_from(dx * side).expect("sidestep y overflow"),
    );
    SimPoint::new(0, 0).step_towards(raw, distance)
}

fn pursuit_arc_step(
    side: i8,
    from: SimPoint,
    toward: SimPoint,
    distance: i32,
    forward: bool,
) -> SimPoint {
    debug_assert!(side == -1 || side == 1);
    let dx = i64::from(toward.x) - i64::from(from.x);
    let dy = i64::from(toward.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return perpendicular_step_with_side(side, from, toward, distance);
    }
    let side = i64::from(side);
    let forward_sign = if forward { 1_i64 } else { -1_i64 };
    let raw_x = dx * forward_sign - dy * side;
    let raw_y = dy * forward_sign + dx * side;
    let raw = SimPoint::new(
        i32::try_from(raw_x).expect("pursuit arc x overflow"),
        i32::try_from(raw_y).expect("pursuit arc y overflow"),
    );
    SimPoint::new(0, 0).step_towards(raw, distance)
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

fn building_attack_envelope_goal(
    source: SimPoint,
    footprint: BuildingFootprint,
    max_range: i32,
    cell_size: i32,
) -> SimPoint {
    let closest = closest_point_on_footprint(source, footprint, cell_size);
    point_attack_envelope_goal(source, closest, max_range)
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
