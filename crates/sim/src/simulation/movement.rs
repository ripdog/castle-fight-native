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
