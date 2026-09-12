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
        };
        for (index, position) in entries {
            grid.insert(index, position);
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
        self.entries[index] = Some(ReservationEntry { position, bucket });
    }

    #[must_use]
    pub fn is_clear(&self, position: SimPoint, minimum_distance: i32) -> bool {
        debug_assert!(minimum_distance >= 0);
        let minimum_distance_sq = {
            let distance = i64::from(minimum_distance);
            (distance * distance) as u64
        };
        let min = SimPoint::new(position.x - minimum_distance, position.y - minimum_distance);
        let max = SimPoint::new(position.x + minimum_distance, position.y + minimum_distance);
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
    fn reservation_grid_moves_entries_without_losing_neighbors() {
        let mut grid = SpatialReservationGrid::build(
            10,
            SimPoint::new(0, 0),
            SimPoint::new(99, 99),
            2,
            [(0, SimPoint::new(10, 10)), (1, SimPoint::new(30, 10))],
        );
        assert!(!grid.is_clear(SimPoint::new(12, 10), 5));
        assert!(grid.is_clear(SimPoint::new(20, 10), 5));

        grid.remove(0);
        assert!(grid.is_clear(SimPoint::new(12, 10), 5));
        grid.insert(0, SimPoint::new(50, 10));
        assert!(!grid.is_clear(SimPoint::new(48, 10), 5));
    }
}
