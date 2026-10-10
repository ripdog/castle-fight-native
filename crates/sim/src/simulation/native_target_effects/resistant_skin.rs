use super::*;

#[test]
fn resistant_skin_uses_hostile_hero_duration_without_hero_armor_or_spell_immunity() {
    let mut sim = simulation(1);
    sim.spawn_unit_with_properties(
        unit(1, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                resistant_skin: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    let armor = units[0].armor;
    assert!(!units[0].classifications.hero);
    assert!(!units[0].classifications.spell_immune);
    assert!(units[0].classifications.uses_hero_spell_rules());
    assert!(apply_ability_effect_to_unit(
        &mut units[0],
        AbilityEffect::FaerieFire {
            native_buff: None,
            modifier: ModifierId(123),
            armor_reduction_per_100: 200,
            duration_ticks: 50,
            hero_duration_ticks: 7,
        },
        Team(0),
        3,
        sim.combat_rules.damage_rules
    ));
    assert_eq!(units[0].status.armor_modifiers[0].expires_tick, 10);
    assert_eq!(units[0].armor, armor);
    let effect = AbilityEffect::Damage { amount: 10 };
    assert!(apply_ability_effect_to_unit(
        &mut units[0],
        effect,
        Team(0),
        4,
        sim.combat_rules.damage_rules
    ));
    assert!(units[0].health < 1000);
    apply_ability_effect_to_unit(
        &mut units[0],
        AbilityEffect::StatBuff {
            modifier: ModifierId(124),
            buff: crate::NativeBuffIdentity {
                rawcode: 124,
                positive: true,
                stealable: true,
                organic_only: false,
            },
            armor_bonus_per_100: 100,
            damage_bonus_per_10k: 0,
            regeneration_per_second_per_10k: 0,
            duration_ticks: 50,
            hero_duration_ticks: 7,
            autocast_range: 10,
        },
        Team(1),
        5,
        sim.combat_rules.damage_rules,
    );
    assert_eq!(
        units[0].status.armor_modifiers[1].expires_tick, 55,
        "positive buffs keep normal duration"
    );
}
