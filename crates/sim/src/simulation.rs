use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use bevy_ecs::{entity::Entity, prelude::World};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use crate::{
    components::{
        AttackCooldown, AttackDelivery, AttackProfile, BallisticProjectile, BuildingFootprint,
        BuildingSpawn, GuaranteedHitProjectile, Health, MovementProfile, Position,
        ProductionProfile, ProductionState, RetaliationState, SimId, SpawnTick, TargetState, Team,
        UnitSpawn,
    },
    math::{SUBUNITS_PER_WORLD_UNIT, SimPoint},
    spatial::{SpatialGrid, SpatialPartition, SpatialReservationGrid},
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
    pub topology: Duration,
    pub timers: Duration,
    pub production: Duration,
    pub snapshot_and_spatial: Duration,
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
    pub attacks_resolved: usize,
    pub deaths: usize,
    pub units_spawned: usize,
    pub spawn_failures: usize,
    pub topology_rebuilds: usize,
    pub pursuit_steps: usize,
    pub a_star_fallbacks: usize,
    pub a_star_cache_hits: usize,
    pub a_star_expanded_nodes: usize,
    pub projectiles_alive: usize,
    pub projectiles_launched: usize,
    pub projectile_impacts: usize,
    pub projectile_effects: usize,
    pub projectile_invalidations: usize,
    pub ballistic_candidate_checks: usize,
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

const PURSUIT_CACHE_CAPACITY: usize = 65_536;

pub struct Simulation {
    world: World,
    config: SimulationConfig,
    pool: ThreadPool,
    topology: TopologyGrid,
    topology_dirty: bool,
    pursuit_cache: BTreeMap<(NavCell, NavCell), NavCell>,
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
            pursuit_cache: BTreeMap::new(),
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
        let topology_rebuilt = self.refresh_topology_if_dirty();
        let topology = phase_start.elapsed();

        let phase_start = Instant::now();
        self.advance_cooldowns();
        let timers = phase_start.elapsed();

        let phase_start = Instant::now();
        let (units_spawned, spawn_failures) = self.advance_production();
        let production = phase_start.elapsed();

        let phase_start = Instant::now();
        let mut units = self.snapshot_units();
        let buildings = self.snapshot_buildings();
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
        let needs_global_unit_grid = units.iter().any(|unit| {
            matches!(
                unit.attack.delivery,
                AttackDelivery::RangedGuaranteedHit { .. } | AttackDelivery::RangedBallistic { .. }
            )
        });
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
                        needs_global_unit_grid.then_some((
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
        let snapshot_and_spatial = phase_start.elapsed();

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
        let due_projectiles = self.snapshot_due_projectiles();
        let due_ballistic_projectiles = self.snapshot_due_ballistic_projectiles();
        let mut projectile_entities_to_remove =
            Vec::with_capacity(due_projectiles.len() + due_ballistic_projectiles.len());
        let mut projectile_impacts = 0usize;
        let mut projectile_effects = 0usize;
        let mut projectile_invalidations = 0usize;
        let mut ballistic_candidate_checks = 0usize;
        for snapshot in due_projectiles {
            projectile_entities_to_remove.push(snapshot.entity);
            let Some(target) = find_target_index(&units, &buildings, snapshot.projectile.target)
            else {
                projectile_invalidations += 1;
                continue;
            };
            if apply_damage_to_target(
                target,
                snapshot.projectile.source,
                snapshot.projectile.damage,
                completed_tick,
                DamageTargetState {
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

        let mut intents = self.attack_intents(&units, &buildings);
        intents.sort_unstable_by_key(|intent| (intent.source_id, intent.target_id));

        let mut attacks_resolved = 0;
        let mut projectile_launches = Vec::new();
        let mut ballistic_projectile_launches = Vec::new();
        for intent in intents {
            if unit_health[intent.source_index] <= 0 {
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

            let source = &units[intent.source_index];
            match source.attack.delivery {
                AttackDelivery::Melee => {
                    let applied = apply_damage_to_target(
                        intent.target,
                        intent.source_id,
                        intent.damage,
                        completed_tick,
                        DamageTargetState {
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
                }
                AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
                    let travel_ticks = projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                    let impact_tick = completed_tick
                        .checked_add(travel_ticks)
                        .expect("projectile impact tick overflow");
                    projectile_launches.push(ProjectileLaunch {
                        source: intent.source_id,
                        target: intent.target_id,
                        damage: intent.damage,
                        launch_position: source.position,
                        launch_tick: completed_tick,
                        impact_tick,
                    });
                }
                AttackDelivery::RangedBallistic {
                    speed_per_tick,
                    impact_radius,
                } => {
                    let travel_ticks = projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                    let impact_tick = completed_tick
                        .checked_add(travel_ticks)
                        .expect("projectile impact tick overflow");
                    ballistic_projectile_launches.push(BallisticProjectileLaunch {
                        source: intent.source_id,
                        source_team: source.team,
                        damage: intent.damage,
                        launch_position: source.position,
                        destination: target_position,
                        impact_radius,
                        launch_tick: completed_tick,
                        impact_tick,
                    });
                }
            }
            cooldowns[intent.source_index] = intent.cooldown_ticks;
            self.last_attacks.push(AttackEvent {
                source: intent.source_id,
                target: intent.target_id,
                source_position: source.position,
                target_position,
                delivery: source.attack.delivery,
            });
            attacks_resolved += 1;
        }
        let projectiles_launched = projectile_launches.len() + ballistic_projectile_launches.len();
        let combat = phase_start.elapsed();

        let movement = self.resolve_movement(
            &units,
            &buildings,
            &unit_health,
            &building_health,
            &mut positions,
        );

        let phase_start = Instant::now();
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
                                .destination
                                .distance_sq(positions[unit_index])
                                <= radius_sq
                        {
                            targets.push(TargetIndex::Unit(unit_index));
                        }
                    },
                );
                for (building_index, building) in buildings.iter().enumerate() {
                    if building.team.0 != enemy_team || building_health[building_index] <= 0 {
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
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        snapshot.projectile.damage,
                        completed_tick,
                        DamageTargetState {
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
            }
        }
        let ballistic_impact = phase_start.elapsed();

        let phase_start = Instant::now();
        for entity in projectile_entities_to_remove {
            self.world.despawn(entity);
        }
        for launch in projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                GuaranteedHitProjectile {
                    source: launch.source,
                    target: launch.target,
                    damage: launch.damage,
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
                    damage: launch.damage,
                    launch_position: launch.launch_position,
                    destination: launch.destination,
                    impact_radius: launch.impact_radius,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }

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
            let mut target_state = entity
                .get_mut::<TargetState>()
                .expect("unit target missing");
            target_state.current = live_target;
            target_state.direct_retaliation_lock =
                live_target.is_some() && unit.direct_retaliation_lock;
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
        let projectiles_alive = self.projectile_count();
        self.next_tick = self
            .next_tick
            .checked_add(1)
            .expect("tick counter exhausted");
        let structural_commit = phase_start.elapsed();

        let phase_start = Instant::now();
        let checksum = canonical_checksum(&self.world, self.next_tick, &self.defense_alerts);
        let checksum_time = phase_start.elapsed();
        let timings = TickTimings {
            topology,
            timers,
            production,
            snapshot_and_spatial,
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
            attacks_resolved,
            deaths,
            units_spawned,
            spawn_failures,
            topology_rebuilds: usize::from(topology_rebuilt),
            pursuit_steps: movement.pursuit_steps,
            a_star_fallbacks: movement.a_star_fallbacks,
            a_star_cache_hits: movement.a_star_cache_hits,
            a_star_expanded_nodes: movement.a_star_expanded_nodes,
            projectiles_alive,
            projectiles_launched,
            projectile_impacts,
            projectile_effects,
            projectile_invalidations,
            ballistic_candidate_checks,
            retained_targets: target_selection.retained_targets,
            target_changes: target_selection.target_changes,
            ally_defense_queries: target_selection.ally_defense_queries,
            ally_defense_victim_candidates: target_selection.ally_defense_victim_candidates,
            ally_defense_attacker_candidates: target_selection.ally_defense_attacker_candidates,
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
    pub fn projectile_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| {
                entity.get::<GuaranteedHitProjectile>().is_some()
                    || entity.get::<BallisticProjectile>().is_some()
            })
            .count()
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
        true
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

        let units = self.units();
        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let reservation_capacity = units
            .len()
            .checked_add(attempts.len())
            .expect("production reservation capacity overflow");
        let mut reservations = SpatialReservationGrid::build(
            self.config.unit_separation_distance.max(1),
            bounds_min,
            bounds_max,
            reservation_capacity,
            units
                .iter()
                .enumerate()
                .map(|(index, unit)| (index, unit.position)),
        );
        let mut next_reservation_index = units.len();
        let mut spawned = 0;
        let mut failed = 0;

        for attempt in attempts {
            let preferred = preferred_spawn_cell(attempt.team, attempt.footprint);
            let spawn =
                spiral_cells(preferred, attempt.profile.search_radius_cells).find_map(|cell| {
                    if !self.topology.contains(cell) || self.topology.is_blocked(cell) {
                        return None;
                    }
                    let position = self.topology.center_of_cell(cell);
                    reservations
                        .is_clear(position, self.config.unit_separation_distance)
                        .then_some((cell, position))
                });

            if let Some((_cell, position)) = spawn {
                self.spawn_unit_unchecked(UnitSpawn::from_template(
                    attempt.team,
                    position,
                    attempt.profile.unit,
                ));
                reservations.insert(next_reservation_index, position);
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
                    direct_retaliation_lock: target.direct_retaliation_lock,
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

    fn unit_will_query_ally_defense(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let current = unit
            .target
            .filter(|target| self.target_retainable_for(unit, *target, units, buildings));
        if let Some(current) = current
            && (unit.direct_retaliation_lock || find_building_index(buildings, current).is_none())
        {
            return false;
        }
        self.recent_retaliation_target(unit, units, buildings)
            .is_none()
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
                    let current = unit.target.filter(|target| {
                        self.target_retainable_for(unit, *target, units, buildings)
                    });

                    if let Some(current) = current {
                        if unit.direct_retaliation_lock {
                            return TargetDecision::without_defense(Some(current), true);
                        }
                        if let Some(attacker) =
                            self.recent_retaliation_target(unit, units, buildings)
                        {
                            return TargetDecision::without_defense(Some(attacker), true);
                        }
                        if find_building_index(buildings, current).is_some() {
                            let defense = self.recent_ally_defense_target(
                                unit,
                                units,
                                buildings,
                                defense_attacker_grid,
                                defense_victims,
                                alert_grid,
                            );
                            if let Some(attacker) = defense.target {
                                return TargetDecision::with_defense(
                                    Some(attacker),
                                    false,
                                    defense,
                                );
                            }
                            return TargetDecision::with_defense(Some(current), false, defense);
                        }
                        return TargetDecision::without_defense(Some(current), false);
                    }

                    if let Some(attacker) = self.recent_retaliation_target(unit, units, buildings) {
                        return TargetDecision::without_defense(Some(attacker), true);
                    }
                    let defense = self.recent_ally_defense_target(
                        unit,
                        units,
                        buildings,
                        defense_attacker_grid,
                        defense_victims,
                        alert_grid,
                    );
                    if let Some(attacker) = defense.target {
                        return TargetDecision::with_defense(Some(attacker), false, defense);
                    }

                    TargetDecision::with_defense(
                        self.acquire_target(unit, units, buildings, grid),
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
            AttackDelivery::RangedGuaranteedHit { .. } | AttackDelivery::RangedBallistic { .. } => {
                SpatialPartition::global(enemy_team)
            }
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

        loop {
            let Some((ally_distance_sq, victim_indices)) = self.nearest_defense_victim_layer(
                source,
                previous_tick,
                rejected_through_distance,
                defense_victims,
                alert_grid,
                &mut search.victim_candidates,
            ) else {
                return search;
            };

            let mut best: Option<(u64, SimId, SimId)> = None;
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
                let key = (attacker_distance_sq, attacker_id, victim.victim_id);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            }

            if let Some((_, attacker, _)) = best {
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
        }

        for &building_index in &victim.building_attackers {
            *attacker_candidates += 1;
            let target = &context.buildings[building_index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            if !self.building_target_reachable(source, target, distance_sq, pursuit_range_sq) {
                continue;
            }
            let key = (distance_sq, target.id);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best
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
            AttackDelivery::RangedGuaranteedHit { .. } | AttackDelivery::RangedBallistic { .. } => {
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
        if matches!(
            source.attack.delivery,
            AttackDelivery::RangedGuaranteedHit { .. } | AttackDelivery::RangedBallistic { .. }
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
                        distance_sq,
                    })
                })
                .collect()
        })
    }

    fn resolve_movement(
        &mut self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        positions: &mut [SimPoint],
    ) -> MovementMetrics {
        let intent_start = Instant::now();
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
            if self.pursuit_cache.len() >= PURSUIT_CACHE_CAPACITY
                && !self.pursuit_cache.contains_key(&(entry.from, entry.target))
            {
                self.pursuit_cache.clear();
            }
            self.pursuit_cache
                .insert((entry.from, entry.target), entry.next);
        }
        let intent = intent_start.elapsed();
        let desired_positions: Vec<_> =
            decisions.iter().map(|decision| decision.position).collect();
        let pursuit_steps = decisions
            .iter()
            .filter(|decision| decision.pursuit_step)
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
            self.apply_crowd_separation(units, unit_health, &desired_positions);
        let legal_positions = self.enforce_hard_non_overlap(
            units,
            unit_health,
            &desired_positions,
            &separated_positions,
        );
        let crowd_and_collision = separation_start.elapsed();
        positions.copy_from_slice(&legal_positions);
        MovementMetrics {
            intent,
            crowd_and_collision,
            pursuit_steps,
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
        if unit_health[index] <= 0 || unit.movement.speed_per_tick == 0 {
            return MovementDecision::stationary(current);
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

        let mut pursuit_step = false;
        let mut used_a_star = false;
        let mut a_star_cache_hit = false;
        let mut a_star_expanded_nodes = 0;
        let mut cache_insert = None;
        let next_cell = match target_cell {
            Some(cell) if cell == source_cell => return MovementDecision::stationary(current),
            Some(cell) => {
                pursuit_step = true;
                let cached_fallback = self.pursuit_cache.get(&(source_cell, cell)).copied();
                let result = self
                    .topology
                    .pursuit_step(source_cell, cell, cached_fallback);
                used_a_star = result.used_a_star;
                a_star_cache_hit = result.a_star_cache_hit;
                a_star_expanded_nodes = result.a_star_expanded_nodes;
                if result.used_a_star
                    && !result.a_star_cache_hit
                    && let Some(next) = result.next_cell
                {
                    cache_insert = Some(PursuitCacheInsert {
                        from: source_cell,
                        target: cell,
                        next,
                    });
                }
                result.next_cell
            }
            None => self.topology.objective_step(unit.team.0, source_cell),
        };
        let Some(next_cell) = next_cell else {
            return MovementDecision {
                position: current,
                pursuit_step,
                used_a_star,
                a_star_cache_hit,
                a_star_expanded_nodes,
                cache_insert,
            };
        };
        let target_position = self.topology.center_of_cell(next_cell);
        let candidate = current.step_towards(target_position, unit.movement.speed_per_tick);
        let candidate_cell = self.topology.cell_of_point(candidate);
        let position = if self.topology.is_blocked(candidate_cell)
            || !self.topology.same_component(source_cell, candidate_cell)
        {
            current
        } else {
            candidate
        };
        MovementDecision {
            position,
            pursuit_step,
            used_a_star,
            a_star_cache_hit,
            a_star_expanded_nodes,
            cache_insert,
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

        let collision_partition = SpatialPartition::global(0);
        let collision_grid = SpatialGrid::build(
            separation_distance.max(1),
            desired_positions
                .iter()
                .enumerate()
                .filter(|(index, _)| unit_health[*index] > 0)
                .map(|(index, position)| (collision_partition, index, *position)),
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
                    if self.topology.component_id(current_cell).is_none() {
                        return desired;
                    }
                    let movement_x = i64::from(desired.x) - i64::from(unit.position.x);
                    let movement_y = i64::from(desired.y) - i64::from(unit.position.y);
                    let mut push_x = 0_i64;
                    let mut push_y = 0_i64;
                    let mut overlaps = 0_i64;

                    collision_grid.for_each_candidate(
                        collision_partition,
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

    fn enforce_hard_non_overlap(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        desired_positions: &[SimPoint],
        separated_positions: &[SimPoint],
    ) -> Vec<SimPoint> {
        let minimum_distance = self.config.unit_separation_distance;
        if minimum_distance == 0 {
            return separated_positions.to_vec();
        }

        let entries = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| (unit_health[index] > 0).then_some((index, unit.position)));
        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let mut reservations = SpatialReservationGrid::build(
            minimum_distance.max(1),
            bounds_min,
            bounds_max,
            units.len(),
            entries,
        );
        let mut result: Vec<_> = units.iter().map(|unit| unit.position).collect();
        let lateral = self.config.max_separation_per_tick.max(1);

        for (index, unit) in units.iter().enumerate() {
            if unit_health[index] <= 0 {
                continue;
            }
            let original_cell = self.topology.cell_of_point(unit.position);
            if self.topology.component_id(original_cell).is_none() {
                continue;
            }
            reservations.remove(index);

            let desired = desired_positions[index];
            let separated = separated_positions[index];
            let sidestep_distance = unit.movement.speed_per_tick.max(lateral).max(1);
            let preferred_side =
                perpendicular_step(unit.id, unit.position, desired, sidestep_distance);
            let opposite_side = SimPoint::new(-preferred_side.x, -preferred_side.y);
            let candidates = [
                Some(separated),
                Some(desired),
                offset_point(unit.position, preferred_side.x, preferred_side.y),
                offset_point(unit.position, opposite_side.x, opposite_side.y),
                Some(unit.position),
            ];

            let chosen = candidates
                .into_iter()
                .flatten()
                .find(|candidate| {
                    self.position_is_traversable_from(original_cell, *candidate)
                        && reservations.is_clear(*candidate, minimum_distance)
                })
                .or_else(|| {
                    self.find_local_non_overlap_position(
                        unit.position,
                        original_cell,
                        minimum_distance,
                        &reservations,
                    )
                })
                .unwrap_or(unit.position);

            reservations.insert(index, chosen);
            result[index] = chosen;
        }

        result
    }

    fn find_local_non_overlap_position(
        &self,
        origin: SimPoint,
        original_cell: NavCell,
        minimum_distance: i32,
        reservations: &SpatialReservationGrid,
    ) -> Option<SimPoint> {
        let step = (minimum_distance / 2).max(1);
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

        for ring in 1..=max_ring {
            let distance = ring.checked_mul(step)?;
            for x_step in -ring..=ring {
                let x = x_step.checked_mul(step)?;
                for y in [-distance, distance] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    if self.position_is_traversable_from(original_cell, candidate)
                        && reservations.is_clear(candidate, minimum_distance)
                    {
                        return Some(candidate);
                    }
                }
            }
            for y_step in (-ring + 1)..=(ring - 1) {
                let y = y_step.checked_mul(step)?;
                for x in [-distance, distance] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    if self.position_is_traversable_from(original_cell, candidate)
                        && reservations.is_clear(candidate, minimum_distance)
                    {
                        return Some(candidate);
                    }
                }
            }
        }
        None
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

    fn position_is_traversable_from(&self, original_cell: NavCell, candidate: SimPoint) -> bool {
        let cell = self.topology.cell_of_point(candidate);
        !self.topology.is_blocked(cell) && self.topology.same_component(original_cell, cell)
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
struct PursuitCacheInsert {
    from: NavCell,
    target: NavCell,
    next: NavCell,
}

#[derive(Debug, Clone, Copy)]
struct MovementDecision {
    position: SimPoint,
    pursuit_step: bool,
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
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MovementMetrics {
    intent: Duration,
    crowd_and_collision: Duration,
    pursuit_steps: usize,
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
    target: Option<SimId>,
    direct_retaliation_lock: bool,
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
    distance_sq: u64,
}

#[derive(Debug, Clone, Copy)]
struct ProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: GuaranteedHitProjectile,
}

#[derive(Debug, Clone, Copy)]
struct BallisticProjectileSnapshot {
    entity: Entity,
    id: SimId,
    projectile: BallisticProjectile,
}

#[derive(Debug, Clone, Copy)]
struct ProjectileLaunch {
    source: SimId,
    target: SimId,
    damage: i32,
    launch_position: SimPoint,
    launch_tick: u64,
    impact_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct BallisticProjectileLaunch {
    source: SimId,
    source_team: Team,
    damage: i32,
    launch_position: SimPoint,
    destination: SimPoint,
    impact_radius: i32,
    launch_tick: u64,
    impact_tick: u64,
}

struct DamageTargetState<'a> {
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
    defense_query: DefenseTargetSearch,
}

impl TargetDecision {
    const fn without_defense(target: Option<SimId>, direct_retaliation_lock: bool) -> Self {
        Self {
            target,
            direct_retaliation_lock,
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
        defense_query: DefenseTargetSearch,
    ) -> Self {
        Self {
            target,
            direct_retaliation_lock,
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

fn validate_unit_template(unit: crate::components::UnitTemplate) {
    assert!(unit.health > 0);
    assert!(unit.attack.damage >= 0);
    assert!(unit.attack.range >= 0);
    assert!(unit.attack.acquisition_range >= unit.attack.range);
    match unit.attack.delivery {
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
    }
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
    let projectile = *entity.get::<BallisticProjectile>()?;
    Some(ProjectileView {
        id,
        source: projectile.source,
        launch_position: projectile.launch_position,
        launch_tick: projectile.launch_tick,
        impact_tick: projectile.impact_tick,
        kind: ProjectileViewKind::Ballistic {
            destination: projectile.destination,
            impact_radius: projectile.impact_radius,
        },
    })
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

fn apply_damage_to_target(
    target: TargetIndex,
    source_id: SimId,
    damage: i32,
    completed_tick: u64,
    state: DamageTargetState<'_>,
) -> Option<SimPoint> {
    match target {
        TargetIndex::Unit(index) => {
            if state.unit_health[index] <= 0 {
                return None;
            }
            state.unit_health[index] = state.unit_health[index]
                .checked_sub(damage)
                .expect("unit damage arithmetic overflowed validated bounds");
            state.attackers_this_tick[index].get_or_insert(source_id);
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
            state.building_health[index] = state.building_health[index]
                .checked_sub(damage)
                .expect("building damage arithmetic overflowed validated bounds");
            Some(footprint_center_point(
                state.buildings[index].footprint,
                state.navigation_cell_size,
            ))
        }
    }
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

fn perpendicular_step(id: SimId, from: SimPoint, toward: SimPoint, distance: i32) -> SimPoint {
    let dx = i64::from(toward.x) - i64::from(from.x);
    let dy = i64::from(toward.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return SimPoint::new(0, sidestep_sign(id) * distance);
    }
    let side = i64::from(sidestep_sign(id));
    let raw = SimPoint::new(
        i32::try_from(-dy * side).expect("sidestep x overflow"),
        i32::try_from(dx * side).expect("sidestep y overflow"),
    );
    SimPoint::new(0, 0).step_towards(raw, distance)
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
            if let Some(projectile) = entity.get::<GuaranteedHitProjectile>() {
                return Some(CanonicalEntity::Projectile(CanonicalProjectile {
                    id,
                    projectile: *projectile,
                }));
            }
            if let Some(projectile) = entity.get::<BallisticProjectile>() {
                return Some(CanonicalEntity::BallisticProjectile(
                    CanonicalBallisticProjectile {
                        id,
                        projectile: *projectile,
                    },
                ));
            }
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
                hash_attack_delivery(&mut hash, unit.attack.delivery);
                hash.write_i32(unit.attack.damage);
                hash.write_i32(unit.attack.range);
                hash.write_i32(unit.attack.acquisition_range);
                hash.write_u16(unit.attack.cooldown_ticks);
                hash.write_i32(unit.movement.speed_per_tick);
                hash.write_u16(unit.cooldown.remaining);
                hash.write_u64(unit.target.current.map_or(0, |target| target.0));
                hash.write_u8(u8::from(unit.target.direct_retaliation_lock));
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
                } else {
                    hash.write_u8(0);
                }
            }
            CanonicalEntity::Projectile(projectile) => {
                hash.write_u8(2);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::BallisticProjectile(projectile) => {
                hash.write_u8(3);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_i32(projectile.projectile.destination.x);
                hash.write_i32(projectile.projectile.destination.y);
                hash.write_i32(projectile.projectile.impact_radius);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
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
    Projectile(CanonicalProjectile),
    BallisticProjectile(CanonicalBallisticProjectile),
}

impl CanonicalEntity {
    const fn id(&self) -> SimId {
        match self {
            Self::Unit(unit) => unit.id,
            Self::Building(building) => building.id,
            Self::Projectile(projectile) => projectile.id,
            Self::BallisticProjectile(projectile) => projectile.id,
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

#[derive(Debug, Clone, Copy)]
struct CanonicalProjectile {
    id: SimId,
    projectile: GuaranteedHitProjectile,
}

#[derive(Debug, Clone, Copy)]
struct CanonicalBallisticProjectile {
    id: SimId,
    projectile: BallisticProjectile,
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
