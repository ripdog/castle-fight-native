use super::*;
use crate::NativeBuffIdentity;

fn profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile::per_second(20, 12, 0),
        ability: AutomaticAbilityProfile {
            id: AbilityId(71),
            mana_cost: 3,
            cooldown_ticks: 1000,
            range: 120 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::FriendlyUnitInCombat,
            effect: AbilityEffect::StatBuff {
                modifier: ModifierId(71),
                buff: NativeBuffIdentity {
                    rawcode: 72,
                    positive: true,
                    stealable: true,
                    organic_only: false,
                },
                armor_bonus_per_100: 300,
                damage_bonus_per_10k: 5000,
                regeneration_per_second_per_10k: 300_000,
                duration_ticks: 5,
                hero_duration_ticks: 3,
                autocast_range: 100 * SUBUNITS_PER_WORLD_UNIT,
            },
        },
    }
}

fn engage(sim: &mut Simulation, source: SimId, target: SimId) {
    sim.world
        .entity_mut(entity(sim, source))
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(target);
}

#[test]
fn positive_native_autocast_has_independent_range_and_accepts_air_mechanical_and_magic_immunity() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let enemy = sim.spawn_unit(unit(1, 130, 0, AttackDelivery::Melee));
    let idle = sim.spawn_unit(unit(0, 30, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(0, 121, 1, AttackDelivery::Melee));
    let invulnerable = sim.spawn_unit_with_properties(
        unit(0, 40, 1, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let target = sim.spawn_unit_with_properties(
        unit(0, 120, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            mechanical: true,
            build_time_ticks: Some(1),
            classifications: UnitClassifications {
                spell_immune: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    for id in [outside, invulnerable, target] {
        engage(&mut sim, id, enemy);
    }
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Health>()
        .unwrap()
        .current = 980;
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(9));
    let view = sim.unit(target).unwrap();
    assert_eq!(view.status.effective_armor_points_per_100(view.armor), 300);
    assert_eq!(view.status.armor_modifier_count, 1);
    assert_eq!(view.status.armor_modifiers[0].expires_tick, 5);
    for id in [caster, enemy, idle, outside, invulnerable] {
        assert_eq!(sim.unit(id).unwrap().status.armor_modifier_count, 0);
    }
    // The permanent weapon stays unchanged; its outgoing damage and fractional health
    // regeneration consume the timed bundle during resolution.
    sim.step();
    assert_eq!(sim.unit(enemy).unwrap().health, 1000 - 15 - 1 - 1);
    assert_eq!(sim.unit(target).unwrap().health, 981);
    assert_eq!(sim.unit(target).unwrap().attack.damage, 10);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..7 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn native_buff_family_revalidates_competing_casters_and_replaces_the_whole_bundle() {
    let mut sim = simulation(1);
    let first = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let mut other = profile();
    other.ability.id = AbilityId(73);
    let AbilityEffect::StatBuff { modifier, .. } = &mut other.ability.effect else {
        unreachable!()
    };
    *modifier = ModifierId(73);
    let second = sim.spawn_unit_with_spellcasting(unit(0, 30, 0, AttackDelivery::Melee), other);
    let enemy = sim.spawn_unit(unit(1, 130, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit_with_properties(
        unit(0, 40, 1, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                hero: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    engage(&mut sim, target, enemy);
    sim.step();
    assert_eq!(sim.unit(first).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(second).unwrap().mana_current, Some(12));
    assert_eq!(
        sim.unit(target).unwrap().status.armor_modifiers[0].expires_tick,
        3
    );
    let target_entity = entity(&sim, target);
    let mut status = sim
        .world
        .get::<StatusState>(target_entity)
        .copied()
        .unwrap();
    let prior = status.armor_modifiers[0];
    apply_timed_armor_modifier(
        &mut status,
        TimedArmorModifier {
            id: ModifierId(73),
            armor_bonus_per_100: 100,
            regeneration_per_second_per_10k: 0,
            damage_bonus_per_10k: 1000,
            expires_tick: 2,
            ..prior
        },
    );
    assert_eq!(status.armor_modifier_count, 1);
    assert_eq!(status.armor_modifiers[0].id, ModifierId(73));
    assert_eq!(
        status.effective_armor_points_per_100(ArmorProfile::default()),
        100
    );
    assert_eq!(status.armor_modifiers[0].expires_tick, 2);
    sim.world.entity_mut(target_entity).insert(status);
    let checksum = sim.checksum();
    sim.world
        .entity_mut(target_entity)
        .get_mut::<StatusState>()
        .unwrap()
        .armor_modifiers[0]
        .native_buff
        .as_mut()
        .unwrap()
        .stealable = false;
    assert_ne!(sim.checksum(), checksum);
}

#[test]
fn native_positive_autocast_does_not_spend_resources_without_combat_or_sufficient_mana() {
    let mut sim = simulation(1);
    let mut poor = profile();
    poor.mana.starting = 2;
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), poor);
    let enemy = sim.spawn_unit(unit(1, 130, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit(unit(0, 40, 1, AttackDelivery::Melee));
    engage(&mut sim, target, enemy);
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(2));
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    assert_eq!(
        sim.world
            .get::<AutomaticAbilityState>(entity(&sim, caster))
            .unwrap()
            .cast_sequence,
        0
    );
    let mut idle = simulation(1);
    let caster =
        idle.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    idle.spawn_unit(unit(0, 40, 1, AttackDelivery::Melee));
    idle.step();
    assert_eq!(idle.unit(caster).unwrap().mana_current, Some(12));
    assert!(idle.last_ability_casts.is_empty());
}

#[test]
fn stat_buff_cast_windups_restore_and_revalidate_the_family_at_release() {
    let mut sim = simulation(1);
    let properties = UnitGameplayProperties {
        action_timing: ActionTimingProfile {
            cast_ticks: 4,
            cast_point_ticks: 2,
            ..ActionTimingProfile::default()
        },
        ..UnitGameplayProperties::default()
    };
    let first = sim.spawn_unit_with_properties_and_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        properties,
        profile(),
    );
    let second = sim.spawn_unit_with_properties_and_spellcasting(
        unit(0, 30, 0, AttackDelivery::Melee),
        properties,
        profile(),
    );
    let enemy = sim.spawn_unit(unit(1, 130, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit(unit(0, 40, 1, AttackDelivery::Melee));
    engage(&mut sim, target, enemy);
    sim.step();
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    assert_eq!(sim.unit(first).unwrap().mana_current, Some(12));
    assert!(sim.unit(first).unwrap().status.pending_cast.is_some());
    assert!(sim.unit(second).unwrap().status.pending_cast.is_some());
    let mut restored = wire_restored(&sim, 4);
    // Combat starts the order; its disappearance during the windup does not change
    // native recipient eligibility. The buff family must still be rechecked.
    for game in [&mut sim, &mut restored] {
        let enemy_entity = entity(game, enemy);
        game.world
            .entity_mut(enemy_entity)
            .get_mut::<Health>()
            .unwrap()
            .current = 0;
    }
    for _ in 0..2 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(first).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(second).unwrap().mana_current, Some(12));
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
    assert_eq!(
        sim.unit(target).unwrap().status.armor_modifiers[0].expires_tick,
        7
    );
}

#[test]
fn positive_autocast_recognizes_an_ally_attacking_a_structure() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(6, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let target = sim.spawn_unit(unit(0, 40, 1, AttackDelivery::Melee));
    engage(&mut sim, target, building);
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
}
