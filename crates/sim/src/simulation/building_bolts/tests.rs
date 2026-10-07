use super::*;
use crate::{ManaProfile, MapVersion, UnitTemplate};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
fn sim() -> Simulation {
    Simulation::new(SimulationConfig::default(), 1)
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn setup(sim: &mut Simulation) -> (SimId, BuildingBoltProfile) {
    let definition = crate::CastleFightTowerKind::ChillingMushroom.definition();
    let mut spawn = definition.spawn(Team(0), BuildingFootprint::new(2, 2, 1, 1));
    let casting = spawn.spellcasting.as_mut().unwrap();
    casting.mana = ManaProfile::per_second(casting.ability.mana_cost, casting.ability.mana_cost, 0);
    let AbilityEffect::BuildingBolt(mut p) = casting.ability.effect else {
        panic!()
    };
    p.bolt.damage = 17;
    p.bolt.stun_ticks = 8;
    p.bolt.hero_stun_ticks = 3;
    p.bolt.speed_per_tick = 5 * SUBUNITS_PER_WORLD_UNIT;
    p.child_range = 1000 * SUBUNITS_PER_WORLD_UNIT;
    casting.ability.effect = AbilityEffect::BuildingBolt(p);
    let id = sim.spawn_building_with_properties(spawn, definition.gameplay_properties());
    // Native parent selects ground/structure; the callback independently selects air.
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(12, 2, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    (id, p)
}
fn victim(
    sim: &mut Simulation,
    team: Team,
    movement_class: MovementClass,
    classifications: UnitClassifications,
) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn::from_template(
            team,
            SimPoint::new(50 * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
            UnitTemplate {
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
        ),
        UnitGameplayProperties {
            movement_class,
            classifications,
            ..Default::default()
        },
    )
}
fn restored(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut result = Simulation::new(sim.config.clone(), 4);
    result.restore_snapshot(&snapshot).unwrap();
    result
}
#[test]
fn callback_bolt_retains_flight_after_source_death_and_uses_live_hero_stun_on_restore() {
    let mut sim = sim();
    let (caster, p) = setup(&mut sim);
    let target = victim(
        &mut sim,
        Team(1),
        MovementClass::Air,
        UnitClassifications {
            hero: true,
            combat_sapper: true,
            ..Default::default()
        },
    );
    sim.step();
    assert_eq!(sim.projectile_count(), 1);
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    sim.world
        .get_mut::<Health>(entity(&sim, caster))
        .unwrap()
        .current = 0;
    let mut other = restored(&sim);
    while sim.unit(target).unwrap().health == 1000 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    let view = sim.unit(target).unwrap();
    let impact = sim.next_tick - 1;
    assert_eq!(
        view.health,
        1000 - sim
            .combat_rules
            .damage_rules
            .apply_spell(p.bolt.damage, view.armor.armor_type)
    );
    assert_eq!(
        view.status.native_stun_until_tick,
        impact + u64::from(p.bolt.hero_stun_ticks)
    );
    assert_eq!(view.status.native_stun_ability, Some(p.bolt.ability));
    let mut other = restored(&sim);
    for _ in 0..5 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
}
#[test]
fn independent_callback_rejects_ground_allied_invulnerable_and_non_sapper_victims_after_parent_spend()
 {
    for (team, movement, flags) in [
        (
            Team(1),
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
        ),
        (
            Team(0),
            MovementClass::Air,
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
        ),
        (
            Team(1),
            MovementClass::Air,
            UnitClassifications {
                combat_sapper: true,
                invulnerable: true,
                ..Default::default()
            },
        ),
        (Team(1), MovementClass::Air, UnitClassifications::default()),
    ] {
        let mut sim = sim();
        let (caster, _) = setup(&mut sim);
        victim(&mut sim, team, movement, flags);
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
        assert_eq!(sim.projectile_count(), 0);
    }
}
#[test]
fn callback_shield_intercepts_before_native_immunity_and_consumes_parent_resources() {
    let mut sim = sim();
    let (caster, _) = setup(&mut sim);
    let target = victim(
        &mut sim,
        Team(1),
        MovementClass::Air,
        UnitClassifications {
            combat_sapper: true,
            spell_immune: true,
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, target))
        .unwrap()
        .current = 500;
    assert!(sim.set_negative_building_shield(target, 2, None));
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert_eq!(sim.unit(target).unwrap().negative_building_shield_level, 1);
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(sim.projectile_count(), 0);
}
#[test]
fn independent_child_range_and_live_immunity_can_fail_without_refunding_parent() {
    for (immune, short_range) in [(false, true), (true, false)] {
        let mut sim = sim();
        let (caster, mut p) = setup(&mut sim);
        if short_range {
            p.child_range = SUBUNITS_PER_WORLD_UNIT;
        }
        let e = entity(&sim, caster);
        sim.world
            .get_mut::<SpellcastingProfile>(e)
            .unwrap()
            .ability
            .effect = AbilityEffect::BuildingBolt(p);
        victim(
            &mut sim,
            Team(1),
            MovementClass::Air,
            UnitClassifications {
                combat_sapper: true,
                spell_immune: immune,
                ..Default::default()
            },
        );
        sim.step();
        assert_eq!(sim.projectile_count(), 0);
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    }
}
#[test]
fn live_child_immunity_rejects_damage_and_stun_after_projectile_commitment() {
    let mut sim = sim();
    let (_, _) = setup(&mut sim);
    let target = victim(
        &mut sim,
        Team(1),
        MovementClass::Air,
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
    );
    sim.step();
    assert_eq!(sim.projectile_count(), 1);
    sim.world
        .get_mut::<UnitClassifications>(entity(&sim, target))
        .unwrap()
        .spell_immune = true;
    let mut other = restored(&sim);
    while sim.projectile_count() > 0 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert_eq!(sim.unit(target).unwrap().status.native_stun_ability, None);
}
#[test]
fn flying_callback_candidates_do_not_substitute_for_an_eligible_native_parent_trigger() {
    let mut sim = sim();
    let (caster, _) = setup(&mut sim);
    let remove = sim
        .world
        .iter_entities()
        .filter(|e| e.contains::<BuildingFootprint>() && e.get::<SimId>() != Some(&caster))
        .map(|e| e.id())
        .collect::<Vec<_>>();
    for e in remove {
        sim.world.despawn(e);
    }
    victim(
        &mut sim,
        Team(1),
        MovementClass::Air,
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
    );
    sim.step();
    assert!(sim.building(caster).unwrap().mana_current.unwrap() > 0);
    assert_eq!(sim.projectile_count(), 0);
}
