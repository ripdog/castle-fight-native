use super::*;

impl Simulation {
    pub(super) fn unit_will_query_ally_defense(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if unit.health <= 0 || self.next_tick < unit.status.stunned_until_tick {
            return false;
        }
        let current = unit
            .target
            .filter(|target| self.current_target_retainable_for(unit, *target, units, buildings));
        if current.is_some_and(|target| find_unit_index(units, target).is_some()) {
            return false;
        }
        !self
            .recent_retaliation_target(unit, units, buildings)
            .is_some_and(|attacker| find_unit_index(units, attacker).is_some())
    }

    pub(super) fn select_targets(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
        defense_attacker_grid: &SpatialGrid,
    ) -> TargetSelectionResult {
        let decisions: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .map(|unit| {
                    if unit.health <= 0 {
                        return TargetDecision::without_defense(None, false, false);
                    }
                    let current = unit.target.filter(|target| {
                        self.current_target_retainable_for(unit, *target, units, buildings)
                    });
                    if self.next_tick < unit.status.stunned_until_tick {
                        return TargetDecision::without_defense(
                            current,
                            current.is_some() && unit.direct_retaliation_lock,
                            current.is_some() && unit.ally_defense_lock,
                        );
                    }

                    let retaliation = self.recent_retaliation_target(unit, units, buildings);
                    if let Some(current) = current
                        && find_unit_index(units, current).is_some()
                    {
                        if unit.direct_retaliation_lock {
                            return TargetDecision::without_defense(Some(current), true, false);
                        }
                        if let Some(attacker) = retaliation
                            && find_unit_index(units, attacker).is_some()
                        {
                            return TargetDecision::without_defense(Some(attacker), true, false);
                        }
                        return TargetDecision::without_defense(
                            Some(current),
                            false,
                            unit.ally_defense_lock,
                        );
                    }

                    if let Some(attacker) = retaliation
                        && find_unit_index(units, attacker).is_some()
                    {
                        return TargetDecision::without_defense(Some(attacker), true, false);
                    }

                    let defense = self.recent_ally_defense_target(
                        unit,
                        units,
                        buildings,
                        defense_attacker_grid,
                        defense_victims,
                        alert_grid,
                    );
                    if let Some(attacker) = defense.target
                        && find_unit_index(units, attacker).is_some()
                    {
                        return TargetDecision::with_defense(Some(attacker), false, true, defense);
                    }

                    if let Some(current) = current {
                        debug_assert!(find_building_index(buildings, current).is_some());
                        if unit.direct_retaliation_lock {
                            return TargetDecision::with_defense(
                                Some(current),
                                true,
                                false,
                                defense,
                            );
                        }
                        if let Some(attacker) = retaliation {
                            return TargetDecision::with_defense(
                                Some(attacker),
                                true,
                                false,
                                defense,
                            );
                        }
                        if unit.ally_defense_lock {
                            return TargetDecision::with_defense(
                                Some(current),
                                false,
                                true,
                                defense,
                            );
                        }
                        if let Some(attacker) = defense.target {
                            return TargetDecision::with_defense(
                                Some(attacker),
                                false,
                                true,
                                defense,
                            );
                        }
                        return TargetDecision::with_defense(Some(current), false, false, defense);
                    }

                    if let Some(attacker) = retaliation {
                        return TargetDecision::with_defense(Some(attacker), true, false, defense);
                    }
                    if let Some(attacker) = defense.target {
                        return TargetDecision::with_defense(Some(attacker), false, true, defense);
                    }

                    TargetDecision::with_defense(
                        self.acquire_target(unit, units, buildings, grid),
                        false,
                        false,
                        defense,
                    )
                })
                .collect()
        });

        let retained_targets = units
            .iter()
            .zip(&decisions)
            .filter(|(unit, decision)| unit.target.is_some() && unit.target == decision.target)
            .count();
        let target_changes = units
            .iter()
            .zip(&decisions)
            .filter(|(unit, decision)| unit.target != decision.target)
            .count();
        TargetSelectionResult {
            ally_defense_queries: decisions
                .iter()
                .filter(|decision| decision.defense_query.queried)
                .count(),
            ally_defense_victim_candidates: decisions
                .iter()
                .map(|decision| decision.defense_query.victim_candidates)
                .sum(),
            ally_defense_attacker_candidates: decisions
                .iter()
                .map(|decision| decision.defense_query.attacker_candidates)
                .sum(),
            decisions,
            retained_targets,
            target_changes,
        }
    }

    pub(super) fn select_building_targets(
        &self,
        buildings: &[BuildingSnapshot],
        units: &[UnitSnapshot],
        grid: &SpatialGrid,
    ) -> BuildingTargetSelectionResult {
        let decisions: Vec<_> = self.pool.install(|| {
            buildings
                .par_iter()
                .map(|building| {
                    let _attack = building.attack?;
                    if building.health <= 0 {
                        return None;
                    }
                    let current = building.target.filter(|target| {
                        self.building_source_target_retainable(building, *target, units, buildings)
                    });
                    if building
                        .status
                        .is_some_and(|status| self.next_tick < status.stunned_until_tick)
                    {
                        return current;
                    }
                    if current.is_some() {
                        return current;
                    }
                    self.acquire_building_target(building, units, buildings, grid)
                })
                .collect()
        });
        let retained_targets = buildings
            .iter()
            .zip(&decisions)
            .filter(|(building, decision)| {
                building.attack.is_some()
                    && building.target.is_some()
                    && building.target == **decision
            })
            .count();
        let target_changes = buildings
            .iter()
            .zip(&decisions)
            .filter(|(building, decision)| {
                building.attack.is_some() && building.target != **decision
            })
            .count();
        BuildingTargetSelectionResult {
            decisions,
            retained_targets,
            target_changes,
        }
    }
}
