use super::*;
use crate::{AdditionalAutomaticAbilityDefinitions, CastleFightUnitKind, ManaProfile};

#[test]
fn promoted_intrinsic_shrine_classifications_follow_retained_inventory() {
    let tsv = include_str!("../../../../../../docs/original_map/extracted/resolved/units.tsv");
    let headers: Vec<_> = tsv.lines().next().unwrap().split('\t').collect();
    let rawcode = headers
        .iter()
        .position(|header| *header == "rawcode")
        .unwrap();
    let abilities = headers
        .iter()
        .position(|header| *header == "abilities")
        .unwrap();
    let classes = headers
        .iter()
        .position(|header| *header == "classifications")
        .unwrap();
    let shrine =
        crate::golden_shrine_definition_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    for kind in CastleFightUnitKind::ALL {
        let definition = kind.definition();
        let row = tsv
            .lines()
            .skip(1)
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .find(|row| row[rawcode].as_bytes() == definition.rawcode.to_be_bytes())
            .unwrap();
        let inventory = row[abilities]
            .split(',')
            .filter(|ability| ability.len() == 4)
            .map(|ability| u32::from_be_bytes(ability.as_bytes().try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            definition.classifications.legendary,
            inventory.contains(&shrine.parameters.exclude_legendary_marker_ability_id)
        );
        assert_eq!(
            definition.classifications.summoned_marker,
            inventory.contains(&shrine.parameters.exclude_summoned_unit_marker_ability_id)
        );
        assert_eq!(
            definition.classifications.combat_sapper,
            row[classes]
                .split(',')
                .any(|class| class.eq_ignore_ascii_case("sapper"))
        );
        assert_eq!(
            definition.classifications.summoned,
            row[classes]
                .split(',')
                .any(|class| class.eq_ignore_ascii_case("summoned"))
        );
    }
}

#[test]
fn replacement_uses_cold_content_stats_mana_and_all_ability_definitions_not_dead_runtime() {
    let mut sim = ready_sim();
    let pending_entity = sim
        .world
        .iter_entities()
        .find(|entity| entity.get::<DelayedShrineRevival>().is_some())
        .unwrap()
        .id();
    let old_pending = *sim
        .world
        .get::<DelayedShrineRevival>(pending_entity)
        .unwrap();
    let mut cold = definition();
    cold.properties.content = Some(ContentIdentity {
        map_version: crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION,
        rawcode: CastleFightUnitKind::Footman.definition().rawcode,
        name: "synthetic baseline",
    });
    cold.properties.armor = ArmorProfile {
        armor_type: ArmorType::Large,
        armor_points: 7,
    };
    let primary = AutomaticAbilityProfile {
        id: AbilityId(1),
        mana_cost: 2,
        cooldown_ticks: 5,
        range: 100,
        target_policy: AbilityTargetPolicy::NearestEnemyInCombat,
        effect: AbilityEffect::Damage { amount: 3 },
    };
    let secondary = AutomaticAbilityProfile {
        id: AbilityId(2),
        ..primary
    };
    cold.spellcasting = Some(SpellcastingProfile {
        mana: ManaProfile {
            maximum: 50,
            starting: 13,
            regen_per_tick_per_10k: 10,
        },
        ability: primary,
    });
    cold.additional_abilities =
        Some(AdditionalAutomaticAbilityDefinitions::try_from_profiles([secondary]).unwrap());
    // Replace the initial pending fixture with a real spellcaster death using the same successful
    // deterministic ID/tick/seed, then corrupt only hot ECS state before the death.
    sim.world.despawn(pending_entity);
    let corpse = sim
        .world
        .iter_entities()
        .find(|entity| entity.get::<Corpse>().is_some())
        .unwrap()
        .id();
    sim.world.despawn(corpse);
    sim.next_id = old_pending.source_unit.0;
    let id = unit(&mut sim, cold);
    let caster = entity(&sim, id);
    sim.world.get_mut::<Health>(caster).unwrap().max = 999;
    sim.world.get_mut::<AttackProfile>(caster).unwrap().damage = 999;
    sim.world
        .get_mut::<ArmorProfile>(caster)
        .unwrap()
        .armor_points = 99;
    sim.world.get_mut::<ManaState>(caster).unwrap().current = 0;
    sim.world
        .get_mut::<AutomaticAbilityState>(caster)
        .unwrap()
        .ready_tick = 10_000;
    sim.world
        .get_mut::<AutomaticAbilityState>(caster)
        .unwrap()
        .cast_sequence = 99;
    sim.world
        .get_mut::<AutomaticAbilityState>(caster)
        .unwrap()
        .autocast_enabled = false;
    sim.world
        .get_mut::<StatusState>(caster)
        .unwrap()
        .stunned_until_tick = 10_000;
    sim.world
        .get_mut::<AttackCooldown>(caster)
        .unwrap()
        .remaining = 9;
    fatality(&mut sim, id);
    assert_eq!(pending(&sim), 1);
    let mut restored = restored(&sim);
    sim.next_tick = old_pending.due_tick;
    restored.next_tick = old_pending.due_tick;
    sim.resolve_shrine_revivals();
    restored.resolve_shrine_revivals();
    assert_eq!(sim.checksum(), restored.checksum());
    let newborn = sim.units()[0].id;
    let newborn = entity(&sim, newborn);
    assert_eq!(
        sim.world.get::<Health>(newborn).unwrap().max,
        cold.template.health
    );
    assert_eq!(
        *sim.world.get::<AttackProfile>(newborn).unwrap(),
        cold.template.attack
    );
    assert_eq!(
        *sim.world.get::<ArmorProfile>(newborn).unwrap(),
        cold.properties.armor
    );
    assert_eq!(
        sim.world.get::<ContentIdentity>(newborn).unwrap().rawcode,
        cold.properties.content.unwrap().rawcode
    );
    assert_eq!(sim.world.get::<ManaState>(newborn).unwrap().current, 13);
    assert_eq!(
        sim.world
            .get::<ManaState>(newborn)
            .unwrap()
            .regen_remainder_per_10k,
        0
    );
    assert_eq!(
        sim.world
            .get::<StatusState>(newborn)
            .unwrap()
            .stunned_until_tick,
        0
    );
    assert_eq!(
        sim.world.get::<AttackCooldown>(newborn).unwrap().remaining,
        0
    );
    let primary_state = sim.world.get::<AutomaticAbilityState>(newborn).unwrap();
    assert_eq!(primary_state.ready_tick, sim.next_tick);
    assert_eq!(primary_state.cast_sequence, 0);
    assert!(primary_state.autocast_enabled);
    let additional = sim
        .world
        .get::<AdditionalAutomaticAbilities>(newborn)
        .unwrap();
    for entry in additional.iter() {
        assert_eq!(entry.profile, secondary);
        assert_eq!(entry.state.ready_tick, sim.next_tick);
        assert_eq!(entry.state.cast_sequence, 0);
        assert_eq!(
            entry.secondary_resurrection,
            SecondaryResurrectionState::default()
        );
    }
}

#[test]
fn real_combat_deaths_schedule_even_without_a_raisable_corpse() {
    let mut success = false;
    for seed in 0..100 {
        let mut sim = simulation(1, seed);
        shrine(&mut sim, Team(0), 20);
        let mut cold = definition();
        cold.properties.corpse = None;
        cold.properties.mechanical = true;
        cold.properties.movement_class = MovementClass::Air;
        cold.properties.classifications.hero = true; // Hero is NOT the legendary marker.
        let victim = unit(&mut sim, cold);
        let mut attacker = cold.template;
        attacker.attack.damage = 1_000;
        attacker.attack.range = 1_000;
        attacker.attack.acquisition_range = 1_000;
        sim.spawn_unit(UnitSpawn::from_template(
            Team(1),
            SimPoint::new(60, 11),
            attacker,
        ));
        sim.step();
        sim.step();
        assert!(sim.unit(victim).is_none());
        assert_eq!(sim.corpse_count(), 0);
        if pending(&sim) == 1 {
            let due = sim
                .world
                .iter_entities()
                .find_map(|entity| {
                    entity
                        .get::<DelayedShrineRevival>()
                        .map(|pending| pending.due_tick)
                })
                .unwrap();
            // Remove enemies/shrine: committed callbacks do not recheck the live team chance.
            let originals: Vec<_> = sim
                .world
                .iter_entities()
                .filter(|entity| {
                    entity.get::<MovementProfile>().is_some()
                        || entity.get::<BuildingFootprint>().is_some()
                })
                .map(|entity| entity.id())
                .collect();
            for entity in originals {
                sim.world.despawn(entity);
            }
            sim.next_tick = due;
            sim.resolve_shrine_revivals();
            assert_eq!(sim.unit_count(), 1);
            assert_eq!(sim.units()[0].owner, PlayerId(0));
            success = true;
            break;
        }
    }
    assert!(success);
}

#[test]
fn shrine_building_native_regeneration_keeps_fractional_state_across_wire_restore() {
    let mut sim = simulation(1, 0);
    let id = shrine(&mut sim, Team(0), 20);
    let building = entity(&sim, id);
    let max = sim.world.get::<Health>(building).unwrap().max;
    sim.world.get_mut::<Health>(building).unwrap().current = max - 100;
    sim.step();
    assert!(
        sim.world
            .get::<HealthRegeneration>(building)
            .unwrap()
            .remainder_per_10k_hz
            > 0
    );
    let mut wire_restored = restored(&sim);
    for _ in 1..CASTLE_FIGHT_SIMULATION_HZ {
        sim.step();
        wire_restored.step();
        assert_eq!(sim.checksum(), wire_restored.checksum());
    }
    let rate =
        shrine_definition(MapVersion::CASTLE_FIGHT_9_27).building_health_regen_per_second_per_10k;
    assert_eq!(
        sim.world.get::<Health>(building).unwrap().current,
        max - 100 + (rate / 10_000) as i32
    );
    assert_eq!(
        sim.world
            .get::<HealthRegeneration>(building)
            .unwrap()
            .remainder_per_10k_hz,
        0
    );
    sim.world.get_mut::<Health>(building).unwrap().current = 0;
    sim.advance_cooldowns();
    assert_eq!(sim.world.get::<Health>(building).unwrap().current, 0);
}

#[test]
fn actual_death_guard_and_incomplete_construction_do_not_grant_revival() {
    let mut sim = ready_sim();
    let id = unit(&mut sim, definition());
    let caster = entity(&sim, id);
    let before = pending(&sim);
    let support = sim.golden_shrine_support(Team(0));
    sim.schedule_shrine_revival(
        caster,
        PlayerId(0),
        Team(0),
        SimPoint::new(50, 11),
        1,
        support,
    );
    assert_eq!(pending(&sim), before);
    let mut sim = simulation(1, 0);
    let definition = CastleFightTowerKind::GoldenShrineOfJustice.definition();
    let spawn = definition.spawn(Team(0), BuildingFootprint::new(20, 20, 4, 4));
    let mut properties = definition.gameplay_properties();
    properties.construction_time_ticks = Some(3); // Synthetic duration exercises lifecycle, not tuning.
    properties.economy = None; // Direct construction fixture bypasses purchasing.
    let id = sim
        .try_start_building_construction(PlayerId(0), spawn, properties)
        .unwrap();
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), 0);
    assert!(sim.transfer_golden_shrine_owner(id, PlayerId(1)));
    assert_eq!(sim.golden_shrine_revive_chance(Team(1)), 0);
    let mut restored = restored(&sim);
    for _ in 0..=3 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(
        sim.golden_shrine_revive_chance(Team(1)),
        shrine_definition(MapVersion::CASTLE_FIGHT_9_27)
            .parameters
            .chance_percent_per_shrine
    );
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), 0);
    assert!(sim.remove_building(id));
    assert_eq!(sim.golden_shrine_revive_chance(Team(1)), 0);
}
