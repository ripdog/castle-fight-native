use super::*;
use crate::HostileArmorAuraProfile;

fn source(sim: &mut Simulation, x: i32, ability: u32, family: u32, armor: i16) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, x, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::HostileArmorAura(
                HostileArmorAuraProfile {
                    ability: AbilityId(ability),
                    buff_rawcode: family,
                    radius: 12 * SUBUNITS_PER_WORLD_UNIT,
                    armor_bonus_per_100: armor,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    linger_ticks: 0,
                },
            )),
            ..Default::default()
        },
    )
}
fn armor(sim: &Simulation, target: SimId) -> i32 {
    let u = sim.unit(target).unwrap();
    u.status.effective_armor_points_per_100(u.armor)
}

#[test]
fn hostile_devotion_is_universal_and_admits_air_mechanical_and_hero_enemies_at_the_boundary() {
    for (air, mechanical, hero, immune) in [(false, false, false, false), (true, true, true, true)]
    {
        let mut sim = simulation(1);
        source(&mut sim, 20, 871, 872, -300);
        let target = sim.spawn_unit_with_properties(
            unit(1, 32, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                classifications: UnitClassifications {
                    hero,
                    spell_immune: immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let outside = sim.spawn_unit(unit(1, 33, 0, AttackDelivery::Melee));
        let ally = sim.spawn_unit(unit(0, 32, 0, AttackDelivery::Melee));
        sim.step();
        assert_eq!(armor(&sim, target), -300);
        assert_eq!(armor(&sim, outside), 0);
        assert_eq!(armor(&sim, ally), 0);
    }
}

#[test]
fn hostile_devotion_rejects_invulnerability_and_locust() {
    let mut sim = simulation(1);
    source(&mut sim, 20, 871, 872, -300);
    let ids = [
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
        UnitClassifications {
            locust: true,
            ..Default::default()
        },
    ]
    .map(|classifications| {
        sim.spawn_unit_with_properties(
            unit(1, 25, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications,
                ..Default::default()
            },
        )
    });
    sim.step();
    for id in ids {
        assert_eq!(armor(&sim, id), 0);
    }
}

#[test]
fn hostile_armor_family_uses_only_the_strongest_in_range_and_falls_back_without_stacking() {
    let mut sim = simulation(1);
    source(&mut sim, 20, 871, 872, -300);
    let strong = source(&mut sim, 21, 873, 872, -700);
    source(&mut sim, 22, 875, 876, -100); // An independent native buff family stacks.
    let target = sim.spawn_unit(unit(1, 30, 0, AttackDelivery::Melee));
    sim.step();
    assert_eq!(armor(&sim, target), -800);
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 2);
    sim.world
        .get_mut::<Position>(entity(&sim, strong))
        .unwrap()
        .0
        .x = 100 * SUBUNITS_PER_WORLD_UNIT;
    sim.step();
    assert_eq!(armor(&sim, target), -400);
    let mut other = wire_restored(&sim, 4);
    sim.world
        .get_mut::<Position>(entity(&sim, target))
        .unwrap()
        .0
        .x = 200 * SUBUNITS_PER_WORLD_UNIT;
    other
        .world
        .get_mut::<Position>(entity(&other, target))
        .unwrap()
        .0
        .x = 200 * SUBUNITS_PER_WORLD_UNIT;
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(armor(&sim, target), 0);
}

#[test]
fn zero_linger_hostile_armor_expires_on_the_next_boundary_after_source_death() {
    let mut sim = simulation(1);
    let emitter = source(&mut sim, 20, 871, 872, -300);
    let target = sim.spawn_unit(unit(1, 30, 0, AttackDelivery::Melee));
    sim.step();
    assert_eq!(armor(&sim, target), -300);
    sim.world
        .get_mut::<Health>(entity(&sim, emitter))
        .unwrap()
        .current = 0;
    let mut other = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(armor(&sim, target), 0);
}
