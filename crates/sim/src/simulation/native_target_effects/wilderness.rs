use super::*;
use crate::building_mechanics::WildernessProfile;
use crate::{
    MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect as Grant, NativeAbilityGrants,
};
fn wilderness(chance: u16) -> WildernessProfile {
    WildernessProfile {
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        trigger_mechanical_only: true,
        grants: [
            NativeAbilityGrant {
                ability: AbilityId(301),
                effect: Grant::SpellResistance(5000),
            },
            NativeAbilityGrant {
                ability: AbilityId(302),
                effect: Grant::HardenedSkin {
                    chance_per_10k: chance,
                    ignored_damage: 7,
                    minimum_damage: 2,
                },
            },
            NativeAbilityGrant {
                ability: AbilityId(303),
                effect: Grant::AuraMarker,
            },
        ],
        exclusions: [
            AbilityId(304),
            AbilityId(305),
            AbilityId(306),
            AbilityId(307),
        ],
        locust: AbilityId(308),
    }
}
fn caster(sim: &mut Simulation, p: WildernessProfile) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(309),
                mana_cost: 3,
                cooldown_ticks: 20,
                range: 200 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::Wilderness(p),
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

#[test]
fn permanent_wilderness_exclusion_only_skips_resistance_and_never_retries_skin() {
    let p = wilderness(10000);
    for excluded in [false, true] {
        let mut sim = simulation(1);
        caster(&mut sim, p);
        let ally = sim.spawn_unit_with_properties(
            unit(0, 40, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical: true,
                build_time_ticks: Some(4),
                repair_time_ticks: Some(4),
                movement_class: MovementClass::Air,
                classifications: UnitClassifications {
                    combat_sapper: true,
                    invulnerable: true,
                    spell_immune: true,
                    hero: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.spawn_unit(unit(0, 50, 0, AttackDelivery::Melee)); // Non-sapper.
        sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    combat_sapper: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let mut units = sim.snapshot_units();
        if excluded {
            units[0].status.native_grants.insert(NativeAbilityGrant {
                ability: p.exclusions[0],
                effect: Grant::AuraMarker,
            });
        }
        let src = source(&mut sim);
        assert!(sim.cast_wilderness(src, AbilityId(309), p, 0, &mut units));
        assert_eq!(units[0].id, ally);
        assert_eq!(
            units[0].status.native_grants.contains(p.grants[0].ability),
            !excluded
        );
        assert!(units[0].status.native_grants.contains(p.grants[1].ability));
        assert!(units[0].status.native_grants.contains(p.grants[2].ability));
        assert!(!sim.cast_wilderness(src, AbilityId(309), p, 1, &mut units));
        assert_eq!(sim.last_ability_casts.last().unwrap().target_position, None);
        assert!(units[1].status.native_grants.iter().next().is_none());
        assert!(units[2].status.native_grants.iter().next().is_none());
    }
}
#[test]
fn native_wilderness_primary_commits_resources_even_with_no_grant_recipient() {
    for structure in [false, true] {
        let mut sim = simulation(1);
        let id = caster(&mut sim, wilderness(10000));
        sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
        sim.step();
        assert_eq!(sim.building(id).unwrap().mana_current, Some(10));
        if structure {
            sim.spawn_building(BuildingSpawn {
                team: Team(1),
                footprint: BuildingFootprint::new(3, 1, 1, 1),
                health: 1000,
                attack: None,
                production: None,
                spellcasting: None,
            });
        } else {
            sim.spawn_unit_with_properties(
                unit(1, 100, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    mechanical: true,
                    build_time_ticks: Some(4),
                    repair_time_ticks: Some(4),
                    ..Default::default()
                },
            );
        }
        for _ in 0..3 {
            sim.step();
        }
        assert_eq!(sim.building(id).unwrap().mana_current, Some(7));
        assert!(
            sim.world
                .iter_entities()
                .filter_map(|e| e.get::<StatusState>())
                .all(|s| s.native_grants.iter().next().is_none())
        );
    }
}
#[test]
fn permanent_skin_only_reduces_attacks_with_floor_and_restored_random_sequence() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    let e = entity(&sim, target);
    sim.world.get_mut::<StatusState>(e).unwrap().native_grants =
        NativeAbilityGrants::from_slice(&wilderness(10000).grants);
    let mut u = sim.snapshot_units()[0];
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut u, 20, SimId(99), 0),
        13
    );
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut u, 5, SimId(99), 0),
        2
    );
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut u, 1, SimId(99), 0),
        1
    );
    assert_eq!(spell_damage_after_defend(u, 20, 0), 10); // Spell resistance, no flat skin reduction.
    u.abilities_disabled = true;
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut u, 20, SimId(99), 0),
        20
    );
    assert_eq!(spell_damage_after_defend(u, 20, 0), 20);
    u.abilities_disabled = false;
    u.status.native_grants = NativeAbilityGrants::from_slice(&wilderness(5000).grants);
    *sim.world.get_mut::<StatusState>(e).unwrap() = u.status;
    let mut restored = wire_restored(&sim, 4);
    let mut v = restored.snapshot_units()[0];
    let mut results = Vec::new();
    for _ in 0..64 {
        let damage = super::super::night_elf::hardened_skin_after_armor(&mut u, 20, SimId(99), 0);
        assert_eq!(
            damage,
            super::super::night_elf::hardened_skin_after_armor(&mut v, 20, SimId(99), 0)
        );
        results.push(damage);
    }
    assert!(results.contains(&13) && results.contains(&20));
    assert_eq!(u.status, v.status);
}
#[test]
fn ordinary_landed_attack_runs_permanent_skin_and_restores_across_workers() {
    let mut sim = simulation(1);
    sim.spawn_unit(unit(1, 20, 20, AttackDelivery::Melee));
    let target = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    sim.world
        .get_mut::<StatusState>(entity(&sim, target))
        .unwrap()
        .native_grants = NativeAbilityGrants::from_slice(&wilderness(10000).grants);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..5 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.unit(target).unwrap().health, 987);
    assert_eq!(sim.unit(target).unwrap().status.hardened_skin_sequence, 1);
}

#[test]
fn wilderness_rejects_locust_partial_grants_and_dead_sappers() {
    for marker in [AbilityId(308), AbilityId(301), AbilityId(302)] {
        let mut sim = simulation(1);
        caster(&mut sim, wilderness(10000));
        sim.spawn_unit_with_properties(
            unit(0, 40, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    combat_sapper: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let mut units = sim.snapshot_units();
        units[0].status.native_grants.insert(NativeAbilityGrant {
            ability: marker,
            effect: Grant::AuraMarker,
        });
        let src = source(&mut sim);
        assert!(!sim.cast_wilderness(src, AbilityId(309), wilderness(10000), 0, &mut units));
        units[0].status.native_grants = NativeAbilityGrants::default();
        units[0].health = 0;
        assert!(!sim.cast_wilderness(src, AbilityId(309), wilderness(10000), 1, &mut units));
    }
}
