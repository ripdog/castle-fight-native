use super::*;

impl Simulation {
    pub(super) fn resolve_automatic_abilities(
        &mut self,
        buildings: &mut [BuildingSnapshot],
        units: &mut [UnitSnapshot],
        grid: &SpatialGrid,
    ) -> AbilityMetrics {
        let building_evaluations: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .enumerate()
                .map(|(source_index, source)| {
                    self.evaluate_automatic_ability(
                        AbilitySourceSnapshot {
                            source: AbilitySourceIndex::Building(source_index),
                            id: source.id,
                            team: source.team,
                            origin: AbilitySourceOrigin::Building(source.footprint),
                            health: source.health,
                            stunned_until_tick: source
                                .status
                                .map_or(0, |status| status.stunned_until_tick),
                            spellcasting: source.spellcasting,
                            mana_current: source.mana_current,
                            ability_state: source.ability_state,
                        },
                        units,
                        grid,
                    )
                })
                .collect()
        });
        let unit_evaluations: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(source_index, source)| {
                    self.evaluate_automatic_ability(
                        AbilitySourceSnapshot {
                            source: AbilitySourceIndex::Unit(source_index),
                            id: source.id,
                            team: source.team,
                            origin: AbilitySourceOrigin::Unit(source.position),
                            health: source.health,
                            stunned_until_tick: source.status.stunned_until_tick,
                            spellcasting: source.spellcasting,
                            mana_current: source.mana_current,
                            ability_state: source.ability_state,
                        },
                        units,
                        grid,
                    )
                })
                .collect()
        });
        let mut metrics = AbilityMetrics {
            evaluations: buildings
                .iter()
                .filter(|building| building.spellcasting.is_some())
                .count()
                + units
                    .iter()
                    .filter(|unit| unit.spellcasting.is_some())
                    .count(),
            candidate_checks: building_evaluations
                .iter()
                .chain(&unit_evaluations)
                .map(|evaluation| evaluation.candidate_checks)
                .sum(),
            ..AbilityMetrics::default()
        };
        let mut intents: Vec<_> = building_evaluations
            .into_iter()
            .chain(unit_evaluations)
            .filter_map(|evaluation| evaluation.intent)
            .collect();
        intents.sort_unstable_by_key(|intent| {
            let (target_kind, target_id) = intent.target.sort_key();
            (
                intent.source_id,
                intent.ability.id,
                intent.cast_sequence,
                target_kind,
                target_id,
            )
        });

        for intent in intents {
            let source = match intent.source {
                AbilitySourceIndex::Unit(index) => {
                    let source = &units[index];
                    AbilitySourceSnapshot {
                        source: intent.source,
                        id: source.id,
                        team: source.team,
                        origin: AbilitySourceOrigin::Unit(source.position),
                        health: source.health,
                        stunned_until_tick: source.status.stunned_until_tick,
                        spellcasting: source.spellcasting,
                        mana_current: source.mana_current,
                        ability_state: source.ability_state,
                    }
                }
                AbilitySourceIndex::Building(index) => {
                    let source = &buildings[index];
                    AbilitySourceSnapshot {
                        source: intent.source,
                        id: source.id,
                        team: source.team,
                        origin: AbilitySourceOrigin::Building(source.footprint),
                        health: source.health,
                        stunned_until_tick: source
                            .status
                            .map_or(0, |status| status.stunned_until_tick),
                        spellcasting: source.spellcasting,
                        mana_current: source.mana_current,
                        ability_state: source.ability_state,
                    }
                }
            };
            if source.health <= 0
                || source.id != intent.source_id
                || self.next_tick < source.stunned_until_tick
            {
                continue;
            }
            let Some(spellcasting) = source.spellcasting else {
                continue;
            };
            if spellcasting.ability != intent.ability {
                continue;
            }
            let Some(mut state) = source.ability_state else {
                continue;
            };
            let Some(mana) = source.mana_current else {
                continue;
            };
            if state.cast_sequence != intent.cast_sequence
                || state.ready_tick > self.next_tick
                || mana < intent.ability.mana_cost
                || !self.ability_target_is_valid(source, intent.target, intent.ability, units)
            {
                continue;
            }

            let remaining_mana = mana
                .checked_sub(intent.ability.mana_cost)
                .expect("ability mana cost exceeded validated current mana");
            state.ready_tick = self
                .next_tick
                .checked_add(u64::from(intent.ability.cooldown_ticks))
                .expect("ability cooldown tick overflow");
            state.cast_sequence = state
                .cast_sequence
                .checked_add(1)
                .expect("ability cast sequence exhausted");
            match intent.source {
                AbilitySourceIndex::Unit(index) => {
                    units[index].mana_current = Some(remaining_mana);
                    units[index].ability_state = Some(state);
                    if intent.ability.id == AbilityId(u32::from_be_bytes(*b"A00K")) {
                        let retreat_start = self
                            .next_tick
                            .checked_add(
                                u64::try_from(3 * CASTLE_FIGHT_SIMULATION_HZ / 10)
                                    .expect("simulation Hz must be positive"),
                            )
                            .expect("Warlock retreat start overflow");
                        let retreat_end = retreat_start
                            .checked_add(
                                u64::try_from(5 * CASTLE_FIGHT_SIMULATION_HZ)
                                    .expect("simulation Hz must be positive"),
                            )
                            .expect("Warlock retreat end overflow");
                        let recovery_end = retreat_end
                            .checked_add(
                                u64::try_from(66 * CASTLE_FIGHT_SIMULATION_HZ / 10)
                                    .expect("simulation Hz must be positive"),
                            )
                            .expect("Warlock recovery end overflow");
                        units[index].status.warlock_retreat_start_tick = retreat_start;
                        units[index].status.warlock_retreat_end_tick = retreat_end;
                        units[index].status.stunned_until_tick =
                            units[index].status.stunned_until_tick.max(recovery_end);
                        units[index].target = None;
                        units[index].direct_retaliation_lock = false;
                        units[index].ally_defense_lock = false;
                    }
                    for effect in units[index].passive_effects.iter() {
                        let PassiveUnitEffect::Aura(profile) = effect else {
                            continue;
                        };
                        if !profile.suspend_during_spell_cooldown {
                            continue;
                        }
                        for modifier in units[index].status.armor_modifiers
                            [..usize::from(units[index].status.armor_modifier_count)]
                            .iter_mut()
                        {
                            if modifier.id.0 == profile.ability.0 {
                                modifier.expires_tick = self.next_tick;
                            }
                        }
                    }
                }
                AbilitySourceIndex::Building(index) => {
                    buildings[index].mana_current = Some(remaining_mana);
                    buildings[index].ability_state = Some(state);
                }
            }

            let target_position = match intent.target {
                AbilityIntentTarget::Unit { index, .. } => Some(units[index].position),
                AbilityIntentTarget::Corpse { position, .. }
                | AbilityIntentTarget::Point { position } => Some(position),
                AbilityIntentTarget::AllEnemyUnits | AbilityIntentTarget::AllFriendlyUnits => None,
            };

            match intent.target {
                AbilityIntentTarget::Unit { index, .. } => {
                    if let AbilityEffect::Prayer { radius, .. } = intent.ability.effect {
                        let center = units[index].position;
                        let radius_sq = square_i32(radius);
                        for target in units.iter_mut() {
                            if target.health <= 0
                                || target.team != source.team
                                || center.distance_sq(target.position) > radius_sq
                            {
                                continue;
                            }
                            if apply_ability_effect_to_unit(
                                target,
                                intent.ability.effect,
                                self.next_tick,
                                self.combat_rules.damage_rules,
                            ) {
                                metrics.effects += 1;
                            }
                        }
                    } else if let AbilityEffect::AreaDamage { radius, .. } = intent.ability.effect {
                        let center = units[index].position;
                        let radius_sq = square_i32(radius);
                        for target in units.iter_mut() {
                            if target.health <= 0
                                || target.team == source.team
                                || center.distance_sq(target.position) > radius_sq
                            {
                                continue;
                            }
                            if apply_ability_effect_to_unit(
                                target,
                                intent.ability.effect,
                                self.next_tick,
                                self.combat_rules.damage_rules,
                            ) {
                                metrics.effects += 1;
                            }
                        }
                    } else if apply_ability_effect_to_unit(
                        &mut units[index],
                        intent.ability.effect,
                        self.next_tick,
                        self.combat_rules.damage_rules,
                    ) {
                        metrics.effects += 1;
                    }
                }
                AbilityIntentTarget::AllEnemyUnits => {
                    for target in units.iter_mut() {
                        if target.health <= 0 || target.team == source.team {
                            continue;
                        }
                        if apply_ability_effect_to_unit(
                            target,
                            intent.ability.effect,
                            self.next_tick,
                            self.combat_rules.damage_rules,
                        ) {
                            metrics.effects += 1;
                        }
                    }
                }
                AbilityIntentTarget::AllFriendlyUnits => {
                    let AbilityEffect::HolyFervour {
                        modifier,
                        radius,
                        duration_ticks,
                    } = intent.ability.effect
                    else {
                        unreachable!("friendly area target requires Holy Fervour")
                    };
                    let level =
                        self.gjallarhorn_constructed_count[usize::from(source.team.0)].clamp(1, 4);
                    let percent_delta = 40 + 5 * (level as i16 - 1);
                    let expires_tick = self.next_tick + u64::from(duration_ticks);
                    for target in units.iter_mut() {
                        if target.health > 0
                            && target.team == source.team
                            && self.ability_source_distance_sq(source.origin, target.position)
                                <= square_i32(radius)
                        {
                            apply_timed_attack_speed_modifier(
                                &mut target.status,
                                modifier,
                                percent_delta,
                                expires_tick,
                            );
                            metrics.effects += 1;
                        }
                    }
                }
                AbilityIntentTarget::Corpse { position, .. } => {
                    let AbilityEffect::Purification {
                        damage,
                        radius,
                        consume_radius,
                        reveal_radius: _,
                        reveal_duration_ticks: _,
                    } = intent.ability.effect
                    else {
                        unreachable!("corpse target requires Purification")
                    };
                    for target in units.iter_mut() {
                        if target.health > 0
                            && target.team != source.team
                            && position.distance_sq(target.position) <= square_i32(radius)
                        {
                            target.health = target.health.saturating_sub(damage);
                            metrics.effects += 1;
                        }
                    }
                    let mut corpses = self
                        .world
                        .iter_entities()
                        .filter_map(|entity| {
                            let corpse = entity.get::<Corpse>()?;
                            if !crate::content::vessel_corpse_qualifies_927(corpse.definition) {
                                return None;
                            }
                            let corpse_position = entity.get::<Position>()?.0;
                            (position.distance_sq(corpse_position) <= square_i32(consume_radius))
                                .then_some((*entity.get::<SimId>()?, entity.id()))
                        })
                        .collect::<Vec<_>>();
                    corpses.sort_unstable_by_key(|(id, _)| *id);
                    for (_, entity) in corpses {
                        self.world.despawn(entity);
                    }
                }
                AbilityIntentTarget::Point { position } => {
                    let AbilityEffect::ArtilleryBombardment {
                        min_damage,
                        max_damage,
                        speed_per_tick,
                        splash,
                        burning_oil,
                    } = intent.ability.effect
                    else {
                        unreachable!("ground point target requires Artillery")
                    };
                    let AbilitySourceOrigin::Building(footprint) = source.origin else {
                        unreachable!("Artillery must originate at a building")
                    };
                    let launch_position =
                        footprint_center_point(footprint, self.config.navigation_cell_size);
                    let roll = deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        source.id,
                        RANDOM_PURPOSE_ARTILLERY_DAMAGE,
                        state.cast_sequence,
                    );
                    let damage = min_damage
                        + i32::try_from(roll % u64::try_from(max_damage - min_damage + 1).unwrap())
                            .unwrap();
                    let impact_tick = self.next_tick
                        + projectile_travel_ticks(
                            launch_position.distance_sq(position),
                            speed_per_tick,
                        );
                    let projectile_id = self.allocate_id();
                    self.world.spawn((
                        projectile_id,
                        BallisticProjectile {
                            source: source.id,
                            source_team: source.team,
                            target_mask: AttackTargetMask::ALL,
                            damage,
                            burning_oil: Some(burning_oil),
                            splash_falloff: Some(splash),
                            damage_type: DamageType::Siege,
                            launch_position,
                            destination: position,
                            impact_radius: splash.outer_radius,
                            launch_tick: self.next_tick,
                            impact_tick,
                        },
                    ));
                    metrics.effects += 1;
                }
            }
            let resurrection = match intent.ability.effect {
                AbilityEffect::HolyAid {
                    resurrection_count,
                    resurrection_radius,
                    ..
                } => Some((resurrection_count, resurrection_radius, source.origin)),
                AbilityEffect::Prayer {
                    resurrection_count,
                    resurrection_radius,
                    ..
                } => target_position.map(|position| {
                    (
                        resurrection_count,
                        resurrection_radius,
                        AbilitySourceOrigin::Unit(position),
                    )
                }),
                _ => None,
            };
            if let Some((count, radius, origin)) = resurrection {
                metrics.effects +=
                    self.resurrect_friendly_corpses(source.team, origin, radius, count);
            }
            self.last_ability_casts.push(AbilityCastEvent {
                source: intent.source_id,
                ability: intent.ability.id,
                target: intent.target.cast_target(),
                target_position,
                effect: intent.ability.effect,
            });
            metrics.casts += 1;
        }

        metrics
    }

    fn resurrect_friendly_corpses(
        &mut self,
        team: Team,
        origin: AbilitySourceOrigin,
        radius: i32,
        count: u8,
    ) -> usize {
        if count == 0 {
            return 0;
        }
        let radius_sq = square_i32(radius);
        let mut query = self.world.query::<(Entity, &SimId, &Corpse, &Position)>();
        let mut candidates = query
            .iter(&self.world)
            .filter_map(|(entity, id, corpse, position)| {
                (corpse.source_team == team
                    && corpse
                        .resurrection
                        .is_some_and(|definition| !definition.properties.mechanical)
                    && self.ability_source_distance_sq(origin, position.0) <= radius_sq)
                    .then_some((
                        self.ability_source_distance_sq(origin, position.0),
                        *id,
                        entity,
                        *corpse,
                        position.0,
                    ))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|(distance, id, ..)| (*distance, *id));
        let mut revived = 0;
        for (_, _, entity, corpse, position) in candidates.into_iter().take(usize::from(count)) {
            let definition = corpse
                .resurrection
                .expect("eligible corpse retains template");
            self.world.despawn(entity);
            self.spawn_unit_unchecked(
                Some(corpse.source_owner),
                UnitSpawn::from_template(team, position, definition.template),
                definition.properties,
                definition.spellcasting,
            );
            revived += 1;
        }
        revived
    }

    fn evaluate_automatic_ability(
        &self,
        source: AbilitySourceSnapshot,
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
    ) -> AbilityEvaluation {
        let Some(spellcasting) = source.spellcasting else {
            return AbilityEvaluation::default();
        };
        if source.health <= 0 || self.next_tick < source.stunned_until_tick {
            return AbilityEvaluation::default();
        }
        let Some(state) = source.ability_state else {
            return AbilityEvaluation::default();
        };
        let Some(mana) = source.mana_current else {
            return AbilityEvaluation::default();
        };
        if state.ready_tick > self.next_tick || mana < spellcasting.ability.mana_cost {
            return AbilityEvaluation::default();
        }

        let mut candidate_checks = 0usize;
        let target = match spellcasting.ability.target_policy {
            AbilityTargetPolicy::RandomEnemyUnit | AbilityTargetPolicy::RandomGroundEnemyUnit => {
                self.random_enemy_ability_target(
                    source,
                    spellcasting.ability,
                    state.cast_sequence,
                    units,
                    grid,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                })
            }
            AbilityTargetPolicy::RandomEnemyUnitGlobal => self
                .random_enemy_ability_target_global(
                    source,
                    spellcasting.ability,
                    state.cast_sequence,
                    units,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
            AbilityTargetPolicy::AllEnemyUnits => {
                candidate_checks = units.len();
                units
                    .iter()
                    .any(|unit| unit.health > 0 && unit.team != source.team)
                    .then_some(AbilityIntentTarget::AllEnemyUnits)
            }
            AbilityTargetPolicy::RecentlyAttackedFriendlyUnit => self
                .recently_attacked_friendly_ability_target(
                    source,
                    spellcasting.ability,
                    units,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
            AbilityTargetPolicy::WoundedFriendlyUnit => self
                .wounded_friendly_ability_target(
                    source,
                    spellcasting.ability,
                    units,
                    &mut candidate_checks,
                )
                .map(|index| AbilityIntentTarget::Unit {
                    index,
                    id: units[index].id,
                }),
            AbilityTargetPolicy::AllFriendlyUnits => {
                candidate_checks = units.len();
                units
                    .iter()
                    .any(|target| {
                        target.health > 0
                            && target.team == source.team
                            && self.ability_source_distance_sq(source.origin, target.position)
                                <= square_i32(spellcasting.ability.range)
                    })
                    .then_some(AbilityIntentTarget::AllFriendlyUnits)
            }
            AbilityTargetPolicy::RandomCorpse => {
                let mut best = None;
                for entity in self.world.iter_entities() {
                    let Some(corpse) = entity.get::<Corpse>() else {
                        continue;
                    };
                    if !crate::content::vessel_corpse_qualifies_927(corpse.definition) {
                        continue;
                    }
                    let Some(position) = entity.get::<Position>().map(|position| position.0) else {
                        continue;
                    };
                    if self.ability_source_distance_sq(source.origin, position)
                        > square_i32(spellcasting.ability.range)
                    {
                        continue;
                    }
                    candidate_checks += 1;
                    let id = *entity.get::<SimId>().expect("corpse missing id");
                    let rank = deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        spellcasting.ability.id,
                        state.cast_sequence,
                        id,
                    );
                    let key = (rank, id, position);
                    if best.is_none_or(|current| key < current) {
                        best = Some(key);
                    }
                }
                best.map(|(_, id, position)| AbilityIntentTarget::Corpse { id, position })
            }
            AbilityTargetPolicy::RandomEnemyBasePoint => {
                let enemy_team = 1u8.checked_sub(source.team.0).unwrap();
                let enemy_index = usize::from(enemy_team);
                let region = self.config.team_castle_regions[enemy_index]
                    .or_else(|| self.config.team_build_regions[enemy_index].first().copied());
                let position = if let Some(region) = region {
                    let roll_x = deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        source.id,
                        RANDOM_PURPOSE_ARTILLERY_POINT,
                        state.cast_sequence * 2,
                    );
                    let roll_y = deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        source.id,
                        RANDOM_PURPOSE_ARTILLERY_POINT,
                        state.cast_sequence * 2 + 1,
                    );
                    let cell_size = u64::try_from(self.config.navigation_cell_size)
                        .expect("navigation cell size must be positive");
                    let width = u64::from(region.width)
                        .checked_mul(cell_size)
                        .expect("Artillery target rectangle width overflow");
                    let height = u64::from(region.height)
                        .checked_mul(cell_size)
                        .expect("Artillery target rectangle height overflow");
                    let min_x = i64::from(region.min_x)
                        .checked_mul(i64::from(self.config.navigation_cell_size))
                        .expect("Artillery target rectangle x overflow");
                    let min_y = i64::from(region.min_y)
                        .checked_mul(i64::from(self.config.navigation_cell_size))
                        .expect("Artillery target rectangle y overflow");
                    SimPoint::new(
                        i32::try_from(min_x + i64::try_from(roll_x % width).unwrap())
                            .expect("Artillery target x must fit i32"),
                        i32::try_from(min_y + i64::try_from(roll_y % height).unwrap())
                            .expect("Artillery target y must fit i32"),
                    )
                } else {
                    self.config.team_objective[enemy_index]
                };
                Some(AbilityIntentTarget::Point { position })
            }
        };
        AbilityEvaluation {
            intent: target.map(|target| AbilityIntent {
                source: source.source,
                source_id: source.id,
                target,
                ability: spellcasting.ability,
                cast_sequence: state.cast_sequence,
            }),
            candidate_checks,
        }
    }

    fn random_enemy_ability_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        cast_sequence: u64,
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let (center, query_radius) = match source.origin {
            AbilitySourceOrigin::Unit(position) => (position, ability.range),
            AbilitySourceOrigin::Building(footprint) => (
                footprint_center_point(footprint, self.config.navigation_cell_size),
                building_source_query_radius(
                    footprint,
                    ability.range,
                    self.config.navigation_cell_size,
                ),
            ),
        };
        let range_sq = square_i32(ability.range);
        let mut best: Option<(u64, SimId, usize)> = None;
        grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            center,
            query_radius,
            |unit_index| {
                *candidate_checks += 1;
                let candidate = &units[unit_index];
                if candidate.health <= 0
                    || (ability.target_policy == AbilityTargetPolicy::RandomGroundEnemyUnit
                        && candidate.movement_class != MovementClass::Ground)
                    || self.ability_source_distance_sq(source.origin, candidate.position) > range_sq
                {
                    return;
                }
                let rank = deterministic_ability_target_rank(
                    self.config.match_seed,
                    source.id,
                    ability.id,
                    cast_sequence,
                    candidate.id,
                );
                let key = (rank, candidate.id, unit_index);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );
        best.map(|(_, _, unit_index)| unit_index)
    }

    fn recently_attacked_friendly_ability_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        units: &[UnitSnapshot],
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let previous_tick = self.next_tick.saturating_sub(1);
        let modifier = match ability.effect {
            AbilityEffect::FrostArmor { modifier, .. } => Some(modifier),
            _ => None,
        };
        let range_sq = square_i32(ability.range);
        units
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                *candidate_checks += 1;
                if candidate.health <= 0
                    || candidate.team != source.team
                    || candidate.retaliation.attacked_tick != Some(previous_tick)
                    || self.ability_source_distance_sq(source.origin, candidate.position) > range_sq
                {
                    return None;
                }
                if modifier.is_some_and(|modifier| {
                    candidate.status.armor_modifiers
                        [..usize::from(candidate.status.armor_modifier_count)]
                        .iter()
                        .any(|active| active.id == modifier && self.next_tick < active.expires_tick)
                }) {
                    return None;
                }
                Some((
                    self.ability_source_distance_sq(source.origin, candidate.position),
                    candidate.id,
                    index,
                ))
            })
            .min()
            .map(|(_, _, index)| index)
    }

    fn wounded_friendly_ability_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        units: &[UnitSnapshot],
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let range_sq = square_i32(ability.range);
        units
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                *candidate_checks += 1;
                (candidate.health > 0
                    && candidate.health < candidate.health_max
                    && candidate.team == source.team
                    && self.ability_source_distance_sq(source.origin, candidate.position)
                        <= range_sq)
                    .then_some((candidate.health, candidate.id, index))
            })
            .min()
            .map(|(_, _, index)| index)
    }

    fn random_enemy_ability_target_global(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        cast_sequence: u64,
        units: &[UnitSnapshot],
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let mut best: Option<(u64, SimId, usize)> = None;
        for (unit_index, candidate) in units.iter().enumerate() {
            *candidate_checks += 1;
            if candidate.health <= 0 || candidate.team == source.team {
                continue;
            }
            let rank = deterministic_ability_target_rank(
                self.config.match_seed,
                source.id,
                ability.id,
                cast_sequence,
                candidate.id,
            );
            let key = (rank, candidate.id, unit_index);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, unit_index)| unit_index)
    }

    fn ability_source_distance_sq(&self, source: AbilitySourceOrigin, target: SimPoint) -> u64 {
        match source {
            AbilitySourceOrigin::Unit(position) => position.distance_sq(target),
            AbilitySourceOrigin::Building(footprint) => {
                point_to_footprint_distance_sq(target, footprint, self.config.navigation_cell_size)
            }
        }
    }

    fn ability_target_is_valid(
        &self,
        source: AbilitySourceSnapshot,
        target: AbilityIntentTarget,
        ability: AutomaticAbilityProfile,
        units: &[UnitSnapshot],
    ) -> bool {
        match target {
            AbilityIntentTarget::Unit { index, id } => {
                let target = &units[index];
                target.id == id
                    && target.health > 0
                    && match ability.target_policy {
                        AbilityTargetPolicy::RandomEnemyUnit => {
                            target.team != source.team
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                        }
                        AbilityTargetPolicy::RandomGroundEnemyUnit => {
                            target.team != source.team
                                && target.movement_class == MovementClass::Ground
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                        }
                        AbilityTargetPolicy::RandomEnemyUnitGlobal => target.team != source.team,
                        AbilityTargetPolicy::AllEnemyUnits => false,
                        AbilityTargetPolicy::AllFriendlyUnits
                        | AbilityTargetPolicy::RandomCorpse
                        | AbilityTargetPolicy::RandomEnemyBasePoint => false,
                        AbilityTargetPolicy::RecentlyAttackedFriendlyUnit => {
                            target.team == source.team
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                                && target.retaliation.attacked_tick
                                    == Some(self.next_tick.saturating_sub(1))
                                && match ability.effect {
                                    AbilityEffect::FrostArmor { modifier, .. } => {
                                        !target.status.armor_modifiers
                                            [..usize::from(target.status.armor_modifier_count)]
                                            .iter()
                                            .any(|active| {
                                                active.id == modifier
                                                    && self.next_tick < active.expires_tick
                                            })
                                    }
                                    _ => true,
                                }
                        }
                        AbilityTargetPolicy::WoundedFriendlyUnit => {
                            target.team == source.team
                                && target.health < target.health_max
                                && self.ability_source_distance_sq(source.origin, target.position)
                                    <= square_i32(ability.range)
                        }
                    }
            }
            AbilityIntentTarget::AllEnemyUnits => {
                ability.target_policy == AbilityTargetPolicy::AllEnemyUnits
                    && units
                        .iter()
                        .any(|target| target.health > 0 && target.team != source.team)
            }
            AbilityIntentTarget::AllFriendlyUnits => {
                ability.target_policy == AbilityTargetPolicy::AllFriendlyUnits
                    && units.iter().any(|target| {
                        target.health > 0
                            && target.team == source.team
                            && self.ability_source_distance_sq(source.origin, target.position)
                                <= square_i32(ability.range)
                    })
            }
            AbilityIntentTarget::Corpse { id, position } => {
                ability.target_policy == AbilityTargetPolicy::RandomCorpse
                    && self.world.iter_entities().any(|entity| {
                        entity.get::<SimId>() == Some(&id)
                            && entity.get::<Corpse>().is_some_and(|corpse| {
                                crate::content::vessel_corpse_qualifies_927(corpse.definition)
                            })
                            && entity
                                .get::<Position>()
                                .is_some_and(|actual| actual.0 == position)
                    })
            }
            AbilityIntentTarget::Point { .. } => {
                ability.target_policy == AbilityTargetPolicy::RandomEnemyBasePoint
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resurrection_uses_friendly_corpse_template_and_survives_snapshot_restore() {
        let config = SimulationConfig::default();
        let mut sim = Simulation::new(config.clone(), 1);
        let template = crate::components::UnitTemplate {
            health: 80,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 7,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 5 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let definition = ResolvedUnitDefinition {
            template,
            properties: UnitGameplayProperties {
                attack_targets: AttackTargetMask::GROUND_UNITS,
                ..UnitGameplayProperties::default()
            },
            spellcasting: None,
        };
        let position = SimPoint::new(100 * SUBUNITS_PER_WORLD_UNIT, 100 * SUBUNITS_PER_WORLD_UNIT);
        for team in [Team(0), Team(1)] {
            let id = sim.allocate_id();
            sim.world.spawn((
                id,
                Position(position),
                Corpse {
                    source_unit: id,
                    source_owner: PlayerId(team.0),
                    source_team: team,
                    definition: CorpseDefinitionId(1),
                    created_tick: 0,
                    expires_tick: None,
                    resurrection: Some(definition),
                },
            ));
        }
        let snapshot = sim.capture_snapshot();
        let mut restored = Simulation::new(config, 1);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(restored.checksum(), sim.checksum());

        for candidate in [&mut sim, &mut restored] {
            assert_eq!(
                candidate.resurrect_friendly_corpses(
                    Team(0),
                    AbilitySourceOrigin::Unit(position),
                    10 * SUBUNITS_PER_WORLD_UNIT,
                    1,
                ),
                1,
            );
            assert_eq!(candidate.corpse_count(), 1);
            assert_eq!(candidate.unit_count(), 1);
        }
        assert_eq!(restored.checksum(), sim.checksum());
    }
}
