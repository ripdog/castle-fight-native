use super::*;

#[test]
fn native_orb_class_probabilities_use_actual_hero_then_summoned_then_ordinary_categories() {
    for (hero_chance, summoned_chance, normal_chance) in [(0, 10000, 0), (10000, 0, 10000)] {
        for (hero, summoned) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut sim = simulation(1);
            sim.spawn_unit_with_properties(
                unit(0, 20, 0, AttackDelivery::RangedInstant),
                UnitGameplayProperties {
                    passive_effects: PassiveUnitEffects::single(
                        PassiveUnitEffect::TriggeredSpellProc(crate::TriggeredSpellProcProfile {
                            ability: AbilityId(951),
                            chance_per_10k: normal_chance,
                            hero_chance_per_10k: Some(hero_chance),
                            summoned_chance_per_10k: Some(summoned_chance),
                            targets: AttackTargetMask::AIR_AND_GROUND,
                            effect: TriggeredAttackEffect::ChainLightning(
                                crate::ChainLightningEffectProfile {
                                    ability: AbilityId(952),
                                    initial_damage: 9,
                                    maximum_targets: 1,
                                    jump_radius: 0,
                                    damage_reduction_per_10k: 0,
                                    targets: AttackTargetMask::AIR_AND_GROUND,
                                },
                            ),
                        }),
                    ),
                    ..Default::default()
                },
            );
            let target = sim.spawn_unit_with_properties(
                unit(1, 50, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    classifications: UnitClassifications {
                        hero,
                        summoned,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            sim.step();
            let mut other = wire_restored(&sim, 4);
            assert_eq!(sim.step().checksum, other.step().checksum);
            let chance = if hero {
                hero_chance
            } else if summoned {
                summoned_chance
            } else {
                normal_chance
            };
            assert_eq!(
                sim.unit(target).unwrap().health,
                1000 - if chance > 0 { 9 } else { 0 }
            );
        }
    }
}
