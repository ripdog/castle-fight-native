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
        for (team, objective) in objectives.into_iter().enumerate() {
            grid.objective_distance[team] = grid.distance_field(grid.cell_of_point(objective));
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
    pub fn objective_step(&self, team: u8, from: NavCell) -> Option<NavCell> {
        let field = self.objective_distance.get(usize::from(team))?;
        let from_index = self.index(from)?;
        let current = field[from_index];
        if current == UNREACHABLE || current == 0 {
            return None;
        }

        self.neighbors(from)
            .into_iter()
            .flatten()
            .filter_map(|cell| self.index(cell).map(|index| (field[index], cell)))
            .filter(|(distance, _)| *distance < current)
            .min_by_key(|(distance, cell)| (*distance, cell.y, cell.x))
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
    pub fn pursuit_step(&self, from: NavCell, target: NavCell) -> Option<NavCell> {
        if !self.same_component(from, target) {
            return None;
        }
        if from == target {
            return Some(from);
        }

        let current_distance = cell_distance_sq(from, target);
        if let Some(greedy) = self
            .neighbors(from)
            .into_iter()
            .flatten()
            .filter(|cell| self.same_component(from, *cell))
            .min_by_key(|cell| (cell_distance_sq(*cell, target), cell.y, cell.x))
            .filter(|cell| cell_distance_sq(*cell, target) < current_distance)
        {
            return Some(greedy);
        }

        self.a_star_first_step(from, target)
    }

    fn a_star_first_step(&self, from: NavCell, target: NavCell) -> Option<NavCell> {
        let start = self.index(from)?;
        let goal = self.index(target)?;
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

        while let Some(Reverse((_, cost, _, _, current))) = open.pop() {
            if cost != g_score[current] {
                continue;
            }
            if current == goal {
                let mut step = goal;
                while came_from[step] != start {
                    let parent = came_from[step];
                    if parent == usize::MAX {
                        return None;
                    }
                    step = parent;
                }
                return Some(self.cell_from_index(step));
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

        None
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

        assert_eq!(
            grid.pursuit_step(NavCell::new(2, 1), NavCell::new(4, 1)),
            Some(NavCell::new(2, 2))
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
