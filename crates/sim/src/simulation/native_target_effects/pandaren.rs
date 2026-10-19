use super::*;
use crate::components::{
    EchoRemnantProfile, EchoRemnantState, HealingSprayProfile, NativeAction, NativeHealProfile,
};

fn critical(ability: u32, multiplier: u16) -> PassiveUnitEffect {
    PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
        ability: AbilityId(ability),
        chance_per_10k: 10_000,
        damage_multiplier_per_10k: multiplier,
        targets: AttackTargetMask::GROUND_UNITS,
    })
}

#[test]
fn a_sub_unity_critical_multiplier_is_a_near_miss_that_loses_to_a_real_critical() {
    let hit = |effects: &[PassiveUnitEffect]| {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 100, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::from_slice(effects),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        sim.step();
        sim.step();
        let critical = sim.attacks_last_tick().iter().any(|attack| attack.critical);
        (1000 - sim.unit(target).unwrap().health, critical)
    };
    let (near_miss, flagged) = hit(&[critical(1, 1_000)]);
    assert!(near_miss > 0 && near_miss < 100 && !flagged);
    let (strong, flagged) = hit(&[critical(1, 1_000), critical(2, 20_000)]);
    assert!(strong > 100 && flagged);
}

#[test]
fn healing_spray_waves_share_the_cap_heal_every_friend_in_the_area_and_outlive_the_caster() {
    let mut sim = simulation(1);
    let spray = HealingSprayProfile {
        ability: AbilityId(300),
        radius: 10 * SUBUNITS_PER_WORLD_UNIT,
        amount: 40,
        maximum_total: 60,
        waves: 1,
        interval_ticks: 5,
        first_wave_ticks: 5,
        lifetime_ticks: 20,
        targets: AttackTargetMask::GROUND_UNITS,
    };
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
                cooldown_ticks: 100,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                effect: AbilityEffect::Heal(NativeHealProfile {
                    amount: 0,
                    targets: AttackTargetMask::GROUND_UNITS,
                    organic_only: false,
                    nonancient_only: false,
                    affects_invulnerable: false,
                    resume_attack_ticks: 1,
                    spray: Some(spray),
                }),
            },
        },
    );
    let wounded = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    let neighbour = sim.spawn_unit(unit(0, 44, 0, AttackDelivery::Melee));
    let distant = sim.spawn_unit(unit(0, 200, 0, AttackDelivery::Melee));
    let enemy = sim.spawn_unit(unit(1, 42, 0, AttackDelivery::Melee));
    for id in [wounded, neighbour, distant, enemy] {
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = 500;
    }
    sim.step();
    sim.step();
    // The wave has not landed yet; the spray is an independent action.
    assert!(
        sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::HealingSpray(_))))
    );
    sim.world
        .get_mut::<Health>(entity(&sim, caster))
        .unwrap()
        .current = 0;
    for _ in 0..8 {
        sim.step();
    }
    let healed = |id| sim.unit(id).unwrap().health - 500;
    // Two friends fall inside the area; their combined 80 exceeds the 60 cap, so each gets 30.
    assert_eq!(healed(wounded), 30);
    assert_eq!(healed(wounded), healed(neighbour));
    assert_eq!(healed(distant), 0);
    assert_eq!(healed(enemy), 0);
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::HealingSpray(_))))
    );
}

#[test]
fn echo_remnant_is_inert_until_armed_then_hurts_only_eligible_enemies_in_its_radius() {
    let mut sim = simulation(1);
    let profile = EchoRemnantProfile {
        ability: AbilityId(400),
        unit_rawcode: 0,
        lifetime_ticks: 100,
        activation_ticks: 10,
        detection_radius: 5 * SUBUNITS_PER_WORLD_UNIT,
        trigger_targets: AttackTargetMask::GROUND_UNITS,
        damage: 100,
        radius: 20 * SUBUNITS_PER_WORLD_UNIT,
        excluded_ability: AbilityId(401),
        excluded_unit_rawcode: 7,
    };
    let sapper = UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    };
    let spawn = |sim: &mut Simulation, team, x, classifications| {
        sim.spawn_unit_with_properties(
            unit(team, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications,
                ..UnitGameplayProperties::default()
            },
        )
    };
    let trigger = spawn(&mut sim, 1, 22, sapper);
    let nearby = spawn(&mut sim, 1, 35, sapper);
    let not_sapper = spawn(&mut sim, 1, 36, UnitClassifications::default());
    let ally = spawn(&mut sim, 0, 22, sapper);
    let far = spawn(&mut sim, 1, 80, sapper);
    let owner = sim.allocate_id();
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::EchoRemnant(EchoRemnantState {
            profile,
            source: owner,
            owner: PlayerId(0),
            team: Team(0),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            created_tick: 0,
            armed_tick: 10,
            expires_tick: 100,
        }),
    ));
    for _ in 0..5 {
        sim.step();
    }
    assert_eq!(
        sim.unit(trigger).unwrap().health,
        1000,
        "unarmed remnants never detonate"
    );
    for _ in 0..10 {
        sim.step();
    }
    assert!(sim.unit(trigger).unwrap().health < 1000);
    assert!(sim.unit(nearby).unwrap().health < 1000);
    for untouched in [not_sapper, ally, far] {
        assert_eq!(sim.unit(untouched).unwrap().health, 1000);
    }
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::EchoRemnant(_))))
    );
}
