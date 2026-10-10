use super::*;
use crate::components::{NativeAction, NativeHealProfile};
fn heal() -> NativeHealProfile {
    NativeHealProfile {
        amount: 17,
        targets: AttackTargetMask::GROUND_UNITS.union(AttackTargetMask::AIR_UNITS),
        organic_only: true,
        nonancient_only: true,
        affects_invulnerable: true,
        resume_attack_ticks: 4,
    }
}
#[test]
fn native_heal_effect_and_ground_trigger_have_distinct_classification_rules() {
    let mut sim = simulation(1);
    let air = sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            classifications: UnitClassifications {
                invulnerable: true,
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    units[0].health = 980;
    assert!(apply_ability_effect_to_unit(
        &mut units[0],
        AbilityEffect::Heal(heal()),
        Team(0),
        1,
        sim.combat_rules.damage_rules
    ));
    assert_eq!(units[0].health, 997);
    for (ancient, mechanical, team) in [(true, false, 0), (false, true, 0), (false, false, 1)] {
        units[0].health = 980;
        units[0].classifications.ancient = ancient;
        units[0].mechanical = mechanical;
        assert!(!apply_ability_effect_to_unit(
            &mut units[0],
            AbilityEffect::Heal(heal()),
            Team(team),
            1,
            sim.combat_rules.damage_rules
        ));
        assert_eq!(units[0].health, 980);
    }
    assert_eq!(units[0].id, air);
}
#[test]
fn heal_commits_once_and_restores_pending_attack_resume_across_workers() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(200),
                mana_cost: 3,
                cooldown_ticks: 20,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                effect: AbilityEffect::Heal(heal()),
            },
        },
    );
    let ally = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    sim.world
        .get_mut::<Health>(entity(&sim, ally))
        .unwrap()
        .current = 980;
    sim.step();
    assert_eq!(sim.unit(ally).unwrap().health, 997);
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(7));
    assert!(sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::ResumeAttack { .. })
    )));
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..3 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
        assert!(sim.world.iter_entities().any(|e| matches!(
            e.get::<NativeAction>(),
            Some(NativeAction::ResumeAttack { .. })
        )));
    }
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::ResumeAttack { .. })
    )));
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(7));
}

#[test]
fn automatic_heal_ignores_more_wounded_air_and_ancient_allies() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(200),
                mana_cost: 3,
                cooldown_ticks: 20,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                effect: AbilityEffect::Heal(heal()),
            },
        },
    );
    let ground = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    let air = sim.spawn_unit_with_properties(
        unit(0, 45, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let ancient = sim.spawn_unit_with_properties(
        unit(0, 50, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                ancient: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    for (id, hp) in [(ground, 980), (air, 500), (ancient, 400)] {
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = hp;
    }
    sim.step();
    assert_eq!(sim.unit(ground).unwrap().health, 997);
    assert_eq!(sim.unit(air).unwrap().health, 500);
    assert_eq!(sim.unit(ancient).unwrap().health, 400);
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(7));
}
