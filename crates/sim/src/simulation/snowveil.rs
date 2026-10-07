//! Script-owned snow terrain; first writer owns each tile until an explosion/round end.
use super::*;
use crate::building_mechanics::{SnowveilProfile, snowveil_for_version};
use bevy_ecs::prelude::Component;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub(super) struct SnowveilState {
    pub version: crate::MapVersion,
    pub tiles: BTreeMap<u32, Team>,
    pub manual_ready: BTreeMap<PlayerId, u64>,
}

fn tile(profile: &SnowveilProfile, point: SimPoint) -> Option<u32> {
    // Lua truncates the real coordinate before integer division (including negatives).
    let axis = |value: i32, origin: i32| {
        ((i64::from(value) - i64::from(origin) + i64::from(profile.tile_size / 2))
            / i64::from(SUBUNITS_PER_WORLD_UNIT))
            / i64::from(profile.tile_size / SUBUNITS_PER_WORLD_UNIT)
    };
    let x = axis(point.x, profile.origin.x);
    let y = axis(point.y, profile.origin.y);
    (x >= 0 && y >= 0 && x < i64::from(profile.width) && y < i64::from(profile.height))
        .then(|| (y * i64::from(profile.width) + x) as u32)
}

fn center(profile: &SnowveilProfile, id: u32) -> SimPoint {
    let x = id % profile.width as u32;
    let y = id / profile.width as u32;
    SimPoint::new(
        profile.origin.x + x as i32 * profile.tile_size,
        profile.origin.y + y as i32 * profile.tile_size,
    )
}

impl Simulation {
    fn snow_state(&self) -> Option<&SnowveilState> {
        self.world
            .iter_entities()
            .find_map(|e| e.get::<SnowveilState>())
    }

    fn snow_entity(&mut self, version: crate::MapVersion) -> Entity {
        let entity = self
            .world
            .iter_entities()
            .find_map(|e| e.contains::<SnowveilState>().then_some(e.id()));
        if let Some(entity) = entity {
            assert_eq!(
                self.world
                    .entity(entity)
                    .get::<SnowveilState>()
                    .unwrap()
                    .version,
                version
            );
            return entity;
        }
        let id = self.allocate_id();
        self.world
            .spawn((
                id,
                SnowveilState {
                    version,
                    tiles: BTreeMap::new(),
                    manual_ready: BTreeMap::new(),
                },
            ))
            .id()
    }

    pub(super) fn place_snow(&mut self, point: SimPoint, team: Team, version: crate::MapVersion) {
        let profile = snowveil_for_version(version);
        let entity = self.snow_entity(version);
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity.get_mut::<SnowveilState>().unwrap();
        for (x, y) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            let point = SimPoint::new(
                point.x + x * profile.tile_size,
                point.y + y * profile.tile_size,
            );
            if let Some(id) = tile(profile, point) {
                state.tiles.entry(id).or_insert(team);
            }
        }
    }

    pub(super) fn refresh_snow_protection(
        &self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        self.refresh_snow_protection_at(units, buildings, None);
    }

    pub(super) fn refresh_snow_protection_at(
        &self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        positions: Option<&[SimPoint]>,
    ) {
        let snow = self.snow_state();
        let factor = |point, team| {
            snow.map_or(10_000, |state| {
                let p = snowveil_for_version(state.version);
                if tile(p, point).and_then(|id| state.tiles.get(&id)) == Some(&team) {
                    p.damage_taken_per_10k
                } else {
                    10_000
                }
            })
        };
        for (i, unit) in units.iter_mut().enumerate() {
            unit.snow_damage_taken_per_10k = factor(
                positions.map_or(unit.position, |points| points[i]),
                unit.team,
            );
        }
        for building in buildings {
            building.snow_damage_taken_per_10k = factor(
                footprint_center_point(building.footprint, self.config.navigation_cell_size),
                building.team,
            );
        }
    }

    /// Cosmetic projection of authoritative marked tiles. The unmodified terrain stays retained.
    pub fn snow_tiles(&self) -> Vec<(SimPoint, Team, i32, u32)> {
        self.snow_state().map_or_else(Vec::new, |state| {
            let p = snowveil_for_version(state.version);
            state
                .tiles
                .iter()
                .map(|(id, team)| (center(p, *id), *team, p.tile_size, p.snow_terrain_rawcode))
                .collect()
        })
    }

    pub fn snowveil_manual_ready_tick(&self, owner: PlayerId) -> u64 {
        self.snow_state()
            .and_then(|s| s.manual_ready.get(&owner))
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn clear_snow(&mut self) {
        let entity = self
            .world
            .iter_entities()
            .find_map(|e| e.contains::<SnowveilState>().then_some(e.id()));
        if let Some(entity) = entity {
            self.world
                .entity_mut(entity)
                .get_mut::<SnowveilState>()
                .unwrap()
                .tiles
                .clear();
        }
    }

    pub fn cast_building_spell_at_for_player(
        &mut self,
        controller: PlayerId,
        building: SimId,
        point: SimPoint,
    ) -> Result<(), BuildingCommandError> {
        if !self.can_player_control_building(controller, building) {
            return Err(BuildingCommandError::NotAuthorized);
        }
        let source = self
            .building(building)
            .ok_or(BuildingCommandError::SourceNotFound)?;
        let content = source
            .content
            .ok_or(BuildingCommandError::SourceCannotCast)?;
        let p = snowveil_for_version(content.map_version);
        if content.rawcode != p.rawcode || source.construction_complete_tick.is_some() {
            return Err(BuildingCommandError::SourceCannotCast);
        }
        let owner = source.owner.ok_or(BuildingCommandError::NotAuthorized)?;
        let team = source.team;
        if self.snowveil_manual_ready_tick(owner) > self.next_tick {
            return Err(BuildingCommandError::AbilityOnCooldown);
        }
        let entity = self.snow_entity(content.map_version);
        let mut units = self.snapshot_units();
        let mut buildings = self.snapshot_buildings();
        self.refresh_snow_protection(&mut units, &mut buildings);
        let state = self.world.entity(entity).get::<SnowveilState>().unwrap();
        let mut damage_updates = Vec::new();
        for target in &mut units {
            if target.health <= 0
                || target.team == team
                || !target.classifications.combat_sapper
                || target.classifications.invulnerable
                || point.distance_sq(target.position) > square_i32(p.explosion_radius)
            {
                continue;
            }
            let Some(id) = tile(p, target.position) else {
                continue;
            };
            if state.tiles.contains_key(&id)
                && center(p, id).distance_sq(point) <= square_i32(p.explosion_radius)
            {
                // DAMAGE_TYPE_UNIVERSAL bypasses armor/Defend/spell resistance, but the global snow listener still applies.
                target.health = target.health.saturating_sub(scale_damage_per_10k(
                    p.explosion_damage,
                    target.snow_damage_taken_per_10k,
                ));
                damage_updates.push((target.entity, target.health));
            }
        }
        for (entity, health) in damage_updates {
            self.world
                .entity_mut(entity)
                .get_mut::<Health>()
                .unwrap()
                .current = health;
        }
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity.get_mut::<SnowveilState>().unwrap();
        state
            .tiles
            .retain(|id, _| center(p, *id).distance_sq(point) > square_i32(p.explosion_radius));
        state
            .manual_ready
            .insert(owner, self.next_tick + u64::from(p.manual_cooldown_ticks));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ManaProfile;
    pub(super) const VERSION: crate::MapVersion = crate::MapVersion::CASTLE_FIGHT_9_27;
    pub(super) fn sim(workers: usize) -> Simulation {
        Simulation::new(
            SimulationConfig {
                navigation_max: NavCell::new(500, 500),
                ..SimulationConfig::default()
            },
            workers,
        )
    }
    pub(super) fn unit(sim: &mut Simulation, point: SimPoint, team: Team, sapper: bool) -> SimId {
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team,
                position: point,
                health: 2000,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1000,
                    delivery: AttackDelivery::Melee,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                collision_radius: Some(CollisionRadius(0)),
                classifications: UnitClassifications {
                    combat_sapper: sapper,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
    }
    pub(super) fn fountain(sim: &mut Simulation, x: i32) -> SimId {
        let source = crate::CastleFightTowerKind::SnowveilFountain.definition();
        sim.spawn_building_with_properties(
            BuildingSpawn {
                team: Team(0),
                footprint: BuildingFootprint::new(x, 3, 1, 1),
                health: 2000,
                production: None,
                attack: None,
                spellcasting: None,
            },
            BuildingGameplayProperties {
                content: source.gameplay_properties().content,
                ..Default::default()
            },
        )
    }
    fn restored(sim: &Simulation) -> Simulation {
        let content = crate::castle_fight_content_bundle(VERSION).unwrap();
        let snapshot = SimulationSnapshot::decode_wire(
            &sim.capture_snapshot().encode_wire().unwrap(),
            content,
        )
        .unwrap();
        let mut other = Simulation::new(sim.config.clone(), 4);
        other.restore_snapshot(&snapshot).unwrap();
        other
    }
    #[test]
    fn snowfall_filters_sappers_but_can_place_on_heroes_and_native_immune_units() {
        let mut sim = sim(1);
        let point = SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0);
        let valid = unit(&mut sim, point, Team(1), true);
        unit(
            &mut sim,
            SimPoint::new(400 * SUBUNITS_PER_WORLD_UNIT, 0),
            Team(1),
            false,
        );
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&valid))
            .unwrap()
            .id();
        *sim.world
            .entity_mut(entity)
            .get_mut::<UnitClassifications>()
            .unwrap() = UnitClassifications {
            combat_sapper: true,
            hero: true,
            spell_immune: true,
            invulnerable: true,
            ..Default::default()
        };
        let source = sim.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(5, 5, 1, 1),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: Some(SpellcastingProfile {
                mana: ManaProfile::per_second(3, 3, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(123),
                    mana_cost: 2,
                    cooldown_ticks: 1000,
                    range: 0,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
                    effect: AbilityEffect::Snowfall {
                        map_version: VERSION,
                    },
                },
            }),
        });
        let mut other = restored(&sim);
        assert_eq!(sim.step().checksum, other.step().checksum);
        assert_eq!(sim.building(source).unwrap().mana_current, Some(1));
        assert_eq!(sim.snow_tiles().len(), 5);
        assert!(
            sim.snow_tiles()
                .iter()
                .all(|(_, team, _, _)| *team == Team(0))
        );
    }
    #[test]
    fn tile_rounding_retains_wurst_truncation_and_grid_bounds() {
        let p = SnowveilProfile {
            rawcode: 0,
            manual_ability: AbilityId(0),
            snow_terrain_rawcode: 0,
            tile_size: 8 * SUBUNITS_PER_WORLD_UNIT,
            origin: SimPoint::new(0, 0),
            battlefield_bounds: [0; 4],
            width: 4,
            height: 3,
            damage_taken_per_10k: 5000,
            explosion_damage: 5,
            explosion_radius: 9,
            manual_cooldown_ticks: 7,
        };
        assert_eq!(
            tile(&p, SimPoint::new(4 * SUBUNITS_PER_WORLD_UNIT - 1, 0)),
            Some(0)
        );
        assert_eq!(
            tile(&p, SimPoint::new(4 * SUBUNITS_PER_WORLD_UNIT, 0)),
            Some(1)
        );
        assert_eq!(
            tile(&p, SimPoint::new(-11 * SUBUNITS_PER_WORLD_UNIT, 0)),
            Some(0)
        );
        assert_eq!(
            tile(&p, SimPoint::new(-12 * SUBUNITS_PER_WORLD_UNIT, 0)),
            None
        );
        assert_eq!(
            tile(&p, SimPoint::new(28 * SUBUNITS_PER_WORLD_UNIT, 0)),
            None
        );
    }
    #[test]
    fn snow_is_first_writer_owned_and_protection_follows_live_positions() {
        let mut sim = sim(1);
        let point = SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT);
        unit(&mut sim, point, Team(0), true);
        unit(&mut sim, point, Team(1), true);
        sim.place_snow(point, Team(0), VERSION);
        sim.place_snow(point, Team(1), VERSION);
        assert!(
            sim.snow_tiles()
                .iter()
                .all(|(_, team, _, _)| *team == Team(0))
        );
        let mut units = sim.snapshot_units();
        sim.refresh_snow_protection(&mut units, &mut []);
        let p = snowveil_for_version(VERSION);
        assert_eq!(units[0].snow_damage_taken_per_10k, p.damage_taken_per_10k);
        assert_eq!(units[1].snow_damage_taken_per_10k, 10_000);
        let far = SimPoint::new(point.x + 5 * p.tile_size, point.y);
        sim.refresh_snow_protection_at(&mut units, &mut [], Some(&[far, point]));
        assert_eq!(units[0].snow_damage_taken_per_10k, 10_000);
        let mut other = restored(&sim);
        for _ in 0..4 {
            assert_eq!(sim.step().checksum, other.step().checksum);
        }
    }
    #[test]
    fn manual_damage_checks_both_circles_and_shares_owner_cooldown_across_fountains() {
        let mut sim = sim(1);
        let a = fountain(&mut sim, 3);
        let b = fountain(&mut sim, 5);
        let p = snowveil_for_version(VERSION);
        let point = center(p, tile(p, SimPoint::new(0, 0)).unwrap());
        let victim = unit(&mut sim, point, Team(1), true);
        let ignored = unit(&mut sim, point, Team(1), false);
        let ally = unit(&mut sim, point, Team(0), true);
        // Enemy-owned snow protects the victim even from UNIVERSAL explosion damage.
        sim.place_snow(point, Team(1), VERSION);
        let mut other = restored(&sim);
        for sim in [&mut sim, &mut other] {
            sim.cast_building_spell_at_for_player(PlayerId(0), a, point)
                .unwrap();
            assert_eq!(
                sim.unit(victim).unwrap().health,
                2000 - scale_damage_per_10k(p.explosion_damage, p.damage_taken_per_10k)
            );
            assert_eq!(sim.unit(ignored).unwrap().health, 2000);
            assert_eq!(sim.unit(ally).unwrap().health, 2000);
            assert!(sim.snow_tiles().is_empty());
            assert_eq!(
                sim.cast_building_spell_at_for_player(PlayerId(0), b, point),
                Err(BuildingCommandError::AbilityOnCooldown)
            );
        }
        assert_eq!(sim.step().checksum, other.step().checksum);
        sim.place_snow(point, Team(0), VERSION);
        sim.finish_match_from_control(MatchOutcome::Draw);
        assert!(sim.snow_tiles().is_empty());
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::tests::*;
    use super::*;
    #[test]
    fn explosion_requires_unit_and_tile_center_inside_and_preserves_unconsumed_snow() {
        let mut sim = sim(1);
        let caster = fountain(&mut sim, 3);
        let p = snowveil_for_version(VERSION);
        let point = center(p, tile(p, SimPoint::new(0, 0)).unwrap());
        let unit_outside = SimPoint::new(point.x + p.explosion_radius + p.tile_size / 4, point.y);
        let tile_outside = SimPoint::new(
            point.x + p.explosion_radius - p.tile_size / 4,
            point.y + p.tile_size * 3 / 4,
        );
        assert!(point.distance_sq(unit_outside) > square_i32(p.explosion_radius));
        assert!(
            point.distance_sq(center(p, tile(p, unit_outside).unwrap()))
                <= square_i32(p.explosion_radius)
        );
        assert!(point.distance_sq(tile_outside) <= square_i32(p.explosion_radius));
        assert!(
            point.distance_sq(center(p, tile(p, tile_outside).unwrap()))
                > square_i32(p.explosion_radius)
        );
        let a = unit(&mut sim, unit_outside, Team(1), true);
        let b = unit(&mut sim, tile_outside, Team(1), true);
        sim.place_snow(unit_outside, Team(0), VERSION);
        sim.place_snow(tile_outside, Team(1), VERSION);
        sim.cast_building_spell_at_for_player(PlayerId(0), caster, point)
            .unwrap();
        assert_eq!(sim.unit(a).unwrap().health, 2000);
        assert_eq!(sim.unit(b).unwrap().health, 2000);
        assert!(sim.snow_tiles().iter().all(|(center, _, _, _)| center.distance_sq(point) > square_i32(p.explosion_radius)));
        assert!(!sim.snow_tiles().is_empty());
    }
}
