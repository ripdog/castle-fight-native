use super::*;
use crate::{CorpseDefinitionId, CorpseProfile, DeathExplosionProfile};

fn profile() -> DeathExplosionProfile {
    DeathExplosionProfile {
        ability: AbilityId(931),
        full_radius: 6 * SUBUNITS_PER_WORLD_UNIT,
        full_damage: 40,
        partial_radius: 12 * SUBUNITS_PER_WORLD_UNIT,
        partial_damage: 17,
        delay_ticks: 4,
        targets: AttackTargetMask::GROUND_AND_BUILDINGS,
    }
}
fn source(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 30, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::DeathExplosion(
                profile(),
            )),
            corpse: Some(CorpseProfile {
                definition: CorpseDefinitionId(99),
                decay_start_ticks: 2,
                lifetime_ticks: Some(100),
            }),
            ..Default::default()
        },
    )
}
fn kill(sim: &mut Simulation, source: SimId) {
    sim.world
        .get_mut::<Health>(entity(sim, source))
        .unwrap()
        .current = 1;
    sim.spawn_unit(unit(1, 31, 40, AttackDelivery::Melee));
    sim.step(); // Birth tick precedes the real fatal weapon event.
}
#[test]
fn native_death_explosion_saves_post_death_delay_and_position_without_destroying_ordinary_corpse() {
    let mut sim = simulation(1);
    let source = source(&mut sim);
    let full = sim.spawn_unit(unit(1, 36, 0, AttackDelivery::Melee));
    let partial = sim.spawn_unit(unit(1, 42, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 43, 0, AttackDelivery::Melee));
    kill(&mut sim, source);
    sim.step();
    assert!(sim.unit(source).is_none());
    assert!(
        sim.world
            .iter_entities()
            .any(|e| e.get::<Corpse>().is_some_and(|c| c.source_unit == source))
    );
    // Synthetic corpse identity is not in the production bundle; the timer is independently saved.
    let corpses = sim
        .world
        .iter_entities()
        .filter_map(|e| e.get::<Corpse>().map(|_| e.id()))
        .collect::<Vec<_>>();
    for e in corpses {
        sim.world.despawn(e);
    }
    let mut other = wire_restored(&sim, 4);
    for _ in 0..3 {
        assert_eq!(sim.step().checksum, other.step().checksum);
        assert_eq!(sim.unit(full).unwrap().health, 1000);
    }
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(sim.unit(full).unwrap().health, 960);
    assert_eq!(sim.unit(partial).unwrap().health, 983);
    assert_eq!(sim.unit(outside).unwrap().health, 1000);
    sim.step();
    assert_eq!(sim.unit(full).unwrap().health, 960);
}
#[test]
fn native_death_explosion_enumerates_recipients_at_delivery_and_admits_magic_immune_mechanical_bodies()
 {
    let mut sim = simulation(1);
    let source = source(&mut sim);
    let leaves = sim.spawn_unit(unit(1, 35, 0, AttackDelivery::Melee));
    let enters = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let air = sim.spawn_unit_with_properties(
        unit(1, 35, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let protected = sim.spawn_unit_with_properties(
        unit(1, 35, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let ally = sim.spawn_unit(unit(0, 35, 0, AttackDelivery::Melee));
    kill(&mut sim, source);
    sim.step();
    sim.world
        .get_mut::<Position>(entity(&sim, leaves))
        .unwrap()
        .0
        .x = 60 * SUBUNITS_PER_WORLD_UNIT;
    sim.world
        .get_mut::<Position>(entity(&sim, enters))
        .unwrap()
        .0
        .x = 35 * SUBUNITS_PER_WORLD_UNIT;
    for _ in 0..4 {
        sim.step();
    }
    assert_eq!(sim.unit(enters).unwrap().health, 960);
    for id in [leaves, air, protected, ally] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
}
#[test]
fn removed_without_death_never_queues_native_death_damage() {
    let mut sim = simulation(1);
    let source = source(&mut sim);
    let target = sim.spawn_unit(unit(1, 35, 0, AttackDelivery::Melee));
    sim.world.despawn(entity(&sim, source));
    for _ in 0..6 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
}
#[test]
fn native_death_explosion_hits_structures_and_spell_armor_type_without_numeric_armor_or_weapon_hooks()
 {
    let mut sim = simulation(1);
    let source = source(&mut sim);
    let hero = sim.spawn_unit_with_properties(
        unit(1, 35, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Hero, 30),
            classifications: UnitClassifications {
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let structure = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(34, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    kill(&mut sim, source);
    sim.step();
    for _ in 0..4 {
        sim.step();
    }
    assert_eq!(
        sim.unit(hero).unwrap().health,
        1000 - sim
            .combat_rules
            .damage_rules
            .apply_spell(40, ArmorType::Hero)
    );
    assert!(sim.building(structure).unwrap().health < 1000);
}
