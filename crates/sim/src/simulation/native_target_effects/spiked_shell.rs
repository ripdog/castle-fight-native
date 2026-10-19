use super::*;
use crate::components::SpikedShellProfile;

fn shell(ability: u32, received: u32, returned: u32, armor: i32) -> PassiveUnitEffect {
    PassiveUnitEffect::SpikedShell(SpikedShellProfile {
        ability: AbilityId(ability),
        received_damage_per_10k: received,
        returned_damage_per_10k: returned,
        armor_bonus_per_100: armor,
    })
}

#[test]
fn shell_melee_reduction_and_return_have_independent_armor_and_no_recursion() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 100, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Large, 7),
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            passive_effects: PassiveUnitEffects::from_slice(&[
                shell(501, 3000, 9000, 0),
                PassiveUnitEffect::SpellResistance(
                    crate::components::SpellResistanceEffectProfile {
                        ability: AbilityId(505),
                        damage_taken_per_10k: 5000,
                    },
                ),
            ]),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Large, 10),
            passive_effects: PassiveUnitEffects::single(shell(502, 8000, 6000, 0)),
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<StatusState>(entity(&sim, source))
        .unwrap()
        .native_grants
        .insert(crate::NativeAbilityGrant {
            ability: AbilityId(506),
            effect: crate::NativeAbilityGrantEffect::HardenedSkin {
                chance_per_10k: 10_000,
                ignored_damage: 1000,
                minimum_damage: 0,
            },
        });
    sim.step();
    let mut copy = wire_restored(&sim, 4);
    assert_eq!(
        {
            sim.step();
            sim.checksum()
        },
        {
            copy.step();
            copy.checksum()
        }
    );
    let rules = DamageRules::default();
    let base = rules.apply_attack(
        100,
        DamageType::Normal,
        ArmorProfile::new(ArmorType::Large, 10),
    );
    let incoming = (base * 8000 + 5000) / 10_000;
    let returned = (base * 6000 + 5000) / 10_000;
    let returned = rules.apply_attack(
        returned,
        DamageType::Spells,
        ArmorProfile::new(ArmorType::Large, 7),
    );
    assert_eq!(sim.unit(target).unwrap().health, 1000 - incoming);
    assert_eq!(
        sim.unit(source).unwrap().health,
        1000 - scale_damage_per_10k(returned, 5000)
    );
    assert_eq!(sim.unit(source).unwrap().status.hardened_skin_sequence, 0);
}

#[test]
fn shell_does_not_reduce_ranged_spell_cleave_or_defensive_packets() {
    for kind in [
        WeaponDamageKind::Ranged,
        WeaponDamageKind::Cleave,
        WeaponDamageKind::Defensive,
    ] {
        let mut sim = simulation(1);
        let source = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
        sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(shell(502, 0, 20_000, 0)),
                ..Default::default()
            },
        );
        let mut units = sim.snapshot_units();
        let positions = units.iter().map(|u| u.position).collect::<Vec<_>>();
        let mut health = vec![1000; 2];
        let mut attackers = vec![None; 2];
        let mut alerts = Vec::new();
        sim.apply_damage_to_target(
            TargetIndex::Unit(1),
            WeaponDamageEvent {
                source_id: source,
                damage: 100,
                damage_type: DamageType::Spells,
                completed_tick: 1,
                kind,
            },
            false,
            DamageTargetState {
                match_seed: 0,
                damage_rules: DamageRules::default(),
                units: &mut units,
                buildings: &[],
                unit_positions: &positions,
                unit_health: &mut health,
                building_health: &mut [],
                attackers_this_tick: &mut attackers,
                next_defense_alerts: &mut alerts,
                navigation_cell_size: sim.config.navigation_cell_size,
            },
        );
        assert_eq!(health, [1000, 900]);
    }
}

#[test]
fn shells_stack_returns_from_one_armor_basis_and_reduce_independently() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit(unit(0, 20, 100, AttackDelivery::Melee));
    let target = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                shell(501, 5000, 6000, 100),
                shell(502, 8000, 2000, 100),
            ]),
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    let base = DamageRules::default().apply_attack(
        100,
        DamageType::Normal,
        ArmorProfile::new(ArmorType::Unarmored, 2),
    );
    let incoming = (((base * 5000 + 5000) / 10_000) * 8000 + 5000) / 10_000;
    let returned = (base * 6000 + 5000) / 10_000 + (base * 2000 + 5000) / 10_000;
    assert_eq!(sim.unit(target).unwrap().health, 1000 - incoming);
    assert_eq!(sim.unit(source).unwrap().health, 1000 - returned);
}

#[test]
fn shell_zero_negative_and_fatal_packets_do_not_add_healing_or_cancel_a_return() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit(unit(0, 20, 2000, AttackDelivery::Melee));
    let target = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(shell(502, 10_000, 5000, 0)),
            ..Default::default()
        },
    );
    let snapshot = sim.snapshot_units();
    for amount in [-10, 0] {
        let result = super::super::spiked_shell::damage(&snapshot[1], amount, true);
        assert_eq!(result.incoming, amount);
        assert_eq!(result.count, 0);
    }
    sim.step();
    sim.step();
    assert!(sim.unit(target).is_none());
    assert!(sim.unit(source).is_none());
}
