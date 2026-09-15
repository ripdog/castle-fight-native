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
pub const CANONICAL_CHECKSUM_SCHEMA_VERSION: u32 = 4;
const ATTACK_PROC_CHANCE_SCALE: u16 = 10_000;
const DIRECT_RETALIATION_RANGE_MULTIPLIER: i32 = 3;
const AVOIDANCE_CLEAR_TICKS: u8 = 8;

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
        MovementProfile, NavigationGoal, NavigationState, PassiveUnitEffect, PassiveUnitEffects,
        PendingAttackEffects, Position, ProductionArmorProfile, ProductionAttackTargets,
        ProductionCollisionRadius, ProductionContentIdentity, ProductionCorpseProfile,
        ProductionDamageType, ProductionHealthRegeneration, ProductionMovementClass,
        ProductionPassiveEffects, ProductionProfile, ProductionSpellcastingProfile,
        ProductionState, ProductionUnitRepairMetadata, ReflectedProjectile, RepairTimeTicks,
        ResolvedUnitDefinition, RetaliationState, SimId, SpawnTick, SpellcastingProfile,
        StatusState, TargetState, Team, TimedArmorModifier, TimedAttackSpeedModifier,
        TimedDamageOverTime, TriggeredAttackEffect, UnitGameplayProperties, UnitSpawn,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
    pub expires_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub content: Option<ContentIdentity>,
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
    UnitOccupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderSpawnError {
    UnsupportedTeam,
    TeamAlreadyHasBuilder,
    OutsideBuildRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderCommandError {
    BuilderNotFound,
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
    player_resources: [PlayerResources; 2],
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
        Self::new_internal(config, workers, combat_rules, None)
    }

    pub fn new_with_gameplay_bundle(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
        gameplay_bundle: GameplayBundleIdentity,
    ) -> Self {
        Self::new_internal(config, workers, combat_rules, Some(gameplay_bundle))
    }

    fn new_internal(
        config: SimulationConfig,
        workers: usize,
        combat_rules: CombatRules,
        gameplay_bundle: Option<GameplayBundleIdentity>,
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
        let configuration_identity =
            canonical_configuration_identity(&config, &combat_rules, gameplay_bundle);
        let starting_resources = PlayerResources {
            gold: config.economy.starting_gold,
            lumber: config.economy.starting_lumber,
            legendary_points_used: 0,
            legendary_points_cap: config.economy.starting_legendary_points,
        };

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
            player_resources: [starting_resources; 2],
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
    pub fn player_resources(&self, team: Team) -> Option<PlayerResources> {
        self.player_resources.get(usize::from(team.0)).copied()
    }

    /// Adds resources directly to one player's authoritative economy state for developer tooling.
    ///
    /// This intentionally lives on `Simulation` rather than allowing debug UI to mutate economy
    /// storage directly, so cheats still cross the same authoritative-state boundary as gameplay
    /// commands. Values saturate instead of wrapping, and invalid teams are rejected.
    pub fn debug_grant_player_resources(&mut self, team: Team, gold: u32, lumber: u32) -> bool {
        let Some(resources) = self.player_resources.get_mut(usize::from(team.0)) else {
            return false;
        };
        resources.gold = resources.gold.saturating_add(gold);
        resources.lumber = resources.lumber.saturating_add(lumber);
        true
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
                &Team,
                &Position,
                &mut Health,
                Option<&CorpseProducer>,
                &MovementProfile,
            )>();
            for (entity, id, team, position, mut health, corpse, _) in
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
                        *team,
                        position.0,
                        corpse.map(|corpse| corpse.0),
                    ));
                }
            }
        }

        fatalities.sort_unstable_by_key(|(_, id, _, _, _)| *id);
        for (entity, source_unit, source_team, position, corpse) in fatalities {
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
                    source_team,
                    definition: profile.definition,
                    created_tick: self.next_tick,
                    expires_tick,
                },
            ));
        }

        affected
    }

    #[must_use]
    pub fn player_income(&self, team: Team) -> Option<u32> {
        (team.0 < 2).then(|| {
            taxed_income_from_fixed(
                self.raw_player_income_per_10k(team),
                self.config.economy.income_tax_bracket_per_10k,
            )
        })
    }

    #[must_use]
    pub fn player_economy(&self, team: Team) -> Option<PlayerEconomyView> {
        let resources = self.player_resources(team)?;
        let interval = self.config.economy.income_interval_ticks;
        let (progress, ticks_until_income) = if interval == 0 {
            (0, 0)
        } else {
            let phase = u32::try_from(self.next_tick % u64::from(interval))
                .expect("income phase fits interval width");
            let progress =
                u16::try_from(u64::from(phase) * RESOURCE_FIXED_SCALE / u64::from(interval))
                    .expect("income progress is at most 10,000");
            let remaining = if phase == 0 {
                interval
            } else {
                interval - phase
            };
            (progress, remaining)
        };
        Some(PlayerEconomyView {
            resources,
            income: self.player_income(team).expect("validated player team"),
            income_interval_ticks: interval,
            income_progress_per_10k: progress,
            ticks_until_income,
        })
    }

    #[must_use]
    pub fn can_afford_building(&self, team: Team, economy: BuildingEconomyProfile) -> bool {
        self.player_resources(team).is_some_and(|resources| {
            resources.gold >= economy.gold_cost && resources.lumber >= economy.lumber_cost
        })
    }

    #[must_use]
    pub fn can_builder_afford_building(
        &self,
        builder: SimId,
        economy: BuildingEconomyProfile,
    ) -> bool {
        let Some(entity) = self.world.iter_entities().find(|entity| {
            entity.get::<SimId>().copied() == Some(builder) && entity.get::<Builder>().is_some()
        }) else {
            return false;
        };
        let Some(team) = entity.get::<Team>().copied() else {
            return false;
        };
        let Some(resources) = self.player_resources(team) else {
            return false;
        };
        let committed = entity
            .get::<BuilderBuildOrder>()
            .and_then(|order| order.properties.economy);
        let available_gold = resources
            .gold
            .saturating_add(committed.map_or(0, |old| old.gold_cost));
        let available_lumber = resources
            .lumber
            .saturating_add(committed.map_or(0, |old| old.lumber_cost));
        available_gold >= economy.gold_cost && available_lumber >= economy.lumber_cost
    }

    pub fn order_building_attack_target(
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

    fn raw_player_income_per_10k(&self, team: Team) -> u64 {
        self.world
            .iter_entities()
            .filter(|entity| entity.get::<Team>() == Some(&team))
            .filter_map(|entity| entity.get::<BuildingEconomyProfile>())
            .fold(self.config.economy.base_income_per_10k, |total, economy| {
                total
                    .checked_add(economy.income_per_10k)
                    .expect("player income overflow")
            })
    }

    pub fn spawn_builder(&mut self, builder: BuilderSpawn) -> SimId {
        self.try_spawn_builder(builder)
            .expect("invalid authored builder spawn")
    }

    pub fn try_spawn_builder(&mut self, builder: BuilderSpawn) -> Result<SimId, BuilderSpawnError> {
        if builder.team.0 >= 2 {
            return Err(BuilderSpawnError::UnsupportedTeam);
        }
        assert!(builder.profile.speed_per_tick >= 0);
        assert!(builder.profile.build_range >= 0);
        assert!(builder.profile.repair_range >= 0);
        assert!(builder.profile.repair_autocast_range >= builder.profile.repair_range);
        assert!(builder.profile.repair_time_ratio_numerator > 0);
        assert!(builder.profile.repair_time_ratio_denominator > 0);
        assert!(builder.profile.full_repair_duration_ticks > 0);
        assert!(builder.profile.blink_range >= 0);
        assert!(builder.profile.blink_boundary_inset >= 0);
        if self.world.iter_entities().any(|entity| {
            entity.get::<Builder>().is_some() && entity.get::<Team>() == Some(&builder.team)
        }) {
            return Err(BuilderSpawnError::TeamAlreadyHasBuilder);
        }
        if !self.point_inside_team_build_region(builder.team, builder.position) {
            return Err(BuilderSpawnError::OutsideBuildRegion);
        }

        let id = self.allocate_id();
        self.world.spawn((
            id,
            builder.team,
            Position(builder.position),
            Builder,
            builder.profile,
            builder.configuration,
            BuilderState {
                repair_autocast_enabled: builder.repair_autocast_enabled,
                ..BuilderState::default()
            },
        ));
        Ok(id)
    }

    pub fn configure_builder(
        &mut self,
        builder: SimId,
        profile: BuilderProfile,
        configuration: BuilderConfiguration,
    ) -> Result<(), BuilderCommandError> {
        assert!(profile.speed_per_tick >= 0);
        assert!(profile.build_range >= 0);
        assert!(profile.repair_range >= 0);
        assert!(profile.repair_autocast_range >= profile.repair_range);
        assert!(profile.repair_time_ratio_numerator > 0);
        assert!(profile.repair_time_ratio_denominator > 0);
        assert!(profile.full_repair_duration_ticks > 0);
        assert!(profile.blink_range >= 0);
        assert!(profile.blink_boundary_inset >= 0);
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        let mut builder_entity = self.world.entity_mut(entity);
        *builder_entity
            .get_mut::<BuilderProfile>()
            .expect("builder missing profile") = profile;
        *builder_entity
            .get_mut::<BuilderConfiguration>()
            .expect("builder missing configuration") = configuration;
        Ok(())
    }

    pub fn order_builder_move(
        &mut self,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<(), BuilderCommandError> {
        let (entity, team) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if !self.point_inside_team_build_region(team, destination) {
            return Err(BuilderCommandError::OutsideBuildRegion);
        }

        self.cancel_builder_build_order_internal(entity);
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = Some(destination);
        state.follow_target = None;
        state.repair_target = None;
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub fn order_builder_follow(
        &mut self,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        let builder_entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if target == builder || self.builder_follow_target(target).is_none() {
            return Err(BuilderCommandError::FollowTargetNotFound);
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let mut entity = self.world.entity_mut(builder_entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = None;
        state.follow_target = Some(target);
        state.repair_target = None;
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub fn order_builder_blink(
        &mut self,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<SimPoint, BuilderCommandError> {
        let (entity, team, position, profile) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                        entity
                            .get::<Position>()
                            .expect("builder missing position")
                            .0,
                        *entity
                            .get::<BuilderProfile>()
                            .expect("builder missing profile"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if position.distance_sq(destination) > square_i32(profile.blink_range) {
            return Err(BuilderCommandError::BlinkOutOfRange);
        }
        let destination = self
            .clamp_builder_blink_destination(team, destination, profile.blink_boundary_inset)
            .expect("spawned builder team must have a legal movement region");

        self.cancel_builder_build_order_internal(entity);
        let mut builder = self.world.entity_mut(entity);
        builder
            .get_mut::<Position>()
            .expect("builder missing position")
            .0 = destination;
        let mut state = builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        let repair_autocast_enabled = state.repair_autocast_enabled;
        *state = BuilderState {
            repair_autocast_enabled,
            ..BuilderState::default()
        };
        Ok(destination)
    }

    pub fn order_builder_repair(
        &mut self,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        let (builder_entity, builder_team) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        let (target_team, repairable) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(target)
                    && entity
                        .get::<Health>()
                        .is_some_and(|health| health.current > 0))
                .then(|| {
                    (
                        *entity.get::<Team>().expect("repair target missing team"),
                        entity.get::<BuildingFootprint>().is_some()
                            || entity.get::<MechanicalUnit>().is_some(),
                    )
                })
            })
            .ok_or(BuilderCommandError::RepairTargetNotFound)?;
        if builder_team != target_team {
            return Err(BuilderCommandError::NotFriendlyRepairTarget);
        }
        if !repairable {
            return Err(BuilderCommandError::RepairTargetNotRepairable);
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let mut entity = self.world.entity_mut(builder_entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = None;
        state.follow_target = None;
        state.repair_target = Some(target);
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub fn set_builder_repair_autocast(
        &mut self,
        builder: SimId,
        enabled: bool,
    ) -> Result<(), BuilderCommandError> {
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        self.world
            .entity_mut(entity)
            .get_mut::<BuilderState>()
            .expect("builder missing command state")
            .repair_autocast_enabled = enabled;
        Ok(())
    }

    pub fn stop_builder(&mut self, builder: SimId) -> Result<(), BuilderCommandError> {
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        self.cancel_builder_build_order_internal(entity);
        let mut builder = self.world.entity_mut(entity);
        let mut state = builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        let repair_autocast_enabled = state.repair_autocast_enabled;
        *state = BuilderState {
            repair_autocast_enabled,
            ..BuilderState::default()
        };
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_builder_summon_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuilderBuildError> {
        self.validate_builder_summon(builder, building, properties)?;
        self.try_spawn_building_with_properties(building, properties)
            .map_err(BuilderBuildError::Placement)
    }

    #[cfg(test)]
    pub(crate) fn try_builder_purchase_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuilderBuildError> {
        self.validate_builder_summon(builder, building, properties)?;
        let economy = properties
            .economy
            .ok_or(BuilderBuildError::MissingEconomyProfile)?;
        let resources = self.player_resources[usize::from(building.team.0)];
        if resources.gold < economy.gold_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    available: resources.gold,
                    required: economy.gold_cost,
                },
            ));
        }
        if resources.lumber < economy.lumber_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: resources.lumber,
                    required: economy.lumber_cost,
                },
            ));
        }

        let id = self
            .try_spawn_building_with_properties(building, properties)
            .map_err(BuilderBuildError::Placement)?;
        let resources = &mut self.player_resources[usize::from(building.team.0)];
        resources.gold -= economy.gold_cost;
        resources.lumber -= economy.lumber_cost;
        resources.lumber = resources
            .lumber
            .checked_add(economy.lumber_refund)
            .expect("player lumber overflow");
        Ok(id)
    }

    pub fn order_builder_purchase_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<(), BuilderBuildError> {
        let builder_entity = self.validate_builder_summon(builder, building, properties)?;
        self.validate_building_placement(building.team, building.footprint)
            .map_err(BuilderBuildError::Placement)?;
        let economy = properties
            .economy
            .ok_or(BuilderBuildError::MissingEconomyProfile)?;
        let current_order = self
            .world
            .entity(builder_entity)
            .get::<BuilderBuildOrder>()
            .copied();
        let current_economy = current_order.and_then(|order| order.properties.economy);
        let resources = self.player_resources[usize::from(building.team.0)];
        let available_gold = resources
            .gold
            .checked_add(current_economy.map_or(0, |old| old.gold_cost))
            .expect("player gold availability overflow");
        let available_lumber = resources
            .lumber
            .checked_add(current_economy.map_or(0, |old| old.lumber_cost))
            .expect("player lumber availability overflow");
        if available_gold < economy.gold_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    available: available_gold,
                    required: economy.gold_cost,
                },
            ));
        }
        if available_lumber < economy.lumber_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: available_lumber,
                    required: economy.lumber_cost,
                },
            ));
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let resources = &mut self.player_resources[usize::from(building.team.0)];
        resources.gold -= economy.gold_cost;
        resources.lumber -= economy.lumber_cost;
        self.world
            .entity_mut(builder_entity)
            .insert(BuilderBuildOrder {
                building,
                properties,
            });
        let mut builder_entity_mut = self.world.entity_mut(builder_entity);
        let mut builder_state = builder_entity_mut
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        builder_state.destination = None;
        builder_state.follow_target = None;
        builder_state.repair_target = None;
        builder_state.repair_progress_remainder = 0;
        Ok(())
    }

    fn cancel_builder_build_order_internal(&mut self, builder_entity: Entity) -> bool {
        let order = self
            .world
            .entity(builder_entity)
            .get::<BuilderBuildOrder>()
            .copied();
        let Some(order) = order else {
            return false;
        };
        self.world
            .entity_mut(builder_entity)
            .remove::<BuilderBuildOrder>();
        if let Some(economy) = order.properties.economy {
            let resources = &mut self.player_resources[usize::from(order.building.team.0)];
            resources.gold = resources
                .gold
                .checked_add(economy.gold_cost)
                .expect("player gold refund overflow");
            resources.lumber = resources
                .lumber
                .checked_add(economy.lumber_cost)
                .expect("player lumber refund overflow");
        }
        true
    }

    fn validate_builder_summon(
        &self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<Entity, BuilderBuildError> {
        let (builder_entity, builder_team) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                    )
                })
            })
            .ok_or(BuilderBuildError::Builder(
                BuilderCommandError::BuilderNotFound,
            ))?;
        if builder_team != building.team {
            return Err(BuilderBuildError::TeamMismatch);
        }
        let building_rawcode = properties
            .content
            .ok_or(BuilderBuildError::MissingBuildingIdentity)?
            .rawcode;
        let building_allowed = self
            .world
            .entity(builder_entity)
            .get::<BuilderConfiguration>()
            .expect("builder missing configuration")
            .allows_building(building_rawcode);
        if !building_allowed {
            return Err(BuilderBuildError::BuildingNotInCatalog);
        }
        Ok(builder_entity)
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
        let unit = UnitSpawn::from_template(team, position, definition.template);
        validate_unit_spawn(unit);
        self.validate_unit_gameplay_properties(position, definition.properties);
        if let Some(spellcasting) = definition.spellcasting {
            validate_spellcasting_profile(spellcasting);
        }
        self.spawn_unit_unchecked(unit, definition.properties, definition.spellcasting)
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
        self.spawn_unit_unchecked(unit, properties, Some(spellcasting))
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
        self.spawn_unit_unchecked(unit, properties, None)
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

    pub fn spawn_building(&mut self, building: BuildingSpawn) -> SimId {
        self.spawn_building_with_properties(building, BuildingGameplayProperties::default())
    }

    pub fn spawn_building_with_properties(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> SimId {
        self.try_spawn_building_with_properties(building, properties)
            .expect("invalid authored building placement")
    }

    pub fn spawn_building_with_attack_targets(
        &mut self,
        building: BuildingSpawn,
        attack_targets: AttackTargetMask,
    ) -> SimId {
        self.spawn_building_with_properties(
            building,
            BuildingGameplayProperties {
                attack_targets,
                ..BuildingGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_corpse(
        &mut self,
        building: BuildingSpawn,
        corpse: CorpseProfile,
    ) -> SimId {
        self.spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                corpse: Some(corpse),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn try_spawn_building(
        &mut self,
        building: BuildingSpawn,
    ) -> Result<SimId, BuildingPlacementError> {
        self.try_spawn_building_with_properties(building, BuildingGameplayProperties::default())
    }

    pub fn try_spawn_building_with_properties(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        self.try_spawn_building_internal(building, properties)
    }

    pub fn try_spawn_building_with_production_corpse(
        &mut self,
        building: BuildingSpawn,
        corpse: CorpseProfile,
    ) -> Result<SimId, BuildingPlacementError> {
        self.try_spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                corpse: Some(corpse),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_collision_radius(
        &mut self,
        building: BuildingSpawn,
        collision_radius: CollisionRadius,
    ) -> SimId {
        self.spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                collision_radius: Some(collision_radius),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_properties(
        &mut self,
        building: BuildingSpawn,
        properties: UnitGameplayProperties,
    ) -> SimId {
        self.try_spawn_building_with_production_properties(building, properties)
            .expect("invalid authored building placement")
    }

    pub fn try_spawn_building_with_production_properties(
        &mut self,
        building: BuildingSpawn,
        properties: UnitGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        assert!(
            building.production.is_some(),
            "production unit properties require a production building"
        );
        if let Some(corpse) = properties.corpse {
            validate_corpse_profile(corpse);
        }
        if let Some(collision_radius) = properties.collision_radius {
            validate_collision_radius(collision_radius);
        }
        self.try_spawn_building_internal(
            building,
            BuildingGameplayProperties {
                production_unit: properties,
                ..BuildingGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_spellcasting(
        &mut self,
        building: BuildingSpawn,
        spellcasting: SpellcastingProfile,
    ) -> SimId {
        self.try_spawn_building_with_production_spellcasting(building, spellcasting)
            .expect("invalid authored building placement")
    }

    pub fn try_spawn_building_with_production_spellcasting(
        &mut self,
        building: BuildingSpawn,
        spellcasting: SpellcastingProfile,
    ) -> Result<SimId, BuildingPlacementError> {
        assert!(
            building.production.is_some(),
            "production spellcasting profile requires a production building"
        );
        validate_spellcasting_profile(spellcasting);
        self.try_spawn_building_internal(
            building,
            BuildingGameplayProperties {
                production_spellcasting: Some(spellcasting),
                ..BuildingGameplayProperties::default()
            },
        )
    }

    fn try_spawn_building_internal(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        self.validate_building_definition(building, properties);
        self.validate_building_placement(building.team, building.footprint)?;
        let (id, entity) = self.spawn_building_shell(building, properties);
        self.activate_building_entity(entity, building, properties);
        self.topology_dirty = true;
        Ok(id)
    }

    fn try_start_building_construction(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        let duration_ticks = properties
            .construction_time_ticks
            .expect("construction start requires authored construction duration");
        assert!(
            duration_ticks > 0,
            "building construction time must be positive"
        );
        self.validate_building_definition(building, properties);
        self.validate_building_placement(building.team, building.footprint)?;

        let complete_tick = self
            .next_tick
            .checked_add(u64::from(duration_ticks))
            .expect("building construction tick overflow");
        let (id, entity) = self.spawn_building_shell(building, properties);
        self.world.entity_mut(entity).insert(BuildingConstruction {
            started_tick: self.next_tick,
            complete_tick,
            building,
            properties,
            upgrade_from: None,
        });
        self.topology_dirty = true;
        Ok(id)
    }

    fn validate_building_definition(
        &self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) {
        assert!(building.health > 0);
        assert!(building.team.0 < 2, "verification slice supports two teams");
        assert!(building.footprint.width > 0 && building.footprint.height > 0);
        if let Some(construction_time_ticks) = properties.construction_time_ticks {
            assert!(
                construction_time_ticks > 0,
                "building construction time must be positive"
            );
        }
        if let Some(production) = building.production {
            assert!(production.interval_ticks > 0);
            validate_unit_template(production.unit);
            if let Some(corpse) = properties.production_unit.corpse {
                validate_corpse_profile(corpse);
            }
            if let Some(collision_radius) = properties.production_unit.collision_radius {
                validate_collision_radius(collision_radius);
            }
            if properties.production_unit.mechanical {
                assert!(
                    properties
                        .production_unit
                        .repair_time_ticks
                        .is_some_and(|ticks| ticks > 0),
                    "mechanical production units require positive repair-time metadata"
                );
            }
            if let Some(spellcasting) = properties.production_spellcasting {
                validate_spellcasting_profile(spellcasting);
            }
        }
        if let Some(attack) = building.attack {
            validate_attack_profile(attack);
        }
        if let Some(spellcasting) = building.spellcasting {
            validate_spellcasting_profile(spellcasting);
        }
        if let Some(repair_time_ticks) = properties.repair_time_ticks {
            assert!(
                repair_time_ticks > 0,
                "building repair time must be positive"
            );
        }
    }

    fn spawn_building_shell(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> (SimId, Entity) {
        let id = self.allocate_id();
        let mut entity = self.world.spawn((
            id,
            building.team,
            building.footprint,
            Health {
                current: building.health,
                max: building.health,
            },
            properties.damage_type,
            properties.armor,
        ));
        if let Some(content) = properties.content {
            entity.insert(content);
        }
        let entity_id = entity.id();
        (id, entity_id)
    }

    fn activate_building_entity(
        &mut self,
        entity: Entity,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) {
        let mut entity = self.world.entity_mut(entity);
        if let Some(economy) = properties.economy {
            entity.insert(economy);
        }
        if let Some(repair_time_ticks) = properties.repair_time_ticks {
            entity.insert(RepairTimeTicks(repair_time_ticks));
        }
        if let Some(production) = building.production {
            let next_spawn_tick = self
                .next_tick
                .checked_add(u64::from(production.initial_delay_ticks))
                .expect("initial production tick overflow");
            entity.insert((
                production,
                ProductionState { next_spawn_tick },
                ProductionMovementClass(properties.production_unit.movement_class),
                ProductionUnitRepairMetadata {
                    mechanical: properties.production_unit.mechanical,
                    build_time_ticks: properties.production_unit.build_time_ticks,
                    repair_time_ticks: properties.production_unit.repair_time_ticks,
                },
                ProductionAttackTargets(properties.production_unit.attack_targets),
                ProductionHealthRegeneration(
                    properties.production_unit.health_regen_per_second_per_10k,
                ),
            ));
            if let Some(content) = properties.production_unit.content {
                entity.insert(ProductionContentIdentity(content));
            }
            if let Some(corpse) = properties.production_unit.corpse {
                entity.insert(ProductionCorpseProfile(corpse));
            }
            if let Some(collision_radius) = properties.production_unit.collision_radius {
                entity.insert(ProductionCollisionRadius(collision_radius));
            }
            entity.insert((
                ProductionDamageType(properties.production_unit.damage_type),
                ProductionArmorProfile(properties.production_unit.armor),
                ProductionPassiveEffects(properties.production_unit.passive_effects),
            ));
            if let Some(spellcasting) = properties.production_spellcasting {
                entity.insert(ProductionSpellcastingProfile(spellcasting));
            }
        }
        if let Some(attack) = building.attack {
            entity.insert((
                attack,
                properties.attack_targets,
                AttackCooldown::default(),
                TargetState::default(),
                SpawnTick(self.next_tick),
            ));
        }
        if let Some(spellcasting) = building.spellcasting {
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
        if building.attack.is_some() || building.spellcasting.is_some() {
            entity.insert(StatusState::default());
        }
    }

    fn deactivate_building_entity(&mut self, entity: Entity) {
        let mut entity = self.world.entity_mut(entity);
        entity.remove::<BuildingEconomyProfile>();
        entity.remove::<RepairTimeTicks>();
        entity.remove::<ProductionProfile>();
        entity.remove::<ProductionState>();
        entity.remove::<ProductionContentIdentity>();
        entity.remove::<ProductionCorpseProfile>();
        entity.remove::<ProductionCollisionRadius>();
        entity.remove::<ProductionMovementClass>();
        entity.remove::<ProductionUnitRepairMetadata>();
        entity.remove::<ProductionAttackTargets>();
        entity.remove::<ProductionHealthRegeneration>();
        entity.remove::<ProductionDamageType>();
        entity.remove::<ProductionArmorProfile>();
        entity.remove::<ProductionPassiveEffects>();
        entity.remove::<ProductionSpellcastingProfile>();
        entity.remove::<AttackProfile>();
        entity.remove::<AttackTargetMask>();
        entity.remove::<AttackCooldown>();
        entity.remove::<TargetState>();
        entity.remove::<SpawnTick>();
        entity.remove::<SpellcastingProfile>();
        entity.remove::<ManaState>();
        entity.remove::<AutomaticAbilityState>();
        entity.remove::<StatusState>();
    }

    fn restore_building_runtime_state(&mut self, entity: Entity, runtime: BuildingRuntimeState) {
        let mut entity = self.world.entity_mut(entity);
        match runtime.production {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<ProductionState>();
            }
        }
        match runtime.attack_cooldown {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<AttackCooldown>();
            }
        }
        match runtime.target {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<TargetState>();
            }
        }
        match runtime.spawn_tick {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<SpawnTick>();
            }
        }
        match runtime.mana {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<ManaState>();
            }
        }
        match runtime.ability_state {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<AutomaticAbilityState>();
            }
        }
        match runtime.status {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<StatusState>();
            }
        }
    }

    pub fn start_building_upgrade(
        &mut self,
        source_id: SimId,
        source_building: BuildingSpawn,
        source_properties: BuildingGameplayProperties,
        target_building: BuildingSpawn,
        target_properties: BuildingGameplayProperties,
    ) -> Result<(), BuildingUpgradeError> {
        self.validate_building_definition(source_building, source_properties);
        self.validate_building_definition(target_building, target_properties);
        let duration_ticks = target_properties
            .construction_time_ticks
            .expect("Castle Fight building upgrades require authored construction time");
        let target_economy = target_properties
            .economy
            .ok_or(BuildingUpgradeError::MissingEconomyProfile)?;

        let Some((entity, actual_team, actual_footprint, actual_health, actual_content, runtime)) =
            self.world.iter_entities().find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(source_id)
                    && entity.get::<BuildingFootprint>().is_some())
                .then(|| {
                    Some((
                        entity.id(),
                        *entity.get::<Team>()?,
                        *entity.get::<BuildingFootprint>()?,
                        *entity.get::<Health>()?,
                        entity.get::<ContentIdentity>().copied(),
                        BuildingRuntimeState {
                            production: entity.get::<ProductionState>().copied(),
                            attack_cooldown: entity.get::<AttackCooldown>().copied(),
                            target: entity.get::<TargetState>().copied(),
                            spawn_tick: entity.get::<SpawnTick>().copied(),
                            mana: entity.get::<ManaState>().copied(),
                            ability_state: entity.get::<AutomaticAbilityState>().copied(),
                            status: entity.get::<StatusState>().copied(),
                        },
                    ))
                })?
            })
        else {
            return Err(BuildingUpgradeError::SourceNotFound);
        };
        if self
            .world
            .entity(entity)
            .get::<BuildingConstruction>()
            .is_some()
        {
            return Err(BuildingUpgradeError::SourceUnderConstruction);
        }
        if source_building.team != actual_team || target_building.team != actual_team {
            return Err(BuildingUpgradeError::TeamMismatch);
        }
        if source_building.footprint != actual_footprint
            || target_building.footprint != actual_footprint
        {
            return Err(BuildingUpgradeError::FootprintMismatch);
        }
        if actual_content != source_properties.content {
            return Err(BuildingUpgradeError::SourceDefinitionMismatch);
        }

        let resources = &mut self.player_resources[usize::from(actual_team.0)];
        if resources.gold < target_economy.gold_cost {
            return Err(BuildingUpgradeError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    required: target_economy.gold_cost,
                    available: resources.gold,
                },
            ));
        }
        if resources.lumber < target_economy.lumber_cost {
            return Err(BuildingUpgradeError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    required: target_economy.lumber_cost,
                    available: resources.lumber,
                },
            ));
        }
        resources.gold -= target_economy.gold_cost;
        resources.lumber -= target_economy.lumber_cost;

        let complete_tick = self
            .next_tick
            .checked_add(u64::from(duration_ticks))
            .expect("building upgrade completion tick overflow");
        let upgrade_from = BuildingUpgradeSource {
            building: source_building,
            properties: source_properties,
            health: actual_health,
            runtime,
        };

        self.deactivate_building_entity(entity);
        let target_health = scale_building_health(
            actual_health.current,
            actual_health.max,
            target_building.health,
        );
        let mut entity_mut = self.world.entity_mut(entity);
        *entity_mut
            .get_mut::<Health>()
            .expect("upgrade source building missing health") = Health {
            current: target_health,
            max: target_building.health,
        };
        *entity_mut
            .get_mut::<DamageType>()
            .expect("upgrade source building missing damage type") = target_properties.damage_type;
        *entity_mut
            .get_mut::<ArmorProfile>()
            .expect("upgrade source building missing armor profile") = target_properties.armor;
        match target_properties.content {
            Some(content) => {
                entity_mut.insert(content);
            }
            None => {
                entity_mut.remove::<ContentIdentity>();
            }
        }
        // An upgrading Castle Fight production building still represents its precursor's
        // completed economic investment until the upgrade completes.
        if let Some(economy) = source_properties.economy {
            entity_mut.insert(economy);
        }
        entity_mut.insert(BuildingConstruction {
            started_tick: self.next_tick,
            complete_tick,
            building: target_building,
            properties: target_properties,
            upgrade_from: Some(upgrade_from),
        });
        Ok(())
    }

    #[must_use]
    pub fn remove_building(&mut self, id: SimId) -> bool {
        let entity = self.world.iter_entities().find_map(|entity| {
            (entity.get::<SimId>().copied() == Some(id)
                && entity.get::<BuildingFootprint>().is_some())
            .then_some(entity.id())
        });
        let Some(entity) = entity else {
            return false;
        };
        self.world.despawn(entity);
        self.topology_dirty = true;
        true
    }

    pub fn cancel_building_construction(
        &mut self,
        team: Team,
        id: SimId,
    ) -> Result<BuildingConstructionCancelOutcome, BuildingConstructionCancelError> {
        let Some((entity, owner, construction, current_health)) =
            self.world.iter_entities().find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(id)).then(|| {
                    Some((
                        entity.id(),
                        *entity.get::<Team>()?,
                        *entity.get::<BuildingConstruction>()?,
                        *entity.get::<Health>()?,
                    ))
                })?
            })
        else {
            return Err(BuildingConstructionCancelError::ConstructionNotFound);
        };
        if owner != team {
            return Err(BuildingConstructionCancelError::NotOwner);
        }

        if let Some(economy) = construction.properties.economy {
            let resources = &mut self.player_resources[usize::from(team.0)];
            resources.gold = resources
                .gold
                .checked_add(economy.gold_cost)
                .expect("player gold refund overflow");
            resources.lumber = resources
                .lumber
                .checked_add(economy.lumber_cost)
                .expect("player lumber refund overflow");
        }
        let outcome = if let Some(source) = construction.upgrade_from {
            self.world
                .entity_mut(entity)
                .remove::<BuildingConstruction>();
            self.deactivate_building_entity(entity);
            let restored_health = scale_building_health(
                current_health.current,
                current_health.max,
                source.health.max,
            );
            {
                let mut entity_mut = self.world.entity_mut(entity);
                *entity_mut
                    .get_mut::<Health>()
                    .expect("upgrade cancellation source missing health") = Health {
                    current: restored_health,
                    max: source.health.max,
                };
                *entity_mut
                    .get_mut::<DamageType>()
                    .expect("upgrade cancellation source missing damage type") =
                    source.properties.damage_type;
                *entity_mut
                    .get_mut::<ArmorProfile>()
                    .expect("upgrade cancellation source missing armor profile") =
                    source.properties.armor;
                match source.properties.content {
                    Some(content) => {
                        entity_mut.insert(content);
                    }
                    None => {
                        entity_mut.remove::<ContentIdentity>();
                    }
                }
            }
            self.activate_building_entity(entity, source.building, source.properties);
            self.restore_building_runtime_state(entity, source.runtime);
            BuildingConstructionCancelOutcome::RevertedUpgrade
        } else {
            self.world.despawn(entity);
            self.topology_dirty = true;
            BuildingConstructionCancelOutcome::RemovedNewBuilding
        };
        Ok(outcome)
    }

    fn advance_building_construction(&mut self) {
        let mut completing: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let construction = *entity.get::<BuildingConstruction>()?;
                (construction.complete_tick <= self.next_tick).then_some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    construction,
                ))
            })
            .collect();
        completing.sort_unstable_by_key(|(id, ..)| *id);

        for (_, entity, construction) in completing {
            self.world
                .entity_mut(entity)
                .remove::<BuildingConstruction>();
            self.activate_building_entity(entity, construction.building, construction.properties);
            if let Some(economy) = construction.properties.economy {
                let resources =
                    &mut self.player_resources[usize::from(construction.building.team.0)];
                resources.lumber = resources
                    .lumber
                    .checked_add(economy.lumber_refund)
                    .expect("player lumber reward overflow");
            }
        }
    }

    fn validate_building_placement(
        &self,
        team: Team,
        footprint: BuildingFootprint,
    ) -> Result<(), BuildingPlacementError> {
        if !self.footprint_inside_navigation(footprint) {
            return Err(BuildingPlacementError::OutsideNavigation);
        }
        if !self.footprint_inside_team_build_region(team, footprint) {
            return Err(BuildingPlacementError::OutsideBuildRegion);
        }
        if self
            .config
            .static_blockers
            .iter()
            .chain(self.config.build_static_blockers.iter())
            .copied()
            .any(|blocker| footprints_overlap(blocker, footprint))
        {
            return Err(BuildingPlacementError::StaticObstacle);
        }
        if self
            .world
            .iter_entities()
            .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
            .any(|existing| footprints_overlap(existing, footprint))
        {
            return Err(BuildingPlacementError::BuildingOverlap);
        }
        if self.footprint_contains_live_unit(footprint) {
            return Err(BuildingPlacementError::UnitOccupied);
        }
        Ok(())
    }

    #[must_use]
    pub fn can_place_building(&self, footprint: BuildingFootprint) -> bool {
        self.footprint_inside_navigation(footprint)
            && !self
                .config
                .static_blockers
                .iter()
                .chain(self.config.build_static_blockers.iter())
                .copied()
                .any(|blocker| footprints_overlap(blocker, footprint))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
                .any(|existing| footprints_overlap(existing, footprint))
            && !self.footprint_contains_live_unit(footprint)
    }

    #[must_use]
    pub fn can_place_building_for_team(&self, team: Team, footprint: BuildingFootprint) -> bool {
        self.can_place_building(footprint)
            && self.footprint_inside_team_build_region(team, footprint)
    }

    /// Returns whether one navigation cell is individually legal for building placement.
    ///
    /// This is presentation-facing diagnostic data for footprint previews. Whole-building
    /// placement must still use [`Self::can_place_building_for_team`], which remains authoritative
    /// for rules that apply to the footprint as a whole.
    #[must_use]
    pub fn can_place_building_cell_for_team(&self, team: Team, cell: NavCell) -> bool {
        let footprint = BuildingFootprint::new(cell.x, cell.y, 1, 1);
        self.topology.contains(cell)
            && self.cell_inside_team_build_region(team, cell)
            && !self
                .config
                .static_blockers
                .iter()
                .chain(self.config.build_static_blockers.iter())
                .copied()
                .any(|blocker| footprint_contains_cell(blocker, cell))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
                .any(|existing| footprint_contains_cell(existing, cell))
            && !self.footprint_contains_live_unit(footprint)
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
        self.last_attacks.clear();
        self.last_ability_casts.clear();
        self.last_chain_lightnings.clear();
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

        let mut chain_query = self.world.query::<(Entity, &SimId, &ChainLightningState)>();
        let mut due_chain_lightnings: Vec<_> = chain_query
            .iter(&self.world)
            .filter_map(|(entity, id, state)| {
                (chain_lightning_jump_due_tick(state.started_tick, state.next_jump_index)
                    <= completed_tick)
                    .then_some((entity, *id, *state))
            })
            .collect();
        due_chain_lightnings.sort_unstable_by_key(|(_, id, _)| *id);
        let mut chain_lightning_updates = Vec::new();
        let mut chain_lightning_entities_to_remove = Vec::new();
        for (entity, _, mut state) in due_chain_lightnings {
            let origin = find_unit_index(&units, state.current_target)
                .filter(|index| unit_health[*index] > 0)
                .map_or(state.last_position, |index| positions[index]);
            let radius_sq = square_i32(state.profile.jump_radius);
            let hit_count = usize::from(state.hit_count);
            let next = units
                .iter()
                .enumerate()
                .filter(|(candidate_index, candidate)| {
                    unit_health[*candidate_index] > 0
                        && candidate.team != state.source_team
                        && state
                            .profile
                            .targets
                            .can_target_unit(candidate.movement_class)
                        && !state.hit_targets[..hit_count].contains(&candidate.id)
                        && origin.distance_sq(positions[*candidate_index]) <= radius_sq
                })
                .min_by_key(|(candidate_index, candidate)| {
                    (
                        origin.distance_sq(positions[*candidate_index]),
                        candidate.id,
                    )
                })
                .map(|(candidate_index, _)| candidate_index);
            let Some(next_index) = next else {
                chain_lightning_entities_to_remove.push(entity);
                continue;
            };

            let adjusted = self
                .combat_rules
                .damage_rules
                .apply_spell(state.next_damage, units[next_index].armor.armor_type);
            let adjusted = spell_damage_after_defend(units[next_index], adjusted, completed_tick);
            unit_health[next_index] = unit_health[next_index]
                .checked_sub(adjusted)
                .expect("Chain Lightning damage overflow");
            let next_position = positions[next_index];
            let mut points = [SimPoint::default(); MAX_BOUNCE_HITS + 1];
            points[0] = origin;
            points[1] = next_position;
            self.last_chain_lightnings.push(ChainLightningEvent {
                source: state.source,
                ability: state.profile.ability,
                bounce_index: state.next_jump_index,
                points,
                point_count: 2,
            });

            let hit_index = usize::from(state.hit_count);
            debug_assert!(hit_index < MAX_BOUNCE_HITS);
            state.hit_targets[hit_index] = units[next_index].id;
            state.hit_count = state
                .hit_count
                .checked_add(1)
                .expect("Chain Lightning hit count overflow");
            state.current_target = units[next_index].id;
            state.last_position = next_position;

            if usize::from(state.hit_count)
                >= usize::from(state.profile.maximum_targets).min(MAX_BOUNCE_HITS)
            {
                chain_lightning_entities_to_remove.push(entity);
                continue;
            }
            state.next_damage = scaled_chain_lightning_damage(
                state.next_damage,
                state.profile.damage_reduction_per_10k,
            );
            if state.next_damage <= 0 {
                chain_lightning_entities_to_remove.push(entity);
                continue;
            }
            state.next_jump_index = state
                .next_jump_index
                .checked_add(1)
                .expect("Chain Lightning jump index overflow");
            chain_lightning_updates.push((entity, state));
        }

        let phase_start = Instant::now();
        let due_projectiles = self.snapshot_due_projectiles();
        let due_reflected_projectiles = self.snapshot_due_reflected_projectiles();
        let due_bounce_projectiles = self.snapshot_due_bounce_projectiles();
        let due_ballistic_projectiles = self.snapshot_due_ballistic_projectiles();
        let due_target_projectile_count =
            due_projectiles.len() + due_reflected_projectiles.len() + due_bounce_projectiles.len();
        let mut due_target_projectiles = Vec::with_capacity(due_target_projectile_count);
        due_target_projectiles.extend(
            due_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::GuaranteedHit),
        );
        due_target_projectiles.extend(
            due_reflected_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::Reflected),
        );
        due_target_projectiles.extend(
            due_bounce_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::Bounce),
        );
        due_target_projectiles.sort_unstable_by_key(DueTargetProjectileSnapshot::id);
        let mut projectile_entities_to_remove =
            Vec::with_capacity(due_target_projectile_count + due_ballistic_projectiles.len());
        let mut bounce_projectile_updates = Vec::with_capacity(due_target_projectile_count);
        let mut projectile_impacts = 0usize;
        let mut projectile_effects = 0usize;
        let mut projectile_invalidations = 0usize;
        let mut ballistic_candidate_checks = 0usize;
        let mut bounce_jumps = 0usize;
        let mut bounce_candidate_checks = 0usize;
        let mut chain_lightning_launches = Vec::new();
        let mut reflected_projectile_launches = Vec::new();
        for snapshot in due_target_projectiles {
            match snapshot {
                DueTargetProjectileSnapshot::GuaranteedHit(snapshot) => {
                    projectile_entities_to_remove.push(snapshot.entity);
                    let Some(target) =
                        find_target_index(&units, &buildings, snapshot.projectile.target)
                    else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    let Some(impact_position) = live_target_position(
                        target,
                        &units,
                        &buildings,
                        &unit_health,
                        &building_health,
                        self.config.navigation_cell_size,
                    ) else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    let defense = resolve_directed_projectile_defense(
                        target,
                        snapshot.id,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.config.match_seed,
                        &units,
                    );
                    if defense.reflected
                        && !snapshot.projectile.source_is_building
                        && let Some(source_index) =
                            find_unit_index(&units, snapshot.projectile.source)
                                .filter(|index| unit_health[*index] > 0)
                    {
                        let source_position = positions[source_index];
                        let impact_tick = completed_tick
                            .checked_add(projectile_travel_ticks(
                                impact_position.distance_sq(source_position),
                                snapshot.projectile.speed_per_tick,
                            ))
                            .expect("reflected projectile impact tick overflow");
                        reflected_projectile_launches.push(ReflectedProjectileLaunch {
                            original_source: snapshot.projectile.source,
                            reflector: target_sim_id(target, &units, &buildings),
                            reflector_team: match target {
                                TargetIndex::Unit(index) => units[index].team,
                                TargetIndex::Building(index) => buildings[index].team,
                            },
                            target: snapshot.projectile.source,
                            damage: snapshot.projectile.damage,
                            damage_type: snapshot.projectile.damage_type,
                            launch_position: impact_position,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    projectile_impacts += 1;
                    if defense.damage <= 0 {
                        continue;
                    }
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        defense.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units: &units,
                            buildings: &buildings,
                            unit_positions: &positions,
                            unit_health: &mut unit_health,
                            building_health: &mut building_health,
                            attackers_this_tick: &mut attackers_this_tick,
                            next_defense_alerts: &mut next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        if !defense.reflected {
                            let pending = apply_pending_attack_effects(
                                target,
                                snapshot.projectile.on_hit,
                                PendingAttackEffectSource {
                                    id: snapshot.projectile.source,
                                    position: snapshot.projectile.launch_position,
                                    team: snapshot.projectile.source_team,
                                },
                                PendingAttackEffectState {
                                    completed_tick,
                                    units: &mut units,
                                    unit_health: &mut unit_health,
                                    damage_rules: self.combat_rules.damage_rules,
                                },
                            );
                            if let Some(event) = pending.chain_event {
                                self.last_chain_lightnings.push(event);
                            }
                            if let Some(state) = pending.chain_state {
                                chain_lightning_launches.push(state);
                            }
                        }
                        projectile_effects += 1;
                    } else {
                        projectile_invalidations += 1;
                    }
                }
                DueTargetProjectileSnapshot::Reflected(snapshot) => {
                    projectile_entities_to_remove.push(snapshot.entity);
                    let Some(target) =
                        find_target_index(&units, &buildings, snapshot.projectile.target)
                    else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.reflector,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units: &units,
                            buildings: &buildings,
                            unit_positions: &positions,
                            unit_health: &mut unit_health,
                            building_health: &mut building_health,
                            attackers_this_tick: &mut attackers_this_tick,
                            next_defense_alerts: &mut next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        projectile_impacts += 1;
                        projectile_effects += 1;
                    } else {
                        projectile_invalidations += 1;
                    }
                }
                DueTargetProjectileSnapshot::Bounce(snapshot) => {
                    let Some(target) =
                        find_target_index(&units, &buildings, snapshot.projectile.target)
                    else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    };
                    let Some(impact_position) = live_target_position(
                        target,
                        &units,
                        &buildings,
                        &unit_health,
                        &building_health,
                        self.config.navigation_cell_size,
                    ) else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    };
                    let defense = resolve_directed_projectile_defense(
                        target,
                        snapshot.id,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.config.match_seed,
                        &units,
                    );
                    if defense.reflected {
                        projectile_entities_to_remove.push(snapshot.entity);
                        if !snapshot.projectile.source_is_building
                            && let Some(source_index) =
                                find_unit_index(&units, snapshot.projectile.source)
                                    .filter(|index| unit_health[*index] > 0)
                        {
                            let source_position = positions[source_index];
                            reflected_projectile_launches.push(ReflectedProjectileLaunch {
                                original_source: snapshot.projectile.source,
                                reflector: target_sim_id(target, &units, &buildings),
                                reflector_team: match target {
                                    TargetIndex::Unit(index) => units[index].team,
                                    TargetIndex::Building(index) => buildings[index].team,
                                },
                                target: snapshot.projectile.source,
                                damage: snapshot.projectile.damage,
                                damage_type: snapshot.projectile.damage_type,
                                launch_position: impact_position,
                                launch_tick: completed_tick,
                                impact_tick: completed_tick
                                    .checked_add(projectile_travel_ticks(
                                        impact_position.distance_sq(source_position),
                                        snapshot.projectile.speed_per_tick,
                                    ))
                                    .expect("reflected projectile impact tick overflow"),
                            });
                        }
                        if defense.damage <= 0 {
                            projectile_impacts += 1;
                            continue;
                        }
                    }
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        defense.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units: &units,
                            buildings: &buildings,
                            unit_positions: &positions,
                            unit_health: &mut unit_health,
                            building_health: &mut building_health,
                            attackers_this_tick: &mut attackers_this_tick,
                            next_defense_alerts: &mut next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_none()
                    {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    }
                    projectile_impacts += 1;
                    projectile_effects += 1;

                    if defense.reflected || snapshot.projectile.remaining_bounces == 0 {
                        projectile_entities_to_remove.push(snapshot.entity);
                        continue;
                    }

                    let bounce_search = BounceSearchContext {
                        completed_tick,
                        units: &units,
                        unit_health: &unit_health,
                        grid: &grid,
                    };
                    let Some(next_index) = self.select_bounce_target(
                        snapshot.id,
                        &snapshot.projectile,
                        impact_position,
                        &bounce_search,
                        &mut bounce_candidate_checks,
                    ) else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        continue;
                    };
                    let next_target = units[next_index].id;
                    let next_position = units[next_index].position;
                    let mut projectile = snapshot.projectile;
                    let next_bounce_index = projectile
                        .bounce_index
                        .checked_add(1)
                        .expect("bounce index overflow");
                    let hit_index = usize::from(projectile.hit_count);
                    debug_assert!(hit_index < MAX_BOUNCE_HITS);
                    projectile.hit_targets[hit_index] = next_target;
                    projectile.hit_count = projectile
                        .hit_count
                        .checked_add(1)
                        .expect("bounce hit count overflow");
                    projectile.target = next_target;
                    projectile.damage = scaled_bounce_damage(
                        projectile.damage,
                        projectile.damage_percent_per_bounce,
                    );
                    projectile.launch_position = impact_position;
                    projectile.launch_tick = completed_tick;
                    projectile.impact_tick = completed_tick
                        .checked_add(projectile_travel_ticks(
                            impact_position.distance_sq(next_position),
                            projectile.speed_per_tick,
                        ))
                        .expect("bounce impact tick overflow");
                    projectile.remaining_bounces -= 1;
                    projectile.bounce_index = next_bounce_index;
                    bounce_projectile_updates.push(BounceProjectileUpdate {
                        entity: snapshot.entity,
                        projectile,
                    });
                    bounce_jumps += 1;
                }
            }
        }

        let mut intents = self.attack_intents(&units, &buildings);
        intents.sort_unstable_by_key(|intent| (intent.source_id, intent.target_id));

        let mut attacks_resolved = 0;
        let mut projectile_launches = Vec::new();
        let mut ballistic_projectile_launches = Vec::new();
        let mut bounce_projectile_launches = Vec::new();
        for intent in intents {
            let source_alive = match intent.source {
                AttackSourceIndex::Unit(index) => unit_health[index] > 0,
                AttackSourceIndex::Building(index) => building_health[index] > 0,
            };
            if !source_alive {
                continue;
            }
            let Some(target_position) = live_target_position(
                intent.target,
                &units,
                &buildings,
                &unit_health,
                &building_health,
                self.config.navigation_cell_size,
            ) else {
                continue;
            };

            let missed =
                self.uphill_attack_misses(&intent, target_position, completed_tick, &units)
                    || self.attack_is_evaded(&intent, &units, completed_tick);
            if !missed {
                let (bonus_damage, on_hit) =
                    self.resolve_passive_attack_effects(&intent, &units, completed_tick);
                let damage = intent
                    .attack
                    .damage
                    .checked_add(bonus_damage)
                    .expect("attack plus passive bonus damage overflowed");
                match intent.attack.delivery {
                    AttackDelivery::Melee => {
                        let applied = apply_damage_to_target(
                            intent.target,
                            intent.source_id,
                            damage,
                            intent.damage_type,
                            completed_tick,
                            DamageTargetState {
                                damage_rules: self.combat_rules.damage_rules,
                                units: &units,
                                buildings: &buildings,
                                unit_positions: &positions,
                                unit_health: &mut unit_health,
                                building_health: &mut building_health,
                                attackers_this_tick: &mut attackers_this_tick,
                                next_defense_alerts: &mut next_defense_alerts,
                                navigation_cell_size: self.config.navigation_cell_size,
                            },
                        );
                        debug_assert!(applied.is_some());
                        let pending = apply_pending_attack_effects(
                            intent.target,
                            on_hit,
                            PendingAttackEffectSource {
                                id: intent.source_id,
                                position: intent.source_position,
                                team: intent.source_team,
                            },
                            PendingAttackEffectState {
                                completed_tick,
                                units: &mut units,
                                unit_health: &mut unit_health,
                                damage_rules: self.combat_rules.damage_rules,
                            },
                        );
                        if let Some(event) = pending.chain_event {
                            self.last_chain_lightnings.push(event);
                        }
                        if let Some(state) = pending.chain_state {
                            chain_lightning_launches.push(state);
                        }
                        if let (
                            AttackSourceIndex::Unit(source_index),
                            TargetIndex::Unit(target_index),
                        ) = (intent.source, intent.target)
                        {
                            apply_melee_reactive_armor_effects(
                                source_index,
                                target_index,
                                completed_tick,
                                &mut units,
                                &unit_health,
                            );
                        }
                    }
                    AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        projectile_launches.push(ProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            source_is_building: matches!(
                                intent.source,
                                AttackSourceIndex::Building(_)
                            ),
                            target: intent.target_id,
                            damage,
                            on_hit,
                            damage_type: intent.damage_type,
                            speed_per_tick,
                            launch_position: intent.source_position,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    AttackDelivery::RangedBallistic {
                        speed_per_tick,
                        impact_radius,
                    } => {
                        assert_eq!(
                            bonus_damage, 0,
                            "ballistic passive bonus damage is unsupported"
                        );
                        assert_eq!(
                            (on_hit.stun_duration_ticks, on_hit.triggered_spell),
                            (0, None),
                            "ballistic stun/triggered-spell passives are unsupported"
                        );
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        ballistic_projectile_launches.push(BallisticProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            target_mask: intent.attack_targets,
                            damage: intent.attack.damage,
                            burning_oil: on_hit.burning_oil,
                            damage_type: intent.damage_type,
                            launch_position: intent.source_position,
                            destination: target_position,
                            impact_radius,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    AttackDelivery::Bounce {
                        speed_per_tick,
                        bounce_range,
                        max_bounces,
                        damage_percent_per_bounce,
                        allow_repeat_targets,
                    } => {
                        assert_eq!(
                            (bonus_damage, on_hit),
                            (0, PendingAttackEffects::default()),
                            "passive on-hit effects are not yet defined for bounce attacks"
                        );
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        bounce_projectile_launches.push(BounceProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            source_is_building: matches!(
                                intent.source,
                                AttackSourceIndex::Building(_)
                            ),
                            target_mask: intent.attack_targets,
                            target: intent.target_id,
                            damage: intent.attack.damage,
                            damage_type: intent.damage_type,
                            launch_position: intent.source_position,
                            launch_tick: completed_tick,
                            impact_tick,
                            speed_per_tick,
                            bounce_range,
                            max_bounces,
                            damage_percent_per_bounce,
                            allow_repeat_targets,
                        });
                    }
                }
            }
            match intent.source {
                AttackSourceIndex::Unit(index) => {
                    cooldowns[index] = effective_attack_cooldown_ticks(
                        intent.attack.cooldown_ticks,
                        units[index].status,
                    );
                    attack_sequences[index] = attack_sequences[index]
                        .checked_add(1)
                        .expect("unit attack sequence overflow");
                }
                AttackSourceIndex::Building(index) => {
                    building_cooldowns[index] = intent.attack.cooldown_ticks;
                }
            }
            self.last_attacks.push(AttackEvent {
                source: intent.source_id,
                target: intent.target_id,
                source_position: intent.source_position,
                target_position,
                delivery: intent.attack.delivery,
                missed,
            });
            attacks_resolved += 1;
        }
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
        let mut burning_oil_zone_launches = Vec::new();
        if !due_ballistic_projectiles.is_empty() {
            let impact_grid = SpatialGrid::build(
                self.config.spatial_cell_size,
                units
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| unit_health[*index] > 0)
                    .map(|(index, unit)| {
                        (
                            SpatialPartition::global(unit.team.0),
                            index,
                            positions[index],
                        )
                    }),
            );
            for snapshot in due_ballistic_projectiles {
                projectile_entities_to_remove.push(snapshot.entity);
                projectile_impacts += 1;
                let enemy_team = 1u8
                    .checked_sub(snapshot.projectile.source_team.0)
                    .expect("verification slice supports teams 0 and 1 only");
                let radius_sq = square_i32(snapshot.projectile.impact_radius);
                let mut targets = Vec::new();
                impact_grid.for_each_candidate(
                    SpatialPartition::global(enemy_team),
                    snapshot.projectile.destination,
                    snapshot.projectile.impact_radius,
                    |unit_index| {
                        ballistic_candidate_checks += 1;
                        if unit_health[unit_index] > 0
                            && snapshot
                                .projectile
                                .target_mask
                                .can_target_unit(units[unit_index].movement_class)
                            && snapshot
                                .projectile
                                .destination
                                .distance_sq(positions[unit_index])
                                <= radius_sq
                        {
                            targets.push(TargetIndex::Unit(unit_index));
                        }
                    },
                );
                for (building_index, building) in buildings.iter().enumerate() {
                    if !snapshot.projectile.target_mask.can_target_buildings()
                        || building.team.0 != enemy_team
                        || building_health[building_index] <= 0
                    {
                        continue;
                    }
                    ballistic_candidate_checks += 1;
                    if point_to_footprint_distance_sq(
                        snapshot.projectile.destination,
                        building.footprint,
                        self.config.navigation_cell_size,
                    ) <= radius_sq
                    {
                        targets.push(TargetIndex::Building(building_index));
                    }
                }
                targets.sort_unstable_by_key(|target| target_sim_id(*target, &units, &buildings));
                for target in targets {
                    let damage = ranged_projectile_damage_after_defend(
                        target,
                        snapshot.projectile.damage,
                        completed_tick,
                        &units,
                    );
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units: &units,
                            buildings: &buildings,
                            unit_positions: &positions,
                            unit_health: &mut unit_health,
                            building_health: &mut building_health,
                            attackers_this_tick: &mut attackers_this_tick,
                            next_defense_alerts: &mut next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        projectile_effects += 1;
                    }
                }
                if let Some(profile) = snapshot.projectile.burning_oil {
                    burning_oil_zone_launches.push((
                        snapshot.projectile.source,
                        snapshot.projectile.source_team,
                        snapshot.projectile.destination,
                        profile,
                    ));
                }
            }
        }
        let ballistic_impact = phase_start.elapsed();

        let phase_start = Instant::now();
        for entity in chain_lightning_entities_to_remove {
            self.world.despawn(entity);
        }
        for (entity, state) in chain_lightning_updates {
            if let Some(mut stored) = self
                .world
                .entity_mut(entity)
                .get_mut::<ChainLightningState>()
            {
                *stored = state;
            }
        }
        for entity in projectile_entities_to_remove {
            self.world.despawn(entity);
        }
        for update in bounce_projectile_updates {
            *self
                .world
                .entity_mut(update.entity)
                .get_mut::<BounceProjectile>()
                .expect("bounce projectile missing during hop update") = update.projectile;
        }
        for (source, source_team, center, profile) in burning_oil_zone_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                BurningOilZone {
                    source,
                    source_team,
                    center,
                    profile,
                    created_tick: completed_tick,
                    pulse_index: 1,
                },
            ));
        }
        for state in chain_lightning_launches {
            let id = self.allocate_id();
            self.world.spawn((id, state));
        }
        for launch in projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                GuaranteedHitProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    source_is_building: launch.source_is_building,
                    target: launch.target,
                    damage: launch.damage,
                    on_hit: launch.on_hit,
                    damage_type: launch.damage_type,
                    speed_per_tick: launch.speed_per_tick,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in reflected_projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                ReflectedProjectile {
                    original_source: launch.original_source,
                    reflector: launch.reflector,
                    reflector_team: launch.reflector_team,
                    target: launch.target,
                    damage: launch.damage,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in ballistic_projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                BallisticProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    target_mask: launch.target_mask,
                    damage: launch.damage,
                    burning_oil: launch.burning_oil,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    destination: launch.destination,
                    impact_radius: launch.impact_radius,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in bounce_projectile_launches {
            let id = self.allocate_id();
            let mut hit_targets = [SimId(0); MAX_BOUNCE_HITS];
            hit_targets[0] = launch.target;
            self.world.spawn((
                id,
                BounceProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    source_is_building: launch.source_is_building,
                    target_mask: launch.target_mask,
                    target: launch.target,
                    damage: launch.damage,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                    speed_per_tick: launch.speed_per_tick,
                    bounce_range: launch.bounce_range,
                    remaining_bounces: launch.max_bounces,
                    bounce_index: 0,
                    damage_percent_per_bounce: launch.damage_percent_per_bounce,
                    allow_repeat_targets: launch.allow_repeat_targets,
                    hit_targets,
                    hit_count: 1,
                },
            ));
        }

        let mut deaths = 0;
        let mut corpse_spawns = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            if unit_health[index] <= 0 {
                if let Some(profile) = unit.corpse {
                    corpse_spawns.push((unit.id, unit.team, positions[index], profile));
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
        for (source_unit, source_team, position, profile) in corpse_spawns {
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

        self.advance_economy_income();
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
            self.next_tick,
            self.next_id,
            self.configuration_identity,
            &self.defense_alerts,
            &self.player_resources,
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

    #[must_use]
    pub fn checksum(&self) -> u64 {
        canonical_checksum(
            &self.world,
            self.next_tick,
            self.next_id,
            self.configuration_identity,
            &self.defense_alerts,
            &self.player_resources,
        )
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
    pub fn builder_for_team(&self, team: Team) -> Option<BuilderView> {
        self.world
            .iter_entities()
            .filter_map(builder_view_from_entity)
            .find(|builder| builder.team == team)
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

    fn advance_economy_income(&mut self) {
        let interval = self.config.economy.income_interval_ticks;
        if interval == 0 {
            return;
        }
        let elapsed_after_tick = self
            .next_tick
            .checked_add(1)
            .expect("income tick counter overflow");
        if !elapsed_after_tick.is_multiple_of(u64::from(interval)) {
            return;
        }

        for team_index in 0..self.player_resources.len() {
            let team = Team(u8::try_from(team_index).expect("two-player team index fits u8"));
            let income = taxed_income_from_fixed(
                self.raw_player_income_per_10k(team),
                self.config.economy.income_tax_bracket_per_10k,
            );
            self.player_resources[team_index].gold = self.player_resources[team_index]
                .gold
                .checked_add(income)
                .expect("player gold overflow");
        }
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

    fn builder_follow_target(&self, target_id: SimId) -> Option<BuilderFollowTargetSnapshot> {
        let default_collision_radius = self.default_collision_radius();
        self.world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(target_id))
            .and_then(|entity| builder_follow_target_from_entity(entity, default_collision_radius))
    }

    fn builder_repair_target(&self, target_id: SimId) -> Option<BuilderRepairTargetSnapshot> {
        self.world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(target_id))
            .and_then(builder_repair_target_from_entity)
    }

    fn select_builder_autocast_repair_target(
        &self,
        team: Team,
        position: SimPoint,
        acquisition_range: i32,
    ) -> Option<SimId> {
        let max_distance_sq = square_i32(acquisition_range);
        self.world
            .iter_entities()
            .filter_map(builder_repair_target_from_entity)
            .filter(|target| {
                target.team == team
                    && target.health.current > 0
                    && target.health.current < target.health.max
                    && match target.geometry {
                        BuilderRepairGeometry::Building(_) => true,
                        BuilderRepairGeometry::Unit(target_position) => {
                            self.point_inside_team_build_region(team, target_position)
                        }
                    }
            })
            .filter_map(|target| {
                let distance_sq = target.distance_sq(position, self.config.navigation_cell_size);
                (distance_sq <= max_distance_sq).then_some((distance_sq, target.id))
            })
            .min_by_key(|(distance_sq, id)| (*distance_sq, *id))
            .map(|(_, id)| id)
    }

    fn advance_builders(&mut self) {
        let mut builders: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                entity.get::<Builder>()?;
                Some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    *entity.get::<Team>()?,
                    entity.get::<Position>()?.0,
                    *entity.get::<BuilderProfile>()?,
                    *entity.get::<BuilderState>()?,
                    entity.get::<BuilderBuildOrder>().copied(),
                ))
            })
            .collect();
        builders.sort_unstable_by_key(|(id, ..)| *id);

        for (_, builder_entity, team, position, profile, mut state, build_order) in builders {
            let mut next_position = position;

            if let Some(order) = build_order {
                let mut distance_sq = point_to_footprint_distance_sq(
                    next_position,
                    order.building.footprint,
                    self.config.navigation_cell_size,
                );
                if distance_sq > square_i32(profile.build_range) {
                    let candidate = next_position.step_towards(
                        footprint_center_point(
                            order.building.footprint,
                            self.config.navigation_cell_size,
                        ),
                        profile.speed_per_tick,
                    );
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                        distance_sq = point_to_footprint_distance_sq(
                            next_position,
                            order.building.footprint,
                            self.config.navigation_cell_size,
                        );
                    } else {
                        self.cancel_builder_build_order_internal(builder_entity);
                    }
                }

                if self
                    .world
                    .entity(builder_entity)
                    .get::<BuilderBuildOrder>()
                    .is_some()
                    && distance_sq <= square_i32(profile.build_range)
                {
                    let starts_construction = order.properties.construction_time_ticks.is_some();
                    let result = if starts_construction {
                        self.try_start_building_construction(order.building, order.properties)
                    } else {
                        self.try_spawn_building_with_properties(order.building, order.properties)
                    };
                    match result {
                        Ok(_) => {
                            self.world
                                .entity_mut(builder_entity)
                                .remove::<BuilderBuildOrder>();
                            if !starts_construction && let Some(economy) = order.properties.economy
                            {
                                let resources =
                                    &mut self.player_resources[usize::from(order.building.team.0)];
                                resources.lumber = resources
                                    .lumber
                                    .checked_add(economy.lumber_refund)
                                    .expect("player lumber reward overflow");
                            }
                        }
                        Err(_) => {
                            // Construction has not begun yet, so Castle Fight's
                            // ConstructionRefundRate=1 returns the full committed cost.
                            self.cancel_builder_build_order_internal(builder_entity);
                        }
                    }
                }
                self.apply_builder_state(builder_entity, next_position, state);
                continue;
            }

            if state.repair_target.is_none()
                && state.follow_target.is_none()
                && state.destination.is_none()
                && state.repair_autocast_enabled
            {
                state.repair_target = self.select_builder_autocast_repair_target(
                    team,
                    next_position,
                    profile.repair_autocast_range,
                );
                state.repair_progress_remainder = 0;
            }

            if let Some(target_id) = state.repair_target {
                let Some(target) = self.builder_repair_target(target_id) else {
                    state.repair_target = None;
                    state.repair_progress_remainder = 0;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                };
                if target.team != team
                    || target.health.current <= 0
                    || target.health.current >= target.health.max
                {
                    state.repair_target = None;
                    state.repair_progress_remainder = 0;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                }

                let mut distance_sq =
                    target.distance_sq(next_position, self.config.navigation_cell_size);
                if distance_sq > square_i32(profile.repair_range) {
                    let candidate = next_position.step_towards(
                        target.approach_position(self.config.navigation_cell_size),
                        profile.speed_per_tick,
                    );
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                        distance_sq =
                            target.distance_sq(next_position, self.config.navigation_cell_size);
                    } else {
                        state.repair_target = None;
                        state.repair_progress_remainder = 0;
                    }
                }

                if state.repair_target.is_some() && distance_sq <= square_i32(profile.repair_range)
                {
                    let duration = builder_repair_duration_ticks(profile, target);
                    let accumulated = u64::from(state.repair_progress_remainder)
                        + u64::try_from(target.health.max)
                            .expect("repair target max health must be positive");
                    let repaired = accumulated / duration;
                    state.repair_progress_remainder = u32::try_from(accumulated % duration)
                        .expect("repair remainder fits builder state");

                    if repaired > 0 {
                        let mut target_entity = self.world.entity_mut(target.entity);
                        let mut target_health = target_entity
                            .get_mut::<Health>()
                            .expect("repair target lost health component");
                        let repaired = i32::try_from(repaired).unwrap_or(i32::MAX);
                        target_health.current = target_health
                            .current
                            .saturating_add(repaired)
                            .min(target_health.max);
                        if target_health.current >= target_health.max {
                            state.repair_target = None;
                            state.repair_progress_remainder = 0;
                        }
                    }
                }
            } else if let Some(target_id) = state.follow_target {
                let Some(target) = self.builder_follow_target(target_id) else {
                    state.follow_target = None;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                };
                if !target.reached(next_position, self.config.navigation_cell_size) {
                    let approach =
                        target.approach_position(next_position, self.config.navigation_cell_size);
                    let candidate = next_position.step_towards(approach, profile.speed_per_tick);
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                    } else {
                        state.follow_target = None;
                    }
                }
            } else if let Some(destination) = state.destination {
                let candidate = next_position.step_towards(destination, profile.speed_per_tick);
                if self.point_inside_team_build_region(team, candidate) {
                    next_position = candidate;
                    if next_position == destination {
                        state.destination = None;
                    }
                } else {
                    state.destination = None;
                }
            }

            self.apply_builder_state(builder_entity, next_position, state);
        }
    }

    fn apply_builder_state(&mut self, entity: Entity, position: SimPoint, state: BuilderState) {
        let mut builder = self.world.entity_mut(entity);
        builder
            .get_mut::<Position>()
            .expect("builder missing position")
            .0 = position;
        *builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state") = state;
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

    fn resolve_burning_oil_zones(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        completed_tick: u64,
    ) {
        let mut query = self.world.query::<(Entity, &SimId, &BurningOilZone)>();
        let mut zones: Vec<_> = query
            .iter(&self.world)
            .map(|(entity, id, zone)| (entity, *id, *zone))
            .collect();
        zones.sort_unstable_by_key(|(_, id, _)| *id);

        let mut expired = Vec::new();
        let mut updates = Vec::new();
        for (entity, _, mut zone) in zones {
            let expires_tick = zone
                .created_tick
                .checked_add(ceil_millis_to_ticks(u64::from(
                    zone.profile.total_duration_millis,
                )))
                .expect("Burning Oil expiry tick overflow");
            if completed_tick >= expires_tick {
                expired.push(entity);
                continue;
            }

            while let Some((offset_millis, damage)) =
                burning_oil_pulse(zone.profile, zone.pulse_index)
            {
                let due_tick = zone
                    .created_tick
                    .checked_add(ceil_millis_to_ticks(u64::from(offset_millis)))
                    .expect("Burning Oil pulse tick overflow");
                if due_tick > completed_tick {
                    break;
                }
                let radius_sq = square_i32(zone.profile.radius);
                if zone.profile.target_ground_units {
                    for unit in units.iter_mut() {
                        if unit.health <= 0
                            || unit.team == zone.source_team
                            || unit.movement_class != MovementClass::Ground
                            || zone.center.distance_sq(unit.position) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, unit.armor.armor_type);
                        let adjusted = spell_damage_after_defend(*unit, adjusted, completed_tick);
                        unit.health = unit
                            .health
                            .checked_sub(adjusted)
                            .expect("Burning Oil unit damage overflow");
                    }
                }
                if zone.profile.target_buildings {
                    for building in buildings.iter_mut() {
                        if building.health <= 0 || building.team == zone.source_team {
                            continue;
                        }
                        if point_to_footprint_distance_sq(
                            zone.center,
                            building.footprint,
                            self.config.navigation_cell_size,
                        ) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, building.armor.armor_type);
                        building.health = building
                            .health
                            .checked_sub(adjusted)
                            .expect("Burning Oil building damage overflow");
                    }
                }
                zone.pulse_index = zone
                    .pulse_index
                    .checked_add(1)
                    .expect("Burning Oil pulse index overflow");
            }
            updates.push((entity, zone));
        }

        for entity in expired {
            self.world.despawn(entity);
        }
        for (entity, zone) in updates {
            if let Some(mut stored) = self.world.entity_mut(entity).get_mut::<BurningOilZone>() {
                *stored = zone;
            }
        }
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
            .filter(|(_, _, _, _, health, _, _, _, _, _, _, _, _)| health.current > 0)
            .map(
                |(
                    entity,
                    id,
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

    fn snapshot_due_projectiles(&mut self) -> Vec<ProjectileSnapshot> {
        let mut query = self
            .world
            .query::<(Entity, &SimId, &GuaranteedHitProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    fn snapshot_due_reflected_projectiles(&mut self) -> Vec<ReflectedProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &ReflectedProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ReflectedProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    fn snapshot_due_bounce_projectiles(&mut self) -> Vec<BounceProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BounceProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BounceProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    fn snapshot_due_ballistic_projectiles(&mut self) -> Vec<BallisticProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BallisticProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BallisticProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    fn resolve_automatic_abilities(
        &mut self,
        buildings: &mut [BuildingSnapshot],
        units: &mut [UnitSnapshot],
        grid: &SpatialGrid,
    ) -> AbilityMetrics {
        let building_evaluations: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .enumerate()
                .map(|(source_index, source)| {
                    self.evaluate_automatic_ability(
                        AbilitySourceSnapshot {
                            source: AbilitySourceIndex::Building(source_index),
                            id: source.id,
                            team: source.team,
                            origin: AbilitySourceOrigin::Building(source.footprint),
                            health: source.health,
                            stunned_until_tick: source
                                .status
                                .map_or(0, |status| status.stunned_until_tick),
                            spellcasting: source.spellcasting,
                            mana_current: source.mana_current,
                            ability_state: source.ability_state,
                        },
                        units,
                        grid,
                    )
                })
                .collect()
        });
        let unit_evaluations: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(source_index, source)| {
                    self.evaluate_automatic_ability(
                        AbilitySourceSnapshot {
                            source: AbilitySourceIndex::Unit(source_index),
                            id: source.id,
                            team: source.team,
                            origin: AbilitySourceOrigin::Unit(source.position),
                            health: source.health,
                            stunned_until_tick: source.status.stunned_until_tick,
                            spellcasting: source.spellcasting,
                            mana_current: source.mana_current,
                            ability_state: source.ability_state,
                        },
                        units,
                        grid,
                    )
                })
                .collect()
        });
        let mut metrics = AbilityMetrics {
            evaluations: buildings
                .iter()
                .filter(|building| building.spellcasting.is_some())
                .count()
                + units
                    .iter()
                    .filter(|unit| unit.spellcasting.is_some())
                    .count(),
            candidate_checks: building_evaluations
                .iter()
                .chain(&unit_evaluations)
                .map(|evaluation| evaluation.candidate_checks)
                .sum(),
            ..AbilityMetrics::default()
        };
        let mut intents: Vec<_> = building_evaluations
            .into_iter()
            .chain(unit_evaluations)
            .filter_map(|evaluation| evaluation.intent)
            .collect();
        intents.sort_unstable_by_key(|intent| {
            let (target_kind, target_id) = intent.target.sort_key();
            (
                intent.source_id,
                intent.ability.id,
                intent.cast_sequence,
                target_kind,
                target_id,
            )
        });

        for intent in intents {
            let source = match intent.source {
                AbilitySourceIndex::Unit(index) => {
                    let source = &units[index];
                    AbilitySourceSnapshot {
                        source: intent.source,
                        id: source.id,
                        team: source.team,
                        origin: AbilitySourceOrigin::Unit(source.position),
                        health: source.health,
                        stunned_until_tick: source.status.stunned_until_tick,
                        spellcasting: source.spellcasting,
                        mana_current: source.mana_current,
                        ability_state: source.ability_state,
                    }
                }
                AbilitySourceIndex::Building(index) => {
                    let source = &buildings[index];
                    AbilitySourceSnapshot {
                        source: intent.source,
                        id: source.id,
                        team: source.team,
                        origin: AbilitySourceOrigin::Building(source.footprint),
                        health: source.health,
                        stunned_until_tick: source
                            .status
                            .map_or(0, |status| status.stunned_until_tick),
                        spellcasting: source.spellcasting,
                        mana_current: source.mana_current,
                        ability_state: source.ability_state,
                    }
                }
            };
            if source.health <= 0
                || source.id != intent.source_id
                || self.next_tick < source.stunned_until_tick
            {
                continue;
            }
            let Some(spellcasting) = source.spellcasting else {
                continue;
            };
            if spellcasting.ability != intent.ability {
                continue;
            }
            let Some(mut state) = source.ability_state else {
                continue;
            };
            let Some(mana) = source.mana_current else {
                continue;
            };
            if state.cast_sequence != intent.cast_sequence
                || state.ready_tick > self.next_tick
                || mana < intent.ability.mana_cost
                || !self.ability_target_is_valid(source, intent.target, intent.ability, units)
            {
                continue;
            }

            let remaining_mana = mana
                .checked_sub(intent.ability.mana_cost)
                .expect("ability mana cost exceeded validated current mana");
            state.ready_tick = self
                .next_tick
                .checked_add(u64::from(intent.ability.cooldown_ticks))
                .expect("ability cooldown tick overflow");
            state.cast_sequence = state
                .cast_sequence
                .checked_add(1)
                .expect("ability cast sequence exhausted");
            match intent.source {
                AbilitySourceIndex::Unit(index) => {
                    units[index].mana_current = Some(remaining_mana);
                    units[index].ability_state = Some(state);
                }
                AbilitySourceIndex::Building(index) => {
                    buildings[index].mana_current = Some(remaining_mana);
                    buildings[index].ability_state = Some(state);
                }
            }

            let target_position = match intent.target {
                AbilityIntentTarget::Unit { index, .. } => Some(units[index].position),
                AbilityIntentTarget::AllEnemyUnits => None,
            };

            match intent.target {
                AbilityIntentTarget::Unit { index, .. } => {
                    if let AbilityEffect::AreaDamage { radius, .. } = intent.ability.effect {
                        let center = units[index].position;
                        let radius_sq = square_i32(radius);
                        for target in units.iter_mut() {
                            if target.health <= 0
                                || target.team == source.team
                                || center.distance_sq(target.position) > radius_sq
                            {
                                continue;
                            }
                            if apply_ability_effect_to_unit(
                                target,
                                intent.ability.effect,
                                self.next_tick,
                                self.combat_rules.damage_rules,
                            ) {
                                metrics.effects += 1;
                            }
                        }
                    } else if apply_ability_effect_to_unit(
                        &mut units[index],
                        intent.ability.effect,
                        self.next_tick,
                        self.combat_rules.damage_rules,
                    ) {
                        metrics.effects += 1;
                    }
                }
                AbilityIntentTarget::AllEnemyUnits => {
                    for target in units.iter_mut() {
                        if target.health <= 0 || target.team == source.team {
                            continue;
                        }
                        if apply_ability_effect_to_unit(
                            target,
                            intent.ability.effect,
                            self.next_tick,
                            self.combat_rules.damage_rules,
                        ) {
                            metrics.effects += 1;
                        }
                    }
                }
            }
            self.last_ability_casts.push(AbilityCastEvent {
                source: intent.source_id,
                ability: intent.ability.id,
                target: intent.target.cast_target(),
                target_position,
                effect: intent.ability.effect,
            });
            metrics.casts += 1;
        }

        metrics
    }

    fn evaluate_automatic_ability(
        &self,
        source: AbilitySourceSnapshot,
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
    ) -> AbilityEvaluation {
        let Some(spellcasting) = source.spellcasting else {
            return AbilityEvaluation::default();
        };
        if source.health <= 0 || self.next_tick < source.stunned_until_tick {
            return AbilityEvaluation::default();
        }
        let Some(state) = source.ability_state else {
            return AbilityEvaluation::default();
        };
        let Some(mana) = source.mana_current else {
            return AbilityEvaluation::default();
        };
        if state.ready_tick > self.next_tick || mana < spellcasting.ability.mana_cost {
            return AbilityEvaluation::default();
        }

        let mut candidate_checks = 0usize;
        let target = match spellcasting.ability.target_policy {
            AbilityTargetPolicy::RandomEnemyUnit => self
                .random_enemy_ability_target(
                    source,
                    spellcasting.ability,
                    state.cast_sequence,
                    units,
                    grid,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
            AbilityTargetPolicy::RandomEnemyUnitGlobal => self
                .random_enemy_ability_target_global(
                    source,
                    spellcasting.ability,
                    state.cast_sequence,
                    units,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
            AbilityTargetPolicy::AllEnemyUnits => {
                candidate_checks = units.len();
                units
                    .iter()
                    .any(|unit| unit.health > 0 && unit.team != source.team)
                    .then_some(AbilityIntentTarget::AllEnemyUnits)
            }
            AbilityTargetPolicy::RecentlyAttackedFriendlyUnit => self
                .recently_attacked_friendly_ability_target(
                    source,
                    spellcasting.ability,
                    units,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
        };
        AbilityEvaluation {
            intent: target.map(|target| AbilityIntent {
                source: source.source,
                source_id: source.id,
                target,
                ability: spellcasting.ability,
                cast_sequence: state.cast_sequence,
            }),
            candidate_checks,
        }
    }

    fn random_enemy_ability_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        cast_sequence: u64,
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let (center, query_radius) = match source.origin {
            AbilitySourceOrigin::Unit(position) => (position, ability.range),
            AbilitySourceOrigin::Building(footprint) => (
                footprint_center_point(footprint, self.config.navigation_cell_size),
                building_source_query_radius(
                    footprint,
                    ability.range,
                    self.config.navigation_cell_size,
                ),
            ),
        };
        let range_sq = square_i32(ability.range);
        let mut best: Option<(u64, SimId, usize)> = None;
        grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            center,
            query_radius,
            |unit_index| {
                *candidate_checks += 1;
                let candidate = &units[unit_index];
                if candidate.health <= 0
                    || self.ability_source_distance_sq(source.origin, candidate.position) > range_sq
                {
                    return;
                }
                let rank = deterministic_ability_target_rank(
                    self.config.match_seed,
                    source.id,
                    ability.id,
                    cast_sequence,
                    candidate.id,
                );
                let key = (rank, candidate.id, unit_index);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );
        best.map(|(_, _, unit_index)| unit_index)
    }

    fn recently_attacked_friendly_ability_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        units: &[UnitSnapshot],
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let previous_tick = self.next_tick.saturating_sub(1);
        let modifier = match ability.effect {
            AbilityEffect::FrostArmor { modifier, .. } => Some(modifier),
            _ => None,
        };
        let range_sq = square_i32(ability.range);
        units
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                *candidate_checks += 1;
                if candidate.health <= 0
                    || candidate.team != source.team
                    || candidate.retaliation.attacked_tick != Some(previous_tick)
                    || self.ability_source_distance_sq(source.origin, candidate.position) > range_sq
                {
                    return None;
                }
                if modifier.is_some_and(|modifier| {
                    candidate.status.armor_modifiers
                        [..usize::from(candidate.status.armor_modifier_count)]
                        .iter()
                        .any(|active| active.id == modifier && self.next_tick < active.expires_tick)
                }) {
                    return None;
                }
                Some((
                    self.ability_source_distance_sq(source.origin, candidate.position),
                    candidate.id,
                    index,
                ))
            })
            .min()
            .map(|(_, _, index)| index)
    }

    fn random_enemy_ability_target_global(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        cast_sequence: u64,
        units: &[UnitSnapshot],
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let mut best: Option<(u64, SimId, usize)> = None;
        for (unit_index, candidate) in units.iter().enumerate() {
            *candidate_checks += 1;
            if candidate.health <= 0 || candidate.team == source.team {
                continue;
            }
            let rank = deterministic_ability_target_rank(
                self.config.match_seed,
                source.id,
                ability.id,
                cast_sequence,
                candidate.id,
            );
            let key = (rank, candidate.id, unit_index);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, unit_index)| unit_index)
    }

    fn ability_source_distance_sq(&self, source: AbilitySourceOrigin, target: SimPoint) -> u64 {
        match source {
            AbilitySourceOrigin::Unit(position) => position.distance_sq(target),
            AbilitySourceOrigin::Building(footprint) => {
                point_to_footprint_distance_sq(target, footprint, self.config.navigation_cell_size)
            }
        }
    }

    fn ability_target_is_valid(
        &self,
        source: AbilitySourceSnapshot,
        target: AbilityIntentTarget,
        ability: AutomaticAbilityProfile,
        units: &[UnitSnapshot],
    ) -> bool {
        match target {
            AbilityIntentTarget::Unit { index, id } => {
                let target = &units[index];
                target.id == id
                    && target.health > 0
                    && match ability.target_policy {
                        AbilityTargetPolicy::RandomEnemyUnit => {
                            target.team != source.team
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                        }
                        AbilityTargetPolicy::RandomEnemyUnitGlobal => target.team != source.team,
                        AbilityTargetPolicy::AllEnemyUnits => false,
                        AbilityTargetPolicy::RecentlyAttackedFriendlyUnit => {
                            target.team == source.team
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                                && target.retaliation.attacked_tick
                                    == Some(self.next_tick.saturating_sub(1))
                                && match ability.effect {
                                    AbilityEffect::FrostArmor { modifier, .. } => {
                                        !target.status.armor_modifiers
                                            [..usize::from(target.status.armor_modifier_count)]
                                            .iter()
                                            .any(|active| {
                                                active.id == modifier
                                                    && self.next_tick < active.expires_tick
                                            })
                                    }
                                    _ => true,
                                }
                        }
                    }
            }
            AbilityIntentTarget::AllEnemyUnits => {
                ability.target_policy == AbilityTargetPolicy::AllEnemyUnits
                    && units
                        .iter()
                        .any(|target| target.health > 0 && target.team != source.team)
            }
        }
    }

    fn unit_will_query_ally_defense(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if unit.health <= 0 || self.next_tick < unit.status.stunned_until_tick {
            return false;
        }
        let current = unit
            .target
            .filter(|target| self.current_target_retainable_for(unit, *target, units, buildings));
        if current.is_some_and(|target| find_unit_index(units, target).is_some()) {
            return false;
        }
        !self
            .recent_retaliation_target(unit, units, buildings)
            .is_some_and(|attacker| find_unit_index(units, attacker).is_some())
    }

    fn select_targets(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
        defense_attacker_grid: &SpatialGrid,
    ) -> TargetSelectionResult {
        let decisions: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .map(|unit| {
                    if unit.health <= 0 {
                        return TargetDecision::without_defense(None, false, false);
                    }
                    let current = unit.target.filter(|target| {
                        self.current_target_retainable_for(unit, *target, units, buildings)
                    });
                    if self.next_tick < unit.status.stunned_until_tick {
                        return TargetDecision::without_defense(
                            current,
                            current.is_some() && unit.direct_retaliation_lock,
                            current.is_some() && unit.ally_defense_lock,
                        );
                    }

                    let retaliation = self.recent_retaliation_target(unit, units, buildings);
                    if let Some(current) = current
                        && find_unit_index(units, current).is_some()
                    {
                        if unit.direct_retaliation_lock {
                            return TargetDecision::without_defense(Some(current), true, false);
                        }
                        if let Some(attacker) = retaliation
                            && find_unit_index(units, attacker).is_some()
                        {
                            return TargetDecision::without_defense(Some(attacker), true, false);
                        }
                        return TargetDecision::without_defense(
                            Some(current),
                            false,
                            unit.ally_defense_lock,
                        );
                    }

                    if let Some(attacker) = retaliation
                        && find_unit_index(units, attacker).is_some()
                    {
                        return TargetDecision::without_defense(Some(attacker), true, false);
                    }

                    let defense = self.recent_ally_defense_target(
                        unit,
                        units,
                        buildings,
                        defense_attacker_grid,
                        defense_victims,
                        alert_grid,
                    );
                    if let Some(attacker) = defense.target
                        && find_unit_index(units, attacker).is_some()
                    {
                        return TargetDecision::with_defense(Some(attacker), false, true, defense);
                    }

                    if let Some(current) = current {
                        debug_assert!(find_building_index(buildings, current).is_some());
                        if unit.direct_retaliation_lock {
                            return TargetDecision::with_defense(
                                Some(current),
                                true,
                                false,
                                defense,
                            );
                        }
                        if let Some(attacker) = retaliation {
                            return TargetDecision::with_defense(
                                Some(attacker),
                                true,
                                false,
                                defense,
                            );
                        }
                        if unit.ally_defense_lock {
                            return TargetDecision::with_defense(
                                Some(current),
                                false,
                                true,
                                defense,
                            );
                        }
                        if let Some(attacker) = defense.target {
                            return TargetDecision::with_defense(
                                Some(attacker),
                                false,
                                true,
                                defense,
                            );
                        }
                        return TargetDecision::with_defense(Some(current), false, false, defense);
                    }

                    if let Some(attacker) = retaliation {
                        return TargetDecision::with_defense(Some(attacker), true, false, defense);
                    }
                    if let Some(attacker) = defense.target {
                        return TargetDecision::with_defense(Some(attacker), false, true, defense);
                    }

                    TargetDecision::with_defense(
                        self.acquire_target(unit, units, buildings, grid),
                        false,
                        false,
                        defense,
                    )
                })
                .collect()
        });

        let retained_targets = units
            .iter()
            .zip(&decisions)
            .filter(|(unit, decision)| unit.target.is_some() && unit.target == decision.target)
            .count();
        let target_changes = units
            .iter()
            .zip(&decisions)
            .filter(|(unit, decision)| unit.target != decision.target)
            .count();
        TargetSelectionResult {
            ally_defense_queries: decisions
                .iter()
                .filter(|decision| decision.defense_query.queried)
                .count(),
            ally_defense_victim_candidates: decisions
                .iter()
                .map(|decision| decision.defense_query.victim_candidates)
                .sum(),
            ally_defense_attacker_candidates: decisions
                .iter()
                .map(|decision| decision.defense_query.attacker_candidates)
                .sum(),
            decisions,
            retained_targets,
            target_changes,
        }
    }

    fn select_building_targets(
        &self,
        buildings: &[BuildingSnapshot],
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
    ) -> BuildingTargetSelectionResult {
        let decisions: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .map(|building| {
                    let _attack = building.attack?;
                    if building.health <= 0 {
                        return None;
                    }
                    let current = building.target.filter(|target| {
                        self.building_source_target_retainable(building, *target, units, buildings)
                    });
                    if building
                        .status
                        .is_some_and(|status| self.next_tick < status.stunned_until_tick)
                    {
                        return current;
                    }
                    if current.is_some() {
                        return current;
                    }
                    self.acquire_building_target(building, units, buildings, grid)
                })
                .collect()
        });
        let retained_targets = buildings
            .iter()
            .zip(&decisions)
            .filter(|(building, decision)| {
                building.attack.is_some()
                    && building.target.is_some()
                    && building.target == **decision
            })
            .count();
        let target_changes = buildings
            .iter()
            .zip(&decisions)
            .filter(|(building, decision)| {
                building.attack.is_some() && building.target != **decision
            })
            .count();
        BuildingTargetSelectionResult {
            decisions,
            retained_targets,
            target_changes,
        }
    }

    fn acquire_building_target(
        &self,
        source: &BuildingSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
    ) -> Option<SimId> {
        let attack = source.attack?;
        let attack_targets = source
            .attack_targets
            .expect("attack building target mask missing");
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let center = footprint_center_point(source.footprint, self.config.navigation_cell_size);
        let query_radius = building_source_query_radius(
            source.footprint,
            attack.acquisition_range,
            self.config.navigation_cell_size,
        );
        let acquisition_range_sq = attack.acquisition_range_sq();
        let mut best_unit: Option<(u64, SimId)> = None;
        grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            center,
            query_radius,
            |index| {
                let candidate = &units[index];
                if candidate.health <= 0
                    || !attack_targets.can_target_unit(candidate.movement_class)
                {
                    return;
                }
                let distance_sq = point_to_footprint_distance_sq(
                    candidate.position,
                    source.footprint,
                    self.config.navigation_cell_size,
                );
                if distance_sq > acquisition_range_sq {
                    return;
                }
                let key = (distance_sq, candidate.id);
                if best_unit.is_none_or(|current| key < current) {
                    best_unit = Some(key);
                }
            },
        );
        if let Some((_, target)) = best_unit {
            return Some(target);
        }
        if !attack_targets.can_target_buildings() {
            return None;
        }

        let mut best_building: Option<(u64, SimId)> = None;
        for candidate in buildings {
            if candidate.team == source.team || candidate.health <= 0 || candidate.id == source.id {
                continue;
            }
            let distance_sq = footprint_to_footprint_distance_sq(
                source.footprint,
                candidate.footprint,
                self.config.navigation_cell_size,
            );
            if distance_sq > acquisition_range_sq {
                continue;
            }
            let key = (distance_sq, candidate.id);
            if best_building.is_none_or(|current| key < current) {
                best_building = Some(key);
            }
        }
        best_building.map(|(_, target)| target)
    }

    fn building_source_target_retainable(
        &self,
        source: &BuildingSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let Some(attack) = source.attack else {
            return false;
        };
        let attack_targets = source
            .attack_targets
            .expect("attack building target mask missing");
        let range_sq = attack.acquisition_range_sq();
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return target.team != source.team
                && target.health > 0
                && attack_targets.can_target_unit(target.movement_class)
                && point_to_footprint_distance_sq(
                    target.position,
                    source.footprint,
                    self.config.navigation_cell_size,
                ) <= range_sq;
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            return attack_targets.can_target_buildings()
                && target.team != source.team
                && target.health > 0
                && footprint_to_footprint_distance_sq(
                    source.footprint,
                    target.footprint,
                    self.config.navigation_cell_size,
                ) <= range_sq;
        }
        false
    }

    fn acquire_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
    ) -> Option<SimId> {
        let source_cell = self.topology.cell_of_point(source.position);
        let component = (source.movement_class == MovementClass::Ground)
            .then(|| self.topology.component_id(source_cell))
            .flatten();
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let partition = if source.movement_class == MovementClass::Ground
            && matches!(source.attack.delivery, AttackDelivery::Melee)
            && !source.attack_targets.can_target_unit(MovementClass::Air)
        {
            SpatialPartition::new(enemy_team, component?)
        } else {
            SpatialPartition::global(enemy_team)
        };
        let mut best: Option<(u8, u64, SimId)> = None;
        grid.for_each_candidate(
            partition,
            source.position,
            source.attack.acquisition_range,
            |index| {
                let candidate = &units[index];
                debug_assert_ne!(source.team, candidate.team);
                if candidate.health <= 0
                    || !source
                        .attack_targets
                        .can_target_unit(candidate.movement_class)
                {
                    return;
                }
                let distance_sq = source.position.distance_sq(candidate.position);
                if !self.unit_target_reachable(
                    source,
                    candidate,
                    distance_sq,
                    source.attack.acquisition_range_sq(),
                ) {
                    return;
                }
                let key = (0, distance_sq, candidate.id);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );

        if !source.attack_targets.can_target_buildings() {
            return best.map(|(_, _, id)| id);
        }
        for building in buildings {
            if source.team == building.team || building.health <= 0 {
                continue;
            }
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                building.footprint,
                self.config.navigation_cell_size,
            );
            if !self.building_target_reachable(
                source,
                building,
                distance_sq,
                source.attack.acquisition_range_sq(),
            ) {
                continue;
            }
            let key = (1, distance_sq, building.id);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, id)| id)
    }

    fn recent_retaliation_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Option<SimId> {
        let previous_tick = self.next_tick.checked_sub(1)?;
        if source.retaliation.attacked_tick != Some(previous_tick) {
            return None;
        }
        let attacker = source.retaliation.attacker?;
        self.direct_retaliation_target_retainable_for(source, attacker, units, buildings)
            .then_some(attacker)
    }

    fn current_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if source.direct_retaliation_lock {
            self.direct_retaliation_target_retainable_for(source, target_id, units, buildings)
        } else if source.ally_defense_lock {
            self.ally_defense_target_retainable_for(source, target_id, units, buildings)
        } else {
            self.target_retainable_for(source, target_id, units, buildings)
        }
    }

    fn ally_defense_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return source.team != target.team
                && target.health > 0
                && self.unit_target_reachable(
                    source,
                    target,
                    source.position.distance_sq(target.position),
                    u64::MAX,
                );
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            return source.team != target.team
                && target.health > 0
                && self.building_target_reachable(source, target, distance_sq, u64::MAX);
        }
        false
    }

    fn direct_retaliation_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let retaliation_range = source
            .attack
            .acquisition_range
            .checked_mul(DIRECT_RETALIATION_RANGE_MULTIPLIER)
            .expect("direct retaliation range overflowed validated bounds");
        let retaliation_range_sq = square_i32(retaliation_range);
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return source.team != target.team
                && target.health > 0
                && source.attack_targets.can_target_unit(target.movement_class)
                && source.position.distance_sq(target.position) <= retaliation_range_sq;
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            return source.attack_targets.can_target_buildings()
                && source.team != target.team
                && target.health > 0
                && point_to_footprint_distance_sq(
                    source.position,
                    target.footprint,
                    self.config.navigation_cell_size,
                ) <= retaliation_range_sq;
        }
        false
    }

    fn recent_ally_defense_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        defense_attacker_grid: &SpatialGrid,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
    ) -> DefenseTargetSearch {
        if defense_victims.is_empty() {
            return DefenseTargetSearch::default();
        }
        let Some(previous_tick) = self.next_tick.checked_sub(1) else {
            return DefenseTargetSearch::default();
        };
        let mut search = DefenseTargetSearch {
            queried: true,
            ..DefenseTargetSearch::default()
        };
        let context = DefenseSearchContext {
            units,
            buildings,
            attacker_grid: defense_attacker_grid,
        };
        let mut rejected_through_distance = None;
        // Buildings are fallback defense targets. Keep the best one seen, but continue through
        // farther attacked-allies until we know no valid combat-unit attacker exists.
        let mut best_building: Option<(u64, u64, SimId, SimId)> = None;

        loop {
            let Some((ally_distance_sq, victim_indices)) = self.nearest_defense_victim_layer(
                source,
                previous_tick,
                rejected_through_distance,
                defense_victims,
                alert_grid,
                &mut search.victim_candidates,
            ) else {
                search.target = best_building.map(|(_, _, attacker, _)| attacker);
                return search;
            };

            let mut best_unit: Option<(u64, SimId, SimId)> = None;
            for victim_index in victim_indices {
                let victim = &defense_victims[victim_index];
                let Some((attacker_distance_sq, attacker_id)) = self
                    .nearest_valid_defense_attacker(
                        source,
                        victim,
                        &context,
                        victim_index,
                        &mut search.attacker_candidates,
                    )
                else {
                    continue;
                };
                if find_unit_index(units, attacker_id).is_some() {
                    let key = (attacker_distance_sq, attacker_id, victim.victim_id);
                    if best_unit.is_none_or(|current| key < current) {
                        best_unit = Some(key);
                    }
                } else {
                    let key = (
                        ally_distance_sq,
                        attacker_distance_sq,
                        attacker_id,
                        victim.victim_id,
                    );
                    if best_building.is_none_or(|current| key < current) {
                        best_building = Some(key);
                    }
                }
            }

            if let Some((_, attacker, _)) = best_unit {
                search.target = Some(attacker);
                return search;
            }
            rejected_through_distance = Some(ally_distance_sq);
        }
    }

    fn nearest_defense_victim_layer(
        &self,
        source: &UnitSnapshot,
        previous_tick: u64,
        rejected_through_distance: Option<u64>,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
        victim_candidates: &mut usize,
    ) -> Option<(u64, Vec<usize>)> {
        let acquisition_range_sq = source.attack.acquisition_range_sq();
        let mut best_distance = None;
        let mut nearest = Vec::new();
        alert_grid.for_each_candidate_nearest_cells(
            SpatialPartition::global(source.team.0),
            source.position,
            source.attack.acquisition_range,
            acquisition_range_sq,
            |victim_index| {
                *victim_candidates += 1;
                let victim = &defense_victims[victim_index];
                if victim.attacked_tick != previous_tick || victim.victim_id == source.id {
                    return None;
                }
                let distance_sq = source.position.distance_sq(victim.victim_position);
                if distance_sq > acquisition_range_sq
                    || rejected_through_distance.is_some_and(|rejected| distance_sq <= rejected)
                {
                    return None;
                }

                match best_distance {
                    None => {
                        best_distance = Some(distance_sq);
                        nearest.push(victim_index);
                        Some(distance_sq)
                    }
                    Some(current) if distance_sq < current => {
                        best_distance = Some(distance_sq);
                        nearest.clear();
                        nearest.push(victim_index);
                        Some(distance_sq)
                    }
                    Some(current) if distance_sq == current => {
                        nearest.push(victim_index);
                        None
                    }
                    Some(_) => None,
                }
            },
        );
        best_distance.map(|distance| (distance, nearest))
    }

    fn nearest_valid_defense_attacker(
        &self,
        source: &UnitSnapshot,
        victim: &DefenseVictim,
        context: &DefenseSearchContext<'_>,
        victim_index: usize,
        attacker_candidates: &mut usize,
    ) -> Option<(u64, SimId)> {
        let pursuit_range = self.target_pursuit_range(source);
        let pursuit_range_sq = square_i32(pursuit_range);
        let mut best: Option<(u64, SimId)> = None;

        if !victim.unit_attackers.is_empty() {
            context.attacker_grid.for_each_candidate_nearest_cells(
                defense_attacker_partition(victim_index),
                source.position,
                pursuit_range,
                pursuit_range_sq,
                |unit_index| {
                    *attacker_candidates += 1;
                    let candidate = &context.units[unit_index];
                    let distance_sq = source.position.distance_sq(candidate.position);
                    if !self.unit_target_reachable(source, candidate, distance_sq, pursuit_range_sq)
                    {
                        return None;
                    }
                    let key = (distance_sq, candidate.id);
                    if best.is_none_or(|current| key < current) {
                        best = Some(key);
                        Some(distance_sq)
                    } else {
                        None
                    }
                },
            );

            // The nearby attacked ally, not the aggressor's distance, gates a call for help. The
            // spatial lookup above keeps the common case cheap; when it finds no ordinary-range
            // attacker, evaluate that victim's explicit attacker relation beyond the pursuit leash.
            if best.is_none() {
                for &unit_index in &victim.unit_attackers {
                    let candidate = &context.units[unit_index];
                    let distance_sq = source.position.distance_sq(candidate.position);
                    if distance_sq <= pursuit_range_sq {
                        continue;
                    }
                    *attacker_candidates += 1;
                    if !self.unit_target_reachable(source, candidate, distance_sq, u64::MAX) {
                        continue;
                    }
                    let key = (distance_sq, candidate.id);
                    if best.is_none_or(|current| key < current) {
                        best = Some(key);
                    }
                }
            }
        }

        if best.is_some() {
            return best;
        }

        for &building_index in &victim.building_attackers {
            *attacker_candidates += 1;
            let target = &context.buildings[building_index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            if !self.building_target_reachable(source, target, distance_sq, u64::MAX) {
                continue;
            }
            let key = (distance_sq, target.id);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best
    }

    fn select_bounce_target(
        &self,
        projectile_id: SimId,
        projectile: &BounceProjectile,
        impact_position: SimPoint,
        context: &BounceSearchContext<'_>,
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let enemy_team = 1u8
            .checked_sub(projectile.source_team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let range_sq = square_i32(projectile.bounce_range);
        let hit_count = usize::from(projectile.hit_count);
        let hit_targets = &projectile.hit_targets[..hit_count];
        let next_bounce_index = u32::from(projectile.bounce_index) + 1;
        let mut best: Option<(u64, SimId, usize)> = None;
        context.grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            impact_position,
            projectile.bounce_range,
            |unit_index| {
                *candidate_checks += 1;
                if context.unit_health[unit_index] <= 0 {
                    return;
                }
                let candidate = &context.units[unit_index];
                if !projectile
                    .target_mask
                    .can_target_unit(candidate.movement_class)
                    || candidate.id == projectile.target
                    || impact_position.distance_sq(candidate.position) > range_sq
                {
                    return;
                }
                if !projectile.allow_repeat_targets && hit_targets.contains(&candidate.id) {
                    return;
                }
                let rank = deterministic_random(
                    self.config.match_seed,
                    context.completed_tick,
                    projectile_id,
                    RANDOM_PURPOSE_BOUNCE_TARGET ^ candidate.id.0,
                    u64::from(next_bounce_index),
                );
                let key = (rank, candidate.id, unit_index);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );
        best.map(|(_, _, unit_index)| unit_index)
    }

    fn target_pursuit_range(&self, source: &UnitSnapshot) -> i32 {
        source
            .attack
            .range
            .checked_add(self.config.target_pursuit_extra_range)
            .expect("pursuit range overflowed validated coordinate bounds")
            .max(source.attack.acquisition_range)
    }

    fn target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let pursuit_range_sq = square_i32(self.target_pursuit_range(source));
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            let distance_sq = source.position.distance_sq(target.position);
            return source.team != target.team
                && target.health > 0
                && self.unit_target_reachable(source, target, distance_sq, pursuit_range_sq);
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            return source.team != target.team
                && target.health > 0
                && self.building_target_reachable(source, target, distance_sq, pursuit_range_sq);
        }
        false
    }

    fn unit_target_reachable(
        &self,
        source: &UnitSnapshot,
        target: &UnitSnapshot,
        distance_sq: u64,
        pursuit_limit_sq: u64,
    ) -> bool {
        if source.health <= 0
            || target.health <= 0
            || distance_sq > pursuit_limit_sq
            || !source.attack_targets.can_target_unit(target.movement_class)
        {
            return false;
        }
        let in_attack_range = distance_sq <= source.attack.range_sq();
        if source.movement_class == MovementClass::Air {
            return in_attack_range || source.movement.speed_per_tick > 0;
        }
        if target.movement_class == MovementClass::Air {
            if in_attack_range {
                return true;
            }
            if source.movement.speed_per_tick == 0 {
                return false;
            }
            let source_cell = self.topology.cell_of_point(source.position);
            return self
                .nearest_reachable_unit_attack_cell(
                    source_cell,
                    source.position,
                    target.position,
                    source.attack.range,
                    source.collision_radius_override,
                )
                .is_some();
        }
        match source.attack.delivery {
            AttackDelivery::Melee => {
                self.topology.same_component(
                    self.topology.cell_of_point(source.position),
                    self.topology.cell_of_point(target.position),
                ) && (in_attack_range || source.movement.speed_per_tick > 0)
            }
            AttackDelivery::RangedGuaranteedHit { .. }
            | AttackDelivery::RangedBallistic { .. }
            | AttackDelivery::Bounce { .. } => {
                in_attack_range
                    || (source.movement.speed_per_tick > 0
                        && self.topology.same_component(
                            self.topology.cell_of_point(source.position),
                            self.topology.cell_of_point(target.position),
                        ))
            }
        }
    }

    fn building_target_reachable(
        &self,
        source: &UnitSnapshot,
        target: &BuildingSnapshot,
        distance_sq: u64,
        pursuit_limit_sq: u64,
    ) -> bool {
        if source.health <= 0
            || target.health <= 0
            || distance_sq > pursuit_limit_sq
            || !source.attack_targets.can_target_buildings()
        {
            return false;
        }
        let in_attack_range = distance_sq <= source.attack.range_sq();
        if source.movement_class == MovementClass::Air {
            return in_attack_range || source.movement.speed_per_tick > 0;
        }
        if matches!(
            source.attack.delivery,
            AttackDelivery::RangedGuaranteedHit { .. }
                | AttackDelivery::RangedBallistic { .. }
                | AttackDelivery::Bounce { .. }
        ) && in_attack_range
        {
            return true;
        }
        (in_attack_range || source.movement.speed_per_tick > 0)
            && self
                .topology
                .nearest_reachable_perimeter_cell(
                    self.topology.cell_of_point(source.position),
                    target.footprint,
                )
                .is_some()
    }

    fn uphill_attack_misses(
        &self,
        intent: &AttackIntent,
        target_position: SimPoint,
        completed_tick: u64,
        units: &[UnitSnapshot],
    ) -> bool {
        let chance = self.combat_rules.uphill_miss_chance_per_10k;
        let (AttackSourceIndex::Unit(source_index), TargetIndex::Unit(target_index)) =
            (intent.source, intent.target)
        else {
            return false;
        };
        if chance == 0
            || units[source_index].movement_class == MovementClass::Air
            || units[target_index].movement_class == MovementClass::Air
        {
            return false;
        }

        let terrain = self
            .combat_rules
            .terrain_elevation
            .as_ref()
            .expect("uphill miss chance requires authoritative terrain elevation");
        let source_level = terrain
            .cliff_level_at(intent.source_position)
            .expect("attacking unit position lies outside authoritative terrain elevation");
        let target_level = terrain
            .cliff_level_at(target_position)
            .expect("target unit position lies outside authoritative terrain elevation");
        if target_level <= source_level {
            return false;
        }

        deterministic_random(
            self.config.match_seed,
            completed_tick,
            intent.source_id,
            RANDOM_PURPOSE_UPHILL_MISS,
            intent.attack_sequence,
        ) % u64::from(UPHILL_MISS_CHANCE_SCALE)
            < u64::from(chance)
    }

    fn attack_is_evaded(
        &self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        completed_tick: u64,
    ) -> bool {
        let TargetIndex::Unit(target_index) = intent.target else {
            return false;
        };
        let target = &units[target_index];
        for effect in target.passive_effects.iter() {
            let PassiveUnitEffect::Evasion(profile) = effect else {
                continue;
            };
            if profile.chance_per_10k == 0 {
                continue;
            }
            let roll = deterministic_random(
                self.config.match_seed,
                completed_tick,
                intent.source_id,
                RANDOM_PURPOSE_ATTACK_PROC
                    ^ u64::from(profile.ability.0)
                    ^ target.id.0.rotate_left(13),
                intent.attack_sequence,
            ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
            if roll < u64::from(profile.chance_per_10k) {
                return true;
            }
        }
        false
    }

    fn resolve_passive_attack_effects(
        &self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        completed_tick: u64,
    ) -> (i32, PendingAttackEffects) {
        let mut bonus_damage = 0i32;
        let mut on_hit = PendingAttackEffects::default();
        for effect in intent.passive_effects.iter() {
            match effect {
                PassiveUnitEffect::Bash(profile) => {
                    let target_matches = match intent.target {
                        TargetIndex::Unit(index) => {
                            profile.targets.can_target_unit(units[index].movement_class)
                        }
                        TargetIndex::Building(_) => profile.targets.can_target_buildings(),
                    };
                    if !target_matches || profile.chance_per_10k == 0 {
                        continue;
                    }
                    let roll = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        intent.source_id,
                        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                        intent.attack_sequence,
                    ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
                    if roll >= u64::from(profile.chance_per_10k) {
                        continue;
                    }
                    bonus_damage = bonus_damage
                        .checked_add(profile.bonus_damage)
                        .expect("passive attack bonus damage overflowed");
                    on_hit.stun_duration_ticks =
                        on_hit.stun_duration_ticks.max(profile.stun_duration_ticks);
                }
                PassiveUnitEffect::TriggeredSpellProc(profile) => {
                    let target_matches = match intent.target {
                        TargetIndex::Unit(index) => {
                            profile.targets.can_target_unit(units[index].movement_class)
                        }
                        TargetIndex::Building(_) => profile.targets.can_target_buildings(),
                    };
                    if !target_matches || profile.chance_per_10k == 0 {
                        continue;
                    }
                    let roll = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        intent.source_id,
                        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                        intent.attack_sequence,
                    ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
                    if roll < u64::from(profile.chance_per_10k) {
                        assert!(
                            on_hit.triggered_spell.is_none(),
                            "multiple triggered spell procs on one attack are not yet supported"
                        );
                        on_hit.triggered_spell = Some(profile.effect);
                    }
                }
                PassiveUnitEffect::BurningOil(profile) => {
                    assert!(
                        on_hit.burning_oil.is_none(),
                        "multiple Burning Oil effects on one attack are not supported"
                    );
                    on_hit.burning_oil = Some(profile);
                }
                PassiveUnitEffect::Evasion(_) | PassiveUnitEffect::Defend(_) => {}
            }
        }
        (bonus_damage, on_hit)
    }

    fn attack_intents(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Vec<AttackIntent> {
        let mut unit_intents: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    if source.spawn_tick == self.next_tick
                        || source.cooldown_remaining != 0
                        || self.next_tick < source.status.stunned_until_tick
                    {
                        return None;
                    }
                    let target_id = source.target?;
                    let (target, distance_sq) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            if !source
                                .attack_targets
                                .can_target_unit(units[index].movement_class)
                            {
                                return None;
                            }
                            (
                                TargetIndex::Unit(index),
                                source.position.distance_sq(units[index].position),
                            )
                        } else {
                            if !source.attack_targets.can_target_buildings() {
                                return None;
                            }
                            let index = find_building_index(buildings, target_id)?;
                            (
                                TargetIndex::Building(index),
                                point_to_footprint_distance_sq(
                                    source.position,
                                    buildings[index].footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        };
                    if distance_sq > source.attack.range_sq() {
                        return None;
                    }
                    Some(AttackIntent {
                        source: AttackSourceIndex::Unit(source_index),
                        target,
                        source_id: source.id,
                        source_team: source.team,
                        source_position: source.position,
                        target_id,
                        attack: source.attack,
                        attack_targets: source.attack_targets,
                        damage_type: source.damage_type,
                        passive_effects: source.passive_effects,
                        attack_sequence: source.attack_sequence,
                        distance_sq,
                    })
                })
                .collect()
        });
        let mut building_intents: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    let attack = source.attack?;
                    if source.spawn_tick == Some(self.next_tick)
                        || source.cooldown_remaining.unwrap_or(0) != 0
                        || source
                            .status
                            .is_some_and(|status| self.next_tick < status.stunned_until_tick)
                    {
                        return None;
                    }
                    let target_id = source.target?;
                    let attack_targets = source
                        .attack_targets
                        .expect("attack building target mask missing");
                    let (target, distance_sq) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            if !attack_targets.can_target_unit(units[index].movement_class) {
                                return None;
                            }
                            (
                                TargetIndex::Unit(index),
                                point_to_footprint_distance_sq(
                                    units[index].position,
                                    source.footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        } else {
                            if !attack_targets.can_target_buildings() {
                                return None;
                            }
                            let index = find_building_index(buildings, target_id)?;
                            (
                                TargetIndex::Building(index),
                                footprint_to_footprint_distance_sq(
                                    source.footprint,
                                    buildings[index].footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        };
                    if distance_sq > attack.range_sq() {
                        return None;
                    }
                    Some(AttackIntent {
                        source: AttackSourceIndex::Building(source_index),
                        target,
                        source_id: source.id,
                        source_team: source.team,
                        source_position: footprint_center_point(
                            source.footprint,
                            self.config.navigation_cell_size,
                        ),
                        target_id,
                        attack,
                        attack_targets: source
                            .attack_targets
                            .expect("attack building target mask missing"),
                        damage_type: source.damage_type,
                        passive_effects: PassiveUnitEffects::EMPTY,
                        attack_sequence: 0,
                        distance_sq,
                    })
                })
                .collect()
        });
        unit_intents.append(&mut building_intents);
        unit_intents
    }

    fn resolve_movement(
        &mut self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        positions: &mut [SimPoint],
        navigation_states: &mut [NavigationState],
    ) -> MovementMetrics {
        let intent_start = Instant::now();
        let mut radius_objective_keys: Vec<_> = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| {
                (unit_health[index] > 0)
                    .then_some(unit.collision_radius_override)
                    .flatten()
                    .map(|radius| (unit.team.0, radius))
            })
            .collect();
        radius_objective_keys.sort_unstable();
        radius_objective_keys.dedup();
        for (team, radius) in radius_objective_keys {
            if self.radius_objective_fields.contains_key(&(team, radius)) {
                continue;
            }
            let field = self
                .topology
                .objective_distance_field_with_radius(team, radius);
            self.radius_objective_fields.insert((team, radius), field);
        }

        let decisions: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(index, unit)| {
                    self.desired_position(
                        index,
                        unit,
                        units,
                        buildings,
                        unit_health,
                        building_health,
                    )
                })
                .collect()
        });
        for decision in &decisions {
            let Some(entry) = decision.cache_insert else {
                continue;
            };
            let key = (
                entry.from,
                entry.target,
                entry.collision_radius,
                entry.route_bias,
            );
            if self.pursuit_cache.len() >= PURSUIT_CACHE_CAPACITY
                && !self.pursuit_cache.contains_key(&key)
            {
                self.pursuit_cache.clear();
            }
            self.pursuit_cache.insert(key, entry.next);
        }
        let intent = intent_start.elapsed();
        let desired_positions: Vec<_> =
            decisions.iter().map(|decision| decision.position).collect();
        let pursuit_steps = decisions
            .iter()
            .filter(|decision| decision.pursuit_step)
            .count();
        let navigation_route_steps = decisions
            .iter()
            .filter(|decision| decision.navigation_route_step)
            .count();
        let movement_intents = decisions
            .iter()
            .zip(units)
            .filter(|(decision, unit)| decision.position != unit.position)
            .count();
        let objective_move_intents = decisions
            .iter()
            .zip(units)
            .filter(|(decision, unit)| {
                navigation_goal(unit, decision) == NavigationGoal::Objective(unit.team)
            })
            .count();
        let a_star_fallbacks = decisions
            .iter()
            .filter(|decision| decision.used_a_star)
            .count();
        let a_star_cache_hits = decisions
            .iter()
            .filter(|decision| decision.a_star_cache_hit)
            .count();
        let a_star_expanded_nodes = decisions
            .iter()
            .map(|decision| decision.a_star_expanded_nodes)
            .sum();

        let separation_start = Instant::now();
        let separated_positions =
            self.apply_crowd_separation(units, unit_health, &decisions, &desired_positions);
        let legal_positions = self.enforce_hard_non_overlap(
            units,
            unit_health,
            navigation_states,
            &decisions,
            &desired_positions,
            &separated_positions,
        );
        let crowd_and_collision = separation_start.elapsed();
        let movement_blocked = decisions
            .iter()
            .zip(units)
            .zip(&legal_positions)
            .filter(|((decision, unit), legal)| {
                decision.position != unit.position && **legal == unit.position
            })
            .count();
        positions.copy_from_slice(&legal_positions);
        MovementMetrics {
            intent,
            crowd_and_collision,
            pursuit_steps,
            navigation_route_steps,
            movement_intents,
            movement_blocked,
            objective_move_intents,
            a_star_fallbacks,
            a_star_cache_hits,
            a_star_expanded_nodes,
        }
    }

    fn desired_position(
        &self,
        index: usize,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
    ) -> MovementDecision {
        let current = unit.position;
        let movement_speed = effective_movement_speed(unit);
        if unit_health[index] <= 0
            || movement_speed == 0
            || self.next_tick < unit.status.stunned_until_tick
        {
            return MovementDecision::stationary(current);
        }
        if unit.movement_class == MovementClass::Air {
            return self.desired_air_position(
                unit,
                units,
                buildings,
                unit_health,
                building_health,
                movement_speed,
            );
        }
        let source_cell = self.topology.cell_of_point(current);
        let mut pursuit_target = None;
        let mut attack_goal = None;

        let target_cell = unit.target.and_then(|target_id| {
            if let Some(target_index) = find_unit_index(units, target_id) {
                if unit_health[target_index] <= 0 {
                    return None;
                }
                let target_position = units[target_index].position;
                if current.distance_sq(target_position) <= unit.attack.range_sq() {
                    attack_goal = Some(current);
                    return Some(source_cell);
                }
                let mut goal =
                    point_attack_envelope_goal(current, target_position, unit.attack.range);
                let mut cell = self.topology.cell_of_point(goal);
                let goal_is_traversable = self.position_is_traversable_from(
                    source_cell,
                    goal,
                    unit.collision_radius_override,
                ) && unit.collision_radius_override.is_none_or(|_| {
                    self.position_is_traversable_from(
                        source_cell,
                        self.topology.center_of_cell(cell),
                        unit.collision_radius_override,
                    )
                });
                if !goal_is_traversable {
                    if unit.collision_radius_override.is_some() {
                        cell = self.nearest_reachable_unit_attack_cell(
                            source_cell,
                            current,
                            target_position,
                            unit.attack.range,
                            unit.collision_radius_override,
                        )?;
                        goal = self.topology.center_of_cell(cell);
                    } else {
                        cell = self.topology.cell_of_point(target_position);
                        if !self.topology.same_component(source_cell, cell) {
                            return None;
                        }
                        goal = target_position;
                    }
                }
                pursuit_target = Some(target_id);
                attack_goal = Some(goal);
                Some(cell)
            } else if let Some(target_index) = find_building_index(buildings, target_id) {
                if building_health[target_index] <= 0 {
                    return None;
                }
                let footprint = buildings[target_index].footprint;
                if point_to_footprint_distance_sq(
                    current,
                    footprint,
                    self.config.navigation_cell_size,
                ) <= unit.attack.range_sq()
                {
                    attack_goal = Some(current);
                    return Some(source_cell);
                }
                let mut goal = building_attack_envelope_goal(
                    current,
                    footprint,
                    unit.attack.range,
                    self.config.navigation_cell_size,
                );
                let mut cell = self.topology.cell_of_point(goal);
                let goal_is_traversable = self.position_is_traversable_from(
                    source_cell,
                    goal,
                    unit.collision_radius_override,
                ) && unit.collision_radius_override.is_none_or(|_| {
                    self.position_is_traversable_from(
                        source_cell,
                        self.topology.center_of_cell(cell),
                        unit.collision_radius_override,
                    )
                });
                if !goal_is_traversable {
                    if unit.collision_radius_override.is_some() {
                        cell = self.nearest_reachable_building_attack_cell(
                            source_cell,
                            current,
                            footprint,
                            unit.attack.range,
                            unit.collision_radius_override,
                        )?;
                        goal = self.topology.center_of_cell(cell);
                    } else {
                        cell = self
                            .topology
                            .nearest_reachable_perimeter_cell(source_cell, footprint)?;
                        goal = building_attack_envelope_goal(
                            self.topology.center_of_cell(cell),
                            footprint,
                            unit.attack.range,
                            self.config.navigation_cell_size,
                        );
                    }
                }
                pursuit_target = Some(target_id);
                attack_goal = Some(goal);
                Some(cell)
            } else {
                None
            }
        });

        if attack_goal == Some(current) {
            return MovementDecision::stationary(current);
        }

        let mut pursuit_step = false;
        let mut targetless_lane_goal = None;
        let route = match target_cell {
            Some(cell) => {
                pursuit_step = pursuit_target.is_some();
                if cell == source_cell {
                    NavigationRoute::at(cell)
                } else {
                    self.route_to_cell(
                        source_cell,
                        cell,
                        unit.collision_radius_override,
                        sidestep_sign(unit.id),
                    )
                }
            }
            None => {
                let route_bias = sidestep_sign(unit.id);
                if let Some(goal) =
                    self.targetless_lane_ingress_goal(unit.team, current, unit.collision_radius)
                {
                    let cell = self.topology.cell_of_point(goal);
                    let lane_route = if cell == source_cell {
                        NavigationRoute::at(cell)
                    } else {
                        self.route_to_cell(
                            source_cell,
                            cell,
                            unit.collision_radius_override,
                            route_bias,
                        )
                    };
                    if lane_route.next_cell.is_some() {
                        targetless_lane_goal = Some((cell, goal));
                        lane_route
                    } else {
                        // A closed cage or other disconnected local component can make the normal
                        // lane entrance unreachable. Preserve the established no-route behavior in
                        // that case: press toward the enemy side within the current component rather
                        // than freezing or inventing a route through blockers.
                        self.targetless_horizontal_route(unit, source_cell, route_bias)
                    }
                } else {
                    self.targetless_horizontal_route(unit, source_cell, route_bias)
                }
            }
        };
        let navigation_route_step = route.navigation_route_step;
        let used_a_star = route.used_a_star;
        let a_star_cache_hit = route.a_star_cache_hit;
        let a_star_expanded_nodes = route.a_star_expanded_nodes;
        let cache_insert = route.cache_insert;
        let next_cell = route.next_cell;
        let Some(next_cell) = next_cell else {
            return MovementDecision {
                position: current,
                pursuit_step,
                pursuit_target,
                attack_goal,
                navigation_route_step,
                used_a_star,
                a_star_cache_hit,
                a_star_expanded_nodes,
                cache_insert,
            };
        };
        let target_position =
            if pursuit_step && next_cell == target_cell.expect("pursuit target cell disappeared") {
                attack_goal.expect("pursuit movement missing attack-envelope goal")
            } else if let Some((goal_cell, goal)) = targetless_lane_goal
                && next_cell == goal_cell
            {
                goal
            } else if target_cell.is_none()
                && targetless_lane_goal.is_none()
                && next_cell.y == source_cell.y
            {
                SimPoint::new(self.topology.center_of_cell(next_cell).x, current.y)
            } else {
                self.topology.center_of_cell(next_cell)
            };
        let candidate = current.step_towards(target_position, movement_speed);
        let position = if self.position_is_traversable_from(
            source_cell,
            candidate,
            unit.collision_radius_override,
        ) {
            candidate
        } else {
            // A unit can be legally positioned off-center inside its nav cell while the straight
            // segment toward the next cell clips an expanded building corner. Repeating the same
            // rejected endpoint forever creates a local corner lock. Move back toward the current
            // cell center first; this gives the radius-aware cell route a legal portal to leave
            // through without adding sticky per-unit path state.
            let recenter_target = self.topology.center_of_cell(source_cell);
            let recenter = current.step_towards(recenter_target, movement_speed);
            if recenter != current
                && self.position_is_traversable_from(
                    source_cell,
                    recenter,
                    unit.collision_radius_override,
                )
            {
                recenter
            } else {
                current
            }
        };
        MovementDecision {
            position,
            pursuit_step,
            pursuit_target,
            attack_goal,
            navigation_route_step,
            used_a_star,
            a_star_cache_hit,
            a_star_expanded_nodes,
            cache_insert,
        }
    }

    fn route_to_cell(
        &self,
        source_cell: NavCell,
        target_cell: NavCell,
        collision_radius: Option<i32>,
        route_bias: i32,
    ) -> NavigationRoute {
        debug_assert_ne!(source_cell, target_cell);
        let route_bias_key = i8::try_from(route_bias).expect("route bias must fit signed byte");
        let cache_key = (source_cell, target_cell, collision_radius, route_bias_key);
        let cached_fallback = self.pursuit_cache.get(&cache_key).copied();
        let result: PursuitStep = if let Some(radius) = collision_radius {
            self.topology.pursuit_step_with_radius(
                source_cell,
                target_cell,
                cached_fallback,
                radius,
                route_bias,
            )
        } else {
            self.topology
                .pursuit_step(source_cell, target_cell, cached_fallback, route_bias)
        };
        let cache_insert = if result.used_a_star && !result.a_star_cache_hit {
            result.next_cell.map(|next| PursuitCacheInsert {
                from: source_cell,
                target: target_cell,
                collision_radius,
                route_bias: route_bias_key,
                next,
            })
        } else {
            None
        };
        NavigationRoute {
            next_cell: result.next_cell,
            navigation_route_step: true,
            used_a_star: result.used_a_star,
            a_star_cache_hit: result.a_star_cache_hit,
            a_star_expanded_nodes: result.a_star_expanded_nodes,
            cache_insert,
        }
    }

    fn targetless_horizontal_route(
        &self,
        unit: &UnitSnapshot,
        source_cell: NavCell,
        route_bias: i32,
    ) -> NavigationRoute {
        let objective_cell = self
            .topology
            .cell_of_point(self.config.team_objective[usize::from(unit.team.0)]);
        if objective_cell.x == source_cell.x {
            return NavigationRoute::none();
        }

        let step_x = (objective_cell.x - source_cell.x).signum();
        let direct_cell = NavCell::new(source_cell.x + step_x, source_cell.y);
        let direct_position = self.topology.center_of_cell(direct_cell);
        if self.position_is_traversable_from(
            source_cell,
            direct_position,
            unit.collision_radius_override,
        ) {
            return NavigationRoute::at(direct_cell);
        }

        let objective_detour_step = if let Some(radius) = unit.collision_radius_override {
            let field = self
                .radius_objective_fields
                .get(&(unit.team.0, radius))
                .expect("radius-aware objective field was not prepared");
            self.topology.step_from_distance_field_with_radius_bias(
                source_cell,
                field,
                radius,
                route_bias,
            )
        } else {
            self.topology
                .objective_step_with_bias(unit.team.0, source_cell, route_bias)
        };
        if let Some(next_cell) = objective_detour_step {
            // While the unit's preferred horizontal step is topologically blocked, follow the
            // stable shared objective field instead of repeatedly A*-routing to a same-row goal
            // that changes as the detour changes rows. Once horizontal progress is clear again,
            // the normal stateless lane rule resumes from the unit's new y coordinate.
            return NavigationRoute {
                next_cell: Some(next_cell),
                navigation_route_step: true,
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
                cache_insert: None,
            };
        }

        let Some(cell) = self.horizontal_objective_goal_cell(
            source_cell,
            objective_cell.x,
            unit.collision_radius_override,
        ) else {
            return NavigationRoute::none();
        };
        // Disconnected/caged components have no objective-field descent. Keep the horizontal
        // best-effort A* fallback so those units still press toward the objective-side wall
        // without inventing a route through blockers.
        if cell == source_cell {
            NavigationRoute::at(cell)
        } else {
            self.route_to_cell(
                source_cell,
                cell,
                unit.collision_radius_override,
                route_bias,
            )
        }
    }

    fn targetless_lane_ingress_goal(
        &self,
        team: Team,
        current: SimPoint,
        collision_radius: i32,
    ) -> Option<SimPoint> {
        let lane = self.config.targetless_lane?;
        let min_center_y = lane.min_y.checked_add(collision_radius)?;
        let max_center_y = lane.max_y.checked_sub(collision_radius)?;
        if min_center_y > max_center_y || (current.y >= min_center_y && current.y <= max_center_y) {
            return None;
        }

        let goal_y = current.y.clamp(min_center_y, max_center_y);
        let inward_distance = (i64::from(current.y) - i64::from(goal_y)).abs();
        let team_index = usize::from(team.0);
        let other_team_index = 1 - team_index;
        let current_x = i64::from(current.x);
        let objective_x = i64::from(self.config.team_objective[team_index].x);
        let other_objective_x = i64::from(self.config.team_objective[other_team_index].x);
        let forward = (objective_x - other_objective_x).signum();
        let projected_x = current_x + forward * inward_distance;
        let goal_x = match forward {
            1 => projected_x.min(objective_x.max(current_x)),
            -1 => projected_x.max(objective_x.min(current_x)),
            _ => current_x,
        };
        Some(SimPoint::new(i32::try_from(goal_x).ok()?, goal_y))
    }

    fn desired_air_position(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        movement_speed: i32,
    ) -> MovementDecision {
        let current = unit.position;
        let mut pursuit_target = None;
        let mut attack_goal = None;
        let goal = unit
            .target
            .and_then(|target_id| {
                if let Some(target_index) = find_unit_index(units, target_id) {
                    if unit_health[target_index] <= 0 {
                        return None;
                    }
                    let target_position = units[target_index].position;
                    if current.distance_sq(target_position) <= unit.attack.range_sq() {
                        attack_goal = Some(current);
                        return Some(current);
                    }
                    pursuit_target = Some(target_id);
                    let goal =
                        point_attack_envelope_goal(current, target_position, unit.attack.range);
                    attack_goal = Some(goal);
                    Some(goal)
                } else if let Some(target_index) = find_building_index(buildings, target_id) {
                    if building_health[target_index] <= 0 {
                        return None;
                    }
                    let footprint = buildings[target_index].footprint;
                    if point_to_footprint_distance_sq(
                        current,
                        footprint,
                        self.config.navigation_cell_size,
                    ) <= unit.attack.range_sq()
                    {
                        attack_goal = Some(current);
                        return Some(current);
                    }
                    pursuit_target = Some(target_id);
                    let goal = building_attack_envelope_goal(
                        current,
                        footprint,
                        unit.attack.range,
                        self.config.navigation_cell_size,
                    );
                    attack_goal = Some(goal);
                    Some(goal)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                self.targetless_lane_ingress_goal(unit.team, current, unit.collision_radius)
                    .unwrap_or_else(|| {
                        SimPoint::new(
                            self.config.team_objective[usize::from(unit.team.0)].x,
                            current.y,
                        )
                    })
            });

        if goal == current {
            return MovementDecision::stationary(current);
        }

        let source_cell = self.air_topology.cell_of_point(current);
        let direct_candidate = current.step_towards(goal, movement_speed);
        if self.air_position_is_traversable_from(
            source_cell,
            direct_candidate,
            unit.collision_radius,
        ) {
            return MovementDecision {
                position: direct_candidate,
                pursuit_step: pursuit_target.is_some(),
                pursuit_target,
                attack_goal,
                navigation_route_step: false,
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
                cache_insert: None,
            };
        }

        let target_cell = self.air_topology.cell_of_point(goal);
        let route = self.air_topology.pursuit_step_with_radius(
            source_cell,
            target_cell,
            None,
            unit.collision_radius,
            sidestep_sign(unit.id),
        );
        let Some(next_cell) = route.next_cell else {
            return MovementDecision {
                position: current,
                pursuit_step: pursuit_target.is_some(),
                pursuit_target,
                attack_goal,
                navigation_route_step: true,
                used_a_star: route.used_a_star,
                a_star_cache_hit: route.a_star_cache_hit,
                a_star_expanded_nodes: route.a_star_expanded_nodes,
                cache_insert: None,
            };
        };
        let route_target = if next_cell == target_cell {
            goal
        } else {
            self.air_topology.center_of_cell(next_cell)
        };
        let candidate = current.step_towards(route_target, movement_speed);
        let position =
            if self.air_position_is_traversable_from(source_cell, candidate, unit.collision_radius)
            {
                candidate
            } else {
                let recenter_target = self.air_topology.center_of_cell(source_cell);
                let recenter = current.step_towards(recenter_target, movement_speed);
                if recenter != current
                    && self.air_position_is_traversable_from(
                        source_cell,
                        recenter,
                        unit.collision_radius,
                    )
                {
                    recenter
                } else {
                    current
                }
            };
        MovementDecision {
            position,
            pursuit_step: pursuit_target.is_some(),
            pursuit_target,
            attack_goal,
            navigation_route_step: true,
            used_a_star: route.used_a_star,
            a_star_cache_hit: route.a_star_cache_hit,
            a_star_expanded_nodes: route.a_star_expanded_nodes,
            cache_insert: None,
        }
    }

    fn horizontal_objective_goal_cell(
        &self,
        source_cell: NavCell,
        objective_x: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let step_x = (objective_x - source_cell.x).signum();
        if step_x == 0 {
            return None;
        }
        let source_component = self.topology.component_id(source_cell)?;
        let mut x = objective_x;
        while x != source_cell.x {
            let cell = NavCell::new(x, source_cell.y);
            let valid = if let Some(radius) = collision_radius {
                self.topology.circle_is_traversable_in_component(
                    self.topology.center_of_cell(cell),
                    radius,
                    source_component,
                )
            } else {
                self.topology.component_id(cell) == Some(source_component)
            };
            if valid {
                return Some(cell);
            }
            x = x.checked_sub(step_x)?;
        }
        None
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
enum BuilderFollowGeometry {
    Building(BuildingFootprint),
    Point { position: SimPoint, stop_range: i32 },
}

#[derive(Debug, Clone, Copy)]
struct BuilderFollowTargetSnapshot {
    geometry: BuilderFollowGeometry,
}

impl BuilderFollowTargetSnapshot {
    fn reached(self, position: SimPoint, navigation_cell_size: i32) -> bool {
        match self.geometry {
            BuilderFollowGeometry::Building(footprint) => {
                point_to_footprint_distance_sq(position, footprint, navigation_cell_size) == 0
            }
            BuilderFollowGeometry::Point {
                position: target,
                stop_range,
            } => position.distance_sq(target) <= square_i32(stop_range),
        }
    }

    fn approach_position(self, source: SimPoint, navigation_cell_size: i32) -> SimPoint {
        match self.geometry {
            BuilderFollowGeometry::Building(footprint) => {
                closest_point_on_footprint(source, footprint, navigation_cell_size)
            }
            BuilderFollowGeometry::Point {
                position: target,
                stop_range,
            } => point_attack_envelope_goal(source, target, stop_range),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BuilderRepairGeometry {
    Building(BuildingFootprint),
    Unit(SimPoint),
}

#[derive(Debug, Clone, Copy)]
struct BuilderRepairTargetSnapshot {
    entity: Entity,
    id: SimId,
    team: Team,
    health: Health,
    geometry: BuilderRepairGeometry,
    repair_time_ticks: Option<u32>,
}

impl BuilderRepairTargetSnapshot {
    fn distance_sq(self, position: SimPoint, navigation_cell_size: i32) -> u64 {
        match self.geometry {
            BuilderRepairGeometry::Building(footprint) => {
                point_to_footprint_distance_sq(position, footprint, navigation_cell_size)
            }
            BuilderRepairGeometry::Unit(target) => position.distance_sq(target),
        }
    }

    fn approach_position(self, navigation_cell_size: i32) -> SimPoint {
        match self.geometry {
            BuilderRepairGeometry::Building(footprint) => {
                footprint_center_point(footprint, navigation_cell_size)
            }
            BuilderRepairGeometry::Unit(position) => position,
        }
    }
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

fn apply_ability_effect_to_unit(
    target: &mut UnitSnapshot,
    effect: AbilityEffect,
    completed_tick: u64,
    damage_rules: DamageRules,
) -> bool {
    if target.health <= 0 {
        return false;
    }
    match effect {
        AbilityEffect::Damage { amount } => {
            let adjusted = damage_rules.apply_spell(amount, target.armor.armor_type);
            let adjusted = spell_damage_after_defend(*target, adjusted, completed_tick);
            target.health = target
                .health
                .checked_sub(adjusted)
                .expect("ability damage overflowed validated bounds");
        }
        AbilityEffect::Stun { duration_ticks } => {
            let stunned_until_tick = completed_tick
                .checked_add(u64::from(duration_ticks))
                .expect("stun expiry tick overflow");
            target.status.stunned_until_tick =
                target.status.stunned_until_tick.max(stunned_until_tick);
        }
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier,
            percent_delta,
            duration_ticks,
        } => {
            let expires_tick = completed_tick
                .checked_add(u64::from(duration_ticks))
                .expect("movement modifier expiry tick overflow");
            apply_timed_movement_modifier(
                &mut target.status,
                modifier,
                percent_delta,
                expires_tick,
            );
        }
        AbilityEffect::AreaDamage { amount, radius: _ } => {
            let adjusted = damage_rules.apply_spell(amount, target.armor.armor_type);
            let adjusted = spell_damage_after_defend(*target, adjusted, completed_tick);
            target.health = target
                .health
                .checked_sub(adjusted)
                .expect("area ability damage overflowed validated bounds");
        }
        AbilityEffect::FrostArmor {
            modifier,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            let expires_tick = completed_tick
                .checked_add(u64::from(armor_duration_ticks))
                .expect("Frost Armor expiry tick overflow");
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    id: modifier,
                    armor_bonus_per_100,
                    expires_tick,
                    reactive_slow_duration_ticks: slow_duration_ticks,
                    reactive_movement_percent_delta: movement_percent_delta,
                    reactive_attack_speed_percent_delta: attack_speed_percent_delta,
                },
            );
        }
    }
    true
}

fn purge_expired_status_modifiers(status: &mut StatusState, tick: u64) {
    purge_expired_movement_modifiers(status, tick);

    let attack_speed_count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(attack_speed_count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let mut attack_speed_write = 0usize;
    for read_index in 0..attack_speed_count {
        let modifier = status.attack_speed_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.attack_speed_modifiers[attack_speed_write] = modifier;
            attack_speed_write += 1;
        }
    }
    for slot in &mut status.attack_speed_modifiers[attack_speed_write..attack_speed_count] {
        *slot = Default::default();
    }
    status.attack_speed_modifier_count =
        u8::try_from(attack_speed_write).expect("attack-speed modifier count exceeds u8");

    let armor_count = usize::from(status.armor_modifier_count);
    debug_assert!(armor_count <= MAX_TIMED_ARMOR_MODIFIERS);
    let mut armor_write = 0usize;
    for read_index in 0..armor_count {
        let modifier = status.armor_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.armor_modifiers[armor_write] = modifier;
            armor_write += 1;
        }
    }
    for slot in &mut status.armor_modifiers[armor_write..armor_count] {
        *slot = Default::default();
    }
    status.armor_modifier_count =
        u8::try_from(armor_write).expect("armor modifier count exceeds u8");

    let dot_count = usize::from(status.damage_over_time_count);
    debug_assert!(dot_count <= MAX_TIMED_DAMAGE_OVER_TIME);
    let mut dot_write = 0usize;
    for read_index in 0..dot_count {
        let effect = status.damage_over_time[read_index];
        if tick < effect.expires_tick {
            status.damage_over_time[dot_write] = effect;
            dot_write += 1;
        }
    }
    for slot in &mut status.damage_over_time[dot_write..dot_count] {
        *slot = Default::default();
    }
    status.damage_over_time_count =
        u8::try_from(dot_write).expect("damage-over-time count exceeds u8");
}

fn purge_expired_movement_modifiers(status: &mut StatusState, tick: u64) {
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let mut write_index = 0usize;
    for read_index in 0..count {
        let modifier = status.movement_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.movement_modifiers[write_index] = modifier;
            write_index += 1;
        }
    }
    for slot in &mut status.movement_modifiers[write_index..count] {
        *slot = Default::default();
    }
    status.movement_modifier_count =
        u8::try_from(write_index).expect("movement modifier count exceeds u8");
}

fn apply_timed_movement_modifier(
    status: &mut StatusState,
    modifier_id: ModifierId,
    percent_delta: i16,
    expires_tick: u64,
) {
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let active = &status.movement_modifiers[..count];
    match active.binary_search_by_key(&modifier_id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.movement_modifiers[index];
            assert_eq!(
                modifier.percent_delta, percent_delta,
                "same ModifierId authored with conflicting movement percentages"
            );
            modifier.expires_tick = modifier.expires_tick.max(expires_tick);
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_MOVEMENT_MODIFIERS,
                "timed movement modifier capacity exceeded"
            );
            status
                .movement_modifiers
                .copy_within(index..count, index + 1);
            status.movement_modifiers[index] = crate::components::TimedMovementModifier {
                id: modifier_id,
                percent_delta,
                expires_tick,
            };
            status.movement_modifier_count = status
                .movement_modifier_count
                .checked_add(1)
                .expect("movement modifier count overflow");
        }
    }
}

fn apply_timed_attack_speed_modifier(
    status: &mut StatusState,
    modifier_id: ModifierId,
    percent_delta: i16,
    expires_tick: u64,
) {
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let active = &status.attack_speed_modifiers[..count];
    match active.binary_search_by_key(&modifier_id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.attack_speed_modifiers[index];
            assert_eq!(
                modifier.percent_delta, percent_delta,
                "same ModifierId authored with conflicting attack-speed percentages"
            );
            modifier.expires_tick = modifier.expires_tick.max(expires_tick);
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_ATTACK_SPEED_MODIFIERS,
                "timed attack-speed modifier capacity exceeded"
            );
            status
                .attack_speed_modifiers
                .copy_within(index..count, index + 1);
            status.attack_speed_modifiers[index] = TimedAttackSpeedModifier {
                id: modifier_id,
                percent_delta,
                expires_tick,
            };
            status.attack_speed_modifier_count = status
                .attack_speed_modifier_count
                .checked_add(1)
                .expect("attack-speed modifier count overflow");
        }
    }
}

fn apply_timed_armor_modifier(status: &mut StatusState, incoming: TimedArmorModifier) {
    let count = usize::from(status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    let active = &status.armor_modifiers[..count];
    match active.binary_search_by_key(&incoming.id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.armor_modifiers[index];
            assert_eq!(
                (
                    modifier.armor_bonus_per_100,
                    modifier.reactive_slow_duration_ticks,
                    modifier.reactive_movement_percent_delta,
                    modifier.reactive_attack_speed_percent_delta,
                ),
                (
                    incoming.armor_bonus_per_100,
                    incoming.reactive_slow_duration_ticks,
                    incoming.reactive_movement_percent_delta,
                    incoming.reactive_attack_speed_percent_delta,
                ),
                "same ModifierId authored with conflicting armor/Frost Armor semantics"
            );
            modifier.expires_tick = modifier.expires_tick.max(incoming.expires_tick);
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_ARMOR_MODIFIERS,
                "timed armor modifier capacity exceeded"
            );
            status.armor_modifiers.copy_within(index..count, index + 1);
            status.armor_modifiers[index] = incoming;
            status.armor_modifier_count = status
                .armor_modifier_count
                .checked_add(1)
                .expect("armor modifier count overflow");
        }
    }
}

fn apply_timed_damage_over_time(
    status: &mut StatusState,
    modifier_id: ModifierId,
    damage_per_pulse: i32,
    pulse_interval_ticks: u16,
    applied_tick: u64,
    expires_tick: u64,
) {
    assert!(damage_per_pulse >= 0);
    assert!(pulse_interval_ticks > 0);
    let count = usize::from(status.damage_over_time_count);
    debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
    let active = &status.damage_over_time[..count];
    let next_pulse_tick = applied_tick
        .checked_add(u64::from(pulse_interval_ticks))
        .expect("damage-over-time pulse tick overflow");
    match active.binary_search_by_key(&modifier_id, |effect| effect.id) {
        Ok(index) => {
            let effect = &mut status.damage_over_time[index];
            assert_eq!(
                (effect.damage_per_pulse, effect.pulse_interval_ticks),
                (damage_per_pulse, pulse_interval_ticks),
                "same ModifierId authored with conflicting damage-over-time parameters"
            );
            effect.expires_tick = effect.expires_tick.max(expires_tick);
            effect.next_pulse_tick = next_pulse_tick;
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_DAMAGE_OVER_TIME,
                "damage-over-time capacity exceeded"
            );
            status.damage_over_time.copy_within(index..count, index + 1);
            status.damage_over_time[index] = TimedDamageOverTime {
                id: modifier_id,
                damage_per_pulse,
                pulse_interval_ticks,
                next_pulse_tick,
                expires_tick,
            };
            status.damage_over_time_count = status
                .damage_over_time_count
                .checked_add(1)
                .expect("damage-over-time count overflow");
        }
    }
}

fn resolve_periodic_unit_statuses(
    units: &mut [UnitSnapshot],
    completed_tick: u64,
    damage_rules: DamageRules,
) {
    for unit in units {
        if unit.health <= 0 {
            continue;
        }
        let count = usize::from(unit.status.damage_over_time_count);
        debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
        let spell_damage_taken_per_10k =
            active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
                .map(|profile| profile.spell_damage_taken_per_10k);
        for index in 0..count {
            let effect = &mut unit.status.damage_over_time[index];
            while completed_tick >= effect.next_pulse_tick
                && effect.next_pulse_tick < effect.expires_tick
                && unit.health > 0
            {
                let adjusted =
                    damage_rules.apply_spell(effect.damage_per_pulse, unit.armor.armor_type);
                let adjusted = spell_damage_taken_per_10k
                    .map_or(adjusted, |factor| scale_damage_per_10k(adjusted, factor));
                unit.health = unit
                    .health
                    .checked_sub(adjusted)
                    .expect("damage-over-time health arithmetic overflow");
                effect.next_pulse_tick = effect
                    .next_pulse_tick
                    .checked_add(u64::from(effect.pulse_interval_ticks))
                    .expect("damage-over-time pulse tick overflow");
            }
        }
    }
}

fn effective_armor_points_per_100(unit: &UnitSnapshot) -> i32 {
    unit.status.effective_armor_points_per_100(unit.armor)
}

fn apply_melee_reactive_armor_effects(
    source_index: usize,
    target_index: usize,
    completed_tick: u64,
    units: &mut [UnitSnapshot],
    unit_health: &[i32],
) {
    if unit_health[source_index] <= 0 {
        return;
    }
    let target_status = units[target_index].status;
    let count = usize::from(target_status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    for armor in target_status.armor_modifiers[..count].iter().copied() {
        if armor.reactive_slow_duration_ticks == 0 || completed_tick >= armor.expires_tick {
            continue;
        }
        let expires_tick = completed_tick
            .checked_add(u64::from(armor.reactive_slow_duration_ticks))
            .expect("reactive Frost Armor slow expiry overflow");
        if armor.reactive_movement_percent_delta != 0 {
            apply_timed_movement_modifier(
                &mut units[source_index].status,
                armor.id,
                armor.reactive_movement_percent_delta,
                expires_tick,
            );
        }
        if armor.reactive_attack_speed_percent_delta != 0 {
            apply_timed_attack_speed_modifier(
                &mut units[source_index].status,
                armor.id,
                armor.reactive_attack_speed_percent_delta,
                expires_tick,
            );
        }
    }
}

fn effective_attack_cooldown_ticks(base_ticks: u16, status: StatusState) -> u16 {
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let percent = status.attack_speed_modifiers[..count]
        .iter()
        .fold(100_i32, |total, modifier| {
            total
                .checked_add(i32::from(modifier.percent_delta))
                .expect("attack-speed percentage overflow")
        })
        .clamp(1, 1_000);
    let scaled = u64::from(base_ticks) * 100;
    u16::try_from(scaled.div_ceil(u64::try_from(percent).expect("positive attack speed")))
        .expect("effective attack cooldown exceeds u16")
        .max(1)
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

fn builder_follow_target_from_entity(
    entity: bevy_ecs::world::EntityRef<'_>,
    default_collision_radius: i32,
) -> Option<BuilderFollowTargetSnapshot> {
    if entity
        .get::<Health>()
        .is_some_and(|health| health.current <= 0)
    {
        return None;
    }
    if let Some(footprint) = entity.get::<BuildingFootprint>().copied() {
        return Some(BuilderFollowTargetSnapshot {
            geometry: BuilderFollowGeometry::Building(footprint),
        });
    }
    let position = entity.get::<Position>()?.0;
    let stop_range = entity
        .get::<CollisionRadius>()
        .map_or(default_collision_radius, |radius| radius.0);
    Some(BuilderFollowTargetSnapshot {
        geometry: BuilderFollowGeometry::Point {
            position,
            stop_range,
        },
    })
}

fn builder_repair_target_from_entity(
    entity: bevy_ecs::world::EntityRef<'_>,
) -> Option<BuilderRepairTargetSnapshot> {
    let id = *entity.get::<SimId>()?;
    let team = *entity.get::<Team>()?;
    let health = *entity.get::<Health>()?;
    if let Some(footprint) = entity.get::<BuildingFootprint>().copied() {
        return Some(BuilderRepairTargetSnapshot {
            entity: entity.id(),
            id,
            team,
            health,
            geometry: BuilderRepairGeometry::Building(footprint),
            repair_time_ticks: entity.get::<RepairTimeTicks>().map(|ticks| ticks.0),
        });
    }
    entity.get::<MechanicalUnit>()?;
    let position = entity.get::<Position>()?.0;
    let repair_time_ticks = entity.get::<RepairTimeTicks>()?.0;
    Some(BuilderRepairTargetSnapshot {
        entity: entity.id(),
        id,
        team,
        health,
        geometry: BuilderRepairGeometry::Unit(position),
        repair_time_ticks: Some(repair_time_ticks),
    })
}

fn builder_repair_duration_ticks(
    profile: BuilderProfile,
    target: BuilderRepairTargetSnapshot,
) -> u64 {
    let Some(repair_time_ticks) = target.repair_time_ticks else {
        return u64::from(profile.full_repair_duration_ticks);
    };
    let scaled = u64::from(repair_time_ticks)
        .checked_mul(u64::from(profile.repair_time_ratio_numerator))
        .expect("mechanical-unit repair duration overflow");
    let denominator = u64::from(profile.repair_time_ratio_denominator);
    scaled.div_ceil(denominator).max(1)
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

fn canonical_configuration_identity(
    config: &SimulationConfig,
    combat_rules: &CombatRules,
    gameplay_bundle: Option<GameplayBundleIdentity>,
) -> u64 {
    const DOMAIN: u64 = 0x4346_434f_4e46_4947;
    const DAMAGE_TYPES: [DamageType; DamageType::COUNT] = [
        DamageType::Normal,
        DamageType::Pierce,
        DamageType::Siege,
        DamageType::Magic,
        DamageType::Chaos,
        DamageType::Spells,
        DamageType::Hero,
    ];
    const ARMOR_TYPES: [ArmorType; ArmorType::COUNT] = [
        ArmorType::Small,
        ArmorType::Medium,
        ArmorType::Large,
        ArmorType::Fortified,
        ArmorType::Normal,
        ArmorType::Hero,
        ArmorType::Divine,
        ArmorType::Unarmored,
    ];

    let mut hash = Fnv64::new();
    hash.write_u64(DOMAIN);
    hash.write_u64(u64::from(CANONICAL_CHECKSUM_SCHEMA_VERSION));
    hash.write_i32(CASTLE_FIGHT_SIMULATION_HZ);
    match gameplay_bundle {
        Some(identity) => {
            hash.write_u8(1);
            hash.write_u32(identity.schema_version);
            hash.write_u64(identity.gameplay_hash);
        }
        None => hash.write_u8(0),
    }
    hash.write_u64(config.match_seed);
    hash.write_i32(config.spatial_cell_size);
    hash.write_i32(config.navigation_cell_size);
    hash.write_i32(config.navigation_min.x);
    hash.write_i32(config.navigation_min.y);
    hash.write_i32(config.navigation_max.x);
    hash.write_i32(config.navigation_max.y);
    hash.write_i32(config.target_pursuit_extra_range);
    hash.write_i32(config.unit_separation_distance);
    hash.write_i32(config.max_separation_per_tick);
    hash_footprint_list(&mut hash, &config.static_blockers);
    hash_footprint_list(&mut hash, &config.air_static_blockers);
    hash_footprint_list(&mut hash, &config.build_static_blockers);
    for regions in &config.team_build_regions {
        hash_footprint_list(&mut hash, regions);
    }
    match config.targetless_lane {
        Some(lane) => {
            hash.write_u8(1);
            hash.write_i32(lane.min_y);
            hash.write_i32(lane.max_y);
        }
        None => hash.write_u8(0),
    }
    for objective in config.team_objective {
        hash.write_i32(objective.x);
        hash.write_i32(objective.y);
    }
    hash.write_u64(u64::from(config.economy.starting_gold));
    hash.write_u64(u64::from(config.economy.starting_lumber));
    hash.write_u16(config.economy.starting_legendary_points);
    hash.write_u64(config.economy.base_income_per_10k);
    hash.write_u64(u64::from(config.economy.income_interval_ticks));
    hash.write_u64(config.economy.income_tax_bracket_per_10k);

    hash.write_u16(combat_rules.uphill_miss_chance_per_10k);
    hash.write_u16(combat_rules.damage_rules.armor_factor_per_10k());
    for damage_type in DAMAGE_TYPES {
        for armor_type in ARMOR_TYPES {
            hash.write_u16(
                combat_rules
                    .damage_rules
                    .bonus_per_10k(damage_type, armor_type),
            );
        }
    }
    match &combat_rules.terrain_elevation {
        Some(terrain) => {
            hash.write_u8(1);
            let origin = terrain.origin();
            hash.write_i32(origin.x);
            hash.write_i32(origin.y);
            hash.write_i32(terrain.tile_size());
            hash.write_u64(u64::from(terrain.width_tiles()));
            hash.write_u64(u64::from(terrain.height_tiles()));
            for y in 0..=terrain.height_tiles() {
                for x in 0..=terrain.width_tiles() {
                    let sample = terrain
                        .vertex_sample(x, y)
                        .expect("validated terrain dimensions must expose every vertex");
                    hash.write_u8(sample.cliff_level);
                    hash.write_i32(sample.ground_height_raw);
                }
            }
        }
        None => hash.write_u8(0),
    }
    hash.finish()
}

fn hash_footprint_list(hash: &mut Fnv64, footprints: &[BuildingFootprint]) {
    let mut canonical = footprints.to_vec();
    canonical.sort_unstable_by_key(|footprint| {
        (
            footprint.min_x,
            footprint.min_y,
            footprint.width,
            footprint.height,
        )
    });
    hash.write_u64(canonical.len() as u64);
    for footprint in canonical {
        hash_building_footprint(hash, footprint);
    }
}

fn hash_building_footprint(hash: &mut Fnv64, footprint: BuildingFootprint) {
    hash.write_i32(footprint.min_x);
    hash.write_i32(footprint.min_y);
    hash.write_u16(footprint.width);
    hash.write_u16(footprint.height);
}

fn canonical_checksum(
    world: &World,
    next_tick: u64,
    next_id: u64,
    configuration_identity: u64,
    defense_alerts: &[DefenseAlert],
    player_resources: &[PlayerResources; 2],
) -> u64 {
    let authoritative_entity_count = world
        .iter_entities()
        .filter(|entity| entity.get::<SimId>().is_some())
        .count();
    let mut entities: Vec<CanonicalEntity> = world
        .iter_entities()
        .filter_map(|entity| {
            let id = *entity.get::<SimId>()?;
            if let Some(projectile) = entity.get::<GuaranteedHitProjectile>() {
                return Some(CanonicalEntity::Projectile(CanonicalProjectile {
                    id,
                    projectile: *projectile,
                }));
            }
            if let Some(projectile) = entity.get::<ReflectedProjectile>() {
                return Some(CanonicalEntity::ReflectedProjectile(
                    CanonicalReflectedProjectile {
                        id,
                        projectile: *projectile,
                    },
                ));
            }
            if let Some(projectile) = entity.get::<BallisticProjectile>() {
                return Some(CanonicalEntity::BallisticProjectile(
                    CanonicalBallisticProjectile {
                        id,
                        projectile: *projectile,
                    },
                ));
            }
            if let Some(projectile) = entity.get::<BounceProjectile>() {
                return Some(CanonicalEntity::BounceProjectile(
                    CanonicalBounceProjectile {
                        id,
                        projectile: *projectile,
                    },
                ));
            }
            if let Some(zone) = entity.get::<BurningOilZone>() {
                return Some(CanonicalEntity::BurningOil(CanonicalBurningOil {
                    id,
                    zone: *zone,
                }));
            }
            if let Some(state) = entity.get::<ChainLightningState>() {
                return Some(CanonicalEntity::ChainLightning(CanonicalChainLightning {
                    id,
                    state: *state,
                }));
            }
            if let Some(corpse) = entity.get::<Corpse>() {
                return Some(CanonicalEntity::Corpse(CanonicalCorpse {
                    id,
                    position: entity.get::<Position>()?.0,
                    corpse: *corpse,
                }));
            }
            if entity.get::<Builder>().is_some() {
                return Some(CanonicalEntity::Builder(CanonicalBuilder {
                    id,
                    team: *entity.get::<Team>()?,
                    position: entity.get::<Position>()?.0,
                    profile: *entity.get::<BuilderProfile>()?,
                    configuration: entity.get::<BuilderConfiguration>()?.clone(),
                    state: *entity.get::<BuilderState>()?,
                    build_order: entity.get::<BuilderBuildOrder>().copied(),
                }));
            }
            let team = *entity.get::<Team>()?;
            let health = *entity.get::<Health>()?;
            if let Some(position) = entity.get::<Position>() {
                Some(CanonicalEntity::Unit(CanonicalUnit {
                    id,
                    content: entity.get::<ContentIdentity>().copied(),
                    team,
                    position: position.0,
                    health,
                    health_regeneration: *entity.get::<HealthRegeneration>()?,
                    attack: *entity.get::<AttackProfile>()?,
                    attack_targets: *entity.get::<AttackTargetMask>()?,
                    damage_type: *entity.get::<DamageType>()?,
                    armor: *entity.get::<ArmorProfile>()?,
                    passive_effects: *entity.get::<PassiveUnitEffects>()?,
                    movement_class: *entity.get::<MovementClass>()?,
                    mechanical: entity.get::<MechanicalUnit>().is_some(),
                    build_time_ticks: entity.get::<BuildTimeTicks>().map(|ticks| ticks.0),
                    repair_time_ticks: entity.get::<RepairTimeTicks>().map(|ticks| ticks.0),
                    movement: *entity.get::<MovementProfile>()?,
                    cooldown: *entity.get::<AttackCooldown>()?,
                    attack_sequence: *entity.get::<AttackSequence>()?,
                    target: *entity.get::<TargetState>()?,
                    retaliation: *entity.get::<RetaliationState>()?,
                    status: *entity.get::<StatusState>()?,
                    navigation: *entity.get::<NavigationState>()?,
                    spawn_tick: *entity.get::<SpawnTick>()?,
                    corpse: entity.get::<CorpseProducer>().map(|corpse| corpse.0),
                    collision_radius: entity.get::<CollisionRadius>().copied(),
                    spellcasting: entity.get::<SpellcastingProfile>().copied(),
                    mana: entity.get::<ManaState>().copied(),
                    ability_state: entity.get::<AutomaticAbilityState>().copied(),
                }))
            } else {
                Some(CanonicalEntity::Building(CanonicalBuilding {
                    id,
                    content: entity.get::<ContentIdentity>().copied(),
                    team,
                    footprint: *entity.get::<BuildingFootprint>()?,
                    health,
                    construction: entity.get::<BuildingConstruction>().copied().map(
                        |construction| {
                            let mut definition_hash = Fnv64::new();
                            hash_building_definition(
                                &mut definition_hash,
                                construction.building,
                                construction.properties,
                            );
                            if let Some(source) = construction.upgrade_from {
                                definition_hash.write_u8(1);
                                hash_building_definition(
                                    &mut definition_hash,
                                    source.building,
                                    source.properties,
                                );
                                definition_hash.write_i32(source.health.current);
                                definition_hash.write_i32(source.health.max);
                                hash_building_runtime_state(&mut definition_hash, source.runtime);
                            } else {
                                definition_hash.write_u8(0);
                            }
                            CanonicalBuildingConstruction {
                                started_tick: construction.started_tick,
                                complete_tick: construction.complete_tick,
                                definition_hash: definition_hash.finish(),
                            }
                        },
                    ),
                    economy: entity.get::<BuildingEconomyProfile>().copied(),
                    repair_time_ticks: entity.get::<RepairTimeTicks>().map(|ticks| ticks.0),
                    production: entity.get::<ProductionProfile>().copied(),
                    production_state: entity.get::<ProductionState>().copied(),
                    production_content: entity
                        .get::<ProductionContentIdentity>()
                        .map(|content| content.0),
                    production_corpse: entity
                        .get::<ProductionCorpseProfile>()
                        .map(|corpse| corpse.0),
                    production_collision_radius: entity
                        .get::<ProductionCollisionRadius>()
                        .map(|radius| radius.0),
                    production_movement_class: entity
                        .get::<ProductionMovementClass>()
                        .map(|class| class.0),
                    production_repair_metadata: entity
                        .get::<ProductionUnitRepairMetadata>()
                        .copied(),
                    production_attack_targets: entity
                        .get::<ProductionAttackTargets>()
                        .map(|targets| targets.0),
                    production_health_regen_per_second_per_10k: entity
                        .get::<ProductionHealthRegeneration>()
                        .map(|regeneration| regeneration.0),
                    production_damage_type: entity
                        .get::<ProductionDamageType>()
                        .map(|damage_type| damage_type.0),
                    production_armor: entity.get::<ProductionArmorProfile>().map(|armor| armor.0),
                    production_passive_effects: entity
                        .get::<ProductionPassiveEffects>()
                        .map(|effects| effects.0),
                    production_spellcasting: entity
                        .get::<ProductionSpellcastingProfile>()
                        .map(|profile| profile.0),
                    attack: entity.get::<AttackProfile>().copied(),
                    attack_targets: entity.get::<AttackTargetMask>().copied(),
                    damage_type: *entity.get::<DamageType>()?,
                    armor: *entity.get::<ArmorProfile>()?,
                    cooldown: entity.get::<AttackCooldown>().copied(),
                    target: entity.get::<TargetState>().copied(),
                    spawn_tick: entity.get::<SpawnTick>().copied(),
                    spellcasting: entity.get::<SpellcastingProfile>().copied(),
                    mana: entity.get::<ManaState>().copied(),
                    ability_state: entity.get::<AutomaticAbilityState>().copied(),
                    status: entity.get::<StatusState>().copied(),
                }))
            }
        })
        .collect();
    assert_eq!(
        entities.len(),
        authoritative_entity_count,
        "canonical checksum omitted an entity with an invalid authoritative component shape"
    );
    entities.sort_unstable_by_key(CanonicalEntity::id);
    for pair in entities.windows(2) {
        assert_ne!(
            pair[0].id(),
            pair[1].id(),
            "canonical state contains duplicate SimIds"
        );
    }

    let mut hash = Fnv64::new();
    hash.write_u64(0x4346_5354_4154_4503);
    hash.write_u64(u64::from(CANONICAL_CHECKSUM_SCHEMA_VERSION));
    hash.write_u64(configuration_identity);
    hash.write_u64(next_tick);
    hash.write_u64(next_id);
    for resources in player_resources {
        hash.write_u64(u64::from(resources.gold));
        hash.write_u64(u64::from(resources.lumber));
        hash.write_u16(resources.legendary_points_used);
        hash.write_u16(resources.legendary_points_cap);
    }
    hash.write_u64(entities.len() as u64);
    for entity in entities {
        match entity {
            CanonicalEntity::Unit(unit) => {
                hash.write_u8(0);
                hash.write_u64(unit.id.0);
                hash_content_identity(&mut hash, unit.content);
                hash.write_u8(unit.team.0);
                hash.write_i32(unit.position.x);
                hash.write_i32(unit.position.y);
                hash.write_i32(unit.health.current);
                hash.write_i32(unit.health.max);
                hash.write_u32(unit.health_regeneration.per_second_per_10k);
                hash.write_u32(unit.health_regeneration.remainder_per_10k_hz);
                hash_attack_delivery(&mut hash, unit.attack.delivery);
                hash.write_u8(unit.attack_targets.bits());
                hash.write_u8(unit.damage_type.stable_tag());
                hash.write_u8(unit.armor.armor_type.stable_tag());
                hash.write_i32(i32::from(unit.armor.armor_points));
                hash_passive_unit_effects(&mut hash, unit.passive_effects);
                hash.write_u8(match unit.movement_class {
                    MovementClass::Ground => 0,
                    MovementClass::Air => 1,
                });
                hash.write_u8(u8::from(unit.mechanical));
                hash_optional_u32(&mut hash, unit.build_time_ticks);
                hash_optional_u32(&mut hash, unit.repair_time_ticks);
                hash.write_i32(unit.attack.damage);
                hash.write_i32(unit.attack.range);
                hash.write_i32(unit.attack.acquisition_range);
                hash.write_u16(unit.attack.cooldown_ticks);
                hash.write_i32(unit.movement.speed_per_tick);
                hash.write_u16(unit.cooldown.remaining);
                hash.write_u64(unit.attack_sequence.0);
                hash_optional_sim_id(&mut hash, unit.target.current);
                hash.write_u8(u8::from(unit.target.direct_retaliation_lock));
                hash.write_u8(u8::from(unit.target.ally_defense_lock));
                hash_optional_sim_id(&mut hash, unit.retaliation.attacker);
                hash_optional_u64(&mut hash, unit.retaliation.attacked_tick);
                hash_status_state(&mut hash, unit.status);
                match unit.navigation.avoidance_goal {
                    NavigationGoal::None => hash.write_u8(0),
                    NavigationGoal::Objective(team) => {
                        hash.write_u8(1);
                        hash.write_u8(team.0);
                    }
                    NavigationGoal::Target(target) => {
                        hash.write_u8(2);
                        hash.write_u64(target.0);
                    }
                }
                hash.write_i32(i32::from(unit.navigation.bypass_side));
                hash.write_u8(unit.navigation.clear_ticks);
                hash.write_u64(unit.spawn_tick.0);
                match unit.corpse {
                    Some(corpse) => {
                        hash.write_u8(1);
                        hash.write_u64(u64::from(corpse.definition.0));
                        hash_optional_u32(&mut hash, corpse.lifetime_ticks);
                    }
                    None => hash.write_u8(0),
                }
                match unit.collision_radius {
                    Some(collision_radius) => {
                        hash.write_u8(1);
                        hash.write_i32(collision_radius.0);
                    }
                    None => hash.write_u8(0),
                }
                if let Some(spellcasting) = unit.spellcasting {
                    hash.write_u8(1);
                    hash.write_i32(spellcasting.mana.maximum);
                    hash.write_i32(spellcasting.mana.starting);
                    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
                    hash_automatic_ability(&mut hash, spellcasting.ability);
                    let mana = unit.mana.expect("spellcasting unit missing mana state");
                    hash.write_i32(mana.current);
                    hash.write_u16(mana.regen_remainder_per_10k);
                    let state = unit
                        .ability_state
                        .expect("spellcasting unit missing ability state");
                    hash.write_u64(state.ready_tick);
                    hash.write_u64(state.cast_sequence);
                } else {
                    hash.write_u8(0);
                }
            }
            CanonicalEntity::Building(building) => {
                hash.write_u8(1);
                hash.write_u64(building.id.0);
                hash_content_identity(&mut hash, building.content);
                hash.write_u8(building.team.0);
                hash.write_i32(building.footprint.min_x);
                hash.write_i32(building.footprint.min_y);
                hash.write_u16(building.footprint.width);
                hash.write_u16(building.footprint.height);
                hash.write_i32(building.health.current);
                hash.write_i32(building.health.max);
                if let Some(construction) = building.construction {
                    hash.write_u8(1);
                    hash.write_u64(construction.started_tick);
                    hash.write_u64(construction.complete_tick);
                    hash.write_u64(construction.definition_hash);
                } else {
                    hash.write_u8(0);
                }
                if let Some(economy) = building.economy {
                    hash.write_u8(1);
                    hash.write_u64(u64::from(economy.gold_cost));
                    hash.write_u64(u64::from(economy.lumber_cost));
                    hash.write_u64(u64::from(economy.lumber_refund));
                    hash.write_u64(economy.income_per_10k);
                } else {
                    hash.write_u8(0);
                }
                hash_optional_u32(&mut hash, building.repair_time_ticks);
                hash.write_u8(building.damage_type.stable_tag());
                hash.write_u8(building.armor.armor_type.stable_tag());
                hash.write_i32(i32::from(building.armor.armor_points));
                if let Some(profile) = building.production {
                    hash.write_u8(1);
                    hash.write_u16(profile.initial_delay_ticks);
                    hash.write_u16(profile.interval_ticks);
                    hash.write_u16(profile.search_radius_cells);
                    hash.write_i32(profile.unit.health);
                    hash_attack_delivery(&mut hash, profile.unit.attack.delivery);
                    hash.write_i32(profile.unit.attack.damage);
                    hash.write_i32(profile.unit.attack.range);
                    hash.write_i32(profile.unit.attack.acquisition_range);
                    hash.write_u16(profile.unit.attack.cooldown_ticks);
                    hash.write_i32(profile.unit.movement.speed_per_tick);
                    hash.write_u64(
                        building
                            .production_state
                            .expect("production profile missing state")
                            .next_spawn_tick,
                    );
                    hash_content_identity(&mut hash, building.production_content);
                    match building.production_corpse {
                        Some(corpse) => {
                            hash.write_u8(1);
                            hash.write_u64(u64::from(corpse.definition.0));
                            hash_optional_u32(&mut hash, corpse.lifetime_ticks);
                        }
                        None => hash.write_u8(0),
                    }
                    match building.production_collision_radius {
                        Some(collision_radius) => {
                            hash.write_u8(1);
                            hash.write_i32(collision_radius.0);
                        }
                        None => hash.write_u8(0),
                    }
                    hash.write_u8(
                        match building
                            .production_movement_class
                            .expect("production building missing movement class")
                        {
                            MovementClass::Ground => 0,
                            MovementClass::Air => 1,
                        },
                    );
                    let repair_metadata = building
                        .production_repair_metadata
                        .expect("production building missing repair metadata");
                    hash.write_u8(u8::from(repair_metadata.mechanical));
                    hash_optional_u32(&mut hash, repair_metadata.build_time_ticks);
                    hash_optional_u32(&mut hash, repair_metadata.repair_time_ticks);
                    hash.write_u8(
                        building
                            .production_attack_targets
                            .expect("production building missing attack target mask")
                            .bits(),
                    );
                    hash.write_u32(
                        building
                            .production_health_regen_per_second_per_10k
                            .expect("production building missing unit health regeneration"),
                    );
                    let production_damage_type = building
                        .production_damage_type
                        .expect("production building missing unit damage type");
                    let production_armor = building
                        .production_armor
                        .expect("production building missing unit armor profile");
                    hash.write_u8(production_damage_type.stable_tag());
                    hash.write_u8(production_armor.armor_type.stable_tag());
                    hash.write_i32(i32::from(production_armor.armor_points));
                    hash_passive_unit_effects(
                        &mut hash,
                        building
                            .production_passive_effects
                            .expect("production building missing unit passive effects"),
                    );
                    if let Some(spellcasting) = building.production_spellcasting {
                        hash.write_u8(1);
                        hash_spellcasting_profile(&mut hash, spellcasting);
                    } else {
                        hash.write_u8(0);
                    }
                } else {
                    hash.write_u8(0);
                }
                if let Some(attack) = building.attack {
                    hash.write_u8(1);
                    hash_attack_delivery(&mut hash, attack.delivery);
                    hash.write_u8(
                        building
                            .attack_targets
                            .expect("attack building missing target mask")
                            .bits(),
                    );
                    hash.write_i32(attack.damage);
                    hash.write_i32(attack.range);
                    hash.write_i32(attack.acquisition_range);
                    hash.write_u16(attack.cooldown_ticks);
                    hash.write_u16(
                        building
                            .cooldown
                            .expect("attack building missing cooldown")
                            .remaining,
                    );
                    let target = building
                        .target
                        .expect("attack building missing target state");
                    hash_optional_sim_id(&mut hash, target.current);
                    hash.write_u8(u8::from(target.direct_retaliation_lock));
                    hash.write_u8(u8::from(target.ally_defense_lock));
                    hash.write_u64(
                        building
                            .spawn_tick
                            .expect("attack building missing spawn tick")
                            .0,
                    );
                } else {
                    hash.write_u8(0);
                }
                if let Some(status) = building.status {
                    hash.write_u8(1);
                    hash_status_state(&mut hash, status);
                } else {
                    hash.write_u8(0);
                }
                if let Some(spellcasting) = building.spellcasting {
                    hash.write_u8(1);
                    hash.write_i32(spellcasting.mana.maximum);
                    hash.write_i32(spellcasting.mana.starting);
                    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
                    hash_automatic_ability(&mut hash, spellcasting.ability);
                    let mana = building
                        .mana
                        .expect("spellcasting building missing mana state");
                    hash.write_i32(mana.current);
                    hash.write_u16(mana.regen_remainder_per_10k);
                    let state = building
                        .ability_state
                        .expect("spellcasting building missing ability state");
                    hash.write_u64(state.ready_tick);
                    hash.write_u64(state.cast_sequence);
                } else {
                    hash.write_u8(0);
                }
            }
            CanonicalEntity::Projectile(projectile) => {
                hash.write_u8(2);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(u8::from(projectile.projectile.source_is_building));
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash_pending_attack_effects(&mut hash, projectile.projectile.on_hit);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.speed_per_tick);
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::ReflectedProjectile(projectile) => {
                hash.write_u8(3);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.original_source.0);
                hash.write_u64(projectile.projectile.reflector.0);
                hash.write_u8(projectile.projectile.reflector_team.0);
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::BallisticProjectile(projectile) => {
                hash.write_u8(4);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(projectile.projectile.target_mask.bits());
                hash.write_i32(projectile.projectile.damage);
                match projectile.projectile.burning_oil {
                    Some(profile) => {
                        hash.write_u8(1);
                        hash_burning_oil_profile(&mut hash, profile);
                    }
                    None => hash.write_u8(0),
                }
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_i32(projectile.projectile.destination.x);
                hash.write_i32(projectile.projectile.destination.y);
                hash.write_i32(projectile.projectile.impact_radius);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::BounceProjectile(projectile) => {
                hash.write_u8(5);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(u8::from(projectile.projectile.source_is_building));
                hash.write_u8(projectile.projectile.target_mask.bits());
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
                hash.write_i32(projectile.projectile.speed_per_tick);
                hash.write_i32(projectile.projectile.bounce_range);
                hash.write_u8(projectile.projectile.remaining_bounces);
                hash.write_u8(projectile.projectile.bounce_index);
                hash.write_u16(projectile.projectile.damage_percent_per_bounce);
                hash.write_u8(u8::from(projectile.projectile.allow_repeat_targets));
                hash.write_u8(projectile.projectile.hit_count);
                for target in projectile.projectile.hit_targets {
                    hash.write_u64(target.0);
                }
            }
            CanonicalEntity::Corpse(corpse) => {
                hash.write_u8(6);
                hash.write_u64(corpse.id.0);
                hash.write_i32(corpse.position.x);
                hash.write_i32(corpse.position.y);
                hash.write_u64(corpse.corpse.source_unit.0);
                hash.write_u8(corpse.corpse.source_team.0);
                hash.write_u64(u64::from(corpse.corpse.definition.0));
                hash.write_u64(corpse.corpse.created_tick);
                hash_optional_u64(&mut hash, corpse.corpse.expires_tick);
            }
            CanonicalEntity::BurningOil(zone) => {
                hash.write_u8(7);
                hash.write_u64(zone.id.0);
                hash.write_u64(zone.zone.source.0);
                hash.write_u8(zone.zone.source_team.0);
                hash.write_i32(zone.zone.center.x);
                hash.write_i32(zone.zone.center.y);
                hash_burning_oil_profile(&mut hash, zone.zone.profile);
                hash.write_u64(zone.zone.created_tick);
                hash.write_u16(zone.zone.pulse_index);
            }
            CanonicalEntity::ChainLightning(chain) => {
                hash.write_u8(8);
                hash.write_u64(chain.id.0);
                hash.write_u64(chain.state.source.0);
                hash.write_u8(chain.state.source_team.0);
                hash.write_u64(u64::from(chain.state.profile.ability.0));
                hash.write_i32(chain.state.profile.initial_damage);
                hash.write_u8(chain.state.profile.maximum_targets);
                hash.write_i32(chain.state.profile.jump_radius);
                hash.write_u16(chain.state.profile.damage_reduction_per_10k);
                hash.write_u8(chain.state.profile.targets.bits());
                hash.write_u64(chain.state.started_tick);
                hash.write_u8(chain.state.next_jump_index);
                hash.write_u64(chain.state.current_target.0);
                hash.write_i32(chain.state.last_position.x);
                hash.write_i32(chain.state.last_position.y);
                hash.write_i32(chain.state.next_damage);
                hash.write_u8(chain.state.hit_count);
                for target in chain.state.hit_targets {
                    hash.write_u64(target.0);
                }
            }
            CanonicalEntity::Builder(builder) => {
                hash.write_u8(9);
                hash.write_u64(builder.id.0);
                hash.write_u8(builder.team.0);
                hash.write_i32(builder.position.x);
                hash.write_i32(builder.position.y);
                hash.write_i32(builder.profile.speed_per_tick);
                hash.write_i32(builder.profile.build_range);
                hash.write_i32(builder.profile.repair_range);
                hash.write_i32(builder.profile.repair_autocast_range);
                hash.write_u16(builder.profile.repair_time_ratio_numerator);
                hash.write_u16(builder.profile.repair_time_ratio_denominator);
                hash.write_u16(builder.profile.full_repair_duration_ticks);
                hash.write_i32(builder.profile.blink_range);
                hash.write_i32(builder.profile.blink_boundary_inset);
                hash.write_u64(u64::from(builder.configuration.appearance.rawcode));
                hash.write_u8(match builder.configuration.locomotion {
                    BuilderLocomotion::Foot => 0,
                    BuilderLocomotion::Hover => 1,
                });
                hash.write_u64(builder.configuration.build_catalog.len() as u64);
                for rawcode in builder.configuration.build_catalog {
                    hash.write_u64(u64::from(rawcode));
                }
                match builder.state.destination {
                    Some(destination) => {
                        hash.write_u8(1);
                        hash.write_i32(destination.x);
                        hash.write_i32(destination.y);
                    }
                    None => hash.write_u8(0),
                }
                hash_optional_sim_id(&mut hash, builder.state.follow_target);
                hash_optional_sim_id(&mut hash, builder.state.repair_target);
                hash.write_u64(u64::from(builder.state.repair_progress_remainder));
                hash.write_u8(u8::from(builder.state.repair_autocast_enabled));
                if let Some(order) = builder.build_order {
                    hash.write_u8(1);
                    hash_building_definition(&mut hash, order.building, order.properties);
                } else {
                    hash.write_u8(0);
                }
            }
        }
    }

    let mut alerts = defense_alerts.to_vec();
    alerts.sort_unstable_by_key(|alert| {
        (
            alert.attacked_tick,
            alert.victim_team.0,
            alert.victim_id,
            alert.attacker_id,
            alert.victim_position.x,
            alert.victim_position.y,
        )
    });
    hash.write_u64(alerts.len() as u64);
    for alert in alerts {
        hash.write_u64(alert.attacked_tick);
        hash.write_u8(alert.victim_team.0);
        hash.write_u64(alert.victim_id.0);
        hash.write_i32(alert.victim_position.x);
        hash.write_i32(alert.victim_position.y);
        hash.write_u64(alert.attacker_id.0);
    }
    hash.finish()
}

#[derive(Debug, Clone)]
enum CanonicalEntity {
    Unit(CanonicalUnit),
    Building(CanonicalBuilding),
    Projectile(CanonicalProjectile),
    ReflectedProjectile(CanonicalReflectedProjectile),
    BallisticProjectile(CanonicalBallisticProjectile),
    BounceProjectile(CanonicalBounceProjectile),
    Corpse(CanonicalCorpse),
    BurningOil(CanonicalBurningOil),
    ChainLightning(CanonicalChainLightning),
    Builder(CanonicalBuilder),
}

impl CanonicalEntity {
    const fn id(&self) -> SimId {
        match self {
            Self::Unit(unit) => unit.id,
            Self::Building(building) => building.id,
            Self::Projectile(projectile) => projectile.id,
            Self::ReflectedProjectile(projectile) => projectile.id,
            Self::BallisticProjectile(projectile) => projectile.id,
            Self::BounceProjectile(projectile) => projectile.id,
            Self::Corpse(corpse) => corpse.id,
            Self::BurningOil(zone) => zone.id,
            Self::ChainLightning(chain) => chain.id,
            Self::Builder(builder) => builder.id,
        }
    }
}

#[derive(Debug, Clone)]
struct CanonicalBuilder {
    id: SimId,
    team: Team,
    position: SimPoint,
    profile: BuilderProfile,
    configuration: BuilderConfiguration,
    state: BuilderState,
    build_order: Option<BuilderBuildOrder>,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalUnit {
    id: SimId,
    content: Option<ContentIdentity>,
    team: Team,
    position: SimPoint,
    health: Health,
    health_regeneration: HealthRegeneration,
    attack: AttackProfile,
    attack_targets: AttackTargetMask,
    damage_type: DamageType,
    armor: ArmorProfile,
    passive_effects: PassiveUnitEffects,
    movement_class: MovementClass,
    mechanical: bool,
    build_time_ticks: Option<u32>,
    repair_time_ticks: Option<u32>,
    movement: MovementProfile,
    cooldown: AttackCooldown,
    attack_sequence: AttackSequence,
    target: TargetState,
    retaliation: RetaliationState,
    status: StatusState,
    navigation: NavigationState,
    spawn_tick: SpawnTick,
    corpse: Option<CorpseProfile>,
    collision_radius: Option<CollisionRadius>,
    spellcasting: Option<SpellcastingProfile>,
    mana: Option<ManaState>,
    ability_state: Option<AutomaticAbilityState>,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBuildingConstruction {
    started_tick: u64,
    complete_tick: u64,
    definition_hash: u64,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBuilding {
    id: SimId,
    content: Option<ContentIdentity>,
    team: Team,
    footprint: BuildingFootprint,
    health: Health,
    construction: Option<CanonicalBuildingConstruction>,
    economy: Option<BuildingEconomyProfile>,
    repair_time_ticks: Option<u32>,
    production: Option<ProductionProfile>,
    production_state: Option<ProductionState>,
    production_content: Option<ContentIdentity>,
    production_corpse: Option<CorpseProfile>,
    production_collision_radius: Option<CollisionRadius>,
    production_movement_class: Option<MovementClass>,
    production_repair_metadata: Option<ProductionUnitRepairMetadata>,
    production_attack_targets: Option<AttackTargetMask>,
    production_health_regen_per_second_per_10k: Option<u32>,
    production_damage_type: Option<DamageType>,
    production_armor: Option<ArmorProfile>,
    production_passive_effects: Option<PassiveUnitEffects>,
    production_spellcasting: Option<SpellcastingProfile>,
    attack: Option<AttackProfile>,
    attack_targets: Option<AttackTargetMask>,
    damage_type: DamageType,
    armor: ArmorProfile,
    cooldown: Option<AttackCooldown>,
    target: Option<TargetState>,
    spawn_tick: Option<SpawnTick>,
    spellcasting: Option<SpellcastingProfile>,
    mana: Option<ManaState>,
    ability_state: Option<AutomaticAbilityState>,
    status: Option<StatusState>,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalProjectile {
    id: SimId,
    projectile: GuaranteedHitProjectile,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalReflectedProjectile {
    id: SimId,
    projectile: ReflectedProjectile,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBallisticProjectile {
    id: SimId,
    projectile: BallisticProjectile,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBounceProjectile {
    id: SimId,
    projectile: BounceProjectile,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalCorpse {
    id: SimId,
    position: SimPoint,
    corpse: Corpse,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBurningOil {
    id: SimId,
    zone: BurningOilZone,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalChainLightning {
    id: SimId,
    state: ChainLightningState,
}

fn hash_content_identity(hash: &mut Fnv64, content: Option<ContentIdentity>) {
    match content {
        Some(content) => {
            hash.write_u8(1);
            hash.write_u64(u64::from(content.rawcode));
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_sim_id(hash: &mut Fnv64, value: Option<SimId>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(value.0);
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_u32(hash: &mut Fnv64, value: Option<u32>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(u64::from(value));
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_u64(hash: &mut Fnv64, value: Option<u64>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(value);
        }
        None => hash.write_u8(0),
    }
}

fn hash_building_runtime_state(hash: &mut Fnv64, runtime: BuildingRuntimeState) {
    match runtime.production {
        Some(production) => {
            hash.write_u8(1);
            hash.write_u64(production.next_spawn_tick);
        }
        None => hash.write_u8(0),
    }
    match runtime.attack_cooldown {
        Some(cooldown) => {
            hash.write_u8(1);
            hash.write_u16(cooldown.remaining);
        }
        None => hash.write_u8(0),
    }
    match runtime.target {
        Some(target) => {
            hash.write_u8(1);
            hash_optional_sim_id(hash, target.current);
            hash.write_u8(u8::from(target.direct_retaliation_lock));
            hash.write_u8(u8::from(target.ally_defense_lock));
        }
        None => hash.write_u8(0),
    }
    match runtime.spawn_tick {
        Some(spawn_tick) => {
            hash.write_u8(1);
            hash.write_u64(spawn_tick.0);
        }
        None => hash.write_u8(0),
    }
    match runtime.mana {
        Some(mana) => {
            hash.write_u8(1);
            hash.write_i32(mana.current);
            hash.write_u16(mana.regen_remainder_per_10k);
        }
        None => hash.write_u8(0),
    }
    match runtime.ability_state {
        Some(state) => {
            hash.write_u8(1);
            hash.write_u64(state.ready_tick);
            hash.write_u64(state.cast_sequence);
        }
        None => hash.write_u8(0),
    }
    match runtime.status {
        Some(status) => {
            hash.write_u8(1);
            hash_status_state(hash, status);
        }
        None => hash.write_u8(0),
    }
}

fn hash_status_state(hash: &mut Fnv64, status: StatusState) {
    hash.write_u64(status.stunned_until_tick);
    hash.write_u8(status.movement_modifier_count);
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    for modifier in &status.movement_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.percent_delta));
        hash.write_u64(modifier.expires_tick);
    }
    hash.write_u8(status.attack_speed_modifier_count);
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    for modifier in &status.attack_speed_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.percent_delta));
        hash.write_u64(modifier.expires_tick);
    }
    hash.write_u8(status.armor_modifier_count);
    let count = usize::from(status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    for modifier in &status.armor_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.armor_bonus_per_100));
        hash.write_u64(modifier.expires_tick);
        hash.write_u16(modifier.reactive_slow_duration_ticks);
        hash.write_i32(i32::from(modifier.reactive_movement_percent_delta));
        hash.write_i32(i32::from(modifier.reactive_attack_speed_percent_delta));
    }
    hash.write_u8(status.damage_over_time_count);
    let count = usize::from(status.damage_over_time_count);
    debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
    for effect in &status.damage_over_time[..count] {
        hash.write_u64(u64::from(effect.id.0));
        hash.write_i32(effect.damage_per_pulse);
        hash.write_u16(effect.pulse_interval_ticks);
        hash.write_u64(effect.next_pulse_tick);
        hash.write_u64(effect.expires_tick);
    }
}

fn hash_building_definition(
    hash: &mut Fnv64,
    building: BuildingSpawn,
    properties: BuildingGameplayProperties,
) {
    hash.write_u8(building.team.0);
    hash.write_i32(building.footprint.min_x);
    hash.write_i32(building.footprint.min_y);
    hash.write_u16(building.footprint.width);
    hash.write_u16(building.footprint.height);
    hash.write_i32(building.health);
    hash_content_identity(hash, properties.content);
    hash_optional_u32(hash, properties.construction_time_ticks);
    hash_optional_u32(hash, properties.repair_time_ticks);
    hash.write_u8(properties.attack_targets.bits());
    hash.write_u8(properties.damage_type.stable_tag());
    hash.write_u8(properties.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(properties.armor.armor_points));
    if let Some(economy) = properties.economy {
        hash.write_u8(1);
        hash.write_u64(u64::from(economy.gold_cost));
        hash.write_u64(u64::from(economy.lumber_cost));
        hash.write_u64(u64::from(economy.lumber_refund));
        hash.write_u64(economy.income_per_10k);
    } else {
        hash.write_u8(0);
    }
    if let Some(production) = building.production {
        hash.write_u8(1);
        hash.write_u16(production.initial_delay_ticks);
        hash.write_u16(production.interval_ticks);
        hash.write_u16(production.search_radius_cells);
        hash.write_i32(production.unit.health);
        hash_attack_delivery(hash, production.unit.attack.delivery);
        hash.write_i32(production.unit.attack.damage);
        hash.write_i32(production.unit.attack.range);
        hash.write_i32(production.unit.attack.acquisition_range);
        hash.write_u16(production.unit.attack.cooldown_ticks);
        hash.write_i32(production.unit.movement.speed_per_tick);
        let unit = properties.production_unit;
        hash_content_identity(hash, unit.content);
        hash.write_u8(match unit.movement_class {
            MovementClass::Ground => 0,
            MovementClass::Air => 1,
        });
        hash.write_u8(u8::from(unit.mechanical));
        hash_optional_u32(hash, unit.build_time_ticks);
        hash_optional_u32(hash, unit.repair_time_ticks);
        hash.write_u8(unit.attack_targets.bits());
        hash.write_u32(unit.health_regen_per_second_per_10k);
        hash.write_u8(unit.damage_type.stable_tag());
        hash.write_u8(unit.armor.armor_type.stable_tag());
        hash.write_i32(i32::from(unit.armor.armor_points));
        hash_passive_unit_effects(hash, unit.passive_effects);
        if let Some(corpse) = unit.corpse {
            hash.write_u8(1);
            hash.write_u64(u64::from(corpse.definition.0));
            hash_optional_u32(hash, corpse.lifetime_ticks);
        } else {
            hash.write_u8(0);
        }
        if let Some(radius) = unit.collision_radius {
            hash.write_u8(1);
            hash.write_i32(radius.0);
        } else {
            hash.write_u8(0);
        }
        if let Some(spellcasting) = properties.production_spellcasting {
            hash.write_u8(1);
            hash_spellcasting_profile(hash, spellcasting);
        } else {
            hash.write_u8(0);
        }
    } else {
        hash.write_u8(0);
    }
    if let Some(attack) = building.attack {
        hash.write_u8(1);
        hash_attack_delivery(hash, attack.delivery);
        hash.write_i32(attack.damage);
        hash.write_i32(attack.range);
        hash.write_i32(attack.acquisition_range);
        hash.write_u16(attack.cooldown_ticks);
    } else {
        hash.write_u8(0);
    }
    if let Some(spellcasting) = building.spellcasting {
        hash.write_u8(1);
        hash_spellcasting_profile(hash, spellcasting);
    } else {
        hash.write_u8(0);
    }
}

fn hash_spellcasting_profile(hash: &mut Fnv64, spellcasting: SpellcastingProfile) {
    hash.write_i32(spellcasting.mana.maximum);
    hash.write_i32(spellcasting.mana.starting);
    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
    hash_automatic_ability(hash, spellcasting.ability);
}

fn hash_passive_unit_effects(hash: &mut Fnv64, effects: PassiveUnitEffects) {
    let effects: Vec<_> = effects.iter().collect();
    hash.write_u8(u8::try_from(effects.len()).expect("passive effect count fits u8"));
    for effect in effects {
        match effect {
            PassiveUnitEffect::Bash(profile) => {
                hash.write_u8(0);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
                hash.write_i32(profile.bonus_damage);
                hash.write_u16(profile.stun_duration_ticks);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Evasion(profile) => {
                hash.write_u8(1);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
            }
            PassiveUnitEffect::Defend(profile) => {
                hash.write_u8(2);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.ranged_damage_taken_per_10k);
                hash.write_u16(profile.spell_damage_taken_per_10k);
                hash.write_u16(profile.deflect_chance_per_10k);
                hash.write_u16(profile.deflected_pierce_damage_taken_per_10k);
                hash.write_u16(profile.activation_delay_ticks);
            }
            PassiveUnitEffect::TriggeredSpellProc(profile) => {
                hash.write_u8(3);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
                hash.write_u8(profile.targets.bits());
                hash_triggered_attack_effect(hash, profile.effect);
            }
            PassiveUnitEffect::BurningOil(profile) => {
                hash.write_u8(4);
                hash_burning_oil_profile(hash, profile);
            }
        }
    }
}

fn hash_triggered_attack_effect(hash: &mut Fnv64, effect: TriggeredAttackEffect) {
    match effect {
        TriggeredAttackEffect::ChainLightning(profile) => {
            hash.write_u8(0);
            hash.write_u64(u64::from(profile.ability.0));
            hash.write_i32(profile.initial_damage);
            hash.write_u8(profile.maximum_targets);
            hash.write_i32(profile.jump_radius);
            hash.write_u16(profile.damage_reduction_per_10k);
            hash.write_u8(profile.targets.bits());
        }
        TriggeredAttackEffect::EntanglingRoots(profile) => {
            hash.write_u8(1);
            hash.write_u64(u64::from(profile.ability.0));
            hash.write_i32(profile.damage_per_second);
            hash.write_u16(profile.duration_ticks);
            hash.write_u8(profile.targets.bits());
        }
    }
}

fn hash_burning_oil_profile(hash: &mut Fnv64, profile: crate::components::BurningOilEffectProfile) {
    hash.write_u64(u64::from(profile.ability.0));
    hash.write_i32(profile.radius);
    hash.write_i32(profile.full_damage);
    hash.write_u16(profile.full_interval_millis);
    hash.write_i32(profile.half_damage);
    hash.write_u16(profile.half_interval_millis);
    hash.write_u16(profile.full_duration_millis);
    hash.write_u16(profile.total_duration_millis);
    hash.write_u8(u8::from(profile.target_ground_units));
    hash.write_u8(u8::from(profile.target_buildings));
}

fn hash_pending_attack_effects(hash: &mut Fnv64, effects: PendingAttackEffects) {
    hash.write_u16(effects.stun_duration_ticks);
    match effects.triggered_spell {
        Some(effect) => {
            hash.write_u8(1);
            hash_triggered_attack_effect(hash, effect);
        }
        None => hash.write_u8(0),
    }
    match effects.burning_oil {
        Some(profile) => {
            hash.write_u8(1);
            hash_burning_oil_profile(hash, profile);
        }
        None => hash.write_u8(0),
    }
}

fn hash_automatic_ability(hash: &mut Fnv64, ability: AutomaticAbilityProfile) {
    hash.write_u64(u64::from(ability.id.0));
    hash.write_i32(ability.mana_cost);
    hash.write_u16(ability.cooldown_ticks);
    hash.write_i32(ability.range);
    hash.write_u8(ability.target_policy.stable_tag());
    hash.write_u8(ability.effect.stable_tag());
    match ability.effect {
        AbilityEffect::Damage { amount } => hash.write_i32(amount),
        AbilityEffect::Stun { duration_ticks } => hash.write_u16(duration_ticks),
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier,
            percent_delta,
            duration_ticks,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(i32::from(percent_delta));
            hash.write_u16(duration_ticks);
        }
        AbilityEffect::AreaDamage { amount, radius } => {
            hash.write_i32(amount);
            hash.write_i32(radius);
        }
        AbilityEffect::FrostArmor {
            modifier,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u16(armor_duration_ticks);
            hash.write_u16(slow_duration_ticks);
            hash.write_i32(i32::from(movement_percent_delta));
            hash.write_i32(i32::from(attack_speed_percent_delta));
        }
    }
}

fn hash_attack_delivery(hash: &mut Fnv64, delivery: AttackDelivery) {
    hash.write_u8(delivery.stable_tag());
    match delivery {
        AttackDelivery::Melee => {}
        AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
            hash.write_i32(speed_per_tick);
        }
        AttackDelivery::RangedBallistic {
            speed_per_tick,
            impact_radius,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(impact_radius);
        }
        AttackDelivery::Bounce {
            speed_per_tick,
            bounce_range,
            max_bounces,
            damage_percent_per_bounce,
            allow_repeat_targets,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(bounce_range);
            hash.write_u8(max_bounces);
            hash.write_u16(damage_percent_per_bounce);
            hash.write_u8(u8::from(allow_repeat_targets));
        }
    }
}

struct Fnv64(u64);

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write(&value.to_le_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod canonical_checksum_tests {
    use super::*;

    fn inert_unit() -> UnitSpawn {
        UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    #[test]
    fn allocator_history_changes_authoritative_checksum() {
        let first = Simulation::new(SimulationConfig::default(), 1);
        let mut second = Simulation::new(SimulationConfig::default(), 1);
        second.next_id = second.next_id.checked_add(1).unwrap();

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn match_seed_changes_authoritative_checksum() {
        let first = Simulation::new(
            SimulationConfig {
                match_seed: 1,
                ..SimulationConfig::default()
            },
            1,
        );
        let second = Simulation::new(
            SimulationConfig {
                match_seed: 2,
                ..SimulationConfig::default()
            },
            1,
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn gameplay_bundle_identity_changes_authoritative_checksum() {
        let first = Simulation::new_with_gameplay_bundle(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
            GameplayBundleIdentity {
                schema_version: 1,
                gameplay_hash: 0x1111,
            },
        );
        let second = Simulation::new_with_gameplay_bundle(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
            GameplayBundleIdentity {
                schema_version: 1,
                gameplay_hash: 0x2222,
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn combat_rules_change_authoritative_checksum() {
        let first = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
        );
        let second = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            1,
            CombatRules {
                damage_rules: DamageRules::from_wc3_misc_text("[Misc]\nDefenseArmor=0.07\n")
                    .unwrap(),
                ..CombatRules::default()
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn optional_component_presence_changes_authoritative_checksum() {
        let mut absent = Simulation::new(SimulationConfig::default(), 1);
        absent.spawn_unit_with_properties(inert_unit(), UnitGameplayProperties::default());

        let mut present = Simulation::new(SimulationConfig::default(), 1);
        present.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                build_time_ticks: Some(0),
                ..UnitGameplayProperties::default()
            },
        );

        assert_ne!(absent.checksum(), present.checksum());
    }

    #[test]
    fn content_rawcode_changes_authoritative_checksum() {
        let mut first = Simulation::new(SimulationConfig::default(), 1);
        first.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                content: Some(ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"u001"),
                    name: "first",
                }),
                ..UnitGameplayProperties::default()
            },
        );

        let mut second = Simulation::new(SimulationConfig::default(), 1);
        second.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                content: Some(ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"u002"),
                    name: "second",
                }),
                ..UnitGameplayProperties::default()
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }
}
