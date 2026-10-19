use super::*;

fn charm_attacker(sim: &mut Simulation) -> SimId {
    charm_attacker_with_delivery(sim, 0, AttackDelivery::RangedInstant)
}

fn charm_attacker_with_delivery(
    sim: &mut Simulation,
    damage: i32,
    delivery: AttackDelivery,
) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 20, damage, delivery),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
                    cooldown_ticks: 0,
                    ability: AbilityId(961),
                    chance_per_10k: 10_000,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::Charm(crate::OrbCharmProfile {
                        ability: AbilityId(962),
                        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                        maximum_creep_level: 0,
                    }),
                },
            )),
            ..Default::default()
        },
    )
}

#[test]
fn orb_charm_converts_eligible_recipients_pierces_spell_immunity_and_skips_heroes_and_invulnerable()
{
    for (classes, converted) in [
        (UnitClassifications::default(), true),
        (
            UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            true,
        ),
        (
            UnitClassifications {
                hero: true,
                ..Default::default()
            },
            false,
        ),
        (
            UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            false,
        ),
    ] {
        let mut sim = simulation(1);
        charm_attacker(&mut sim);
        let target = sim.spawn_unit_with_properties(
            unit(1, 50, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: classes,
                ..Default::default()
            },
        );
        sim.step();
        let mut other = wire_restored(&sim, 4);
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
        let team = sim.unit(target).unwrap().team;
        assert_eq!(team == Team(0), converted, "{classes:?}");
    }
}

#[test]
fn ballistic_orb_charm_converts_original_target_only_when_the_projectile_lands() {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let mut sim = simulation(1);
    charm_attacker_with_delivery(
        &mut sim,
        10,
        AttackDelivery::RangedBallistic {
            speed_per_tick: 10 * world,
            impact_radius: 0,
        },
    );
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    let is_ballistic = |sim: &Simulation| {
        sim.projectiles()
            .iter()
            .any(|p| matches!(p.kind, ProjectileViewKind::Ballistic { .. }))
    };
    for _ in 0..8 {
        if is_ballistic(&sim) {
            break;
        }
        sim.step();
    }
    assert!(is_ballistic(&sim), "ballistic attack must launch");
    while is_ballistic(&sim) {
        assert_eq!(
            sim.unit(target).unwrap().team,
            Team(1),
            "charm applied before impact"
        );
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().team, Team(0));
}
