use super::*;
use crate::{
    ManaProfile, MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect, NativeAbilityGrants,
};

fn profile() -> NagaEarthquakeProfile {
    NagaEarthquakeProfile {
        parent: crate::NativeParasiteProfile {
            buff_rawcode: 506,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: 1000 * SUBUNITS_PER_WORLD_UNIT,
            affects_invulnerable: true,
            duration_ticks: 1,
            hero_duration_ticks: 0,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        trigger_mechanical_only: true,
        selector_exclusions: [AbilityId(504), AbilityId(505)],
        child: AbilityId(502),
        buff: 503,
        radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        damage: 23,
        movement_percent_delta: -25,
        attack_speed_percent_delta: -75,
        duration_ticks: 7,
        hero_duration_ticks: 2,
        targets: AttackTargetMask::GROUND_UNITS,
    }
}
fn simulation() -> Simulation {
    Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
            navigation_max: NavCell::new(500, 64),
            ..Default::default()
        },
        1,
    )
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
                maximum: 9,
                starting: 9,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(501),
                mana_cost: 7,
                cooldown_ticks: 30,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::NagaEarthquake(profile()),
            },
        }),
    })
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
fn restored(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}

#[test]
fn native_parent_commits_independently_of_empty_global_selector() {
    for structure in [false, true] {
        let mut sim = simulation();
        let caster = building(&mut sim, 0, 1, true);
        unit(&mut sim, 1, 100, false); // Ordinary organic bodies cannot trigger the parent.
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(9));
        if structure {
            building(&mut sim, 1, 3, false);
        } else {
            let target = unit(&mut sim, 1, 100, false);
            sim.world.entity_mut(entity(&sim, target)).insert((
                crate::components::MechanicalUnit,
                crate::components::BuildTimeTicks(4),
                crate::components::RepairTimeTicks(4),
            ));
        }
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(2));
        assert!(sim.last_ability_casts.is_empty());
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(2));
    }
}

#[test]
fn selected_point_is_independent_of_trigger_and_native_masks_durations_survive_restore() {
    let mut sim = simulation();
    let caster = building(&mut sim, 0, 1, true);
    let trigger = unit(&mut sim, 1, 100, false);
    sim.world.entity_mut(entity(&sim, trigger)).insert((
        crate::components::MechanicalUnit,
        crate::components::BuildTimeTicks(4),
        crate::components::RepairTimeTicks(4),
    ));
    let center = unit(&mut sim, 1, 500, true);
    let ground = unit(&mut sim, 1, 510, false);
    let hero = unit(&mut sim, 1, 515, false);
    sim.world
        .entity_mut(entity(&sim, hero))
        .insert(UnitClassifications {
            hero: true,
            ..Default::default()
        });
    let air = unit(&mut sim, 1, 520, false);
    sim.world
        .entity_mut(entity(&sim, air))
        .insert(MovementClass::Air);
    let immune = unit(&mut sim, 1, 525, false);
    sim.world
        .entity_mut(entity(&sim, immune))
        .insert(UnitClassifications {
            spell_immune: true,
            ..Default::default()
        });
    let invulnerable = unit(&mut sim, 1, 530, false);
    sim.world
        .entity_mut(entity(&sim, invulnerable))
        .insert(UnitClassifications {
            invulnerable: true,
            ..Default::default()
        });
    let friend = unit(&mut sim, 0, 505, false);
    let far = unit(&mut sim, 1, 600, false);
    let structure = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(31, 3, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(2));
    let cast = sim
        .last_ability_casts
        .iter()
        .find(|c| c.ability == profile().child)
        .unwrap();
    assert_eq!(
        cast.target_position,
        Some(sim.unit(center).unwrap().position)
    );
    for id in [center, ground, hero] {
        assert_eq!(sim.unit(id).unwrap().health, 977);
    }
    for id in [trigger, air, immune, invulnerable, friend, far] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(sim.building(structure).unwrap().health, 1000);
    let status = sim.world.get::<StatusState>(entity(&sim, ground)).unwrap();
    assert!(
        status.movement_modifiers[0].undispellable
            && status.attack_speed_modifiers[0].undispellable
    );
    let mut copy = *status;
    native_summons::dispel_magic_buffs(&mut copy);
    assert_eq!(copy.movement_modifier_count, 1);
    assert_eq!(copy.attack_speed_modifier_count, 1);
    sim.world
        .get_mut::<Health>(entity(&sim, caster))
        .unwrap()
        .current = 0;
    let mut other = restored(&sim);
    for _ in 0..8 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(
        sim.world
            .get::<StatusState>(entity(&sim, ground))
            .unwrap()
            .movement_modifier_count,
        0
    );
}

#[test]
fn independent_selector_keeps_native_exclusions_and_accepts_hidden_immune_ground_bodies() {
    let mut sim = simulation();
    building(&mut sim, 0, 1, false);
    let id = unit(&mut sim, 1, 500, true);
    let src = source(&mut sim);
    let mut units = sim.snapshot_units();
    for exclusion in profile().selector_exclusions {
        units[0].status.native_grants = NativeAbilityGrants::from_slice(&[NativeAbilityGrant {
            ability: exclusion,
            effect: NativeAbilityGrantEffect::AuraMarker,
        }]);
        assert_eq!(
            sim.cast_naga_earthquake(src, AbilityId(501), id, profile(), 0, &mut units),
            0
        );
        assert!(sim.last_ability_casts.is_empty());
    }
    units[0].status.native_grants = NativeAbilityGrants::default();
    units[0].movement_class = MovementClass::Air;
    assert_eq!(
        sim.cast_naga_earthquake(src, AbilityId(501), id, profile(), 0, &mut units),
        0
    );
    units[0].movement_class = MovementClass::Ground;
    units[0].classifications.invisible = true;
    units[0].classifications.spell_immune = true;
    assert_eq!(
        sim.cast_naga_earthquake(src, AbilityId(501), id, profile(), 0, &mut units),
        0
    );
    assert_eq!(
        sim.last_ability_casts[0].target_position,
        Some(sim.unit(id).unwrap().position)
    );
    assert_eq!(units[0].health, 1000); // Selector succeeds; independent native effect rejects immunity.
}

#[test]
fn native_parent_structure_death_creates_independent_body_after_source_removal_and_restore() {
    let mut sim = simulation();
    let caster = building(&mut sim, 0, 1, false);
    let target = building(&mut sim, 1, 7, false);
    let source = source(&mut sim);
    let mut p = profile().parent;
    p.duration_ticks = 8;
    sim.launch_native_parasite(source, PlayerId(0), target, p);
    sim.step();
    assert!(
        sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<crate::components::NativeAction>(),
        Some(crate::components::NativeAction::Parasite(p)) if p.buff_until_tick == Some(8)))
    );
    let mut other = restored(&sim);
    for world in [&mut sim, &mut other] {
        world.world.despawn(entity(world, caster));
        let attacker = unit(world, 0, 140, false);
        let e = entity(world, attacker);
        world
            .world
            .entity_mut(e)
            .insert(AttackTargetMask::GROUND_AND_BUILDINGS);
        let mut attack = world.world.get_mut::<AttackProfile>(e).unwrap();
        attack.damage = 5000;
        attack.range = 100 * SUBUNITS_PER_WORLD_UNIT;
        attack.acquisition_range = attack.range;
        attack.cooldown_ticks = 1;
    }
    // Ordinary combat kills the target after snapshotting; the new attacker skips its birth tick.
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(sim.step().checksum, other.step().checksum);
    let body = sim
        .snapshot_units()
        .into_iter()
        .find(|u| u.rawcode == p.body_rawcode)
        .unwrap();
    assert_eq!(body.owner, PlayerId(0));
    assert_eq!(body.team, Team(0));
    assert!(
        body.classifications.summoned && body.mechanical && !body.classifications.combat_sapper
    );
    assert_eq!(body.attack_targets.bits(), 0);
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert!(sim.unit(body.id).is_none());
}

#[test]
fn native_parent_impact_rechecks_immunity_and_zero_duration_uses_native_permanent_buff_rules() {
    for (allowed, invulnerable, immune, succeeds) in [
        (false, true, false, false),
        (true, false, true, false),
        (true, true, false, true),
    ] {
        let mut sim = simulation();
        building(&mut sim, 0, 1, false);
        let target = unit(&mut sim, 1, 500, false);
        sim.world
            .entity_mut(entity(&sim, target))
            .insert(UnitClassifications {
                hero: true,
                invulnerable,
                spell_immune: immune,
                ..Default::default()
            });
        let source = source(&mut sim);
        let mut p = profile().parent;
        p.affects_invulnerable = allowed;
        sim.launch_native_parasite(source, PlayerId(0), target, p);
        sim.step();
        let active = sim.world.iter_entities().find_map(|e| match e
            .get::<crate::components::NativeAction>()
        {
            Some(crate::components::NativeAction::Parasite(p)) => Some(p.buff_until_tick),
            _ => None,
        });
        assert_eq!(active, succeeds.then_some(Some(u64::MAX)));
        assert_eq!(
            sim.native_parasite_buffs(),
            if succeeds {
                vec![(target, p.buff_rawcode)]
            } else {
                vec![]
            }
        );
        let mut other = restored(&sim);
        for _ in 0..4 {
            assert_eq!(sim.step().checksum, other.step().checksum);
        }
    }
}
