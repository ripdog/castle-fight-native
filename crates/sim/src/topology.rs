use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

use crate::{
    components::BuildingFootprint,
    math::{CellDivisor, SimPoint},
};

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
    divisor: CellDivisor,
    min: NavCell,
    width: usize,
    height: usize,
    blocked: Vec<bool>,
    component: Vec<u32>,
    /// Chebyshev clearance per cell: every cell within this many cells (Chebyshev distance) is
    /// in-grid, not blocked, and reachable. Recomputed whenever `blocked`/`component` change.
    clearance: Vec<u16>,
    objective_distance: [Vec<u32>; 2],
    /// Added to bias-adjusted cell coordinates so they pack into unsigned open-set key fields.
    /// Every in-grid coordinate satisfies `|coord| <= open_key_offset`; checked at build.
    open_key_offset: i64,
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
        let divisor = CellDivisor::new(cell_size);
        let objective = objectives
            .map(|point| NavCell::new(divisor.floor_div(point.x), divisor.floor_div(point.y)));
        let open_key_offset = [min.x, min.y, max.x, max.y]
            .into_iter()
            .map(|coord| i64::from(coord).abs())
            .max()
            .unwrap_or(0);
        assert!(
            2 * open_key_offset <= i64::from(u32::MAX),
            "navigation coordinates exceed open-set key range"
        );
        let mut grid = Self {
            cell_size,
            divisor,
            min,
            width,
            height,
            blocked: vec![false; len],
            component: vec![UNREACHABLE; len],
            clearance: vec![0; len],
            objective_distance: [vec![UNREACHABLE; len], vec![UNREACHABLE; len]],
            open_key_offset,
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
            self.divisor.floor_div(point.x),
            self.divisor.floor_div(point.y),
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
    pub fn circle_is_inside_bounds(&self, center: SimPoint, radius: i32) -> bool {
        debug_assert!(radius >= 0);
        let radius_i64 = i64::from(radius);
        let min_world_x = i64::from(self.min.x) * i64::from(self.cell_size);
        let min_world_y = i64::from(self.min.y) * i64::from(self.cell_size);
        let max_world_x = (i64::from(self.min.x) + self.width as i64) * i64::from(self.cell_size);
        let max_world_y = (i64::from(self.min.y) + self.height as i64) * i64::from(self.cell_size);
        let center_x = i64::from(center.x);
        let center_y = i64::from(center.y);
        center_x - radius_i64 >= min_world_x
            && center_y - radius_i64 >= min_world_y
            && center_x + radius_i64 <= max_world_x
            && center_y + radius_i64 <= max_world_y
    }

    #[must_use]
    pub fn circle_is_traversable_in_component(
        &self,
        center: SimPoint,
        radius: i32,
        component: u32,
    ) -> bool {
        debug_assert!(radius >= 0);
        let center_cell = self.cell_of_point(center);
        let Some(center_component) = self.component_id(center_cell) else {
            return false;
        };
        if center_component != component || !self.circle_is_inside_bounds(center, radius) {
            return false;
        }
        if radius == 0 {
            return true;
        }

        let radius_i64 = i64::from(radius);

        let min_cell = self.cell_of_point(SimPoint::new(
            center.x.saturating_sub(radius),
            center.y.saturating_sub(radius),
        ));
        let max_cell = self.cell_of_point(SimPoint::new(
            center.x.saturating_add(radius),
            center.y.saturating_add(radius),
        ));

        if self.clearance_covers_span(center_cell, min_cell, max_cell) {
            return true;
        }

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

    /// True when every cell of the inclusive span `[min, max]` lies within the clearance of
    /// `center`, which must itself be in the grid. Clearance k means every cell within Chebyshev
    /// distance k is in-grid, unblocked and reachable. Such a span is a filled rectangle of cells
    /// that is 4-connected (rebuild_components floods over 4-neighbours) and contains `center`,
    /// so all of its cells share `center`'s component.
    fn clearance_covers_span(&self, center: NavCell, min: NavCell, max: NavCell) -> bool {
        let Some(center_index) = self.index(center) else {
            return false;
        };
        let span_reach = (i64::from(center.x) - i64::from(min.x))
            .max(i64::from(max.x) - i64::from(center.x))
            .max(i64::from(center.y) - i64::from(min.y))
            .max(i64::from(max.y) - i64::from(center.y));
        i64::from(self.clearance[center_index]) >= span_reach
    }

    /// Validate the entire swept collision circle, not just its destination. Tangency is legal.
    pub(crate) fn circle_segment_is_traversable_in_component(
        &self,
        from: SimPoint,
        to: SimPoint,
        radius: i32,
        component: u32,
    ) -> bool {
        if !self.circle_is_traversable_in_component(from, radius, component)
            || !self.circle_is_traversable_in_component(to, radius, component)
        {
            return false;
        }
        let min = self.cell_of_point(SimPoint::new(
            from.x.min(to.x).saturating_sub(radius),
            from.y.min(to.y).saturating_sub(radius),
        ));
        let max = self.cell_of_point(SimPoint::new(
            from.x.max(to.x).saturating_add(radius),
            from.y.max(to.y).saturating_add(radius),
        ));
        // The swept span contains `from`'s cell, which lies in `component` once the endpoint
        // check passed. Clearance covering the span therefore proves every span cell is in
        // `component`, so the exact scan below could not find a foreign cell.
        if self.clearance_covers_span(self.cell_of_point(from), min, max) {
            return true;
        }
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let cell = NavCell::new(x, y);
                if self.component_id(cell) != Some(component)
                    && segment_overlaps_cell_circle(from, to, cell, self.cell_size, radius)
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

    /// Per-cell mask of cells whose center can host a circle of `radius` inside the cell's own
    /// component. Callers that route with a fixed radius use it through `center_circle_ok`, which
    /// answers the same question as `circle_is_traversable_in_component(center_of_cell(cell))`.
    pub(crate) fn center_traversable_mask(&self, radius: i32) -> Vec<bool> {
        debug_assert!(radius >= 0);
        let mut mask = vec![false; self.blocked.len()];
        for (index, is_valid) in mask.iter_mut().enumerate() {
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
        mask
    }

    pub(crate) fn objective_distance_field_with_radius(&self, team: u8, radius: i32) -> Vec<u32> {
        debug_assert!(radius >= 0);
        let Some(base_field) = self.objective_distance.get(usize::from(team)) else {
            return vec![UNREACHABLE; self.blocked.len()];
        };

        let valid = self.center_traversable_mask(radius);

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
        self.step_from_distance_field_with_filter(from, field, bias, |_| true)
    }

    pub(crate) fn step_from_distance_field_with_radius_bias(
        &self,
        from: NavCell,
        field: &[u32],
        radius: i32,
        bias: i32,
        center_mask: Option<&[bool]>,
    ) -> Option<NavCell> {
        let component = self.component_id(from)?;
        self.step_from_distance_field_with_filter(from, field, bias, |cell| {
            self.radius_transition_is_traversable(from, cell, radius, component, center_mask)
        })
    }

    fn step_from_distance_field_with_filter(
        &self,
        from: NavCell,
        field: &[u32],
        bias: i32,
        mut valid: impl FnMut(NavCell) -> bool,
    ) -> Option<NavCell> {
        debug_assert!(bias == -1 || bias == 1);
        let from_index = self.index(from)?;
        let current = *field.get(from_index)?;
        if current == UNREACHABLE || current == 0 {
            return None;
        }
        self.movement_neighbors(from)
            .into_iter()
            .flatten()
            .filter(|cell| valid(*cell))
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
        bias: i32,
    ) -> PursuitStep {
        debug_assert!(bias == -1 || bias == 1);
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

        if let Some(greedy) = self.greedy_route_first_step(from, target, bias) {
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

        let (next_cell, a_star_expanded_nodes) = self.a_star_first_step(from, target, bias);
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
        bias: i32,
        center_mask: Option<&[bool]>,
    ) -> PursuitStep {
        debug_assert!(bias == -1 || bias == 1);
        if !self.same_component(from, target) {
            return PursuitStep::none();
        }
        let Some(component) = self.component_id(from) else {
            return PursuitStep::none();
        };
        if from == target {
            return if self.center_circle_ok(from, radius, component, center_mask) {
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

        if let Some(greedy) =
            self.greedy_route_first_step_with_radius(from, target, radius, bias, center_mask)
        {
            return PursuitStep {
                next_cell: Some(greedy),
                used_a_star: false,
                a_star_cache_hit: false,
                a_star_expanded_nodes: 0,
            };
        }

        if let Some(next_cell) = cached_fallback
            && self.radius_transition_is_traversable(
                from,
                next_cell,
                radius,
                component,
                center_mask,
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
            self.a_star_first_step_with_radius(from, target, radius, bias, center_mask);
        PursuitStep {
            next_cell,
            used_a_star: true,
            a_star_cache_hit: false,
            a_star_expanded_nodes,
        }
    }

    fn greedy_route_first_step(
        &self,
        from: NavCell,
        target: NavCell,
        bias: i32,
    ) -> Option<NavCell> {
        debug_assert!(bias == -1 || bias == 1);
        let mut current = from;
        let mut first = None;
        while current != target {
            let current_distance = cell_distance_sq(current, target);
            let next = self
                .movement_neighbors(current)
                .into_iter()
                .flatten()
                .filter(|cell| self.same_component(from, *cell))
                .min_by_key(|cell| {
                    (
                        cell_distance_sq(*cell, target),
                        -i64::from(bias) * i64::from(cell.y),
                        -i64::from(bias) * i64::from(cell.x),
                    )
                })
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
        bias: i32,
        center_mask: Option<&[bool]>,
    ) -> Option<NavCell> {
        debug_assert!(bias == -1 || bias == 1);
        let component = self.component_id(from)?;
        let mut current = from;
        let mut first = None;
        while current != target {
            let current_distance = cell_distance_sq(current, target);
            let next = self
                .movement_neighbors(current)
                .into_iter()
                .flatten()
                .filter(|cell| {
                    self.radius_transition_is_traversable(
                        current,
                        *cell,
                        radius,
                        component,
                        center_mask,
                    )
                })
                .min_by_key(|cell| {
                    (
                        cell_distance_sq(*cell, target),
                        -i64::from(bias) * i64::from(cell.y),
                        -i64::from(bias) * i64::from(cell.x),
                    )
                })
                .filter(|cell| cell_distance_sq(*cell, target) < current_distance)?;
            first.get_or_insert(next);
            current = next;
        }
        first
    }

    fn a_star_first_step(
        &self,
        from: NavCell,
        target: NavCell,
        bias: i32,
    ) -> (Option<NavCell>, usize) {
        debug_assert!(bias == -1 || bias == 1);
        let Some(start) = self.index(from) else {
            return (None, 0);
        };
        let Some(goal) = self.index(target) else {
            return (None, 0);
        };

        with_a_star_scratch(self.blocked.len(), |scratch| {
            scratch.relax(start, 0, usize::MAX);
            scratch
                .open
                .push(self.open_entry(manhattan(from, target), 0, from, bias));

            let mut expanded_nodes = 0usize;
            while let Some(Reverse(key)) = scratch.open.pop() {
                let cost = open_entry_cost(key);
                let current = self.open_entry_index(key, bias);
                if cost != scratch.cost_of(current) {
                    continue;
                }
                expanded_nodes += 1;
                if current == goal {
                    let mut step = goal;
                    while scratch.parent_of(step) != start {
                        let parent = scratch.parent_of(step);
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
                    if next_cost >= scratch.cost_of(next) {
                        continue;
                    }

                    scratch.relax(next, next_cost, current);
                    let estimate = next_cost
                        .checked_add(manhattan(neighbor, target))
                        .expect("navigation path estimate overflow");
                    scratch
                        .open
                        .push(self.open_entry(estimate, next_cost, neighbor, bias));
                }
            }

            (None, expanded_nodes)
        })
    }

    fn a_star_first_step_with_radius(
        &self,
        from: NavCell,
        target: NavCell,
        radius: i32,
        bias: i32,
        center_mask: Option<&[bool]>,
    ) -> (Option<NavCell>, usize) {
        debug_assert!(bias == -1 || bias == 1);
        let Some(start) = self.index(from) else {
            return (None, 0);
        };
        let Some(goal) = self.index(target) else {
            return (None, 0);
        };
        let Some(component) = self.component_id(from) else {
            return (None, 0);
        };
        if !self.center_circle_ok(target, radius, component, center_mask) {
            return (None, 0);
        }

        with_a_star_scratch(self.blocked.len(), |scratch| {
            scratch.relax(start, 0, usize::MAX);
            scratch
                .open
                .push(self.open_entry(manhattan(from, target), 0, from, bias));

            let mut expanded_nodes = 0usize;
            while let Some(Reverse(key)) = scratch.open.pop() {
                let cost = open_entry_cost(key);
                let current = self.open_entry_index(key, bias);
                if cost != scratch.cost_of(current) {
                    continue;
                }
                expanded_nodes += 1;
                if current == goal {
                    let mut step = goal;
                    while scratch.parent_of(step) != start {
                        let parent = scratch.parent_of(step);
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
                    if !self.center_circle_ok(neighbor, radius, component, center_mask) {
                        continue;
                    }
                    let next_cost = cost.checked_add(1).expect("navigation path cost overflow");
                    if next_cost >= scratch.cost_of(next) {
                        continue;
                    }
                    scratch.relax(next, next_cost, current);
                    let estimate = next_cost
                        .checked_add(manhattan(neighbor, target))
                        .expect("navigation path estimate overflow");
                    scratch
                        .open
                        .push(self.open_entry(estimate, next_cost, neighbor, bias));
                }
            }
            (None, expanded_nodes)
        })
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
        self.rebuild_clearance();
    }

    /// Multi-source BFS over the king-move graph, which yields exact Chebyshev distances.
    /// Blocked, unreachable and out-of-grid cells are sources at distance zero. The grid is padded
    /// by one ring of out-of-grid cells so border cells see the outside as bad.
    fn rebuild_clearance(&mut self) {
        let padded_width = self.width + 2;
        let padded_height = self.height + 2;
        let mut distance = vec![UNREACHABLE; padded_width * padded_height];
        let mut queue = VecDeque::new();

        for py in 0..padded_height {
            for px in 0..padded_width {
                let is_bad =
                    if px == 0 || py == 0 || px == padded_width - 1 || py == padded_height - 1 {
                        true
                    } else {
                        let index = (py - 1) * self.width + (px - 1);
                        self.blocked[index] || self.component[index] == UNREACHABLE
                    };
                if is_bad {
                    let padded_index = py * padded_width + px;
                    distance[padded_index] = 0;
                    queue.push_back(padded_index);
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            let next_distance = distance[current] + 1;
            let cx = current % padded_width;
            let cy = current / padded_width;
            for ny in cy.saturating_sub(1)..=(cy + 1).min(padded_height - 1) {
                for nx in cx.saturating_sub(1)..=(cx + 1).min(padded_width - 1) {
                    let neighbor = ny * padded_width + nx;
                    if distance[neighbor] == UNREACHABLE {
                        distance[neighbor] = next_distance;
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // Clearance k means the nearest bad cell is at Chebyshev distance k + 1.
        // Bad cells report 0 (their value is never consulted).
        self.clearance.clear();
        self.clearance.reserve(self.width * self.height);
        for y in 0..self.height {
            let row_start = (y + 1) * padded_width + 1;
            self.clearance.extend(
                distance[row_start..row_start + self.width]
                    .iter()
                    .map(|&d| u16::try_from(d.saturating_sub(1)).unwrap_or(u16::MAX)),
            );
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

    fn movement_neighbors(&self, cell: NavCell) -> [Option<NavCell>; 8] {
        let east = NavCell::new(cell.x + 1, cell.y);
        let north = NavCell::new(cell.x, cell.y + 1);
        let west = NavCell::new(cell.x - 1, cell.y);
        let south = NavCell::new(cell.x, cell.y - 1);
        let east_open = !self.is_blocked(east);
        let north_open = !self.is_blocked(north);
        let west_open = !self.is_blocked(west);
        let south_open = !self.is_blocked(south);
        [
            Some(east),
            Some(north),
            Some(west),
            Some(south),
            (east_open && north_open).then_some(NavCell::new(cell.x + 1, cell.y + 1)),
            (west_open && north_open).then_some(NavCell::new(cell.x - 1, cell.y + 1)),
            (west_open && south_open).then_some(NavCell::new(cell.x - 1, cell.y - 1)),
            (east_open && south_open).then_some(NavCell::new(cell.x + 1, cell.y - 1)),
        ]
    }

    /// Whether a circle of `radius` centered in `cell` fits in `component`. With a mask (from
    /// `center_traversable_mask(radius)`) this is a table lookup; without one it runs the exact
    /// geometric check. Both answer identically.
    fn center_circle_ok(
        &self,
        cell: NavCell,
        radius: i32,
        component: u32,
        center_mask: Option<&[bool]>,
    ) -> bool {
        match center_mask {
            Some(mask) => self
                .index(cell)
                .is_some_and(|index| mask[index] && self.component[index] == component),
            None => self.circle_is_traversable_in_component(
                self.center_of_cell(cell),
                radius,
                component,
            ),
        }
    }

    fn radius_transition_is_traversable(
        &self,
        from: NavCell,
        to: NavCell,
        radius: i32,
        component: u32,
        center_mask: Option<&[bool]>,
    ) -> bool {
        if !self.center_circle_ok(to, radius, component, center_mask) {
            return false;
        }
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        if dx == 0 || dy == 0 {
            return true;
        }
        debug_assert_eq!(dx.abs(), 1);
        debug_assert_eq!(dy.abs(), 1);
        [
            NavCell::new(from.x + dx, from.y),
            NavCell::new(from.x, from.y + dy),
        ]
        .into_iter()
        .all(|side| self.center_circle_ok(side, radius, component, center_mask))
    }

    /// Open-set entry for an in-grid `cell` reached with `estimate` and `cost`.
    #[inline]
    fn open_entry(&self, estimate: u32, cost: u32, cell: NavCell, bias: i32) -> AStarOpenEntry {
        let sign = -i64::from(bias);
        let by = self.biased_open_coord(sign * i64::from(cell.y));
        let bx = self.biased_open_coord(sign * i64::from(cell.x));
        Reverse(
            (u128::from(estimate) << 96)
                | (u128::from(cost) << 64)
                | (u128::from(by) << 32)
                | u128::from(bx),
        )
    }

    /// Index of the cell named by a key produced by `open_entry` with the same `bias`.
    #[inline]
    fn open_entry_index(&self, key: u128, bias: i32) -> usize {
        let sign = -i64::from(bias);
        let by = i64::from((key >> 32) as u32) - self.open_key_offset;
        let bx = i64::from(key as u32) - self.open_key_offset;
        let cell = NavCell::new((sign * bx) as i32, (sign * by) as i32);
        self.index(cell)
            .expect("open-set key decodes to an in-grid cell")
    }

    #[inline]
    fn biased_open_coord(&self, value: i64) -> u32 {
        let biased = value + self.open_key_offset;
        debug_assert!((0..=i64::from(u32::MAX)).contains(&biased));
        biased as u32
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

/// Open-set entry packing `(estimate, cost, bias-adjusted y, bias-adjusted x)` into one key whose
/// integer order is the lexicographic order of that tuple (min-heap via `Reverse`). Each
/// bias-adjusted coordinate is `-bias * coord + open_key_offset`, which is injective per search, so
/// the key determines the cell and no separate index is needed. Two distinct cells never share a
/// key, so equal keys are always duplicate pushes of one cell.
type AStarOpenEntry = Reverse<u128>;

#[inline]
fn open_entry_cost(key: u128) -> u32 {
    (key >> 64) as u32
}

/// Reusable A* working memory. Entries are valid only when their stamp equals `generation`, so a
/// search never clears the full grid. Each worker thread keeps its own instance (movement runs in
/// a rayon pool), and no search re-enters `with_a_star_scratch`.
#[derive(Debug, Default)]
struct AStarScratch {
    g_score: Vec<u32>,
    came_from: Vec<usize>,
    stamp: Vec<u32>,
    generation: u32,
    open: BinaryHeap<AStarOpenEntry>,
}

impl AStarScratch {
    fn begin(&mut self, len: usize) {
        self.g_score.resize(len, u32::MAX);
        self.came_from.resize(len, usize::MAX);
        self.stamp.resize(len, 0);
        self.generation = match self.generation.checked_add(1) {
            Some(generation) => generation,
            None => {
                self.stamp.fill(0);
                1
            }
        };
        self.open.clear();
    }

    #[inline]
    fn cost_of(&self, index: usize) -> u32 {
        if self.stamp[index] == self.generation {
            self.g_score[index]
        } else {
            u32::MAX
        }
    }

    #[inline]
    fn parent_of(&self, index: usize) -> usize {
        if self.stamp[index] == self.generation {
            self.came_from[index]
        } else {
            usize::MAX
        }
    }

    #[inline]
    fn relax(&mut self, index: usize, cost: u32, parent: usize) {
        self.stamp[index] = self.generation;
        self.g_score[index] = cost;
        self.came_from[index] = parent;
    }
}

thread_local! {
    static A_STAR_SCRATCH: RefCell<AStarScratch> = RefCell::new(AStarScratch::default());
}

fn with_a_star_scratch<R>(len: usize, search: impl FnOnce(&mut AStarScratch) -> R) -> R {
    A_STAR_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.begin(len);
        search(&mut scratch)
    })
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

fn segment_overlaps_cell_circle(
    from: SimPoint,
    to: SimPoint,
    cell: NavCell,
    cell_size: i32,
    radius: i32,
) -> bool {
    let min_x = i64::from(cell.x) * i64::from(cell_size);
    let min_y = i64::from(cell.y) * i64::from(cell_size);
    let max_x = min_x + i64::from(cell_size);
    let max_y = min_y + i64::from(cell_size);
    let corners = [
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ];
    let dx = i128::from(to.x) - i128::from(from.x);
    let dy = i128::from(to.y) - i128::from(from.y);
    let crosses = corners.map(|(x, y)| {
        dx * (i128::from(y) - i128::from(from.y)) - dy * (i128::from(x) - i128::from(from.x))
    });
    // Separating axes for a line segment and an axis-aligned rectangle.
    let intersects = i64::from(from.x.max(to.x)) >= min_x
        && i64::from(from.x.min(to.x)) <= max_x
        && i64::from(from.y.max(to.y)) >= min_y
        && i64::from(from.y.min(to.y)) <= max_y
        && !crosses.iter().all(|cross| *cross > 0)
        && !crosses.iter().all(|cross| *cross < 0);
    if intersects {
        return true;
    }
    let radius_sq = i128::from(radius) * i128::from(radius);
    if i128::from(cell_rect_min_distance_sq(from, cell, cell_size)) < radius_sq
        || i128::from(cell_rect_min_distance_sq(to, cell, cell_size)) < radius_sq
    {
        return true;
    }
    let length_sq = dx * dx + dy * dy;
    if length_sq == 0 {
        return false;
    }
    // Otherwise the closest pair includes a rectangle corner projected onto the segment.
    corners.iter().zip(crosses).any(|(&(x, y), cross)| {
        let projection =
            (i128::from(x) - i128::from(from.x)) * dx + (i128::from(y) - i128::from(from.y)) * dy;
        if projection <= 0 || projection >= length_sq {
            return false;
        }
        let cross = cross.unsigned_abs();
        cross
            .checked_mul(cross)
            .is_some_and(|square| square < (radius_sq as u128) * (length_sq as u128))
    })
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
    fn swept_collision_checks_corner_clearance_and_allows_tangency() {
        let grid = TopologyGrid::build(
            32,
            NavCell::new(0, 0),
            NavCell::new(6, 6),
            [BuildingFootprint::new(2, 2, 2, 2)],
            [SimPoint::new(16, 16), SimPoint::new(176, 176)],
        );
        let component = grid.component_id(NavCell::new(1, 1)).unwrap();
        for (from, to, clear) in [
            (SimPoint::new(80, 48), SimPoint::new(48, 80), false),
            (SimPoint::new(48, 48), SimPoint::new(48, 144), true),
            (SimPoint::new(49, 48), SimPoint::new(48, 80), false),
            (SimPoint::new(48, 48), SimPoint::new(144, 48), true),
            (SimPoint::new(52, 52), SimPoint::new(53, 51), true),
            (SimPoint::new(48, 48), SimPoint::new(48, 48), true),
        ] {
            assert_eq!(
                grid.circle_segment_is_traversable_in_component(from, to, 16, component),
                clear,
                "{from:?} -> {to:?}"
            );
            assert_eq!(
                grid.circle_segment_is_traversable_in_component(to, from, 16, component),
                clear,
                "reverse segment"
            );
        }
    }

    #[test]
    fn objective_step_uses_diagonal_when_both_axes_reduce_distance() {
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

        for bias in [-1, 1] {
            assert_eq!(
                grid.objective_step_with_bias(0, NavCell::new(1, 1), bias),
                Some(NavCell::new(2, 2))
            );
            assert_eq!(
                grid.objective_step_with_bias(0, NavCell::new(1, 3), bias),
                Some(NavCell::new(2, 2))
            );
        }
    }

    #[test]
    fn diagonal_movement_does_not_cut_blocked_corners() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(3, 3),
            [
                BuildingFootprint::new(2, 1, 1, 1),
                BuildingFootprint::new(1, 2, 1, 1),
            ],
            [SimPoint::new(3 * cell, 3 * cell), SimPoint::new(0, 0)],
        );

        let from = NavCell::new(1, 1);
        assert!(
            !grid
                .movement_neighbors(from)
                .into_iter()
                .flatten()
                .any(|cell| cell == NavCell::new(2, 2)),
            "diagonal route cut between two blocked cardinal neighbors"
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
            -1,
            None,
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
            .step_from_distance_field_with_radius_bias(from, &field, radius, -1, None)
            .expect("radius-aware objective field should provide a detour");
        assert_ne!(first, NavCell::new(3, 3));

        let mut current = first;
        let mut path = vec![from, current];
        for _ in 0..12 {
            if current.x > 4 {
                break;
            }
            current = grid
                .step_from_distance_field_with_radius_bias(current, &field, radius, -1, None)
                .expect("radius-aware objective detour should remain reachable");
            path.push(current);
        }
        assert!(
            current.x > 4,
            "radius-aware objective pursuit never cleared the wall: {path:?}"
        );
    }

    #[test]
    fn opposite_pursuit_biases_split_equal_cost_target_routes() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let grid = TopologyGrid::build(
            cell,
            NavCell::new(0, 0),
            NavCell::new(6, 6),
            [BuildingFootprint::new(3, 3, 1, 1)],
            [
                SimPoint::new(6 * cell, 3 * cell),
                SimPoint::new(0, 3 * cell),
            ],
        );
        let from = NavCell::new(2, 3);
        let target = NavCell::new(4, 3);

        let low_bias = grid.pursuit_step(from, target, None, -1);
        let high_bias = grid.pursuit_step(from, target, None, 1);
        assert_eq!(low_bias.next_cell, Some(NavCell::new(2, 2)));
        assert_eq!(high_bias.next_cell, Some(NavCell::new(2, 4)));
        assert!(low_bias.used_a_star);
        assert!(high_bias.used_a_star);
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

        let result = grid.pursuit_step(NavCell::new(2, 1), NavCell::new(4, 1), None, -1);
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
            let result = grid.pursuit_step(current, target, None, -1);
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
