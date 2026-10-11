use super::*;
use crate::components::{ManaProfile, NativeSummonProfile, PurgeProfile, PurgeState};
use crate::{MapVersion, NativeParasiteProfile};

fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn body(sim: &mut Simulation, team: Team, position: SimPoint) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team,
            position,
            health: 1000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 10,
                acquisition_range: 500,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn purge() -> SirenPurgeProfile {
    SirenPurgeProfile {
        parent: NativeParasiteProfile {
            buff_rawcode: 960,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: 100,
            affects_invulnerable: false,
            duration_ticks: 1,
            hero_duration_ticks: 1,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        child: PurgeProfile {
            ability: AbilityId(950),
            range: 500,
            targets: AttackTargetMask::AIR_AND_GROUND,
            movement_factor_per_10k: 50_000,
            minimum_speed_per_tick: 1,
            duration_ticks: 100,
            hero_duration_ticks: 30,
            pause_ticks: 10,
            hero_pause_ticks: 0,
            summoned_damage: 150,
        },
        summon_ability: AbilityId(951),
        summon_delay_ticks: 3,
        resume_delay_ticks: 4,
        parent_nonhero_only: true,
    }
}
fn summon() -> NativeSummonProfile {
    NativeSummonProfile {
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        ability: AbilityId(951),
        body_rawcode: u32::from_be_bytes(*b"n00W"),
        count: 2,
        duration_ticks: 20,
        forward_distance: 30,
    }
}
fn caster(sim: &mut Simulation, mana: i32) -> SimId {
    let id = body(sim, Team(0), SimPoint::new(200, 200));
    sim.world.entity_mut(entity(sim, id)).insert((
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 300,
                starting: mana,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(949),
                mana_cost: 50,
                cooldown_ticks: 100,
                range: 500,
                target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                effect: AbilityEffect::SirenPurge(purge()),
            },
        },
        ManaState {
            current: mana,
            regen_remainder_per_10k: 0,
        },
        AutomaticAbilityState {
            ready_tick: 0,
            cast_sequence: 0,
            autocast_enabled: true,
            manual_cast_requested: false,
        },
        ActionTimingProfile {
            cast_point_ticks: 2,
            cast_ticks: 3,
            ..Default::default()
        },
    ));
    sim.configure_additional_automatic_abilities(
        id,
        &[AutomaticAbilityProfile {
            id: AbilityId(951),
            mana_cost: 70,
            cooldown_ticks: 200,
            range: 0,
            target_policy: AbilityTargetPolicy::ScriptedController,
            effect: AbilityEffect::NativeSummon(summon()),
        }],
    )
    .unwrap();
    id
}
#[test]
fn delayed_manual_summon_spends_its_own_resources_and_continues_after_wire_restore() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = caster(&mut sim, 200);
    body(&mut sim, Team(1), SimPoint::new(210, 200));
    for _ in 0..4 {
        sim.step();
    }
    assert_eq!(
        sim.world
            .get::<ManaState>(entity(&sim, source))
            .unwrap()
            .current,
        150
    );
    assert!(sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::ImmediateSummonOrder { .. })
    )));
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = Simulation::new(sim.config.clone(), 4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 0..8 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(
        sim.world
            .get::<ManaState>(entity(&sim, source))
            .unwrap()
            .current,
        80
    );
    let children: Vec<_> = sim
        .world
        .iter_entities()
        .filter(|e| {
            e.get::<ContentIdentity>()
                .is_some_and(|c| c.rawcode == summon().body_rawcode)
        })
        .collect();
    assert_eq!(children.len(), 2);
    for child in children {
        assert!(child.get::<UnitClassifications>().unwrap().summoned);
        assert!(child.get::<StatusState>().unwrap().spawn_building.is_none());
        assert!(
            child
                .get::<PassiveUnitEffects>()
                .unwrap()
                .iter()
                .any(|p| matches!(p, PassiveUnitEffect::HardenedSkin(_)))
        );
    }
    for _ in 0..30 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(!sim.world.iter_entities().any(|e| {
        e.get::<ContentIdentity>()
            .is_some_and(|c| c.rawcode == summon().body_rawcode)
    }));
}
#[test]
fn failed_manual_summon_keeps_parent_cost_without_spending_secondary_cooldown() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = caster(&mut sim, 100);
    body(&mut sim, Team(1), SimPoint::new(210, 200));
    for _ in 0..12 {
        sim.step();
    }
    assert_eq!(
        sim.world
            .get::<ManaState>(entity(&sim, source))
            .unwrap()
            .current,
        50
    );
    let slot = sim
        .world
        .get::<AdditionalAutomaticAbilities>(entity(&sim, source))
        .unwrap()
        .get(AbilityId(951))
        .unwrap();
    assert_eq!(slot.state.cast_sequence, 0);
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::SummonFamily { .. })
    )));
}
#[test]
fn purge_child_failure_still_schedules_followup_and_native_pause_has_its_own_clock() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = caster(&mut sim, 200);
    let target = body(&mut sim, Team(1), SimPoint::new(210, 200));
    let mut units = sim.snapshot_units();
    let i = find_unit_index(&units, target).unwrap();
    let s = find_unit_index(&units, source).unwrap();
    let source = AbilitySourceSnapshot {
        map_version: None,
        source: AbilitySourceIndex::Unit(s),
        id: source,
        team: Team(0),
        origin: AbilitySourceOrigin::Unit(units[s].position),
        health: 1000,
        stunned_until_tick: 0,
        spellcasting: units[s].spellcasting,
        mana_current: units[s].mana_current,
        ability_state: units[s].ability_state,
    };
    units[i].classifications.invulnerable = true;
    assert_eq!(sim.cast_siren_purge(source, i, purge(), &mut units), 0);
    assert!(units[i].status.purge.is_none());
    assert!(sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::ImmediateSummonOrder { .. })
    )));
    units[i].classifications.invulnerable = false;
    units[i].classifications.summoned = true;
    units[i].status.stunned_until_tick = 2;
    assert_eq!(sim.cast_siren_purge(source, i, purge(), &mut units), 1);
    assert!(units[i].health < 1000);
    assert!(units[i].status.is_stunned(9));
    assert!(!units[i].status.is_stunned(10));
    units[i].mana_current = Some(270);
    sim.cast_siren_purge(source, i, purge(), &mut units);
    assert_eq!(units[i].mana_current, Some(270));
    let mut state = units[i].status;
    advance_purge(&mut state, 99);
    assert_eq!(state.movement_modifiers[0].percent_delta, -80);
    advance_purge(&mut state, 100);
    assert!(state.purge.is_none());
    assert_eq!(state.movement_modifiers[0].percent_delta, 0);
    let mut hero = purge().child;
    hero.hero_pause_ticks = 0;
    state.purge = Some(PurgeState {
        profile: hero,
        applied_tick: 100,
        pause_until_tick: 100,
        expires_tick: 130,
    });
    assert!(!state.is_stunned(100));
}

#[test]
fn purge_removes_both_magic_polarities_but_keeps_permanent_modifiers_and_stun() {
    let mut state = StatusState {
        native_stun_until_tick: 17,
        stunned_until_tick: 17,
        ..Default::default()
    };
    for (id, expiry) in [
        (100, 100),
        (101, 100),
        (102, u64::MAX),
        (103, 100),
        (104, 100),
    ] {
        super::super::status::apply_timed_armor_modifier(
            &mut state,
            TimedArmorModifier {
                undispellable: id == 104,
                id: ModifierId(id),
                aura_membership: id == 103,
                native_buff: Some(crate::NativeBuffIdentity {
                    rawcode: id,
                    positive: id == 100,
                    stealable: true,
                    organic_only: false,
                }),
                armor_bonus_per_100: if id == 101 { -100 } else { 100 },
                expires_tick: expiry,
                ..Default::default()
            },
        );
    }
    state.purge = Some(PurgeState {
        profile: purge().child,
        applied_tick: 0,
        pause_until_tick: 10,
        expires_tick: 100,
    });
    advance_purge(&mut state, 0);
    dispel_magic_buffs(&mut state);
    assert_eq!(state.armor_modifier_count, 3);
    assert_eq!(state.armor_modifiers[2].id, ModifierId(104));
    assert_eq!(state.armor_modifiers[1].id, ModifierId(103));
    assert_eq!(state.armor_modifiers[0].id, ModifierId(102));
    assert!(state.purge.is_some());
    assert_eq!(state.stunned_until_tick, 17);
}
#[test]
fn native_summon_recast_retires_old_family_and_caster_removal_retires_new_family() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = caster(&mut sim, 200);
    let mut units = sim.snapshot_units();
    let s = find_unit_index(&units, source).unwrap();
    assert_eq!(sim.cast_native_summon(s, summon(), &mut units), 2);
    units = sim.snapshot_units();
    let s = find_unit_index(&units, source).unwrap();
    let old: Vec<_> = units
        .iter()
        .filter(|u| u.id != source)
        .map(|u| u.id)
        .collect();
    assert_eq!(sim.cast_native_summon(s, summon(), &mut units), 2);
    for &id in &old {
        assert_eq!(units[find_unit_index(&units, id).unwrap()].health, 0);
    }
    // Newly created handles have default status and independent native mana/abilities.
    let fresh: Vec<_> = sim
        .snapshot_units()
        .into_iter()
        .filter(|u| u.id != source && !old.contains(&u.id))
        .collect();
    assert_eq!(fresh.len(), 2);
    for child in &fresh {
        assert!(child.status.purge.is_none());
        assert!(child.status.pending_cast.is_none());
    }
    sim.world.despawn(entity(&sim, source));
    let mut units = sim.snapshot_units();
    sim.resolve_native_actions(&mut units, &mut []);
    for child in &fresh {
        assert_eq!(units[find_unit_index(&units, child.id).unwrap()].health, 0);
    }
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::SummonFamily { .. })
    )));
}

#[test]
fn hardened_skin_uses_latest_successful_grant_then_intrinsic_fallback() {
    let grant = |id, chance, amount| crate::NativeAbilityGrant {
        ability: AbilityId(id),
        effect: crate::NativeAbilityGrantEffect::HardenedSkin {
            chance_per_10k: chance,
            ignored_damage: amount,
            minimum_damage: 0,
        },
    };
    let mut sim = Simulation::new(Default::default(), 1);
    let target = body(&mut sim, Team(0), SimPoint::new(200, 200));
    sim.world
        .entity_mut(entity(&sim, target))
        .insert(PassiveUnitEffects::from_slice(&[
            PassiveUnitEffect::HardenedSkin(grant(900, 10000, 9)),
        ]));
    let mut unit = sim.snapshot_units()[0];
    unit.status.native_grants = crate::NativeAbilityGrants::from_slice(&[
        grant(700, 10000, 3),
        grant(600, 10000, 7),
        grant(400, 0, 20),
    ]);
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut unit, 100, SimId(1), 0),
        93
    );
    assert_eq!(unit.status.hardened_skin_sequence, 1);
    unit.status.native_grants = crate::NativeAbilityGrants::from_slice(&[grant(400, 0, 20)]);
    assert_eq!(
        super::super::night_elf::hardened_skin_after_armor(&mut unit, 100, SimId(1), 0),
        91
    );
    assert_eq!(unit.status.hardened_skin_sequence, 2);
    let left =
        crate::NativeAbilityGrants::from_slice(&[grant(700, 10000, 3), grant(600, 10000, 7)]);
    let right =
        crate::NativeAbilityGrants::from_slice(&[grant(600, 10000, 7), grant(700, 10000, 3)]);
    assert_ne!(left, right);
    let wire = serde_json::to_string(&left).unwrap();
    assert_eq!(
        serde_json::from_str::<crate::NativeAbilityGrants>(&wire).unwrap(),
        left
    );
}

#[test]
fn native_parent_flight_buff_ownership_and_timed_body_survive_wire_restore() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = body(&mut sim, Team(0), SimPoint::new(200, 200));
    let target = body(&mut sim, Team(1), SimPoint::new(450, 200));
    let owner = sim.unit(source).unwrap().owner;
    let mut profile = purge().parent;
    profile.duration_ticks = 5;
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::Parasite(crate::components::NativeParasiteState {
            source,
            owner,
            team: Team(0),
            target,
            profile,
            position: SimPoint::new(200, 200),
            buff_until_tick: None,
        }),
    ));
    sim.step();
    let bundle = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, bundle).unwrap();
    let mut restored = Simulation::new(sim.config.clone(), 4);
    restored.restore_snapshot(&snapshot).unwrap();
    sim.world.despawn(entity(&sim, source));
    restored.world.despawn(entity(&restored, source));
    for _ in 0..2 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    for simulation in [&mut sim, &mut restored] {
        let attacker = body(simulation, Team(0), SimPoint::new(455, 200));
        let mut attack = simulation
            .world
            .get_mut::<AttackProfile>(entity(simulation, attacker))
            .unwrap();
        attack.damage = 5000;
        attack.range = 100;
        attack.cooldown_ticks = 1;
    }
    // The attacker cannot act on its creation tick. The following ordinary hit kills under the buff.
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    let child = sim
        .units()
        .into_iter()
        .find(|u| u.content.is_some_and(|c| c.rawcode == profile.body_rawcode))
        .expect("death summons a real native body");
    assert_eq!(child.owner, owner);
    assert!(child.classifications.summoned && !child.classifications.combat_sapper);
    assert!(child.mechanical);
    assert!(!child.attack_targets.can_target_unit(MovementClass::Ground));
    // Restore while the ephemeral body and its native deadline exist, not just the missile.
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, bundle).unwrap();
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert!(sim.unit(child.id).is_none());

    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let target = body(&mut sim, Team(1), SimPoint::new(200, 200));
    for owner in [PlayerId(0), PlayerId(1)] {
        let id = sim.allocate_id();
        sim.world.spawn((
            id,
            NativeAction::Parasite(crate::components::NativeParasiteState {
                source: SimId(100),
                owner,
                team: Team(0),
                target,
                profile,
                position: SimPoint::new(200, 200),
                buff_until_tick: None,
            }),
        ));
    }
    sim.step();
    let buffs: Vec<_> = sim
        .world
        .iter_entities()
        .filter_map(|e| {
            if let Some(NativeAction::Parasite(p)) = e.get::<NativeAction>() {
                Some(p.owner)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(buffs, [PlayerId(1)]);
    for _ in 0..5 {
        sim.step();
    }
    sim.world
        .get_mut::<Health>(entity(&sim, target))
        .unwrap()
        .current = 0;
    sim.step();
    assert!(
        sim.units()
            .into_iter()
            .all(|u| !u.content.is_some_and(|c| c.rawcode == profile.body_rawcode))
    );
}

#[test]
fn native_family_keeps_corpse_handle_rebinds_on_resurrection_and_retires_on_ability_removal() {
    let mut sim = Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            ..Default::default()
        },
        1,
    );
    let source = sim.spawn_resolved_unit_unchecked(
        Some(PlayerId(0)),
        UnitSpawn::from_template(
            Team(0),
            SimPoint::new(200, 200),
            crate::CastleFightUnitKind::NagaSiren
                .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
                .unwrap()
                .template(),
        ),
        crate::CastleFightUnitKind::NagaSiren
            .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap()
            .resolved(),
    );
    let mut units = sim.snapshot_units();
    let mut profile = summon();
    profile.ability = AbilityId(u32::from_be_bytes(*b"A02S"));
    sim.cast_native_summon(
        find_unit_index(&units, source).unwrap(),
        profile,
        &mut units,
    );
    let children: Vec<_> = sim
        .snapshot_units()
        .into_iter()
        .filter(|u| u.id != source)
        .map(|u| u.id)
        .collect();
    let definition = crate::CastleFightUnitKind::NagaSiren
        .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
        .unwrap()
        .resolved();
    sim.world.despawn(entity(&sim, source));
    let corpse_id = sim.allocate_id();
    sim.world.spawn((
        corpse_id,
        Position(SimPoint::new(200, 200)),
        Corpse {
            source_unit: source,
            source_owner: PlayerId(0),
            source_team: Team(0),
            definition: CorpseDefinitionId(definition.properties.content.unwrap().rawcode),
            created_tick: 0,
            decay_start_tick: 0,
            expires_tick: Some(10),
            resurrection: Some(definition),
            shrine_state: ShrineRevivalState::default(),
            vertex_rgba: None,
        },
    ));
    sim.step();
    assert!(children.iter().all(|&id| sim.unit(id).is_some()));
    assert_eq!(
        sim.resurrect_friendly_corpses(
            Team(0),
            AbilitySourceOrigin::Unit(SimPoint::new(200, 200)),
            10,
            1
        ),
        1
    );
    let revived = sim
        .snapshot_units()
        .into_iter()
        .find(|u| u.rawcode == definition.properties.content.unwrap().rawcode)
        .unwrap()
        .id;
    sim.step();
    assert!(children.iter().all(|&id| sim.unit(id).is_some()));
    if sim
        .world
        .get::<SpellcastingProfile>(entity(&sim, revived))
        .unwrap()
        .ability
        .id
        == profile.ability
    {
        sim.world
            .entity_mut(entity(&sim, revived))
            .remove::<(SpellcastingProfile, AutomaticAbilityState)>();
    } else {
        sim.configure_additional_automatic_abilities(revived, &[])
            .unwrap();
    }
    sim.step();
    assert!(children.iter().all(|&id| sim.unit(id).is_none()));
}

#[test]
fn purge_distinguishes_physical_weapon_frost_from_magic_frost_slow() {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    body(&mut sim, Team(1), SimPoint::new(200, 200));
    body(&mut sim, Team(1), SimPoint::new(400, 200));
    let mut units = sim.snapshot_units();
    let profile = crate::FrostAttackEffectProfile {
        targets: AttackTargetMask::AIR_AND_GROUND,
        ability: AbilityId(701),
        movement_percent_delta: -40,
        attack_speed_percent_delta: -30,
        duration_ticks: 30,
        hero_duration_ticks: 10,
    };
    apply_native_frost_attack(&mut units[0], profile, 0);
    apply_frost_attack(&mut units[1], profile, 0);
    for u in &mut units {
        dispel_magic_buffs(&mut u.status);
    }
    assert_eq!(units[0].status.movement_modifier_count, 1);
    assert_eq!(units[0].status.attack_speed_modifier_count, 1);
    assert_eq!(units[1].status.movement_modifier_count, 0);
    assert_eq!(units[1].status.attack_speed_modifier_count, 0);
    let before = units[0].status;
    let bytes = serde_json::to_vec(&before).unwrap();
    let mut after: StatusState = serde_json::from_slice(&bytes).unwrap();
    dispel_magic_buffs(&mut after);
    assert_eq!(before, after);
}
