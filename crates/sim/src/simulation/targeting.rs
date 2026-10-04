use super::*;

impl Simulation {
    pub(super) fn unit_will_query_ally_defense(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if unit.health <= 0
            || unit.attacks_disabled
            || unit.orders_suspended
            || self.next_tick < unit.status.stunned_until_tick
        {
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
                    if unit.health <= 0 || unit.attacks_disabled || unit.orders_suspended {
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

    fn acquire_building_target(
        &self,
        source: &BuildingSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
    ) -> Option<SimId> {
        let attack = source.attack?;
        let attack_targets = source
            .attack_targets
            .expect("attack building target mask missing");
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let center = footprint_center_point(source.footprint, self.config.navigation_cell_size);
        let query_radius = building_source_query_radius(
            source.footprint,
            attack.acquisition_range,
            self.config.navigation_cell_size,
        );
        let acquisition_range_sq = attack.acquisition_range_sq();
        let mut best_unit: Option<(u64, SimId)> = None;
        grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            center,
            query_radius,
            |index| {
                let candidate = &units[index];
                if candidate.health <= 0
                    || !attack_targets.can_target_unit(candidate.movement_class)
                {
                    return;
                }
                let distance_sq = point_to_footprint_distance_sq(
                    candidate.position,
                    source.footprint,
                    self.config.navigation_cell_size,
                );
                if distance_sq > acquisition_range_sq {
                    return;
                }
                let key = (distance_sq, candidate.id);
                if best_unit.is_none_or(|current| key < current) {
                    best_unit = Some(key);
                }
            },
        );
        if let Some((_, target)) = best_unit {
            return Some(target);
        }
        if !attack_targets.can_target_buildings() {
            return None;
        }

        let mut best_building: Option<(u64, SimId)> = None;
        for candidate in buildings {
            if candidate.team == source.team || candidate.health <= 0 || candidate.id == source.id {
                continue;
            }
            let distance_sq = footprint_to_footprint_distance_sq(
                source.footprint,
                candidate.footprint,
                self.config.navigation_cell_size,
            );
            if distance_sq > acquisition_range_sq {
                continue;
            }
            let key = (distance_sq, candidate.id);
            if best_building.is_none_or(|current| key < current) {
                best_building = Some(key);
            }
        }
        best_building.map(|(_, target)| target)
    }

    fn building_source_target_retainable(
        &self,
        source: &BuildingSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let Some(attack) = source.attack else {
            return false;
        };
        let attack_targets = source
            .attack_targets
            .expect("attack building target mask missing");
        let range_sq = attack.acquisition_range_sq();
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return target.team != source.team
                && target.health > 0
                && attack_targets.can_target_unit(target.movement_class)
                && point_to_footprint_distance_sq(
                    target.position,
                    source.footprint,
                    self.config.navigation_cell_size,
                ) <= range_sq;
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            return attack_targets.can_target_buildings()
                && target.team != source.team
                && target.health > 0
                && footprint_to_footprint_distance_sq(
                    source.footprint,
                    target.footprint,
                    self.config.navigation_cell_size,
                ) <= range_sq;
        }
        false
    }

    fn acquire_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        grid: &SpatialGrid,
    ) -> Option<SimId> {
        let source_cell = self.topology.cell_of_point(source.position);
        let component = (source.movement_class == MovementClass::Ground)
            .then(|| self.topology.component_id(source_cell))
            .flatten();
        let enemy_team = 1u8
            .checked_sub(source.team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let partition = if source.movement_class == MovementClass::Ground
            && matches!(source.attack.delivery, AttackDelivery::Melee)
            && !source.attack_targets.can_target_unit(MovementClass::Air)
        {
            SpatialPartition::new(enemy_team, component?)
        } else {
            SpatialPartition::global(enemy_team)
        };
        let mut best: Option<(u8, u64, SimId)> = None;
        grid.for_each_candidate(
            partition,
            source.position,
            source.attack.acquisition_range,
            |index| {
                let candidate = &units[index];
                debug_assert_ne!(source.team, candidate.team);
                if candidate.health <= 0
                    || !source
                        .attack_targets
                        .can_target_unit(candidate.movement_class)
                {
                    return;
                }
                let distance_sq = source.position.distance_sq(candidate.position);
                if !self.unit_target_reachable(
                    source,
                    candidate,
                    distance_sq,
                    source.attack.acquisition_range_sq(),
                ) {
                    return;
                }
                let key = (0, distance_sq, candidate.id);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );

        if !source.attack_targets.can_target_buildings() {
            return best.map(|(_, _, id)| id);
        }
        for building in buildings {
            if source.team == building.team || building.health <= 0 {
                continue;
            }
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                building.footprint,
                self.config.navigation_cell_size,
            );
            if !self.building_target_reachable(
                source,
                building,
                distance_sq,
                source.attack.acquisition_range_sq(),
            ) {
                continue;
            }
            let key = (1, distance_sq, building.id);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best.map(|(_, _, id)| id)
    }

    fn recent_retaliation_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Option<SimId> {
        let previous_tick = self.next_tick.checked_sub(1)?;
        if source.retaliation.attacked_tick != Some(previous_tick) {
            return None;
        }
        let attacker = source.retaliation.attacker?;
        self.direct_retaliation_target_retainable_for(source, attacker, units, buildings)
            .then_some(attacker)
    }

    fn current_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if source.direct_retaliation_lock {
            self.direct_retaliation_target_retainable_for(source, target_id, units, buildings)
        } else if source.ally_defense_lock {
            self.ally_defense_target_retainable_for(source, target_id, units, buildings)
        } else {
            self.target_retainable_for(source, target_id, units, buildings)
        }
    }

    fn ally_defense_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return source.team != target.team
                && target.health > 0
                && self.unit_target_reachable(
                    source,
                    target,
                    source.position.distance_sq(target.position),
                    u64::MAX,
                );
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            return source.team != target.team
                && target.health > 0
                && self.building_target_reachable(source, target, distance_sq, u64::MAX);
        }
        false
    }

    fn direct_retaliation_target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let retaliation_range = source
            .attack
            .acquisition_range
            .checked_mul(DIRECT_RETALIATION_RANGE_MULTIPLIER)
            .expect("direct retaliation range overflowed validated bounds");
        let retaliation_range_sq = square_i32(retaliation_range);
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            return source.team != target.team
                && target.health > 0
                && source.attack_targets.can_target_unit(target.movement_class)
                && source.position.distance_sq(target.position) <= retaliation_range_sq;
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            return source.attack_targets.can_target_buildings()
                && source.team != target.team
                && target.health > 0
                && point_to_footprint_distance_sq(
                    source.position,
                    target.footprint,
                    self.config.navigation_cell_size,
                ) <= retaliation_range_sq;
        }
        false
    }

    fn recent_ally_defense_target(
        &self,
        source: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        defense_attacker_grid: &SpatialGrid,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
    ) -> DefenseTargetSearch {
        if defense_victims.is_empty() {
            return DefenseTargetSearch::default();
        }
        let Some(previous_tick) = self.next_tick.checked_sub(1) else {
            return DefenseTargetSearch::default();
        };
        let mut search = DefenseTargetSearch {
            queried: true,
            ..DefenseTargetSearch::default()
        };
        let context = DefenseSearchContext {
            units,
            buildings,
            attacker_grid: defense_attacker_grid,
        };
        let mut rejected_through_distance = None;
        let mut best_building: Option<(u64, u64, SimId, SimId)> = None;

        loop {
            let Some((ally_distance_sq, victim_indices)) = self.nearest_defense_victim_layer(
                source,
                previous_tick,
                rejected_through_distance,
                defense_victims,
                alert_grid,
                &mut search.victim_candidates,
            ) else {
                search.target = best_building.map(|(_, _, attacker, _)| attacker);
                return search;
            };

            let mut best_unit: Option<(u64, SimId, SimId)> = None;
            for victim_index in victim_indices {
                let victim = &defense_victims[victim_index];
                let Some((attacker_distance_sq, attacker_id)) = self
                    .nearest_valid_defense_attacker(
                        source,
                        victim,
                        &context,
                        victim_index,
                        &mut search.attacker_candidates,
                    )
                else {
                    continue;
                };
                if find_unit_index(units, attacker_id).is_some() {
                    let key = (attacker_distance_sq, attacker_id, victim.victim_id);
                    if best_unit.is_none_or(|current| key < current) {
                        best_unit = Some(key);
                    }
                } else {
                    let key = (
                        ally_distance_sq,
                        attacker_distance_sq,
                        attacker_id,
                        victim.victim_id,
                    );
                    if best_building.is_none_or(|current| key < current) {
                        best_building = Some(key);
                    }
                }
            }

            if let Some((_, attacker, _)) = best_unit {
                search.target = Some(attacker);
                return search;
            }
            rejected_through_distance = Some(ally_distance_sq);
        }
    }

    fn nearest_defense_victim_layer(
        &self,
        source: &UnitSnapshot,
        previous_tick: u64,
        rejected_through_distance: Option<u64>,
        defense_victims: &[DefenseVictim],
        alert_grid: &SpatialGrid,
        victim_candidates: &mut usize,
    ) -> Option<(u64, Vec<usize>)> {
        let acquisition_range_sq = source.attack.acquisition_range_sq();
        let mut best_distance = None;
        let mut nearest = Vec::new();
        alert_grid.for_each_candidate_nearest_cells(
            SpatialPartition::global(source.team.0),
            source.position,
            source.attack.acquisition_range,
            acquisition_range_sq,
            |victim_index| {
                *victim_candidates += 1;
                let victim = &defense_victims[victim_index];
                if victim.attacked_tick != previous_tick || victim.victim_id == source.id {
                    return None;
                }
                let distance_sq = source.position.distance_sq(victim.victim_position);
                if distance_sq > acquisition_range_sq
                    || rejected_through_distance.is_some_and(|rejected| distance_sq <= rejected)
                {
                    return None;
                }

                match best_distance {
                    None => {
                        best_distance = Some(distance_sq);
                        nearest.push(victim_index);
                        Some(distance_sq)
                    }
                    Some(current) if distance_sq < current => {
                        best_distance = Some(distance_sq);
                        nearest.clear();
                        nearest.push(victim_index);
                        Some(distance_sq)
                    }
                    Some(current) if distance_sq == current => {
                        nearest.push(victim_index);
                        None
                    }
                    Some(_) => None,
                }
            },
        );
        best_distance.map(|distance| (distance, nearest))
    }

    fn nearest_valid_defense_attacker(
        &self,
        source: &UnitSnapshot,
        victim: &DefenseVictim,
        context: &DefenseSearchContext<'_>,
        victim_index: usize,
        attacker_candidates: &mut usize,
    ) -> Option<(u64, SimId)> {
        let pursuit_range = self.target_pursuit_range(source);
        let pursuit_range_sq = square_i32(pursuit_range);
        let mut best: Option<(u64, SimId)> = None;

        if !victim.unit_attackers.is_empty() {
            context.attacker_grid.for_each_candidate_nearest_cells(
                defense_attacker_partition(victim_index),
                source.position,
                pursuit_range,
                pursuit_range_sq,
                |unit_index| {
                    *attacker_candidates += 1;
                    let candidate = &context.units[unit_index];
                    let distance_sq = source.position.distance_sq(candidate.position);
                    if !self.unit_target_reachable(source, candidate, distance_sq, pursuit_range_sq)
                    {
                        return None;
                    }
                    let key = (distance_sq, candidate.id);
                    if best.is_none_or(|current| key < current) {
                        best = Some(key);
                        Some(distance_sq)
                    } else {
                        None
                    }
                },
            );

            if best.is_none() {
                for &unit_index in &victim.unit_attackers {
                    let candidate = &context.units[unit_index];
                    let distance_sq = source.position.distance_sq(candidate.position);
                    if distance_sq <= pursuit_range_sq {
                        continue;
                    }
                    *attacker_candidates += 1;
                    if !self.unit_target_reachable(source, candidate, distance_sq, u64::MAX) {
                        continue;
                    }
                    let key = (distance_sq, candidate.id);
                    if best.is_none_or(|current| key < current) {
                        best = Some(key);
                    }
                }
            }
        }

        if best.is_some() {
            return best;
        }

        for &building_index in &victim.building_attackers {
            *attacker_candidates += 1;
            let target = &context.buildings[building_index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            if !self.building_target_reachable(source, target, distance_sq, u64::MAX) {
                continue;
            }
            let key = (distance_sq, target.id);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
            }
        }
        best
    }

    fn target_pursuit_range(&self, source: &UnitSnapshot) -> i32 {
        source
            .attack
            .range
            .checked_add(self.config.target_pursuit_extra_range)
            .expect("pursuit range overflowed validated coordinate bounds")
            .max(source.attack.acquisition_range)
    }

    fn target_retainable_for(
        &self,
        source: &UnitSnapshot,
        target_id: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let pursuit_range_sq = square_i32(self.target_pursuit_range(source));
        if let Some(index) = find_unit_index(units, target_id) {
            let target = &units[index];
            let distance_sq = source.position.distance_sq(target.position);
            return source.team != target.team
                && target.health > 0
                && self.unit_target_reachable(source, target, distance_sq, pursuit_range_sq);
        }
        if let Some(index) = find_building_index(buildings, target_id) {
            let target = &buildings[index];
            let distance_sq = point_to_footprint_distance_sq(
                source.position,
                target.footprint,
                self.config.navigation_cell_size,
            );
            return source.team != target.team
                && target.health > 0
                && self.building_target_reachable(source, target, distance_sq, pursuit_range_sq);
        }
        false
    }

    fn unit_target_reachable(
        &self,
        source: &UnitSnapshot,
        target: &UnitSnapshot,
        distance_sq: u64,
        pursuit_limit_sq: u64,
    ) -> bool {
        if source.health <= 0
            || target.health <= 0
            || distance_sq > pursuit_limit_sq
            || !source.attack_targets.can_target_unit(target.movement_class)
        {
            return false;
        }
        let Some((attack, _, _)) = source.attack_for_unit(target.movement_class) else {
            return false;
        };
        let in_attack_range = distance_sq <= attack.range_sq();
        if source.movement_class == MovementClass::Air {
            return in_attack_range || source.movement.speed_per_tick > 0;
        }
        if target.movement_class == MovementClass::Air {
            if in_attack_range {
                return true;
            }
            if source.movement.speed_per_tick == 0 {
                return false;
            }
            let source_cell = self.topology.cell_of_point(source.position);
            return self
                .nearest_reachable_unit_attack_cell(
                    source_cell,
                    source.position,
                    target.position,
                    attack.range,
                    source.collision_radius_override,
                )
                .is_some();
        }
        match attack.delivery {
            AttackDelivery::Melee => {
                self.topology.same_component(
                    self.topology.cell_of_point(source.position),
                    self.topology.cell_of_point(target.position),
                ) && (in_attack_range || source.movement.speed_per_tick > 0)
            }
            AttackDelivery::RangedInstant
            | AttackDelivery::RangedGuaranteedHit { .. }
            | AttackDelivery::RangedBallistic { .. }
            | AttackDelivery::Bounce { .. } => {
                in_attack_range
                    || (source.movement.speed_per_tick > 0
                        && self.topology.same_component(
                            self.topology.cell_of_point(source.position),
                            self.topology.cell_of_point(target.position),
                        ))
            }
        }
    }

    fn building_target_reachable(
        &self,
        source: &UnitSnapshot,
        target: &BuildingSnapshot,
        distance_sq: u64,
        pursuit_limit_sq: u64,
    ) -> bool {
        if source.health <= 0
            || target.health <= 0
            || distance_sq > pursuit_limit_sq
            || !source.attack_targets.can_target_buildings()
        {
            return false;
        }
        let Some((attack, _, _)) = source.attack_for_building() else {
            return false;
        };
        let in_attack_range = distance_sq <= attack.range_sq();
        if source.movement_class == MovementClass::Air {
            return in_attack_range || source.movement.speed_per_tick > 0;
        }
        if matches!(
            attack.delivery,
            AttackDelivery::RangedInstant
                | AttackDelivery::RangedGuaranteedHit { .. }
                | AttackDelivery::RangedBallistic { .. }
                | AttackDelivery::Bounce { .. }
        ) && in_attack_range
        {
            return true;
        }
        (in_attack_range || source.movement.speed_per_tick > 0)
            && self
                .topology
                .nearest_reachable_perimeter_cell(
                    self.topology.cell_of_point(source.position),
                    target.footprint,
                )
                .is_some()
    }
}
