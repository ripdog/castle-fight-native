use std::collections::HashMap;

use crate::math::SimPoint;

const NONE: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpatialPartition {
    pub team: u8,
    pub navigation_component: u32,
}

impl SpatialPartition {
    pub const GLOBAL_COMPONENT: u32 = u32::MAX;

    #[must_use]
    pub const fn new(team: u8, navigation_component: u32) -> Self {
        Self {
            team,
            navigation_component,
        }
    }

    #[must_use]
    pub const fn global(team: u8) -> Self {
        Self::new(team, Self::GLOBAL_COMPONENT)
    }
}

#[derive(Debug)]
pub struct SpatialGrid {
    cell_size: i32,
    buckets: HashMap<(SpatialPartition, i32, i32), Vec<usize>>,
}

#[derive(Debug, Clone, Copy)]
struct ReservationEntry {
    position: SimPoint,
    radius: i32,
    bucket: usize,
}

#[derive(Debug)]
pub struct SpatialReservationGrid {
    cell_size: i32,
    min_cell_x: i32,
    min_cell_y: i32,
    width: usize,
    height: usize,
    heads: Vec<usize>,
    next: Vec<usize>,
    prev: Vec<usize>,
    entries: Vec<Option<ReservationEntry>>,
    max_radius: i32,
}

impl SpatialGrid {
    #[must_use]
    pub fn build(
        cell_size: i32,
        entries: impl IntoIterator<Item = (SpatialPartition, usize, SimPoint)>,
    ) -> Self {
        assert!(cell_size > 0);
        let mut buckets = HashMap::new();

        for (partition, index, position) in entries {
            let (x, y) = cell_of(position, cell_size);
            buckets
                .entry((partition, x, y))
                .or_insert_with(Vec::new)
                .push(index);
        }

        Self { cell_size, buckets }
    }

    pub fn for_each_candidate(
        &self,
        partition: SpatialPartition,
        center: SimPoint,
        radius: i32,
        mut f: impl FnMut(usize),
    ) {
        debug_assert!(radius >= 0);
        let min = SimPoint::new(center.x - radius, center.y - radius);
        let max = SimPoint::new(center.x + radius, center.y + radius);
        let (min_x, min_y) = cell_of(min, self.cell_size);
        let (max_x, max_y) = cell_of(max, self.cell_size);

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if let Some(bucket) = self.buckets.get(&(partition, x, y)) {
                    for &index in bucket {
                        f(index);
                    }
                }
            }
        }
    }

    pub fn for_each_candidate_nearest_cells(
        &self,
        partition: SpatialPartition,
        center: SimPoint,
        radius: i32,
        mut max_distance_sq: u64,
        mut f: impl FnMut(usize) -> Option<u64>,
    ) {
        debug_assert!(radius >= 0);
        let min = SimPoint::new(center.x - radius, center.y - radius);
        let max = SimPoint::new(center.x + radius, center.y + radius);
        let (min_x, min_y) = cell_of(min, self.cell_size);
        let (max_x, max_y) = cell_of(max, self.cell_size);
        let (center_x, center_y) = cell_of(center, self.cell_size);
        let max_ring = (center_x - min_x)
            .max(max_x - center_x)
            .max(center_y - min_y)
            .max(max_y - center_y);

        for ring in 0..=max_ring {
            if ring == 0 {
                self.visit_pruned_cell(
                    partition,
                    center,
                    (center_x, center_y),
                    &mut max_distance_sq,
                    &mut f,
                );
                continue;
            }

            let left = center_x - ring;
            let right = center_x + ring;
            let top = center_y - ring;
            let bottom = center_y + ring;

            for x in left..=right {
                if x >= min_x && x <= max_x && top >= min_y && top <= max_y {
                    self.visit_pruned_cell(
                        partition,
                        center,
                        (x, top),
                        &mut max_distance_sq,
                        &mut f,
                    );
                }
            }
            for y in top + 1..=bottom {
                if right >= min_x && right <= max_x && y >= min_y && y <= max_y {
                    self.visit_pruned_cell(
                        partition,
                        center,
                        (right, y),
                        &mut max_distance_sq,
                        &mut f,
                    );
                }
            }
            if bottom >= min_y && bottom <= max_y {
                for x in (left..right).rev() {
                    if x >= min_x && x <= max_x {
                        self.visit_pruned_cell(
                            partition,
                            center,
                            (x, bottom),
                            &mut max_distance_sq,
                            &mut f,
                        );
                    }
                }
            }
            if left >= min_x && left <= max_x {
                for y in (top + 1..bottom).rev() {
                    if y >= min_y && y <= max_y {
                        self.visit_pruned_cell(
                            partition,
                            center,
                            (left, y),
                            &mut max_distance_sq,
                            &mut f,
                        );
                    }
                }
            }
        }
    }

    fn visit_pruned_cell(
        &self,
        partition: SpatialPartition,
        center: SimPoint,
        cell: (i32, i32),
        max_distance_sq: &mut u64,
        f: &mut impl FnMut(usize) -> Option<u64>,
    ) {
        let (cell_x, cell_y) = cell;
        if cell_min_distance_sq(center, cell_x, cell_y, self.cell_size) > *max_distance_sq {
            return;
        }
        let Some(bucket) = self.buckets.get(&(partition, cell_x, cell_y)) else {
            return;
        };
        for &index in bucket {
            if let Some(tighter) = f(index) {
                *max_distance_sq = (*max_distance_sq).min(tighter);
            }
        }
    }
}

fn cell_min_distance_sq(center: SimPoint, cell_x: i32, cell_y: i32, cell_size: i32) -> u64 {
    let min_x = i64::from(cell_x) * i64::from(cell_size);
    let min_y = i64::from(cell_y) * i64::from(cell_size);
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

impl SpatialReservationGrid {
    #[must_use]
    pub fn build(
        cell_size: i32,
        bounds_min: SimPoint,
        bounds_max: SimPoint,
        entry_capacity: usize,
        entries: impl IntoIterator<Item = (usize, SimPoint)>,
    ) -> Self {
        assert!(cell_size > 0);
        assert!(bounds_max.x >= bounds_min.x && bounds_max.y >= bounds_min.y);
        let (min_cell_x, min_cell_y) = cell_of(bounds_min, cell_size);
        let (max_cell_x, max_cell_y) = cell_of(bounds_max, cell_size);
        let width =
            usize::try_from(max_cell_x - min_cell_x + 1).expect("reservation grid width overflow");
        let height =
            usize::try_from(max_cell_y - min_cell_y + 1).expect("reservation grid height overflow");
        let bucket_count = width
            .checked_mul(height)
            .expect("reservation grid too large");
        let mut grid = Self {
            cell_size,
            min_cell_x,
            min_cell_y,
            width,
            height,
            heads: vec![NONE; bucket_count],
            next: vec![NONE; entry_capacity],
            prev: vec![NONE; entry_capacity],
            entries: vec![None; entry_capacity],
            max_radius: 0,
        };
        for (index, position) in entries {
            grid.insert(index, position);
        }
        grid
    }

    #[must_use]
    pub fn build_with_radii(
        cell_size: i32,
        bounds_min: SimPoint,
        bounds_max: SimPoint,
        entry_capacity: usize,
        entries: impl IntoIterator<Item = (usize, SimPoint, i32)>,
    ) -> Self {
        let mut grid = Self::build(
            cell_size,
            bounds_min,
            bounds_max,
            entry_capacity,
            std::iter::empty(),
        );
        for (index, position, radius) in entries {
            grid.insert_with_radius(index, position, radius);
        }
        grid
    }

    pub fn remove(&mut self, index: usize) {
        let Some(entry) = self.entries.get_mut(index).and_then(Option::take) else {
            return;
        };
        let previous = self.prev[index];
        let next = self.next[index];
        if previous == NONE {
            self.heads[entry.bucket] = next;
        } else {
            self.next[previous] = next;
        }
        if next != NONE {
            self.prev[next] = previous;
        }
        self.prev[index] = NONE;
        self.next[index] = NONE;
    }

    pub fn insert(&mut self, index: usize, position: SimPoint) {
        self.insert_with_radius(index, position, 0);
    }

    pub fn insert_with_radius(&mut self, index: usize, position: SimPoint, radius: i32) {
        assert!(radius >= 0, "reservation radius must be non-negative");
        assert!(
            index < self.entries.len(),
            "reservation index out of bounds"
        );
        assert!(
            self.entries[index].is_none(),
            "reservation already occupied"
        );
        let bucket = self
            .bucket_index(position)
            .expect("reservation position outside configured bounds");
        let head = self.heads[bucket];
        self.heads[bucket] = index;
        self.next[index] = head;
        self.prev[index] = NONE;
        if head != NONE {
            self.prev[head] = index;
        }
        self.entries[index] = Some(ReservationEntry {
            position,
            radius,
            bucket,
        });
        self.max_radius = self.max_radius.max(radius);
    }

    #[must_use]
    pub fn is_clear_with_radius(&self, position: SimPoint, radius: i32) -> bool {
        debug_assert!(radius >= 0);
        let search_distance = radius.saturating_add(self.max_radius);
        let min = SimPoint::new(
            position.x.saturating_sub(search_distance),
            position.y.saturating_sub(search_distance),
        );
        let max = SimPoint::new(
            position.x.saturating_add(search_distance),
            position.y.saturating_add(search_distance),
        );
        let (mut min_x, mut min_y) = cell_of(min, self.cell_size);
        let (mut max_x, mut max_y) = cell_of(max, self.cell_size);
        let grid_max_x = self.min_cell_x + self.width as i32 - 1;
        let grid_max_y = self.min_cell_y + self.height as i32 - 1;
        min_x = min_x.max(self.min_cell_x);
        min_y = min_y.max(self.min_cell_y);
        max_x = max_x.min(grid_max_x);
        max_y = max_y.min(grid_max_y);
        if min_x > max_x || min_y > max_y {
            return true;
        }

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let bucket = self.bucket_index_from_cell(x, y);
                let mut index = self.heads[bucket];
                while index != NONE {
                    let entry =
                        self.entries[index].expect("reservation list referenced missing entry");
                    let minimum_distance = radius.saturating_add(entry.radius);
                    let minimum_distance_sq = {
                        let distance = i64::from(minimum_distance);
                        (distance * distance) as u64
                    };
                    if position.distance_sq(entry.position) < minimum_distance_sq {
                        return false;
                    }
                    index = self.next[index];
                }
            }
        }
        true
    }

    fn bucket_index(&self, position: SimPoint) -> Option<usize> {
        let (x, y) = cell_of(position, self.cell_size);
        if x < self.min_cell_x || y < self.min_cell_y {
            return None;
        }
        let local_x = usize::try_from(x - self.min_cell_x).ok()?;
        let local_y = usize::try_from(y - self.min_cell_y).ok()?;
        if local_x >= self.width || local_y >= self.height {
            return None;
        }
        local_y.checked_mul(self.width)?.checked_add(local_x)
    }

    fn bucket_index_from_cell(&self, x: i32, y: i32) -> usize {
        let local_x = usize::try_from(x - self.min_cell_x)
            .expect("reservation x cell below configured bounds");
        let local_y = usize::try_from(y - self.min_cell_y)
            .expect("reservation y cell below configured bounds");
        local_y * self.width + local_x
    }
}

fn cell_of(point: SimPoint, cell_size: i32) -> (i32, i32) {
    (point.x.div_euclid(cell_size), point.y.div_euclid(cell_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coordinates_use_euclidean_cells() {
        assert_eq!(cell_of(SimPoint::new(-1, -1), 10), (-1, -1));
        assert_eq!(cell_of(SimPoint::new(-10, -10), 10), (-1, -1));
        assert_eq!(cell_of(SimPoint::new(-11, -11), 10), (-2, -2));
    }

    #[test]
    fn partitioned_query_skips_other_components() {
        let wanted = SpatialPartition::new(1, 7);
        let other_component = SpatialPartition::new(1, 8);
        let other_team = SpatialPartition::new(0, 7);
        let grid = SpatialGrid::build(
            10,
            [
                (wanted, 1, SimPoint::new(5, 5)),
                (other_component, 2, SimPoint::new(5, 5)),
                (other_team, 3, SimPoint::new(5, 5)),
            ],
        );

        let mut found = Vec::new();
        grid.for_each_candidate(wanted, SimPoint::new(5, 5), 10, |index| found.push(index));
        assert_eq!(found, vec![1]);
    }

    #[test]
    fn nearest_cell_query_prunes_without_changing_nearest_choice() {
        let partition = SpatialPartition::global(0);
        let positions = [
            SimPoint::new(95, 0),
            SimPoint::new(12, 0),
            SimPoint::new(35, 0),
            SimPoint::new(-18, 0),
        ];
        let grid = SpatialGrid::build(
            10,
            positions
                .iter()
                .enumerate()
                .map(|(index, position)| (partition, index, *position)),
        );
        let center = SimPoint::new(0, 0);
        let mut best: Option<(u64, usize)> = None;
        let mut visited = 0usize;
        grid.for_each_candidate_nearest_cells(partition, center, 100, 10_000, |index| {
            visited += 1;
            let distance_sq = center.distance_sq(positions[index]);
            let key = (distance_sq, index);
            if best.is_none_or(|current| key < current) {
                best = Some(key);
                Some(distance_sq)
            } else {
                None
            }
        });

        assert_eq!(best, Some((144, 1)));
        assert!(visited < positions.len());
    }

    #[test]
    fn reservation_grid_moves_entries_without_losing_neighbors() {
        let mut grid = SpatialReservationGrid::build(
            10,
            SimPoint::new(0, 0),
            SimPoint::new(99, 99),
            2,
            [(0, SimPoint::new(10, 10)), (1, SimPoint::new(30, 10))],
        );
        assert!(!grid.is_clear_with_radius(SimPoint::new(12, 10), 5));
        assert!(grid.is_clear_with_radius(SimPoint::new(20, 10), 5));

        grid.remove(0);
        assert!(grid.is_clear_with_radius(SimPoint::new(12, 10), 5));
        grid.insert(0, SimPoint::new(50, 10));
        assert!(!grid.is_clear_with_radius(SimPoint::new(48, 10), 5));
    }

    #[test]
    fn reservation_grid_uses_sum_of_collision_radii() {
        let grid = SpatialReservationGrid::build_with_radii(
            20,
            SimPoint::new(0, 0),
            SimPoint::new(100, 100),
            1,
            [(0, SimPoint::new(20, 20), 10)],
        );

        assert!(!grid.is_clear_with_radius(SimPoint::new(34, 20), 5));
        assert!(grid.is_clear_with_radius(SimPoint::new(35, 20), 5));
    }
}
