use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use bevy_ecs::{entity::Entity, prelude::World};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use crate::{
    components::{
        AttackCooldown, AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, Health,
        MovementProfile, Position, ProductionProfile, ProductionState, RetaliationState, SimId,
        SpawnTick, TargetState, Team, UnitSpawn,
    },
    math::{SUBUNITS_PER_WORLD_UNIT, SimPoint},
    spatial::{SpatialGrid, SpatialPartition},
    topology::{NavCell, TopologyGrid},
};

#[derive(Debug, Clone)]
pub struct SimulationConfig {
    pub spatial_cell_size: i32,
    pub navigation_cell_size: i32,
    pub navigation_min: NavCell,
    pub navigation_max: NavCell,
    pub target_pursuit_extra_range: i32,
    pub unit_separation_distance: i32,
    pub max_separation_per_tick: i32,
    pub static_blockers: Vec<BuildingFootprint>,
    pub team_objective: [SimPoint; 2],
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            spatial_cell_size: 8 * SUBUNITS_PER_WORLD_UNIT,
            navigation_cell_size: SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, -64),
            navigation_max: NavCell::new(120, 64),
            target_pursuit_extra_range: 3 * SUBUNITS_PER_WORLD_UNIT,
            unit_separation_distance: 3 * SUBUNITS_PER_WORLD_UNIT / 4,
            max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT / 16,
            static_blockers: Vec::new(),
            team_objective: [
                SimPoint::new(120 * SUBUNITS_PER_WORLD_UNIT, 0),
                SimPoint::new(0, 0),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickTimings {
    pub topology_and_timers: Duration,
    pub production: Duration,
    pub snapshot_and_spatial: Duration,
    pub targeting: Duration,
    pub combat: Duration,
    pub crowd_separation: Duration,
    pub movement_and_commit: Duration,
    pub checksum: Duration,
    pub total: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickResult {
    pub completed_tick: u64,
    pub units_alive: usize,
    pub buildings_alive: usize,
    pub attacks_resolved: usize,
    pub deaths: usize,
    pub units_spawned: usize,
    pub spawn_failures: usize,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub health: i32,
    pub attack_delivery: AttackDelivery,
    pub target: Option<SimId>,
    pub last_attacker: Option<SimId>,
    pub cooldown_remaining: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingView {
    pub id: SimId,
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub production: Option<ProductionProfile>,
    pub next_spawn_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingPlacementError {
    OutsideNavigation,
    StaticObstacle,
    BuildingOverlap,
    UnitOccupied,
}

pub struct Simulation {
    world: World,
    config: SimulationConfig,
    pool: ThreadPool,
    topology: TopologyGrid,
    topology_dirty: bool,
    defense_alerts: Vec<DefenseAlert>,
    last_attacks: Vec<AttackEvent>,
    next_tick: u64,
    next_id: u64,
}

impl Simulation {
    pub fn new(config: SimulationConfig, workers: usize) -> Self {
        assert!(workers > 0, "simulation requires at least one worker");
        assert!(config.spatial_cell_size > 0);
        assert!(config.navigation_cell_size > 0);
        assert!(config.target_pursuit_extra_range >= 0);
        assert!(config.unit_separation_distance >= 0);
        assert!(config.max_separation_per_tick >= 0);

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

        Self {
            world: World::new(),
            config,
            pool,
            topology,
            topology_dirty: false,
            defense_alerts: Vec::new(),
            last_attacks: Vec::new(),
            next_tick: 0,
            next_id: 1,
        }
    }

    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.next_tick
    }

    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.pool.current_num_threads()
    }

    pub fn spawn_unit(&mut self, unit: UnitSpawn) -> SimId {
        validate_unit_spawn(unit);
        self.spawn_unit_unchecked(unit)
    }

    pub fn spawn_building(&mut self, building: BuildingSpawn) -> SimId {
        self.try_spawn_building(building)
            .expect("invalid authored building placement")
    }

    pub fn try_spawn_building(
        &mut self,
        building: BuildingSpawn,
    ) -> Result<SimId, BuildingPlacementError> {
        assert!(building.health > 0);
        assert!(building.team.0 < 2, "verification slice supports two teams");
        assert!(building.footprint.width > 0 && building.footprint.height > 0);
        if let Some(production) = building.production {
            assert!(production.interval_ticks > 0);
            validate_unit_template(production.unit);
        }

        if !self.footprint_inside_navigation(building.footprint) {
            return Err(BuildingPlacementError::OutsideNavigation);
        }
        if self
            .config
            .static_blockers
            .iter()
            .copied()
            .any(|blocker| footprints_overlap(blocker, building.footprint))
        {
            return Err(BuildingPlacementError::StaticObstacle);
        }
        if self
            .world
            .iter_entities()
            .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
            .any(|existing| footprints_overlap(existing, building.footprint))
        {
            return Err(BuildingPlacementError::BuildingOverlap);
        }
        if self.footprint_contains_live_unit(building.footprint) {
            return Err(BuildingPlacementError::UnitOccupied);
        }

        let id = self.allocate_id();
        let mut entity = self.world.spawn((
            id,
            building.team,
            building.footprint,
            Health {
                current: building.health,
                max: building.health,
            },
        ));
        if let Some(production) = building.production {
            let next_spawn_tick = self
                .next_tick
                .checked_add(u64::from(production.initial_delay_ticks))
                .expect("initial production tick overflow");
            entity.insert((production, ProductionState { next_spawn_tick }));
        }
        self.topology_dirty = true;
        Ok(id)
    }

    #[must_use]
    pub fn can_place_building(&self, footprint: BuildingFootprint) -> bool {
        self.footprint_inside_navigation(footprint)
            && !self
                .config
                .static_blockers
                .iter()
                .copied()
                .any(|blocker| footprints_overlap(blocker, footprint))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
                .any(|existing| footprints_overlap(existing, footprint))
            && !self.footprint_contains_live_unit(footprint)
    }

    fn footprint_contains_live_unit(&self, footprint: BuildingFootprint) -> bool {
        self.world.iter_entities().any(|entity| {
            let Some(position) = entity.get::<Position>() else {
                return false;
            };
            let alive = entity
                .get::<Health>()
                .is_none_or(|health| health.current > 0);
            alive && footprint_contains_cell(footprint, self.topology.cell_of_point(position.0))
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
        let tick_start = Instant::now();
        let completed_tick = self.next_tick;

        let phase_start = Instant::now();
        self.refresh_topology_if_dirty();
        self.advance_cooldowns();
        let topology_and_timers = phase_start.elapsed();

        let phase_start = Instant::now();
        let (units_spawned, spawn_failures) = self.advance_production();
        let production = phase_start.elapsed();

        let phase_start = Instant::now();
        let mut units = self.snapshot_units();
        let buildings = self.snapshot_buildings();
        let has_ranged = units
            .iter()
            .any(|unit| unit.attack.delivery == AttackDelivery::RangedGuaranteedHit);
        let grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            units
                .iter()
                .enumerate()
                .filter_map(|(index, unit)| {
                    let cell = self.topology.cell_of_point(unit.position);
                    let component = self.topology.component_id(cell)?;
                    Some((index, *unit, component))
                })
                .flat_map(|(index, unit, component)| {
                    [
                        Some((
                            SpatialPartition::new(unit.team.0, component),
                            index,
                            unit.position,
                        )),
                        has_ranged.then_some((
                            SpatialPartition::global(unit.team.0),
                            index,
                            unit.position,
                        )),
                    ]
                    .into_iter()
                    .flatten()
                }),
        );
        let alert_grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            self.defense_alerts
                .iter()
                .enumerate()
                .map(|(index, alert)| {
                    (
                        SpatialPartition::global(alert.victim_team.0),
                        index,
                        alert.victim_position,
                    )
                }),
        );
        let snapshot_and_spatial = phase_start.elapsed();

        let phase_start = Instant::now();
        let choices = self.select_targets(&units, &buildings, &grid, &alert_grid);
        for (unit, target) in units.iter_mut().zip(choices) {
            unit.target = target;
        }
        let targeting = phase_start.elapsed();

        let mut unit_health: Vec<i32> = units.iter().map(|unit| unit.health).collect();
        let mut building_health: Vec<i32> =
            buildings.iter().map(|building| building.health).collect();
        let mut cooldowns: Vec<u16> = units.iter().map(|unit| unit.cooldown_remaining).collect();
        let mut positions: Vec<SimPoint> = units.iter().map(|unit| unit.position).collect();
        let mut attackers_this_tick = vec![None; units.len()];
        let mut next_defense_alerts = Vec::new();

        let phase_start = Instant::now();
        let mut intents = self.attack_intents(&units, &buildings);
        intents.sort_unstable_by_key(|intent| (intent.source_id, intent.target_id));

        let mut attacks_resolved = 0;
        for intent in intents {
            if unit_health[intent.source_index] <= 0 {
                continue;
            }

            let target_alive = match intent.target {
                TargetIndex::Unit(index) => unit_health[index] > 0,
                TargetIndex::Building(index) => building_health[index] > 0,
            };
            if !target_alive {
                continue;
            }

            let target_position = match intent.target {
                TargetIndex::Unit(index) => {
                    unit_health[index] = unit_health[index]
                        .checked_sub(intent.damage)
                        .expect("unit damage arithmetic overflowed validated bounds");
                    attackers_this_tick[index].get_or_insert(intent.source_id);
                    next_defense_alerts.push(DefenseAlert {
                        victim_id: units[index].id,
                        victim_team: units[index].team,
                        victim_position: units[index].position,
                        attacker_id: intent.source_id,
                        attacked_tick: completed_tick,
                    });
                    units[index].position
                }
                TargetIndex::Building(index) => {
                    building_health[index] = building_health[index]
                        .checked_sub(intent.damage)
                        .expect("building damage arithmetic overflowed validated bounds");
                    footprint_center_point(
                        buildings[index].footprint,
                        self.config.navigation_cell_size,
                    )
                }
            };
            cooldowns[intent.source_index] = intent.cooldown_ticks;
            self.last_attacks.push(AttackEvent {
                source: intent.source_id,
                target: intent.target_id,
                source_position: units[intent.source_index].position,
                target_position,
                delivery: units[intent.source_index].attack.delivery,
            });
            attacks_resolved += 1;
        }
        let combat = phase_start.elapsed();

        let phase_start = Instant::now();
        let crowd_separation = self.resolve_movement(
            &units,
            &buildings,
            &unit_health,
            &building_health,
            &mut positions,
        );

        let mut deaths = 0;
        for (index, unit) in units.iter().enumerate() {
            if unit_health[index] <= 0 {
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
                .get_mut::<Position>()
                .expect("unit position missing")
                .0 = positions[index];
            entity
                .get_mut::<TargetState>()
                .expect("unit target missing")
                .current = live_target;
            *entity
                .get_mut::<RetaliationState>()
                .expect("unit retaliation state missing") = match attackers_this_tick[index] {
                Some(attacker) => RetaliationState {
                    attacker: Some(attacker),
                    attacked_tick: Some(completed_tick),
                },
                None => RetaliationState::default(),
            };
        }

        let mut building_deaths = Vec::new();
        for (index, building) in buildings.iter().enumerate() {
            if building_health[index] <= 0 {
                building_deaths.push(building.entity);
                deaths += 1;
            } else {
                self.world
                    .entity_mut(building.entity)
                    .get_mut::<Health>()
                    .expect("building health missing")
                    .current = building_health[index];
            }
        }
        if !building_deaths.is_empty() {
            for entity in building_deaths {
                self.world.despawn(entity);
            }
            self.topology_dirty = true;
        }

        self.defense_alerts = next_defense_alerts;
        self.next_tick = self
            .next_tick
            .checked_add(1)
            .expect("tick counter exhausted");
        let movement_and_commit = phase_start.elapsed();

        let phase_start = Instant::now();
        let checksum = canonical_checksum(&self.world, self.next_tick, &self.defense_alerts);
        let checksum_time = phase_start.elapsed();
        let timings = TickTimings {
            topology_and_timers,
            production,
            snapshot_and_spatial,
            targeting,
            combat,
            crowd_separation,
            movement_and_commit,
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
            attacks_resolved,
            deaths,
            units_spawned,
            spawn_failures,
            checksum,
            timings,
        }
    }

    #[must_use]
    pub fn checksum(&self) -> u64 {
        canonical_checksum(&self.world, self.next_tick, &self.defense_alerts)
    }

    #[must_use]
    pub fn attacks_last_tick(&self) -> &[AttackEvent] {
        &self.last_attacks
    }

    #[must_use]
    pub fn unit_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| {
                entity.get::<Position>().is_some() && entity.get::<BuildingFootprint>().is_none()
            })
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
    pub fn units(&self) -> Vec<UnitView> {
        let mut units: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(unit_view_from_entity)
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
        self.world
            .iter_entities()
            .filter_map(unit_view_from_entity)
            .find(|unit| unit.id == id)
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

    fn spawn_unit_unchecked(&mut self, unit: UnitSpawn) -> SimId {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            unit.team,
            Position(unit.position),
            Health {
                current: unit.health,
                max: unit.health,
            },
            unit.attack,
            AttackCooldown::default(),
            TargetState::default(),
            RetaliationState::default(),
            unit.movement,
            SpawnTick(self.next_tick),
        ));
        id
    }

    fn refresh_topology_if_dirty(&mut self) {
        if !self.topology_dirty {
            return;
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
    }

    fn advance_cooldowns(&mut self) {
        let mut query = self.world.query::<&mut AttackCooldown>();
        for mut cooldown in query.iter_mut(&mut self.world) {
            cooldown.remaining = cooldown.remaining.saturating_sub(1);
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
        )>();
        let mut attempts: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, _, state)| state.next_spawn_tick <= self.next_tick)
            .map(
                |(entity, id, team, footprint, profile, state)| ProductionAttempt {
                    entity,
                    id: *id,
                    team: *team,
                    footprint: *footprint,
                    profile: *profile,
                    next_spawn_tick: state.next_spawn_tick,
                },
            )
            .collect();
        attempts.sort_unstable_by_key(|attempt| attempt.id);

        if attempts.is_empty() {
            return (0, 0);
        }

        let mut occupied: HashSet<NavCell> = self
            .units()
            .into_iter()
            .map(|unit| self.topology.cell_of_point(unit.position))
            .collect();
        let mut spawned = 0;
        let mut failed = 0;

        for attempt in attempts {
            let preferred = preferred_spawn_cell(attempt.team, attempt.footprint);
            let spawn_cell =
                spiral_cells(preferred, attempt.profile.search_radius_cells).find(|cell| {
                    self.topology.contains(*cell)
                        && !self.topology.is_blocked(*cell)
                        && !occupied.contains(cell)
                });

            if let Some(cell) = spawn_cell {
                let position = self.topology.center_of_cell(cell);
                self.spawn_unit_unchecked(UnitSpawn::from_template(
                    attempt.team,
                    position,
                    attempt.profile.unit,
                ));
                occupied.insert(cell);
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
            &MovementProfile,
            &SpawnTick,
        )>();
        let mut units: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, health, _, _, _, _, _, _)| health.current > 0)
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
                    movement,
                    spawn_tick,
                )| UnitSnapshot {
                    entity,
                    id: *id,
                    team: *team,
                    position: position.0,
                    health: health.current,
                    attack: *attack,
                    cooldown_remaining: cooldown.remaining,
                    target: target.current,
                    retaliation: *retaliation,
                    movement: *movement,
                    spawn_tick: spawn_tick.0,
                },
            )
            .collect();
        units.sort_unstable_by_key(|unit| unit.id);
        units
    }

    fn snapshot_buildings(&mut self) -> Vec<BuildingSnapshot> {
        let mut query = self
            .world
            .query::<(Entity, &SimId, &Team, &BuildingFootprint, &Health)>();
        let mut buildings: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, health)| health.current > 0)
            .map(|(entity, id, team, footprint, health)| BuildingSnapshot {
                entity,
                id: *id,
                team: *team,
                footprint: *footprint,
                health: health.current,
            })
            .collect();
        buildings.sort_unstable_by_key(|building| building.id);
        buildings
    }

    fn select_targets(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
        alert_grid: &SpatialGrid,
    ) -> Vec<Option<SimId>> {
        self.pool.install(|| {
            units
                .par_iter()
                .map(|unit| {
                    let current = unit.target.filter(|target| {
                        self.target_retainable_for(unit, *target, units, buildings)
                    });
                    let retaliation = self.recent_retaliation_target(unit, units, buildings);

                    if let Some(current) = current {
                        let current_fights_back = find_unit_index(units, current)
                            .is_some_and(|index| units[index].target == Some(unit.id));
                        if current_fights_back {
                            return Some(current);
                        }
                        if let Some(attacker) = retaliation
                            && attacker != current
                        {
                            return Some(attacker);
                        }
                        if let Some(attacker) =
                            self.recent_ally_defense_target(unit, units, buildings, alert_grid)
                            && attacker != current
                        {
                            return Some(attacker);
                        }
                        return Some(current);
                    }

                    if let Some(attacker) = retaliation {
                        return Some(attacker);
                    }
                    if let Some(attacker) =
                        self.recent_ally_defense_target(unit, units, buildings, alert_grid)
                    {
                        return Some(attacker);
                    }

                    self.acquire_target(unit, units, buildings, grid)
                })
                .collect()
        })
    }

    fn acquire_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
    ) -> Option<SimId> {
        let source_cell = self.topology.cell_of_point(source.position);
        let component = self.topology.component_id(source_cell)?;
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let partition = match source.attack.delivery {
            AttackDelivery::Melee => SpatialPartition::new(enemy_team, component),
            AttackDelivery::RangedGuaranteedHit => SpatialPartition::global(enemy_team),
        };
        let mut best: Option<(u8, u64, SimId)> = None;
        grid.for_each_candidate(
            partition,
            source.position,
            source.attack.acquisition_range,
            |index| {
                let candidate = &units[index];
                debug_assert_ne!(source.team, candidate.team);
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
        self.target_retainable_for(source, attacker, units, buildings)
            .then_some(attacker)
    }

    fn recent_ally_defense_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        alert_grid: &SpatialGrid,
    ) -> Option<SimId> {
        let previous_tick = self.next_tick.checked_sub(1)?;
        let acquisition_range_sq = source.attack.acquisition_range_sq();
        let mut best: Option<(u64, u64, SimId, SimId)> = None;

        alert_grid.for_each_candidate(
            SpatialPartition::global(source.team.0),
            source.position,
            source.attack.acquisition_range,
            |alert_index| {
                let alert = &self.defense_alerts[alert_index];
                if alert.attacked_tick != previous_tick || alert.victim_id == source.id {
                    return;
                }
                let ally_distance_sq = source.position.distance_sq(alert.victim_position);
                if ally_distance_sq > acquisition_range_sq
                    || !self.target_retainable_for(source, alert.attacker_id, units, buildings)
                {
                    return;
                }

                let attacker_distance_sq =
                    if let Some(index) = find_unit_index(units, alert.attacker_id) {
                        source.position.distance_sq(units[index].position)
                    } else if let Some(index) = find_building_index(buildings, alert.attacker_id) {
                        point_to_footprint_distance_sq(
                            source.position,
                            buildings[index].footprint,
                            self.config.navigation_cell_size,
                        )
                    } else {
                        return;
                    };
                let key = (
                    ally_distance_sq,
                    attacker_distance_sq,
                    alert.attacker_id,
                    alert.victim_id,
                );
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );

        best.map(|(_, _, attacker, _)| attacker)
    }

    fn target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let pursuit_range = source
            .attack
            .range
            .checked_add(self.config.target_pursuit_extra_range)
            .expect("pursuit range overflowed validated coordinate bounds")
            .max(source.attack.acquisition_range);
        let pursuit_range = i64::from(pursuit_range);
        let pursuit_range_sq = (pursuit_range * pursuit_range) as u64;
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
        if distance_sq > pursuit_limit_sq {
            return false;
        }
        let in_attack_range = distance_sq <= source.attack.range_sq();
        match source.attack.delivery {
            AttackDelivery::Melee => {
                self.topology.same_component(
                    self.topology.cell_of_point(source.position),
                    self.topology.cell_of_point(target.position),
                ) && (in_attack_range || source.movement.speed_per_tick > 0)
            }
            AttackDelivery::RangedGuaranteedHit => {
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
        if distance_sq > pursuit_limit_sq {
            return false;
        }
        let in_attack_range = distance_sq <= source.attack.range_sq();
        if source.attack.delivery == AttackDelivery::RangedGuaranteedHit && in_attack_range {
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

    fn attack_intents(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Vec<AttackIntent> {
        self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    if source.spawn_tick == self.next_tick || source.cooldown_remaining != 0 {
                        return None;
                    }
                    let target_id = source.target?;
                    let (target, distance_sq) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            (
                                TargetIndex::Unit(index),
                                source.position.distance_sq(units[index].position),
                            )
                        } else {
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
                        source_index,
                        target,
                        source_id: source.id,
                        target_id,
                        damage: source.attack.damage,
                        cooldown_ticks: source.attack.cooldown_ticks,
                    })
                })
                .collect()
        })
    }

    fn resolve_movement(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        positions: &mut [SimPoint],
    ) -> Duration {
        let desired_positions: Vec<_> = self.pool.install(|| {
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

        let separation_start = Instant::now();
        let separated_positions =
            self.apply_crowd_separation(units, unit_health, &desired_positions);
        let crowd_separation = separation_start.elapsed();
        positions.copy_from_slice(&separated_positions);
        crowd_separation
    }

    fn desired_position(
        &self,
        index: usize,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
    ) -> SimPoint {
        let current = unit.position;
        if unit_health[index] <= 0 || unit.movement.speed_per_tick == 0 {
            return current;
        }
        let source_cell = self.topology.cell_of_point(current);

        let target_cell = unit.target.and_then(|target_id| {
            if let Some(target_index) = find_unit_index(units, target_id) {
                if unit_health[target_index] <= 0 {
                    return None;
                }
                let target_position = units[target_index].position;
                if current.distance_sq(target_position) <= unit.attack.range_sq() {
                    return Some(source_cell);
                }
                let cell = self.topology.cell_of_point(target_position);
                self.topology
                    .same_component(source_cell, cell)
                    .then_some(cell)
            } else if let Some(target_index) = find_building_index(buildings, target_id) {
                if building_health[target_index] <= 0 {
                    return None;
                }
                if point_to_footprint_distance_sq(
                    current,
                    buildings[target_index].footprint,
                    self.config.navigation_cell_size,
                ) <= unit.attack.range_sq()
                {
                    return Some(source_cell);
                }
                self.topology.nearest_reachable_perimeter_cell(
                    source_cell,
                    buildings[target_index].footprint,
                )
            } else {
                None
            }
        });

        let next_cell = match target_cell {
            Some(cell) if cell == source_cell => return current,
            Some(cell) => self.topology.pursuit_step(source_cell, cell),
            None => self.topology.objective_step(unit.team.0, source_cell),
        };
        let Some(next_cell) = next_cell else {
            return current;
        };
        let target_position = self.topology.center_of_cell(next_cell);
        let candidate = current.step_towards(target_position, unit.movement.speed_per_tick);
        let candidate_cell = self.topology.cell_of_point(candidate);
        if self.topology.is_blocked(candidate_cell)
            || !self.topology.same_component(source_cell, candidate_cell)
        {
            current
        } else {
            candidate
        }
    }

    fn apply_crowd_separation(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        desired_positions: &[SimPoint],
    ) -> Vec<SimPoint> {
        let separation_distance = self.config.unit_separation_distance;
        let max_separation = self.config.max_separation_per_tick;
        if separation_distance == 0 || max_separation == 0 {
            return desired_positions.to_vec();
        }

        let collision_grid = SpatialGrid::build(
            separation_distance.max(1),
            desired_positions
                .iter()
                .enumerate()
                .filter(|(index, _)| unit_health[*index] > 0)
                .filter_map(|(index, position)| {
                    let component = self
                        .topology
                        .component_id(self.topology.cell_of_point(*position))?;
                    Some((SpatialPartition::new(0, component), index, *position))
                }),
        );
        let separation_sq = square_i32(separation_distance);

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
                    let Some(component) = self.topology.component_id(current_cell) else {
                        return desired;
                    };
                    let partition = SpatialPartition::new(0, component);
                    let movement_x = i64::from(desired.x) - i64::from(unit.position.x);
                    let movement_y = i64::from(desired.y) - i64::from(unit.position.y);
                    let mut push_x = 0_i64;
                    let mut push_y = 0_i64;
                    let mut overlaps = 0_i64;

                    collision_grid.for_each_candidate(
                        partition,
                        desired,
                        separation_distance,
                        |other_index| {
                            if other_index == index || unit_health[other_index] <= 0 {
                                return;
                            }
                            let other = desired_positions[other_index];
                            let distance_sq = desired.distance_sq(other);
                            if distance_sq >= separation_sq {
                                return;
                            }

                            let dx = i64::from(desired.x) - i64::from(other.x);
                            let dy = i64::from(desired.y) - i64::from(other.y);
                            let (direction_x, direction_y, axis_distance) = if dx == 0 && dy == 0 {
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

                            if movement_x != 0 || movement_y != 0 {
                                let other_move_x =
                                    i64::from(other.x) - i64::from(units[other_index].position.x);
                                let other_move_y =
                                    i64::from(other.y) - i64::from(units[other_index].position.y);
                                let to_other_x = i64::from(other.x) - i64::from(unit.position.x);
                                let to_other_y = i64::from(other.y) - i64::from(unit.position.y);
                                let other_is_stationary = other_move_x == 0 && other_move_y == 0;
                                let other_is_ahead =
                                    movement_x * to_other_x + movement_y * to_other_y > 0;
                                if other_is_stationary && other_is_ahead {
                                    let side = i64::from(sidestep_sign(unit.id));
                                    let perpendicular_x = -movement_y.signum() * side;
                                    let perpendicular_y = movement_x.signum() * side;
                                    push_x += perpendicular_x * penetration;
                                    push_y += perpendicular_y * penetration;
                                }
                            }
                            overlaps += 1;
                        },
                    );

                    if overlaps == 0 {
                        return desired;
                    }
                    push_x /= overlaps;
                    push_y /= overlaps;
                    let raw_offset = SimPoint::new(
                        i32::try_from(push_x).expect("crowd x offset overflow"),
                        i32::try_from(push_y).expect("crowd y offset overflow"),
                    );
                    let offset = SimPoint::new(0, 0).step_towards(raw_offset, max_separation);
                    self.valid_separated_position(current_cell, desired, offset)
                })
                .collect()
        })
    }

    fn valid_separated_position(
        &self,
        original_cell: NavCell,
        desired: SimPoint,
        offset: SimPoint,
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
            .find(|candidate| {
                let cell = self.topology.cell_of_point(*candidate);
                !self.topology.is_blocked(cell) && self.topology.same_component(original_cell, cell)
            })
            .unwrap_or(desired)
    }
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
    target: Option<SimId>,
    retaliation: RetaliationState,
    movement: MovementProfile,
    spawn_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct BuildingSnapshot {
    entity: Entity,
    id: SimId,
    team: Team,
    footprint: BuildingFootprint,
    health: i32,
}

#[derive(Debug, Clone, Copy)]
struct ProductionAttempt {
    entity: Entity,
    id: SimId,
    team: Team,
    footprint: BuildingFootprint,
    profile: ProductionProfile,
    next_spawn_tick: u64,
}

#[derive(Debug, Clone, Copy)]
enum TargetIndex {
    Unit(usize),
    Building(usize),
}

#[derive(Debug, Clone, Copy)]
struct AttackIntent {
    source_index: usize,
    target: TargetIndex,
    source_id: SimId,
    target_id: SimId,
    damage: i32,
    cooldown_ticks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DefenseAlert {
    victim_id: SimId,
    victim_team: Team,
    victim_position: SimPoint,
    attacker_id: SimId,
    attacked_tick: u64,
}

fn validate_unit_template(unit: crate::components::UnitTemplate) {
    assert!(unit.health > 0);
    assert!(unit.attack.damage >= 0);
    assert!(unit.attack.range >= 0);
    assert!(unit.attack.acquisition_range >= unit.attack.range);
    assert!(unit.movement.speed_per_tick >= 0);
}

fn validate_unit_spawn(unit: UnitSpawn) {
    validate_unit_template(crate::components::UnitTemplate {
        health: unit.health,
        attack: unit.attack,
        movement: unit.movement,
    });
    assert!(unit.team.0 < 2, "verification slice supports two teams");
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

fn unit_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<UnitView> {
    if entity.get::<BuildingFootprint>().is_some() {
        return None;
    }
    Some(UnitView {
        id: *entity.get::<SimId>()?,
        team: *entity.get::<Team>()?,
        position: entity.get::<Position>()?.0,
        health: entity.get::<Health>()?.current,
        attack_delivery: entity.get::<AttackProfile>()?.delivery,
        target: entity.get::<TargetState>()?.current,
        last_attacker: entity.get::<RetaliationState>()?.attacker,
        cooldown_remaining: entity.get::<AttackCooldown>()?.remaining,
    })
}

fn building_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<BuildingView> {
    let production = entity.get::<ProductionProfile>().copied();
    Some(BuildingView {
        id: *entity.get::<SimId>()?,
        team: *entity.get::<Team>()?,
        footprint: *entity.get::<BuildingFootprint>()?,
        health: entity.get::<Health>()?.current,
        production,
        next_spawn_tick: entity
            .get::<ProductionState>()
            .map(|state| state.next_spawn_tick),
    })
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

fn footprints_overlap(a: BuildingFootprint, b: BuildingFootprint) -> bool {
    a.min_x <= b.max_x() && a.max_x() >= b.min_x && a.min_y <= b.max_y() && a.max_y() >= b.min_y
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

fn canonical_checksum(world: &World, next_tick: u64, defense_alerts: &[DefenseAlert]) -> u64 {
    let mut entities: Vec<CanonicalEntity> = world
        .iter_entities()
        .filter_map(|entity| {
            let id = *entity.get::<SimId>()?;
            let team = *entity.get::<Team>()?;
            let health = *entity.get::<Health>()?;
            if let Some(position) = entity.get::<Position>() {
                Some(CanonicalEntity::Unit(CanonicalUnit {
                    id,
                    team,
                    position: position.0,
                    health,
                    attack: *entity.get::<AttackProfile>()?,
                    movement: *entity.get::<MovementProfile>()?,
                    cooldown: *entity.get::<AttackCooldown>()?,
                    target: *entity.get::<TargetState>()?,
                    retaliation: *entity.get::<RetaliationState>()?,
                    spawn_tick: *entity.get::<SpawnTick>()?,
                }))
            } else {
                Some(CanonicalEntity::Building(CanonicalBuilding {
                    id,
                    team,
                    footprint: *entity.get::<BuildingFootprint>()?,
                    health,
                    production: entity.get::<ProductionProfile>().copied(),
                    production_state: entity.get::<ProductionState>().copied(),
                }))
            }
        })
        .collect();
    entities.sort_unstable_by_key(CanonicalEntity::id);

    let mut hash = Fnv64::new();
    hash.write_u64(next_tick);
    hash.write_u64(entities.len() as u64);
    for entity in entities {
        match entity {
            CanonicalEntity::Unit(unit) => {
                hash.write_u8(0);
                hash.write_u64(unit.id.0);
                hash.write_u8(unit.team.0);
                hash.write_i32(unit.position.x);
                hash.write_i32(unit.position.y);
                hash.write_i32(unit.health.current);
                hash.write_i32(unit.health.max);
                hash.write_u8(unit.attack.delivery.stable_tag());
                hash.write_i32(unit.attack.damage);
                hash.write_i32(unit.attack.range);
                hash.write_i32(unit.attack.acquisition_range);
                hash.write_u16(unit.attack.cooldown_ticks);
                hash.write_i32(unit.movement.speed_per_tick);
                hash.write_u16(unit.cooldown.remaining);
                hash.write_u64(unit.target.current.map_or(0, |target| target.0));
                hash.write_u64(unit.retaliation.attacker.map_or(0, |attacker| attacker.0));
                hash.write_u64(unit.retaliation.attacked_tick.unwrap_or(u64::MAX));
                hash.write_u64(unit.spawn_tick.0);
            }
            CanonicalEntity::Building(building) => {
                hash.write_u8(1);
                hash.write_u64(building.id.0);
                hash.write_u8(building.team.0);
                hash.write_i32(building.footprint.min_x);
                hash.write_i32(building.footprint.min_y);
                hash.write_u16(building.footprint.width);
                hash.write_u16(building.footprint.height);
                hash.write_i32(building.health.current);
                hash.write_i32(building.health.max);
                if let Some(profile) = building.production {
                    hash.write_u8(1);
                    hash.write_u16(profile.initial_delay_ticks);
                    hash.write_u16(profile.interval_ticks);
                    hash.write_u16(profile.search_radius_cells);
                    hash.write_i32(profile.unit.health);
                    hash.write_u8(profile.unit.attack.delivery.stable_tag());
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

#[derive(Debug, Clone, Copy)]
enum CanonicalEntity {
    Unit(CanonicalUnit),
    Building(CanonicalBuilding),
}

impl CanonicalEntity {
    const fn id(&self) -> SimId {
        match self {
            Self::Unit(unit) => unit.id,
            Self::Building(building) => building.id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CanonicalUnit {
    id: SimId,
    team: Team,
    position: SimPoint,
    health: Health,
    attack: AttackProfile,
    movement: MovementProfile,
    cooldown: AttackCooldown,
    target: TargetState,
    retaliation: RetaliationState,
    spawn_tick: SpawnTick,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBuilding {
    id: SimId,
    team: Team,
    footprint: BuildingFootprint,
    health: Health,
    production: Option<ProductionProfile>,
    production_state: Option<ProductionState>,
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
