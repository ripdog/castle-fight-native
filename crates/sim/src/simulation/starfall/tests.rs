use super::*;
use crate::building_mechanics::StarfallWaveProfile;
use crate::{
    ManaProfile, MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect, NativeAbilityGrants,
};
fn profile() -> StarfallProfile {
    let normal = StarfallWaveProfile {
        ability: AbilityId(401),
        buff: 402,
        radius: 30 * SUBUNITS_PER_WORLD_UNIT,
        damage: 20,
        interval_ticks: 2,
        duration_ticks: 9,
        targets: AttackTargetMask::ALL,
        building_damage_per_10k: 5000,
        mana_cost: 0,
        cooldown_ticks: 13,
    };
    StarfallProfile {
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        trigger_mechanical_only: true,
        options: [
            normal,
            StarfallWaveProfile {
                ability: AbilityId(403),
                targets: AttackTargetMask::AIR_AND_GROUND,
                building_damage_per_10k: 0,
                mana_cost: 3,
                ..normal
            },
        ],
        selector_exclusions: [AbilityId(404), AbilityId(405)],
        selector_excluded_unit: 406,
        dummy_rawcode: 408,
        dummy_mana: 10,
        dummy_lifetime_ticks: 7,
        visual_duration_ticks: 4,
        vision_radius: 30 * SUBUNITS_PER_WORLD_UNIT,
    }
}
fn simulation(artillery: bool, fog: bool) -> Simulation {
    let mut config = SimulationConfig {
        navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
        navigation_max: NavCell::new(500, 64),
        ..Default::default()
    };
    config.match_rules.artillery_enabled = artillery;
    if fog {
        config.fog = Some(crate::FogRules {
            cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
            fallback_sight: crate::SightProfile { day: 0, night: 0 },
            initially_explored: false,
            night: false,
            clock: None,
            attack_reveal: None,
            permanent_rectangles: Default::default(),
            sight_blockers: vec![],
        });
    }
    Simulation::new(config, 1)
}
fn building(sim: &mut Simulation, team: u8, x: i32, spell: bool) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(team),
        footprint: BuildingFootprint::new(x, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: spell.then_some(SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(407),
                mana_cost: 3,
                cooldown_ticks: 30,
                range: 400 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::Starfall(profile()),
            },
        }),
    })
}
fn source(sim: &mut Simulation) -> AbilitySourceSnapshot {
    let b = sim.snapshot_buildings()[0];
    AbilitySourceSnapshot {
        map_version: b.map_version,
        source: AbilitySourceIndex::Building(0),
        id: b.id,
        team: b.team,
        origin: AbilitySourceOrigin::Building(b.footprint),
        health: b.health,
        stunned_until_tick: 0,
        spellcasting: b.spellcasting,
        mana_current: b.mana_current,
        ability_state: b.ability_state,
    }
}
fn unit(sim: &mut Simulation, team: u8, x: i32, sapper: bool) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
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
            classifications: UnitClassifications {
                combat_sapper: sapper,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn restored(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}
fn action(sim: &Simulation) -> Option<StarfallState> {
    sim.world
        .iter_entities()
        .find_map(|e| match e.get::<crate::components::NativeAction>() {
            Some(crate::components::NativeAction::Starfall(s)) => Some(s.clone()),
            _ => None,
        })
}
#[test]
fn native_channel_uses_fixed_point_and_live_masks_after_source_death_and_wire_restore() {
    for artillery in [true, false] {
        let mut sim = simulation(artillery, false);
        let caster = building(&mut sim, 0, 1, false);
        let structure = building(&mut sim, 1, 9, false);
        let center = unit(&mut sim, 1, 168, true);
        let immune = unit(&mut sim, 1, 170, false);
        let invulnerable = unit(&mut sim, 1, 175, false);
        let friendly = unit(&mut sim, 0, 176, false);
        let distant = unit(&mut sim, 1, 300, false);
        let immune_entity = entity(&sim, immune);
        sim.world
            .entity_mut(immune_entity)
            .insert(UnitClassifications {
                spell_immune: true,
                ..Default::default()
            });
        sim.world
            .entity_mut(immune_entity)
            .insert(MovementClass::Air);
        sim.world
            .get_mut::<StatusState>(immune_entity)
            .unwrap()
            .native_grants = NativeAbilityGrants::from_slice(&[
            NativeAbilityGrant {
                ability: AbilityId(501),
                effect: NativeAbilityGrantEffect::SpellResistance(0),
            },
            NativeAbilityGrant {
                ability: AbilityId(502),
                effect: NativeAbilityGrantEffect::HardenedSkin {
                    chance_per_10k: 10000,
                    ignored_damage: 1000,
                    minimum_damage: 0,
                },
            },
        ]);
        sim.world
            .entity_mut(entity(&sim, invulnerable))
            .insert(UnitClassifications {
                invulnerable: true,
                ..Default::default()
            });
        let src = source(&mut sim);
        let units = sim.snapshot_units();
        assert!(sim.cast_starfall(src, AbilityId(407), profile(), 0, &units));
        let s = action(&sim).unwrap();
        assert_eq!(s.center, sim.unit(center).unwrap().position);
        assert_eq!(s.mana_after_cast, if artillery { 10 } else { 7 });
        assert_eq!(s.cooldown_until_tick, 13);
        sim.world
            .get_mut::<Health>(entity(&sim, caster))
            .unwrap()
            .current = 0;
        // Moving the selected body proves the channel does not follow it.
        sim.world
            .get_mut::<Position>(entity(&sim, center))
            .unwrap()
            .0
            .x = 300 * SUBUNITS_PER_WORLD_UNIT;
        let mut other = restored(&sim);
        for tick in 0..=7 {
            assert_eq!(sim.step().checksum, other.step().checksum);
            let expected = 20 * (tick / 2).min(3);
            assert_eq!(sim.unit(immune).unwrap().health, 1000 - expected);
            assert_eq!(sim.unit(invulnerable).unwrap().health, 1000);
            assert_eq!(sim.unit(friendly).unwrap().health, 1000);
            assert_eq!(sim.unit(distant).unwrap().health, 1000);
            assert_eq!(sim.unit(center).unwrap().health, 1000);
            assert_eq!(
                sim.building(structure).unwrap().health,
                1000 - if artillery { expected / 2 } else { 0 }
            );
        }
        assert!(action(&sim).is_none());
    }
}
#[test]
fn vision_cleanup_precedes_channel_expiry_without_detecting_invisible_units() {
    let mut sim = simulation(true, true);
    building(&mut sim, 0, 1, false);
    let target = unit(&mut sim, 1, 168, true);
    sim.world
        .get_mut::<UnitClassifications>(entity(&sim, target))
        .unwrap()
        .invisible = true;
    let src = source(&mut sim);
    let units = sim.snapshot_units();
    assert!(sim.cast_starfall(src, AbilityId(407), profile(), 0, &units));
    let center = action(&sim).unwrap().center;
    assert_eq!(sim.fog.as_ref().unwrap().reveals[0].expires_tick, 4);
    let mut other = restored(&sim);
    for _ in 0..=4 {
        assert_eq!(sim.step().checksum, other.step().checksum);
        assert!(!sim.fog.as_ref().unwrap().detects_invisible(Team(0), center));
        assert_eq!(
            sim.fog.as_ref().unwrap().reveals.is_empty(),
            sim.next_tick >= 4
        );
    }
    assert!(action(&sim).is_some());
    assert_eq!(sim.unit(target).unwrap().health, 960);
}
#[test]
fn selector_exclusions_and_failed_secondary_resources_are_independent_of_parent_commit() {
    let mut sim = simulation(false, false);
    building(&mut sim, 0, 1, false);
    let src = source(&mut sim);
    let empty = sim.snapshot_units();
    assert!(!sim.cast_starfall(src, AbilityId(407), profile(), 0, &empty));
    assert!(sim.last_ability_casts.is_empty());
    unit(&mut sim, 1, 168, true);
    let mut units = sim.snapshot_units();
    for exclusion in profile().selector_exclusions {
        units[0].status.native_grants = NativeAbilityGrants::from_slice(&[NativeAbilityGrant {
            ability: exclusion,
            effect: NativeAbilityGrantEffect::AuraMarker,
        }]);
        assert!(!sim.cast_starfall(src, AbilityId(407), profile(), 0, &units));
    }
    units[0].status.native_grants = NativeAbilityGrants::default();
    units[0].rawcode = profile().selector_excluded_unit;
    assert!(!sim.cast_starfall(src, AbilityId(407), profile(), 0, &units));
    units[0].rawcode = 0;
    units[0].classifications.invulnerable = true; // Selector accepts it; pulses independently reject it.
    units[0].movement_class = MovementClass::Air;
    let mut p = profile();
    p.dummy_mana = 2;
    assert!(!sim.cast_starfall(src, AbilityId(407), p, 0, &units));
    assert!(action(&sim).is_none());
    assert_eq!(sim.last_ability_casts.len(), 1); // Caster effect precedes the failed native child.
}
#[test]
fn native_starfall_primary_commits_once_without_a_sapper_at_the_enemy_trigger() {
    for structure in [true, false] {
        let mut sim = simulation(true, false);
        let caster = building(&mut sim, 0, 1, true);
        unit(&mut sim, 1, 80, false); // Organic parent target fails.
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(10));
        if structure {
            building(&mut sim, 1, 10, false);
        } else {
            let enemy = unit(&mut sim, 1, 100, false);
            sim.world.entity_mut(entity(&sim, enemy)).insert((
                crate::components::MechanicalUnit,
                crate::components::BuildTimeTicks(4),
                crate::components::RepairTimeTicks(4),
            ));
        }
        for _ in 0..3 {
            sim.step();
        }
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(7));
        assert!(action(&sim).is_none());
    }
}
