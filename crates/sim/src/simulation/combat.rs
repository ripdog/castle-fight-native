use super::*;

pub(super) struct AttackResolutionContext<'a> {
    pub(super) units: &'a mut [UnitSnapshot],
    pub(super) buildings: &'a [BuildingSnapshot],
    pub(super) unit_health: &'a mut [i32],
    pub(super) building_health: &'a mut [i32],
    pub(super) cooldowns: &'a mut [u16],
    pub(super) attack_sequences: &'a mut [u64],
    pub(super) building_cooldowns: &'a mut [u16],
    pub(super) positions: &'a [SimPoint],
    pub(super) attackers_this_tick: &'a mut [Option<SimId>],
    pub(super) next_defense_alerts: &'a mut Vec<DefenseAlert>,
    pub(super) completed_tick: u64,
}

pub(super) struct AttackResolution {
    pub(super) attacks_resolved: usize,
    pub(super) native_barrage_launches: usize,
    pub(super) projectile_launches: Vec<ProjectileLaunch>,
    pub(super) line_projectile_launches: Vec<LineProjectile>,
    pub(super) ballistic_projectile_launches: Vec<BallisticProjectileLaunch>,
    pub(super) bounce_projectile_launches: Vec<BounceProjectileLaunch>,
    pub(super) chain_lightning_launches: Vec<ChainLightningState>,
}

impl Simulation {
    pub(super) fn resolve_attacks(
        &mut self,
        context: AttackResolutionContext<'_>,
    ) -> AttackResolution {
        let AttackResolutionContext {
            units,
            buildings,
            unit_health,
            building_health,
            cooldowns,
            attack_sequences,
            building_cooldowns,
            positions,
            attackers_this_tick,
            next_defense_alerts,
            completed_tick,
        } = context;
        let mut line_projectile_launches = Vec::new();
        let mut intents = self.attack_intents(units, buildings);
        intents.sort_unstable_by_key(|intent| (intent.source_id, intent.target_id));
        for unit in units.iter_mut() {
            let Some(pending) = unit.status.pending_attack else {
                continue;
            };
            let cancelled = unit.status.is_stunned(completed_tick)
                || unit.attacks_disabled
                || unit.orders_suspended
                || unit.target != Some(pending.target)
                || (completed_tick >= pending.release_tick
                    && intents
                        .binary_search_by_key(&unit.id, |intent| intent.source_id)
                        .is_err());
            if cancelled {
                unit.status.pending_attack = None;
                unit.status.action_animation = None;
            }
        }

        let mut attacks_resolved = 0;
        let mut native_barrage_launches = 0;
        let mut projectile_launches = Vec::new();
        let mut ballistic_projectile_launches = Vec::new();
        let mut bounce_projectile_launches = Vec::new();
        let mut chain_lightning_launches = Vec::new();
        for intent in intents {
            let source_alive = match intent.source {
                AttackSourceIndex::Unit(index) => unit_health[index] > 0,
                AttackSourceIndex::Building(index) => building_health[index] > 0,
            };
            if !source_alive {
                continue;
            }
            let Some(target_position) = live_target_position(
                intent.target,
                units,
                buildings,
                unit_health,
                building_health,
                self.config.navigation_cell_size,
            ) else {
                if let AttackSourceIndex::Unit(index) = intent.source
                    && intent.completing_windup
                {
                    units[index].status.pending_attack = None;
                    units[index].status.action_animation = None;
                }
                continue;
            };

            if let AttackSourceIndex::Unit(index) = intent.source {
                if intent.completing_windup {
                    units[index].status.pending_attack = None;
                } else {
                    let primary = match intent.target {
                        TargetIndex::Unit(target) => units[index]
                            .primary_attack_targets()
                            .can_target_unit(units[target].movement_class),
                        TargetIndex::Building(_) => {
                            units[index].primary_attack_targets().can_target_buildings()
                        }
                    };
                    let timing = units[index].action_timing;
                    let (animation_ticks, point_ticks) = if primary {
                        (
                            timing.primary_attack_ticks,
                            timing.primary_attack_point_ticks,
                        )
                    } else {
                        (
                            timing.secondary_attack_ticks,
                            timing.secondary_attack_point_ticks,
                        )
                    };
                    let cooldown = effective_attack_cooldown_ticks(
                        intent.attack.cooldown_ticks,
                        units[index].status,
                    );
                    let windup = if point_ticks == 0 {
                        0
                    } else {
                        effective_attack_cooldown_ticks(point_ticks, units[index].status)
                            .min(cooldown.saturating_sub(1))
                    };
                    let duration = if animation_ticks == 0 {
                        0
                    } else {
                        effective_attack_cooldown_ticks(animation_ticks, units[index].status)
                            .min(cooldown)
                    }
                    .max(if windup > 0 { windup + 1 } else { 0 });
                    units[index].status.begin_action_animation(
                        ActionAnimationKind::Attack,
                        completed_tick,
                        duration,
                    );
                    // Windup is part of the existing cycle, never added on top of the cooldown.
                    cooldowns[index] = cooldown;
                    if windup > 0 {
                        units[index].status.pending_attack = Some(crate::PendingAttackState {
                            target: intent.target_id,
                            release_tick: completed_tick
                                .checked_add(u64::from(windup))
                                .expect("attack release tick overflow"),
                        });
                        continue;
                    }
                }
            }

            let missed = self.uphill_attack_misses(&intent, target_position, completed_tick, units)
                || self.attack_is_evaded(&intent, units, completed_tick);
            let mut critical = false;
            if !missed {
                let (bonus_damage, on_hit, critical_strike) =
                    self.resolve_passive_attack_effects(&intent, units, completed_tick);
                critical = critical_strike;
                let damage = intent
                    .attack
                    .damage
                    .checked_add(bonus_damage)
                    .expect("attack plus passive bonus damage overflowed");
                match intent.attack.delivery {
                    AttackDelivery::Melee | AttackDelivery::RangedInstant => {
                        let applied = apply_damage_to_target(
                            intent.target,
                            intent.source_id,
                            damage,
                            intent.damage_type,
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
                        );
                        debug_assert!(applied.is_some());
                        if matches!(intent.attack.delivery, AttackDelivery::Melee) {
                            for effect in intent.passive_effects.iter() {
                                let PassiveUnitEffect::Cleave(profile) = effect else {
                                    continue;
                                };
                                let splash_damage = i32::try_from(
                                    i64::from(damage) * i64::from(profile.damage_per_10k) / 10_000,
                                )
                                .expect("cleave damage exceeds i32");
                                let target_count = units.len()
                                    + if profile.targets.can_target_buildings() {
                                        buildings.len()
                                    } else {
                                        0
                                    };
                                for index in 0..target_count {
                                    let target = if index < units.len() {
                                        if units[index].team == intent.source_team
                                            || !profile
                                                .targets
                                                .can_target_unit(units[index].movement_class)
                                            || unit_health[index] <= 0
                                            || target_position.distance_sq(positions[index])
                                                > square_i32(profile.radius)
                                        {
                                            continue;
                                        }
                                        TargetIndex::Unit(index)
                                    } else {
                                        let index = index - units.len();
                                        if buildings[index].team == intent.source_team
                                            || building_health[index] <= 0
                                            || point_to_footprint_distance_sq(
                                                target_position,
                                                buildings[index].footprint,
                                                self.config.navigation_cell_size,
                                            ) > square_i32(profile.radius)
                                        {
                                            continue;
                                        }
                                        TargetIndex::Building(index)
                                    };
                                    if target == intent.target {
                                        continue;
                                    }
                                    apply_damage_to_target(
                                        target,
                                        intent.source_id,
                                        splash_damage,
                                        intent.damage_type,
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
                                    );
                                }
                            }
                        }
                        let pending = apply_pending_attack_effects(
                            intent.target,
                            on_hit,
                            PendingAttackEffectSource {
                                id: intent.source_id,
                                position: intent.source_position,
                                team: intent.source_team,
                            },
                            PendingAttackEffectState {
                                completed_tick,
                                units,
                                unit_health,
                                damage_rules: self.combat_rules.damage_rules,
                            },
                        );
                        if let Some(event) = pending.chain_event {
                            self.last_chain_lightnings.push(event);
                        }
                        if let Some(state) = pending.chain_state {
                            chain_lightning_launches.push(state);
                        }
                        if matches!(intent.attack.delivery, AttackDelivery::Melee)
                            && let (
                                AttackSourceIndex::Unit(source_index),
                                TargetIndex::Unit(target_index),
                            ) = (intent.source, intent.target)
                        {
                            apply_melee_reactive_armor_effects(
                                source_index,
                                target_index,
                                completed_tick,
                                units,
                                unit_health,
                            );
                        }
                    }
                    AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        projectile_launches.push(ProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            source_is_building: matches!(
                                intent.source,
                                AttackSourceIndex::Building(_)
                            ),
                            target: intent.target_id,
                            damage,
                            on_hit,
                            damage_type: intent.damage_type,
                            speed_per_tick,
                            launch_position: intent.source_position,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    AttackDelivery::RangedBallistic {
                        speed_per_tick,
                        impact_radius,
                    } => {
                        assert_eq!(
                            (
                                on_hit.stun_duration_ticks,
                                on_hit.triggered_spell,
                                on_hit.feedback
                            ),
                            (0, None, None),
                            "ballistic stun/triggered-spell/Feedback passives are unsupported"
                        );
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        ballistic_projectile_launches.push(BallisticProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            target_mask: intent.attack_targets,
                            damage,
                            burning_oil: on_hit.burning_oil,
                            splash_falloff: on_hit.splash_falloff,
                            damage_type: intent.damage_type,
                            launch_position: intent.source_position,
                            destination: target_position,
                            impact_radius,
                            launch_tick: completed_tick,
                            impact_tick,
                        });
                    }
                    AttackDelivery::Line {
                        speed_per_tick,
                        spill_distance,
                        ..
                    } => {
                        assert_eq!(
                            on_hit,
                            PendingAttackEffects::default(),
                            "line on-hit passives unsupported"
                        );
                        let primary_impact_tick = completed_tick
                            .checked_add(projectile_travel_ticks(
                                intent.source_position.distance_sq(target_position),
                                speed_per_tick,
                            ))
                            .expect("line primary impact tick overflow");
                        let spill_ticks = if spill_distance == 0 {
                            0
                        } else {
                            projectile_travel_ticks(square_i32(spill_distance), speed_per_tick)
                        };
                        let impact_tick = primary_impact_tick
                            .checked_add(spill_ticks)
                            .expect("line final impact tick overflow");
                        let source_entity = match intent.source {
                            AttackSourceIndex::Unit(index) => units[index].entity,
                            AttackSourceIndex::Building(index) => buildings[index].entity,
                        };
                        let source_rawcode = self
                            .world
                            .get::<ContentIdentity>(source_entity)
                            .map(|content| content.rawcode);
                        line_projectile_launches.push(LineProjectile {
                            source: intent.source_id,
                            source_rawcode,
                            source_team: intent.source_team,
                            target: intent.target_id,
                            damage,
                            damage_type: intent.damage_type,
                            delivery: intent.attack.delivery,
                            launch_position: intent.source_position,
                            launch_tick: completed_tick,
                            primary_impact_tick,
                            impact_tick,
                            spill_origin: None,
                            destination: target_position,
                            hit_targets: Vec::new(),
                        });
                    }
                    AttackDelivery::Bounce {
                        speed_per_tick,
                        bounce_range,
                        max_bounces,
                        damage_percent_per_bounce,
                        allow_repeat_targets,
                    } => {
                        assert_eq!(
                            (bonus_damage, on_hit),
                            (0, PendingAttackEffects::default()),
                            "passive on-hit effects are not yet defined for bounce attacks"
                        );
                        let travel_ticks =
                            projectile_travel_ticks(intent.distance_sq, speed_per_tick);
                        let impact_tick = completed_tick
                            .checked_add(travel_ticks)
                            .expect("projectile impact tick overflow");
                        bounce_projectile_launches.push(BounceProjectileLaunch {
                            source: intent.source_id,
                            source_team: intent.source_team,
                            source_is_building: matches!(
                                intent.source,
                                AttackSourceIndex::Building(_)
                            ),
                            target_mask: intent.attack_targets,
                            target: intent.target_id,
                            damage: intent.attack.damage,
                            damage_type: intent.damage_type,
                            launch_position: intent.source_position,
                            launch_tick: completed_tick,
                            impact_tick,
                            speed_per_tick,
                            bounce_range,
                            max_bounces,
                            damage_percent_per_bounce,
                            allow_repeat_targets,
                        });
                    }
                }
            }
            // Native critical strikes suppress Barrage. A primary miss is not a
            // critical strike and does not collapse independent secondary arrows.
            if !critical {
                native_barrage_launches += self.launch_native_barrage(
                    &intent,
                    units,
                    buildings,
                    unit_health,
                    completed_tick,
                );
            }
            match intent.source {
                AttackSourceIndex::Unit(index) => {
                    let elapsed = if intent.completing_windup {
                        units[index].status.action_animation.map_or(0, |action| {
                            completed_tick.saturating_sub(action.started_tick)
                        })
                    } else {
                        0
                    };
                    cooldowns[index] = effective_attack_cooldown_ticks(
                        intent.attack.cooldown_ticks,
                        units[index].status,
                    )
                    .saturating_sub(u16::try_from(elapsed).unwrap_or(u16::MAX));
                    attack_sequences[index] = attack_sequences[index]
                        .checked_add(1)
                        .expect("unit attack sequence overflow");
                }
                AttackSourceIndex::Building(index) => {
                    building_cooldowns[index] = intent.attack.cooldown_ticks;
                }
            }
            self.last_attacks.push(AttackEvent {
                source: intent.source_id,
                target: intent.target_id,
                source_position: intent.source_position,
                target_position,
                delivery: intent.attack.delivery,
                missed,
                critical,
            });
            attacks_resolved += 1;
        }

        AttackResolution {
            attacks_resolved,
            line_projectile_launches,
            native_barrage_launches,
            projectile_launches,
            ballistic_projectile_launches,
            bounce_projectile_launches,
            chain_lightning_launches,
        }
    }

    pub(super) fn uphill_attack_misses(
        &self,
        intent: &AttackIntent,
        target_position: SimPoint,
        completed_tick: u64,
        units: &[UnitSnapshot],
    ) -> bool {
        let chance = self.combat_rules.uphill_miss_chance_per_10k;
        let (AttackSourceIndex::Unit(source_index), TargetIndex::Unit(target_index)) =
            (intent.source, intent.target)
        else {
            return false;
        };
        if chance == 0
            || units[source_index].movement_class == MovementClass::Air
            || units[target_index].movement_class == MovementClass::Air
        {
            return false;
        }

        let terrain = self
            .combat_rules
            .terrain_elevation
            .as_ref()
            .expect("uphill miss chance requires authoritative terrain elevation");
        let source_level = terrain
            .cliff_level_at(intent.source_position)
            .expect("attacking unit position lies outside authoritative terrain elevation");
        let target_level = terrain
            .cliff_level_at(target_position)
            .expect("target unit position lies outside authoritative terrain elevation");
        if target_level <= source_level {
            return false;
        }

        deterministic_random(
            self.config.match_seed,
            completed_tick,
            intent.source_id,
            RANDOM_PURPOSE_UPHILL_MISS,
            intent.attack_sequence,
        ) % u64::from(UPHILL_MISS_CHANCE_SCALE)
            < u64::from(chance)
    }

    pub(super) fn attack_is_evaded(
        &self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        completed_tick: u64,
    ) -> bool {
        let TargetIndex::Unit(target_index) = intent.target else {
            return false;
        };
        let target = &units[target_index];
        // Native Evasion does not independently roll every copy. Only the highest
        // chance is effective; ties use rawcode identity, not inventory order.
        let Some(profile) = target
            .passive_effects
            .iter()
            .filter_map(|effect| {
                if let PassiveUnitEffect::Evasion(profile) = effect {
                    Some(profile)
                } else {
                    None
                }
            })
            .max_by_key(|profile| (profile.chance_per_10k, std::cmp::Reverse(profile.ability)))
        else {
            return false;
        };
        let roll = deterministic_random(
            self.config.match_seed,
            completed_tick,
            intent.source_id,
            RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0) ^ target.id.0.rotate_left(13),
            intent.attack_sequence,
        ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
        roll < u64::from(profile.chance_per_10k)
    }

    pub(super) fn resolve_passive_attack_effects(
        &self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        completed_tick: u64,
    ) -> (i32, PendingAttackEffects, bool) {
        let mut bonus_damage = 0i32;
        let mut on_hit = PendingAttackEffects::default();
        let mut critical = false;
        let mut critical_bonus = 0;
        for effect in intent.passive_effects.iter() {
            match effect {
                PassiveUnitEffect::CriticalStrike(profile) => {
                    let target_matches = match intent.target {
                        TargetIndex::Unit(index) => {
                            profile.targets.can_target_unit(units[index].movement_class)
                        }
                        TargetIndex::Building(_) => profile.targets.can_target_buildings(),
                    };
                    if profile.chance_per_10k == 0 || !target_matches {
                        continue;
                    }
                    let roll = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        intent.source_id,
                        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                        intent.attack_sequence,
                    ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
                    if roll < u64::from(profile.chance_per_10k) {
                        critical = true;
                        let extra = i64::from(intent.attack.damage)
                            * i64::from(profile.damage_multiplier_per_10k - 10_000)
                            / 10_000;
                        // Multiple native critical multipliers roll separately, but
                        // only the highest successful multiplier contributes damage.
                        critical_bonus = critical_bonus
                            .max(i32::try_from(extra).expect("critical bonus overflowed"));
                    }
                }
                PassiveUnitEffect::Bash(profile) => {
                    let target_matches = match intent.target {
                        TargetIndex::Unit(index) => {
                            profile.targets.can_target_unit(units[index].movement_class)
                        }
                        TargetIndex::Building(_) => profile.targets.can_target_buildings(),
                    };
                    if !target_matches || profile.chance_per_10k == 0 {
                        continue;
                    }
                    let roll = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        intent.source_id,
                        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                        intent.attack_sequence,
                    ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
                    if roll >= u64::from(profile.chance_per_10k) {
                        continue;
                    }
                    bonus_damage = bonus_damage
                        .checked_add(profile.bonus_damage)
                        .expect("passive attack bonus damage overflowed");
                    let duration = match intent.target {
                        TargetIndex::Unit(index) if units[index].classifications.hero => {
                            profile.hero_stun_duration_ticks
                        }
                        _ => profile.stun_duration_ticks,
                    };
                    on_hit.stun_duration_ticks = on_hit.stun_duration_ticks.max(duration);
                }
                PassiveUnitEffect::TriggeredSpellProc(profile) => {
                    let target_matches = match intent.target {
                        TargetIndex::Unit(index) => {
                            profile.targets.can_target_unit(units[index].movement_class)
                        }
                        TargetIndex::Building(_) => profile.targets.can_target_buildings(),
                    };
                    if !target_matches || profile.chance_per_10k == 0 {
                        continue;
                    }
                    let roll = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        intent.source_id,
                        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                        intent.attack_sequence,
                    ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
                    if roll < u64::from(profile.chance_per_10k) {
                        assert!(
                            on_hit.triggered_spell.is_none(),
                            "multiple triggered spell procs on one attack are not yet supported"
                        );
                        on_hit.triggered_spell = Some(profile.effect);
                    }
                }
                PassiveUnitEffect::FrostAttack(profile) => {
                    assert!(
                        on_hit.frost.replace(profile).is_none(),
                        "multiple Frost Attack effects"
                    );
                }
                PassiveUnitEffect::Feedback(profile) => {
                    let TargetIndex::Unit(index) = intent.target else {
                        continue;
                    };
                    if profile.targets.can_target_unit(units[index].movement_class) {
                        assert!(
                            on_hit.feedback.replace(profile).is_none(),
                            "multiple Feedback payloads are unsupported"
                        );
                    }
                }
                PassiveUnitEffect::BurningOil(profile) => {
                    assert!(
                        on_hit.burning_oil.is_none(),
                        "multiple Burning Oil effects on one attack are not supported"
                    );
                    on_hit.burning_oil = Some(profile);
                }
                PassiveUnitEffect::SplashFalloff(profile) => {
                    assert!(
                        on_hit.splash_falloff.is_none(),
                        "multiple splash profiles on one attack"
                    );
                    on_hit.splash_falloff = Some(profile);
                }
                PassiveUnitEffect::Evasion(_)
                | PassiveUnitEffect::Defend(_)
                | PassiveUnitEffect::Cleave(_)
                | PassiveUnitEffect::Aura(_)
                | PassiveUnitEffect::SpellResistance(_) => {}
            }
        }
        (
            bonus_damage
                .checked_add(critical_bonus)
                .expect("passive attack damage overflowed"),
            on_hit,
            critical,
        )
    }

    pub(super) fn attack_intents(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Vec<AttackIntent> {
        let mut unit_intents: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    if source.spawn_tick == self.next_tick
                        || source.status.is_stunned(self.next_tick)
                        || source.attacks_disabled
                        || source.orders_suspended
                    {
                        return None;
                    }
                    let completing_windup = source.status.pending_attack.is_some();
                    let target_id = if let Some(pending) = source.status.pending_attack {
                        if self.next_tick < pending.release_tick
                            || source.target != Some(pending.target)
                        {
                            return None;
                        }
                        pending.target
                    } else {
                        if source.cooldown_remaining != 0
                            || source.status.is_performing_action(self.next_tick)
                        {
                            return None;
                        }
                        source.target?
                    };
                    let (target, distance_sq, attack, attack_targets, damage_type) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            let (attack, targets, damage_type) =
                                source.attack_for_unit(units[index].movement_class)?;
                            (
                                TargetIndex::Unit(index),
                                source.position.distance_sq(units[index].position),
                                attack,
                                targets,
                                damage_type,
                            )
                        } else {
                            let (attack, targets, damage_type) = source.attack_for_building()?;
                            let index = find_building_index(buildings, target_id)?;
                            (
                                TargetIndex::Building(index),
                                point_to_footprint_distance_sq(
                                    source.position,
                                    buildings[index].footprint,
                                    self.config.navigation_cell_size,
                                ),
                                attack,
                                targets,
                                damage_type,
                            )
                        };
                    if !attack.in_range(distance_sq) {
                        return None;
                    }
                    let mut attack = attack;
                    let damage_bonus_per_10k: u32 = source.status.armor_modifiers
                        [..usize::from(source.status.armor_modifier_count)]
                        .iter()
                        .filter(|modifier| self.next_tick < modifier.expires_tick)
                        .map(|modifier| u32::from(modifier.damage_bonus_per_10k))
                        .sum();
                    attack.damage = attack.damage.saturating_add(
                        i32::try_from(
                            i64::from(attack.damage) * i64::from(damage_bonus_per_10k) / 10_000,
                        )
                        .expect("buffed attack damage exceeds i32"),
                    );
                    Some(AttackIntent {
                        source: AttackSourceIndex::Unit(source_index),
                        target,
                        source_id: source.id,
                        source_team: source.team,
                        source_position: source.position,
                        target_id,
                        attack,
                        attack_targets,
                        damage_type,
                        passive_effects: source.passive_effects,
                        attack_sequence: source.attack_sequence,
                        distance_sq,
                        completing_windup,
                    })
                })
                .collect()
        });
        let mut building_intents: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .enumerate()
                .filter_map(|(source_index, source)| {
                    let attack = source.attack?;
                    if source.spawn_tick == Some(self.next_tick)
                        || source.cooldown_remaining.unwrap_or(0) != 0
                        || source
                            .status
                            .is_some_and(|status| self.next_tick < status.stunned_until_tick)
                    {
                        return None;
                    }
                    let target_id = source.target?;
                    let attack_targets = source
                        .attack_targets
                        .expect("attack building target mask missing");
                    let (target, distance_sq) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            if !attack_targets.can_target_unit(units[index].movement_class) {
                                return None;
                            }
                            (
                                TargetIndex::Unit(index),
                                point_to_footprint_distance_sq(
                                    units[index].position,
                                    source.footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        } else {
                            if !attack_targets.can_target_buildings() {
                                return None;
                            }
                            let index = find_building_index(buildings, target_id)?;
                            (
                                TargetIndex::Building(index),
                                footprint_to_footprint_distance_sq(
                                    source.footprint,
                                    buildings[index].footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        };
                    if !attack.in_range(distance_sq) {
                        return None;
                    }
                    Some(AttackIntent {
                        source: AttackSourceIndex::Building(source_index),
                        target,
                        source_id: source.id,
                        source_team: source.team,
                        source_position: footprint_center_point(
                            source.footprint,
                            self.config.navigation_cell_size,
                        ),
                        target_id,
                        attack,
                        attack_targets: source
                            .attack_targets
                            .expect("attack building target mask missing"),
                        damage_type: source.damage_type,
                        passive_effects: PassiveUnitEffects::EMPTY,
                        attack_sequence: 0,
                        distance_sq,
                        completing_windup: false,
                    })
                })
                .collect()
        });
        let multishot_rawcode = crate::content::CastleFightTowerKind::TinyMultishotTower
            .definition()
            .rawcode;
        let mut multishot_intents = Vec::new();
        for intent in &building_intents {
            let AttackSourceIndex::Building(source_index) = intent.source else {
                continue;
            };
            let source = &buildings[source_index];
            if self
                .world
                .entity(source.entity)
                .get::<ContentIdentity>()
                .map(|identity| identity.rawcode)
                != Some(multishot_rawcode)
            {
                continue;
            }
            let mut candidates = units
                .iter()
                .enumerate()
                .filter_map(|(index, target)| {
                    if target.health <= 0
                        || target.team == source.team
                        || target.id == intent.target_id
                        || !intent.attack_targets.can_target_unit(target.movement_class)
                    {
                        return None;
                    }
                    let distance_sq = point_to_footprint_distance_sq(
                        target.position,
                        source.footprint,
                        self.config.navigation_cell_size,
                    );
                    intent
                        .attack
                        .in_range(distance_sq)
                        .then_some((distance_sq, target.id, index))
                })
                .collect::<Vec<_>>();
            candidates.sort_unstable_by_key(|(distance, id, _)| (*distance, *id));
            for (distance_sq, target_id, index) in candidates.into_iter().take(2) {
                multishot_intents.push(AttackIntent {
                    target: TargetIndex::Unit(index),
                    target_id,
                    distance_sq,
                    ..*intent
                });
            }
        }
        building_intents.extend(multishot_intents);
        unit_intents.append(&mut building_intents);
        unit_intents
    }
}
