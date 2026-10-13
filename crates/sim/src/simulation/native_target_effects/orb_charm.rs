use super::*;

fn charm_attacker(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
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
        assert_eq!(sim.step().checksum, other.step().checksum);
        let team = sim.unit(target).unwrap().team;
        assert_eq!(team == Team(0), converted, "{classes:?}");
    }
}
