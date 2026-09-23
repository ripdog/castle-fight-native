use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use castle_fight_sim::{SUBUNITS_PER_WORLD_UNIT, TerrainElevationMap, TerrainElevationSample};
use serde::Deserialize;

const HEIGHT_QUARTERS_PER_WORLD_UNIT: f32 = 4.0;
const TERRAIN_TEXTURE_MANIFEST: &str = "wc3/terrain/manifest.json";
const TERRAIN_TEXTURE_ASSET_PREFIX: &str = "wc3/terrain";
const WC3_BLEND_ATLAS_SIDE: u32 = 4;
const TERRAIN_PRESENTATION_SUBDIVISIONS: u32 = 4;
// Average presentation normals across half a terrain tile. WC3's classic terrain renderer
// shades ramps as broad, soft transitions rather than exposing each height-sample change.
const TERRAIN_NORMAL_SAMPLE_GRID_DELTA: f32 = 0.5;
const TERRAIN_SLOPE_SHADE_STRENGTH: f32 = 0.24;
const TERRAIN_SLOPE_MIN_BRIGHTNESS: f32 = 0.84;
const TERRAIN_SIDE_MASK_HEIGHT_OFFSET: f32 = 0.6;

#[derive(Resource, Debug, Clone)]
pub struct TerrainSurface {
    elevation: TerrainElevationMap,
    origin_world: Vec2,
    tile_world: f32,
    max_world: Vec2,
    vertex_width: usize,
    vertex_heights: Vec<f32>,
}

#[derive(Resource, Debug, Clone)]
pub struct TerrainTextureLayout {
    width_tiles: u32,
    height_tiles: u32,
    tile_palette: Vec<String>,
    ground_texture: Vec<u8>,
    ground_variation: Vec<u8>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct TerrainTextureSet {
    ground: BTreeMap<usize, TerrainGroundAtlas>,
}

#[derive(Debug, Clone)]
pub struct TerrainGroundAtlas {
    rawcode: String,
    asset_path: String,
    width: u32,
    height: u32,
    extended: bool,
}

#[derive(Debug)]
pub struct TerrainTextureMesh {
    pub palette_index: usize,
    pub mesh: Mesh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerrainTextureLayer {
    palette_index: usize,
    variation: u8,
}

#[derive(Default)]
struct TerrainMeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wc3TerrainTextureJson {
    tile_palette: Vec<String>,
    map: Wc3TerrainTextureMap,
    ground_texture: Vec<u8>,
    ground_variation: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct Wc3TerrainTextureMap {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct TerrainTextureManifest {
    schema_version: u32,
    ground: Vec<TerrainGroundManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct TerrainGroundManifestEntry {
    rawcode: String,
    palette_index: usize,
    png: String,
    width: u32,
    height: u32,
    atlas: TerrainGroundManifestAtlas,
}

#[derive(Debug, Deserialize)]
struct TerrainGroundManifestAtlas {
    extended: bool,
}

impl TerrainSurface {
    #[must_use]
    pub fn new(elevation: TerrainElevationMap) -> Self {
        let origin = elevation.origin();
        let maximum = elevation.max_point();
        let subunits = SUBUNITS_PER_WORLD_UNIT as f32;
        let vertex_width =
            usize::try_from(elevation.width_tiles()).expect("terrain width fits usize") + 1;
        let vertex_height =
            usize::try_from(elevation.height_tiles()).expect("terrain height fits usize") + 1;
        let mut vertex_heights = Vec::with_capacity(vertex_width * vertex_height);
        for y in 0..=elevation.height_tiles() {
            for x in 0..=elevation.width_tiles() {
                vertex_heights.push(sample_height(elevation.vertex_sample(x, y)));
            }
        }
        Self {
            origin_world: Vec2::new(origin.x as f32 / subunits, origin.y as f32 / subunits),
            tile_world: elevation.tile_size() as f32 / subunits,
            max_world: Vec2::new(maximum.x as f32 / subunits, maximum.y as f32 / subunits),
            elevation,
            vertex_width,
            vertex_heights,
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

    #[cfg(test)]
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
        self.height_at_grid((clamped - self.origin_world) / self.tile_world)
    }

    fn height_at_grid(&self, grid: Vec2) -> f32 {
        let width = self.elevation.width_tiles();
        let height = self.elevation.height_tiles();
        let clamped = grid.clamp(Vec2::ZERO, Vec2::new(width as f32, height as f32));
        let cell_x = (clamped.x.floor() as u32).min(width - 1);
        let cell_y = (clamped.y.floor() as u32).min(height - 1);
        let tx = (clamped.x - cell_x as f32).clamp(0.0, 1.0);
        let ty = (clamped.y - cell_y as f32).clamp(0.0, 1.0);

        let mut rows = [0.0; 4];
        for (row_index, offset_y) in (-1_i32..=2).enumerate() {
            let y = cell_y as i32 + offset_y;
            let p0 = self.vertex_height_clamped(cell_x as i32 - 1, y);
            let p1 = self.vertex_height_clamped(cell_x as i32, y);
            let p2 = self.vertex_height_clamped(cell_x as i32 + 1, y);
            let p3 = self.vertex_height_clamped(cell_x as i32 + 2, y);
            rows[row_index] = catmull_rom(p0, p1, p2, p3, tx);
        }
        let smoothed = catmull_rom(rows[0], rows[1], rows[2], rows[3], ty);

        let corners = [
            self.vertex_height_clamped(cell_x as i32, cell_y as i32),
            self.vertex_height_clamped(cell_x as i32 + 1, cell_y as i32),
            self.vertex_height_clamped(cell_x as i32, cell_y as i32 + 1),
            self.vertex_height_clamped(cell_x as i32 + 1, cell_y as i32 + 1),
        ];
        let minimum = corners.into_iter().fold(f32::INFINITY, f32::min);
        let maximum = corners.into_iter().fold(f32::NEG_INFINITY, f32::max);
        smoothed.clamp(minimum, maximum)
    }

    fn vertex_height_clamped(&self, x: i32, y: i32) -> f32 {
        let vertex_x = x.clamp(0, self.elevation.width_tiles() as i32) as usize;
        let vertex_y = y.clamp(0, self.elevation.height_tiles() as i32) as usize;
        self.vertex_heights[vertex_y * self.vertex_width + vertex_x]
    }

    fn position_at_grid(&self, grid: Vec2) -> [f32; 3] {
        [
            self.origin_world.x + grid.x * self.tile_world,
            self.height_at_grid(grid),
            self.origin_world.y + grid.y * self.tile_world,
        ]
    }

    fn normal_at_grid(&self, grid: Vec2) -> [f32; 3] {
        let maximum = Vec2::new(
            self.elevation.width_tiles() as f32,
            self.elevation.height_tiles() as f32,
        );
        let left = Vec2::new((grid.x - TERRAIN_NORMAL_SAMPLE_GRID_DELTA).max(0.0), grid.y);
        let right = Vec2::new(
            (grid.x + TERRAIN_NORMAL_SAMPLE_GRID_DELTA).min(maximum.x),
            grid.y,
        );
        let bottom = Vec2::new(grid.x, (grid.y - TERRAIN_NORMAL_SAMPLE_GRID_DELTA).max(0.0));
        let top = Vec2::new(
            grid.x,
            (grid.y + TERRAIN_NORMAL_SAMPLE_GRID_DELTA).min(maximum.y),
        );
        let dx = ((right.x - left.x) * self.tile_world).max(f32::EPSILON);
        let dz = ((top.y - bottom.y) * self.tile_world).max(f32::EPSILON);
        let dh_dx = (self.height_at_grid(right) - self.height_at_grid(left)) / dx;
        let dh_dz = (self.height_at_grid(top) - self.height_at_grid(bottom)) / dz;
        Vec3::new(-dh_dx, 1.0, -dh_dz).normalize().to_array()
    }

    #[must_use]
    pub fn clamp_world_position(&self, mut position: Vec3) -> Vec3 {
        position.x = position.x.clamp(self.origin_world.x, self.max_world.x);
        position.z = position.z.clamp(self.origin_world.y, self.max_world.y);
        position.y = self.height_at_world(position.xz());
        position
    }

    #[must_use]
    pub fn side_mask_mesh(&self, buildable_min_x: f32, buildable_max_x: f32) -> Mesh {
        assert!(buildable_min_x <= buildable_max_x);
        let width = self.elevation.width_tiles() * TERRAIN_PRESENTATION_SUBDIVISIONS;
        let height = self.elevation.height_tiles() * TERRAIN_PRESENTATION_SUBDIVISIONS;
        let row_width = width + 1;
        let subdivisions = TERRAIN_PRESENTATION_SUBDIVISIONS as f32;
        let mut positions = Vec::with_capacity((row_width as usize) * ((height + 1) as usize));
        let mut normals = Vec::with_capacity(positions.capacity());

        for y in 0..=height {
            for x in 0..=width {
                let grid = Vec2::new(x as f32 / subdivisions, y as f32 / subdivisions);
                let mut position = self.position_at_grid(grid);
                position[1] += TERRAIN_SIDE_MASK_HEIGHT_OFFSET;
                positions.push(position);
                normals.push(self.normal_at_grid(grid));
            }
        }

        let mut indices = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let left_world = self.origin_world.x + x as f32 / subdivisions * self.tile_world;
                let right_world =
                    self.origin_world.x + (x + 1) as f32 / subdivisions * self.tile_world;
                if right_world > buildable_min_x && left_world < buildable_max_x {
                    continue;
                }
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
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
    }

    #[must_use]
    pub fn mesh(&self) -> Mesh {
        let width = self.elevation.width_tiles() * TERRAIN_PRESENTATION_SUBDIVISIONS;
        let height = self.elevation.height_tiles() * TERRAIN_PRESENTATION_SUBDIVISIONS;
        let row_width = width + 1;
        let vertex_count = (row_width as usize) * ((height + 1) as usize);
        let mut positions = Vec::with_capacity(vertex_count);
        let mut normals = Vec::with_capacity(vertex_count);
        let mut uvs = Vec::with_capacity(vertex_count);
        let subdivisions = TERRAIN_PRESENTATION_SUBDIVISIONS as f32;
        let terrain_width = self.elevation.width_tiles() as f32;
        let terrain_height = self.elevation.height_tiles() as f32;

        for y in 0..=height {
            for x in 0..=width {
                let grid = Vec2::new(x as f32 / subdivisions, y as f32 / subdivisions);
                positions.push(self.position_at_grid(grid));
                normals.push(self.normal_at_grid(grid));
                uvs.push([
                    grid.x / terrain_width.max(1.0),
                    1.0 - grid.y / terrain_height.max(1.0),
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
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
    }

    fn texture_sort_bounds(&self) -> Result<([f32; 3], [f32; 3]), String> {
        let mut min_height = f32::INFINITY;
        let mut max_height = f32::NEG_INFINITY;
        for y in 0..=self.elevation.height_tiles() {
            for x in 0..=self.elevation.width_tiles() {
                let sample = self
                    .elevation
                    .vertex_sample(x, y)
                    .ok_or_else(|| format!("missing terrain vertex ({x}, {y})"))?;
                let height = sample_display_height(sample);
                min_height = min_height.min(height);
                max_height = max_height.max(height);
            }
        }
        Ok((
            [self.origin_world.x, min_height, self.origin_world.y],
            [self.max_world.x, max_height, self.max_world.y],
        ))
    }

    pub fn textured_meshes(
        &self,
        layout: &TerrainTextureLayout,
        textures: &TerrainTextureSet,
    ) -> Result<Vec<TerrainTextureMesh>, String> {
        if layout.width_tiles != self.elevation.width_tiles()
            || layout.height_tiles != self.elevation.height_tiles()
        {
            return Err("terrain texture layout dimensions do not match elevation map".into());
        }
        textures.validate_for(layout)?;

        let mut builders: BTreeMap<usize, TerrainMeshBuilder> = BTreeMap::new();
        for y in 0..layout.height_tiles {
            for x in 0..layout.width_tiles {
                for layer in layout.layers_for_cell(x, y, textures)? {
                    let atlas = textures
                        .ground
                        .get(&layer.palette_index)
                        .expect("validated terrain texture set contains palette entry");
                    let uvs = atlas_uvs(atlas, layer.variation)?;
                    builders
                        .entry(layer.palette_index)
                        .or_default()
                        .push_quad(self, x, y, uvs)?;
                }
            }
        }

        let sort_bounds = self.texture_sort_bounds()?;
        Ok(builders
            .into_iter()
            .filter_map(|(palette_index, builder)| {
                builder.finish(sort_bounds).map(|mesh| TerrainTextureMesh {
                    palette_index,
                    mesh,
                })
            })
            .collect())
    }
}

impl TerrainTextureLayout {
    pub fn from_wc3_terrain_json(json: &str) -> Result<Self, String> {
        let terrain: Wc3TerrainTextureJson =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        if terrain.map.width == 0 || terrain.map.height == 0 || terrain.tile_palette.is_empty() {
            return Err("terrain texture layout has invalid dimensions or empty palette".into());
        }
        let expected = usize::try_from(terrain.map.width + 1)
            .ok()
            .and_then(|width| {
                usize::try_from(terrain.map.height + 1)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| "terrain texture layout sample count overflow".to_owned())?;
        if terrain.ground_texture.len() != expected || terrain.ground_variation.len() != expected {
            return Err(format!(
                "terrain texture layout expected {expected} tilepoints but got {} textures and {} variations",
                terrain.ground_texture.len(),
                terrain.ground_variation.len()
            ));
        }
        for (index, palette_index) in terrain.ground_texture.iter().copied().enumerate() {
            if usize::from(palette_index) >= terrain.tile_palette.len() {
                return Err(format!(
                    "terrain tilepoint {index} references palette index {palette_index}, but palette has {} entries",
                    terrain.tile_palette.len()
                ));
            }
        }
        for (index, variation) in terrain.ground_variation.iter().copied().enumerate() {
            if variation & 0b111 != 0 {
                return Err(format!(
                    "terrain tilepoint {index} has unmasked WC3 ground variation {variation}"
                ));
            }
        }

        Ok(Self {
            width_tiles: terrain.map.width,
            height_tiles: terrain.map.height,
            tile_palette: terrain.tile_palette,
            ground_texture: terrain.ground_texture,
            ground_variation: terrain.ground_variation,
        })
    }

    fn layers_for_cell(
        &self,
        x: u32,
        y: u32,
        textures: &TerrainTextureSet,
    ) -> Result<Vec<TerrainTextureLayer>, String> {
        let bottom_left = self.texture_at(x, y)?;
        let bottom_right = self.texture_at(x + 1, y)?;
        let top_left = self.texture_at(x, y + 1)?;
        let top_right = self.texture_at(x + 1, y + 1)?;
        let corners = [bottom_left, bottom_right, top_right, top_left];

        let mut unique = corners;
        unique.sort_unstable();
        let unique_len = {
            let mut write = 0;
            for read in 0..unique.len() {
                if read == 0 || unique[read] != unique[read - 1] {
                    unique[write] = unique[read];
                    write += 1;
                }
            }
            write
        };

        let base_index = unique[0];
        let base_atlas = textures.ground.get(&base_index).ok_or_else(|| {
            format!("missing generated terrain texture for palette index {base_index}")
        })?;
        let detail = self.variation_at(x, y)? >> 3;
        let mut layers = Vec::with_capacity(unique_len);
        layers.push(TerrainTextureLayer {
            palette_index: base_index,
            variation: full_tile_variation(base_atlas.extended, detail),
        });

        for &palette_index in unique.iter().take(unique_len).skip(1) {
            let mut mask = 0_u8;
            mask |= u8::from(bottom_right == palette_index);
            mask |= u8::from(bottom_left == palette_index) << 1;
            mask |= u8::from(top_right == palette_index) << 2;
            mask |= u8::from(top_left == palette_index) << 3;
            layers.push(TerrainTextureLayer {
                palette_index,
                variation: mask,
            });
        }
        Ok(layers)
    }

    fn texture_at(&self, x: u32, y_from_bottom: u32) -> Result<usize, String> {
        let index = self.tilepoint_index(x, y_from_bottom)?;
        Ok(usize::from(self.ground_texture[index]))
    }

    fn variation_at(&self, x: u32, y_from_bottom: u32) -> Result<u8, String> {
        let index = self.tilepoint_index(x, y_from_bottom)?;
        Ok(self.ground_variation[index])
    }

    fn tilepoint_index(&self, x: u32, y_from_bottom: u32) -> Result<usize, String> {
        if x > self.width_tiles || y_from_bottom > self.height_tiles {
            return Err(format!(
                "terrain tilepoint ({x}, {y_from_bottom}) is outside {}x{} tile map",
                self.width_tiles, self.height_tiles
            ));
        }
        let row_from_top = self.height_tiles - y_from_bottom;
        let row_width = usize::try_from(self.width_tiles + 1)
            .map_err(|_| "terrain row width overflow".to_owned())?;
        usize::try_from(row_from_top)
            .ok()
            .and_then(|row| row.checked_mul(row_width))
            .and_then(|base| {
                usize::try_from(x)
                    .ok()
                    .and_then(|column| base.checked_add(column))
            })
            .ok_or_else(|| "terrain tilepoint index overflow".to_owned())
    }
}

#[must_use]
pub fn client_asset_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

impl TerrainTextureSet {
    #[must_use]
    pub fn load_default() -> Self {
        let asset_root = client_asset_root();
        let path = asset_root.join(TERRAIN_TEXTURE_MANIFEST);
        if !path.is_file() {
            return Self::default();
        }
        match Self::load_manifest(&path, TERRAIN_TEXTURE_ASSET_PREFIX) {
            Ok(textures) => textures,
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 terrain textures: {error}");
                Self::default()
            }
        }
    }

    pub fn load_manifest(path: &Path, asset_prefix: &str) -> Result<Self, String> {
        let json = fs::read_to_string(path)
            .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
        let manifest: TerrainTextureManifest = serde_json::from_str(&json)
            .map_err(|error| format!("invalid terrain manifest: {error}"))?;
        if manifest.schema_version != 1 {
            return Err(format!(
                "unsupported terrain texture manifest schema {}",
                manifest.schema_version
            ));
        }

        let mut ground = BTreeMap::new();
        for entry in manifest.ground {
            if entry.width == 0 || entry.height == 0 || entry.height % WC3_BLEND_ATLAS_SIDE != 0 {
                return Err(format!(
                    "terrain texture {} has invalid dimensions {}x{}",
                    entry.rawcode, entry.width, entry.height
                ));
            }
            let expected_extended = entry.width == entry.height.saturating_mul(2);
            if entry.width != entry.height && !expected_extended {
                return Err(format!(
                    "terrain texture {} is neither square nor extended: {}x{}",
                    entry.rawcode, entry.width, entry.height
                ));
            }
            if expected_extended != entry.atlas.extended {
                return Err(format!(
                    "terrain texture {} manifest has inconsistent extended flag",
                    entry.rawcode
                ));
            }
            if ground.contains_key(&entry.palette_index) {
                return Err(format!(
                    "duplicate generated terrain palette index {}",
                    entry.palette_index
                ));
            }
            let png = entry.png.replace('\\', "/");
            ground.insert(
                entry.palette_index,
                TerrainGroundAtlas {
                    rawcode: entry.rawcode,
                    asset_path: format!("{asset_prefix}/{png}"),
                    width: entry.width,
                    height: entry.height,
                    extended: entry.atlas.extended,
                },
            );
        }
        Ok(Self { ground })
    }

    #[must_use]
    pub fn is_available(&self) -> bool {
        !self.ground.is_empty()
    }

    pub fn validate_for(&self, layout: &TerrainTextureLayout) -> Result<(), String> {
        for (palette_index, rawcode) in layout.tile_palette.iter().enumerate() {
            let Some(atlas) = self.ground.get(&palette_index) else {
                return Err(format!(
                    "generated terrain manifest is missing palette {palette_index} ({rawcode})"
                ));
            };
            if atlas.rawcode != *rawcode {
                return Err(format!(
                    "generated terrain palette {palette_index} is {} but map expects {rawcode}",
                    atlas.rawcode
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn atlas(&self, palette_index: usize) -> Option<&TerrainGroundAtlas> {
        self.ground.get(&palette_index)
    }
}

impl TerrainGroundAtlas {
    #[must_use]
    pub fn asset_path(&self) -> &str {
        &self.asset_path
    }
}

impl TerrainMeshBuilder {
    fn push_quad(
        &mut self,
        terrain: &TerrainSurface,
        x: u32,
        y: u32,
        uvs: [[f32; 2]; 4],
    ) -> Result<(), String> {
        let subdivisions = TERRAIN_PRESENTATION_SUBDIVISIONS;
        let row_width = subdivisions + 1;
        let base_index = u32::try_from(self.positions.len())
            .map_err(|_| "terrain texture mesh has too many vertices".to_owned())?;

        for sub_y in 0..=subdivisions {
            let fy = sub_y as f32 / subdivisions as f32;
            for sub_x in 0..=subdivisions {
                let fx = sub_x as f32 / subdivisions as f32;
                let grid = Vec2::new(x as f32 + fx, y as f32 + fy);
                self.positions.push(terrain.position_at_grid(grid));
                let normal = terrain.normal_at_grid(grid);
                self.normals.push(normal);
                self.colors.push(terrain_slope_tint(normal));
                self.uvs.push(interpolate_quad_uv(uvs, fx, fy));
            }
        }

        for sub_y in 0..subdivisions {
            for sub_x in 0..subdivisions {
                let bottom_left = base_index + sub_y * row_width + sub_x;
                let bottom_right = bottom_left + 1;
                let top_left = bottom_left + row_width;
                let top_right = top_left + 1;
                self.indices.extend_from_slice(&[
                    bottom_left,
                    top_left,
                    bottom_right,
                    bottom_right,
                    top_left,
                    top_right,
                ]);
            }
        }
        Ok(())
    }

    fn finish(mut self, sort_bounds: ([f32; 3], [f32; 3])) -> Option<Mesh> {
        if self.positions.is_empty() {
            return None;
        }

        // Bevy sorts transparent meshes by each mesh AABB center. WC3 terrain layers
        // must instead have one global, palette-defined order. These two unindexed
        // anchor vertices give every palette mesh the exact same AABB, leaving the
        // material depth bias as the only transparent-sort discriminator.
        self.positions
            .extend_from_slice(&[sort_bounds.0, sort_bounds.1]);
        self.normals
            .extend_from_slice(&[[0.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
        self.colors
            .extend_from_slice(&[[1.0, 1.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]]);
        self.uvs.extend_from_slice(&[[0.0, 0.0], [0.0, 0.0]]);

        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices)),
        )
    }
}

fn full_tile_variation(extended: bool, detail: u8) -> u8 {
    if extended {
        if detail <= 15 {
            16 + detail
        } else if detail == 16 {
            15
        } else {
            0
        }
    } else if detail == 0 {
        0
    } else {
        15
    }
}

fn terrain_slope_tint(normal: [f32; 3]) -> [f32; 4] {
    let up = normal[1].clamp(0.0, 1.0);
    let brightness =
        (1.0 - (1.0 - up) * TERRAIN_SLOPE_SHADE_STRENGTH).max(TERRAIN_SLOPE_MIN_BRIGHTNESS);
    [brightness, brightness, brightness, 1.0]
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

fn interpolate_quad_uv(uvs: [[f32; 2]; 4], fx: f32, fy: f32) -> [f32; 2] {
    let bottom_left = Vec2::from_array(uvs[0]);
    let top_left = Vec2::from_array(uvs[1]);
    let bottom_right = Vec2::from_array(uvs[2]);
    let top_right = Vec2::from_array(uvs[3]);
    bottom_left
        .lerp(top_left, fy)
        .lerp(bottom_right.lerp(top_right, fy), fx)
        .to_array()
}

fn atlas_uvs(atlas: &TerrainGroundAtlas, variation: u8) -> Result<[[f32; 2]; 4], String> {
    if variation >= 16 && !atlas.extended {
        return Err(format!(
            "square terrain texture {} cannot sample variation {variation}",
            atlas.rawcode
        ));
    }
    if variation >= 32 {
        return Err(format!(
            "terrain texture {} cannot sample variation {variation}",
            atlas.rawcode
        ));
    }

    let local = u32::from(variation % 16);
    let square_offset = if variation >= 16 {
        WC3_BLEND_ATLAS_SIDE
    } else {
        0
    };
    let column = square_offset + local % WC3_BLEND_ATLAS_SIDE;
    let row = local / WC3_BLEND_ATLAS_SIDE;
    let tile_pixels = atlas.height / WC3_BLEND_ATLAS_SIDE;
    let half_pixel_u = 0.5 / atlas.width as f32;
    let half_pixel_v = 0.5 / atlas.height as f32;
    let u0 = column as f32 * tile_pixels as f32 / atlas.width as f32 + half_pixel_u;
    let u1 = (column + 1) as f32 * tile_pixels as f32 / atlas.width as f32 - half_pixel_u;
    let v0 = row as f32 * tile_pixels as f32 / atlas.height as f32 + half_pixel_v;
    let v1 = (row + 1) as f32 * tile_pixels as f32 / atlas.height as f32 - half_pixel_v;

    Ok([[u0, v1], [u0, v0], [u1, v1], [u1, v0]])
}

fn sample_height(sample: Option<TerrainElevationSample>) -> f32 {
    sample.map_or(0.0, sample_display_height)
}

fn sample_display_height(sample: TerrainElevationSample) -> f32 {
    sample.display_height_quarters() as f32 / HEIGHT_QUARTERS_PER_WORLD_UNIT
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::SimPoint;

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
    fn side_mask_covers_only_terrain_outside_the_buildable_x_bounds() {
        let terrain = original_terrain();
        let mask = terrain.side_mask_mesh(-6_176.0, 6_176.0);
        let indices = mask.indices().expect("side mask should be indexed");
        let expected_side_columns = 63usize + 79;
        let expected_rows = 64usize * TERRAIN_PRESENTATION_SUBDIVISIONS as usize;
        assert_eq!(indices.len(), expected_side_columns * expected_rows * 6);
    }

    fn original_texture_layout() -> TerrainTextureLayout {
        TerrainTextureLayout::from_wc3_terrain_json(include_str!(
            "../../../docs/original_map/extracted/terrain.json"
        ))
        .unwrap()
    }

    fn rounded_ramp_terrain() -> TerrainSurface {
        TerrainSurface::new(
            TerrainElevationMap::from_vertex_samples(
                SimPoint::new(0, 0),
                128 * SUBUNITS_PER_WORLD_UNIT,
                1,
                1,
                vec![2, 2, 2, 2],
                vec![8_592, 8_192, 8_592, 8_192],
            )
            .unwrap(),
        )
    }

    fn dummy_texture_set(layout: &TerrainTextureLayout) -> TerrainTextureSet {
        TerrainTextureSet {
            ground: layout
                .tile_palette
                .iter()
                .enumerate()
                .map(|(palette_index, rawcode)| {
                    (
                        palette_index,
                        TerrainGroundAtlas {
                            rawcode: rawcode.clone(),
                            asset_path: format!("unused/{rawcode}.png"),
                            width: 512,
                            height: 256,
                            extended: true,
                        },
                    )
                })
                .collect(),
        }
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

    #[test]
    fn presentation_height_rounds_ramp_profile_without_moving_vertices() {
        let terrain = rounded_ramp_terrain();
        let left = terrain.height_at_world(Vec2::new(0.0, 64.0));
        let quarter = terrain.height_at_world(Vec2::new(32.0, 64.0));
        let middle = terrain.height_at_world(Vec2::new(64.0, 64.0));
        let right = terrain.height_at_world(Vec2::new(128.0, 64.0));

        assert!((left - 100.0).abs() < 0.001);
        assert!((right - 0.0).abs() < 0.001);
        assert!((middle - 50.0).abs() < 0.001);
        assert!(
            quarter > 75.0,
            "rounded ramp should ease out of the plateau"
        );
    }

    #[test]
    fn presentation_mesh_subdivides_tiles_for_curved_ramps() {
        let terrain = rounded_ramp_terrain();
        let mesh = terrain.mesh();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap();
        let side = (TERRAIN_PRESENTATION_SUBDIVISIONS + 1) as usize;

        assert_eq!(positions.len(), side * side);
        assert_eq!(normals.len(), positions.len());
        assert!(normals.iter().all(|normal| normal[1] > 0.0));
    }

    #[test]
    fn terrain_slope_tint_keeps_flats_authored_and_shades_ramps_subtly() {
        assert_eq!(terrain_slope_tint([0.0, 1.0, 0.0]), [1.0; 4]);

        let ramp = terrain_slope_tint([0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0]);
        assert!(ramp[0] < 1.0);
        assert!(ramp[0] > 0.9);
        assert_eq!(ramp[0], ramp[1]);
        assert_eq!(ramp[1], ramp[2]);
        assert_eq!(ramp[3], 1.0);

        let steep = terrain_slope_tint([0.0, 0.0, 1.0]);
        assert_eq!(steep[0], TERRAIN_SLOPE_MIN_BRIGHTNESS);
    }

    #[test]
    fn original_texture_layout_matches_terrain_dimensions_and_palette() {
        let layout = original_texture_layout();
        assert_eq!(layout.width_tiles, 132);
        assert_eq!(layout.height_tiles, 64);
        assert_eq!(layout.tile_palette[0], "Zdtr");
        assert_eq!(layout.tile_palette[4], "Agrd");
        assert_eq!(layout.tile_palette[9], "Nice");
    }

    #[test]
    fn original_left_base_battle_road_starts_at_plus_minus_384_world_y() {
        let layout = original_texture_layout();
        // x tilepoint 20 is world x = -5632, through the open left base shown in gameplay.
        // The authored road's outer continuous Vstp rows are +/-384; +/-512 is still grass.
        for (y_from_bottom, expected) in [(36, "Agrs"), (35, "Vstp"), (29, "Vstp"), (28, "Agrs")] {
            let palette = layout.texture_at(20, y_from_bottom).unwrap();
            assert_eq!(layout.tile_palette[palette], expected);
        }
    }

    #[test]
    fn textured_palette_meshes_share_one_camera_independent_sort_center() {
        let terrain = original_terrain();
        let layout = original_texture_layout();
        let textures = dummy_texture_set(&layout);
        let (min_bound, max_bound) = terrain.texture_sort_bounds().unwrap();
        let meshes = terrain.textured_meshes(&layout, &textures).unwrap();

        assert!(!meshes.is_empty());
        for texture_mesh in meshes {
            let positions = texture_mesh
                .mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            assert!(positions.contains(&min_bound));
            assert!(positions.contains(&max_bound));
        }
    }

    #[test]
    fn full_tile_variation_matches_wc3_extended_and_square_rules() {
        assert_eq!(full_tile_variation(true, 0), 16);
        assert_eq!(full_tile_variation(true, 10), 26);
        assert_eq!(full_tile_variation(true, 16), 15);
        assert_eq!(full_tile_variation(true, 17), 0);
        assert_eq!(full_tile_variation(false, 0), 0);
        assert_eq!(full_tile_variation(false, 1), 15);
    }

    #[test]
    fn atlas_uvs_select_left_and_right_four_by_four_squares() {
        let atlas = TerrainGroundAtlas {
            rawcode: "test".into(),
            asset_path: "unused.png".into(),
            width: 512,
            height: 256,
            extended: true,
        };
        let blend = atlas_uvs(&atlas, 0).unwrap();
        let full = atlas_uvs(&atlas, 16).unwrap();
        assert!(blend[0][0] < 0.125);
        assert!(full[0][0] > 0.5);
        assert!(blend[0][1] > blend[1][1]);
    }
}
