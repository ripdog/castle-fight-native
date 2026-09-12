use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

use crate::{components::BuildingFootprint, math::SimPoint};

const UNREACHABLE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NavCell {
    pub x: i32,
    pub y: i32,
}

impl NavCell {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PursuitStep {
    pub next_cell: Option<NavCell>,
    pub used_a_star: bool,
    pub a_star_cache_hit: bool,
    pub a_star_expanded_nodes: usize,
}

impl PursuitStep {
    const fn none() -> Self {
        Self {
            next_cell: None,
            used_a_star: false,
            a_star_cache_hit: false,
            a_star_expanded_nodes: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TopologyGrid {
    cell_size: i32,
    min: NavCell,
    width: usize,
    height: usize,
    blocked: Vec<bool>,
    component: Vec<u32>,
    objective_distance: [Vec<u32>; 2],
}

impl TopologyGrid {
    #[must_use]
    pub fn build(
        cell_size: i32,
        min: NavCell,
        max: NavCell,
        buildings: impl IntoIterator<Item = BuildingFootprint>,
        objectives: [SimPoint; 2],
    ) -> Self {
        assert!(cell_size > 0);
        assert!(max.x >= min.x && max.y >= min.y);

        let width = usize::try_from(max.x - min.x + 1).expect("navigation width overflow");
        let height = usize::try_from(max.y - min.y + 1).expect("navigation height overflow");
        let len = width
            .checked_mul(height)
            .expect("navigation grid too large");
        let objective = objectives.map(|point| {
            NavCell::new(point.x.div_euclid(cell_size), point.y.div_euclid(cell_size))
        });
        let mut grid = Self {
            cell_size,
            min,
            width,
            height,
            blocked: vec![false; len],
            component: vec![UNREACHABLE; len],
            objective_distance: [vec![UNREACHABLE; len], vec![UNREACHABLE; len]],
        };

        for footprint in buildings {
            for y in footprint.min_y..=footprint.max_y() {
                for x in footprint.min_x..=footprint.max_x() {
                    if let Some(index) = grid.index(NavCell::new(x, y)) {
                        grid.blocked[index] = true;
                    }
                }
            }
        }

        grid.rebuild_components();
        for (team, objective) in objective.into_iter().enumerate() {
            grid.objective_distance[team] = grid.distance_field(objective);
        }
        grid
    }

    #[must_use]
    pub fn cell_of_point(&self, point: SimPoint) -> NavCell {
        NavCell::new(
            point.x.div_euclid(self.cell_size),
            point.y.div_euclid(self.cell_size),
        )
    }

    #[must_use]
    pub fn center_of_cell(&self, cell: NavCell) -> SimPoint {
        let half = self.cell_size / 2;
        SimPoint::new(
            cell.x
                .checked_mul(self.cell_size)
                .and_then(|value| value.checked_add(half))
                .expect("navigation x coordinate overflow"),
            cell.y
                .checked_mul(self.cell_size)
                .and_then(|value| value.checked_add(half))
                .expect("navigation y coordinate overflow"),
        )
    }

    #[must_use]
    pub fn contains(&self, cell: NavCell) -> bool {
        self.index(cell).is_some()
    }

    #[must_use]
    pub fn is_blocked(&self, cell: NavCell) -> bool {
        self.index(cell).is_none_or(|index| self.blocked[index])
    }

    #[must_use]
    pub fn component_id(&self, cell: NavCell) -> Option<u32> {
        let index = self.index(cell)?;
        (!self.blocked[index] && self.component[index] != UNREACHABLE)
            .then_some(self.component[index])
    }

    #[must_use]
    pub fn same_component(&self, a: NavCell, b: NavCell) -> bool {
        let Some(a) = self.index(a) else {
            return false;
        };
        let Some(b) = self.index(b) else {
            return false;
        };
        !self.blocked[a]
            && !self.blocked[b]
            && self.component[a] != UNREACHABLE
            && self.component[a] == self.component[b]
    }

    #[must_use]
    pub fn circle_is_traversable_in_component(
        &self,
        center: SimPoint,
        radius: i32,
        component: u32,
    ) -> bool {
        debug_assert!(radius >= 0);
        let Some(center_component) = self.component_id(self.cell_of_point(center)) else {
            return false;
        };
        if center_component != component {
            return false;
        }
        if radius == 0 {
            return true;
        }

        let radius_i64 = i64::from(radius);
        let min_world_x = i64::from(self.min.x) * i64::from(self.cell_size);
        let min_world_y = i64::from(self.min.y) * i64::from(self.cell_size);
        let max_world_x = (i64::from(self.min.x) + self.width as i64) * i64::from(self.cell_size);
        let max_world_y = (i64::from(self.min.y) + self.height as i64) * i64::from(self.cell_size);
        let center_x = i64::from(center.x);
        let center_y = i64::from(center.y);
        if center_x - radius_i64 < min_world_x
            || center_y - radius_i64 < min_world_y
            || center_x + radius_i64 > max_world_x
            || center_y + radius_i64 > max_world_y
        {
            return false;
        }

        let min_cell = self.cell_of_point(SimPoint::new(
            center.x.saturating_sub(radius),
            center.y.saturating_sub(radius),
        ));
        let max_cell = self.cell_of_point(SimPoint::new(
            center.x.saturating_add(radius),
            center.y.saturating_add(radius),
        ));
        let radius_sq = (radius_i64 * radius_i64) as u64;
        for y in min_cell.y..=max_cell.y {
            for x in min_cell.x..=max_cell.x {
                let cell = NavCell::new(x, y);
                if cell_rect_min_distance_sq(center, cell, self.cell_size) >= radius_sq {
                    continue;
                }
                let Some(index) = self.index(cell) else {
                    return false;
                };
                if self.blocked[index]
                    || self.component[index] == UNREACHABLE
                    || self.component[index] != component
                {
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn objective_step_with_bias(
        &self,
        team: u8,
        from: NavCell,
        bias: i32,
    ) -> Option<NavCell> {
        let field = self.objective_distance.get(usize::from(team))?;
        self.step_from_distance_field_with_bias(from, field, bias)
    }

    pub(crate) fn objective_distance_field_with_radius(&self, team: u8, radius: i32) -> Vec<u32> {
        debug_assert!(radius >= 0);
        let Some(base_field) = self.objective_distance.get(usize::from(team)) else {
            return vec![UNREACHABLE; self.blocked.len()];
        };

        let mut valid = vec![false; self.blocked.len()];
        for (index, is_valid) in valid.iter_mut().enumerate() {
            let cell = self.cell_from_index(index);
            let Some(component) = self.component_id(cell) else {
                continue;
            };
            *is_valid = self.circle_is_traversable_in_component(
                self.center_of_cell(cell),
                radius,
                component,
            );
        }

        let mut radius_component = vec![UNREACHABLE; self.blocked.len()];
        let mut component_best = Vec::new();
        let mut queue = VecDeque::new();
        let mut next_component = 0u32;
        for start in 0..self.blocked.len() {
            if !valid[start] || radius_component[start] != UNREACHABLE {
                continue;
            }
            let mut best = UNREACHABLE;
            radius_component[start] = next_component;
            queue.push_back(start);
            while let Some(current) = queue.pop_front() {
                best = best.min(base_field[current]);
                let cell = self.cell_from_index(current);
                for neighbor in self.neighbors(cell).into_iter().flatten() {
                    let Some(next) = self.index(neighbor) else {
                        continue;
                    };
                    if !valid[next] || radius_component[next] != UNREACHABLE {
                        continue;
                    }
                    radius_component[next] = next_component;
                    queue.push_back(next);
                }
            }
            component_best.push(best);
            next_component = next_component
                .checked_add(1)
                .expect("too many radius-aware navigation components");
        }

        let mut result = vec![UNREACHABLE; self.blocked.len()];
        for index in 0..self.blocked.len() {
            let component = radius_component[index];
            if component == UNREACHABLE {
                continue;
            }
            let best = component_best
                [usize::try_from(component).expect("radius-aware component index overflow")];
            if best != UNREACHABLE && base_field[index] == best {
                result[index] = 0;
                queue.push_back(index);
            }
        }

        while let Some(current) = queue.pop_front() {
            let next_distance = result[current]
                .checked_add(1)
                .expect("radius-aware objective field overflow");
            let component = radius_component[current];
            let cell = self.cell_from_index(current);
            for neighbor in self.neighbors(cell).into_iter().flatten() {
                let Some(next) = self.index(neighbor) else {
                    continue;
                };
                if radius_component[next] != component || result[next] != UNREACHABLE {
                    continue;
                }
                result[next] = next_distance;
                queue.push_back(next);
            }
        }
        result
    }

    pub(crate) fn step_from_distance_field_with_bias(
        &self,
        from: NavCell,
        field: &[u32],
        bias: i32,
    ) -> Option<NavCell> {
        debug_assert!(bias == -1 || bias == 1);
        let from_index = self.index(from)?;
        let current = *field.get(from_index)?;
        if current == UNREACHABLE || current == 0 {
            return None;
        }
        self.neighbors(from)
            .into_iter()
            .flatten()
            .filter_map(|cell| self.index(cell).map(|index| (field[index], cell)))
            .filter(|(distance, _)| *distance < current)
            .min_by_key(|(distance, cell)| {
                let bias = i64::from(bias);
                (
                    *distance,
                    -bias * i64::from(cell.y),
                    -bias * i64::from(cell.x),
                )
            })
            .map(|(_, cell)| cell)
    }

    #[must_use]
    pub fn nearest_reachable_perimeter_cell(
        &self,
        from: NavCell,
        footprint: BuildingFootprint,
    ) -> Option<NavCell> {
        let mut best: Option<(i64, NavCell)> = None;
        let min_x = footprint.min_x - 1;
        let max_x = footprint.max_x() + 1;
        let min_y = footprint.min_y - 1;
        let max_y = footprint.max_y() + 1;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if x > min_x && x < max_x && y > min_y && y < max_y {
                    continue;
                }
                let cell = NavCell::new(x, y);
                if self.is_blocked(cell) || !self.same_component(from, cell) {
                    continue;
                }

                let dx = i64::from(cell.x - from.x);
                let dy = i64::from(cell.y - from.y);
                let key = (dx * dx + dy * dy, cell);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            }
        }

        best.map(|(_, cell)| cell)
    }

    #[must_use]
    pub fn pursuit_step(
        &self,
        from: NavCell,
        target: NavCell,
        cached_fallback: Option<NavCell>,
    ) -> PursuitStep {
        if !self.same_component(from, target) {
            return PursuitStep::none();
        }
        if from == target {
            return PursuitStep {
                next_cell: Some(from),
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
            };
        }

        if let Some(greedy) = self.greedy_route_first_step(from, target) {
            return PursuitStep {
                next_cell: Some(greedy),
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
            };
        }

        if let Some(next_cell) = cached_fallback {
            return PursuitStep {
                next_cell: Some(next_cell),
                used_a_star: true,
                a_star_cache_hit: true,
                a_star_expanded_nodes: 0,
            };
        }

        let (next_cell, a_star_expanded_nodes) = self.a_star_first_step(from, target);
        PursuitStep {
            next_cell,
            used_a_star: true,
            a_star_cache_hit: false,
            a_star_expanded_nodes,
        }
    }

    #[must_use]
    pub fn pursuit_step_with_radius(
        &self,
        from: NavCell,
        target: NavCell,
        cached_fallback: Option<NavCell>,
        radius: i32,
    ) -> PursuitStep {
        if !self.same_component(from, target) {
            return PursuitStep::none();
        }
        let Some(component) = self.component_id(from) else {
            return PursuitStep::none();
        };
        if from == target {
            return if self.circle_is_traversable_in_component(
                self.center_of_cell(from),
                radius,
                component,
            ) {
                PursuitStep {
                    next_cell: Some(from),
                    used_a_star: false,
                    a_star_cache_hit: false,
                    a_star_expanded_nodes: 0,
                }
            } else {
                PursuitStep::none()
            };
        }

        if let Some(greedy) = self.greedy_route_first_step_with_radius(from, target, radius) {
            return PursuitStep {
                next_cell: Some(greedy),
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
            };
        }

        if let Some(next_cell) = cached_fallback
            && self.circle_is_traversable_in_component(
                self.center_of_cell(next_cell),
                radius,
                component,
            )
        {
            return PursuitStep {
                next_cell: Some(next_cell),
                used_a_star: true,
                a_star_cache_hit: true,
                a_star_expanded_nodes: 0,
            };
        }

        let (next_cell, a_star_expanded_nodes) =
            self.a_star_first_step_with_radius(from, target, radius);
        PursuitStep {
            next_cell,
            used_a_star: true,
            a_star_cache_hit: false,
            a_star_expanded_nodes,
        }
    }

    fn greedy_route_first_step(&self, from: NavCell, target: NavCell) -> Option<NavCell> {
        let mut current = from;
        let mut first = None;
        while current != target {
            let current_distance = cell_distance_sq(current, target);
            let next = self
                .neighbors(current)
                .into_iter()
                .flatten()
                .filter(|cell| self.same_component(from, *cell))
                .min_by_key(|cell| (cell_distance_sq(*cell, target), cell.y, cell.x))
                .filter(|cell| cell_distance_sq(*cell, target) < current_distance)?;
            first.get_or_insert(next);
            current = next;
        }
        first
    }

    fn greedy_route_first_step_with_radius(
        &self,
        from: NavCell,
        target: NavCell,
        radius: i32,
    ) -> Option<NavCell> {
        let component = self.component_id(from)?;
        let mut current = from;
        let mut first = None;
        while current != target {
            let current_distance = cell_distance_sq(current, target);
            let next = self
                .neighbors(current)
                .into_iter()
                .flatten()
                .filter(|cell| {
                    self.circle_is_traversable_in_component(
                        self.center_of_cell(*cell),
                        radius,
                        component,
                    )
                })
                .min_by_key(|cell| (cell_distance_sq(*cell, target), cell.y, cell.x))
                .filter(|cell| cell_distance_sq(*cell, target) < current_distance)?;
            first.get_or_insert(next);
            current = next;
        }
        first
    }

    fn a_star_first_step(&self, from: NavCell, target: NavCell) -> (Option<NavCell>, usize) {
        let Some(start) = self.index(from) else {
            return (None, 0);
        };
        let Some(goal) = self.index(target) else {
            return (None, 0);
        };
        let mut g_score = vec![u32::MAX; self.blocked.len()];
        let mut came_from = vec![usize::MAX; self.blocked.len()];
        let mut open = BinaryHeap::new();

        g_score[start] = 0;
        open.push(Reverse((
            manhattan(from, target),
            0u32,
            from.y,
            from.x,
            start,
        )));

        let mut expanded_nodes = 0usize;
        while let Some(Reverse((_, cost, _, _, current))) = open.pop() {
            if cost != g_score[current] {
                continue;
            }
            expanded_nodes += 1;
            if current == goal {
                let mut step = goal;
                while came_from[step] != start {
                    let parent = came_from[step];
                    if parent == usize::MAX {
                        return (None, expanded_nodes);
                    }
                    step = parent;
                }
                return (Some(self.cell_from_index(step)), expanded_nodes);
            }

            let cell = self.cell_from_index(current);
            for neighbor in self.neighbors(cell).into_iter().flatten() {
                let Some(next) = self.index(neighbor) else {
                    continue;
                };
                if self.blocked[next] || self.component[next] != self.component[start] {
                    continue;
                }
                let next_cost = cost.checked_add(1).expect("navigation path cost overflow");
                if next_cost >= g_score[next] {
                    continue;
                }

                g_score[next] = next_cost;
                came_from[next] = current;
                let estimate = next_cost
                    .checked_add(manhattan(neighbor, target))
                    .expect("navigation path estimate overflow");
                open.push(Reverse((estimate, next_cost, neighbor.y, neighbor.x, next)));
            }
        }

        (None, expanded_nodes)
    }

    fn a_star_first_step_with_radius(
        &self,
        from: NavCell,
        target: NavCell,
        radius: i32,
    ) -> (Option<NavCell>, usize) {
        let Some(start) = self.index(from) else {
            return (None, 0);
        };
        let Some(goal) = self.index(target) else {
            return (None, 0);
        };
        let Some(component) = self.component_id(from) else {
            return (None, 0);
        };
        if !self.circle_is_traversable_in_component(self.center_of_cell(target), radius, component)
        {
            return (None, 0);
        }

        let mut g_score = vec![u32::MAX; self.blocked.len()];
        let mut came_from = vec![usize::MAX; self.blocked.len()];
        let mut open = BinaryHeap::new();
        g_score[start] = 0;
        open.push(Reverse((
            manhattan(from, target),
            0u32,
            from.y,
            from.x,
            start,
        )));

        let mut expanded_nodes = 0usize;
        while let Some(Reverse((_, cost, _, _, current))) = open.pop() {
            if cost != g_score[current] {
                continue;
            }
            expanded_nodes += 1;
            if current == goal {
                let mut step = goal;
                while came_from[step] != start {
                    let parent = came_from[step];
                    if parent == usize::MAX {
                        return (None, expanded_nodes);
                    }
                    step = parent;
                }
                return (Some(self.cell_from_index(step)), expanded_nodes);
            }

            let cell = self.cell_from_index(current);
            for neighbor in self.neighbors(cell).into_iter().flatten() {
                let Some(next) = self.index(neighbor) else {
                    continue;
                };
                if !self.circle_is_traversable_in_component(
                    self.center_of_cell(neighbor),
                    radius,
                    component,
                ) {
                    continue;
                }
                let next_cost = cost.checked_add(1).expect("navigation path cost overflow");
                if next_cost >= g_score[next] {
                    continue;
                }
                g_score[next] = next_cost;
                came_from[next] = current;
                let estimate = next_cost
                    .checked_add(manhattan(neighbor, target))
                    .expect("navigation path estimate overflow");
                open.push(Reverse((estimate, next_cost, neighbor.y, neighbor.x, next)));
            }
        }
        (None, expanded_nodes)
    }

    fn rebuild_components(&mut self) {
        let mut next_component = 0u32;
        let mut queue = VecDeque::new();

        for index in 0..self.blocked.len() {
            if self.blocked[index] || self.component[index] != UNREACHABLE {
                continue;
            }

            self.component[index] = next_component;
            queue.push_back(index);
            while let Some(current) = queue.pop_front() {
                let cell = self.cell_from_index(current);
                for neighbor in self.neighbors(cell).into_iter().flatten() {
                    let Some(neighbor_index) = self.index(neighbor) else {
                        continue;
                    };
                    if self.blocked[neighbor_index] || self.component[neighbor_index] != UNREACHABLE
                    {
                        continue;
                    }
                    self.component[neighbor_index] = next_component;
                    queue.push_back(neighbor_index);
                }
            }
            next_component = next_component
                .checked_add(1)
                .expect("too many nav components");
        }
    }

    fn distance_field(&self, destination: NavCell) -> Vec<u32> {
        let mut result = vec![UNREACHABLE; self.blocked.len()];
        let Some(destination_index) = self.index(destination) else {
            return result;
        };
        if self.blocked[destination_index] {
            return result;
        }

        let mut queue = VecDeque::new();
        result[destination_index] = 0;
        queue.push_back(destination_index);

        while let Some(current) = queue.pop_front() {
            let current_distance = result[current];
            let cell = self.cell_from_index(current);
            for neighbor in self.neighbors(cell).into_iter().flatten() {
                let Some(neighbor_index) = self.index(neighbor) else {
                    continue;
                };
                if self.blocked[neighbor_index] || result[neighbor_index] != UNREACHABLE {
                    continue;
                }
                result[neighbor_index] = current_distance + 1;
                queue.push_back(neighbor_index);
            }
        }
        result
    }

    fn neighbors(&self, cell: NavCell) -> [Option<NavCell>; 4] {
        [
            Some(NavCell::new(cell.x + 1, cell.y)),
            Some(NavCell::new(cell.x, cell.y + 1)),
            Some(NavCell::new(cell.x - 1, cell.y)),
            Some(NavCell::new(cell.x, cell.y - 1)),
        ]
    }

    fn index(&self, cell: NavCell) -> Option<usize> {
        let local_x = cell.x - self.min.x;
        let local_y = cell.y - self.min.y;
        if local_x < 0 || local_y < 0 {
            return None;
        }
        let x = usize::try_from(local_x).ok()?;
        let y = usize::try_from(local_y).ok()?;
        if x >= self.width || y >= self.height {
            return None;
        }
        y.checked_mul(self.width)?.checked_add(x)
    }

    fn cell_from_index(&self, index: usize) -> NavCell {
        let y = index / self.width;
        let x = index % self.width;
        NavCell::new(self.min.x + x as i32, self.min.y + y as i32)
    }
}

fn cell_rect_min_distance_sq(center: SimPoint, cell: NavCell, cell_size: i32) -> u64 {
    let min_x = i64::from(cell.x) * i64::from(cell_size);
    let min_y = i64::from(cell.y) * i64::from(cell_size);
    let max_x = min_x + i64::from(cell_size);
    let max_y = min_y + i64::from(cell_size);
    let center_x = i64::from(center.x);
    let center_y = i64::from(center.y);
    let closest_x = center_x.clamp(min_x, max_x);
    let closest_y = center_y.clamp(min_y, max_y);
    let dx = center_x - closest_x;
    let dy = center_y - closest_y;
    (dx * dx + dy * dy) as u64
}

fn cell_distance_sq(a: NavCell, b: NavCell) -> i64 {
    let dx = i64::from(a.x - b.x);
    let dy = i64::from(a.y - b.y);
    dx * dx + dy * dy
}

fn manhattan(a: NavCell, b: NavCell) -> u32 {
    let dx = a.x.abs_diff(b.x);
    let dy = a.y.abs_diff(b.y);
    dx.checked_add(dy).expect("navigation heuristic overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::SUBUNITS_PER_WORLD_UNIT;

    #[test]
    fn objective_ties_distribute_across_both_lateral_directions() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(5, 4),
            [],
            [
                SimPoint::new(4 * cell, 2 * cell),
                SimPoint::new(cell, 2 * cell),
            ],
        );

        let north = NavCell::new(1, 1);
        assert_eq!(
            grid.objective_step_with_bias(0, north, -1),
            Some(NavCell::new(2, 1))
        );
        assert_eq!(
            grid.objective_step_with_bias(0, north, 1),
            Some(NavCell::new(1, 2))
        );

        let south = NavCell::new(1, 3);
        assert_eq!(
            grid.objective_step_with_bias(0, south, -1),
            Some(NavCell::new(1, 2))
        );
        assert_eq!(
            grid.objective_step_with_bias(0, south, 1),
            Some(NavCell::new(2, 3))
        );
    }

    #[test]
    fn opposite_objective_biases_take_opposite_sides_of_symmetric_blocker() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(8, 8),
            [BuildingFootprint::new(4, 3, 1, 3)],
            [
                SimPoint::new(8 * cell, 4 * cell),
                SimPoint::new(0, 4 * cell),
            ],
        );

        let trace = |bias| {
            let mut current = NavCell::new(1, 4);
            let mut min_y = current.y;
            let mut max_y = current.y;
            for _ in 0..24 {
                let Some(next) = grid.objective_step_with_bias(0, current, bias) else {
                    break;
                };
                current = next;
                min_y = min_y.min(current.y);
                max_y = max_y.max(current.y);
            }
            (current, min_y, max_y)
        };

        let lower = trace(-1);
        let upper = trace(1);
        assert_eq!(lower.0, NavCell::new(8, 4));
        assert_eq!(upper.0, NavCell::new(8, 4));
        assert!(lower.1 <= 2, "low-coordinate bias did not use lower gap");
        assert!(upper.2 >= 6, "high-coordinate bias did not use upper gap");
    }

    #[test]
    fn collision_circle_respects_blocked_cell_edges_and_map_bounds() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(4, 4),
            [BuildingFootprint::new(2, 1, 1, 1)],
            [SimPoint::new(4 * cell, 4 * cell), SimPoint::new(0, 0)],
        );
        let center = SimPoint::new(3 * cell / 2, 3 * cell / 2);
        let component = grid.component_id(NavCell::new(1, 1)).unwrap();

        assert!(grid.circle_is_traversable_in_component(center, cell / 2, component));
        assert!(!grid.circle_is_traversable_in_component(center, cell / 2 + 1, component));
        assert!(!grid.circle_is_traversable_in_component(
            SimPoint::new(cell / 4, 3 * cell / 2),
            cell / 2,
            component
        ));
    }

    #[test]
    fn radius_aware_pursuit_routes_around_inflated_blocker_clearance() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, -2),
            NavCell::new(6, 8),
            [BuildingFootprint::new(3, 1, 1, 5)],
            [
                SimPoint::new(6 * cell, 3 * cell),
                SimPoint::new(0, 3 * cell),
            ],
        );

        let result = grid.pursuit_step_with_radius(
            NavCell::new(1, 3),
            NavCell::new(5, 3),
            None,
            cell / 2 + 1,
        );
        assert!(result.used_a_star);
        assert!(result.a_star_expanded_nodes > 0);
        assert!(result.next_cell.is_some());
    }

    #[test]
    fn radius_aware_objective_pursuit_detours_without_requiring_objective_clearance() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(8, 7),
            [BuildingFootprint::new(4, 1, 1, 4)],
            [
                SimPoint::new(8 * cell, 3 * cell),
                SimPoint::new(0, 3 * cell),
            ],
        );
        let from = NavCell::new(2, 3);
        let radius = cell / 2 + 1;

        let field = grid.objective_distance_field_with_radius(0, radius);
        let first = grid
            .step_from_distance_field_with_bias(from, &field, -1)
            .expect("radius-aware objective field should provide a detour");
        assert_ne!(first, NavCell::new(3, 3));

        let mut current = first;
        let mut path = vec![from, current];
        for _ in 0..12 {
            if current.x > 4 {
                break;
            }
            current = grid
                .step_from_distance_field_with_bias(current, &field, -1)
                .expect("radius-aware objective detour should remain reachable");
            path.push(current);
        }
        assert!(
            current.x > 4,
            "radius-aware objective pursuit never cleared the wall: {path:?}"
        );
    }

    #[test]
    fn pursuit_falls_back_to_a_star_when_greedy_progress_is_blocked() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(6, 4),
            [BuildingFootprint::new(3, 0, 1, 3)],
            [
                SimPoint::new(6 * cell, 2 * cell),
                SimPoint::new(0, 2 * cell),
            ],
        );

        let result = grid.pursuit_step(NavCell::new(2, 1), NavCell::new(4, 1), None);
        assert_eq!(result.next_cell, Some(NavCell::new(2, 2)));
        assert!(result.used_a_star);
        assert!(result.a_star_expanded_nodes > 0);
    }

    #[test]
    fn pursuit_keeps_following_detour_instead_of_greedy_backtracking() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(6, 4),
            [BuildingFootprint::new(3, 0, 1, 3)],
            [
                SimPoint::new(6 * cell, 2 * cell),
                SimPoint::new(0, 2 * cell),
            ],
        );
        let target = NavCell::new(4, 1);
        let mut current = NavCell::new(2, 1);
        let mut fallback_count = 0;

        for _ in 0..8 {
            if current == target {
                break;
            }
            let result = grid.pursuit_step(current, target, None);
            fallback_count += usize::from(result.used_a_star);
            current = result.next_cell.expect("detour should remain reachable");
        }

        assert_eq!(current, target);
        assert!(
            fallback_count >= 2,
            "wall detour should require sustained fallback"
        );
    }

    #[test]
    fn wall_splits_components() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(4, 4),
            [BuildingFootprint::new(2, 0, 1, 5)],
            [
                SimPoint::new(4 * cell, 2 * cell),
                SimPoint::new(0, 2 * cell),
            ],
        );

        assert!(!grid.same_component(NavCell::new(1, 2), NavCell::new(3, 2)));
        assert!(grid.same_component(NavCell::new(0, 0), NavCell::new(1, 4)));
    }
}
