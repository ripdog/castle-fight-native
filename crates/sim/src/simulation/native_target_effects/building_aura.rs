use super::*;
use crate::components::AuraEffectProfile;

fn aura(radius: i32) -> AuraEffectProfile {
    AuraEffectProfile {
        self_only: false,
        melee_life_steal_per_10k: 0,
        melee_damage_return_per_10k: 0,
        ability: AbilityId(89),
        radius: radius * SUBUNITS_PER_WORLD_UNIT,
        armor_bonus_per_100: 230,
        damage_bonus_per_10k: 0,
        mana_regeneration_per_second_per_10k: 450_000,
        affects_melee: true,
        affects_ranged: true,
        linger_ticks: 0,
        suspend_during_spell_cooldown: false,
    }
}

fn source(x: i32) -> BuildingSpawn {
    BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(x, 18, 4, 4),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    }
}

fn properties(profile: AuraEffectProfile) -> BuildingGameplayProperties {
    BuildingGameplayProperties {
        passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Aura(profile)),
        ..Default::default()
    }
}

#[test]
fn building_aura_activates_at_completion_and_refreshes_fractional_mana_across_wire() {
    let mut sim = simulation(1);
    let profile = aura(10);
    let building = sim
        .try_start_building_construction(
            PlayerId(0),
            source(40),
            BuildingGameplayProperties {
                construction_time_ticks: Some(3),
                ..properties(profile)
            },
        )
        .unwrap();
    let target = sim.spawn_unit_with_properties_and_spellcasting(
        unit(0, 52, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties::default(),
        mana_profile(0),
    );
    for _ in 0..3 {
        sim.step();
        assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
        assert_eq!(sim.unit(target).unwrap().mana_current, Some(0));
    }
    sim.step();
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
    assert_eq!(
        sim.unit(target).unwrap().status.armor_modifiers[0].armor_bonus_per_100,
        profile.armor_bonus_per_100
    );
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(0));
    sim.step();
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(1));
    assert_eq!(
        sim.world
            .get::<ManaState>(entity(&sim, target))
            .unwrap()
            .regen_remainder_per_10k,
        5000
    );
    assert!(
        sim.world
            .get::<StatusState>(entity(&sim, building))
            .is_none()
    );
    assert_eq!(sim.building(building).unwrap().attack, None);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..7 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(12));
    assert!(sim.remove_building(building));
    assert!(restored.remove_building(building));
    assert_eq!(
        {
            sim.step();
            sim.checksum()
        },
        {
            restored.step();
            restored.checksum()
        }
    );
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(12));
}

#[test]
fn building_aura_is_nonstacking_and_accepts_allied_air_mechanical_and_invulnerable_units() {
    let mut sim = simulation(1);
    let profile = aura(20);
    let first = sim.spawn_building_with_properties(source(40), properties(profile));
    let second = sim.spawn_building_with_properties(source(44), properties(profile));
    let mut allies = Vec::new();
    for (air, mechanical, invulnerable) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        allies.push(sim.spawn_unit_with_properties_and_spellcasting(
            unit(0, 52, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                collision_radius: Some(CollisionRadius(0)),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                mechanical,
                build_time_ticks: Some(1),
                repair_time_ticks: Some(1),
                classifications: UnitClassifications {
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
            mana_profile(0),
        ));
    }
    let enemy = sim.spawn_unit(unit(1, 52, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(0, 67, 0, AttackDelivery::Melee));
    let structure = sim.spawn_building(BuildingSpawn {
        footprint: BuildingFootprint::new(50, 26, 4, 4),
        ..source(50)
    });
    sim.step();
    for id in &allies {
        let target = sim.unit(*id).unwrap();
        assert_eq!(target.status.armor_modifier_count, 1);
        assert_eq!(
            target.status.armor_modifiers[0].armor_bonus_per_100,
            profile.armor_bonus_per_100
        );
        assert_eq!(target.mana_current, Some(1));
    }
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 0);
    assert_eq!(sim.unit(outside).unwrap().status.armor_modifier_count, 0);
    assert_eq!(
        sim.building(structure).unwrap().status.armor_modifier_count,
        0
    );
    sim.world
        .get_mut::<Health>(entity(&sim, first))
        .unwrap()
        .current = 0;
    sim.step();
    for id in &allies {
        assert_eq!(sim.unit(*id).unwrap().status.armor_modifier_count, 1);
    }
    sim.world.get_mut::<Team>(entity(&sim, second)).unwrap().0 = 1;
    sim.step();
    for id in &allies {
        assert_eq!(sim.unit(*id).unwrap().status.armor_modifier_count, 0);
    }
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 1);
}
