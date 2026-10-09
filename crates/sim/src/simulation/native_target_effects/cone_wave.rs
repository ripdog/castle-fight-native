use super::*;
use crate::components::{ConeWaveProfile, NativeAction};
fn profile() -> ConeWaveProfile {
    ConeWaveProfile {
        ability: AbilityId(33),
        damage: 30,
        maximum_damage: 45,
        distance: 100 * SUBUNITS_PER_WORLD_UNIT,
        initial_radius: 5 * SUBUNITS_PER_WORLD_UNIT,
        final_radius: 25 * SUBUNITS_PER_WORLD_UNIT,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        targets: AttackTargetMask::AIR_AND_GROUND,
    }
}
fn point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}
fn victim(sim: &mut Simulation, team: u8, x: i32, y: i32, air: bool, immune: bool) -> SimId {
    let mut spawn = unit(team, x, 0, AttackDelivery::Melee);
    spawn.position = point(x, y);
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            movement_class: if air {
                MovementClass::Air
            } else {
                MovementClass::Ground
            },
            classifications: UnitClassifications {
                spell_immune: immune,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
#[test]
fn cone_wave_is_fixed_heading_expands_hits_once_and_retains_aggregate_damage_budget() {
    let mut sim = simulation(1);
    let near = victim(&mut sim, 1, 30, 20, false, false);
    let far = victim(&mut sim, 1, 80, 37, true, false);
    let outside = victim(&mut sim, 1, 30, 30, false, false);
    let behind = victim(&mut sim, 1, 5, 20, false, false);
    let ally = victim(&mut sim, 0, 40, 20, false, false);
    let immune = victim(&mut sim, 1, 25, 20, false, true);
    let state = super::super::native_actions::cone_wave::new_state(
        SimId(777),
        Team(0),
        point(10, 20),
        point(60, 20),
        profile(),
        0,
    );
    let id = sim.allocate_id();
    sim.world.spawn((id, NativeAction::ConeWave(state)));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(near).unwrap().health, 1000);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..12 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(near).unwrap().health, 970);
    assert_eq!(sim.unit(far).unwrap().health, 985);
    for target in [outside, behind, ally, immune] {
        assert_eq!(sim.unit(target).unwrap().health, 1000);
    }
    assert_eq!(sim.projectile_count(), 0);
}
#[test]
fn cone_proc_uses_caster_origin_and_preserves_wave_when_trigger_victim_dies() {
    let mut sim = simulation(1);
    let mut spawn = unit(0, 10, 2000, AttackDelivery::RangedInstant);
    spawn.attack.range = 200 * SUBUNITS_PER_WORLD_UNIT;
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
                    ability: AbilityId(99),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::ConeWave(profile()),
                },
            )),
            ..Default::default()
        },
    );
    let trigger = victim(&mut sim, 1, 40, 20, false, false);
    let behind_target = victim(&mut sim, 1, 60, 20, false, false);
    sim.step();
    sim.step();
    assert!(sim.unit(trigger).is_none());
    assert_eq!(sim.projectile_count(), 1);
    for _ in 0..7 {
        sim.step();
    }
    assert_eq!(sim.unit(behind_target).unwrap().health, 970);
}
