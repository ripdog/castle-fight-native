use super::*;
use crate::{ManaProfile, MapVersion, building_mechanics::BuildingEraserProfile};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
fn profile() -> BuildingEraserProfile {
    let AbilityEffect::BuildingEraser(mut p) = crate::CastleFightTowerKind::Eraser
        .definition()
        .spellcasting
        .unwrap()
        .ability
        .effect
    else {
        panic!()
    };
    p.damage = 17;
    p.trigger_mechanical_only = false;
    p
}
fn sim() -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 500),
            ..Default::default()
        },
        1,
    )
}
fn entity(s: &Simulation, id: SimId) -> Entity {
    s.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn setup(s: &mut Simulation) -> SimId {
    let p = profile();
    let caster = s.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(7, 7, 0),
            ability: AutomaticAbilityProfile {
                id: p.ability,
                mana_cost: 7,
                cooldown_ticks: 100,
                range: 1000 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingEraser(p),
            },
        }),
    });
    s.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(100, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    caster
}
fn victim(
    s: &mut Simulation,
    team: u8,
    flags: UnitClassifications,
    movement: MovementClass,
) -> SimId {
    s.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(50 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 100,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            classifications: flags,
            movement_class: movement,
            armor: ArmorProfile {
                armor_type: ArmorType::Unarmored,
                armor_points: 100,
            },
            collision_radius: Some(CollisionRadius(0)),
            ..Default::default()
        },
    )
}
fn picker() -> UnitClassifications {
    UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    }
}
fn restored(s: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snap =
        SimulationSnapshot::decode_wire(&s.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut other = Simulation::new(s.config.clone(), 4);
    other.restore_snapshot(&snap).unwrap();
    other
}

#[test]
fn selected_type_wipe_admits_air_immune_and_nonsapper_matches_but_keeps_other_types_and_allies() {
    let mut s = sim();
    let caster = setup(&mut s);
    let ground = victim(&mut s, 1, picker(), MovementClass::Ground);
    let immune_air = victim(
        &mut s,
        1,
        UnitClassifications {
            spell_immune: true,
            ..Default::default()
        },
        MovementClass::Air,
    );
    let nonsapper = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    let ally = victim(&mut s, 0, Default::default(), MovementClass::Ground);
    let other_type = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    let definition = crate::CastleFightUnitKind::Satyr.definition();
    s.world
        .entity_mut(entity(&s, other_type))
        .insert(ContentIdentity {
            map_version: VERSION,
            rawcode: definition.rawcode,
            name: definition.name,
        });
    let protected = victim(
        &mut s,
        1,
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
        MovementClass::Ground,
    );
    let locust = victim(
        &mut s,
        1,
        UnitClassifications {
            locust: true,
            ..Default::default()
        },
        MovementClass::Ground,
    );
    s.step();
    for id in [ground, immune_air, nonsapper] {
        assert_eq!(s.unit(id).unwrap().health, 983);
    }
    for id in [ally, other_type, protected, locust] {
        assert_eq!(s.unit(id).unwrap().health, 1000);
    }
    assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(s.building(caster).unwrap().ability_ready_tick, Some(100));
    assert!(
        s.ability_casts_last_tick()
            .iter()
            .any(|e| e.target == AbilityCastTarget::Unit(protected))
    );
    assert!(
        !s.ability_casts_last_tick()
            .iter()
            .any(|e| e.target == AbilityCastTarget::Unit(locust))
    );
    let mut other = restored(&s);
    for _ in 0..4 {
        assert_eq!(
            {
                s.step();
                s.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
    }
}

#[test]
fn strong_shield_is_removed_without_healing_and_weak_shield_does_not_intercept() {
    let mut s = sim();
    setup(&mut s);
    let pick = victim(&mut s, 1, picker(), MovementClass::Ground);
    let strong = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    let weak = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    for id in [strong, weak] {
        s.world.get_mut::<Health>(entity(&s, id)).unwrap().current = 900;
    }
    s.set_negative_building_shield(strong, 2, None);
    s.set_negative_building_shield(weak, 1, None);
    s.step();
    assert_eq!(s.unit(pick).unwrap().health, 983);
    assert_eq!(s.unit(strong).unwrap().health, 900);
    assert_eq!(s.unit(strong).unwrap().negative_building_shield_level, 0);
    assert_eq!(s.unit(weak).unwrap().health, 883);
    assert_eq!(s.unit(weak).unwrap().negative_building_shield_level, 1);
    assert!(
        !s.ability_casts_last_tick()
            .iter()
            .any(|e| e.target == AbilityCastTarget::Unit(strong))
    );
    assert!(
        s.ability_casts_last_tick()
            .iter()
            .any(|e| e.target == AbilityCastTarget::Unit(weak))
    );
    let mut other = restored(&s);
    for _ in 0..3 {
        assert_eq!(
            {
                s.step();
                s.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
    }
}

#[test]
fn anti_negative_marker_is_checked_after_recipient_art_without_consuming_a_weak_shield() {
    let mut s = sim();
    setup(&mut s);
    victim(&mut s, 1, picker(), MovementClass::Ground);
    let protected = victim(&mut s, 1, Default::default(), MovementClass::Air);
    s.set_negative_building_markers(protected, true, false);
    s.set_negative_building_shield(protected, 1, None);
    s.step();
    assert_eq!(s.unit(protected).unwrap().health, 1000);
    assert_eq!(s.unit(protected).unwrap().negative_building_shield_level, 1);
    assert!(
        s.ability_casts_last_tick()
            .iter()
            .any(|e| e.target == AbilityCastTarget::Unit(protected))
    );
}

#[test]
fn nil_picker_still_commits_parent_and_only_renders_caster_while_effect_respects_battlefield() {
    let mut s = sim();
    let caster = setup(&mut s);
    let nonsapper = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    s.step();
    assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(s.unit(nonsapper).unwrap().health, 1000);
    assert_eq!(s.ability_casts_last_tick().len(), 1);
    assert_eq!(s.ability_casts_last_tick()[0].target_position, None);
    let mut s = sim();
    setup(&mut s);
    victim(&mut s, 1, picker(), MovementClass::Ground);
    let outside = victim(&mut s, 1, Default::default(), MovementClass::Ground);
    let bounds = crate::building_mechanics::snowveil_for_version(VERSION).battlefield_bounds;
    s.world
        .entity_mut(entity(&s, outside))
        .insert(Position(SimPoint::new(bounds[0] - 1, 0)));
    s.step();
    assert_eq!(s.unit(outside).unwrap().health, 1000);
}
