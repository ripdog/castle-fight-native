use super::*;

fn aura() -> PassiveUnitEffect {
    PassiveUnitEffect::Aura(crate::components::AuraEffectProfile {
        self_only: true,
        melee_life_steal_per_10k: 5000,
        melee_damage_return_per_10k: 0,
        ability: AbilityId(930),
        radius: 100 * SUBUNITS_PER_WORLD_UNIT,
        damage_bonus_per_10k: 0,
        armor_bonus_per_100: 0,
        mana_regeneration_per_second_per_10k: 0,
        affects_melee: true,
        affects_ranged: false,
        linger_ticks: 1,
        suspend_during_spell_cooldown: false,
    })
}

#[test]
fn self_life_steal_heals_post_armor_damage_only_on_melee_primary_unit_hits() {
    for (delivery, immune, mechanical) in [
        (AttackDelivery::Melee, false, false),
        (AttackDelivery::Melee, true, false),
        (AttackDelivery::Melee, false, true),
        (AttackDelivery::RangedInstant, false, false),
        (
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 100 * SUBUNITS_PER_WORLD_UNIT,
            },
            false,
            false,
        ),
    ] {
        let mut sim = simulation(1);
        let source = sim.spawn_unit_with_properties(
            unit(0, 20, 100, delivery),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(aura()),
                ..Default::default()
            },
        );
        let ally = sim.spawn_unit(unit(0, 22, 0, AttackDelivery::Melee));
        let victim = sim.spawn_unit_with_properties(
            unit(1, 30, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical,
                build_time_ticks: Some(1),
                armor: ArmorProfile::new(ArmorType::Large, 7),
                classifications: UnitClassifications {
                    spell_immune: immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.world
            .get_mut::<Health>(entity(&sim, source))
            .unwrap()
            .current = 500;
        for _ in 0..3 {
            sim.step();
        }
        let received = 1000 - sim.unit(victim).unwrap().health;
        assert!(received > 0 && received < 100);
        assert_eq!(
            sim.unit(source).unwrap().health,
            500 + if delivery == AttackDelivery::Melee && !immune {
                received / 2
            } else {
                0
            }
        );
        assert_eq!(sim.unit(ally).unwrap().status.armor_modifier_count, 0);
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..4 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
    }
}

#[test]
fn life_steal_does_not_heal_structure_damage_or_revive_a_source_killed_by_return_damage() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 100, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(aura()),
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 500;
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(2, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    for _ in 0..3 {
        sim.step();
    }
    assert_eq!(sim.unit(source).unwrap().health, 500);

    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 100, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(aura()),
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 1;
    sim.spawn_unit_with_properties(
        unit(1, 30, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::SpikedShell(
                crate::SpikedShellProfile {
                    ability: AbilityId(931),
                    returned_damage_per_10k: 10000,
                    received_damage_per_10k: 10000,
                    armor_bonus_per_100: 0,
                },
            )),
            ..Default::default()
        },
    );
    for _ in 0..3 {
        sim.step();
    }
    assert!(sim.unit(source).is_none_or(|u| u.health <= 0));
}
