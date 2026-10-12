use super::*;
use crate::IncinerateProfile;

fn profile() -> IncinerateProfile {
    IncinerateProfile {
        ability: AbilityId(891),
        buff_rawcode: 892,
        bonus_per_hit: 3,
        maximum_stacks: 4,
        duration_ticks: 15,
        hero_duration_ticks: 9,
        full_damage: 24,
        half_damage: 11,
        full_radius: 6 * SUBUNITS_PER_WORLD_UNIT,
        half_radius: 12 * SUBUNITS_PER_WORLD_UNIT,
        delay_ticks: 4,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
    }
}
fn attacker(sim: &mut Simulation, p: IncinerateProfile, delivery: AttackDelivery) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 20, 5, delivery),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Incinerate(p)),
            ..Default::default()
        },
    )
}
fn ready(sim: &mut Simulation, id: SimId) {
    sim.world
        .get_mut::<AttackCooldown>(entity(sim, id))
        .unwrap()
        .remaining = 0;
}
#[test]
fn incinerate_landed_hits_stack_to_profile_limit_and_restart_after_expiry() {
    let mut sim = simulation(1);
    let source = attacker(&mut sim, profile(), AttackDelivery::RangedInstant);
    let victim = sim.spawn_unit_with_properties(
        unit(1, 30, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Normal, 0),
            ..Default::default()
        },
    );
    sim.step(); // Birth tick has no weapon release.
    let mut damage = 0;
    for n in 1..=6 {
        ready(&mut sim, source);
        sim.step();
        damage += 5 + 3 * n.min(4);
        assert_eq!(sim.unit(victim).unwrap().health, 1000 - damage);
        assert_eq!(
            sim.unit(victim).unwrap().status.incinerate.unwrap().stacks,
            n.min(4) as u32
        );
    }
    let mut other = wire_restored(&sim, 4);
    for _ in 0..16 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    ready(&mut sim, source);
    ready(&mut other, source);
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(
        sim.unit(victim).unwrap().status.incinerate.unwrap().stacks,
        1
    );
}
#[test]
fn incinerate_fatal_weapon_hit_destroys_corpse_and_saved_delayed_explosion_uses_dead_victim_position()
 {
    let mut sim = simulation(1);
    let source = attacker(&mut sim, profile(), AttackDelivery::RangedInstant);
    let mut spawn = unit(1, 30, 0, AttackDelivery::Melee);
    spawn.health = 4;
    let victim = sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            corpse: Some(crate::CorpseProfile {
                definition: crate::CorpseDefinitionId(99),
                decay_start_ticks: 2,
                lifetime_ticks: Some(100),
            }),
            ..Default::default()
        },
    );
    let inside = sim.spawn_unit(unit(1, 36, 0, AttackDelivery::Melee));
    let half = sim.spawn_unit(unit(1, 42, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 43, 0, AttackDelivery::Melee));
    let ally = sim.spawn_unit(unit(0, 31, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert!(sim.unit(victim).is_none());
    assert_eq!(sim.unit(inside).unwrap().health, 1000);
    assert!(
        sim.world
            .iter_entities()
            .all(|e| !e.get::<Corpse>().is_some_and(|c| c.source_unit == victim))
    );
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..3 {
        assert_eq!(sim.step().checksum, other.step().checksum);
        assert_eq!(sim.unit(inside).unwrap().health, 1000);
    }
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(sim.unit(inside).unwrap().health, 976);
    assert_eq!(sim.unit(half).unwrap().health, 989);
    assert_eq!(sim.unit(outside).unwrap().health, 1000);
    assert_eq!(sim.unit(ally).unwrap().health, 1000);
}
#[test]
fn incinerate_released_native_missile_keeps_payload_after_caster_death_and_clone_retains_only_death_effect()
 {
    let mut sim = simulation(1);
    let mut p = profile();
    p.bonus_per_hit = 0;
    let source = attacker(
        &mut sim,
        p,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
        },
    );
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..3 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().health, 995);
    assert_eq!(
        sim.unit(target)
            .unwrap()
            .status
            .incinerate
            .unwrap()
            .profile
            .bonus_per_hit,
        0
    );
}
#[test]
fn incinerate_excludes_mechanical_ancient_invulnerable_and_magic_immune_but_admits_air_and_heroes()
{
    for (mechanical, ancient, invulnerable, spell_immune, air, hero) in [
        (true, false, false, false, false, false),
        (false, true, false, false, false, false),
        (false, false, true, false, false, false),
        (false, false, false, true, false, false),
        (false, false, false, false, true, true),
    ] {
        let mut sim = simulation(1);
        attacker(&mut sim, profile(), AttackDelivery::RangedInstant);
        let target = sim.spawn_unit_with_properties(
            unit(1, 30, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                classifications: UnitClassifications {
                    ancient,
                    invulnerable,
                    spell_immune,
                    hero,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        let buff = sim.unit(target).unwrap().status.incinerate;
        assert_eq!(
            buff.is_some(),
            !mechanical && !ancient && !invulnerable && !spell_immune
        );
        if let Some(p) = buff {
            assert_eq!(p.expires_tick - (sim.tick() - 1), 9);
        }
    }
}
