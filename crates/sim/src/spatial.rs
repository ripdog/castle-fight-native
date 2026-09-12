use std::collections::HashMap;

use crate::math::SimPoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpatialPartition {
    pub team: u8,
    pub navigation_component: u32,
}

impl SpatialPartition {
    #[must_use]
    pub const fn new(team: u8, navigation_component: u32) -> Self {
        Self {
            team,
            navigation_component,
        }
    }
}

#[derive(Debug)]
pub struct SpatialGrid {
    cell_size: i32,
    buckets: HashMap<(SpatialPartition, i32, i32), Vec<usize>>,
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
}
