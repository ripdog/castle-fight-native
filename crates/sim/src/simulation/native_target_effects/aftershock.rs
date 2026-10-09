use super::*;
use crate::{HealthScaledAftershockProfile, PulverizeEffectProfile};

fn aftershock() -> PassiveUnitEffect {
    PassiveUnitEffect::HealthScaledAftershock(HealthScaledAftershockProfile {
        marker: AbilityId(801),
        dispatcher: AbilityId(802),
        maximum_damage: 80,
    })
}

fn source(sim: &mut Simulation, effects: PassiveUnitEffects) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                melee_attacker: true,
                ..Default::default()
            },
            passive_effects: effects,
            damage_type: DamageType::Siege,
            ..Default::default()
        },
    )
}

#[test]
fn aftershock_samples_current_source_life_and_only_registered_victims_without_spell_reduction() {
    let mut sim = simulation(1);
    let source = source(&mut sim, PassiveUnitEffects::single(aftershock()));
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                spell_immune: true,
                ..Default::default()
            },
            armor: ArmorProfile {
                armor_type: ArmorType::Large,
                armor_points: 100,
            },
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::SpellResistance(
                crate::components::SpellResistanceEffectProfile {
                    ability: AbilityId(803),
                    damage_taken_per_10k: 0,
                },
            )),
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    let source_index = find_unit_index(&units, source).unwrap();
    let victim_index = find_unit_index(&units, victim).unwrap();
    let mut health: Vec<_> = units.iter().map(|u| u.health).collect();
    health[source_index] = 250;
    let rules = DamageRules::warcraft_frozen_throne();
    let invoke = |units: &mut [UnitSnapshot], health: &mut [i32]| {
        super::super::source_damage_runtime::apply_aftershock(
            source,
            victim_index,
            super::super::source_damage_runtime::SourceDamageState {
                units,
                health,
                rules,
                tick: 1,
            },
        )
    };
    invoke(&mut units, &mut health);
    let damage = rules.apply_attack_with_armor_per_100(20, DamageType::Siege, ArmorType::Large, 0);
    assert_eq!(health[victim_index], 1000 - damage);
    units[victim_index].classifications.combat_sapper = false;
    invoke(&mut units, &mut health);
    assert_eq!(health[victim_index], 1000 - damage);
    units[victim_index].classifications.combat_sapper = true;
    health[source_index] = 1000;
    invoke(&mut units, &mut health);
    assert_eq!(health[victim_index], 1000 - damage * 5);
}

#[test]
fn aftershock_runs_on_pulverize_and_zero_damage_parent_hits_and_restores() {
    let mut sim = simulation(1);
    source(
        &mut sim,
        PassiveUnitEffects::from_slice(&[
            aftershock(),
            PassiveUnitEffect::Pulverize(PulverizeEffectProfile {
                ability: AbilityId(804),
                chance_per_10k: 10_000,
                damage: 20,
                full_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
                half_radius: 70 * SUBUNITS_PER_WORLD_UNIT,
                targets: AttackTargetMask::GROUND_UNITS,
            }),
        ]),
    );
    let primary = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let secondary = sim.spawn_unit_with_properties(
        unit(1, 85, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.step();
    let mut restored = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    let rules = DamageRules::warcraft_frozen_throne();
    let bonus =
        rules.apply_attack_with_armor_per_100(80, DamageType::Siege, ArmorType::Unarmored, 0);
    let full = rules.apply_spell(20, ArmorType::Unarmored);
    let half = rules.apply_spell(10, ArmorType::Unarmored);
    assert_eq!(sim.unit(primary).unwrap().health, 1000 - full - bonus * 2);
    assert_eq!(sim.unit(secondary).unwrap().health, 1000 - half - bonus);
}

#[test]
fn nested_aftershock_disables_charge_and_recursion_but_keeps_independent_threshold_listener() {
    let mut sim = simulation(1);
    let source = source(&mut sim, PassiveUnitEffects::single(aftershock()));
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            passive_effects: PassiveUnitEffects::from_slice(&[
                aftershock(),
                PassiveUnitEffect::DamageThreshold(crate::DamageThresholdProfile {
                    ability: AbilityId(805),
                    thresholds: [crate::DamageThreshold {
                        hp_ratio_below_per_10k: 9000,
                        attack_bonus: 3,
                        adds_if_absent: true,
                        vertex_rgba: [1, 2, 3, 4],
                    }; 3],
                }),
            ]),
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, victim).unwrap();
    let mut health: Vec<_> = units.iter().map(|u| u.health).collect();
    health[index] = 800;
    units[index].status.damage_charge = Some(crate::DamageChargeState {
        profile: super::damage_charge::profile(),
        mana_per_10k_hz: 0,
        armed: false,
        detonation_tick: None,
    });
    let before = units[index].status.damage_charge;
    super::super::source_damage_runtime::apply_aftershock(
        source,
        index,
        super::super::source_damage_runtime::SourceDamageState {
            units: &mut units,
            health: &mut health,
            rules: DamageRules::warcraft_frozen_throne(),
            tick: 1,
        },
    );
    assert_eq!(units[index].status.damage_charge, before);
    assert!(units[index].status.damage_threshold.ability_level > 0);
    assert_eq!(health[find_unit_index(&units, source).unwrap()], 1000);
    assert!(health[index] < 800);
}

#[test]
fn fatal_aftershock_suppresses_parent_target_response_before_original_health_commit() {
    let mut sim = simulation(1);
    source(&mut sim, PassiveUnitEffects::single(aftershock()));
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            passive_effects: super::lightning::shield(10_000),
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, victim))
        .unwrap()
        .current = 10;
    sim.step();
    sim.step();
    assert!(sim.unit(victim).is_none());
    assert_eq!(
        sim.projectile_count(),
        0,
        "the dead victim cannot issue the parent event's retaliation order"
    );
}
