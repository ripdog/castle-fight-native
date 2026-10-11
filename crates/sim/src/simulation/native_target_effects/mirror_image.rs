use super::*;
use crate::components::MirrorImageProfile;

fn profile() -> MirrorImageProfile {
    MirrorImageProfile {
        ability: AbilityId(123),
        chance_per_10k: 10_000,
        cooldown_ticks: 12,
        split_ticks: 3,
        lifetime_ticks: 20,
        displacement: 12 * SUBUNITS_PER_WORLD_UNIT,
        images: 1,
        damage_dealt_per_10k: 5000,
        damage_taken_per_10k: 20_000,
    }
}
fn caster(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 50, 10, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::MirrorImage(profile())),
            action_timing: ActionTimingProfile {
                cast_point_ticks: 2,
                cast_ticks: 4,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn victim(sim: &mut Simulation, sapper: bool, evasive: bool) -> SimId {
    sim.spawn_unit_with_properties(
        unit(1, 80, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: sapper,
                ..Default::default()
            },
            passive_effects: if evasive {
                PassiveUnitEffects::single(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                    ability: AbilityId(2),
                    chance_per_10k: 10_000,
                }))
            } else {
                PassiveUnitEffects::EMPTY
            },
            ..Default::default()
        },
    )
}

#[test]
fn mirror_image_waits_for_native_cast_point_and_split_then_recovers_and_expires() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    victim(&mut sim, true, false);
    sim.step(); // Native units do not attack on their creation tick.
    sim.step();
    let state = sim.unit(source).unwrap().status.mirror_image.unwrap();
    assert_eq!(state.cast_tick, Some(3));
    assert!(state.split_tick.is_none());
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..5 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    let images: Vec<_> = sim
        .units()
        .into_iter()
        .filter(|unit| unit.classifications.illusion)
        .collect();
    assert_eq!(images.len(), 1);
    let image = images[0];
    assert_eq!(image.owner, sim.unit(source).unwrap().owner);
    assert!(!image.classifications.summoned);
    assert!(image.status.mirror_image.is_none());
    assert!(image.status.illusion.is_some());
    assert_ne!(image.position, sim.unit(source).unwrap().position);
    assert_eq!(image.health, sim.unit(source).unwrap().health);
    let expiry = image.status.illusion.unwrap().expires_tick;
    let mut restored = wire_restored(&sim, 3);
    while sim.tick() <= expiry {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert!(sim.unit(image.id).is_none());
    assert!(sim.corpses().is_empty());
}

#[test]
fn source_damage_dispatch_requires_registered_sapper_victim_and_a_hit_not_damage_amount() {
    for (sapper, evasive, expected) in [
        (true, false, true),
        (false, false, false),
        (true, true, false),
    ] {
        let mut sim = simulation(1);
        let source = caster(&mut sim);
        let e = entity(&sim, source);
        sim.world
            .entity_mut(e)
            .get_mut::<AttackProfile>()
            .unwrap()
            .damage = 0;
        victim(&mut sim, sapper, evasive);
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(source)
                .unwrap()
                .status
                .mirror_image
                .unwrap()
                .cast_tick
                .is_some(),
            expected
        );
    }
}

#[test]
fn native_mirror_split_is_invulnerable_and_suspends_orders_without_changing_intrinsic_flags() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    victim(&mut sim, true, false);
    for _ in 0..4 {
        sim.step();
    }
    let units = sim.snapshot_units();
    let projected = units.iter().find(|unit| unit.id == source).unwrap();
    assert!(projected.classifications.invulnerable);
    assert!(projected.attacks_disabled && projected.orders_suspended);
    assert!(
        !sim.world
            .get::<UnitClassifications>(projected.entity)
            .unwrap()
            .invulnerable
    );
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..3 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    let units = sim.snapshot_units();
    let projected = units.iter().find(|unit| unit.id == source).unwrap();
    assert!(!projected.classifications.invulnerable);
    assert!(!projected.orders_suspended);
}

#[test]
fn illusion_damage_multipliers_are_separate_from_armor_and_native_spell_resistance() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    let target = victim(&mut sim, true, false);
    for _ in 0..7 {
        sim.step();
    }
    let image = sim
        .units()
        .into_iter()
        .find(|unit| unit.classifications.illusion)
        .unwrap();
    let before = sim.unit(target).unwrap().health;
    sim.step();
    assert_eq!(before - sim.unit(target).unwrap().health, 5);
    let mut units = sim.snapshot_units();
    let image = units.iter_mut().find(|unit| unit.id == image.id).unwrap();
    let before = image.health;
    super::super::status::apply_ability_effect_to_unit(
        image,
        AbilityEffect::Damage { amount: 10 },
        Team(1),
        sim.tick(),
        DamageRules::warcraft_frozen_throne(),
    );
    assert_eq!(before - image.health, 20);
    assert!(sim.unit(source).is_some());
}

#[test]
fn mirror_image_recast_unsummons_old_images_and_native_control_rejects_damage_orders() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    victim(&mut sim, true, false);
    for _ in 0..7 {
        sim.step();
    }
    let old = sim
        .units()
        .into_iter()
        .find(|unit| unit.classifications.illusion)
        .unwrap()
        .id;
    let mut units = sim.snapshot_units();
    let original = units.iter_mut().find(|unit| unit.id == source).unwrap();
    original.status.mirror_image.as_mut().unwrap().ready_tick = sim.tick();
    original.abilities_disabled = true;
    super::super::mirror_images::observe_source_damage(original, sim.tick(), 1);
    assert!(original.status.mirror_image.unwrap().cast_tick.is_none());
    original.abilities_disabled = false;
    super::super::mirror_images::observe_source_damage(original, sim.tick(), 1);
    assert!(original.status.mirror_image.unwrap().cast_tick.is_some());
    let e = entity(&sim, source);
    sim.world.entity_mut(e).insert(original.status);
    for _ in 0..3 {
        sim.step();
    }
    assert!(sim.unit(old).is_none());
    assert!(sim.corpses().is_empty());
}

#[test]
fn missile_damage_orders_mirror_at_impact_and_stun_cancels_uncommitted_cast() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    let e = entity(&sim, source);
    sim.world
        .entity_mut(e)
        .get_mut::<AttackProfile>()
        .unwrap()
        .delivery = AttackDelivery::RangedGuaranteedHit {
        speed_per_tick: 2 * SUBUNITS_PER_WORLD_UNIT,
    };
    victim(&mut sim, true, false);
    for _ in 0..10 {
        sim.step();
        assert_eq!(
            sim.unit(source)
                .unwrap()
                .status
                .mirror_image
                .unwrap()
                .event_sequence,
            0
        );
    }
    while sim
        .unit(source)
        .unwrap()
        .status
        .mirror_image
        .unwrap()
        .cast_tick
        .is_none()
    {
        sim.step();
    }
    let status = sim.unit(source).unwrap().status;
    assert_eq!(status.mirror_image.unwrap().event_sequence, 1);
    assert!(status.mirror_image.unwrap().split_tick.is_none());
    let tick = sim.tick();
    sim.world
        .entity_mut(e)
        .get_mut::<StatusState>()
        .unwrap()
        .stunned_until_tick = tick + 5;
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..6 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    let state = sim.unit(source).unwrap().status.mirror_image.unwrap();
    assert!(state.cast_tick.is_none() && state.split_tick.is_none());
    assert_eq!(
        state.ready_tick, 0,
        "an interrupted cast must not spend its native cooldown"
    );
    assert!(
        sim.units()
            .iter()
            .all(|unit| !unit.classifications.illusion)
    );
}

#[test]
fn mirror_purges_native_buff_families_but_preserves_intrinsic_and_script_grants() {
    let mut status = StatusState::default();
    status.armor_modifiers[0] = TimedArmorModifier {
        undispellable: false,
        id: ModifierId(1),
        native_buff: Some(crate::NativeBuffIdentity {
            rawcode: 2,
            positive: true,
            stealable: false,
            organic_only: false,
        }),
        expires_tick: 100,
        armor_bonus_per_100: 100,
        ..Default::default()
    };
    status.armor_modifiers[1] = TimedArmorModifier {
        undispellable: false,
        id: ModifierId(3),
        expires_tick: u64::MAX,
        armor_bonus_per_100: 300,
        ..Default::default()
    };
    status.armor_modifier_count = 2;
    super::super::mirror_images::clear_native_magic(&mut status);
    assert_eq!(status.armor_modifier_count, 1);
    assert_eq!(status.armor_modifiers[0].id, ModifierId(3));
    assert!(status.armor_modifiers[0].native_buff.is_none());
}
