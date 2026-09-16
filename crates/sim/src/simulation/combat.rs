use super::*;

impl Simulation {
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
        for effect in target.passive_effects.iter() {
            let PassiveUnitEffect::Evasion(profile) = effect else {
                continue;
            };
            if profile.chance_per_10k == 0 {
                continue;
            }
            let roll = deterministic_random(
                self.config.match_seed,
                completed_tick,
                intent.source_id,
                RANDOM_PURPOSE_ATTACK_PROC
                    ^ u64::from(profile.ability.0)
                    ^ target.id.0.rotate_left(13),
                intent.attack_sequence,
            ) % u64::from(ATTACK_PROC_CHANCE_SCALE);
            if roll < u64::from(profile.chance_per_10k) {
                return true;
            }
        }
        false
    }

    pub(super) fn resolve_passive_attack_effects(
        &self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        completed_tick: u64,
    ) -> (i32, PendingAttackEffects) {
        let mut bonus_damage = 0i32;
        let mut on_hit = PendingAttackEffects::default();
        for effect in intent.passive_effects.iter() {
            match effect {
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
                    on_hit.stun_duration_ticks =
                        on_hit.stun_duration_ticks.max(profile.stun_duration_ticks);
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
                PassiveUnitEffect::BurningOil(profile) => {
                    assert!(
                        on_hit.burning_oil.is_none(),
                        "multiple Burning Oil effects on one attack are not supported"
                    );
                    on_hit.burning_oil = Some(profile);
                }
                PassiveUnitEffect::Evasion(_) | PassiveUnitEffect::Defend(_) => {}
            }
        }
        (bonus_damage, on_hit)
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
                        || source.cooldown_remaining != 0
                        || self.next_tick < source.status.stunned_until_tick
                    {
                        return None;
                    }
                    let target_id = source.target?;
                    let (target, distance_sq) =
                        if let Some(index) = find_unit_index(units, target_id) {
                            if !source
                                .attack_targets
                                .can_target_unit(units[index].movement_class)
                            {
                                return None;
                            }
                            (
                                TargetIndex::Unit(index),
                                source.position.distance_sq(units[index].position),
                            )
                        } else {
                            if !source.attack_targets.can_target_buildings() {
                                return None;
                            }
                            let index = find_building_index(buildings, target_id)?;
                            (
                                TargetIndex::Building(index),
                                point_to_footprint_distance_sq(
                                    source.position,
                                    buildings[index].footprint,
                                    self.config.navigation_cell_size,
                                ),
                            )
                        };
                    if distance_sq > source.attack.range_sq() {
                        return None;
                    }
                    Some(AttackIntent {
                        source: AttackSourceIndex::Unit(source_index),
                        target,
                        source_id: source.id,
                        source_team: source.team,
                        source_position: source.position,
                        target_id,
                        attack: source.attack,
                        attack_targets: source.attack_targets,
                        damage_type: source.damage_type,
                        passive_effects: source.passive_effects,
                        attack_sequence: source.attack_sequence,
                        distance_sq,
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
                    if distance_sq > attack.range_sq() {
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
                    })
                })
                .collect()
        });
        unit_intents.append(&mut building_intents);
        unit_intents
    }
}
