use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use castle_fight_sim::{SUBUNITS_PER_WORLD_UNIT, TerrainElevationMap, TerrainElevationSample};

const HEIGHT_QUARTERS_PER_WORLD_UNIT: f32 = 4.0;

#[derive(Resource, Debug, Clone)]
pub struct TerrainSurface {
    elevation: TerrainElevationMap,
    origin_world: Vec2,
    tile_world: f32,
    max_world: Vec2,
}

impl TerrainSurface {
    #[must_use]
    pub fn new(elevation: TerrainElevationMap) -> Self {
        let origin = elevation.origin();
        let maximum = elevation.max_point();
        let subunits = SUBUNITS_PER_WORLD_UNIT as f32;
        Self {
            origin_world: Vec2::new(origin.x as f32 / subunits, origin.y as f32 / subunits),
            tile_world: elevation.tile_size() as f32 / subunits,
            max_world: Vec2::new(maximum.x as f32 / subunits, maximum.y as f32 / subunits),
            elevation,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn world_min(&self) -> Vec2 {
        self.origin_world
    }

    #[cfg(test)]
    #[must_use]
    pub fn world_max(&self) -> Vec2 {
        self.max_world
    }

    #[must_use]
    pub fn world_size(&self) -> Vec2 {
        self.max_world - self.origin_world
    }

    #[must_use]
    pub fn contains_world(&self, point: Vec2) -> bool {
        point.x >= self.origin_world.x
            && point.x <= self.max_world.x
            && point.y >= self.origin_world.y
            && point.y <= self.max_world.y
    }

    #[must_use]
    pub fn height_at_world(&self, point: Vec2) -> f32 {
        let clamped = point.clamp(self.origin_world, self.max_world);
        let grid = (clamped - self.origin_world) / self.tile_world;
        let max_x = self.elevation.width_tiles();
        let max_y = self.elevation.height_tiles();
        let x0 = (grid.x.floor() as u32).min(max_x);
        let y0 = (grid.y.floor() as u32).min(max_y);
        let x1 = x0.saturating_add(1).min(max_x);
        let y1 = y0.saturating_add(1).min(max_y);
        let tx = if x0 == x1 { 0.0 } else { grid.x - x0 as f32 };
        let ty = if y0 == y1 { 0.0 } else { grid.y - y0 as f32 };

        let h00 = sample_height(self.elevation.vertex_sample(x0, y0));
        let h10 = sample_height(self.elevation.vertex_sample(x1, y0));
        let h01 = sample_height(self.elevation.vertex_sample(x0, y1));
        let h11 = sample_height(self.elevation.vertex_sample(x1, y1));
        let bottom = h00 + (h10 - h00) * tx;
        let top = h01 + (h11 - h01) * tx;
        bottom + (top - bottom) * ty
    }

    #[must_use]
    pub fn clamp_world_position(&self, mut position: Vec3) -> Vec3 {
        position.x = position.x.clamp(self.origin_world.x, self.max_world.x);
        position.z = position.z.clamp(self.origin_world.y, self.max_world.y);
        position.y = self.height_at_world(position.xz());
        position
    }

    #[must_use]
    pub fn mesh(&self) -> Mesh {
        let width = self.elevation.width_tiles();
        let height = self.elevation.height_tiles();
        let row_width = width + 1;
        let vertex_count = (row_width as usize) * ((height + 1) as usize);
        let mut positions = Vec::with_capacity(vertex_count);
        let mut uvs = Vec::with_capacity(vertex_count);

        for y in 0..=height {
            for x in 0..=width {
                let sample = self
                    .elevation
                    .vertex_sample(x, y)
                    .expect("validated terrain vertex grid is complete");
                positions.push([
                    self.origin_world.x + x as f32 * self.tile_world,
                    sample_display_height(sample),
                    self.origin_world.y + y as f32 * self.tile_world,
                ]);
                uvs.push([
                    x as f32 / width.max(1) as f32,
                    1.0 - y as f32 / height.max(1) as f32,
                ]);
            }
        }

        let mut indices = Vec::with_capacity(width as usize * height as usize * 6);
        for y in 0..height {
            for x in 0..width {
                let bottom_left = y * row_width + x;
                let bottom_right = bottom_left + 1;
                let top_left = bottom_left + row_width;
                let top_right = top_left + 1;
                indices.extend_from_slice(&[
                    bottom_left,
                    top_left,
                    bottom_right,
                    bottom_right,
                    top_left,
                    top_right,
                ]);
            }
        }

        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
        .with_computed_smooth_normals()
    }
}

fn sample_height(sample: Option<TerrainElevationSample>) -> f32 {
    sample.map_or(0.0, sample_display_height)
}

fn sample_display_height(sample: TerrainElevationSample) -> f32 {
    sample.display_height_quarters() as f32 / HEIGHT_QUARTERS_PER_WORLD_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original_terrain() -> TerrainSurface {
        TerrainSurface::new(
            TerrainElevationMap::from_wc3_terrain_json(include_str!(
                "../../../docs/original_map/extracted/terrain.json"
            ))
            .unwrap(),
        )
    }

    #[test]
    fn original_terrain_uses_extracted_world_bounds() {
        let terrain = original_terrain();
        assert_eq!(terrain.world_min(), Vec2::new(-8192.0, -4096.0));
        assert_eq!(terrain.world_max(), Vec2::new(8704.0, 4096.0));
        assert_eq!(terrain.world_size(), Vec2::new(16896.0, 8192.0));
    }

    #[test]
    fn original_base_plateaus_are_above_the_lane() {
        let terrain = original_terrain();
        let lane = terrain.height_at_world(Vec2::new(0.0, 0.0));
        let left_base = terrain.height_at_world(Vec2::new(-6000.0, 0.0));
        let right_base = terrain.height_at_world(Vec2::new(6000.0, 0.0));

        assert!(left_base > lane + 300.0);
        assert!(right_base > lane + 300.0);
        assert!((left_base - right_base).abs() < 0.01);
    }

    #[test]
    fn height_queries_clamp_to_terrain_edges() {
        let terrain = original_terrain();
        let corner = terrain.height_at_world(terrain.world_min());
        let outside = terrain.height_at_world(Vec2::new(-100_000.0, -100_000.0));
        assert_eq!(outside, corner);
    }
}
