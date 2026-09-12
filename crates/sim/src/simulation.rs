use bevy_ecs::{entity::Entity, prelude::World};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use crate::{
    components::{
        AttackCooldown, AttackProfile, Health, MovementProfile, Position, SimId, SpawnTick,
        TargetState, Team, UnitSpawn,
    },
    math::SimPoint,
    spatial::SpatialGrid,
};

#[derive(Debug, Clone, Copy)]
pub struct SimulationConfig {
    pub spatial_cell_size: i32,
    pub team_objective_x: [i32; 2],
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            spatial_cell_size: 8 * crate::math::SUBUNITS_PER_WORLD_UNIT,
            team_objective_x: [120 * crate::math::SUBUNITS_PER_WORLD_UNIT, 0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickResult {
    pub completed_tick: u64,
    pub units_alive: usize,
    pub attacks_resolved: usize,
    pub deaths: usize,
    pub checksum: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub health: i32,
    pub target: Option<SimId>,
    pub cooldown_remaining: u16,
}

pub struct Simulation {
    world: World,
    config: SimulationConfig,
    pool: ThreadPool,
    next_tick: u64,
    next_id: u64,
}

impl Simulation {
    pub fn new(config: SimulationConfig, workers: usize) -> Self {
        assert!(workers > 0, "simulation requires at least one worker");
        assert!(config.spatial_cell_size > 0);

        let pool = ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("castle-sim-{index}"))
            .build()
            .expect("failed to create simulation worker pool");

        Self {
            world: World::new(),
            config,
            pool,
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
        assert!(unit.health > 0);
        assert!(unit.attack.damage >= 0);
        assert!(unit.attack.range >= 0);
        assert!(unit.attack.acquisition_range >= unit.attack.range);
        assert!(unit.movement.speed_per_tick >= 0);
        assert!(
            unit.team.0 < 2,
            "first verification slice supports two teams"
        );

        let id = SimId(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("SimId space exhausted");

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
            unit.movement,
            SpawnTick(self.next_tick),
        ));

        id
    }

    pub fn step(&mut self) -> TickResult {
        let completed_tick = self.next_tick;
        self.advance_cooldowns();

        let mut units = self.snapshot_units();
        let grid = SpatialGrid::build(
            self.config.spatial_cell_size,
            units
                .iter()
                .enumerate()
                .map(|(index, unit)| (index, unit.position)),
        );

        let choices = self.select_targets(&units, &grid);
        for (unit, target) in units.iter_mut().zip(choices) {
            unit.target = target;
        }

        let mut health: Vec<i32> = units.iter().map(|unit| unit.health).collect();
        let mut cooldowns: Vec<u16> = units.iter().map(|unit| unit.cooldown_remaining).collect();
        let mut positions: Vec<SimPoint> = units.iter().map(|unit| unit.position).collect();

        let mut intents = self.attack_intents(&units);
        intents.sort_unstable_by_key(|intent| (intent.source_id, intent.target_id));

        let mut attacks_resolved = 0;
        for intent in intents {
            if health[intent.source_index] <= 0 || health[intent.target_index] <= 0 {
                continue;
            }

            health[intent.target_index] = health[intent.target_index]
                .checked_sub(intent.damage)
                .expect("damage arithmetic overflowed validated combat bounds");
            cooldowns[intent.source_index] = intent.cooldown_ticks;
            attacks_resolved += 1;
        }

        self.resolve_movement(&units, &health, &mut positions);

        let mut deaths = 0;
        for (index, unit) in units.iter().enumerate() {
            if health[index] <= 0 {
                self.world.despawn(unit.entity);
                deaths += 1;
                continue;
            }

            let live_target = unit.target.filter(|target| {
                find_index_by_id(&units, *target)
                    .is_some_and(|target_index| health[target_index] > 0)
            });

            let mut entity = self.world.entity_mut(unit.entity);
            entity
                .get_mut::<Health>()
                .expect("unit health missing")
                .current = health[index];
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
        }

        self.next_tick = self
            .next_tick
            .checked_add(1)
            .expect("tick counter exhausted");
        let checksum = canonical_checksum(&self.world, self.next_tick);

        TickResult {
            completed_tick,
            units_alive: units.len() - deaths,
            attacks_resolved,
            deaths,
            checksum,
        }
    }

    #[must_use]
    pub fn checksum(&self) -> u64 {
        canonical_checksum(&self.world, self.next_tick)
    }

    #[must_use]
    pub fn unit_count(&self) -> usize {
        self.world
            .iter_entities()
            .filter(|entity| entity.get::<SimId>().is_some())
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
    pub fn unit(&self, id: SimId) -> Option<UnitView> {
        self.world
            .iter_entities()
            .filter_map(unit_view_from_entity)
            .find(|unit| unit.id == id)
    }

    fn advance_cooldowns(&mut self) {
        let mut query = self.world.query::<&mut AttackCooldown>();
        for mut cooldown in query.iter_mut(&mut self.world) {
            cooldown.remaining = cooldown.remaining.saturating_sub(1);
        }
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
            &MovementProfile,
            &SpawnTick,
        )>();

        let mut units: Vec<UnitSnapshot> = query
            .iter(&self.world)
            .filter(|(_, _, _, _, health, _, _, _, _, _)| health.current > 0)
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
                    movement,
                    spawn_tick,
                )| {
                    UnitSnapshot {
                        entity,
                        id: *id,
                        team: *team,
                        position: position.0,
                        health: health.current,
                        attack: *attack,
                        cooldown_remaining: cooldown.remaining,
                        target: target.current,
                        movement: *movement,
                        spawn_tick: spawn_tick.0,
                    }
                },
            )
            .collect();

        units.sort_unstable_by_key(|unit| unit.id);
        units
    }

    fn select_targets(&self, units: &[UnitSnapshot], grid: &SpatialGrid) -> Vec<Option<SimId>> {
        self.pool.install(|| {
            units
                .par_iter()
                .map(|unit| {
                    if let Some(current) = unit.target
                        && let Some(index) = find_index_by_id(units, current)
                        && is_valid_target(unit, &units[index])
                        && unit.position.distance_sq(units[index].position)
                            <= unit.attack.acquisition_range_sq()
                    {
                        return Some(current);
                    }

                    let mut best: Option<(u64, SimId)> = None;
                    grid.for_each_candidate(
                        unit.position,
                        unit.attack.acquisition_range,
                        |index| {
                            let candidate = &units[index];
                            if !is_valid_target(unit, candidate) {
                                return;
                            }

                            let distance_sq = unit.position.distance_sq(candidate.position);
                            if distance_sq > unit.attack.acquisition_range_sq() {
                                return;
                            }

                            let key = (distance_sq, candidate.id);
                            if best.is_none_or(|current| key < current) {
                                best = Some(key);
                            }
                        },
                    );
                    best.map(|(_, id)| id)
                })
                .collect()
        })
    }

    fn attack_intents(&self, units: &[UnitSnapshot]) -> Vec<AttackIntent> {
        self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    if source.spawn_tick == self.next_tick || source.cooldown_remaining != 0 {
                        return None;
                    }

                    let target_id = source.target?;
                    let target_index = find_index_by_id(units, target_id)?;
                    let target = &units[target_index];
                    if source.position.distance_sq(target.position) > source.attack.range_sq() {
                        return None;
                    }

                    Some(AttackIntent {
                        source_index,
                        target_index,
                        source_id: source.id,
                        target_id,
                        damage: source.attack.damage,
                        cooldown_ticks: source.attack.cooldown_ticks,
                    })
                })
                .collect()
        })
    }

    fn resolve_movement(&self, units: &[UnitSnapshot], health: &[i32], positions: &mut [SimPoint]) {
        for (index, unit) in units.iter().enumerate() {
            if health[index] <= 0 || unit.movement.speed_per_tick == 0 {
                continue;
            }

            if let Some(target_id) = unit.target
                && let Some(target_index) = find_index_by_id(units, target_id)
                && health[target_index] > 0
            {
                let target_position = positions[target_index];
                if positions[index].distance_sq(target_position) > unit.attack.range_sq() {
                    positions[index] = positions[index]
                        .step_towards(target_position, unit.movement.speed_per_tick);
                }
                continue;
            }

            let objective_x = self.config.team_objective_x[usize::from(unit.team.0)];
            positions[index] = positions[index].step_towards(
                SimPoint::new(objective_x, positions[index].y),
                unit.movement.speed_per_tick,
            );
        }
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
    movement: MovementProfile,
    spawn_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct AttackIntent {
    source_index: usize,
    target_index: usize,
    source_id: SimId,
    target_id: SimId,
    damage: i32,
    cooldown_ticks: u16,
}

fn unit_view_from_entity(entity: bevy_ecs::world::EntityRef<'_>) -> Option<UnitView> {
    Some(UnitView {
        id: *entity.get::<SimId>()?,
        team: *entity.get::<Team>()?,
        position: entity.get::<Position>()?.0,
        health: entity.get::<Health>()?.current,
        target: entity.get::<TargetState>()?.current,
        cooldown_remaining: entity.get::<AttackCooldown>()?.remaining,
    })
}

fn is_valid_target(source: &UnitSnapshot, target: &UnitSnapshot) -> bool {
    source.id != target.id && source.team != target.team && target.health > 0
}

fn find_index_by_id(units: &[UnitSnapshot], id: SimId) -> Option<usize> {
    units.binary_search_by_key(&id, |unit| unit.id).ok()
}

fn canonical_checksum(world: &World, next_tick: u64) -> u64 {
    let mut units: Vec<CanonicalUnit> = world
        .iter_entities()
        .filter_map(|entity| {
            Some(CanonicalUnit {
                id: *entity.get::<SimId>()?,
                team: *entity.get::<Team>()?,
                position: entity.get::<Position>()?.0,
                health: *entity.get::<Health>()?,
                cooldown: *entity.get::<AttackCooldown>()?,
                target: *entity.get::<TargetState>()?,
                spawn_tick: *entity.get::<SpawnTick>()?,
            })
        })
        .collect();
    units.sort_unstable_by_key(|unit| unit.id);

    let mut hash = Fnv64::new();
    hash.write_u64(next_tick);
    hash.write_u64(units.len() as u64);
    for unit in units {
        hash.write_u64(unit.id.0);
        hash.write_u8(unit.team.0);
        hash.write_i32(unit.position.x);
        hash.write_i32(unit.position.y);
        hash.write_i32(unit.health.current);
        hash.write_i32(unit.health.max);
        hash.write_u16(unit.cooldown.remaining);
        hash.write_u64(unit.target.current.map_or(0, |target| target.0));
        hash.write_u64(unit.spawn_tick.0);
    }
    hash.finish()
}

#[derive(Debug, Clone, Copy)]
struct CanonicalUnit {
    id: SimId,
    team: Team,
    position: SimPoint,
    health: Health,
    cooldown: AttackCooldown,
    target: TargetState,
    spawn_tick: SpawnTick,
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
