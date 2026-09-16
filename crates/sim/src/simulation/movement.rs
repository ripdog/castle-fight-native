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
        let legal_positions = self.enforce_hard_non_overlap(
            units,
            unit_health,
            navigation_states,
            &decisions,
            &desired_positions,
            &separated_positions,
        );
        let crowd_and_collision = separation_start.elapsed();
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
}
