use super::*;
use crate::building_mechanics::SilverGladeProfile;
use crate::{MapVersion, NativeBuffIdentity};
fn blessing() -> SilverGladeProfile {
    SilverGladeProfile {
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        trigger_targets: AttackTargetMask::ALL,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        trigger_mechanical_only: true,
        effect: AbilityId(201),
        excluded: AbilityId(202),
        radius: 30 * SUBUNITS_PER_WORLD_UNIT,
        health: 17,
        mana: 5,
        armor_per_100: 300,
        buff: NativeBuffIdentity {
            rawcode: u32::from_be_bytes(*b"TEST"),
            positive: true,
            stealable: true,
            organic_only: false,
        },
        duration_ticks: 9,
        hero_duration_ticks: 9,
    }
}
#[test]
fn team_charge_fires_at_attack_start_before_cancelled_windup() {
    let mut sim = simulation(1);
    let victim = sim.spawn_unit_with_properties(
        unit(0, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let attacker = sim.spawn_unit_with_properties(
        unit(1, 50, 30, AttackDelivery::Melee),
        UnitGameplayProperties {
            action_timing: ActionTimingProfile {
                primary_attack_ticks: 8,
                primary_attack_point_ticks: 4,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, victim))
        .unwrap()
        .current = 980;
    sim.add_silver_charge(Team(0), blessing());
    for _ in 0..8 {
        sim.step();
        if sim.unit(attacker).unwrap().status.pending_attack.is_some() {
            break;
        }
    }
    assert_eq!(sim.unit(victim).unwrap().health, 997);
    assert_eq!(sim.unit(victim).unwrap().status.armor_modifier_count, 1);
    assert!(sim.unit(attacker).unwrap().status.pending_attack.is_some());
    sim.world
        .get_mut::<Health>(entity(&sim, attacker))
        .unwrap()
        .current = 0;
    sim.step();
    assert_eq!(sim.unit(victim).unwrap().health, 997);
    assert_eq!(charges(&sim), [0, 0]);
}
fn charges(sim: &Simulation) -> [u32; 2] {
    sim.world
        .iter_entities()
        .find_map(|e| match e.get::<crate::components::NativeAction>() {
            Some(crate::components::NativeAction::SilverGlade(s)) => Some(s.charges),
            _ => None,
        })
        .unwrap()
}
#[test]
fn stored_team_charges_restore_and_native_area_payload_refreshes_one_family() {
    let mut sim = simulation(1);
    let victim = sim.spawn_unit_with_properties(
        unit(0, 80, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let air =
        sim.spawn_unit_with_spellcasting(unit(0, 90, 0, AttackDelivery::Melee), mana_profile(2));
    sim.world
        .entity_mut(entity(&sim, air))
        .insert(MovementClass::Air);
    let excluded = sim.spawn_unit_with_properties(
        unit(0, 95, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let distant = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    for id in [victim, air, excluded, distant] {
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = 980;
    }
    sim.add_silver_charge(Team(0), blessing());
    sim.add_silver_charge(Team(0), blessing());
    sim.add_silver_charge(Team(1), blessing());
    let mut restored = wire_restored(&sim, 4);
    let proc = |sim: &mut Simulation| {
        let mut units = sim.snapshot_units();
        let i = units.iter().position(|u| u.id == victim).unwrap();
        let mut hp = units.iter().map(|u| u.health).collect::<Vec<_>>();
        sim.silver_attack_attempt(i, &mut units, &mut hp, 0);
        (units, hp)
    };
    let (mut units, mut hp) = proc(&mut sim);
    let (other, other_hp) = proc(&mut restored);
    assert_eq!(hp, other_hp);
    assert_eq!(units[1].status, other[1].status);
    assert_eq!(hp, vec![997, 997, 980, 980]);
    assert_eq!(units[1].mana_current, Some(7));
    let i = units.iter().position(|u| u.id == victim).unwrap();
    sim.silver_attack_attempt(i, &mut units, &mut hp, 1);
    assert_eq!(units[1].status.armor_modifier_count, 1);
    assert_eq!(charges(&sim), [0, 1]);
    sim.reset_silver_charges();
    assert_eq!(charges(&sim), [0, 0]);
}

#[test]
fn native_building_order_commits_charges_without_a_friendly_effect_recipient() {
    for structure in [false, true] {
        let mut sim = simulation(1);
        let mut p = blessing();
        p.trigger_targets = AttackTargetMask::GROUND_AND_BUILDINGS;
        let caster = sim.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(1, 1, 1, 1),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: Some(SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 10,
                    starting: 10,
                    regen_per_tick_per_10k: 0,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(203),
                    mana_cost: 3,
                    cooldown_ticks: 20,
                    range: 200 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                    effect: AbilityEffect::SilverGlade(p),
                },
            }),
        });
        sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
        sim.step();
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(10));
        if structure {
            sim.spawn_building(BuildingSpawn {
                team: Team(1),
                footprint: BuildingFootprint::new(3, 1, 1, 1),
                health: 1000,
                production: None,
                attack: None,
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
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(7));
        assert_eq!(charges(&sim), [1, 0]);
    }
}
