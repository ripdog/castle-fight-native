use super::*;
use crate::NativeBuffIdentity;

fn profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile::per_second(20, 12, 0),
        ability: AutomaticAbilityProfile {
            id: AbilityId(81),
            mana_cost: 3,
            cooldown_ticks: 1000,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::NativeBuffDonor,
            effect: AbilityEffect::SpellSteal {
                recipient_radius: 20 * SUBUNITS_PER_WORLD_UNIT,
            },
        },
    }
}

fn buff(positive: bool) -> TimedArmorModifier {
    TimedArmorModifier {
        id: ModifierId(82),
        native_buff: Some(NativeBuffIdentity {
            rawcode: 83,
            positive,
            stealable: true,
            organic_only: !positive,
        }),
        armor_bonus_per_100: if positive { 300 } else { -300 },
        damage_bonus_per_10k: if positive { 1000 } else { -1000 },
        regeneration_per_second_per_10k: if positive { 300_000 } else { 0 },
        revealed_to: (!positive).then_some(Team(1)),
        expires_tick: 6,
        ..TimedArmorModifier::default()
    }
}

fn grant(sim: &mut Simulation, id: SimId, modifier: TimedArmorModifier) {
    let mut entity = sim.world.entity_mut(entity(sim, id));
    apply_timed_armor_modifier(&mut entity.get_mut::<StatusState>().unwrap(), modifier);
}

fn status(sim: &Simulation, id: SimId) -> StatusState {
    *sim.world.get::<StatusState>(entity(sim, id)).unwrap()
}

#[test]
fn positive_transfer_moves_one_complete_bundle_preserves_expiry_and_leaves_intrinsic_state() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let donor = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
    let already_buffed = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    let recipient = sim.spawn_unit_with_properties(
        unit(0, 100, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            movement_class: MovementClass::Air,
            classifications: UnitClassifications {
                spell_immune: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    grant(&mut sim, donor, buff(true));
    grant(&mut sim, already_buffed, buff(true));
    let intrinsic = TimedArmorModifier {
        id: ModifierId(80),
        armor_bonus_per_100: 200,
        expires_tick: u64::MAX,
        ..TimedArmorModifier::default()
    };
    grant(&mut sim, donor, intrinsic);
    let mut heroic = buff(true);
    heroic.id = ModifierId(84);
    heroic.native_buff.as_mut().unwrap().rawcode = 85;
    heroic.native_buff.as_mut().unwrap().stealable = false;
    grant(&mut sim, donor, heroic);
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current.unwrap(), 9);
    let donor_status = status(&sim, donor);
    assert_eq!(&donor_status.armor_modifiers[..2], &[intrinsic, heroic]);
    assert_eq!(status(&sim, recipient).armor_modifiers[0], buff(true));
    assert_eq!(status(&sim, caster).armor_modifier_count, 0);
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..6 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(status(&sim, recipient).armor_modifier_count, 0);
    assert_eq!(status(&sim, donor).armor_modifier_count, 1);
}

#[test]
fn negative_transfer_obeys_polarity_and_recipient_immunity_and_reassigns_reveal_team() {
    let mut sim = simulation(2);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let donor = sim.spawn_unit(unit(0, 80, 0, AttackDelivery::Melee));
    let mechanical = sim.spawn_unit_with_properties(
        unit(1, 81, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            ..UnitGameplayProperties::default()
        },
    );
    let immune = sim.spawn_unit_with_properties(
        unit(1, 82, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let invulnerable = sim.spawn_unit_with_properties(
        unit(1, 83, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let outside = sim.spawn_unit(unit(1, 101, 0, AttackDelivery::Melee));
    let recipient = sim.spawn_unit_with_properties(
        unit(1, 100, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..UnitGameplayProperties::default()
        },
    );
    grant(&mut sim, donor, buff(false));
    sim.step();
    assert_eq!(
        sim.world
            .get::<AutomaticAbilityState>(entity(&sim, caster))
            .unwrap()
            .cast_sequence,
        1
    );
    assert_eq!(status(&sim, donor).armor_modifier_count, 0);
    let mut expected = buff(false);
    expected.revealed_to = Some(Team(0));
    assert_eq!(status(&sim, recipient).armor_modifiers[0], expected);
    for id in [mechanical, immune, invulnerable, outside] {
        assert_eq!(status(&sim, id).armor_modifier_count, 0);
    }
}

#[test]
fn pending_transfers_restore_and_competing_casters_cannot_duplicate_the_buff() {
    let mut sim = simulation(1);
    let caster_template = unit(0, 20, 0, AttackDelivery::Melee);
    let properties = UnitGameplayProperties {
        action_timing: crate::ActionTimingProfile {
            cast_point_ticks: 2,
            cast_ticks: 4,
            ..crate::ActionTimingProfile::default()
        },
        ..UnitGameplayProperties::default()
    };
    let first =
        sim.spawn_unit_with_properties_and_spellcasting(caster_template, properties, profile());
    let second =
        sim.spawn_unit_with_properties_and_spellcasting(caster_template, properties, profile());
    let donor = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
    let recipient = sim.spawn_unit(unit(0, 100, 0, AttackDelivery::Melee));
    grant(&mut sim, donor, buff(true));
    sim.step();
    assert!(status(&sim, first).pending_cast.is_some());
    assert!(status(&sim, second).pending_cast.is_some());
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..2 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(first).unwrap().mana_current.unwrap(), 9);
    assert_eq!(sim.unit(second).unwrap().mana_current.unwrap(), 12);
    assert_eq!(
        sim.world
            .get::<AutomaticAbilityState>(entity(&sim, second))
            .unwrap()
            .cast_sequence,
        0
    );
    assert_eq!(status(&sim, donor).armor_modifier_count, 0);
    assert_eq!(status(&sim, recipient).armor_modifiers[0].expires_tick, 6);
}

#[test]
fn no_transferable_pair_cannot_consume_mana_or_cooldown() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let enemy = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
    grant(&mut sim, enemy, buff(false)); // Wrong polarity on a hostile donor.
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current.unwrap(), 12);
    assert_eq!(
        sim.world
            .get::<AutomaticAbilityState>(entity(&sim, caster))
            .unwrap()
            .cast_sequence,
        0
    );
    grant(&mut sim, enemy, buff(true)); // No ally within the recipient radius.
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current.unwrap(), 12);
    assert_eq!(
        sim.world
            .get::<AutomaticAbilityState>(entity(&sim, caster))
            .unwrap()
            .cast_sequence,
        0
    );
}
