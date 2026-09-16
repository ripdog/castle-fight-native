use super::*;

/// Logical authoritative snapshot schema. This is intentionally independent of Bevy entity handles
/// and storage order; wire encoding/versioning is layered on top of this logical representation.
pub const AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct SimulationSnapshot {
    schema_version: u32,
    configuration_identity: u64,
    completed_tick: Option<u64>,
    next_tick: u64,
    next_id: u64,
    players: Vec<PlayerState>,
    lifecycle: MatchLifecycle,
    team_objectives: [Option<SimId>; 2],
    defense_alerts: Vec<DefenseAlert>,
    entities: Vec<CanonicalEntity>,
    checksum: u64,
}

impl SimulationSnapshot {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn completed_tick(&self) -> Option<u64> {
        self.completed_tick
    }

    #[must_use]
    pub const fn next_tick(&self) -> u64 {
        self.next_tick
    }

    #[must_use]
    pub const fn checksum(&self) -> u64 {
        self.checksum
    }

    #[must_use]
    pub const fn configuration_identity(&self) -> u64 {
        self.configuration_identity
    }

    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRestoreError {
    SchemaMismatch {
        expected: u32,
        actual: u32,
    },
    ConfigurationMismatch {
        expected: u64,
        actual: u64,
    },
    TickBoundaryMismatch,
    PlayerIdentityMismatch,
    DuplicateEntityId(SimId),
    AllocatorBehindEntityIds {
        next_id: u64,
        maximum_live_id: SimId,
    },
    ChecksumMismatch {
        expected: u64,
        actual: u64,
    },
}

impl Simulation {
    /// Captures a completed-boundary logical snapshot. Presentation events, worker-pool state, and
    /// rebuildable navigation caches are deliberately excluded.
    #[must_use]
    pub fn capture_snapshot(&self) -> SimulationSnapshot {
        SimulationSnapshot {
            schema_version: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
            configuration_identity: self.configuration_identity,
            completed_tick: self.next_tick.checked_sub(1),
            next_tick: self.next_tick,
            next_id: self.next_id,
            players: self.players.clone(),
            lifecycle: self.lifecycle,
            team_objectives: self.team_objectives,
            defense_alerts: self.defense_alerts.clone(),
            entities: canonical_entities(&self.world),
            checksum: self.checksum(),
        }
    }

    /// Replaces mutable authoritative state from a snapshot while retaining this instance's
    /// immutable match/content configuration and execution worker pool.
    pub fn restore_snapshot(
        &mut self,
        snapshot: &SimulationSnapshot,
    ) -> Result<(), SnapshotRestoreError> {
        if snapshot.schema_version != AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION {
            return Err(SnapshotRestoreError::SchemaMismatch {
                expected: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
                actual: snapshot.schema_version,
            });
        }
        if snapshot.configuration_identity != self.configuration_identity {
            return Err(SnapshotRestoreError::ConfigurationMismatch {
                expected: self.configuration_identity,
                actual: snapshot.configuration_identity,
            });
        }
        if snapshot.completed_tick != snapshot.next_tick.checked_sub(1) {
            return Err(SnapshotRestoreError::TickBoundaryMismatch);
        }
        if snapshot.players.len() != self.players.len()
            || snapshot.players.iter().zip(&self.players).any(
                |(snapshot_player, current_player)| {
                    snapshot_player.id != current_player.id
                        || snapshot_player.team != current_player.team
                },
            )
        {
            return Err(SnapshotRestoreError::PlayerIdentityMismatch);
        }

        for pair in snapshot.entities.windows(2) {
            if pair[0].id() == pair[1].id() {
                return Err(SnapshotRestoreError::DuplicateEntityId(pair[0].id()));
            }
        }
        if let Some(maximum_live_id) = snapshot.entities.last().map(CanonicalEntity::id)
            && snapshot.next_id <= maximum_live_id.0
        {
            return Err(SnapshotRestoreError::AllocatorBehindEntityIds {
                next_id: snapshot.next_id,
                maximum_live_id,
            });
        }

        let mut restored_world = World::new();
        restore_entities(&mut restored_world, &snapshot.entities);
        let restored_checksum = canonical_checksum(
            &restored_world,
            CanonicalMatchState {
                next_tick: snapshot.next_tick,
                next_id: snapshot.next_id,
                configuration_identity: snapshot.configuration_identity,
                defense_alerts: &snapshot.defense_alerts,
                players: &snapshot.players,
                lifecycle: snapshot.lifecycle,
                team_objectives: snapshot.team_objectives,
            },
        );
        if restored_checksum != snapshot.checksum {
            return Err(SnapshotRestoreError::ChecksumMismatch {
                expected: snapshot.checksum,
                actual: restored_checksum,
            });
        }

        self.world = restored_world;
        self.players.clone_from(&snapshot.players);
        self.lifecycle = snapshot.lifecycle;
        self.team_objectives = snapshot.team_objectives;
        self.defense_alerts.clone_from(&snapshot.defense_alerts);
        self.next_tick = snapshot.next_tick;
        self.next_id = snapshot.next_id;
        self.clear_presentation_events();
        self.pursuit_cache.clear();
        self.radius_objective_fields.clear();
        self.topology_dirty = true;
        self.refresh_topology_if_dirty();
        debug_assert_eq!(self.checksum(), snapshot.checksum);
        Ok(())
    }
}

pub(super) fn canonical_entities(world: &World) -> Vec<CanonicalEntity> {
    let authoritative_entity_count = world
        .iter_entities()
        .filter(|entity| entity.get::<SimId>().is_some())
        .count();
    let mut entities: Vec<_> = world
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
                    owner: entity.get::<Owner>()?.0,
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
                    owner: entity.get::<Owner>()?.0,
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
                    owner: entity.get::<Owner>().map(|owner| owner.0),
                    team,
                    footprint: *entity.get::<BuildingFootprint>()?,
                    health,
                    construction: entity.get::<BuildingConstruction>().copied().map(Box::new),
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
        "canonical state omitted an entity with an invalid authoritative component shape"
    );
    entities.sort_unstable_by_key(CanonicalEntity::id);
    for pair in entities.windows(2) {
        assert_ne!(
            pair[0].id(),
            pair[1].id(),
            "canonical state contains duplicate SimIds"
        );
    }
    entities
}

fn restore_entities(world: &mut World, entities: &[CanonicalEntity]) {
    // Deliberately rebuild in reverse canonical order. Restoration must never depend on Bevy
    // insertion/archetype order, and this continuously exercises that invariant.
    for entity in entities.iter().rev() {
        match entity {
            CanonicalEntity::Unit(unit) => {
                let mut entity = world.spawn((
                    unit.id,
                    Owner(unit.owner),
                    unit.team,
                    Position(unit.position),
                    unit.health,
                    unit.health_regeneration,
                    unit.attack,
                    unit.attack_targets,
                    unit.damage_type,
                    unit.armor,
                ));
                entity.insert((
                    unit.passive_effects,
                    unit.movement_class,
                    unit.movement,
                    unit.cooldown,
                    unit.attack_sequence,
                    unit.target,
                    unit.retaliation,
                    unit.status,
                    unit.navigation,
                    unit.spawn_tick,
                ));
                if let Some(content) = unit.content {
                    entity.insert(content);
                }
                if unit.mechanical {
                    entity.insert(MechanicalUnit);
                }
                if let Some(ticks) = unit.build_time_ticks {
                    entity.insert(BuildTimeTicks(ticks));
                }
                if let Some(ticks) = unit.repair_time_ticks {
                    entity.insert(RepairTimeTicks(ticks));
                }
                if let Some(corpse) = unit.corpse {
                    entity.insert(CorpseProducer(corpse));
                }
                if let Some(radius) = unit.collision_radius {
                    entity.insert(radius);
                }
                if let Some(spellcasting) = unit.spellcasting {
                    entity.insert(spellcasting);
                }
                if let Some(mana) = unit.mana {
                    entity.insert(mana);
                }
                if let Some(state) = unit.ability_state {
                    entity.insert(state);
                }
            }
            CanonicalEntity::Building(building) => {
                let mut entity = world.spawn((
                    building.id,
                    building.team,
                    building.footprint,
                    building.health,
                    building.damage_type,
                    building.armor,
                ));
                if let Some(owner) = building.owner {
                    entity.insert(Owner(owner));
                }
                if let Some(content) = building.content {
                    entity.insert(content);
                }
                if let Some(construction) = building.construction.as_deref() {
                    entity.insert(*construction);
                }
                if let Some(economy) = building.economy {
                    entity.insert(economy);
                }
                if let Some(ticks) = building.repair_time_ticks {
                    entity.insert(RepairTimeTicks(ticks));
                }
                if let Some(production) = building.production {
                    entity.insert(production);
                }
                if let Some(state) = building.production_state {
                    entity.insert(state);
                }
                if let Some(content) = building.production_content {
                    entity.insert(ProductionContentIdentity(content));
                }
                if let Some(corpse) = building.production_corpse {
                    entity.insert(ProductionCorpseProfile(corpse));
                }
                if let Some(radius) = building.production_collision_radius {
                    entity.insert(ProductionCollisionRadius(radius));
                }
                if let Some(class) = building.production_movement_class {
                    entity.insert(ProductionMovementClass(class));
                }
                if let Some(metadata) = building.production_repair_metadata {
                    entity.insert(metadata);
                }
                if let Some(targets) = building.production_attack_targets {
                    entity.insert(ProductionAttackTargets(targets));
                }
                if let Some(regeneration) = building.production_health_regen_per_second_per_10k {
                    entity.insert(ProductionHealthRegeneration(regeneration));
                }
                if let Some(damage_type) = building.production_damage_type {
                    entity.insert(ProductionDamageType(damage_type));
                }
                if let Some(armor) = building.production_armor {
                    entity.insert(ProductionArmorProfile(armor));
                }
                if let Some(effects) = building.production_passive_effects {
                    entity.insert(ProductionPassiveEffects(effects));
                }
                if let Some(spellcasting) = building.production_spellcasting {
                    entity.insert(ProductionSpellcastingProfile(spellcasting));
                }
                if let Some(attack) = building.attack {
                    entity.insert(attack);
                }
                if let Some(targets) = building.attack_targets {
                    entity.insert(targets);
                }
                if let Some(cooldown) = building.cooldown {
                    entity.insert(cooldown);
                }
                if let Some(target) = building.target {
                    entity.insert(target);
                }
                if let Some(spawn_tick) = building.spawn_tick {
                    entity.insert(spawn_tick);
                }
                if let Some(spellcasting) = building.spellcasting {
                    entity.insert(spellcasting);
                }
                if let Some(mana) = building.mana {
                    entity.insert(mana);
                }
                if let Some(state) = building.ability_state {
                    entity.insert(state);
                }
                if let Some(status) = building.status {
                    entity.insert(status);
                }
            }
            CanonicalEntity::Projectile(projectile) => {
                world.spawn((projectile.id, projectile.projectile));
            }
            CanonicalEntity::ReflectedProjectile(projectile) => {
                world.spawn((projectile.id, projectile.projectile));
            }
            CanonicalEntity::BallisticProjectile(projectile) => {
                world.spawn((projectile.id, projectile.projectile));
            }
            CanonicalEntity::BounceProjectile(projectile) => {
                world.spawn((projectile.id, projectile.projectile));
            }
            CanonicalEntity::Corpse(corpse) => {
                world.spawn((corpse.id, Position(corpse.position), corpse.corpse));
            }
            CanonicalEntity::BurningOil(zone) => {
                world.spawn((zone.id, zone.zone));
            }
            CanonicalEntity::ChainLightning(chain) => {
                world.spawn((chain.id, chain.state));
            }
            CanonicalEntity::Builder(builder) => {
                let mut entity = world.spawn((
                    builder.id,
                    Owner(builder.owner),
                    builder.team,
                    Position(builder.position),
                    Builder,
                    builder.profile,
                    builder.configuration.clone(),
                    builder.state,
                ));
                if let Some(order) = builder.build_order {
                    entity.insert(order);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::TimedMovementModifier;
    use crate::{BurningOilEffectProfile, ChainLightningEffectProfile, ManaProfile};

    #[test]
    fn initial_snapshot_restores_pre_tick_boundary_across_worker_counts() {
        let config = SimulationConfig::default();
        let original = Simulation::new(config.clone(), 1);
        let snapshot = original.capture_snapshot();
        assert_eq!(snapshot.completed_tick(), None);
        assert_eq!(snapshot.checksum(), original.checksum());

        let mut restored = Simulation::new(config, 3);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(restored.tick(), 0);
        assert_eq!(restored.checksum(), original.checksum());
        assert_eq!(restored.worker_count(), 3);
    }

    #[test]
    fn snapshot_restores_construction_projectiles_allocator_and_continuation() {
        let config = SimulationConfig::default();
        let mut original = Simulation::new(config.clone(), 1);

        let construction = BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(8, 0, 2, 2),
            health: 500,
            production: None,
            attack: None,
            spellcasting: None,
        };
        let construction_id = original
            .try_start_building_construction(
                PlayerId(0),
                construction,
                BuildingGameplayProperties {
                    construction_time_ticks: Some(6),
                    ..BuildingGameplayProperties::default()
                },
            )
            .unwrap();

        let discarded = original.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(14, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        });
        assert!(original.remove_building(discarded));

        let ranged_attack = AttackProfile {
            delivery: AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            },
            damage: 7,
            range: 8 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 10,
        };
        let mut combatants = Vec::new();
        for (team, x) in [(Team(0), 20), (Team(1), 24)] {
            combatants.push(original.spawn_unit(UnitSpawn {
                team,
                position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: ranged_attack,
                movement: MovementProfile { speed_per_tick: 0 },
            }));
        }

        for _ in 0..3 {
            original.step();
            if original.projectile_count() > 0 {
                break;
            }
        }
        assert!(original.projectile_count() > 0);
        let stunned_until_tick = original.tick().checked_add(9).unwrap();
        let stunned_entity = original
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(combatants[0]))
            .unwrap()
            .id();
        original
            .world
            .entity_mut(stunned_entity)
            .get_mut::<StatusState>()
            .unwrap()
            .stunned_until_tick = stunned_until_tick;
        let snapshot = original.capture_snapshot();
        assert_eq!(snapshot.completed_tick(), original.tick().checked_sub(1));

        let mut restored = Simulation::new(config, 4);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(restored.checksum(), original.checksum());
        assert_eq!(restored.projectile_count(), original.projectile_count());
        assert_eq!(
            restored.unit(combatants[0]).unwrap().stunned_until_tick,
            stunned_until_tick
        );
        assert!(restored.attacks_last_tick().is_empty());
        assert!(restored.ability_casts_last_tick().is_empty());
        assert!(restored.chain_lightnings_last_tick().is_empty());
        let construction_view = restored.building(construction_id).unwrap();
        assert_eq!(construction_view.construction_started_tick, Some(0));
        assert_eq!(construction_view.construction_complete_tick, Some(6));

        let allocator_probe = UnitSpawn {
            team: Team(0),
            position: SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 10,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        assert_eq!(
            original.spawn_unit(allocator_probe),
            restored.spawn_unit(allocator_probe),
            "restored allocator history must preserve the next SimId"
        );

        for _ in 0..20 {
            let original_tick = original.step();
            let restored_tick = restored.step();
            assert_eq!(original_tick.checksum, restored_tick.checksum);
        }
    }

    #[test]
    fn snapshot_restores_effect_histories_and_fractional_state_exactly() {
        let config = SimulationConfig::default();
        let mut original = Simulation::new(config.clone(), 1);
        let inert_attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 0,
            range: SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 10,
        };
        let source = original.spawn_unit_with_spellcasting(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: inert_attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 100,
                    starting: 50,
                    regen_per_tick_per_10k: 1_234,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(0x5153_4e50),
                    mana_cost: 10,
                    cooldown_ticks: 7,
                    range: 4 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 1 },
                },
            },
        );
        let target_a = original.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(24 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: inert_attack,
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target_b = original.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(26 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: inert_attack,
            movement: MovementProfile { speed_per_tick: 0 },
        });

        let source_entity = original
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(source))
            .unwrap()
            .id();
        {
            let mut source_entity = original.world.entity_mut(source_entity);
            *source_entity.get_mut::<ManaState>().unwrap() = ManaState {
                current: 37,
                regen_remainder_per_10k: 4_567,
            };
            *source_entity.get_mut::<AutomaticAbilityState>().unwrap() = AutomaticAbilityState {
                ready_tick: 91,
                cast_sequence: 12,
            };
            let mut status = source_entity.get_mut::<StatusState>().unwrap();
            status.movement_modifiers[0] = TimedMovementModifier {
                id: ModifierId(7),
                percent_delta: -25,
                expires_tick: 77,
            };
            status.movement_modifier_count = 1;
            status.damage_over_time[0] = TimedDamageOverTime {
                id: ModifierId(8),
                damage_per_pulse: 3,
                pulse_interval_ticks: 20,
                next_pulse_tick: 44,
                expires_tick: 88,
            };
            status.damage_over_time_count = 1;
        }

        let builder = original.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(4 * SUBUNITS_PER_WORLD_UNIT, 0),
            profile: BuilderProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
                build_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                repair_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                repair_autocast_range: 4 * SUBUNITS_PER_WORLD_UNIT,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 2,
                full_repair_duration_ticks: 40,
                blink_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                blink_boundary_inset: SUBUNITS_PER_WORLD_UNIT,
            },
            configuration: BuilderConfiguration {
                appearance: ContentIdentity {
                    rawcode: 0x6253_4e50,
                    name: "snapshot-builder",
                },
                locomotion: BuilderLocomotion::Foot,
                build_catalog: vec![0x6830_3031],
            },
            repair_autocast_enabled: true,
        });
        let builder_entity = original
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(builder))
            .unwrap()
            .id();
        original
            .world
            .entity_mut(builder_entity)
            .get_mut::<BuilderState>()
            .unwrap()
            .repair_progress_remainder = 17;

        let mut bounce_hits = [SimId(0); MAX_BOUNCE_HITS];
        bounce_hits[0] = target_a;
        bounce_hits[1] = target_b;
        let bounce_id = original.allocate_id();
        original.world.spawn((
            bounce_id,
            BounceProjectile {
                source,
                source_team: Team(0),
                source_is_building: false,
                target_mask: AttackTargetMask::ALL,
                target: target_b,
                damage: 31,
                damage_type: DamageType::Magic,
                launch_position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
                launch_tick: 3,
                impact_tick: 50,
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                bounce_range: 6 * SUBUNITS_PER_WORLD_UNIT,
                remaining_bounces: 2,
                bounce_index: 1,
                damage_percent_per_bounce: 75,
                allow_repeat_targets: false,
                hit_targets: bounce_hits,
                hit_count: 2,
            },
        ));

        let burning_profile = BurningOilEffectProfile {
            ability: AbilityId(0x424f_494c),
            radius: 3 * SUBUNITS_PER_WORLD_UNIT,
            full_damage: 8,
            full_interval_millis: 500,
            half_damage: 4,
            half_interval_millis: 500,
            full_duration_millis: 1_500,
            total_duration_millis: 3_000,
            target_ground_units: true,
            target_buildings: true,
        };
        let burning_id = original.allocate_id();
        original.world.spawn((
            burning_id,
            BurningOilZone {
                source,
                source_team: Team(0),
                center: SimPoint::new(25 * SUBUNITS_PER_WORLD_UNIT, 0),
                profile: burning_profile,
                created_tick: 6,
                pulse_index: 3,
            },
        ));

        let chain_profile = ChainLightningEffectProfile {
            ability: AbilityId(0x4348_4149),
            initial_damage: 40,
            maximum_targets: 5,
            jump_radius: 7 * SUBUNITS_PER_WORLD_UNIT,
            damage_reduction_per_10k: 1_500,
            targets: AttackTargetMask::AIR_AND_GROUND,
        };
        let mut chain_hits = [SimId(0); MAX_BOUNCE_HITS];
        chain_hits[0] = target_a;
        chain_hits[1] = target_b;
        let chain_id = original.allocate_id();
        original.world.spawn((
            chain_id,
            ChainLightningState {
                source,
                source_team: Team(0),
                profile: chain_profile,
                started_tick: 8,
                next_jump_index: 2,
                current_target: target_b,
                last_position: SimPoint::new(26 * SUBUNITS_PER_WORLD_UNIT, 0),
                next_damage: 29,
                hit_targets: chain_hits,
                hit_count: 2,
            },
        ));

        let snapshot = original.capture_snapshot();
        let mut restored = Simulation::new(config, 4);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(restored.checksum(), original.checksum());

        let restored_source = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(source))
            .unwrap();
        assert_eq!(
            restored_source.get::<ManaState>(),
            original.world.entity(source_entity).get::<ManaState>()
        );
        assert_eq!(
            restored_source.get::<AutomaticAbilityState>(),
            original
                .world
                .entity(source_entity)
                .get::<AutomaticAbilityState>()
        );
        assert_eq!(
            restored_source.get::<StatusState>(),
            original.world.entity(source_entity).get::<StatusState>()
        );

        let restored_builder = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(builder))
            .unwrap();
        assert_eq!(
            restored_builder
                .get::<BuilderState>()
                .unwrap()
                .repair_progress_remainder,
            17
        );

        let restored_bounce = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(bounce_id))
            .unwrap();
        assert_eq!(
            restored_bounce.get::<BounceProjectile>(),
            original
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>().copied() == Some(bounce_id))
                .unwrap()
                .get::<BounceProjectile>()
        );
        let restored_burning = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(burning_id))
            .unwrap();
        assert_eq!(
            restored_burning.get::<BurningOilZone>(),
            original
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>().copied() == Some(burning_id))
                .unwrap()
                .get::<BurningOilZone>()
        );
        let restored_chain = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(chain_id))
            .unwrap();
        assert_eq!(
            restored_chain.get::<ChainLightningState>(),
            original
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>().copied() == Some(chain_id))
                .unwrap()
                .get::<ChainLightningState>()
        );
    }
}
