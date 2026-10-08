use super::*;
use crate::KillBerserkProfile;

fn properties() -> UnitGameplayProperties {
    UnitGameplayProperties {
        attack_targets: AttackTargetMask::ALL,
        passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::KillBerserk(
            KillBerserkProfile {
                ability: AbilityId(91),
                duration_ticks: 5,
                movement_percent_delta: 25,
                attack_speed_percent_delta: 200,
                damage_taken_bonus_per_10k: 5000,
            },
        )),
        ..UnitGameplayProperties::default()
    }
}

fn engage(sim: &mut Simulation, source: SimId, target: SimId) {
    sim.world
        .entity_mut(entity(sim, source))
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(target);
}

fn victim(sim: &mut Simulation, combat_sapper: bool) -> SimId {
    sim.spawn_unit_with_properties(
        unit(1, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    )
}

#[test]
fn credited_kill_immediately_changes_future_attack_rate_and_later_incoming_damage() {
    let mut sim = simulation(1);
    let source =
        sim.spawn_unit_with_properties(unit(0, 20, 2000, AttackDelivery::Melee), properties());
    let target = victim(&mut sim, true);
    let attacker = sim.spawn_unit(unit(1, 60, 100, AttackDelivery::Melee));
    engage(&mut sim, source, target);
    engage(&mut sim, attacker, source);
    sim.step();
    assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
    sim.step();
    let source_view = sim.unit(source).unwrap();
    assert_eq!(source_view.health, 850);
    assert_eq!(source_view.cooldown_remaining, 334);
    assert_eq!(source_view.status.movement_modifiers[0].percent_delta, 25);
    assert_eq!(
        source_view.status.attack_speed_modifiers[0].percent_delta,
        200
    );
    assert_eq!(
        source_view.status.armor_modifiers[0].damage_taken_bonus_per_10k,
        5000
    );
    assert_eq!(source_view.status.armor_modifiers[0].expires_tick, 6);
    assert_eq!(source_view.target, None);
    assert_eq!(source_view.mana_current, None);
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..5 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    let expired = sim.unit(source).unwrap().status;
    assert_eq!(expired.movement_modifier_count, 0);
    assert_eq!(expired.attack_speed_modifier_count, 0);
    assert_eq!(expired.armor_modifier_count, 0);
}

#[test]
fn ranged_kill_waits_for_impact_and_restores_during_flight() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            2000,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        properties(),
    );
    let target = victim(&mut sim, true);
    engage(&mut sim, source, target);
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..8 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
        if sim.unit(source).unwrap().status.armor_modifier_count > 0 {
            assert_eq!(
                sim.unit(source).unwrap().status.armor_modifiers[0].expires_tick,
                sim.next_tick - 1 + 5
            );
            return;
        }
    }
    panic!("released missile never activated its live killer's buff");
}

#[test]
fn nonfatal_noncombat_building_and_external_deaths_cannot_grant_kill_berserk() {
    for (damage, combat_sapper) in [(1, true), (2000, false)] {
        let mut sim = simulation(1);
        let source = sim
            .spawn_unit_with_properties(unit(0, 20, damage, AttackDelivery::Melee), properties());
        let target = victim(&mut sim, combat_sapper);
        engage(&mut sim, source, target);
        sim.step();
        sim.step();
        assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
    }
    let mut sim = simulation(1);
    let source =
        sim.spawn_unit_with_properties(unit(0, 20, 2000, AttackDelivery::Melee), properties());
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(6, 0, 1, 1),
        health: 10,
        production: None,
        attack: None,
        spellcasting: None,
    });
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
    let target = victim(&mut sim, true);
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Health>()
        .unwrap()
        .current = 0;
    sim.step();
    assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn source_death_before_impact_does_not_redirect_berserk_to_another_unit() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            2000,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        properties(),
    );
    let observer =
        sim.spawn_unit_with_properties(unit(0, 25, 0, AttackDelivery::Melee), properties());
    let target = victim(&mut sim, true);
    engage(&mut sim, source, target);
    sim.step();
    sim.step();
    sim.world
        .entity_mut(entity(&sim, source))
        .get_mut::<Health>()
        .unwrap()
        .current = 0;
    for _ in 0..8 {
        sim.step();
    }
    assert_eq!(sim.unit(observer).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn incoming_bonus_scales_spells_and_periodic_damage_until_expiry() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    let mut caster = mana_profile(10);
    caster.ability.range = 200 * SUBUNITS_PER_WORLD_UNIT;
    caster.ability.effect = AbilityEffect::Damage { amount: 100 };
    sim.spawn_unit_with_spellcasting(unit(1, 60, 0, AttackDelivery::Melee), caster);
    let mut status = sim.world.entity_mut(entity(&sim, target));
    let mut status = status.get_mut::<StatusState>().unwrap();
    apply_timed_armor_modifier(
        &mut status,
        TimedArmorModifier {
            id: ModifierId(91),
            damage_taken_bonus_per_10k: 5000,
            expires_tick: 2,
            ..TimedArmorModifier::default()
        },
    );
    apply_timed_damage_over_time(&mut status, ModifierId(92), 100, 1, 0, 4);
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 850);
    let mut restored = wire_restored(&sim, 6);
    for expected_health in [700, 600, 500] {
        sim.step();
        restored.step();
        assert_eq!(sim.unit(target).unwrap().health, expected_health);
        assert_eq!(sim.checksum(), restored.checksum());
    }
}
