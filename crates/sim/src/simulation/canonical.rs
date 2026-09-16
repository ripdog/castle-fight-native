use super::*;

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
    } = state;
    let entities = super::snapshot::canonical_entities(world);

    let mut hash = Fnv64::new();
    hash.write_u64(0x4346_5354_4154_4503);
    hash.write_u64(u64::from(CANONICAL_CHECKSUM_SCHEMA_VERSION));
    hash.write_u64(configuration_identity);
    hash.write_u64(next_tick);
    hash.write_u64(next_id);
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
                hash_optional_u32(&mut hash, unit.build_time_ticks);
                hash_optional_u32(&mut hash, unit.repair_time_ticks);
                hash.write_i32(unit.attack.damage);
                hash.write_i32(unit.attack.range);
                hash.write_i32(unit.attack.acquisition_range);
                hash.write_u16(unit.attack.cooldown_ticks);
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
                } else {
                    hash.write_u8(0);
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
                    hash.write_u64(economy.income_per_10k);
                } else {
                    hash.write_u8(0);
                }
                hash_optional_u32(&mut hash, building.repair_time_ticks);
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
                    hash_content_identity(&mut hash, building.production_content);
                    match building.production_corpse {
                        Some(corpse) => {
                            hash.write_u8(1);
                            hash.write_u64(u64::from(corpse.definition.0));
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
                } else {
                    hash.write_u8(0);
                }
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
            CanonicalEntity::BallisticProjectile(projectile) => {
                hash.write_u8(4);
                hash.write_u64(projectile.id.0);
                hash.write_u64(projectile.projectile.source.0);
                hash.write_u8(projectile.projectile.source_team.0);
                hash.write_u8(projectile.projectile.target_mask.bits());
                hash.write_i32(projectile.projectile.damage);
                match projectile.projectile.burning_oil {
                    Some(profile) => {
                        hash.write_u8(1);
                        hash_burning_oil_profile(&mut hash, profile);
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
                hash_optional_u64(&mut hash, corpse.corpse.expires_tick);
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
            CanonicalEntity::Builder(builder) => {
                hash.write_u8(9);
                hash.write_u64(builder.id.0);
                hash.write_u8(builder.owner.0);
                hash.write_u8(builder.team.0);
                hash.write_i32(builder.position.x);
                hash.write_i32(builder.position.y);
                hash.write_i32(builder.profile.speed_per_tick);
                hash.write_i32(builder.profile.build_range);
                hash.write_i32(builder.profile.repair_range);
                hash.write_i32(builder.profile.repair_autocast_range);
                hash.write_u16(builder.profile.repair_time_ratio_numerator);
                hash.write_u16(builder.profile.repair_time_ratio_denominator);
                hash.write_u16(builder.profile.full_repair_duration_ticks);
                hash.write_i32(builder.profile.blink_range);
                hash.write_i32(builder.profile.blink_boundary_inset);
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
                if let Some(order) = builder.build_order {
                    hash.write_u8(1);
                    hash_building_definition(&mut hash, order.building, order.properties);
                } else {
                    hash.write_u8(0);
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
