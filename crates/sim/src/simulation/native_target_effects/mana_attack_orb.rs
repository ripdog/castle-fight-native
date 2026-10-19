use super::*;
use crate::{ManaAttackOrbProfile, SplashFalloffProfile};

fn profile() -> ManaAttackOrbProfile {
    ManaAttackOrbProfile {
        ability: AbilityId(911),
        mana_cost: 7,
        bonus_damage: 8,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        primary_targets: AttackTargetMask::AIR_AND_GROUND,
        splash: SplashFalloffProfile {
            full_radius: 3 * SUBUNITS_PER_WORLD_UNIT,
            medium_radius: 6 * SUBUNITS_PER_WORLD_UNIT,
            outer_radius: 9 * SUBUNITS_PER_WORLD_UNIT,
            medium_damage_per_10k: 5000,
            outer_damage_per_10k: 2500,
            targets: AttackTargetMask::AIR_AND_GROUND,
        },
    }
}
fn source(sim: &mut Simulation, mana: i32, windup: u16) -> SimId {
    sim.spawn_unit_with_properties_and_spellcasting(
        unit(
            0,
            20,
            12,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(
                PassiveUnitEffect::ManaAttackOrb(profile()),
            ),
            action_timing: ActionTimingProfile {
                primary_attack_point_ticks: windup,
                ..Default::default()
            },
            ..Default::default()
        },
        mana_profile(mana),
    )
}
#[test]
fn mana_attack_orb_released_mode_owns_bonus_falloff_native_art_and_saved_payload_after_source_death()
 {
    let mut sim = simulation(1);
    let source = source(&mut sim, 7, 0);
    let primary = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    let full = sim.spawn_unit(unit(1, 53, 0, AttackDelivery::Melee));
    let half = sim.spawn_unit(unit(1, 56, 0, AttackDelivery::Melee));
    let outer = sim.spawn_unit(unit(1, 59, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let ally = sim.spawn_unit(unit(0, 51, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(0));
    let projectile = sim
        .projectiles()
        .into_iter()
        .find(|p| p.source == source)
        .unwrap();
    assert_eq!(projectile.ability, Some(profile().ability));
    assert_eq!(projectile.impact_tick - projectile.launch_tick, 3);
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..3 {
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
    }
    for (id, damage) in [
        (primary, 20),
        (full, 20),
        (half, 10),
        (outer, 5),
        (outside, 0),
        (ally, 0),
    ] {
        assert_eq!(sim.unit(id).unwrap().health, 1000 - damage);
    }
}
#[test]
fn mana_attack_orb_insufficient_mana_releases_ordinary_weapon_without_late_upgrade_or_collateral() {
    let mut sim = simulation(1);
    let source = source(&mut sim, 6, 0);
    let primary = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    let collateral = sim.spawn_unit(unit(1, 53, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(6));
    let p = sim
        .projectiles()
        .into_iter()
        .find(|p| p.source == source)
        .unwrap();
    assert_eq!(p.ability, None);
    assert_eq!(p.impact_tick - p.launch_tick, 30);
    sim.world
        .get_mut::<ManaState>(entity(&sim, source))
        .unwrap()
        .current = 50;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..30 {
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
    }
    assert_eq!(sim.unit(primary).unwrap().health, 988);
    assert_eq!(sim.unit(collateral).unwrap().health, 1000);
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(50));
}
#[test]
fn mana_attack_orb_commits_at_windup_release_and_spends_mana_even_if_evaded() {
    for evaded in [false, true] {
        let mut sim = simulation(1);
        let source = source(&mut sim, 14, 2);
        sim.spawn_unit_with_properties(
            unit(1, 50, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: if evaded {
                    PassiveUnitEffects::single(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                        ability: AbilityId(913),
                        chance_per_10k: 10000,
                    }))
                } else {
                    Default::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(14));
        assert!(sim.unit(source).unwrap().status.pending_attack.is_some());
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
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(14));
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
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(7));
        assert_eq!(
            sim.projectiles().iter().any(|p| p.source == source),
            !evaded
        );
    }
}
#[test]
fn mana_attack_orb_live_mana_drain_during_windup_preserves_fallback() {
    let mut sim = simulation(1);
    let source = source(&mut sim, 14, 2);
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    sim.world
        .get_mut::<ManaState>(entity(&sim, source))
        .unwrap()
        .current = 0;
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(0));
    assert_eq!(
        sim.projectiles()
            .iter()
            .find(|p| p.source == source)
            .unwrap()
            .ability,
        None
    );
    assert_eq!(sim.unit(target).unwrap().health, 1000);
}

#[test]
fn mana_attack_orb_primary_structure_keeps_ordinary_weapon_and_does_not_spend_mana() {
    let mut sim = simulation(1);
    let source = source(&mut sim, 14, 0);
    let target = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(30, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(14));
    let p = sim
        .projectiles()
        .into_iter()
        .find(|p| p.source == source)
        .expect("ordinary building attack still releases");
    assert_eq!(p.ability, None);
    assert!(matches!(p.kind,ProjectileViewKind::GuaranteedHit { target: id } if id==target));
}

#[test]
fn mana_attack_orb_magic_immunity_prevents_native_mode_without_blocking_the_ordinary_packet() {
    let mut sim = simulation(1);
    let source = source(&mut sim, 14, 0);
    let target = sim.spawn_unit_with_properties(
        unit(1, 50, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(14));
    assert_eq!(
        sim.projectiles()
            .iter()
            .find(|p| p.source == source)
            .unwrap()
            .ability,
        None
    );
    for _ in 0..30 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 988);
}
#[test]
fn mana_attack_orb_collateral_mask_can_admit_structures_while_primary_mask_cannot() {
    let mut sim = simulation(1);
    let source = source(&mut sim, 14, 0);
    let mut p = profile();
    p.splash.targets = AttackTargetMask::ALL;
    sim.world
        .entity_mut(entity(&sim, source))
        .insert(PassiveUnitEffects::single(
            PassiveUnitEffect::ManaAttackOrb(p),
        ));
    let primary = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    let collateral = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(54, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    sim.step();
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(7));
    for _ in 0..3 {
        sim.step();
    }
    assert_eq!(sim.unit(primary).unwrap().health, 980);
    assert!(sim.building(collateral).unwrap().health < 1000);
}
