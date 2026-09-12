use std::collections::HashMap;

use crate::math::SimPoint;

#[derive(Debug)]
pub struct SpatialGrid {
    cell_size: i32,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}

impl SpatialGrid {
    #[must_use]
    pub fn build(cell_size: i32, positions: impl IntoIterator<Item = (usize, SimPoint)>) -> Self {
        assert!(cell_size > 0);
        let mut buckets = HashMap::new();

        for (index, position) in positions {
            buckets
                .entry(cell_of(position, cell_size))
                .or_insert_with(Vec::new)
                .push(index);
        }

        Self { cell_size, buckets }
    }

    pub fn for_each_candidate(&self, center: SimPoint, radius: i32, mut f: impl FnMut(usize)) {
        debug_assert!(radius >= 0);
        let min = SimPoint::new(center.x - radius, center.y - radius);
        let max = SimPoint::new(center.x + radius, center.y + radius);
        let (min_x, min_y) = cell_of(min, self.cell_size);
        let (max_x, max_y) = cell_of(max, self.cell_size);

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if let Some(bucket) = self.buckets.get(&(x, y)) {
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
}
