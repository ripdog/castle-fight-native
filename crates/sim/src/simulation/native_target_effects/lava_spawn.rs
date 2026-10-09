use super::*;
use crate::{ContentIdentity, LavaSpawnProfile};
fn profile() -> LavaSpawnProfile {
    LavaSpawnProfile {
        ability: AbilityId(91),
        body_rawcode: 0,
        child_rawcode: u32::from_be_bytes(*b"hfoo"),
        attacks_to_split: 2,
        split_delay_ticks: 3,
        maximum_splits: 2,
        healing_per_10k: 2000,
        order_refresh_ticks: 2,
        targets: AttackTargetMask::AIR_AND_GROUND,
    }
}
fn source(sim: &mut Simulation, delivery: AttackDelivery) -> SimId {
    let mut spawn = unit(0, 30, 0, delivery);
    spawn.attack.cooldown_ticks = 1;
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            content: Some(ContentIdentity {
                map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
                rawcode: u32::from_be_bytes(*b"hfoo"),
                name: "synthetic split source",
            }),
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::LavaSpawn(profile())),
            ..Default::default()
        },
    )
}
#[test]
fn lava_split_waits_heals_and_replaces_child_using_default_health_and_provenance() {
    let mut sim = simulation(1);
    let source = source(&mut sim, AttackDelivery::RangedInstant);
    let e = entity(&sim, source);
    sim.world.get_mut::<Health>(e).unwrap().current = 400;
    sim.world.get_mut::<StatusState>(e).unwrap().spawn_building = Some(SimId(900));
    sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    sim.step();
    let state = sim.unit(source).unwrap().status.lava_spawn.unwrap();
    assert_eq!(state.split_tick, Some(5));
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..2 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.units().len(), 2);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.unit(source).unwrap().health, 600);
    let child = sim
        .units()
        .into_iter()
        .find(|unit| unit.id != source && unit.team == Team(0))
        .unwrap();
    assert_eq!(child.content.unwrap().rawcode, profile().child_rawcode);
    assert_eq!(child.health, child.health_max);
    assert!(child.status.lava_spawn.is_none());
    assert!(!child.classifications.summoned);
    assert_eq!(child.status.spawn_building, Some(SimId(900)));
    assert_eq!(child.status.summon_order_refresh_tick, Some(7));
    let mut restored = wire_restored(&sim, 3);
    for _ in 0..12 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(
        sim.unit(source).unwrap().status.lava_spawn.unwrap().splits,
        2
    );
}
#[test]
fn lava_split_counts_landed_zero_damage_hits_and_rejects_misses_and_buildings() {
    for evasive in [false, true] {
        let mut sim = simulation(1);
        let source = source(&mut sim, AttackDelivery::RangedInstant);
        sim.spawn_unit_with_properties(
            unit(1, 70, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: if evasive {
                    PassiveUnitEffects::single(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                        ability: AbilityId(8),
                        chance_per_10k: 10_000,
                    }))
                } else {
                    PassiveUnitEffects::EMPTY
                },
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(source)
                .unwrap()
                .status
                .lava_spawn
                .unwrap()
                .landed_attacks,
            u16::from(!evasive)
        );
    }
    let mut sim = simulation(1);
    let source = source(&mut sim, AttackDelivery::RangedInstant);
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(3, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    sim.step();
    sim.step();
    sim.step();
    assert_eq!(
        sim.unit(source)
            .unwrap()
            .status
            .lava_spawn
            .unwrap()
            .landed_attacks,
        0
    );
}
#[test]
fn lava_split_is_cancelled_by_source_death_and_counter_waits_for_missile_impact() {
    let mut sim = simulation(1);
    let source = source(
        &mut sim,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        },
    );
    let attacker = sim.spawn_unit(unit(1, 90, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(
        sim.unit(source)
            .unwrap()
            .status
            .lava_spawn
            .unwrap()
            .landed_attacks,
        0
    );
    for _ in 0..7 {
        sim.step();
    }
    assert!(
        sim.unit(source)
            .unwrap()
            .status
            .lava_spawn
            .unwrap()
            .split_tick
            .is_some()
    );
    let e = entity(&sim, attacker);
    sim.world.get_mut::<AttackProfile>(e).unwrap().damage = 2000;
    sim.world.get_mut::<AttackCooldown>(e).unwrap().remaining = 0;
    for _ in 0..8 {
        sim.step();
    }
    assert!(sim.units().iter().all(|unit| unit.team != Team(0)));
}
