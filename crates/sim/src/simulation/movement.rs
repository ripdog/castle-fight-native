use super::*;

impl Simulation {
    pub(super) fn resolve_movement(
        &mut self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        positions: &mut [SimPoint],
        navigation_states: &mut [NavigationState],
    ) -> MovementMetrics {
        let intent_start = Instant::now();
        let mut radius_objective_keys: Vec<_> = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| {
                (unit_health[index] > 0)
                    .then_some(unit.collision_radius_override)
                    .flatten()
                    .map(|radius| (unit.team.0, radius))
            })
            .collect();
        radius_objective_keys.sort_unstable();
        radius_objective_keys.dedup();
        for (team, radius) in radius_objective_keys {
            if self.radius_objective_fields.contains_key(&(team, radius)) {
                continue;
            }
            let field = self
                .topology
                .objective_distance_field_with_radius(team, radius);
            self.radius_objective_fields.insert((team, radius), field);
        }

        let decisions: Vec<_> = self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(index, unit)| {
                    self.desired_position(
                        index,
                        unit,
                        units,
                        buildings,
                        unit_health,
                        building_health,
                    )
                })
                .collect()
        });
        for decision in &decisions {
            let Some(entry) = decision.cache_insert else {
                continue;
            };
            let key = (
                entry.from,
                entry.target,
                entry.collision_radius,
                entry.route_bias,
            );
            if self.pursuit_cache.len() >= PURSUIT_CACHE_CAPACITY
                && !self.pursuit_cache.contains_key(&key)
            {
                self.pursuit_cache.clear();
            }
            self.pursuit_cache.insert(key, entry.next);
        }
        let intent = intent_start.elapsed();
        let desired_positions: Vec<_> =
            decisions.iter().map(|decision| decision.position).collect();
        let pursuit_steps = decisions
            .iter()
            .filter(|decision| decision.pursuit_step)
            .count();
        let navigation_route_steps = decisions
            .iter()
            .filter(|decision| decision.navigation_route_step)
            .count();
        let movement_intents = decisions
            .iter()
            .zip(units)
            .filter(|(decision, unit)| decision.position != unit.position)
            .count();
        let objective_move_intents = decisions
            .iter()
            .zip(units)
            .filter(|(decision, unit)| {
                navigation_goal(unit, decision) == NavigationGoal::Objective(unit.team)
            })
            .count();
        let a_star_fallbacks = decisions
            .iter()
            .filter(|decision| decision.used_a_star)
            .count();
        let a_star_cache_hits = decisions
            .iter()
            .filter(|decision| decision.a_star_cache_hit)
            .count();
        let a_star_expanded_nodes = decisions
            .iter()
            .map(|decision| decision.a_star_expanded_nodes)
            .sum();

        let separation_start = Instant::now();
        let separated_positions =
            self.apply_crowd_separation(units, unit_health, &decisions, &desired_positions);
        let crowd_separation = separation_start.elapsed();
        let collision_start = Instant::now();
        let (
            legal_positions,
            collision_fallback_search,
            collision_fallback_searches,
            collision_fallback_candidate_checks,
            collision_fallback_max_ring,
        ) = self.enforce_hard_non_overlap(
            units,
            unit_health,
            navigation_states,
            &decisions,
            &desired_positions,
            &separated_positions,
        );
        let hard_collision = collision_start.elapsed();
        let crowd_and_collision = crowd_separation + hard_collision;
        let movement_blocked = decisions
            .iter()
            .zip(units)
            .zip(&legal_positions)
            .filter(|((decision, unit), legal)| {
                decision.position != unit.position && **legal == unit.position
            })
            .count();
        positions.copy_from_slice(&legal_positions);
        MovementMetrics {
            intent,
            crowd_separation,
            hard_collision,
            collision_fallback_search,
            collision_fallback_searches,
            collision_fallback_candidate_checks,
            collision_fallback_max_ring,
            crowd_and_collision,
            pursuit_steps,
            navigation_route_steps,
            movement_intents,
            movement_blocked,
            objective_move_intents,
            a_star_fallbacks,
            a_star_cache_hits,
            a_star_expanded_nodes,
        }
    }
}

impl Simulation {
    fn desired_position(
        &self,
        index: usize,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
    ) -> MovementDecision {
        let current = unit.position;
        let movement_speed = effective_movement_speed(unit);
        if unit_health[index] <= 0
            || movement_speed == 0
            || self.next_tick < unit.status.stunned_until_tick
        {
            return MovementDecision::stationary(current);
        }
        if unit.movement_class == MovementClass::Air {
            return self.desired_air_position(
                unit,
                units,
                buildings,
                unit_health,
                building_health,
                movement_speed,
            );
        }
        let source_cell = self.topology.cell_of_point(current);
        let mut pursuit_target = None;
        let mut attack_goal = None;

        let target_cell = unit.target.and_then(|target_id| {
            if let Some(target_index) = find_unit_index(units, target_id) {
                if unit_health[target_index] <= 0 {
                    return None;
                }
                let target_position = units[target_index].position;
                if current.distance_sq(target_position) <= unit.attack.range_sq() {
                    attack_goal = Some(current);
                    return Some(source_cell);
                }
                let mut goal =
                    point_attack_envelope_goal(current, target_position, unit.attack.range);
                let mut cell = self.topology.cell_of_point(goal);
                let goal_is_traversable = self.position_is_traversable_from(
                    source_cell,
                    goal,
                    unit.collision_radius_override,
                ) && unit.collision_radius_override.is_none_or(|_| {
                    self.position_is_traversable_from(
                        source_cell,
                        self.topology.center_of_cell(cell),
                        unit.collision_radius_override,
                    )
                });
                if !goal_is_traversable {
                    if unit.collision_radius_override.is_some() {
                        cell = self.nearest_reachable_unit_attack_cell(
                            source_cell,
                            current,
                            target_position,
                            unit.attack.range,
                            unit.collision_radius_override,
                        )?;
                        goal = self.topology.center_of_cell(cell);
                    } else {
                        cell = self.topology.cell_of_point(target_position);
                        if !self.topology.same_component(source_cell, cell) {
                            return None;
                        }
                        goal = target_position;
                    }
                }
                pursuit_target = Some(target_id);
                attack_goal = Some(goal);
                Some(cell)
            } else if let Some(target_index) = find_building_index(buildings, target_id) {
                if building_health[target_index] <= 0 {
                    return None;
                }
                let footprint = buildings[target_index].footprint;
                if point_to_footprint_distance_sq(
                    current,
                    footprint,
                    self.config.navigation_cell_size,
                ) <= unit.attack.range_sq()
                {
                    attack_goal = Some(current);
                    return Some(source_cell);
                }
                let mut goal = building_attack_envelope_goal(
                    current,
                    footprint,
                    unit.attack.range,
                    self.config.navigation_cell_size,
                );
                let mut cell = self.topology.cell_of_point(goal);
                let goal_is_traversable = self.position_is_traversable_from(
                    source_cell,
                    goal,
                    unit.collision_radius_override,
                ) && unit.collision_radius_override.is_none_or(|_| {
                    self.position_is_traversable_from(
                        source_cell,
                        self.topology.center_of_cell(cell),
                        unit.collision_radius_override,
                    )
                });
                if !goal_is_traversable {
                    if unit.collision_radius_override.is_some() {
                        cell = self.nearest_reachable_building_attack_cell(
                            source_cell,
                            current,
                            footprint,
                            unit.attack.range,
                            unit.collision_radius_override,
                        )?;
                        goal = self.topology.center_of_cell(cell);
                    } else {
                        cell = self
                            .topology
                            .nearest_reachable_perimeter_cell(source_cell, footprint)?;
                        goal = building_attack_envelope_goal(
                            self.topology.center_of_cell(cell),
                            footprint,
                            unit.attack.range,
                            self.config.navigation_cell_size,
                        );
                    }
                }
                pursuit_target = Some(target_id);
                attack_goal = Some(goal);
                Some(cell)
            } else {
                None
            }
        });

        if attack_goal == Some(current) {
            return MovementDecision::stationary(current);
        }

        let mut pursuit_step = false;
        let mut targetless_lane_goal = None;
        let route = match target_cell {
            Some(cell) => {
                pursuit_step = pursuit_target.is_some();
                if cell == source_cell {
                    NavigationRoute::at(cell)
                } else {
                    self.route_to_cell(
                        source_cell,
                        cell,
                        unit.collision_radius_override,
                        sidestep_sign(unit.id),
                    )
                }
            }
            None => {
                let route_bias = sidestep_sign(unit.id);
                if let Some(goal) =
                    self.targetless_lane_ingress_goal(unit.team, current, unit.collision_radius)
                {
                    let cell = self.topology.cell_of_point(goal);
                    let lane_route = if cell == source_cell {
                        NavigationRoute::at(cell)
                    } else {
                        self.route_to_cell(
                            source_cell,
                            cell,
                            unit.collision_radius_override,
                            route_bias,
                        )
                    };
                    if lane_route.next_cell.is_some() {
                        targetless_lane_goal = Some((cell, goal));
                        lane_route
                    } else {
                        // A closed cage or other disconnected local component can make the normal
                        // lane entrance unreachable. Preserve the established no-route behavior in
                        // that case: press toward the enemy side within the current component rather
                        // than freezing or inventing a route through blockers.
                        self.targetless_horizontal_route(unit, source_cell, route_bias)
                    }
                } else {
                    self.targetless_horizontal_route(unit, source_cell, route_bias)
                }
            }
        };
        let navigation_route_step = route.navigation_route_step;
        let used_a_star = route.used_a_star;
        let a_star_cache_hit = route.a_star_cache_hit;
        let a_star_expanded_nodes = route.a_star_expanded_nodes;
        let cache_insert = route.cache_insert;
        let next_cell = route.next_cell;
        let Some(next_cell) = next_cell else {
            return MovementDecision {
                position: current,
                pursuit_step,
                pursuit_target,
                attack_goal,
                navigation_route_step,
                used_a_star,
                a_star_cache_hit,
                a_star_expanded_nodes,
                cache_insert,
            };
        };
        let target_position =
            if pursuit_step && next_cell == target_cell.expect("pursuit target cell disappeared") {
                attack_goal.expect("pursuit movement missing attack-envelope goal")
            } else if let Some((goal_cell, goal)) = targetless_lane_goal
                && next_cell == goal_cell
            {
                goal
            } else if target_cell.is_none()
                && targetless_lane_goal.is_none()
                && next_cell.y == source_cell.y
            {
                SimPoint::new(self.topology.center_of_cell(next_cell).x, current.y)
            } else {
                self.topology.center_of_cell(next_cell)
            };
        let candidate = current.step_towards(target_position, movement_speed);
        let position = if self.position_is_traversable_from(
            source_cell,
            candidate,
            unit.collision_radius_override,
        ) {
            candidate
        } else {
            // A unit can be legally positioned off-center inside its nav cell while the straight
            // segment toward the next cell clips an expanded building corner. Repeating the same
            // rejected endpoint forever creates a local corner lock. Move back toward the current
            // cell center first; this gives the radius-aware cell route a legal portal to leave
            // through without adding sticky per-unit path state.
            let recenter_target = self.topology.center_of_cell(source_cell);
            let recenter = current.step_towards(recenter_target, movement_speed);
            if recenter != current
                && self.position_is_traversable_from(
                    source_cell,
                    recenter,
                    unit.collision_radius_override,
                )
            {
                recenter
            } else {
                current
            }
        };
        MovementDecision {
            position,
            pursuit_step,
            pursuit_target,
            attack_goal,
            navigation_route_step,
            used_a_star,
            a_star_cache_hit,
            a_star_expanded_nodes,
            cache_insert,
        }
    }

    fn route_to_cell(
        &self,
        source_cell: NavCell,
        target_cell: NavCell,
        collision_radius: Option<i32>,
        route_bias: i32,
    ) -> NavigationRoute {
        debug_assert_ne!(source_cell, target_cell);
        let route_bias_key = i8::try_from(route_bias).expect("route bias must fit signed byte");
        let cache_key = (source_cell, target_cell, collision_radius, route_bias_key);
        let cached_fallback = self.pursuit_cache.get(&cache_key).copied();
        let result: PursuitStep = if let Some(radius) = collision_radius {
            self.topology.pursuit_step_with_radius(
                source_cell,
                target_cell,
                cached_fallback,
                radius,
                route_bias,
            )
        } else {
            self.topology
                .pursuit_step(source_cell, target_cell, cached_fallback, route_bias)
        };
        let cache_insert = if result.used_a_star && !result.a_star_cache_hit {
            result.next_cell.map(|next| PursuitCacheInsert {
                from: source_cell,
                target: target_cell,
                collision_radius,
                route_bias: route_bias_key,
                next,
            })
        } else {
            None
        };
        NavigationRoute {
            next_cell: result.next_cell,
            navigation_route_step: true,
            used_a_star: result.used_a_star,
            a_star_cache_hit: result.a_star_cache_hit,
            a_star_expanded_nodes: result.a_star_expanded_nodes,
            cache_insert,
        }
    }

    fn targetless_horizontal_route(
        &self,
        unit: &UnitSnapshot,
        source_cell: NavCell,
        route_bias: i32,
    ) -> NavigationRoute {
        let objective_cell = self
            .topology
            .cell_of_point(self.config.team_objective[usize::from(unit.team.0)]);
        if objective_cell.x == source_cell.x {
            return NavigationRoute::none();
        }

        let step_x = (objective_cell.x - source_cell.x).signum();
        let direct_cell = NavCell::new(source_cell.x + step_x, source_cell.y);
        let direct_position = self.topology.center_of_cell(direct_cell);
        if self.position_is_traversable_from(
            source_cell,
            direct_position,
            unit.collision_radius_override,
        ) {
            return NavigationRoute::at(direct_cell);
        }

        let objective_detour_step = if let Some(radius) = unit.collision_radius_override {
            let field = self
                .radius_objective_fields
                .get(&(unit.team.0, radius))
                .expect("radius-aware objective field was not prepared");
            self.topology.step_from_distance_field_with_radius_bias(
                source_cell,
                field,
                radius,
                route_bias,
            )
        } else {
            self.topology
                .objective_step_with_bias(unit.team.0, source_cell, route_bias)
        };
        if let Some(next_cell) = objective_detour_step {
            // While the unit's preferred horizontal step is topologically blocked, follow the
            // stable shared objective field instead of repeatedly A*-routing to a same-row goal
            // that changes as the detour changes rows. Once horizontal progress is clear again,
            // the normal stateless lane rule resumes from the unit's new y coordinate.
            return NavigationRoute {
                next_cell: Some(next_cell),
                navigation_route_step: true,
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
                cache_insert: None,
            };
        }

        let Some(cell) = self.horizontal_objective_goal_cell(
            source_cell,
            objective_cell.x,
            unit.collision_radius_override,
        ) else {
            return NavigationRoute::none();
        };
        // Disconnected/caged components have no objective-field descent. Keep the horizontal
        // best-effort A* fallback so those units still press toward the objective-side wall
        // without inventing a route through blockers.
        if cell == source_cell {
            NavigationRoute::at(cell)
        } else {
            self.route_to_cell(
                source_cell,
                cell,
                unit.collision_radius_override,
                route_bias,
            )
        }
    }

    fn targetless_lane_ingress_goal(
        &self,
        team: Team,
        current: SimPoint,
        collision_radius: i32,
    ) -> Option<SimPoint> {
        let lane = self.config.targetless_lane?;
        let min_center_y = lane.min_y.checked_add(collision_radius)?;
        let max_center_y = lane.max_y.checked_sub(collision_radius)?;
        if min_center_y > max_center_y || (current.y >= min_center_y && current.y <= max_center_y) {
            return None;
        }

        let goal_y = current.y.clamp(min_center_y, max_center_y);
        let inward_distance = (i64::from(current.y) - i64::from(goal_y)).abs();
        let team_index = usize::from(team.0);
        let other_team_index = 1 - team_index;
        let current_x = i64::from(current.x);
        let objective_x = i64::from(self.config.team_objective[team_index].x);
        let other_objective_x = i64::from(self.config.team_objective[other_team_index].x);
        let forward = (objective_x - other_objective_x).signum();
        let projected_x = current_x + forward * inward_distance;
        let goal_x = match forward {
            1 => projected_x.min(objective_x.max(current_x)),
            -1 => projected_x.max(objective_x.min(current_x)),
            _ => current_x,
        };
        Some(SimPoint::new(i32::try_from(goal_x).ok()?, goal_y))
    }

    fn desired_air_position(
        &self,
        unit: &UnitSnapshot,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        unit_health: &[i32],
        building_health: &[i32],
        movement_speed: i32,
    ) -> MovementDecision {
        let current = unit.position;
        let mut pursuit_target = None;
        let mut attack_goal = None;
        let goal = unit
            .target
            .and_then(|target_id| {
                if let Some(target_index) = find_unit_index(units, target_id) {
                    if unit_health[target_index] <= 0 {
                        return None;
                    }
                    let target_position = units[target_index].position;
                    if current.distance_sq(target_position) <= unit.attack.range_sq() {
                        attack_goal = Some(current);
                        return Some(current);
                    }
                    pursuit_target = Some(target_id);
                    let goal =
                        point_attack_envelope_goal(current, target_position, unit.attack.range);
                    attack_goal = Some(goal);
                    Some(goal)
                } else if let Some(target_index) = find_building_index(buildings, target_id) {
                    if building_health[target_index] <= 0 {
                        return None;
                    }
                    let footprint = buildings[target_index].footprint;
                    if point_to_footprint_distance_sq(
                        current,
                        footprint,
                        self.config.navigation_cell_size,
                    ) <= unit.attack.range_sq()
                    {
                        attack_goal = Some(current);
                        return Some(current);
                    }
                    pursuit_target = Some(target_id);
                    let goal = building_attack_envelope_goal(
                        current,
                        footprint,
                        unit.attack.range,
                        self.config.navigation_cell_size,
                    );
                    attack_goal = Some(goal);
                    Some(goal)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                self.targetless_lane_ingress_goal(unit.team, current, unit.collision_radius)
                    .unwrap_or_else(|| {
                        SimPoint::new(
                            self.config.team_objective[usize::from(unit.team.0)].x,
                            current.y,
                        )
                    })
            });

        if goal == current {
            return MovementDecision::stationary(current);
        }

        let source_cell = self.air_topology.cell_of_point(current);
        let direct_candidate = current.step_towards(goal, movement_speed);
        if self.air_position_is_traversable_from(
            source_cell,
            direct_candidate,
            unit.collision_radius,
        ) {
            return MovementDecision {
                position: direct_candidate,
                pursuit_step: pursuit_target.is_some(),
                pursuit_target,
                attack_goal,
                navigation_route_step: false,
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
                cache_insert: None,
            };
        }

        let target_cell = self.air_topology.cell_of_point(goal);
        let route = self.air_topology.pursuit_step_with_radius(
            source_cell,
            target_cell,
            None,
            unit.collision_radius,
            sidestep_sign(unit.id),
        );
        let Some(next_cell) = route.next_cell else {
            return MovementDecision {
                position: current,
                pursuit_step: pursuit_target.is_some(),
                pursuit_target,
                attack_goal,
                navigation_route_step: true,
                used_a_star: route.used_a_star,
                a_star_cache_hit: route.a_star_cache_hit,
                a_star_expanded_nodes: route.a_star_expanded_nodes,
                cache_insert: None,
            };
        };
        let route_target = if next_cell == target_cell {
            goal
        } else {
            self.air_topology.center_of_cell(next_cell)
        };
        let candidate = current.step_towards(route_target, movement_speed);
        let position =
            if self.air_position_is_traversable_from(source_cell, candidate, unit.collision_radius)
            {
                candidate
            } else {
                let recenter_target = self.air_topology.center_of_cell(source_cell);
                let recenter = current.step_towards(recenter_target, movement_speed);
                if recenter != current
                    && self.air_position_is_traversable_from(
                        source_cell,
                        recenter,
                        unit.collision_radius,
                    )
                {
                    recenter
                } else {
                    current
                }
            };
        MovementDecision {
            position,
            pursuit_step: pursuit_target.is_some(),
            pursuit_target,
            attack_goal,
            navigation_route_step: true,
            used_a_star: route.used_a_star,
            a_star_cache_hit: route.a_star_cache_hit,
            a_star_expanded_nodes: route.a_star_expanded_nodes,
            cache_insert: None,
        }
    }

    fn horizontal_objective_goal_cell(
        &self,
        source_cell: NavCell,
        objective_x: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let step_x = (objective_x - source_cell.x).signum();
        if step_x == 0 {
            return None;
        }
        let source_component = self.topology.component_id(source_cell)?;
        let mut x = objective_x;
        while x != source_cell.x {
            let cell = NavCell::new(x, source_cell.y);
            let valid = if let Some(radius) = collision_radius {
                self.topology.circle_is_traversable_in_component(
                    self.topology.center_of_cell(cell),
                    radius,
                    source_component,
                )
            } else {
                self.topology.component_id(cell) == Some(source_component)
            };
            if valid {
                return Some(cell);
            }
            x = x.checked_sub(step_x)?;
        }
        None
    }

    fn apply_crowd_separation(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        decisions: &[MovementDecision],
        desired_positions: &[SimPoint],
    ) -> Vec<SimPoint> {
        let max_separation = self.config.max_separation_per_tick;
        let max_radius = units
            .iter()
            .enumerate()
            .filter(|(index, _)| unit_health[*index] > 0)
            .map(|(_, unit)| unit.collision_radius)
            .max()
            .unwrap_or(0);
        if max_radius == 0 || max_separation == 0 {
            return desired_positions.to_vec();
        }

        let max_pair_distance = max_radius.saturating_mul(2);
        let max_anticipation_distance = max_pair_distance.saturating_mul(2);
        let collision_grid = SpatialGrid::build(
            max_anticipation_distance.max(1),
            desired_positions
                .iter()
                .enumerate()
                .filter(|(index, _)| unit_health[*index] > 0)
                .map(|(index, position)| {
                    (
                        movement_collision_partition(units[index].movement_class),
                        index,
                        *position,
                    )
                }),
        );
        self.pool.install(|| {
            units
                .par_iter()
                .enumerate()
                .map(|(index, unit)| {
                    if unit_health[index] <= 0 {
                        return desired_positions[index];
                    }
                    let desired = desired_positions[index];
                    let current_cell = self.topology.cell_of_point(unit.position);
                    if unit.movement_class == MovementClass::Ground
                        && self.topology.component_id(current_cell).is_none()
                    {
                        return desired;
                    }
                    let movement_x = i64::from(desired.x) - i64::from(unit.position.x);
                    let movement_y = i64::from(desired.y) - i64::from(unit.position.y);
                    let mut push_x = 0_i64;
                    let mut push_y = 0_i64;
                    let mut contributions = 0_i64;

                    let query_radius = unit
                        .collision_radius
                        .saturating_add(max_radius)
                        .saturating_mul(2);
                    collision_grid.for_each_candidate(
                        movement_collision_partition(unit.movement_class),
                        desired,
                        query_radius,
                        |other_index| {
                            if other_index == index || unit_health[other_index] <= 0 {
                                return;
                            }
                            let other = desired_positions[other_index];
                            let separation_distance = unit
                                .collision_radius
                                .saturating_add(units[other_index].collision_radius);
                            let anticipation_distance = separation_distance.saturating_mul(2);
                            let separation_sq = square_i32(separation_distance);
                            let anticipation_sq = square_i32(anticipation_distance);
                            let distance_sq = desired.distance_sq(other);
                            if distance_sq >= anticipation_sq {
                                return;
                            }

                            let mut contributed = false;
                            if distance_sq < separation_sq {
                                let dx = i64::from(desired.x) - i64::from(other.x);
                                let dy = i64::from(desired.y) - i64::from(other.y);
                                let (direction_x, direction_y, axis_distance) =
                                    if dx == 0 && dy == 0 {
                                        let (x, y) =
                                            exact_overlap_direction(unit.id, units[other_index].id);
                                        (i64::from(x), i64::from(y), 0_i64)
                                    } else {
                                        (dx.signum(), dy.signum(), dx.abs().max(dy.abs()))
                                    };
                                let penetration =
                                    (i64::from(separation_distance) - axis_distance).max(1);
                                push_x += direction_x * penetration;
                                push_y += direction_y * penetration;
                                contributed = true;
                            }

                            if (movement_x != 0 || movement_y != 0)
                                && unit.target != Some(units[other_index].id)
                            {
                                let other_move_x =
                                    i64::from(other.x) - i64::from(units[other_index].position.x);
                                let other_move_y =
                                    i64::from(other.y) - i64::from(units[other_index].position.y);
                                let to_other_x = i64::from(units[other_index].position.x)
                                    - i64::from(unit.position.x);
                                let to_other_y = i64::from(units[other_index].position.y)
                                    - i64::from(unit.position.y);
                                let other_is_ahead =
                                    movement_x * to_other_x + movement_y * to_other_y > 0;
                                let relative_move_x = movement_x - other_move_x;
                                let relative_move_y = movement_y - other_move_y;
                                let closing =
                                    relative_move_x * to_other_x + relative_move_y * to_other_y > 0;
                                if other_is_ahead && closing {
                                    let axis_distance = to_other_x.abs().max(to_other_y.abs());
                                    let pressure =
                                        (i64::from(anticipation_distance) - axis_distance).max(1);
                                    let goal = navigation_goal(unit, &decisions[index]);
                                    let remembered_side = if unit.navigation.avoidance_goal == goal
                                        && goal != NavigationGoal::None
                                        && unit.navigation.bypass_side != 0
                                    {
                                        i64::from(unit.navigation.bypass_side)
                                    } else {
                                        i64::from(sidestep_sign(unit.id))
                                    };
                                    let perpendicular_x = -movement_y.signum() * remembered_side;
                                    let perpendicular_y = movement_x.signum() * remembered_side;
                                    push_x += perpendicular_x * pressure;
                                    push_y += perpendicular_y * pressure;
                                    contributed = true;
                                }
                            }
                            contributions += i64::from(contributed);
                        },
                    );

                    if contributions == 0 {
                        return desired;
                    }
                    push_x /= contributions;
                    push_y /= contributions;
                    let raw_offset = SimPoint::new(
                        i32::try_from(push_x).expect("crowd x offset overflow"),
                        i32::try_from(push_y).expect("crowd y offset overflow"),
                    );
                    let offset = SimPoint::new(0, 0).step_towards(raw_offset, max_separation);
                    self.valid_separated_position(current_cell, desired, offset, unit)
                })
                .collect()
        })
    }

    fn enforce_hard_non_overlap(
        &self,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        navigation_states: &mut [NavigationState],
        decisions: &[MovementDecision],
        desired_positions: &[SimPoint],
        separated_positions: &[SimPoint],
    ) -> (Vec<SimPoint>, Duration, usize, usize, u32) {
        let max_radius = units
            .iter()
            .enumerate()
            .filter(|(index, _)| unit_health[*index] > 0)
            .map(|(_, unit)| unit.collision_radius)
            .max()
            .unwrap_or(0);
        if max_radius == 0 {
            return (separated_positions.to_vec(), Duration::ZERO, 0, 0, 0);
        }

        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let reservation_cell_size = max_radius.saturating_mul(2).max(1);
        let mut ground_reservations = SpatialReservationGrid::build_with_radii(
            reservation_cell_size,
            bounds_min,
            bounds_max,
            units.len(),
            units.iter().enumerate().filter_map(|(index, unit)| {
                (unit_health[index] > 0 && unit.movement_class == MovementClass::Ground)
                    .then_some((index, separated_positions[index], unit.collision_radius))
            }),
        );
        let mut air_reservations =
            SpatialReservationGrid::build_with_radii(
                reservation_cell_size,
                bounds_min,
                bounds_max,
                units.len(),
                units.iter().enumerate().filter_map(|(index, unit)| {
                    (unit_health[index] > 0 && unit.movement_class == MovementClass::Air)
                        .then_some((index, separated_positions[index], unit.collision_radius))
                }),
            );
        let mut result: Vec<_> = units.iter().map(|unit| unit.position).collect();
        let mut fallback_search = Duration::ZERO;
        let mut fallback_searches = 0usize;
        let mut fallback_candidate_checks = 0usize;
        let mut fallback_max_ring = 0u32;
        let lateral = self.config.max_separation_per_tick.max(1);

        for index in (0..units.len()).rev() {
            let unit = &units[index];
            if unit_health[index] <= 0 {
                continue;
            }
            let original_cell = self.topology.cell_of_point(unit.position);
            if unit.movement_class == MovementClass::Ground
                && self.topology.component_id(original_cell).is_none()
            {
                continue;
            }
            let reservations = match unit.movement_class {
                MovementClass::Ground => &mut ground_reservations,
                MovementClass::Air => &mut air_reservations,
            };
            reservations.remove(index);

            let desired = desired_positions[index];
            let separated = separated_positions[index];
            let sidestep_distance = effective_movement_speed(unit).max(lateral).max(1);
            let goal = navigation_goal(unit, &decisions[index]);
            let navigating = goal != NavigationGoal::None;
            let navigation = &mut navigation_states[index];
            if !navigating || navigation.avoidance_goal != goal {
                *navigation = NavigationState::default();
            }

            let direct_clear = self.position_is_legal_for_unit(unit, original_cell, desired)
                && reservations.is_clear_with_radius(desired, unit.collision_radius);
            let mut avoiding = false;
            if navigating {
                if !direct_clear {
                    navigation.avoidance_goal = goal;
                    if navigation.bypass_side == 0 {
                        navigation.bypass_side =
                            i8::try_from(sidestep_sign(unit.id)).expect("sidestep sign fits i8");
                    }
                    navigation.clear_ticks = 0;
                    avoiding = true;
                } else if navigation.avoidance_goal == goal && navigation.bypass_side != 0 {
                    navigation.clear_ticks = navigation.clear_ticks.saturating_add(1);
                    if navigation.clear_ticks < AVOIDANCE_CLEAR_TICKS {
                        avoiding = true;
                    } else {
                        *navigation = NavigationState::default();
                    }
                }
            }

            let steer_toward = if desired != unit.position {
                desired
            } else {
                decisions[index].attack_goal.unwrap_or(desired)
            };
            let default_side = i8::try_from(sidestep_sign(unit.id)).expect("sidestep sign fits i8");
            let side = if avoiding {
                navigation.bypass_side
            } else {
                default_side
            };
            let preferred_tangent =
                perpendicular_step_with_side(side, unit.position, steer_toward, sidestep_distance);
            let opposite_tangent =
                perpendicular_step_with_side(-side, unit.position, steer_toward, sidestep_distance);
            let preferred_arc =
                pursuit_arc_step(side, unit.position, steer_toward, sidestep_distance, true);
            let preferred_back_arc =
                pursuit_arc_step(side, unit.position, steer_toward, sidestep_distance, false);
            let opposite_arc =
                pursuit_arc_step(-side, unit.position, steer_toward, sidestep_distance, true);
            let preferred_tangent =
                offset_point(unit.position, preferred_tangent.x, preferred_tangent.y);
            let opposite_tangent =
                offset_point(unit.position, opposite_tangent.x, opposite_tangent.y);
            let preferred_arc = offset_point(unit.position, preferred_arc.x, preferred_arc.y);
            let preferred_back_arc =
                offset_point(unit.position, preferred_back_arc.x, preferred_back_arc.y);
            let opposite_arc = offset_point(unit.position, opposite_arc.x, opposite_arc.y);

            let mut chosen = None;
            if avoiding {
                let candidates = [
                    (preferred_arc, side),
                    (preferred_tangent, side),
                    (preferred_back_arc, side),
                    (Some(separated), 0),
                    (opposite_arc, -side),
                    (opposite_tangent, -side),
                    (Some(desired), 0),
                    (Some(unit.position), side),
                ];
                for (candidate, candidate_side) in candidates {
                    let Some(candidate) = candidate else {
                        continue;
                    };
                    if self.position_is_legal_for_unit(unit, original_cell, candidate)
                        && reservations.is_clear_with_radius(candidate, unit.collision_radius)
                    {
                        if candidate_side != 0 && candidate_side != navigation.bypass_side {
                            navigation.bypass_side = candidate_side;
                            navigation.clear_ticks = 0;
                        }
                        chosen = Some(candidate);
                        break;
                    }
                }
            } else {
                for candidate in [
                    Some(separated),
                    Some(desired),
                    preferred_tangent,
                    opposite_tangent,
                    Some(unit.position),
                ]
                .into_iter()
                .flatten()
                {
                    if self.position_is_legal_for_unit(unit, original_cell, candidate)
                        && reservations.is_clear_with_radius(candidate, unit.collision_radius)
                    {
                        chosen = Some(candidate);
                        break;
                    }
                }
            }

            let chosen = match chosen {
                Some(chosen) => chosen,
                None => {
                    fallback_searches += 1;
                    let fallback_started = Instant::now();
                    let fallback = self.find_local_non_overlap_position(
                        unit,
                        original_cell,
                        sidestep_sign(unit.id),
                        reservations,
                        &mut fallback_candidate_checks,
                        &mut fallback_max_ring,
                    );
                    fallback_search += fallback_started.elapsed();
                    fallback.unwrap_or(unit.position)
                }
            };

            reservations.insert_with_radius(index, chosen, unit.collision_radius);
            result[index] = chosen;
        }

        (
            result,
            fallback_search,
            fallback_searches,
            fallback_candidate_checks,
            fallback_max_ring,
        )
    }

    fn find_local_non_overlap_position(
        &self,
        unit: &UnitSnapshot,
        original_cell: NavCell,
        search_bias: i32,
        reservations: &SpatialReservationGrid,
        candidate_checks: &mut usize,
        max_ring_reached: &mut u32,
    ) -> Option<SimPoint> {
        debug_assert!(search_bias == -1 || search_bias == 1);
        let origin = unit.position;
        let collision_radius = unit.collision_radius;
        let step = collision_radius.max(1);
        let (bounds_min, bounds_max) = self.navigation_world_bounds();
        let max_radius = [
            origin.x.saturating_sub(bounds_min.x).abs(),
            bounds_max.x.saturating_sub(origin.x).abs(),
            origin.y.saturating_sub(bounds_min.y).abs(),
            bounds_max.y.saturating_sub(origin.y).abs(),
        ]
        .into_iter()
        .max()
        .unwrap_or(step)
        .max(step);
        let max_ring = (max_radius + step - 1) / step;

        let legal_position = |candidate| match unit.movement_class {
            MovementClass::Ground => self.position_is_traversable_from(
                original_cell,
                candidate,
                unit.collision_radius_override,
            ),
            MovementClass::Air => {
                let source_cell = self.air_topology.cell_of_point(origin);
                self.air_position_is_traversable_from(source_cell, candidate, collision_radius)
            }
        };
        let order_multiplier = -search_bias;
        for ring in 1..=max_ring {
            *max_ring_reached = (*max_ring_reached)
                .max(u32::try_from(ring).expect("fallback ring is non-negative"));
            let distance = ring.checked_mul(step)?;
            for raw_x_step in -ring..=ring {
                let x_step = raw_x_step.checked_mul(order_multiplier)?;
                let x = x_step.checked_mul(step)?;
                for y in [
                    search_bias.checked_mul(distance)?,
                    (-search_bias).checked_mul(distance)?,
                ] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    *candidate_checks = candidate_checks.saturating_add(1);
                    if reservations.is_clear_with_radius(candidate, collision_radius)
                        && legal_position(candidate)
                    {
                        return Some(candidate);
                    }
                }
            }
            for raw_y_step in (-ring + 1)..=(ring - 1) {
                let y_step = raw_y_step.checked_mul(order_multiplier)?;
                let y = y_step.checked_mul(step)?;
                for x in [
                    search_bias.checked_mul(distance)?,
                    (-search_bias).checked_mul(distance)?,
                ] {
                    let Some(candidate) = offset_point(origin, x, y) else {
                        continue;
                    };
                    *candidate_checks = candidate_checks.saturating_add(1);
                    if reservations.is_clear_with_radius(candidate, collision_radius)
                        && legal_position(candidate)
                    {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    pub(super) fn nearest_reachable_unit_attack_cell(
        &self,
        source_cell: NavCell,
        current: SimPoint,
        target: SimPoint,
        attack_range: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let cell_size = self.config.navigation_cell_size;
        let radius_cells = attack_range
            .saturating_add(cell_size - 1)
            .div_euclid(cell_size)
            .saturating_add(1);
        let target_cell = self.topology.cell_of_point(target);
        let range_sq = square_i32(attack_range);
        let mut best: Option<(u64, i32, i32, NavCell)> = None;

        for y in target_cell.y - radius_cells..=target_cell.y + radius_cells {
            for x in target_cell.x - radius_cells..=target_cell.x + radius_cells {
                let cell = NavCell::new(x, y);
                if !self.topology.contains(cell) {
                    continue;
                }
                let position = self.topology.center_of_cell(cell);
                if position.distance_sq(target) > range_sq
                    || !self.position_is_traversable_from(source_cell, position, collision_radius)
                {
                    continue;
                }
                let key = (current.distance_sq(position), y, x, cell);
                if best.is_none_or(|existing| key < existing) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, _, _, cell)| cell)
    }

    fn nearest_reachable_building_attack_cell(
        &self,
        source_cell: NavCell,
        current: SimPoint,
        footprint: BuildingFootprint,
        attack_range: i32,
        collision_radius: Option<i32>,
    ) -> Option<NavCell> {
        let cell_size = self.config.navigation_cell_size;
        let radius_cells = attack_range
            .saturating_add(cell_size - 1)
            .div_euclid(cell_size)
            .saturating_add(1);
        let range_sq = square_i32(attack_range);
        let mut best: Option<(u64, i32, i32, NavCell)> = None;
        let min_x = footprint.min_x.saturating_sub(radius_cells);
        let max_x = footprint.max_x().saturating_add(radius_cells);
        let min_y = footprint.min_y.saturating_sub(radius_cells);
        let max_y = footprint.max_y().saturating_add(radius_cells);

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let cell = NavCell::new(x, y);
                if !self.topology.contains(cell) {
                    continue;
                }
                let position = self.topology.center_of_cell(cell);
                if point_to_footprint_distance_sq(position, footprint, cell_size) > range_sq
                    || !self.position_is_traversable_from(source_cell, position, collision_radius)
                {
                    continue;
                }
                let key = (current.distance_sq(position), y, x, cell);
                if best.is_none_or(|existing| key < existing) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, _, _, cell)| cell)
    }

    pub(super) fn navigation_world_bounds(&self) -> (SimPoint, SimPoint) {
        let cell_size = i64::from(self.config.navigation_cell_size);
        let min_x = i64::from(self.config.navigation_min.x) * cell_size;
        let min_y = i64::from(self.config.navigation_min.y) * cell_size;
        let max_x = (i64::from(self.config.navigation_max.x) + 1) * cell_size - 1;
        let max_y = (i64::from(self.config.navigation_max.y) + 1) * cell_size - 1;
        (
            SimPoint::new(
                i32::try_from(min_x).expect("navigation minimum x overflow"),
                i32::try_from(min_y).expect("navigation minimum y overflow"),
            ),
            SimPoint::new(
                i32::try_from(max_x).expect("navigation maximum x overflow"),
                i32::try_from(max_y).expect("navigation maximum y overflow"),
            ),
        )
    }

    fn position_is_legal_for_unit(
        &self,
        unit: &UnitSnapshot,
        original_cell: NavCell,
        candidate: SimPoint,
    ) -> bool {
        match unit.movement_class {
            MovementClass::Ground => self.position_is_traversable_from(
                original_cell,
                candidate,
                unit.collision_radius_override,
            ),
            MovementClass::Air => self.air_position_is_traversable_from(
                self.air_topology.cell_of_point(unit.position),
                candidate,
                unit.collision_radius,
            ),
        }
    }

    fn position_is_traversable_from(
        &self,
        original_cell: NavCell,
        candidate: SimPoint,
        collision_radius: Option<i32>,
    ) -> bool {
        let Some(component) = self.topology.component_id(original_cell) else {
            return false;
        };
        if let Some(collision_radius) = collision_radius {
            self.topology
                .circle_is_traversable_in_component(candidate, collision_radius, component)
        } else {
            self.topology
                .component_id(self.topology.cell_of_point(candidate))
                == Some(component)
        }
    }

    pub(super) fn air_position_is_traversable_from(
        &self,
        original_cell: NavCell,
        candidate: SimPoint,
        collision_radius: i32,
    ) -> bool {
        let Some(component) = self.air_topology.component_id(original_cell) else {
            return false;
        };
        self.air_topology
            .circle_is_traversable_in_component(candidate, collision_radius, component)
    }

    fn valid_separated_position(
        &self,
        original_cell: NavCell,
        desired: SimPoint,
        offset: SimPoint,
        unit: &UnitSnapshot,
    ) -> SimPoint {
        let candidates = [
            SimPoint::new(
                desired
                    .x
                    .checked_add(offset.x)
                    .expect("separation x overflow"),
                desired
                    .y
                    .checked_add(offset.y)
                    .expect("separation y overflow"),
            ),
            SimPoint::new(
                desired
                    .x
                    .checked_add(offset.x)
                    .expect("separation x overflow"),
                desired.y,
            ),
            SimPoint::new(
                desired.x,
                desired
                    .y
                    .checked_add(offset.y)
                    .expect("separation y overflow"),
            ),
            desired,
        ];

        candidates
            .into_iter()
            .find(|candidate| self.position_is_legal_for_unit(unit, original_cell, *candidate))
            .unwrap_or(desired)
    }
}

#[derive(Debug, Clone, Copy)]
struct NavigationRoute {
    next_cell: Option<NavCell>,
    navigation_route_step: bool,
    used_a_star: bool,
    a_star_cache_hit: bool,
    a_star_expanded_nodes: usize,
    cache_insert: Option<PursuitCacheInsert>,
}

impl NavigationRoute {
    const fn at(cell: NavCell) -> Self {
        Self {
            next_cell: Some(cell),
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }

    const fn none() -> Self {
        Self {
            next_cell: None,
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PursuitCacheInsert {
    from: NavCell,
    target: NavCell,
    collision_radius: Option<i32>,
    route_bias: i8,
    next: NavCell,
}

#[derive(Debug, Clone, Copy)]
struct MovementDecision {
    position: SimPoint,
    pursuit_step: bool,
    pursuit_target: Option<SimId>,
    attack_goal: Option<SimPoint>,
    navigation_route_step: bool,
    used_a_star: bool,
    a_star_cache_hit: bool,
    a_star_expanded_nodes: usize,
    cache_insert: Option<PursuitCacheInsert>,
}

impl MovementDecision {
    const fn stationary(position: SimPoint) -> Self {
        Self {
            position,
            pursuit_step: false,
            pursuit_target: None,
            attack_goal: None,
            navigation_route_step: false,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
            cache_insert: None,
        }
    }
}

fn navigation_goal(unit: &UnitSnapshot, decision: &MovementDecision) -> NavigationGoal {
    if let Some(target) = decision.pursuit_target {
        NavigationGoal::Target(target)
    } else if decision.position != unit.position {
        NavigationGoal::Objective(unit.team)
    } else {
        NavigationGoal::None
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct MovementMetrics {
    pub(super) intent: Duration,
    pub(super) crowd_separation: Duration,
    pub(super) hard_collision: Duration,
    pub(super) collision_fallback_search: Duration,
    pub(super) collision_fallback_searches: usize,
    pub(super) collision_fallback_candidate_checks: usize,
    pub(super) collision_fallback_max_ring: u32,
    pub(super) crowd_and_collision: Duration,
    pub(super) pursuit_steps: usize,
    pub(super) navigation_route_steps: usize,
    pub(super) movement_intents: usize,
    pub(super) movement_blocked: usize,
    pub(super) objective_move_intents: usize,
    pub(super) a_star_fallbacks: usize,
    pub(super) a_star_cache_hits: usize,
    pub(super) a_star_expanded_nodes: usize,
}

fn movement_collision_partition(movement_class: MovementClass) -> SpatialPartition {
    let component = match movement_class {
        MovementClass::Ground => 0,
        MovementClass::Air => 1,
    };
    SpatialPartition::new(0, component)
}

fn effective_movement_speed(unit: &UnitSnapshot) -> i32 {
    let count = usize::from(unit.status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let percent = unit.status.movement_modifiers[..count]
        .iter()
        .fold(100_i32, |total, modifier| {
            total
                .checked_add(i32::from(modifier.percent_delta))
                .expect("movement percentage overflow")
        })
        .clamp(0, 1_000);
    i32::try_from(i64::from(unit.movement.speed_per_tick) * i64::from(percent) / 100)
        .expect("effective movement speed overflowed validated bounds")
}

fn offset_point(point: SimPoint, x: i32, y: i32) -> Option<SimPoint> {
    Some(SimPoint::new(
        point.x.checked_add(x)?,
        point.y.checked_add(y)?,
    ))
}

fn exact_overlap_direction(a: SimId, b: SimId) -> (i32, i32) {
    debug_assert_ne!(a, b);
    let (low, high, sign) = if a < b { (a.0, b.0, -1) } else { (b.0, a.0, 1) };
    let axis = (low.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ high.rotate_left(17)) & 1;
    if axis == 0 { (sign, 0) } else { (0, sign) }
}

fn sidestep_sign(id: SimId) -> i32 {
    let mixed = id.0 ^ id.0.rotate_left(21) ^ 0x9e37_79b9_7f4a_7c15;
    if mixed & 1 == 0 { -1 } else { 1 }
}

fn perpendicular_step_with_side(
    side: i8,
    from: SimPoint,
    toward: SimPoint,
    distance: i32,
) -> SimPoint {
    debug_assert!(side == -1 || side == 1);
    let dx = i64::from(toward.x) - i64::from(from.x);
    let dy = i64::from(toward.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return SimPoint::new(0, i32::from(side) * distance);
    }
    let side = i64::from(side);
    let raw = SimPoint::new(
        i32::try_from(-dy * side).expect("sidestep x overflow"),
        i32::try_from(dx * side).expect("sidestep y overflow"),
    );
    SimPoint::new(0, 0).step_towards(raw, distance)
}

fn pursuit_arc_step(
    side: i8,
    from: SimPoint,
    toward: SimPoint,
    distance: i32,
    forward: bool,
) -> SimPoint {
    debug_assert!(side == -1 || side == 1);
    let dx = i64::from(toward.x) - i64::from(from.x);
    let dy = i64::from(toward.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return perpendicular_step_with_side(side, from, toward, distance);
    }
    let side = i64::from(side);
    let forward_sign = if forward { 1_i64 } else { -1_i64 };
    let raw_x = dx * forward_sign - dy * side;
    let raw_y = dy * forward_sign + dx * side;
    let raw = SimPoint::new(
        i32::try_from(raw_x).expect("pursuit arc x overflow"),
        i32::try_from(raw_y).expect("pursuit arc y overflow"),
    );
    SimPoint::new(0, 0).step_towards(raw, distance)
}

fn building_attack_envelope_goal(
    source: SimPoint,
    footprint: BuildingFootprint,
    max_range: i32,
    cell_size: i32,
) -> SimPoint {
    let closest = closest_point_on_footprint(source, footprint, cell_size);
    point_attack_envelope_goal(source, closest, max_range)
}
