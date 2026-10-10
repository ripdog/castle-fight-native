use super::*;
use crate::ManaProfile;

fn profile() -> crate::ScriptedRootsProfile {
    crate::ScriptedRootsProfile {
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        roots: crate::EntanglingRootsEffectProfile {
            ability: AbilityId(102),
            damage_per_second: 9,
            duration_ticks: 61,
            hero_duration_ticks: 11,
            nonhero_only: false,
            targets: AttackTargetMask::GROUND_UNITS,
        },
        primary_nonhero_only: true,
        search_radius: 0,
        maximum_targets: 1,
        child_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        excluded_abilities: [0, 0],
        priority_buffs: [0, 0],
        vision_radius: 0,
        vision_ticks: 0,
    }
}
fn sim() -> Simulation {
    Simulation::new(SimulationConfig::default(), 1)
}
fn spawn(s: &mut Simulation, team: u8, x: i32, flags: UnitClassifications) -> SimId {
    s.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
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
            classifications: flags,
            collision_radius: Some(CollisionRadius(0)),
            ..Default::default()
        },
    )
}
fn sapper() -> UnitClassifications {
    UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    }
}
fn source(units: &[UnitSnapshot]) -> AbilitySourceSnapshot {
    AbilitySourceSnapshot {
        map_version: None,
        source: AbilitySourceIndex::Unit(0),
        id: units[0].id,
        team: units[0].team,
        origin: AbilitySourceOrigin::Unit(units[0].position),
        health: units[0].health,
        stunned_until_tick: 0,
        spellcasting: None,
        mana_current: None,
        ability_state: None,
    }
}
#[test]
fn roots_parent_eligibility_is_separate_from_child_hero_duration_and_native_immunity() {
    let mut s = sim();
    spawn(&mut s, 1, 0, sapper());
    let mut u = s.snapshot_units()[0];
    assert!(primary_eligible(&u, Team(0), profile(), s.next_tick));
    u.classifications.hero = true;
    assert!(!primary_eligible(&u, Team(0), profile(), s.next_tick));
    let p = crate::ScriptedRootsProfile {
        primary_nonhero_only: false,
        ..profile()
    };
    assert!(primary_eligible(&u, Team(0), p, s.next_tick));
    u.classifications.spell_immune = true;
    assert!(!primary_eligible(&u, Team(0), p, s.next_tick));
    u.classifications.spell_immune = false;
    u.classifications.invulnerable = true;
    assert!(!primary_eligible(&u, Team(0), p, s.next_tick));
    u.classifications.invulnerable = false;
    u.classifications.combat_sapper = false;
    assert!(!primary_eligible(&u, Team(0), p, s.next_tick));
}
#[test]
fn failed_children_consume_selection_capacity_before_fallback_and_native_range_checks() {
    let mut s = sim();
    spawn(&mut s, 0, 0, sapper());
    spawn(&mut s, 1, 0, sapper());
    spawn(
        &mut s,
        1,
        0,
        UnitClassifications {
            spell_immune: true,
            ..sapper()
        },
    );
    spawn(&mut s, 1, 10, sapper());
    spawn(
        &mut s,
        1,
        1,
        UnitClassifications {
            hero: true,
            ..sapper()
        },
    );
    let mut units = s.snapshot_units();
    units[1].status.armor_modifier_count = 1;
    units[1].status.armor_modifiers[0] = TimedArmorModifier {
        id: ModifierId(555),
        native_buff: Some(crate::NativeBuffIdentity {
            rawcode: 100,
            positive: false,
            stealable: false,
            organic_only: false,
        }),
        expires_tick: 1000,
        ..Default::default()
    };
    let mut p = crate::ScriptedRootsProfile {
        search_radius: 20 * SUBUNITS_PER_WORLD_UNIT,
        child_range: 2 * SUBUNITS_PER_WORLD_UNIT,
        maximum_targets: 2,
        priority_buffs: [100, 200],
        ..profile()
    };
    let src = source(&units);
    assert_eq!(s.cast_scripted_roots(src, 1, p, &mut units), 0);
    assert_eq!(units[4].status.rooted_until_tick, 0);
    p.maximum_targets = 3;
    assert_eq!(s.cast_scripted_roots(src, 1, p, &mut units), 1);
    assert_eq!(
        units[4].status.rooted_until_tick,
        s.next_tick + u64::from(p.roots.hero_duration_ticks)
    );
    assert_eq!(units[1].status.rooted_until_tick, 0);
}
#[test]
fn roots_commit_parent_resources_once_and_periodic_damage_survives_source_death_and_wire_restore() {
    let mut s = sim();
    let p = profile();
    let caster = s.spawn_unit_with_properties_and_spellcasting(
        UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
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
            classifications: sapper(),
            collision_radius: Some(CollisionRadius(0)),
            ..Default::default()
        },
        SpellcastingProfile {
            mana: ManaProfile::per_second(10, 10, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(101),
                mana_cost: 3,
                cooldown_ticks: 1000,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                effect: AbilityEffect::ScriptedRoots(p),
            },
        },
    );
    spawn(
        &mut s,
        1,
        1,
        UnitClassifications {
            hero: true,
            ..sapper()
        },
    );
    let target = spawn(&mut s, 1, 10, sapper());
    s.step();
    assert!(s.unit(target).unwrap().status.rooted_until_tick > s.tick());
    assert_eq!(s.unit(caster).unwrap().mana_current, Some(7));
    let e = s
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&caster))
        .unwrap()
        .id();
    s.world.get_mut::<Health>(e).unwrap().current = 0;
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let decoded =
        SimulationSnapshot::decode_wire(&s.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = Simulation::new(SimulationConfig::default(), 4);
    restored.restore_snapshot(&decoded).unwrap();
    for _ in 0..65 {
        assert_eq!(s.step().checksum, restored.step().checksum);
    }
    assert_eq!(s.unit(target).unwrap().health, 82);
    assert!(s.unit(target).unwrap().status.rooted_until_tick <= s.tick());
    assert_eq!(s.unit(target).unwrap().status.damage_over_time_count, 0);
}

#[test]
fn temporary_child_vision_reveals_fog_without_detecting_invisible_recipients() {
    let mut s = Simulation::new(
        SimulationConfig {
            navigation_min: NavCell::new(-5, -5),
            navigation_max: NavCell::new(5, 5),
            fog: Some(crate::FogRules {
                cell_size: SUBUNITS_PER_WORLD_UNIT,
                fallback_sight: crate::SightProfile::default(),
                initially_explored: false,
                night: false,
                clock: None,
                attack_reveal: None,
                permanent_rectangles: [Vec::new(), Vec::new()],
                sight_blockers: Vec::new(),
            }),
            ..SimulationConfig::default()
        },
        1,
    );
    spawn(&mut s, 0, 0, sapper());
    spawn(&mut s, 1, 1, sapper());
    spawn(&mut s, 1, 2, sapper());
    spawn(
        &mut s,
        1,
        3,
        UnitClassifications {
            invisible: true,
            ..sapper()
        },
    );
    let mut units = s.snapshot_units();
    units[1].visible_teams = 3;
    units[2].visible_teams = 0;
    units[3].visible_teams = 0;
    let p = crate::ScriptedRootsProfile {
        maximum_targets: 3,
        search_radius: 5 * SUBUNITS_PER_WORLD_UNIT,
        vision_radius: SUBUNITS_PER_WORLD_UNIT,
        vision_ticks: 4,
        ..profile()
    };
    assert_eq!(s.cast_scripted_roots(source(&units), 1, p, &mut units), 2);
    assert!(units[2].status.rooted_until_tick > 0);
    assert_eq!(units[3].status.rooted_until_tick, 0);
    let reveals = &s.fog.as_ref().unwrap().reveals;
    assert_eq!(reveals.len(), 2);
    assert!(
        reveals
            .iter()
            .all(|r| !r.detects_invisible && r.expires_tick == s.next_tick + 4)
    );
}
