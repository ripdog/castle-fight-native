use super::*;
use crate::{FogOfWar, FogReveal, RememberedStructure};

impl Simulation {
    /// Readback also resolves tick-zero spawns and command-boundary teleports without advancing time.
    pub fn fog_of_war(&self) -> Option<FogOfWar> {
        let mut fog = self.fog.clone()?;
        self.populate_vision(&mut fog);
        Some(fog)
    }

    pub(crate) fn refresh_vision(&mut self) {
        if let Some(mut fog) = self.fog.take() {
            self.populate_vision(&mut fog);
            self.fog = Some(fog);
        }
    }

    pub(super) fn reveal_fogged_attack_sources(
        &mut self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) {
        let Some(profile) = self
            .config
            .fog
            .as_ref()
            .and_then(|rules| rules.attack_reveal)
        else {
            return;
        };
        let grants: Vec<_> = self
            .last_attacks
            .iter()
            .filter_map(|attack| {
                let team = find_unit_index(units, attack.target)
                    .map(|index| units[index].team)
                    .or_else(|| {
                        find_building_index(buildings, attack.target)
                            .map(|index| buildings[index].team)
                    })?;
                (self.visible_teams(attack.source_position) & (1 << team.0) == 0)
                    .then_some((team, attack.source_position))
            })
            .collect();
        for (team, position) in grants {
            self.reveal_area(
                team,
                position,
                profile.radius,
                profile.duration_ticks,
                false,
            );
        }
    }

    pub(super) fn visible_teams(&self, position: SimPoint) -> u8 {
        self.fog.as_ref().map_or(3, |fog| {
            u8::from(fog.is_visible(Team(0), position))
                | (u8::from(fog.is_visible(Team(1), position)) << 1)
        })
    }

    pub(super) fn reveal_area(
        &mut self,
        team: Team,
        position: SimPoint,
        radius: i32,
        duration: u64,
        detects_invisible: bool,
    ) {
        if radius > 0
            && duration > 0
            && let Some(fog) = &mut self.fog
        {
            fog.reveals.push(FogReveal {
                team,
                position,
                radius,
                detects_invisible,
                expires_tick: self
                    .next_tick
                    .checked_add(duration)
                    .expect("reveal expiry overflow"),
            });
        }
    }

    fn populate_vision(&self, fog: &mut FogOfWar) {
        let rules = self
            .config
            .fog
            .as_ref()
            .expect("fog rules accompany fog state");
        for cells in &mut fog.visible {
            cells.fill(0);
        }
        fog.reveals
            .retain(|reveal| self.next_tick < reveal.expires_tick);
        for team in [Team(0), Team(1)] {
            for &(min, max) in &rules.permanent_rectangles[usize::from(team.0)] {
                fog.unveil_rectangle(team, min, max);
            }
        }
        // Explicit script fog modifiers ignore occlusion and never detect invisible units.
        for index in 0..fog.reveals.len() {
            let reveal = fog.reveals[index];
            fog.unveil_circle(reveal.team, reveal.position, reveal.radius, |_| true);
        }
        let mut sources = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let id = *entity.get::<SimId>()?;
                let team = *entity.get::<Team>()?;
                if entity.get::<Corpse>().is_some()
                    || entity.get::<Health>().is_some_and(|h| h.current <= 0)
                {
                    return None;
                }
                let building = entity.get::<BuildingFootprint>().copied();
                let builder = entity.get::<BuilderConfiguration>();
                // Projectiles, dummies and delayed effect entities have no ordinary vision source.
                if building.is_none()
                    && builder.is_none()
                    && entity.get::<AttackProfile>().is_none()
                {
                    return None;
                }
                let position = building
                    .map(|footprint| {
                        footprint_center_point(footprint, self.config.navigation_cell_size)
                    })
                    .or_else(|| entity.get::<Position>().map(|p| p.0))?;
                let content = entity
                    .get::<ContentIdentity>()
                    .copied()
                    .or_else(|| builder.map(|b| b.appearance));
                let sight = content.map_or(rules.fallback_sight, |content| {
                    crate::castle_fight_sight_for_version(content.map_version, content.rawcode)
                        .expect("map entity must have extracted day/night sight")
                });
                let flying = entity.get::<MovementClass>() == Some(&MovementClass::Air);
                let revealed_teams = entity.get::<StatusState>().map_or(0, |status| {
                    u8::from(status.is_revealed_to(Team(0), self.next_tick))
                        | (u8::from(status.is_revealed_to(Team(1), self.next_tick)) << 1)
                });
                Some((
                    id,
                    team,
                    position,
                    sight.radius(rules.is_night(self.next_tick)),
                    flying,
                    revealed_teams,
                    building,
                    content,
                    entity.get::<Owner>().map(|owner| owner.0),
                    entity.get::<BuildingConstruction>().map(|construction| {
                        crate::fog::RememberedConstruction {
                            started_tick: construction.started_tick,
                            complete_tick: construction.complete_tick,
                            observed_tick: self.next_tick,
                        }
                    }),
                ))
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|source| source.0);
        for &(_, team, position, radius, flying, revealed_teams, _, _, _, _) in &sources {
            // Capture size outside the closure so mutating the visibility grid needs no clone.
            let cell_size = fog.cell_size;
            fog.unveil_circle(team, position, radius, |point| {
                flying
                    || ground_line_visible(
                        position,
                        point,
                        cell_size,
                        self.combat_rules.terrain_elevation.as_ref(),
                        &rules.sight_blockers,
                    )
            });
            // Faerie Fire shares the marked target's sight and tracks it as it moves.
            for observer in [Team(0), Team(1)] {
                if observer != team && revealed_teams & (1 << observer.0) != 0 {
                    fog.unveil_circle(observer, position, radius, |point| {
                        flying
                            || ground_line_visible(
                                position,
                                point,
                                cell_size,
                                self.combat_rules.terrain_elevation.as_ref(),
                                &rules.sight_blockers,
                            )
                    });
                }
            }
        }
        for observer in [Team(0), Team(1)] {
            let team = usize::from(observer.0);
            for (explored, visible) in fog.explored[team].iter_mut().zip(&fog.visible[team]) {
                *explored |= *visible;
            }
            let mut memory = std::mem::take(&mut fog.remembered_structures[team]);
            // If the old footprint is observed, replace its memory with live information. Destruction
            // outside vision leaves the old silhouette until its location is scouted again.
            memory.retain(|structure| {
                !fog.is_visible(
                    observer,
                    footprint_center_point(structure.footprint, self.config.navigation_cell_size),
                )
            });
            for &(id, owner_team, position, _, _, _, footprint, content, owner, construction) in
                &sources
            {
                if owner_team != observer
                    && fog.is_visible(observer, position)
                    && let Some(footprint) = footprint
                {
                    memory.retain(|structure| structure.id != id);
                    memory.push(RememberedStructure {
                        id,
                        content,
                        owner,
                        footprint,
                        construction,
                    });
                }
            }
            memory.sort_unstable_by_key(|structure| structure.id);
            fog.remembered_structures[team] = memory;
        }
    }
}

/// Ground vision stops at higher cliff levels and authored occluders; air vision bypasses both.
/// Smooth visual terrain height does not create an occluder by itself.
fn ground_line_visible(
    from: SimPoint,
    to: SimPoint,
    step: i32,
    terrain: Option<&TerrainElevationMap>,
    blockers: &[(SimPoint, SimPoint)],
) -> bool {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    let steps = (dx
        .unsigned_abs()
        .max(dy.unsigned_abs())
        .div_ceil(step as u64)
        .max(1)) as i64;
    let source_cliff = terrain
        .and_then(|terrain| terrain.sample(from))
        .map(|s| s.cliff_level);
    for index in 1..=steps {
        let point = SimPoint::new(
            (i64::from(from.x) + dx * index / steps) as i32,
            (i64::from(from.y) + dy * index / steps) as i32,
        );
        if let (Some(terrain), Some(source_cliff)) = (terrain, source_cliff)
            && terrain
                .sample(point)
                .is_some_and(|sample| sample.cliff_level > source_cliff)
        {
            return false;
        }
        // The occluding surface itself is visible, but it hides cells behind it.
        if index < steps
            && blockers.iter().any(|&(min, max)| {
                point.x >= min.x && point.x < max.x && point.y >= min.y && point.y < max.y
            })
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FogRules, SightProfile};

    fn fixture(workers: usize) -> Simulation {
        Simulation::new(
            SimulationConfig {
                fog: Some(FogRules {
                    cell_size: SUBUNITS_PER_WORLD_UNIT,
                    fallback_sight: SightProfile {
                        day: 4 * SUBUNITS_PER_WORLD_UNIT,
                        night: 2 * SUBUNITS_PER_WORLD_UNIT,
                    },
                    initially_explored: false,
                    night: false,
                    clock: None,
                    attack_reveal: None,
                    permanent_rectangles: [Vec::new(), Vec::new()],
                    sight_blockers: Vec::new(),
                }),
                navigation_min: NavCell::new(-20, -20),
                navigation_max: NavCell::new(40, 20),
                ..SimulationConfig::default()
            },
            workers,
        )
    }
    fn point(x: i32) -> SimPoint {
        SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT / 2)
    }
    fn actor(sim: &mut Simulation, team: Team, x: i32) -> SimId {
        sim.spawn_unit(UnitSpawn {
            team,
            position: point(x),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        })
    }
    fn entity(sim: &Simulation, id: SimId) -> Entity {
        sim.world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&id))
            .unwrap()
            .id()
    }

    #[test]
    fn attacks_from_fog_flash_the_source_to_the_victims_team() {
        let mut sim = fixture(1);
        sim.config.fog.as_mut().unwrap().attack_reveal = Some(crate::FogAttackReveal {
            radius: 2 * SUBUNITS_PER_WORLD_UNIT,
            duration_ticks: 3,
        });
        let attacker = actor(&mut sim, Team(0), 0);
        actor(&mut sim, Team(0), 8);
        actor(&mut sim, Team(1), 10);
        let id = entity(&sim, attacker);
        sim.world.entity_mut(id).insert(AttackProfile {
            damage: 1,
            range: 15 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 15 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 100,
            delivery: AttackDelivery::Melee,
        });
        sim.refresh_vision();
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(1), point(0)));
        sim.step(); // Ordinary weapons do not release on their spawn tick.
        sim.step();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(1), point(0)));
        for _ in 0..3 {
            sim.step();
        }
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(1), point(0)));
    }

    #[test]
    fn exploration_survives_lost_sight_and_enemy_sources_do_not_reveal_to_opponent() {
        let mut sim = fixture(1);
        let scout = actor(&mut sim, Team(0), 0);
        actor(&mut sim, Team(1), 20);
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(2)));
        assert!(!sim.fog.as_ref().unwrap().is_explored(Team(0), point(20)));
        let id = entity(&sim, scout);
        sim.world.get_mut::<Position>(id).unwrap().0 = point(10);
        sim.refresh_vision();
        let fog = sim.fog.as_ref().unwrap();
        assert!(!fog.is_visible(Team(0), point(2)));
        assert!(fog.is_explored(Team(0), point(2)));
        assert!(fog.is_visible(Team(0), point(12)));
    }

    #[test]
    fn faerie_revelation_tracks_target_shares_sight_and_expires_exclusively() {
        let mut sim = fixture(1);
        actor(&mut sim, Team(0), -10);
        let target = actor(&mut sim, Team(1), 12);
        let id = entity(&sim, target);
        sim.world.entity_mut(id).insert(UnitClassifications {
            invisible: true,
            ..Default::default()
        });
        apply_timed_armor_modifier(
            &mut sim.world.get_mut::<StatusState>(id).unwrap(),
            TimedArmorModifier {
                id: ModifierId(900),
                expires_tick: 3,
                revealed_to: Some(Team(0)),
                ..Default::default()
            },
        );
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(14)));
        assert!(
            sim.snapshot_units()
                .iter()
                .find(|u| u.id == target)
                .unwrap()
                .visible_to(Team(0), 0)
        );
        sim.world.get_mut::<Position>(id).unwrap().0 = point(22);
        sim.refresh_vision();
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(0), point(14)));
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(24)));
        sim.next_tick = 3;
        sim.refresh_vision();
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(0), point(24)));
        assert!(
            !sim.snapshot_units()
                .iter()
                .find(|u| u.id == target)
                .unwrap()
                .visible_to(Team(0), 3)
        );
        assert!(sim.fog.as_ref().unwrap().is_explored(Team(0), point(24)));
    }

    #[test]
    fn timed_area_vision_is_not_true_sight_and_survives_wire_restore_with_other_workers() {
        let mut sim = fixture(1);
        actor(&mut sim, Team(0), -10);
        let hidden = actor(&mut sim, Team(1), 20);
        let id = entity(&sim, hidden);
        sim.world.entity_mut(id).insert(UnitClassifications {
            invisible: true,
            ..Default::default()
        });
        sim.reveal_area(Team(0), point(20), 3 * SUBUNITS_PER_WORLD_UNIT, 3, false);
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(20)));
        assert!(
            !sim.snapshot_units()
                .iter()
                .find(|u| u.id == hidden)
                .unwrap()
                .visible_to(Team(0), 0)
        );
        let bundle =
            crate::castle_fight_content_bundle(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
        let snapshot =
            SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
                .unwrap();
        let mut restored = fixture(4);
        restored.restore_snapshot(&snapshot).unwrap();
        for _ in 0..5 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
        }
        assert!(sim.fog.as_ref().unwrap().reveals.is_empty());
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(0), point(20)));
    }

    #[test]
    fn detecting_area_reveals_invisibility_only_inside_radius_and_until_expiry() {
        let mut sim = fixture(1);
        let hidden = actor(&mut sim, Team(1), 20);
        let outside = actor(&mut sim, Team(1), 24);
        for unit in [hidden, outside] {
            let id = entity(&sim, unit);
            sim.world.entity_mut(id).insert(UnitClassifications {
                invisible: true,
                ..Default::default()
            });
        }
        sim.reveal_area(Team(0), point(20), 2 * SUBUNITS_PER_WORLD_UNIT, 3, true);
        sim.refresh_vision();
        let visible = |sim: &mut Simulation, id| {
            sim.snapshot_units()
                .iter()
                .find(|u| u.id == id)
                .unwrap()
                .visible_to(Team(0), sim.next_tick)
        };
        assert!(visible(&mut sim, hidden));
        assert!(!visible(&mut sim, outside));
        let snapshot = sim.capture_snapshot();
        let mut restored = fixture(4);
        restored.restore_snapshot(&snapshot).unwrap();
        assert!(visible(&mut restored, hidden));
        for _ in 0..3 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
        }
        assert!(!visible(&mut sim, hidden));
    }

    #[test]
    fn discovered_structure_destruction_in_fog_is_learned_only_when_rescouted() {
        let mut sim = fixture(1);
        let scout = actor(&mut sim, Team(0), 0);
        let structure = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(2, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        });
        sim.refresh_vision();
        assert_eq!(
            sim.fog.as_ref().unwrap().remembered_structures[0][0].id,
            structure
        );
        let scout_entity = entity(&sim, scout);
        sim.world.get_mut::<Position>(scout_entity).unwrap().0 = point(-10);
        let building_entity = entity(&sim, structure);
        sim.world.despawn(building_entity);
        sim.refresh_vision();
        assert_eq!(sim.fog.as_ref().unwrap().remembered_structures[0].len(), 1);
        let snapshot = sim.capture_snapshot();
        let mut restored = fixture(4);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(sim.fog, restored.fog);
        sim.world.get_mut::<Position>(scout_entity).unwrap().0 = point(0);
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().remembered_structures[0].is_empty());
    }

    #[test]
    fn acquisition_requires_team_vision_and_an_allied_scout_grants_it() {
        let mut sim = fixture(1);
        let source = actor(&mut sim, Team(0), 0);
        let target = actor(&mut sim, Team(1), 10);
        let id = entity(&sim, source);
        sim.world.entity_mut(id).insert(AttackProfile {
            damage: 5,
            range: 15 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 15 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 1,
            delivery: AttackDelivery::Melee,
        });
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, None);
        actor(&mut sim, Team(0), 8);
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(target));
        assert!(sim.unit(target).unwrap().health < 100);
    }

    #[test]
    fn structure_memory_freezes_observed_construction_until_rescouted() {
        let mut sim = fixture(1);
        let scout = actor(&mut sim, Team(0), 0);
        let structure = sim
            .try_start_building_construction(
                PlayerId(1),
                BuildingSpawn {
                    team: Team(1),
                    footprint: BuildingFootprint::new(3, 0, 1, 1),
                    health: 100,
                    production: None,
                    attack: None,
                    spellcasting: None,
                },
                BuildingGameplayProperties {
                    construction_time_ticks: Some(20),
                    ..Default::default()
                },
            )
            .unwrap();
        sim.refresh_vision();
        let observed = sim.fog.as_ref().unwrap().remembered_structures[0][0];
        assert!(observed.construction.is_some());
        let scout_entity = entity(&sim, scout);
        sim.world.get_mut::<Position>(scout_entity).unwrap().0 = point(-10);
        for _ in 0..25 {
            sim.step();
        }
        assert!(
            sim.building(structure)
                .unwrap()
                .construction_complete_tick
                .is_none()
        );
        assert_eq!(
            sim.fog.as_ref().unwrap().remembered_structures[0][0],
            observed
        );
        let mut restored = fixture(4);
        restored.restore_snapshot(&sim.capture_snapshot()).unwrap();
        assert_eq!(sim.fog, restored.fog);
        sim.world.get_mut::<Position>(scout_entity).unwrap().0 = point(0);
        sim.refresh_vision();
        assert!(
            sim.fog.as_ref().unwrap().remembered_structures[0][0]
                .construction
                .is_none()
        );
    }

    #[test]
    fn native_automatic_spell_requires_team_vision_and_invisible_detection() {
        let mut sim = fixture(1);
        let source = sim.spawn_unit_with_spellcasting(
            UnitSpawn {
                team: Team(0),
                position: point(0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                    delivery: AttackDelivery::Melee,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            SpellcastingProfile {
                mana: crate::ManaProfile::per_second(0, 0, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(900),
                    mana_cost: 0,
                    cooldown_ticks: 1,
                    range: 30 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitOrBuilding,
                    effect: AbilityEffect::PhoenixFire(crate::NativeBoltProfile {
                        ability: AbilityId(901),
                        damage: 1,
                        stun_ticks: 0,
                        hero_stun_ticks: 0,
                        damage_per_second: 0,
                        duration_ticks: 0,
                        speed_per_tick: 30 * SUBUNITS_PER_WORLD_UNIT,
                        cleanse: false,
                        targets: AttackTargetMask::ALL,
                    }),
                },
            },
        );
        let target = actor(&mut sim, Team(1), 10);
        sim.step();
        assert!(
            !sim.ability_casts_last_tick()
                .iter()
                .any(|cast| cast.source == source && cast.ability == AbilityId(900))
        );
        actor(&mut sim, Team(0), 8);
        sim.step();
        assert!(
            sim.ability_casts_last_tick()
                .iter()
                .any(|cast| cast.source == source && cast.ability == AbilityId(900))
        );
        let target_entity = entity(&sim, target);
        sim.world
            .entity_mut(target_entity)
            .insert(UnitClassifications {
                invisible: true,
                ..Default::default()
            });
        sim.step();
        assert!(
            !sim.ability_casts_last_tick()
                .iter()
                .any(|cast| cast.source == source && cast.ability == AbilityId(900))
        );
        sim.reveal_area(Team(0), point(10), 2 * SUBUNITS_PER_WORLD_UNIT, 3, true);
        sim.step();
        assert!(
            sim.ability_casts_last_tick()
                .iter()
                .any(|cast| cast.source == source && cast.ability == AbilityId(900))
        );
    }

    #[test]
    fn day_night_clock_changes_sight_at_exact_boundaries_and_wraps() {
        let mut sim = fixture(1);
        sim.config.fog.as_mut().unwrap().clock = Some(crate::FogClock {
            cycle_ticks: 8,
            dawn_phase_ticks: 2,
            dusk_phase_ticks: 6,
            initial_phase_ticks: 2,
        });
        actor(&mut sim, Team(0), 0);
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(3)));
        sim.next_tick = 4;
        sim.refresh_vision();
        let fog = sim.fog.as_ref().unwrap();
        assert!(!fog.is_visible(Team(0), point(3)));
        assert!(fog.is_explored(Team(0), point(3)));
        assert!(fog.is_visible(Team(0), point(1)));
        sim.next_tick = 8;
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(3)));
    }

    #[test]
    fn air_vision_bypasses_ground_occluders() {
        let mut sim = fixture(1);
        sim.config.fog.as_mut().unwrap().sight_blockers.push((
            SimPoint::new(SUBUNITS_PER_WORLD_UNIT, -SUBUNITS_PER_WORLD_UNIT),
            SimPoint::new(2 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT),
        ));
        let scout = actor(&mut sim, Team(0), 0);
        sim.refresh_vision();
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(0), point(3)));
        let id = entity(&sim, scout);
        sim.world.entity_mut(id).insert(MovementClass::Air);
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), point(3)));
    }

    #[test]
    fn malformed_visibility_snapshot_is_rejected_without_mutating_world() {
        let mut sim = fixture(1);
        actor(&mut sim, Team(0), 0);
        sim.refresh_vision();
        let before = sim.checksum();
        let mut wire = serde_json::to_value(sim.capture_snapshot()).unwrap();
        wire["fog"]["visible"][0].as_array_mut().unwrap().pop();
        let snapshot = serde_json::from_value(wire).unwrap();
        assert!(sim.restore_snapshot(&snapshot).is_err());
        assert_eq!(sim.checksum(), before);
    }

    #[test]
    fn ground_sight_is_blocked_by_occluders_and_higher_cliffs() {
        let blockers = [(SimPoint::new(4, -2), SimPoint::new(6, 2))];
        assert!(!ground_line_visible(
            SimPoint::new(0, 0),
            SimPoint::new(10, 0),
            1,
            None,
            &blockers
        ));
        assert!(ground_line_visible(
            SimPoint::new(0, 0),
            SimPoint::new(4, 0),
            1,
            None,
            &blockers
        ));
        let terrain = TerrainElevationMap::from_vertex_samples(
            SimPoint::new(0, 0),
            1,
            2,
            1,
            vec![2, 3, 3, 2, 3, 3],
            vec![0x2000; 6],
        )
        .unwrap();
        assert!(!ground_line_visible(
            SimPoint::new(0, 0),
            SimPoint::new(2, 0),
            1,
            Some(&terrain),
            &[]
        ));
        assert!(ground_line_visible(
            SimPoint::new(2, 0),
            SimPoint::new(0, 0),
            1,
            Some(&terrain),
            &[]
        ));
    }
}
