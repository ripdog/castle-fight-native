use super::*;
use crate::components::NativeAction;
use serde::{Deserialize, Serialize};

pub(super) fn canonical_configuration_identity(
    config: &SimulationConfig,
    combat_rules: &CombatRules,
    gameplay_bundle: Option<GameplayBundleIdentity>,
    players: &[PlayerConfig],
) -> u64 {
    const DOMAIN: u64 = 0x4346_434f_4e46_4947;
    const DAMAGE_TYPES: [DamageType; DamageType::COUNT] = [
        DamageType::Normal,
        DamageType::Pierce,
        DamageType::Siege,
        DamageType::Magic,
        DamageType::Chaos,
        DamageType::Spells,
        DamageType::Hero,
    ];
    const ARMOR_TYPES: [ArmorType; ArmorType::COUNT] = [
        ArmorType::Small,
        ArmorType::Medium,
        ArmorType::Large,
        ArmorType::Fortified,
        ArmorType::Normal,
        ArmorType::Hero,
        ArmorType::Divine,
        ArmorType::Unarmored,
    ];

    let mut hash = Fnv64::new();
    hash.write_u64(DOMAIN);
    hash.write_u64(u64::from(CANONICAL_CHECKSUM_SCHEMA_VERSION));
    hash.write_i32(CASTLE_FIGHT_SIMULATION_HZ);
    match gameplay_bundle {
        Some(identity) => {
            hash.write_u8(1);
            hash.write_u32(identity.schema_version);
            hash.write_u64(identity.gameplay_hash);
        }
        None => hash.write_u8(0),
    }
    hash.write_u64(players.len() as u64);
    for player in players {
        hash.write_u8(player.id.0);
        hash.write_u8(player.team.0);
    }
    match &config.fog {
        None => hash.write_u8(0),
        Some(rules) => {
            hash.write_u8(1);
            hash.write_i32(rules.cell_size);
            hash.write_i32(rules.fallback_sight.day);
            hash.write_i32(rules.fallback_sight.night);
            hash.write_u8(u8::from(rules.initially_explored));
            hash.write_u8(u8::from(rules.night));
            hash.write_u8(u8::from(rules.attack_reveal.is_some()));
            if let Some(reveal) = rules.attack_reveal {
                hash.write_i32(reveal.radius);
                hash.write_u64(reveal.duration_ticks);
            }
            match rules.clock {
                Some(clock) => {
                    hash.write_u8(1);
                    hash.write_u64(clock.cycle_ticks);
                    hash.write_u64(clock.dawn_phase_ticks);
                    hash.write_u64(clock.dusk_phase_ticks);
                    hash.write_u64(clock.initial_phase_ticks);
                }
                None => hash.write_u8(0),
            }
            for rects in rules
                .permanent_rectangles
                .iter()
                .chain(std::iter::once(&rules.sight_blockers))
            {
                hash.write_u64(rects.len() as u64);
                for &(min, max) in rects {
                    hash.write_i32(min.x);
                    hash.write_i32(min.y);
                    hash.write_i32(max.x);
                    hash.write_i32(max.y);
                }
            }
        }
    }
    hash.write_u64(config.match_seed);
    hash.write_i32(config.spatial_cell_size);
    hash.write_i32(config.navigation_cell_size);
    hash.write_i32(config.navigation_min.x);
    hash.write_i32(config.navigation_min.y);
    hash.write_i32(config.navigation_max.x);
    hash.write_i32(config.navigation_max.y);
    hash.write_i32(config.target_pursuit_extra_range);
    hash.write_i32(config.unit_separation_distance);
    hash.write_i32(config.max_separation_per_tick);
    hash_footprint_list(&mut hash, &config.static_blockers);
    hash_footprint_list(&mut hash, &config.air_static_blockers);
    hash_footprint_list(&mut hash, &config.build_static_blockers);
    for regions in &config.team_build_regions {
        hash_footprint_list(&mut hash, regions);
    }
    for region in config.team_castle_regions {
        match region {
            Some(region) => {
                hash.write_u8(1);
                hash_building_footprint(&mut hash, region);
            }
            None => hash.write_u8(0),
        }
    }
    match config.targetless_lane {
        Some(lane) => {
            hash.write_u8(1);
            hash.write_i32(lane.min_y);
            hash.write_i32(lane.max_y);
        }
        None => hash.write_u8(0),
    }
    for objective in config.team_objective {
        hash.write_i32(objective.x);
        hash.write_i32(objective.y);
    }
    hash.write_u64(u64::from(config.economy.starting_gold));
    hash.write_u64(u64::from(config.economy.starting_lumber));
    hash.write_u16(config.economy.starting_legendary_points);
    hash.write_u64(config.economy.base_income_per_10k);
    hash.write_u64(u64::from(config.economy.income_interval_ticks));
    hash.write_u64(config.economy.income_tax_bracket_per_10k);

    hash.write_u16(combat_rules.uphill_miss_chance_per_10k);
    hash.write_u16(combat_rules.damage_rules.armor_factor_per_10k());
    for damage_type in DAMAGE_TYPES {
        for armor_type in ARMOR_TYPES {
            hash.write_u16(
                combat_rules
                    .damage_rules
                    .bonus_per_10k(damage_type, armor_type),
            );
        }
    }
    match &combat_rules.terrain_elevation {
        Some(terrain) => {
            hash.write_u8(1);
            let origin = terrain.origin();
            hash.write_i32(origin.x);
            hash.write_i32(origin.y);
            hash.write_i32(terrain.tile_size());
            hash.write_u64(u64::from(terrain.width_tiles()));
            hash.write_u64(u64::from(terrain.height_tiles()));
            for y in 0..=terrain.height_tiles() {
                for x in 0..=terrain.width_tiles() {
                    let sample = terrain
                        .vertex_sample(x, y)
                        .expect("validated terrain dimensions must expose every vertex");
                    hash.write_u8(sample.cliff_level);
                    hash.write_i32(sample.ground_height_raw);
                }
            }
        }
        None => hash.write_u8(0),
    }
    hash.finish()
}

fn hash_footprint_list(hash: &mut Fnv64, footprints: &[BuildingFootprint]) {
    let mut canonical = footprints.to_vec();
    canonical.sort_unstable_by_key(|footprint| {
        (
            footprint.min_x,
            footprint.min_y,
            footprint.width,
            footprint.height,
        )
    });
    hash.write_u64(canonical.len() as u64);
    for footprint in canonical {
        hash_building_footprint(hash, footprint);
    }
}

fn hash_building_footprint(hash: &mut Fnv64, footprint: BuildingFootprint) {
    hash.write_i32(footprint.min_x);
    hash.write_i32(footprint.min_y);
    hash.write_u16(footprint.width);
    hash.write_u16(footprint.height);
}

#[derive(Clone, Copy)]
pub(super) struct CanonicalMatchState<'a> {
    pub(super) next_tick: u64,
    pub(super) next_id: u64,
    pub(super) configuration_identity: u64,
    pub(super) defense_alerts: &'a [DefenseAlert],
    pub(super) players: &'a [PlayerState],
    pub(super) lifecycle: MatchLifecycle,
    pub(super) team_objectives: [Option<SimId>; 2],
    pub(super) gjallarhorn_constructed_count: [u32; 2],
    pub(super) shrine_death_generation: u64,
    pub(super) debug_buildings_invulnerable: bool,
    pub(super) fog: Option<&'a crate::FogOfWar>,
}

pub(super) fn canonical_checksum(world: &World, state: CanonicalMatchState<'_>) -> u64 {
    let CanonicalMatchState {
        next_tick,
        next_id,
        configuration_identity,
        defense_alerts,
        players,
        lifecycle,
        team_objectives,
        gjallarhorn_constructed_count,
        shrine_death_generation,
        debug_buildings_invulnerable,
        fog,
    } = state;
    let entities = super::snapshot::canonical_entities(world);

    let mut hash = Fnv64::new();
    hash.write_u64(0x4346_5354_4154_4503);
    hash.write_u64(u64::from(CANONICAL_CHECKSUM_SCHEMA_VERSION));
    hash.write_u64(configuration_identity);
    match fog {
        None => hash.write_u8(0),
        Some(fog) => {
            hash.write_u8(1);
            for cells in &fog.explored {
                hash.write_u64(cells.len() as u64);
                for &cell in cells {
                    hash.write_u8(cell);
                }
            }
            hash.write_u64(fog.reveals.len() as u64);
            for reveal in &fog.reveals {
                hash.write_u8(reveal.team.0);
                hash.write_i32(reveal.position.x);
                hash.write_i32(reveal.position.y);
                hash.write_i32(reveal.radius);
                hash.write_u8(u8::from(reveal.detects_invisible));
                hash.write_u64(reveal.expires_tick);
            }
            for structures in &fog.remembered_structures {
                hash.write_u64(structures.len() as u64);
                for structure in structures {
                    hash.write_u64(structure.id.0);
                    hash_content_identity(&mut hash, structure.content);
                    match structure.owner {
                        Some(owner) => {
                            hash.write_u8(1);
                            hash.write_u8(owner.0);
                        }
                        None => hash.write_u8(0),
                    }
                    hash_building_footprint(&mut hash, structure.footprint);
                    hash.write_u8(u8::from(structure.construction.is_some()));
                    if let Some(construction) = structure.construction {
                        hash.write_u64(construction.started_tick);
                        hash.write_u64(construction.complete_tick);
                        hash.write_u64(construction.observed_tick);
                    }
                }
            }
        }
    }
    hash.write_u64(next_tick);
    hash.write_u64(next_id);
    hash.write_u64(shrine_death_generation);
    hash.write_u8(u8::from(debug_buildings_invulnerable));
    match lifecycle {
        MatchLifecycle::Running => hash.write_u8(0),
        MatchLifecycle::PausedForDisconnect {
            disconnected_teams_mask,
        } => {
            hash.write_u8(1);
            hash.write_u8(disconnected_teams_mask);
        }
        MatchLifecycle::Finished {
            outcome,
            finished_tick,
        } => {
            hash.write_u8(2);
            match outcome {
                MatchOutcome::Victory(team) => {
                    hash.write_u8(0);
                    hash.write_u8(team.0);
                }
                MatchOutcome::Draw => hash.write_u8(1),
            }
            hash.write_u64(finished_tick);
        }
    }
    for objective in team_objectives {
        hash_optional_sim_id(&mut hash, objective);
    }
    for count in gjallarhorn_constructed_count {
        hash.write_u32(count);
    }
    hash.write_u64(players.len() as u64);
    for player in players {
        hash.write_u8(player.id.0);
        hash.write_u8(player.team.0);
        hash.write_u8(match player.connection {
            PlayerConnectionStatus::Connected => 0,
            PlayerConnectionStatus::Disconnected => 1,
        });
        hash.write_u64(u64::from(player.resources.gold));
        hash.write_u64(u64::from(player.resources.lumber));
        hash.write_u16(player.resources.legendary_points_used);
        hash.write_u16(player.resources.legendary_points_cap);
    }
    hash.write_u64(entities.len() as u64);
    for entity in entities {
        match entity {
            CanonicalEntity::NativeAction { id, action } => {
                hash.write_u8(10);
                hash.write_u64(id.0);
                match action {
                    NativeAction::Hailstone(state) => {
                        hash.write_u8(2);
                        hash.write_u64(state.source.0);
                        hash.write_u8(state.team.0);
                        hash.write_u64(state.target.0);
                        hash_hailstone_profile(&mut hash, state.profile);
                        hash.write_i32(state.origin.x);
                        hash.write_i32(state.origin.y);
                        hash.write_i32(state.destination.x);
                        hash.write_i32(state.destination.y);
                        hash.write_u64(state.launch_tick);
                        hash.write_u64(state.impact_tick);
                    }
                    NativeAction::Bolt(state) => {
                        hash.write_u8(1);
                        hash.write_u64(state.source.0);
                        hash.write_u8(state.team.0);
                        hash.write_u64(state.target.0);
                        hash_native_bolt_profile(&mut hash, state.profile);
                        hash.write_i32(state.launch_position.x);
                        hash.write_i32(state.launch_position.y);
                        hash.write_u64(state.launch_tick);
                        hash.write_i32(state.position.x);
                        hash.write_i32(state.position.y);
                        hash.write_u64(state.position_tick);
                        hash.write_u64(state.impact_tick);
                    }
                    NativeAction::HealingWave(state) => {
                        hash.write_u8(0);
                        hash.write_u64(state.source.0);
                        hash.write_u8(state.team.0);
                        hash_healing_wave_profile(&mut hash, state.profile);
                        hash.write_u64(state.started_tick);
                        hash.write_u8(state.jump_index);
                        hash.write_u64(state.current_target.0);
                        hash.write_i32(state.last_position.x);
                        hash.write_i32(state.last_position.y);
                        hash.write_i32(state.next_healing);
                        hash.write_u8(state.hit_count);
                        for hit in &state.hit_targets[..usize::from(state.hit_count)] {
                            hash.write_u64(hit.0);
                        }
                    }
                }
            }
            CanonicalEntity::Snowveil { id, state } => {
                hash.write_u8(14);
                hash.write_u64(id.0);
                hash.write_u16(state.version.major);
                hash.write_u16(state.version.minor);
                hash.write_u64(state.tiles.len() as u64);
                for (tile, team) in &state.tiles {
                    hash.write_u32(*tile);
                    hash.write_u8(team.0);
                }
                hash.write_u64(state.manual_ready.len() as u64);
                for (owner, tick) in &state.manual_ready {
                    hash.write_u8(owner.0);
                    hash.write_u64(*tick);
                }
            }
            CanonicalEntity::BuildingSpellTarget { id, state } => {
                hash.write_u8(12);
                hash.write_u64(id.0);
                hash.write_u64(state.target.0);
                hash.write_u16(state.version.major);
                hash.write_u16(state.version.minor);
                hash.write_u8(state.shield_level);
                hash.write_u8(u8::from(state.shield_expires_tick.is_some()));
                if let Some(tick) = state.shield_expires_tick {
                    hash.write_u64(tick);
                }
                hash.write_u8(state.anti_negative as u8);
                hash.write_u8(state.selector_excluded as u8);
                hash.write_u8(state.overheat_level);
                hash.write_u8(state.defend_disabled as u8);
                hash.write_u8(state.orders_suspended as u8);
                hash.write_u8(state.hex.is_some() as u8);
                if let Some(hex) = state.hex {
                    hash.write_u64(hex.expires_tick);
                    hash_hex_form(&mut hash, hex.form);
                }
                hash.write_u64(state.callbacks.len() as u64);
                for callback in &state.callbacks {
                    hash.write_u64(callback.due_tick);
                    hash.write_u8(match callback.action {
                        super::building_spells::ControlAction::Attack => 0,
                        super::building_spells::ControlAction::DefenderDefend => 1,
                        super::building_spells::ControlAction::DefenderAttack => 2,
                    });
                    hash_hex_profile(&mut hash, callback.profile);
                }
            }
            CanonicalEntity::Unit(unit) => {
                hash.write_u8(0);
                hash.write_u64(unit.id.0);
                hash_content_identity(&mut hash, unit.content);
                hash.write_u8(unit.owner.0);
                hash.write_u8(unit.team.0);
                hash.write_i32(unit.position.x);
                hash.write_i32(unit.position.y);
                hash.write_i32(unit.health.current);
                hash.write_i32(unit.health.max);
                hash.write_u32(unit.health_regeneration.per_second_per_10k);
                hash.write_u32(unit.health_regeneration.remainder_per_10k_hz);
                hash_attack_delivery(&mut hash, unit.attack.delivery);
                hash_action_timing(&mut hash, unit.action_timing);
                hash.write_u8(unit.attack_targets.bits());
                hash.write_u8(unit.damage_type.stable_tag());
                hash.write_u8(unit.armor.armor_type.stable_tag());
                hash.write_i32(i32::from(unit.armor.armor_points));
                hash_passive_unit_effects(&mut hash, unit.passive_effects);
                hash.write_u8(match unit.movement_class {
                    MovementClass::Ground => 0,
                    MovementClass::Air => 1,
                });
                hash.write_u8(u8::from(unit.mechanical));
                hash_unit_classifications(&mut hash, unit.classifications);
                hash_shrine_state(&mut hash, unit.shrine_state);
                hash_optional_u32(&mut hash, unit.build_time_ticks);
                hash_optional_u32(&mut hash, unit.repair_time_ticks);
                hash.write_i32(unit.attack.damage);
                hash.write_i32(unit.attack.range);
                hash.write_i32(unit.attack.acquisition_range);
                hash.write_u16(unit.attack.cooldown_ticks);
                hash_secondary_attack(&mut hash, unit.secondary_attack);
                hash.write_i32(unit.movement.speed_per_tick);
                hash.write_u16(unit.cooldown.remaining);
                hash.write_u64(unit.attack_sequence.0);
                hash_optional_sim_id(&mut hash, unit.target.current);
                hash.write_u8(u8::from(unit.target.direct_retaliation_lock));
                hash.write_u8(u8::from(unit.target.ally_defense_lock));
                hash_optional_sim_id(&mut hash, unit.retaliation.attacker);
                hash_optional_u64(&mut hash, unit.retaliation.attacked_tick);
                hash_status_state(&mut hash, unit.status);
                match unit.navigation.avoidance_goal {
                    NavigationGoal::None => hash.write_u8(0),
                    NavigationGoal::Objective(team) => {
                        hash.write_u8(1);
                        hash.write_u8(team.0);
                    }
                    NavigationGoal::Target(target) => {
                        hash.write_u8(2);
                        hash.write_u64(target.0);
                    }
                }
                hash.write_i32(i32::from(unit.navigation.bypass_side));
                hash.write_u8(unit.navigation.clear_ticks);
                hash.write_u64(unit.spawn_tick.0);
                match unit.corpse {
                    Some(corpse) => {
                        hash.write_u8(1);
                        hash.write_u64(u64::from(corpse.definition.0));
                        hash.write_u32(corpse.decay_start_ticks);
                        hash_optional_u32(&mut hash, corpse.lifetime_ticks);
                    }
                    None => hash.write_u8(0),
                }
                match unit.collision_radius {
                    Some(collision_radius) => {
                        hash.write_u8(1);
                        hash.write_i32(collision_radius.0);
                    }
                    None => hash.write_u8(0),
                }
                if let Some(spellcasting) = unit.spellcasting {
                    hash.write_u8(1);
                    hash.write_i32(spellcasting.mana.maximum);
                    hash.write_i32(spellcasting.mana.starting);
                    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
                    hash_automatic_ability(&mut hash, spellcasting.ability);
                    let mana = unit.mana.expect("spellcasting unit missing mana state");
                    hash.write_i32(mana.current);
                    hash.write_u16(mana.regen_remainder_per_10k);
                    let state = unit
                        .ability_state
                        .expect("spellcasting unit missing ability state");
                    hash.write_u64(state.ready_tick);
                    hash.write_u64(state.cast_sequence);
                    hash.write_u8(u8::from(state.autocast_enabled));
                    hash.write_u8(u8::from(state.manual_cast_requested));
                } else {
                    hash.write_u8(0);
                }
                hash_additional_abilities(&mut hash, unit.additional_abilities);
                match &unit.resurrection {
                    None => hash.write_u8(0),
                    Some(ResurrectionDefinitionSource::CurrentUnit) => hash.write_u8(1),
                    Some(ResurrectionDefinitionSource::Original(definition)) => {
                        hash.write_u8(2);
                        hash_resolved_unit_definition(&mut hash, Some(**definition));
                    }
                }
            }
            CanonicalEntity::Building(building) => {
                hash.write_u8(1);
                hash.write_u64(building.id.0);
                hash_content_identity(&mut hash, building.content);
                match building.owner {
                    Some(owner) => {
                        hash.write_u8(1);
                        hash.write_u8(owner.0);
                    }
                    None => hash.write_u8(0),
                }
                hash.write_u8(building.team.0);
                hash.write_i32(building.footprint.min_x);
                hash.write_i32(building.footprint.min_y);
                hash.write_u16(building.footprint.width);
                hash.write_u16(building.footprint.height);
                hash.write_i32(building.health.current);
                hash.write_i32(building.health.max);
                if let Some(regeneration) = building.health_regeneration {
                    hash.write_u8(1);
                    hash.write_u32(regeneration.per_second_per_10k);
                    hash.write_u32(regeneration.remainder_per_10k_hz);
                } else {
                    hash.write_u8(0);
                }
                if let Some(construction) = building.construction {
                    hash.write_u8(1);
                    hash.write_u64(construction.started_tick);
                    hash.write_u64(construction.complete_tick);
                    let mut definition_hash = Fnv64::new();
                    hash_building_definition(
                        &mut definition_hash,
                        construction.building,
                        construction.properties,
                    );
                    if let Some(source) = construction.upgrade_from {
                        definition_hash.write_u8(1);
                        hash_building_definition(
                            &mut definition_hash,
                            source.building,
                            source.properties,
                        );
                        definition_hash.write_i32(source.health.current);
                        definition_hash.write_i32(source.health.max);
                        hash_building_runtime_state(&mut definition_hash, source.runtime);
                    } else {
                        definition_hash.write_u8(0);
                    }
                    hash.write_u64(definition_hash.finish());
                } else {
                    hash.write_u8(0);
                }
                if let Some(economy) = building.economy {
                    hash.write_u8(1);
                    hash.write_u64(u64::from(economy.gold_cost));
                    hash.write_u64(u64::from(economy.lumber_cost));
                    hash.write_u64(u64::from(economy.lumber_refund));
                    hash.write_u16(economy.legendary_points_cost);
                    hash.write_u64(economy.income_per_10k);
                } else {
                    hash.write_u8(0);
                }
                hash_optional_u32(&mut hash, building.repair_time_ticks);
                hash_unit_classifications(&mut hash, building.classifications);
                hash.write_u8(building.damage_type.stable_tag());
                hash.write_u8(building.armor.armor_type.stable_tag());
                hash.write_i32(i32::from(building.armor.armor_points));
                if let Some(profile) = building.production {
                    hash.write_u8(1);
                    hash.write_u16(profile.initial_delay_ticks);
                    hash.write_u16(profile.interval_ticks);
                    hash.write_u16(profile.search_radius_cells);
                    hash.write_i32(profile.unit.health);
                    hash_attack_delivery(&mut hash, profile.unit.attack.delivery);
                    hash.write_i32(profile.unit.attack.damage);
                    hash.write_i32(profile.unit.attack.range);
                    hash.write_i32(profile.unit.attack.acquisition_range);
                    hash.write_u16(profile.unit.attack.cooldown_ticks);
                    hash.write_i32(profile.unit.movement.speed_per_tick);
                    hash.write_u64(
                        building
                            .production_state
                            .expect("production profile missing state")
                            .next_spawn_tick,
                    );
                    hash.write_u8(
                        building
                            .production_state
                            .expect("production profile missing state")
                            .queued,
                    );
                    hash_content_identity(&mut hash, building.production_content);
                    match building.production_corpse {
                        Some(corpse) => {
                            hash.write_u8(1);
                            hash.write_u64(u64::from(corpse.definition.0));
                            hash.write_u32(corpse.decay_start_ticks);
                            hash_optional_u32(&mut hash, corpse.lifetime_ticks);
                        }
                        None => hash.write_u8(0),
                    }
                    match building.production_collision_radius {
                        Some(collision_radius) => {
                            hash.write_u8(1);
                            hash.write_i32(collision_radius.0);
                        }
                        None => hash.write_u8(0),
                    }
                    hash.write_u8(
                        match building
                            .production_movement_class
                            .expect("production building missing movement class")
                        {
                            MovementClass::Ground => 0,
                            MovementClass::Air => 1,
                        },
                    );
                    let repair_metadata = building
                        .production_repair_metadata
                        .expect("production building missing repair metadata");
                    hash.write_u8(u8::from(repair_metadata.mechanical));
                    hash_optional_u32(&mut hash, repair_metadata.build_time_ticks);
                    hash_optional_u32(&mut hash, repair_metadata.repair_time_ticks);
                    hash.write_u8(
                        building
                            .production_attack_targets
                            .expect("production building missing attack target mask")
                            .bits(),
                    );
                    hash_secondary_attack(&mut hash, building.production_secondary_attack);
                    hash_action_timing(
                        &mut hash,
                        building
                            .production_action_timing
                            .expect("production action timing missing"),
                    );
                    hash.write_u32(
                        building
                            .production_health_regen_per_second_per_10k
                            .expect("production building missing unit health regeneration"),
                    );
                    let production_damage_type = building
                        .production_damage_type
                        .expect("production building missing unit damage type");
                    let production_armor = building
                        .production_armor
                        .expect("production building missing unit armor profile");
                    hash.write_u8(production_damage_type.stable_tag());
                    hash.write_u8(production_armor.armor_type.stable_tag());
                    hash.write_i32(i32::from(production_armor.armor_points));
                    hash_passive_unit_effects(
                        &mut hash,
                        building
                            .production_passive_effects
                            .expect("production building missing unit passive effects"),
                    );
                    hash_additional_ability_definitions(
                        &mut hash,
                        building.production_additional_abilities.as_deref().copied(),
                    );
                    hash_unit_classifications(&mut hash, building.production_classifications);
                    if let Some(spellcasting) = building.production_spellcasting {
                        hash.write_u8(1);
                        hash_spellcasting_profile(&mut hash, spellcasting);
                    } else {
                        hash.write_u8(0);
                    }
                } else {
                    hash.write_u8(0);
                }
                if let Some(attack) = building.attack {
                    hash.write_u8(1);
                    hash_attack_delivery(&mut hash, attack.delivery);
                    hash.write_u8(
                        building
                            .attack_targets
                            .expect("attack building missing target mask")
                            .bits(),
                    );
                    hash.write_i32(attack.damage);
                    hash.write_i32(attack.range);
                    hash.write_i32(attack.acquisition_range);
                    hash.write_u16(attack.cooldown_ticks);
                    hash.write_u16(
                        building
                            .cooldown
                            .expect("attack building missing cooldown")
                            .remaining,
                    );
                    let target = building
                        .target
                        .expect("attack building missing target state");
                    hash_optional_sim_id(&mut hash, target.current);
                    hash.write_u8(u8::from(target.direct_retaliation_lock));
                    hash.write_u8(u8::from(target.ally_defense_lock));
                    hash.write_u64(
                        building
                            .spawn_tick
                            .expect("attack building missing spawn tick")
                            .0,
                    );
                } else {
                    hash.write_u8(0);
                }
                if let Some(status) = building.status {
                    hash.write_u8(1);
                    hash_status_state(&mut hash, status);
                } else {
                    hash.write_u8(0);
                }
                if let Some(spellcasting) = building.spellcasting {
                    hash.write_u8(1);
                    hash.write_i32(spellcasting.mana.maximum);
                    hash.write_i32(spellcasting.mana.starting);
                    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
                    hash_automatic_ability(&mut hash, spellcasting.ability);
                    let mana = building
                        .mana
                        .expect("spellcasting building missing mana state");
                    hash.write_i32(mana.current);
                    hash.write_u16(mana.regen_remainder_per_10k);
                    let state = building
                        .ability_state
                        .expect("spellcasting building missing ability state");
                    hash.write_u64(state.ready_tick);
                    hash.write_u64(state.cast_sequence);
                    hash.write_u8(u8::from(state.autocast_enabled));
                    hash.write_u8(u8::from(state.manual_cast_requested));
                } else {
                    hash.write_u8(0);
                }
                hash_additional_abilities(&mut hash, building.additional_abilities);
            }
            CanonicalEntity::Projectile(projectile) => {
                hash.write_u8(2);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(u8::from(projectile.projectile.source_is_building));
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash_pending_attack_effects(&mut hash, projectile.projectile.on_hit);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.speed_per_tick);
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::ReflectedProjectile(projectile) => {
                hash.write_u8(3);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.original_source.0);
                hash.write_u64(projectile.projectile.reflector.0);
                hash.write_u8(projectile.projectile.reflector_team.0);
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::LineProjectile(projectile) => {
                hash.write_u8(14);
                hash.write_u64(projectile.id.0);
                let p = &projectile.projectile;
                hash.write_u64(p.source.0);
                match p.source_rawcode {
                    Some(rawcode) => {
                        hash.write_u8(1);
                        hash.write_u32(rawcode);
                    }
                    None => hash.write_u8(0),
                }
                hash.write_u8(p.source_team.0);
                hash.write_u64(p.target.0);
                hash.write_i32(p.damage);
                hash.write_u8(p.damage_type.stable_tag());
                hash_attack_delivery(&mut hash, p.delivery);
                hash.write_i32(p.launch_position.x);
                hash.write_i32(p.launch_position.y);
                hash.write_u64(p.launch_tick);
                hash.write_u64(p.primary_impact_tick);
                hash.write_u64(p.impact_tick);
                match p.spill_origin {
                    Some(origin) => {
                        hash.write_u8(1);
                        hash.write_i32(origin.x);
                        hash.write_i32(origin.y);
                    }
                    None => hash.write_u8(0),
                }
                hash.write_i32(p.destination.x);
                hash.write_i32(p.destination.y);
                hash.write_u64(p.hit_targets.len() as u64);
                for target in &p.hit_targets {
                    hash.write_u64(target.0);
                }
            }
            CanonicalEntity::BallisticProjectile(projectile) => {
                hash.write_u8(4);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(projectile.projectile.target_mask.bits());
                hash.write_i32(projectile.projectile.damage);
                hash_frost_profile_option(&mut hash, projectile.projectile.frost);
                match projectile.projectile.burning_oil {
                    Some(profile) => {
                        hash.write_u8(1);
                        hash_burning_oil_profile(&mut hash, profile);
                    }
                    None => hash.write_u8(0),
                }
                match projectile.projectile.splash_falloff {
                    Some(profile) => {
                        hash.write_u8(1);
                        hash_splash_falloff_profile(&mut hash, profile);
                    }
                    None => hash.write_u8(0),
                }
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_i32(projectile.projectile.destination.x);
                hash.write_i32(projectile.projectile.destination.y);
                hash.write_i32(projectile.projectile.impact_radius);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
            }
            CanonicalEntity::BounceProjectile(projectile) => {
                hash.write_u8(5);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(u8::from(projectile.projectile.source_is_building));
                hash.write_u8(projectile.projectile.target_mask.bits());
                hash.write_u64(projectile.projectile.target.0);
                hash.write_i32(projectile.projectile.damage);
                hash.write_u8(projectile.projectile.damage_type.stable_tag());
                hash.write_i32(projectile.projectile.launch_position.x);
                hash.write_i32(projectile.projectile.launch_position.y);
                hash.write_u64(projectile.projectile.launch_tick);
                hash.write_u64(projectile.projectile.impact_tick);
                hash.write_i32(projectile.projectile.speed_per_tick);
                hash.write_i32(projectile.projectile.bounce_range);
                hash.write_u8(projectile.projectile.remaining_bounces);
                hash.write_u8(projectile.projectile.bounce_index);
                hash.write_u16(projectile.projectile.damage_percent_per_bounce);
                hash.write_u8(u8::from(projectile.projectile.allow_repeat_targets));
                hash.write_u8(projectile.projectile.hit_count);
                for target in projectile.projectile.hit_targets {
                    hash.write_u64(target.0);
                }
            }
            CanonicalEntity::Corpse(corpse) => {
                hash.write_u8(6);
                hash.write_u64(corpse.id.0);
                hash.write_i32(corpse.position.x);
                hash.write_i32(corpse.position.y);
                hash.write_u64(corpse.corpse.source_unit.0);
                hash.write_u8(corpse.corpse.source_owner.0);
                hash.write_u8(corpse.corpse.source_team.0);
                hash.write_u64(u64::from(corpse.corpse.definition.0));
                hash.write_u64(corpse.corpse.created_tick);
                hash.write_u64(corpse.corpse.decay_start_tick);
                hash_optional_u64(&mut hash, corpse.corpse.expires_tick);
                hash_resolved_unit_definition(&mut hash, corpse.corpse.resurrection);
                hash_shrine_state(&mut hash, corpse.corpse.shrine_state);
            }
            CanonicalEntity::BurningOil(zone) => {
                hash.write_u8(7);
                hash.write_u64(zone.id.0);
                hash.write_u64(zone.zone.source.0);
                hash.write_u8(zone.zone.source_team.0);
                hash.write_i32(zone.zone.center.x);
                hash.write_i32(zone.zone.center.y);
                hash_burning_oil_profile(&mut hash, zone.zone.profile);
                hash.write_u64(zone.zone.created_tick);
                hash.write_u16(zone.zone.pulse_index);
            }
            CanonicalEntity::ChainLightning(chain) => {
                hash.write_u8(8);
                hash.write_u64(chain.id.0);
                hash.write_u64(chain.state.source.0);
                hash.write_u8(chain.state.source_team.0);
                hash.write_u64(u64::from(chain.state.profile.ability.0));
                hash.write_i32(chain.state.profile.initial_damage);
                hash.write_u8(chain.state.profile.maximum_targets);
                hash.write_i32(chain.state.profile.jump_radius);
                hash.write_u16(chain.state.profile.damage_reduction_per_10k);
                hash.write_u8(chain.state.profile.targets.bits());
                hash.write_u64(chain.state.started_tick);
                hash.write_u8(chain.state.next_jump_index);
                hash.write_u64(chain.state.current_target.0);
                hash.write_i32(chain.state.last_position.x);
                hash.write_i32(chain.state.last_position.y);
                hash.write_i32(chain.state.next_damage);
                hash.write_u8(chain.state.hit_count);
                for target in chain.state.hit_targets {
                    hash.write_u64(target.0);
                }
            }
            CanonicalEntity::DelayedShrineRevival { id, revival } => {
                hash.write_u8(11);
                hash.write_u64(id.0);
                hash.write_u16(revival.map_version.major);
                hash.write_u16(revival.map_version.minor);
                hash.write_u64(revival.source_unit.0);
                hash.write_u8(revival.owner.0);
                hash.write_u8(revival.team.0);
                hash.write_i32(revival.position.x);
                hash.write_i32(revival.position.y);
                hash.write_u64(revival.due_tick);
                hash.write_u64(revival.death_generation);
                hash_resolved_unit_definition(&mut hash, Some(revival.definition));
            }
            CanonicalEntity::NativeCarrier { id, state } => {
                use crate::native_carriers::NativeCarrierState;
                hash.write_u8(13);
                hash.write_u64(id.0);
                match state {
                    NativeCarrierState::Carrier {
                        building,
                        owner,
                        team,
                        position,
                        map_version,
                        ready_tick,
                        sequence,
                    } => {
                        hash.write_u8(0);
                        hash.write_u64(building.0);
                        match owner {
                            Some(owner) => {
                                hash.write_u8(1);
                                hash.write_u8(owner.0);
                            }
                            None => hash.write_u8(0),
                        }
                        hash.write_u8(team.0);
                        hash.write_i32(position.x);
                        hash.write_i32(position.y);
                        hash.write_u16(map_version.major);
                        hash.write_u16(map_version.minor);
                        hash.write_u64(ready_tick);
                        hash.write_u64(sequence);
                    }
                    NativeCarrierState::Bolt(bolt) => {
                        hash.write_u8(1);
                        hash.write_u64(bolt.source.0);
                        hash.write_u64(bolt.visual_source.0);
                        hash.write_u8(bolt.source_team.0);
                        hash.write_u64(bolt.target.0);
                        hash.write_u64(u64::from(bolt.ability.0));
                        hash.write_u16(bolt.map_version.major);
                        hash.write_u16(bolt.map_version.minor);
                        hash.write_i32(bolt.damage);
                        match bolt.attack_damage_type {
                            Some(kind) => {
                                hash.write_u8(1);
                                hash.write_u8(kind.stable_tag());
                            }
                            None => hash.write_u8(0),
                        }
                        hash.write_i32(bolt.launch_position.x);
                        hash.write_i32(bolt.launch_position.y);
                        hash.write_u64(bolt.launch_tick);
                        hash.write_i32(bolt.position.x);
                        hash.write_i32(bolt.position.y);
                        hash.write_u64(bolt.position_tick);
                        hash.write_u64(bolt.impact_tick);
                    }
                }
            }
            CanonicalEntity::Builder(builder) => {
                hash.write_u8(9);
                hash.write_u64(builder.id.0);
                hash.write_u8(builder.owner.0);
                hash.write_u8(builder.team.0);
                hash.write_i32(builder.position.x);
                hash.write_i32(builder.position.y);
                hash.write_i32(builder.profile.speed_per_tick);
                hash.write_u16(builder.profile.order_queue_capacity);
                hash.write_i32(builder.profile.build_range);
                hash.write_i32(builder.profile.repair_range);
                hash.write_i32(builder.profile.repair_autocast_range);
                hash.write_u16(builder.profile.repair_time_ratio_numerator);
                hash.write_u16(builder.profile.repair_time_ratio_denominator);
                hash.write_u16(builder.profile.full_repair_duration_ticks);
                hash.write_i32(builder.profile.blink_range);
                hash.write_i32(builder.profile.blink_boundary_inset);
                hash.write_u16(builder.configuration.appearance.map_version.major);
                hash.write_u16(builder.configuration.appearance.map_version.minor);
                hash.write_u64(u64::from(builder.configuration.appearance.rawcode));
                hash.write_u8(match builder.configuration.locomotion {
                    BuilderLocomotion::Foot => 0,
                    BuilderLocomotion::Hover => 1,
                });
                hash.write_u64(builder.configuration.build_catalog.len() as u64);
                for rawcode in builder.configuration.build_catalog {
                    hash.write_u64(u64::from(rawcode));
                }
                match builder.state.destination {
                    Some(destination) => {
                        hash.write_u8(1);
                        hash.write_i32(destination.x);
                        hash.write_i32(destination.y);
                    }
                    None => hash.write_u8(0),
                }
                hash_optional_sim_id(&mut hash, builder.state.follow_target);
                hash_optional_sim_id(&mut hash, builder.state.repair_target);
                hash.write_u64(u64::from(builder.state.repair_progress_remainder));
                hash.write_u8(u8::from(builder.state.repair_autocast_enabled));
                hash.write_u64(builder.orders.0.len() as u64);
                for order in builder.orders.0 {
                    match order {
                        BuilderOrder::Build(build) => {
                            hash.write_u8(0);
                            hash_building_definition(&mut hash, build.building, build.properties);
                        }
                        BuilderOrder::Move(point) => {
                            hash.write_u8(1);
                            hash.write_i32(point.x);
                            hash.write_i32(point.y);
                        }
                        BuilderOrder::Follow(id) => {
                            hash.write_u8(2);
                            hash.write_u64(id.0);
                        }
                        BuilderOrder::Repair(id) => {
                            hash.write_u8(3);
                            hash.write_u64(id.0);
                        }
                        BuilderOrder::Blink(point) => {
                            hash.write_u8(4);
                            hash.write_i32(point.x);
                            hash.write_i32(point.y);
                        }
                        BuilderOrder::Stop => hash.write_u8(5),
                    }
                }
            }
        }
    }

    let mut alerts = defense_alerts.to_vec();
    alerts.sort_unstable_by_key(|alert| {
        (
            alert.attacked_tick,
            alert.victim_team.0,
            alert.victim_id,
            alert.attacker_id,
            alert.victim_position.x,
            alert.victim_position.y,
        )
    });
    hash.write_u64(alerts.len() as u64);
    for alert in alerts {
        hash.write_u64(alert.attacked_tick);
        hash.write_u8(alert.victim_team.0);
        hash.write_u64(alert.victim_id.0);
        hash.write_i32(alert.victim_position.x);
        hash.write_i32(alert.victim_position.y);
        hash.write_u64(alert.attacker_id.0);
    }
    hash.finish()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum CanonicalEntity {
    Unit(CanonicalUnit),
    Building(CanonicalBuilding),
    Projectile(CanonicalProjectile),
    ReflectedProjectile(CanonicalReflectedProjectile),
    BallisticProjectile(CanonicalBallisticProjectile),
    BounceProjectile(CanonicalBounceProjectile),
    Corpse(CanonicalCorpse),
    BurningOil(CanonicalBurningOil),
    ChainLightning(CanonicalChainLightning),
    Builder(CanonicalBuilder),
    NativeAction {
        id: SimId,
        action: NativeAction,
    },
    DelayedShrineRevival {
        id: SimId,
        revival: DelayedShrineRevival,
    },
    LineProjectile(CanonicalLineProjectile),
    BuildingSpellTarget {
        id: SimId,
        state: BuildingSpellTargetState,
    },
    Snowveil {
        id: SimId,
        state: SnowveilState,
    },
    NativeCarrier {
        id: SimId,
        state: crate::native_carriers::NativeCarrierState,
    },
}

impl CanonicalEntity {
    pub(super) const fn id(&self) -> SimId {
        match self {
            Self::Unit(unit) => unit.id,
            Self::Building(building) => building.id,
            Self::Projectile(projectile) => projectile.id,
            Self::ReflectedProjectile(projectile) => projectile.id,
            Self::BallisticProjectile(projectile) => projectile.id,
            Self::BounceProjectile(projectile) => projectile.id,
            Self::Corpse(corpse) => corpse.id,
            Self::BurningOil(zone) => zone.id,
            Self::ChainLightning(chain) => chain.id,
            Self::Builder(builder) => builder.id,
            Self::NativeAction { id, .. } => *id,
            Self::DelayedShrineRevival { id, .. } => *id,
            Self::LineProjectile(projectile) => projectile.id,
            Self::BuildingSpellTarget { id, .. } => *id,
            Self::NativeCarrier { id, .. } => *id,
            Self::Snowveil { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CanonicalLineProjectile {
    pub(super) id: SimId,
    pub(super) projectile: LineProjectile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CanonicalBuilder {
    pub(super) id: SimId,
    pub(super) owner: PlayerId,
    pub(super) team: Team,
    pub(super) position: SimPoint,
    pub(super) profile: BuilderProfile,
    pub(super) configuration: BuilderConfiguration,
    pub(super) state: BuilderState,
    pub(super) orders: BuilderOrderQueue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CanonicalUnit {
    pub(super) id: SimId,
    pub(super) content: Option<ContentIdentity>,
    pub(super) owner: PlayerId,
    pub(super) team: Team,
    pub(super) position: SimPoint,
    pub(super) health: Health,
    pub(super) health_regeneration: HealthRegeneration,
    pub(super) attack: AttackProfile,
    pub(super) action_timing: ActionTimingProfile,
    pub(super) secondary_attack: Option<SecondaryAttackProfile>,
    pub(super) attack_targets: AttackTargetMask,
    pub(super) damage_type: DamageType,
    pub(super) armor: ArmorProfile,
    pub(super) passive_effects: PassiveUnitEffects,
    pub(super) movement_class: MovementClass,
    pub(super) mechanical: bool,
    pub(super) classifications: UnitClassifications,
    pub(super) shrine_state: ShrineRevivalState,
    pub(super) build_time_ticks: Option<u32>,
    pub(super) repair_time_ticks: Option<u32>,
    pub(super) movement: MovementProfile,
    pub(super) cooldown: AttackCooldown,
    pub(super) attack_sequence: AttackSequence,
    pub(super) target: TargetState,
    pub(super) retaliation: RetaliationState,
    pub(super) status: StatusState,
    pub(super) navigation: NavigationState,
    pub(super) spawn_tick: SpawnTick,
    pub(super) corpse: Option<CorpseProfile>,
    pub(super) collision_radius: Option<CollisionRadius>,
    pub(super) spellcasting: Option<SpellcastingProfile>,
    pub(super) mana: Option<ManaState>,
    pub(super) ability_state: Option<AutomaticAbilityState>,
    pub(super) additional_abilities: Option<AdditionalAutomaticAbilities>,
    pub(super) resurrection: Option<ResurrectionDefinitionSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum ResurrectionDefinitionSource {
    CurrentUnit,
    Original(Box<ResolvedUnitDefinition>),
}

impl CanonicalUnit {
    fn current_definition(&self) -> ResolvedUnitDefinition {
        ResolvedUnitDefinition {
            template: crate::components::UnitTemplate {
                health: self.health.max,
                attack: self.attack,
                movement: self.movement,
            },
            properties: UnitGameplayProperties {
                content: self.content,
                health_regen_per_second_per_10k: self.health_regeneration.per_second_per_10k,
                corpse: self.corpse,
                collision_radius: self.collision_radius,
                movement_class: self.movement_class,
                mechanical: self.mechanical,
                classifications: self.classifications,
                build_time_ticks: self.build_time_ticks,
                repair_time_ticks: self.repair_time_ticks,
                attack_targets: self.attack_targets,
                secondary_attack: self.secondary_attack,
                action_timing: self.action_timing,
                damage_type: self.damage_type,
                armor: self.armor,
                passive_effects: self.passive_effects,
            },
            spellcasting: self.spellcasting,
            additional_abilities: self
                .additional_abilities
                .map(AdditionalAutomaticAbilityDefinitions::from_runtime),
        }
    }

    pub(super) fn set_resurrection_definition(
        &mut self,
        definition: Option<ResolvedUnitDefinition>,
    ) {
        self.resurrection = definition.map(|definition| {
            let mut authored = definition;
            let mut current = self.current_definition();
            // Presentation labels must not choose a different authoritative encoding.
            for definition in [&mut authored, &mut current] {
                if let Some(content) = &mut definition.properties.content {
                    content.name = "";
                }
            }
            if authored == current {
                ResurrectionDefinitionSource::CurrentUnit
            } else {
                ResurrectionDefinitionSource::Original(Box::new(definition))
            }
        });
    }

    pub(super) fn resurrection_definition(&self) -> Option<ResolvedUnitDefinition> {
        self.resurrection
            .as_ref()
            .map(|definition| match definition {
                ResurrectionDefinitionSource::CurrentUnit => self.current_definition(),
                ResurrectionDefinitionSource::Original(definition) => **definition,
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CanonicalBuilding {
    pub(super) id: SimId,
    pub(super) content: Option<ContentIdentity>,
    pub(super) owner: Option<PlayerId>,
    pub(super) team: Team,
    pub(super) footprint: BuildingFootprint,
    pub(super) health: Health,
    pub(super) health_regeneration: Option<HealthRegeneration>,
    pub(super) construction: Option<Box<BuildingConstruction>>,
    pub(super) economy: Option<BuildingEconomyProfile>,
    pub(super) repair_time_ticks: Option<u32>,
    pub(super) production: Option<ProductionProfile>,
    pub(super) production_state: Option<ProductionState>,
    pub(super) production_content: Option<ContentIdentity>,
    pub(super) production_corpse: Option<CorpseProfile>,
    pub(super) production_collision_radius: Option<CollisionRadius>,
    pub(super) production_movement_class: Option<MovementClass>,
    pub(super) production_repair_metadata: Option<ProductionUnitRepairMetadata>,
    pub(super) production_attack_targets: Option<AttackTargetMask>,
    pub(super) production_secondary_attack: Option<SecondaryAttackProfile>,
    pub(super) production_action_timing: Option<ActionTimingProfile>,
    pub(super) production_health_regen_per_second_per_10k: Option<u32>,
    pub(super) production_damage_type: Option<DamageType>,
    pub(super) production_armor: Option<ArmorProfile>,
    pub(super) production_passive_effects: Option<PassiveUnitEffects>,
    pub(super) production_spellcasting: Option<SpellcastingProfile>,
    // Cold optional definitions must not inflate every canonical entity record.
    pub(super) production_additional_abilities: Option<Box<AdditionalAutomaticAbilityDefinitions>>,
    pub(super) production_classifications: UnitClassifications,
    pub(super) classifications: UnitClassifications,
    pub(super) attack: Option<AttackProfile>,
    pub(super) attack_targets: Option<AttackTargetMask>,
    pub(super) damage_type: DamageType,
    pub(super) armor: ArmorProfile,
    pub(super) cooldown: Option<AttackCooldown>,
    pub(super) target: Option<TargetState>,
    pub(super) spawn_tick: Option<SpawnTick>,
    pub(super) spellcasting: Option<SpellcastingProfile>,
    pub(super) mana: Option<ManaState>,
    pub(super) ability_state: Option<AutomaticAbilityState>,
    pub(super) additional_abilities: Option<AdditionalAutomaticAbilities>,
    pub(super) status: Option<StatusState>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalProjectile {
    pub(super) id: SimId,
    pub(super) projectile: GuaranteedHitProjectile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalReflectedProjectile {
    pub(super) id: SimId,
    pub(super) projectile: ReflectedProjectile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalBallisticProjectile {
    pub(super) id: SimId,
    pub(super) projectile: BallisticProjectile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalBounceProjectile {
    pub(super) id: SimId,
    pub(super) projectile: BounceProjectile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalCorpse {
    pub(super) id: SimId,
    pub(super) position: SimPoint,
    pub(super) corpse: Corpse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalBurningOil {
    pub(super) id: SimId,
    pub(super) zone: BurningOilZone,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct CanonicalChainLightning {
    pub(super) id: SimId,
    pub(super) state: ChainLightningState,
}

fn hash_content_identity(hash: &mut Fnv64, content: Option<ContentIdentity>) {
    match content {
        Some(content) => {
            hash.write_u8(1);
            hash.write_u16(content.map_version.major);
            hash.write_u16(content.map_version.minor);
            hash.write_u64(u64::from(content.rawcode));
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_sim_id(hash: &mut Fnv64, value: Option<SimId>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(value.0);
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_u32(hash: &mut Fnv64, value: Option<u32>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(u64::from(value));
        }
        None => hash.write_u8(0),
    }
}

fn hash_optional_u64(hash: &mut Fnv64, value: Option<u64>) {
    match value {
        Some(value) => {
            hash.write_u8(1);
            hash.write_u64(value);
        }
        None => hash.write_u8(0),
    }
}

fn hash_building_runtime_state(hash: &mut Fnv64, runtime: BuildingRuntimeState) {
    hash_unit_classifications(hash, runtime.classifications);
    match runtime.production {
        Some(production) => {
            hash.write_u8(1);
            hash.write_u64(production.next_spawn_tick);
            hash.write_u8(production.queued);
        }
        None => hash.write_u8(0),
    }
    match runtime.attack_cooldown {
        Some(cooldown) => {
            hash.write_u8(1);
            hash.write_u16(cooldown.remaining);
        }
        None => hash.write_u8(0),
    }
    match runtime.target {
        Some(target) => {
            hash.write_u8(1);
            hash_optional_sim_id(hash, target.current);
            hash.write_u8(u8::from(target.direct_retaliation_lock));
            hash.write_u8(u8::from(target.ally_defense_lock));
        }
        None => hash.write_u8(0),
    }
    match runtime.spawn_tick {
        Some(spawn_tick) => {
            hash.write_u8(1);
            hash.write_u64(spawn_tick.0);
        }
        None => hash.write_u8(0),
    }
    match runtime.health_regeneration {
        Some(regeneration) => {
            hash.write_u8(1);
            hash.write_u32(regeneration.per_second_per_10k);
            hash.write_u32(regeneration.remainder_per_10k_hz);
        }
        None => hash.write_u8(0),
    }
    match runtime.mana {
        Some(mana) => {
            hash.write_u8(1);
            hash.write_i32(mana.current);
            hash.write_u16(mana.regen_remainder_per_10k);
        }
        None => hash.write_u8(0),
    }
    match runtime.ability_state {
        Some(state) => {
            hash.write_u8(1);
            hash.write_u64(state.ready_tick);
            hash.write_u64(state.cast_sequence);
            hash.write_u8(u8::from(state.autocast_enabled));
            hash.write_u8(u8::from(state.manual_cast_requested));
        }
        None => hash.write_u8(0),
    }
    hash_additional_abilities(hash, runtime.additional_abilities);
    match runtime.status {
        Some(status) => {
            hash.write_u8(1);
            hash_status_state(hash, status);
        }
        None => hash.write_u8(0),
    }
}

fn hash_resolved_unit_definition(hash: &mut Fnv64, definition: Option<ResolvedUnitDefinition>) {
    let Some(definition) = definition else {
        hash.write_u8(0);
        return;
    };
    hash.write_u8(1);
    hash.write_i32(definition.template.health);
    hash_attack_delivery(hash, definition.template.attack.delivery);
    hash.write_i32(definition.template.attack.damage);
    hash.write_i32(definition.template.attack.range);
    hash.write_i32(definition.template.attack.acquisition_range);
    hash.write_u16(definition.template.attack.cooldown_ticks);
    hash.write_i32(definition.template.movement.speed_per_tick);
    let properties = definition.properties;
    hash_content_identity(hash, properties.content);
    hash_action_timing(hash, properties.action_timing);
    hash.write_u32(properties.health_regen_per_second_per_10k);
    hash.write_u8(u8::from(properties.corpse.is_some()));
    if let Some(profile) = properties.corpse {
        hash.write_u32(profile.definition.0);
        hash.write_u32(profile.decay_start_ticks);
        hash_optional_u32(hash, profile.lifetime_ticks);
    }
    hash.write_i32(properties.collision_radius.map_or(-1, |radius| radius.0));
    hash.write_u8(match properties.movement_class {
        MovementClass::Ground => 0,
        MovementClass::Air => 1,
    });
    hash.write_u8(u8::from(properties.mechanical));
    hash_unit_classifications(hash, properties.classifications);
    hash_optional_u32(hash, properties.build_time_ticks);
    hash_optional_u32(hash, properties.repair_time_ticks);
    hash.write_u8(properties.attack_targets.bits());
    hash_secondary_attack(hash, properties.secondary_attack);
    hash.write_u8(properties.damage_type.stable_tag());
    hash.write_u8(properties.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(properties.armor.armor_points));
    hash_passive_unit_effects(hash, properties.passive_effects);
    if let Some(spellcasting) = definition.spellcasting {
        hash.write_u8(1);
        hash_spellcasting_profile(hash, spellcasting);
    } else {
        hash.write_u8(0);
    }
    hash_additional_ability_definitions(hash, definition.additional_abilities);
}

fn hash_additional_ability_definitions(
    hash: &mut Fnv64,
    definitions: Option<AdditionalAutomaticAbilityDefinitions>,
) {
    let Some(definitions) = definitions else {
        hash.write_u8(0);
        return;
    };
    hash.write_u8(1);
    hash.write_u64(definitions.iter().count() as u64);
    for ability in definitions.iter() {
        hash_automatic_ability(hash, ability);
    }
}

fn hash_additional_abilities(hash: &mut Fnv64, abilities: Option<AdditionalAutomaticAbilities>) {
    let Some(abilities) = abilities else {
        hash.write_u8(0);
        return;
    };
    hash.write_u8(1);
    hash.write_u64(abilities.iter().count() as u64);
    for entry in abilities.iter() {
        hash_automatic_ability(hash, entry.profile);
        hash.write_u64(entry.state.ready_tick);
        hash.write_u64(entry.state.cast_sequence);
        hash.write_u8(u8::from(entry.state.autocast_enabled));
        hash.write_u8(u8::from(entry.state.manual_cast_requested));
        hash.write_u64(entry.secondary_resurrection.due_tick);
        hash.write_u64(entry.secondary_resurrection.ready_tick);
    }
}

fn hash_action_timing(hash: &mut Fnv64, timing: ActionTimingProfile) {
    hash.write_u16(timing.primary_attack_ticks);
    hash.write_u16(timing.primary_attack_point_ticks);
    hash.write_u16(timing.secondary_attack_ticks);
    hash.write_u16(timing.secondary_attack_point_ticks);
    hash.write_u16(timing.cast_ticks);
    hash.write_u16(timing.cast_point_ticks);
}

fn hash_status_state(hash: &mut Fnv64, status: StatusState) {
    hash.write_u8(u8::from(status.pending_attack.is_some()));
    if let Some(pending) = status.pending_attack {
        hash.write_u64(pending.target.0);
        hash.write_u64(pending.release_tick);
    }
    hash.write_u8(u8::from(status.pending_cast.is_some()));
    if let Some(pending) = status.pending_cast {
        hash_automatic_ability(hash, pending.ability);
        hash.write_u64(pending.cast_sequence);
        hash.write_u64(pending.release_tick);
        match pending.target {
            PendingCastTarget::Unit(id) => {
                hash.write_u8(0);
                hash.write_u64(id.0);
            }
            PendingCastTarget::Building { id, position } => {
                hash.write_u8(1);
                hash.write_u64(id.0);
                hash.write_i32(position.x);
                hash.write_i32(position.y);
            }
            PendingCastTarget::AllEnemyUnits => hash.write_u8(2),
            PendingCastTarget::AllFriendlyUnits => hash.write_u8(3),
            PendingCastTarget::Corpse { id, position } => {
                hash.write_u8(4);
                hash.write_u64(id.0);
                hash.write_i32(position.x);
                hash.write_i32(position.y);
            }
            PendingCastTarget::Point(position) => {
                hash.write_u8(5);
                hash.write_i32(position.x);
                hash.write_i32(position.y);
            }
        }
    }
    hash.write_u8(u8::from(status.action_animation.is_some()));
    if let Some(action) = status.action_animation {
        hash.write_u8(match action.kind {
            ActionAnimationKind::Attack => 0,
            ActionAnimationKind::Cast => 1,
        });
        hash.write_u64(action.started_tick);
        hash.write_u64(action.until_tick);
    }
    hash.write_u64(status.stunned_until_tick);
    hash.write_u64(status.native_stun_until_tick);
    hash.write_u8(u8::from(status.native_stun_ability.is_some()));
    if let Some(a) = status.native_stun_ability {
        hash.write_u32(a.0);
    }
    hash.write_u64(status.rooted_until_tick);
    hash.write_u64(status.frozen_until_tick);
    hash.write_u8(status.frozen_ability.is_some() as u8);
    if let Some(ability) = status.frozen_ability {
        hash.write_u32(ability.0);
    }
    hash.write_u64(status.order_recovery_until_tick);
    hash.write_u64(status.ability_retreat_start_tick);
    hash.write_u64(status.ability_retreat_end_tick);
    hash.write_u64(status.secondary_resurrection_due_tick);
    hash.write_u64(status.secondary_resurrection_ready_tick);
    match status.secondary_resurrection_ability {
        Some(id) => {
            hash.write_u8(1);
            hash.write_u32(id.0);
        }
        None => hash.write_u8(0),
    }
    hash.write_u8(u8::from(status.permanent_holy_health_bonus));
    hash.write_u8(status.movement_modifier_count);
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    for modifier in &status.movement_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.percent_delta));
        hash.write_u64(modifier.expires_tick);
    }
    hash.write_u8(status.attack_speed_modifier_count);
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    for modifier in &status.attack_speed_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.percent_delta));
        hash.write_u64(modifier.expires_tick);
    }
    hash.write_u8(status.armor_modifier_count);
    let count = usize::from(status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    for modifier in &status.armor_modifiers[..count] {
        hash.write_u64(u64::from(modifier.id.0));
        hash.write_i32(i32::from(modifier.armor_bonus_per_100));
        hash.write_u64(u64::from(modifier.regeneration_per_second_per_10k));
        hash.write_u64(u64::from(modifier.mana_regeneration_per_second_per_10k));
        hash.write_i32(i32::from(modifier.damage_bonus_per_10k));
        hash.write_u64(modifier.expires_tick);
        hash.write_u16(modifier.reactive_slow_duration_ticks);
        hash.write_i32(i32::from(modifier.reactive_movement_percent_delta));
        hash.write_i32(i32::from(modifier.reactive_attack_speed_percent_delta));
        hash.write_u8(modifier.revealed_to.map_or(u8::MAX, |team| team.0));
    }
    hash.write_u8(status.damage_over_time_count);
    let count = usize::from(status.damage_over_time_count);
    debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
    for effect in &status.damage_over_time[..count] {
        hash.write_u64(u64::from(effect.id.0));
        hash.write_i32(effect.damage_per_pulse);
        hash.write_u16(effect.pulse_interval_ticks);
        hash.write_u64(effect.next_pulse_tick);
        hash.write_u64(effect.expires_tick);
        hash.write_u8(u8::from(effect.final_pulse_at_expiry));
    }
}

fn hash_building_definition(
    hash: &mut Fnv64,
    building: BuildingSpawn,
    properties: BuildingGameplayProperties,
) {
    hash.write_u8(building.team.0);
    hash.write_i32(building.footprint.min_x);
    hash.write_i32(building.footprint.min_y);
    hash.write_u16(building.footprint.width);
    hash.write_u16(building.footprint.height);
    hash.write_i32(building.health);
    hash_content_identity(hash, properties.content);
    hash_unit_classifications(hash, properties.classifications);
    hash_optional_u32(hash, properties.construction_time_ticks);
    hash_optional_u32(hash, properties.repair_time_ticks);
    hash.write_u32(properties.health_regen_per_second_per_10k);
    hash.write_u8(properties.attack_targets.bits());
    hash.write_u8(properties.damage_type.stable_tag());
    hash.write_u8(properties.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(properties.armor.armor_points));
    if let Some(economy) = properties.economy {
        hash.write_u8(1);
        hash.write_u64(u64::from(economy.gold_cost));
        hash.write_u64(u64::from(economy.lumber_cost));
        hash.write_u64(u64::from(economy.lumber_refund));
        hash.write_u16(economy.legendary_points_cost);
        hash.write_u64(economy.income_per_10k);
    } else {
        hash.write_u8(0);
    }
    if let Some(production) = building.production {
        hash.write_u8(1);
        hash.write_u16(production.initial_delay_ticks);
        hash.write_u16(production.interval_ticks);
        hash.write_u16(production.search_radius_cells);
        hash.write_i32(production.unit.health);
        hash_attack_delivery(hash, production.unit.attack.delivery);
        hash.write_i32(production.unit.attack.damage);
        hash.write_i32(production.unit.attack.range);
        hash.write_i32(production.unit.attack.acquisition_range);
        hash.write_u16(production.unit.attack.cooldown_ticks);
        hash.write_i32(production.unit.movement.speed_per_tick);
        let unit = properties.production_unit;
        hash_content_identity(hash, unit.content);
        hash.write_u8(match unit.movement_class {
            MovementClass::Ground => 0,
            MovementClass::Air => 1,
        });
        hash.write_u8(u8::from(unit.mechanical));
        hash_unit_classifications(hash, unit.classifications);
        hash_optional_u32(hash, unit.build_time_ticks);
        hash_optional_u32(hash, unit.repair_time_ticks);
        hash.write_u8(unit.attack_targets.bits());
        hash_secondary_attack(hash, unit.secondary_attack);
        hash.write_u32(unit.health_regen_per_second_per_10k);
        hash_action_timing(hash, unit.action_timing);
        hash.write_u8(unit.damage_type.stable_tag());
        hash.write_u8(unit.armor.armor_type.stable_tag());
        hash.write_i32(i32::from(unit.armor.armor_points));
        hash_passive_unit_effects(hash, unit.passive_effects);
        if let Some(corpse) = unit.corpse {
            hash.write_u8(1);
            hash.write_u64(u64::from(corpse.definition.0));
            hash.write_u32(corpse.decay_start_ticks);
            hash_optional_u32(hash, corpse.lifetime_ticks);
        } else {
            hash.write_u8(0);
        }
        if let Some(radius) = unit.collision_radius {
            hash.write_u8(1);
            hash.write_i32(radius.0);
        } else {
            hash.write_u8(0);
        }
        hash_additional_ability_definitions(hash, properties.production_additional_abilities);
        if let Some(spellcasting) = properties.production_spellcasting {
            hash.write_u8(1);
            hash_spellcasting_profile(hash, spellcasting);
        } else {
            hash.write_u8(0);
        }
    } else {
        hash.write_u8(0);
    }
    if let Some(attack) = building.attack {
        hash.write_u8(1);
        hash_attack_delivery(hash, attack.delivery);
        hash.write_i32(attack.damage);
        hash.write_i32(attack.range);
        hash.write_i32(attack.acquisition_range);
        hash.write_u16(attack.cooldown_ticks);
    } else {
        hash.write_u8(0);
    }
    if let Some(spellcasting) = building.spellcasting {
        hash.write_u8(1);
        hash_spellcasting_profile(hash, spellcasting);
    } else {
        hash.write_u8(0);
    }
}

fn hash_spellcasting_profile(hash: &mut Fnv64, spellcasting: SpellcastingProfile) {
    hash.write_i32(spellcasting.mana.maximum);
    hash.write_i32(spellcasting.mana.starting);
    hash.write_u64(u64::from(spellcasting.mana.regen_per_tick_per_10k));
    hash_automatic_ability(hash, spellcasting.ability);
}

fn hash_passive_unit_effects(hash: &mut Fnv64, effects: PassiveUnitEffects) {
    let effects: Vec<_> = effects.iter().collect();
    hash.write_u8(u8::try_from(effects.len()).expect("passive effect count fits u8"));
    for effect in effects {
        match effect {
            PassiveUnitEffect::SplashFalloff(profile) => {
                hash.write_u8(6);
                hash_splash_falloff_profile(hash, profile);
            }
            PassiveUnitEffect::CriticalStrike(profile) => {
                hash.write_u8(5);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
                hash.write_u16(profile.damage_multiplier_per_10k);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Bash(profile) => {
                hash.write_u8(0);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
                hash.write_i32(profile.bonus_damage);
                hash.write_u16(profile.stun_duration_ticks);
                hash.write_u16(profile.hero_stun_duration_ticks);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Evasion(profile) => {
                hash.write_u8(1);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
            }
            PassiveUnitEffect::Defend(profile) => {
                hash.write_u8(2);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.ranged_damage_taken_per_10k);
                hash.write_u16(profile.spell_damage_taken_per_10k);
                hash.write_u16(profile.deflect_chance_per_10k);
                hash.write_u16(profile.deflected_pierce_damage_taken_per_10k);
                hash.write_u16(profile.activation_delay_ticks);
            }
            PassiveUnitEffect::TriggeredSpellProc(profile) => {
                hash.write_u8(3);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.chance_per_10k);
                hash.write_u8(profile.targets.bits());
                hash_triggered_attack_effect(hash, profile.effect);
            }
            PassiveUnitEffect::BurningOil(profile) => {
                hash.write_u8(4);
                hash_burning_oil_profile(hash, profile);
            }
            PassiveUnitEffect::Cleave(profile) => {
                hash.write_u8(7);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_i32(profile.radius);
                hash.write_u16(profile.damage_per_10k);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Aura(profile) => {
                hash.write_u8(8);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_i32(profile.radius);
                hash.write_i32(i32::from(profile.armor_bonus_per_100));
                hash.write_u64(u64::from(profile.mana_regeneration_per_second_per_10k));
                hash.write_u8(u8::from(profile.suspend_during_spell_cooldown));
            }
            PassiveUnitEffect::Pulverize(profile) => {
                hash.write_u8(12);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
                hash.write_i32(profile.damage);
                hash.write_i32(profile.full_radius);
                hash.write_i32(profile.half_radius);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::FrostAttack(profile) => {
                hash.write_u8(11);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.duration_ticks);
                hash.write_u16(profile.hero_duration_ticks);
                hash.write_i32(i32::from(profile.movement_percent_delta));
                hash.write_i32(i32::from(profile.attack_speed_percent_delta));
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Feedback(profile) => {
                hash.write_u8(10);
                hash_feedback_profile(hash, profile);
            }
            PassiveUnitEffect::SpellResistance(profile) => {
                hash.write_u8(9);
                hash.write_u64(u64::from(profile.ability.0));
                hash.write_u16(profile.damage_taken_per_10k);
            }
        }
    }
}

fn hash_triggered_attack_effect(hash: &mut Fnv64, effect: TriggeredAttackEffect) {
    match effect {
        TriggeredAttackEffect::ChainLightning(profile) => {
            hash.write_u8(0);
            hash.write_u64(u64::from(profile.ability.0));
            hash.write_i32(profile.initial_damage);
            hash.write_u8(profile.maximum_targets);
            hash.write_i32(profile.jump_radius);
            hash.write_u16(profile.damage_reduction_per_10k);
            hash.write_u8(profile.targets.bits());
        }
        TriggeredAttackEffect::EntanglingRoots(profile) => {
            hash.write_u8(1);
            hash.write_u64(u64::from(profile.ability.0));
            hash.write_i32(profile.damage_per_second);
            hash.write_u16(profile.duration_ticks);
            hash.write_u16(profile.hero_duration_ticks);
            hash.write_u8(u8::from(profile.nonhero_only));
            hash.write_u8(profile.targets.bits());
        }
    }
}

fn hash_burning_oil_profile(hash: &mut Fnv64, profile: crate::components::BurningOilEffectProfile) {
    hash.write_u64(u64::from(profile.ability.0));
    hash.write_i32(profile.radius);
    hash.write_i32(profile.full_damage);
    hash.write_u16(profile.full_interval_millis);
    hash.write_i32(profile.half_damage);
    hash.write_u16(profile.half_interval_millis);
    hash.write_u16(profile.full_duration_millis);
    hash.write_u16(profile.total_duration_millis);
    hash.write_u8(u8::from(profile.target_ground_units));
    hash.write_u8(u8::from(profile.target_buildings));
}

fn hash_splash_falloff_profile(hash: &mut Fnv64, profile: crate::components::SplashFalloffProfile) {
    hash.write_i32(profile.full_radius);
    hash.write_i32(profile.medium_radius);
    hash.write_i32(profile.outer_radius);
    hash.write_u16(profile.medium_damage_per_10k);
    hash.write_u16(profile.outer_damage_per_10k);
    hash.write_u8(profile.targets.bits());
}

fn hash_unit_classifications(hash: &mut Fnv64, flags: UnitClassifications) {
    hash.write_u8(u8::from(flags.hero));
    hash.write_u8(u8::from(flags.summoned));
    hash.write_u8(u8::from(flags.spell_immune));
    hash.write_u8(u8::from(flags.invisible));
    hash.write_u8(u8::from(flags.combat_sapper));
    hash.write_u8(u8::from(flags.invulnerable));
    hash.write_u8(u8::from(flags.legendary));
    hash.write_u8(u8::from(flags.summoned_marker));
    hash.write_u8(u8::from(flags.illusion));
}

fn hash_shrine_state(hash: &mut Fnv64, state: ShrineRevivalState) {
    hash.write_u8(u8::from(state.revived));
    hash.write_u8(u8::from(state.suppress_next_death));
    hash_optional_sim_id(hash, state.death_identity);
}

fn hash_feedback_profile(hash: &mut Fnv64, profile: crate::components::FeedbackEffectProfile) {
    hash.write_u32(profile.ability.0);
    hash.write_i32(profile.maximum_mana_drained);
    hash.write_u16(profile.damage_per_mana_per_10k);
    hash.write_i32(profile.summoned_damage);
    hash.write_u8(profile.targets.bits());
}

fn hash_frost_profile_option(
    hash: &mut Fnv64,
    frost: Option<crate::components::FrostAttackEffectProfile>,
) {
    match frost {
        None => hash.write_u8(0),
        Some(p) => {
            hash.write_u8(1);
            hash.write_u32(p.ability.0);
            hash.write_u16(p.duration_ticks);
            hash.write_u16(p.hero_duration_ticks);
            hash.write_i32(i32::from(p.movement_percent_delta));
            hash.write_i32(i32::from(p.attack_speed_percent_delta));
            hash.write_u8(p.targets.bits());
        }
    }
}

fn hash_pending_attack_effects(hash: &mut Fnv64, effects: PendingAttackEffects) {
    hash_frost_profile_option(hash, effects.frost);
    hash.write_u16(effects.stun_duration_ticks);
    if let Some(profile) = effects.feedback {
        hash.write_u8(1);
        hash_feedback_profile(hash, profile);
    } else {
        hash.write_u8(0);
    }
    match effects.triggered_spell {
        Some(effect) => {
            hash.write_u8(1);
            hash_triggered_attack_effect(hash, effect);
        }
        None => hash.write_u8(0),
    }
    match effects.burning_oil {
        Some(profile) => {
            hash.write_u8(1);
            hash_burning_oil_profile(hash, profile);
        }
        None => hash.write_u8(0),
    }
    match effects.splash_falloff {
        Some(profile) => {
            hash.write_u8(1);
            hash_splash_falloff_profile(hash, profile);
        }
        None => hash.write_u8(0),
    }
}

fn hash_hex_form(hash: &mut Fnv64, form: crate::building_mechanics::HexFormProfile) {
    hash.write_u32(form.rawcode);
    hash.write_i32(form.speed_per_tick);
    hash.write_i32(form.collision_radius);
    hash.write_i32(i32::from(form.armor.armor_points));
    hash.write_u8(form.armor.armor_type.stable_tag());
}

fn hash_hex_profile(hash: &mut Fnv64, profile: crate::building_mechanics::HexEffectProfile) {
    hash.write_u16(profile.map_version.major);
    hash.write_u16(profile.map_version.minor);
    hash.write_u16(profile.duration_ticks);
    hash.write_u16(profile.hero_duration_ticks);
    hash.write_u16(profile.initial_reengage_ticks);
    hash.write_u16(profile.defender_restore_ticks);
    hash.write_u16(profile.defender_resume_ticks);
    hash.write_u32(profile.defender_rawcode);
    hash_hex_form(hash, profile.ground);
    hash_hex_form(hash, profile.air);
}

fn hash_automatic_ability(hash: &mut Fnv64, ability: AutomaticAbilityProfile) {
    hash.write_u64(u64::from(ability.id.0));
    hash.write_i32(ability.mana_cost);
    hash.write_u16(ability.cooldown_ticks);
    hash.write_i32(ability.range);
    hash.write_u8(ability.target_policy.stable_tag());
    hash.write_u8(ability.effect.stable_tag());
    match ability.effect {
        AbilityEffect::Hailstone(profile) => hash_hailstone_profile(hash, profile),
        AbilityEffect::BuildingBolt(p) => {
            hash.write_u16(p.map_version.major);
            hash.write_u16(p.map_version.minor);
            hash.write_u8(p.trigger_targets.bits());
            hash.write_u8(u8::from(p.trigger_invulnerable));
            hash.write_u8(u8::from(p.trigger_spell_immune));
            hash_native_bolt_profile(hash, p.bolt);
            hash.write_i32(p.child_range);
            hash.write_i32(p.vision_radius);
            hash.write_u16(p.vision_ticks);
        }
        AbilityEffect::Snowfall { map_version } => {
            hash.write_u16(map_version.major);
            hash.write_u16(map_version.minor);
        }
        AbilityEffect::Hex { profile } => hash_hex_profile(hash, profile),
        AbilityEffect::FaerieFire {
            modifier,
            armor_reduction_per_100,
            duration_ticks,
            hero_duration_ticks,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(i32::from(armor_reduction_per_100));
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
        }
        AbilityEffect::PhoenixFire(profile) => hash_native_bolt_profile(hash, profile),
        AbilityEffect::SolarStrike {
            profile,
            radius,
            maximum_targets,
        } => {
            hash_native_bolt_profile(hash, profile);
            hash.write_i32(radius);
            hash.write_u8(maximum_targets);
        }
        AbilityEffect::HealingWave(profile) => hash_healing_wave_profile(hash, profile),
        AbilityEffect::Damage { amount } => hash.write_i32(amount),
        AbilityEffect::Stun { duration_ticks } => hash.write_u16(duration_ticks),
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier,
            percent_delta,
            duration_ticks,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(i32::from(percent_delta));
            hash.write_u16(duration_ticks);
        }
        AbilityEffect::FrostNova {
            ability,
            radius,
            primary_damage,
            area_damage,
            duration_ticks,
            hero_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(radius);
            hash.write_i32(primary_damage);
            hash.write_i32(area_damage);
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
            hash.write_i32(i32::from(movement_percent_delta));
            hash.write_i32(i32::from(attack_speed_percent_delta));
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaDebuff {
            ability,
            radius,
            armor_delta_per_100,
            damage_delta_per_10k,
            duration_ticks,
            hero_duration_ticks,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(radius);
            hash.write_i32(i32::from(armor_delta_per_100));
            hash.write_i32(i32::from(damage_delta_per_10k));
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaStun {
            ability,
            damage,
            radius,
            stun_ticks,
            hero_stun_ticks,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(damage);
            hash.write_i32(radius);
            hash.write_u16(stun_ticks);
            hash.write_u16(hero_stun_ticks);
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaDamage {
            amount,
            radius,
            origin,
        } => {
            hash.write_i32(amount);
            hash.write_i32(radius);
            hash.write_u8(match origin {
                AreaDamageOrigin::Caster => 0,
                AreaDamageOrigin::Target => 1,
            });
        }
        AbilityEffect::FrostArmor {
            modifier,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u16(armor_duration_ticks);
            hash.write_u16(slow_duration_ticks);
            hash.write_i32(i32::from(movement_percent_delta));
            hash.write_i32(i32::from(attack_speed_percent_delta));
        }
        AbilityEffect::HolyAid {
            modifier,
            healing,
            armor_bonus_per_100,
            regeneration_per_second_per_10k,
            duration_ticks,
            permanent_max_health_bonus,
            resurrection_count,
            resurrection_radius,
            resurrection_mana_cost,
            resurrection_cooldown_ticks,
            resurrection_delay_ticks,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(healing);
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u64(u64::from(regeneration_per_second_per_10k));
            hash.write_u16(duration_ticks);
            hash.write_i32(permanent_max_health_bonus);
            hash.write_u8(resurrection_count);
            hash.write_i32(resurrection_radius);
            hash.write_i32(resurrection_mana_cost);
            hash.write_u16(resurrection_cooldown_ticks);
            hash.write_u16(resurrection_delay_ticks);
        }
        AbilityEffect::Prayer {
            modifier,
            healing,
            mana_restored,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            duration_ticks,
            radius,
            resurrection_count,
            resurrection_radius,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(healing);
            hash.write_i32(mana_restored);
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u16(damage_bonus_per_10k);
            hash.write_u16(duration_ticks);
            hash.write_i32(radius);
            hash.write_u8(resurrection_count);
            hash.write_i32(resurrection_radius);
        }
        AbilityEffect::HolyFervour {
            modifier,
            radius,
            duration_ticks,
        } => {
            hash.write_u64(u64::from(modifier.0));
            hash.write_i32(radius);
            hash.write_u16(duration_ticks);
        }
        AbilityEffect::Purification {
            damage,
            radius,
            consume_radius,
            reveal_radius,
            reveal_duration_ticks,
        } => {
            hash.write_i32(damage);
            hash.write_i32(radius);
            hash.write_i32(consume_radius);
            hash.write_i32(reveal_radius);
            hash.write_u16(reveal_duration_ticks);
        }
        AbilityEffect::ArtilleryBombardment {
            min_damage,
            max_damage,
            speed_per_tick,
            splash,
            burning_oil,
        } => {
            hash.write_i32(min_damage);
            hash.write_i32(max_damage);
            hash.write_i32(speed_per_tick);
            hash_splash_falloff_profile(hash, splash);
            hash.write_u64(u64::from(burning_oil.ability.0));
            hash.write_i32(burning_oil.radius);
            hash.write_i32(burning_oil.full_damage);
            hash.write_u16(burning_oil.full_interval_millis);
            hash.write_i32(burning_oil.half_damage);
            hash.write_u16(burning_oil.half_interval_millis);
            hash.write_u16(burning_oil.full_duration_millis);
            hash.write_u16(burning_oil.total_duration_millis);
            hash.write_u8(u8::from(burning_oil.target_ground_units));
            hash.write_u8(u8::from(burning_oil.target_buildings));
        }
    }
}

fn hash_native_bolt_profile(hash: &mut Fnv64, profile: crate::components::NativeBoltProfile) {
    hash.write_u64(u64::from(profile.ability.0));
    hash.write_i32(profile.damage);
    hash.write_u16(profile.stun_ticks);
    hash.write_u16(profile.hero_stun_ticks);
    hash.write_i32(profile.damage_per_second);
    hash.write_u16(profile.duration_ticks);
    hash.write_i32(profile.speed_per_tick);
    hash.write_u8(u8::from(profile.cleanse));
    hash.write_u8(profile.targets.bits());
}

fn hash_healing_wave_profile(hash: &mut Fnv64, profile: crate::components::HealingWaveProfile) {
    hash.write_u64(u64::from(profile.ability.0));
    hash.write_i32(profile.healing);
    hash.write_i32(profile.trigger_healing);
    hash.write_u8(profile.maximum_targets);
    hash.write_i32(profile.jump_radius);
    hash.write_u16(profile.retention_per_10k);
    hash.write_u16(profile.recovery_ticks);
}

fn hash_secondary_attack(hash: &mut Fnv64, secondary: Option<SecondaryAttackProfile>) {
    if let Some(profile) = secondary {
        hash.write_u8(1);
        hash_attack_delivery(hash, profile.attack.delivery);
        hash.write_i32(profile.attack.damage);
        hash.write_i32(profile.attack.range);
        hash.write_i32(profile.attack.acquisition_range);
        hash.write_u16(profile.attack.cooldown_ticks);
        hash.write_u8(profile.primary_targets.bits());
        hash.write_u8(profile.targets.bits());
        hash.write_u8(profile.damage_type.stable_tag());
    } else {
        hash.write_u8(0);
    }
}

fn hash_attack_delivery(hash: &mut Fnv64, delivery: AttackDelivery) {
    hash.write_u8(delivery.stable_tag());
    match delivery {
        AttackDelivery::Melee | AttackDelivery::RangedInstant => {}
        AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
            hash.write_i32(speed_per_tick);
        }
        AttackDelivery::RangedBallistic {
            speed_per_tick,
            impact_radius,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(impact_radius);
        }
        AttackDelivery::Line {
            speed_per_tick,
            minimum_range,
            spill_distance,
            spill_radius,
            damage_retention_per_10k,
            spill_targets,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(minimum_range);
            hash.write_i32(spill_distance);
            hash.write_i32(spill_radius);
            hash.write_u16(damage_retention_per_10k);
            hash.write_u8(spill_targets.bits());
        }
        AttackDelivery::Bounce {
            speed_per_tick,
            bounce_range,
            max_bounces,
            damage_percent_per_bounce,
            allow_repeat_targets,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(bounce_range);
            hash.write_u8(max_bounces);
            hash.write_u16(damage_percent_per_bounce);
            hash.write_u8(u8::from(allow_repeat_targets));
        }
    }
}

struct Fnv64(u64);

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write(&value.to_le_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

fn hash_hailstone_profile(hash: &mut Fnv64, p: crate::building_mechanics::HailstoneProfile) {
    hash.write_u16(p.map_version.major);
    hash.write_u16(p.map_version.minor);
    hash.write_u32(p.dummy_rawcode);
    hash.write_u32(p.ability.0);
    hash.write_i32(p.damage);
    hash.write_i32(p.speed_per_tick);
    hash.write_i32(p.range);
    hash.write_i32(p.full_radius);
    hash.write_u8(p.splash_targets.bits());
    hash.write_u8(p.trigger_targets.bits());
    hash.write_u8(u8::from(p.trigger_invulnerable));
    hash.write_u8(u8::from(p.trigger_spell_immune));
    hash.write_u8(p.freeze_targets.bits());
    hash.write_u16(p.freeze_ticks);
    hash.write_u16(p.hero_freeze_ticks);
    for code in p.excluded_rawcodes {
        hash.write_u32(code);
    }
    hash.write_u32(p.excluded_buff);
    hash.write_i32(p.vision_radius);
    hash.write_u16(p.vision_ticks);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inert_unit() -> UnitSpawn {
        UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    #[test]
    fn allocator_history_changes_authoritative_checksum() {
        let first = Simulation::new(SimulationConfig::default(), 1);
        let mut second = Simulation::new(SimulationConfig::default(), 1);
        second.next_id = second.next_id.checked_add(1).unwrap();

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn match_seed_changes_authoritative_checksum() {
        let first = Simulation::new(
            SimulationConfig {
                match_seed: 1,
                ..SimulationConfig::default()
            },
            1,
        );
        let second = Simulation::new(
            SimulationConfig {
                match_seed: 2,
                ..SimulationConfig::default()
            },
            1,
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn gameplay_bundle_identity_changes_authoritative_checksum() {
        let first = Simulation::new_with_gameplay_bundle(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
            GameplayBundleIdentity {
                schema_version: 1,
                gameplay_hash: 0x1111,
            },
        );
        let second = Simulation::new_with_gameplay_bundle(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
            GameplayBundleIdentity {
                schema_version: 1,
                gameplay_hash: 0x2222,
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn combat_rules_change_authoritative_checksum() {
        let first = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            1,
            CombatRules::default(),
        );
        let second = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            1,
            CombatRules {
                damage_rules: DamageRules::from_wc3_misc_text("[Misc]\nDefenseArmor=0.07\n")
                    .unwrap(),
                ..CombatRules::default()
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }

    #[test]
    fn optional_component_presence_changes_authoritative_checksum() {
        let mut absent = Simulation::new(SimulationConfig::default(), 1);
        absent.spawn_unit_with_properties(inert_unit(), UnitGameplayProperties::default());

        let mut present = Simulation::new(SimulationConfig::default(), 1);
        present.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                build_time_ticks: Some(0),
                ..UnitGameplayProperties::default()
            },
        );

        assert_ne!(absent.checksum(), present.checksum());
    }

    #[test]
    fn content_rawcode_changes_authoritative_checksum() {
        let mut first = Simulation::new(SimulationConfig::default(), 1);
        first.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                content: Some(ContentIdentity {
                    map_version: crate::MapVersion::new(1, 1),
                    rawcode: u32::from_be_bytes(*b"u001"),
                    name: "first",
                }),
                ..UnitGameplayProperties::default()
            },
        );

        let mut second = Simulation::new(SimulationConfig::default(), 1);
        second.spawn_unit_with_properties(
            inert_unit(),
            UnitGameplayProperties {
                content: Some(ContentIdentity {
                    map_version: crate::MapVersion::new(1, 1),
                    rawcode: u32::from_be_bytes(*b"u002"),
                    name: "second",
                }),
                ..UnitGameplayProperties::default()
            },
        );

        assert_ne!(first.checksum(), second.checksum());
    }
}
