use super::*;

pub(super) struct ChainLightningHopResolution {
    pub(super) updates: Vec<(Entity, ChainLightningState)>,
    pub(super) removals: Vec<Entity>,
}

pub(super) struct TargetProjectileContext<'a> {
    pub(super) units: &'a mut [UnitSnapshot],
    pub(super) buildings: &'a mut [BuildingSnapshot],
    pub(super) grid: &'a SpatialGrid,
    pub(super) unit_health: &'a mut [i32],
    pub(super) building_health: &'a mut [i32],
    pub(super) positions: &'a [SimPoint],
    pub(super) attackers_this_tick: &'a mut [Option<SimId>],
    pub(super) next_defense_alerts: &'a mut Vec<DefenseAlert>,
    pub(super) completed_tick: u64,
}

pub(super) struct BallisticImpactContext<'a> {
    pub(super) due_projectiles: Vec<BallisticProjectileSnapshot>,
    pub(super) units: &'a mut [UnitSnapshot],
    pub(super) buildings: &'a [BuildingSnapshot],
    pub(super) positions: &'a [SimPoint],
    pub(super) unit_health: &'a mut [i32],
    pub(super) building_health: &'a mut [i32],
    pub(super) attackers_this_tick: &'a mut [Option<SimId>],
    pub(super) next_defense_alerts: &'a mut Vec<DefenseAlert>,
    pub(super) completed_tick: u64,
}

pub(super) struct BallisticImpactResolution {
    pub(super) projectile_entities_to_remove: Vec<Entity>,
    pub(super) projectile_impacts: usize,
    pub(super) projectile_effects: usize,
    pub(super) candidate_checks: usize,
    pub(super) burning_oil_zone_launches: Vec<(
        SimId,
        Team,
        SimPoint,
        crate::components::BurningOilEffectProfile,
    )>,
}

pub(super) struct ProjectileWorldChanges {
    pub(super) chain_lightning_removals: Vec<Entity>,
    pub(super) chain_lightning_updates: Vec<(Entity, ChainLightningState)>,
    pub(super) projectile_removals: Vec<Entity>,
    pub(super) bounce_updates: Vec<BounceProjectileUpdate>,
    pub(super) burning_oil_zone_launches: Vec<(
        SimId,
        Team,
        SimPoint,
        crate::components::BurningOilEffectProfile,
    )>,
    pub(super) chain_lightning_launches: Vec<ChainLightningState>,
    pub(super) projectile_launches: Vec<ProjectileLaunch>,
    pub(super) line_projectile_launches: Vec<LineProjectile>,
    pub(super) reflected_projectile_launches: Vec<ReflectedProjectileLaunch>,
    pub(super) ballistic_projectile_launches: Vec<BallisticProjectileLaunch>,
    pub(super) bounce_projectile_launches: Vec<BounceProjectileLaunch>,
    pub(super) completed_tick: u64,
}

pub(super) struct TargetProjectileResolution {
    pub(super) due_ballistic_projectiles: Vec<BallisticProjectileSnapshot>,
    pub(super) projectile_entities_to_remove: Vec<Entity>,
    pub(super) bounce_projectile_updates: Vec<BounceProjectileUpdate>,
    pub(super) projectile_impacts: usize,
    pub(super) projectile_effects: usize,
    pub(super) projectile_invalidations: usize,
    pub(super) bounce_jumps: usize,
    pub(super) bounce_candidate_checks: usize,
    pub(super) chain_lightning_launches: Vec<ChainLightningState>,
    pub(super) reflected_projectile_launches: Vec<ReflectedProjectileLaunch>,
}

impl Simulation {
    pub(super) fn resolve_due_chain_lightning_hops(
        &mut self,
        units: &[UnitSnapshot],
        unit_health: &mut [i32],
        positions: &[SimPoint],
        completed_tick: u64,
    ) -> ChainLightningHopResolution {
        let mut chain_query = self.world.query::<(Entity, &SimId, &ChainLightningState)>();
        let mut due_chain_lightnings: Vec<_> = chain_query
            .iter(&self.world)
            .filter_map(|(entity, id, state)| {
                (chain_lightning_jump_due_tick(state.started_tick, state.next_jump_index)
                    <= completed_tick)
                    .then_some((entity, *id, *state))
            })
            .collect();
        due_chain_lightnings.sort_unstable_by_key(|(_, id, _)| *id);
        let mut updates = Vec::new();
        let mut removals = Vec::new();
        for (entity, _, mut state) in due_chain_lightnings {
            let origin = find_unit_index(units, state.current_target)
                .filter(|index| unit_health[*index] > 0)
                .map_or(state.last_position, |index| positions[index]);
            let radius_sq = square_i32(state.profile.jump_radius);
            let hit_count = usize::from(state.hit_count);
            let next = units
                .iter()
                .enumerate()
                .filter(|(candidate_index, candidate)| {
                    unit_health[*candidate_index] > 0
                        && candidate.team != state.source_team
                        && state
                            .profile
                            .targets
                            .can_target_unit(candidate.movement_class)
                        && !state.hit_targets[..hit_count].contains(&candidate.id)
                        && origin.distance_sq(positions[*candidate_index]) <= radius_sq
                })
                .min_by_key(|(candidate_index, candidate)| {
                    (
                        origin.distance_sq(positions[*candidate_index]),
                        candidate.id,
                    )
                })
                .map(|(candidate_index, _)| candidate_index);
            let Some(next_index) = next else {
                removals.push(entity);
                continue;
            };

            let adjusted = self
                .combat_rules
                .damage_rules
                .apply_spell(state.next_damage, units[next_index].armor.armor_type);
            let adjusted = spell_damage_after_defend(units[next_index], adjusted, completed_tick);
            unit_health[next_index] = unit_health[next_index]
                .checked_sub(adjusted)
                .expect("Chain Lightning damage overflow");
            let next_position = positions[next_index];
            let mut points = [SimPoint::default(); MAX_BOUNCE_HITS + 1];
            points[0] = origin;
            points[1] = next_position;
            self.last_chain_lightnings.push(ChainLightningEvent {
                source: state.source,
                ability: state.profile.ability,
                bounce_index: state.next_jump_index,
                points,
                point_count: 2,
            });

            let hit_index = usize::from(state.hit_count);
            debug_assert!(hit_index < MAX_BOUNCE_HITS);
            state.hit_targets[hit_index] = units[next_index].id;
            state.hit_count = state
                .hit_count
                .checked_add(1)
                .expect("Chain Lightning hit count overflow");
            state.current_target = units[next_index].id;
            state.last_position = next_position;

            if usize::from(state.hit_count)
                >= usize::from(state.profile.maximum_targets).min(MAX_BOUNCE_HITS)
            {
                removals.push(entity);
                continue;
            }
            state.next_damage = scaled_chain_lightning_damage(
                state.next_damage,
                state.profile.damage_reduction_per_10k,
            );
            if state.next_damage <= 0 {
                removals.push(entity);
                continue;
            }
            state.next_jump_index = state
                .next_jump_index
                .checked_add(1)
                .expect("Chain Lightning jump index overflow");
            updates.push((entity, state));
        }

        ChainLightningHopResolution { updates, removals }
    }

    pub(super) fn resolve_due_target_projectiles(
        &mut self,
        context: TargetProjectileContext<'_>,
    ) -> TargetProjectileResolution {
        let TargetProjectileContext {
            units,
            buildings,
            grid,
            unit_health,
            building_health,
            positions,
            attackers_this_tick,
            next_defense_alerts,
            completed_tick,
        } = context;
        let due_projectiles = self.snapshot_due_projectiles();
        let due_reflected_projectiles = self.snapshot_due_reflected_projectiles();
        let due_bounce_projectiles = self.snapshot_due_bounce_projectiles();
        let due_ballistic_projectiles = self.snapshot_due_ballistic_projectiles();
        let due_target_projectile_count =
            due_projectiles.len() + due_reflected_projectiles.len() + due_bounce_projectiles.len();
        let mut due_target_projectiles = Vec::with_capacity(due_target_projectile_count);
        due_target_projectiles.extend(
            due_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::GuaranteedHit),
        );
        due_target_projectiles.extend(
            due_reflected_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::Reflected),
        );
        due_target_projectiles.extend(
            due_bounce_projectiles
                .into_iter()
                .map(DueTargetProjectileSnapshot::Bounce),
        );
        due_target_projectiles.sort_unstable_by_key(DueTargetProjectileSnapshot::id);
        let mut projectile_entities_to_remove =
            Vec::with_capacity(due_target_projectile_count + due_ballistic_projectiles.len());
        let mut bounce_projectile_updates = Vec::with_capacity(due_target_projectile_count);
        let mut projectile_impacts = 0usize;
        let mut projectile_effects = 0usize;
        let mut projectile_invalidations = 0usize;
        let mut bounce_jumps = 0usize;
        let mut bounce_candidate_checks = 0usize;
        let mut chain_lightning_launches = Vec::new();
        let mut reflected_projectile_launches = Vec::new();

        for snapshot in due_target_projectiles {
            match snapshot {
                DueTargetProjectileSnapshot::GuaranteedHit(snapshot) => {
                    projectile_entities_to_remove.push(snapshot.entity);
                    let Some(target) =
                        find_target_index(units, buildings, snapshot.projectile.target)
                    else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    let Some(impact_position) = live_target_position(
                        target,
                        units,
                        buildings,
                        unit_health,
                        building_health,
                        self.config.navigation_cell_size,
                    ) else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    let defense = resolve_directed_projectile_defense(
                        target,
                        snapshot.id,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.config.match_seed,
                        units,
                    );
                    if defense.reflected
                        && !snapshot.projectile.source_is_building
                        && let Some(source_index) =
                            find_unit_index(units, snapshot.projectile.source)
                                .filter(|index| unit_health[*index] > 0)
                    {
                        let source_position = positions[source_index];
                        let impact_tick = completed_tick
                            .checked_add(projectile_travel_ticks(
                                impact_position.distance_sq(source_position),
                                snapshot.projectile.speed_per_tick,
                            ))
                            .expect("reflected projectile impact tick overflow");
                        reflected_projectile_launches.push(ReflectedProjectileLaunch {
                            original_source: snapshot.projectile.source,
                            reflector: target_sim_id(target, units, buildings),
                            reflector_team: match target {
                                TargetIndex::Unit(index) => units[index].team,
                                TargetIndex::Building(index) => buildings[index].team,
                            },
                            target: snapshot.projectile.source,
                            damage: snapshot.projectile.damage,
                            damage_type: snapshot.projectile.damage_type,
                            launch_position: impact_position,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    projectile_impacts += 1;
                    if defense.damage <= 0 {
                        continue;
                    }
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        defense.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.debug_buildings_invulnerable,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units,
                            buildings,
                            unit_positions: positions,
                            unit_health,
                            building_health,
                            attackers_this_tick,
                            next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        if !defense.reflected {
                            let pending = apply_pending_attack_effects(
                                target,
                                snapshot.projectile.on_hit,
                                PendingAttackEffectSource {
                                    id: snapshot.projectile.source,
                                    position: snapshot.projectile.launch_position,
                                    team: snapshot.projectile.source_team,
                                },
                                PendingAttackEffectState {
                                    completed_tick,
                                    units,
                                    unit_health,
                                    buildings,
                                    building_health,
                                    damage_rules: self.combat_rules.damage_rules,
                                },
                            );
                            if let Some(event) = pending.chain_event {
                                self.last_chain_lightnings.push(event);
                            }
                            if let Some(state) = pending.chain_state {
                                chain_lightning_launches.push(state);
                            }
                        }
                        projectile_effects += 1;
                    } else {
                        projectile_invalidations += 1;
                    }
                }
                DueTargetProjectileSnapshot::Reflected(snapshot) => {
                    projectile_entities_to_remove.push(snapshot.entity);
                    let Some(target) =
                        find_target_index(units, buildings, snapshot.projectile.target)
                    else {
                        projectile_invalidations += 1;
                        continue;
                    };
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.reflector,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.debug_buildings_invulnerable,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units,
                            buildings,
                            unit_positions: positions,
                            unit_health,
                            building_health,
                            attackers_this_tick,
                            next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        projectile_impacts += 1;
                        projectile_effects += 1;
                    } else {
                        projectile_invalidations += 1;
                    }
                }
                DueTargetProjectileSnapshot::Bounce(snapshot) => {
                    let Some(target) =
                        find_target_index(units, buildings, snapshot.projectile.target)
                    else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    };
                    let Some(impact_position) = live_target_position(
                        target,
                        units,
                        buildings,
                        unit_health,
                        building_health,
                        self.config.navigation_cell_size,
                    ) else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    };
                    let defense = resolve_directed_projectile_defense(
                        target,
                        snapshot.id,
                        snapshot.projectile.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.config.match_seed,
                        units,
                    );
                    if defense.reflected {
                        projectile_entities_to_remove.push(snapshot.entity);
                        if !snapshot.projectile.source_is_building
                            && let Some(source_index) =
                                find_unit_index(units, snapshot.projectile.source)
                                    .filter(|index| unit_health[*index] > 0)
                        {
                            let source_position = positions[source_index];
                            reflected_projectile_launches.push(ReflectedProjectileLaunch {
                                original_source: snapshot.projectile.source,
                                reflector: target_sim_id(target, units, buildings),
                                reflector_team: match target {
                                    TargetIndex::Unit(index) => units[index].team,
                                    TargetIndex::Building(index) => buildings[index].team,
                                },
                                target: snapshot.projectile.source,
                                damage: snapshot.projectile.damage,
                                damage_type: snapshot.projectile.damage_type,
                                launch_position: impact_position,
                                launch_tick: completed_tick,
                                impact_tick: completed_tick
                                    .checked_add(projectile_travel_ticks(
                                        impact_position.distance_sq(source_position),
                                        snapshot.projectile.speed_per_tick,
                                    ))
                                    .expect("reflected projectile impact tick overflow"),
                            });
                        }
                        if defense.damage <= 0 {
                            projectile_impacts += 1;
                            continue;
                        }
                    }
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        defense.damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.debug_buildings_invulnerable,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units,
                            buildings,
                            unit_positions: positions,
                            unit_health,
                            building_health,
                            attackers_this_tick,
                            next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_none()
                    {
                        projectile_entities_to_remove.push(snapshot.entity);
                        projectile_invalidations += 1;
                        continue;
                    }
                    projectile_impacts += 1;
                    projectile_effects += 1;

                    if defense.reflected || snapshot.projectile.remaining_bounces == 0 {
                        projectile_entities_to_remove.push(snapshot.entity);
                        continue;
                    }

                    let bounce_search = BounceSearchContext {
                        completed_tick,
                        units,
                        unit_health,
                        grid,
                    };
                    let Some(next_index) = self.select_bounce_target(
                        snapshot.id,
                        &snapshot.projectile,
                        impact_position,
                        &bounce_search,
                        &mut bounce_candidate_checks,
                    ) else {
                        projectile_entities_to_remove.push(snapshot.entity);
                        continue;
                    };
                    let next_target = units[next_index].id;
                    let next_position = units[next_index].position;
                    let mut projectile = snapshot.projectile;
                    let next_bounce_index = projectile
                        .bounce_index
                        .checked_add(1)
                        .expect("bounce index overflow");
                    let hit_index = usize::from(projectile.hit_count);
                    debug_assert!(hit_index < MAX_BOUNCE_HITS);
                    projectile.hit_targets[hit_index] = next_target;
                    projectile.hit_count = projectile
                        .hit_count
                        .checked_add(1)
                        .expect("bounce hit count overflow");
                    projectile.target = next_target;
                    projectile.damage = scaled_bounce_damage(
                        projectile.damage,
                        projectile.damage_percent_per_bounce,
                    );
                    projectile.launch_position = impact_position;
                    projectile.launch_tick = completed_tick;
                    projectile.impact_tick = completed_tick
                        .checked_add(projectile_travel_ticks(
                            impact_position.distance_sq(next_position),
                            projectile.speed_per_tick,
                        ))
                        .expect("bounce impact tick overflow");
                    projectile.remaining_bounces -= 1;
                    projectile.bounce_index = next_bounce_index;
                    bounce_projectile_updates.push(BounceProjectileUpdate {
                        entity: snapshot.entity,
                        projectile,
                    });
                    bounce_jumps += 1;
                }
            }
        }

        TargetProjectileResolution {
            due_ballistic_projectiles,
            projectile_entities_to_remove,
            bounce_projectile_updates,
            projectile_impacts,
            projectile_effects,
            projectile_invalidations,
            bounce_jumps,
            bounce_candidate_checks,
            chain_lightning_launches,
            reflected_projectile_launches,
        }
    }

    pub(super) fn resolve_due_ballistic_impacts(
        &mut self,
        context: BallisticImpactContext<'_>,
    ) -> BallisticImpactResolution {
        let BallisticImpactContext {
            due_projectiles,
            units,
            buildings,
            positions,
            unit_health,
            building_health,
            attackers_this_tick,
            next_defense_alerts,
            completed_tick,
        } = context;
        let mut projectile_entities_to_remove = Vec::with_capacity(due_projectiles.len());
        let mut projectile_impacts = 0usize;
        let mut projectile_effects = 0usize;
        let mut candidate_checks = 0usize;
        let mut burning_oil_zone_launches = Vec::new();

        if !due_projectiles.is_empty() {
            let impact_grid = SpatialGrid::build(
                self.config.spatial_cell_size,
                units
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| unit_health[*index] > 0)
                    .map(|(index, unit)| {
                        (
                            SpatialPartition::global(unit.team.0),
                            index,
                            positions[index],
                        )
                    }),
            );
            for snapshot in due_projectiles {
                projectile_entities_to_remove.push(snapshot.entity);
                projectile_impacts += 1;
                let enemy_team = 1u8
                    .checked_sub(snapshot.projectile.source_team.0)
                    .expect("verification slice supports teams 0 and 1 only");
                let radius_sq = square_i32(snapshot.projectile.impact_radius);
                let splash_targets = snapshot
                    .projectile
                    .splash_falloff
                    .map_or(snapshot.projectile.target_mask, |profile| profile.targets);
                let mut targets = Vec::new();
                impact_grid.for_each_candidate(
                    SpatialPartition::global(enemy_team),
                    snapshot.projectile.destination,
                    snapshot.projectile.impact_radius,
                    |unit_index| {
                        candidate_checks += 1;
                        if unit_health[unit_index] > 0
                            && splash_targets.can_target_unit(units[unit_index].movement_class)
                            && snapshot
                                .projectile
                                .destination
                                .distance_sq(positions[unit_index])
                                <= radius_sq
                        {
                            targets.push(TargetIndex::Unit(unit_index));
                        }
                    },
                );
                for (building_index, building) in buildings.iter().enumerate() {
                    if !splash_targets.can_target_buildings()
                        || building.team.0 != enemy_team
                        || building_health[building_index] <= 0
                    {
                        continue;
                    }
                    candidate_checks += 1;
                    if point_to_footprint_distance_sq(
                        snapshot.projectile.destination,
                        building.footprint,
                        self.config.navigation_cell_size,
                    ) <= radius_sq
                    {
                        targets.push(TargetIndex::Building(building_index));
                    }
                }
                targets.sort_unstable_by_key(|target| target_sim_id(*target, units, buildings));
                for target in targets {
                    let distance_sq = match target {
                        TargetIndex::Unit(index) => snapshot
                            .projectile
                            .destination
                            .distance_sq(positions[index]),
                        TargetIndex::Building(index) => point_to_footprint_distance_sq(
                            snapshot.projectile.destination,
                            buildings[index].footprint,
                            self.config.navigation_cell_size,
                        ),
                    };
                    let scaled_damage = snapshot.projectile.splash_falloff.map_or(
                        snapshot.projectile.damage,
                        |profile| {
                            let factor = if distance_sq <= square_i32(profile.full_radius) {
                                10_000
                            } else if distance_sq <= square_i32(profile.medium_radius) {
                                i32::from(profile.medium_damage_per_10k)
                            } else {
                                i32::from(profile.outer_damage_per_10k)
                            };
                            i32::try_from(
                                i64::from(snapshot.projectile.damage) * i64::from(factor) / 10_000,
                            )
                            .expect("splash damage overflowed")
                        },
                    );
                    let damage = ranged_projectile_damage_after_defend(
                        target,
                        scaled_damage,
                        completed_tick,
                        units,
                    );
                    if apply_damage_to_target(
                        target,
                        snapshot.projectile.source,
                        damage,
                        snapshot.projectile.damage_type,
                        completed_tick,
                        self.debug_buildings_invulnerable,
                        DamageTargetState {
                            damage_rules: self.combat_rules.damage_rules,
                            units,
                            buildings,
                            unit_positions: positions,
                            unit_health,
                            building_health,
                            attackers_this_tick,
                            next_defense_alerts,
                            navigation_cell_size: self.config.navigation_cell_size,
                        },
                    )
                    .is_some()
                    {
                        projectile_effects += 1;
                        if let TargetIndex::Unit(index) = target
                            && let Some(profile) = snapshot.projectile.frost
                            && unit_health[index] > 0
                            && !units[index].classifications.spell_immune
                            && !units[index].classifications.invulnerable
                            && profile.targets.can_target_unit(units[index].movement_class)
                        {
                            apply_frost_attack(&mut units[index], profile, completed_tick);
                        }
                    }
                }
                if let Some(profile) = snapshot.projectile.burning_oil {
                    burning_oil_zone_launches.push((
                        snapshot.projectile.source,
                        snapshot.projectile.source_team,
                        snapshot.projectile.destination,
                        profile,
                    ));
                }
            }
        }

        BallisticImpactResolution {
            projectile_entities_to_remove,
            projectile_impacts,
            projectile_effects,
            candidate_checks,
            burning_oil_zone_launches,
        }
    }

    pub(super) fn commit_projectile_world_changes(&mut self, changes: ProjectileWorldChanges) {
        let ProjectileWorldChanges {
            chain_lightning_removals,
            chain_lightning_updates,
            projectile_removals,
            bounce_updates,
            burning_oil_zone_launches,
            chain_lightning_launches,
            projectile_launches,
            line_projectile_launches,
            reflected_projectile_launches,
            ballistic_projectile_launches,
            bounce_projectile_launches,
            completed_tick,
        } = changes;

        for entity in chain_lightning_removals {
            self.world.despawn(entity);
        }
        for (entity, state) in chain_lightning_updates {
            if let Some(mut stored) = self
                .world
                .entity_mut(entity)
                .get_mut::<ChainLightningState>()
            {
                *stored = state;
            }
        }
        for entity in projectile_removals {
            self.world.despawn(entity);
        }
        for update in bounce_updates {
            *self
                .world
                .entity_mut(update.entity)
                .get_mut::<BounceProjectile>()
                .expect("bounce projectile missing during hop update") = update.projectile;
        }
        for (source, source_team, center, profile) in burning_oil_zone_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                BurningOilZone {
                    source,
                    source_team,
                    center,
                    profile,
                    created_tick: completed_tick,
                    pulse_index: 1,
                },
            ));
        }
        for state in chain_lightning_launches {
            let id = self.allocate_id();
            self.world.spawn((id, state));
        }
        for launch in projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                GuaranteedHitProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    source_is_building: launch.source_is_building,
                    target: launch.target,
                    damage: launch.damage,
                    on_hit: launch.on_hit,
                    damage_type: launch.damage_type,
                    speed_per_tick: launch.speed_per_tick,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in reflected_projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                ReflectedProjectile {
                    original_source: launch.original_source,
                    reflector: launch.reflector,
                    reflector_team: launch.reflector_team,
                    target: launch.target,
                    damage: launch.damage,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in ballistic_projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                BallisticProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    target_mask: launch.target_mask,
                    damage: launch.damage,
                    burning_oil: launch.burning_oil,
                    splash_falloff: launch.splash_falloff,
                    frost: launch.frost,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    destination: launch.destination,
                    impact_radius: launch.impact_radius,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                },
            ));
        }
        for launch in line_projectile_launches {
            let id = self.allocate_id();
            self.world.spawn((id, launch));
        }
        for launch in bounce_projectile_launches {
            let id = self.allocate_id();
            let mut hit_targets = [SimId(0); MAX_BOUNCE_HITS];
            hit_targets[0] = launch.target;
            self.world.spawn((
                id,
                BounceProjectile {
                    source: launch.source,
                    source_team: launch.source_team,
                    source_is_building: launch.source_is_building,
                    target_mask: launch.target_mask,
                    target: launch.target,
                    damage: launch.damage,
                    damage_type: launch.damage_type,
                    launch_position: launch.launch_position,
                    launch_tick: launch.launch_tick,
                    impact_tick: launch.impact_tick,
                    speed_per_tick: launch.speed_per_tick,
                    bounce_range: launch.bounce_range,
                    remaining_bounces: launch.max_bounces,
                    bounce_index: 0,
                    damage_percent_per_bounce: launch.damage_percent_per_bounce,
                    allow_repeat_targets: launch.allow_repeat_targets,
                    hit_targets,
                    hit_count: 1,
                },
            ));
        }
    }

    pub(super) fn resolve_burning_oil_zones(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        completed_tick: u64,
    ) {
        let mut query = self.world.query::<(Entity, &SimId, &BurningOilZone)>();
        let mut zones: Vec<_> = query
            .iter(&self.world)
            .map(|(entity, id, zone)| (entity, *id, *zone))
            .collect();
        zones.sort_unstable_by_key(|(_, id, _)| *id);

        let mut expired = Vec::new();
        let mut updates = Vec::new();
        for (entity, _, mut zone) in zones {
            let expires_tick = zone
                .created_tick
                .checked_add(ceil_millis_to_ticks(u64::from(
                    zone.profile.total_duration_millis,
                )))
                .expect("Burning Oil expiry tick overflow");
            if completed_tick >= expires_tick {
                expired.push(entity);
                continue;
            }

            while let Some((offset_millis, damage)) =
                burning_oil_pulse(zone.profile, zone.pulse_index)
            {
                let due_tick = zone
                    .created_tick
                    .checked_add(ceil_millis_to_ticks(u64::from(offset_millis)))
                    .expect("Burning Oil pulse tick overflow");
                if due_tick > completed_tick {
                    break;
                }
                let radius_sq = square_i32(zone.profile.radius);
                if zone.profile.target_ground_units {
                    for unit in units.iter_mut() {
                        if unit.health <= 0
                            || unit.team == zone.source_team
                            || unit.movement_class != MovementClass::Ground
                            || zone.center.distance_sq(unit.position) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, unit.armor.armor_type);
                        let adjusted = spell_damage_after_defend(*unit, adjusted, completed_tick);
                        unit.health = unit
                            .health
                            .checked_sub(adjusted)
                            .expect("Burning Oil unit damage overflow");
                    }
                }
                if zone.profile.target_buildings && !self.debug_buildings_invulnerable {
                    for building in buildings.iter_mut() {
                        if building.health <= 0 || building.team == zone.source_team {
                            continue;
                        }
                        if point_to_footprint_distance_sq(
                            zone.center,
                            building.footprint,
                            self.config.navigation_cell_size,
                        ) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, building.armor.armor_type);
                        building.health = building
                            .health
                            .checked_sub(scale_damage_per_10k(
                                adjusted,
                                building.snow_damage_taken_per_10k,
                            ))
                            .expect("Burning Oil building damage overflow");
                    }
                }
                zone.pulse_index = zone
                    .pulse_index
                    .checked_add(1)
                    .expect("Burning Oil pulse index overflow");
            }
            updates.push((entity, zone));
        }

        for entity in expired {
            self.world.despawn(entity);
        }
        for (entity, zone) in updates {
            if let Some(mut stored) = self.world.entity_mut(entity).get_mut::<BurningOilZone>() {
                *stored = zone;
            }
        }
    }

    pub(super) fn snapshot_due_projectiles(&mut self) -> Vec<ProjectileSnapshot> {
        let mut query = self
            .world
            .query::<(Entity, &SimId, &GuaranteedHitProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_reflected_projectiles(
        &mut self,
    ) -> Vec<ReflectedProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &ReflectedProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ReflectedProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_bounce_projectiles(&mut self) -> Vec<BounceProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BounceProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BounceProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_ballistic_projectiles(
        &mut self,
    ) -> Vec<BallisticProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BallisticProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BallisticProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn select_bounce_target(
        &self,
        projectile_id: SimId,
        projectile: &BounceProjectile,
        impact_position: SimPoint,
        context: &BounceSearchContext<'_>,
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let enemy_team = 1u8
            .checked_sub(projectile.source_team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let range_sq = square_i32(projectile.bounce_range);
        let hit_count = usize::from(projectile.hit_count);
        let hit_targets = &projectile.hit_targets[..hit_count];
        let next_bounce_index = u32::from(projectile.bounce_index) + 1;
        let mut best: Option<(u64, SimId, usize)> = None;
        context.grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            impact_position,
            projectile.bounce_range,
            |unit_index| {
                *candidate_checks += 1;
                if context.unit_health[unit_index] <= 0 {
                    return;
                }
                let candidate = &context.units[unit_index];
                if !projectile
                    .target_mask
                    .can_target_unit(candidate.movement_class)
                    || candidate.id == projectile.target
                    || impact_position.distance_sq(candidate.position) > range_sq
                {
                    return;
                }
                if !projectile.allow_repeat_targets && hit_targets.contains(&candidate.id) {
                    return;
                }
                let rank = deterministic_random(
                    self.config.match_seed,
                    context.completed_tick,
                    projectile_id,
                    RANDOM_PURPOSE_BOUNCE_TARGET ^ candidate.id.0,
                    u64::from(next_bounce_index),
                );
                let key = (rank, candidate.id, unit_index);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );
        best.map(|(_, _, unit_index)| unit_index)
    }
}
