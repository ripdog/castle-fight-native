use std::{error::Error, fmt, fs, path::Path};

use serde::Deserialize;

use crate::math::{SUBUNITS_PER_WORLD_UNIT, SimPoint};

pub const WC3_TERRAIN_TILE_WORLD_UNITS: i32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainElevationSample {
    pub cliff_level: u8,
    pub ground_height_raw: i32,
}

impl TerrainElevationSample {
    #[must_use]
    pub const fn display_height_quarters(self) -> i32 {
        self.ground_height_raw - 0x2000 + (self.cliff_level as i32 - 2) * 0x0200
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainElevationMap {
    origin: SimPoint,
    tile_size: i32,
    width_tiles: u32,
    height_tiles: u32,
    cliff_levels: Vec<u8>,
    ground_heights_raw: Vec<i32>,
}

impl TerrainElevationMap {
    pub fn from_vertex_samples(
        origin: SimPoint,
        tile_size: i32,
        width_tiles: u32,
        height_tiles: u32,
        cliff_levels_north_to_south: Vec<u8>,
        ground_heights_north_to_south: Vec<i32>,
    ) -> Result<Self, TerrainLoadError> {
        if tile_size <= 0 || width_tiles == 0 || height_tiles == 0 {
            return Err(TerrainLoadError::InvalidDimensions);
        }
        let expected = expected_vertex_count(width_tiles, height_tiles)?;
        if cliff_levels_north_to_south.len() != expected {
            return Err(TerrainLoadError::InvalidSampleCount {
                field: "layerHeight",
                expected,
                actual: cliff_levels_north_to_south.len(),
            });
        }
        if ground_heights_north_to_south.len() != expected {
            return Err(TerrainLoadError::InvalidSampleCount {
                field: "groundHeight",
                expected,
                actual: ground_heights_north_to_south.len(),
            });
        }
        if cliff_levels_north_to_south
            .iter()
            .any(|level| *level > 0x0f)
        {
            return Err(TerrainLoadError::InvalidCliffLevel);
        }

        Ok(Self {
            origin,
            tile_size,
            width_tiles,
            height_tiles,
            cliff_levels: cliff_levels_north_to_south,
            ground_heights_raw: ground_heights_north_to_south,
        })
    }

    pub fn from_wc3_terrain_json(json: &str) -> Result<Self, TerrainLoadError> {
        let terrain: Wc3TerrainJson = serde_json::from_str(json)?;
        let origin = SimPoint::new(
            wc3_offset_to_subunits("x", terrain.map.offset.x)?,
            wc3_offset_to_subunits("y", terrain.map.offset.y)?,
        );
        let tile_size = WC3_TERRAIN_TILE_WORLD_UNITS
            .checked_mul(SUBUNITS_PER_WORLD_UNIT)
            .ok_or(TerrainLoadError::CoordinateOverflow)?;

        Self::from_vertex_samples(
            origin,
            tile_size,
            terrain.map.width,
            terrain.map.height,
            terrain.layer_height,
            terrain.ground_height,
        )
    }

    pub fn load_wc3_terrain_file(path: impl AsRef<Path>) -> Result<Self, TerrainLoadError> {
        let json = fs::read_to_string(path)?;
        Self::from_wc3_terrain_json(&json)
    }

    #[must_use]
    pub const fn origin(&self) -> SimPoint {
        self.origin
    }

    #[must_use]
    pub const fn tile_size(&self) -> i32 {
        self.tile_size
    }

    #[must_use]
    pub const fn width_tiles(&self) -> u32 {
        self.width_tiles
    }

    #[must_use]
    pub const fn height_tiles(&self) -> u32 {
        self.height_tiles
    }

    #[must_use]
    pub fn max_point(&self) -> SimPoint {
        SimPoint::new(
            checked_axis_max(self.origin.x, self.tile_size, self.width_tiles),
            checked_axis_max(self.origin.y, self.tile_size, self.height_tiles),
        )
    }

    #[must_use]
    pub fn sample(&self, point: SimPoint) -> Option<TerrainElevationSample> {
        let index = self.nearest_vertex_index(point)?;
        self.sample_at_index(index)
    }

    #[must_use]
    pub fn vertex_sample(
        &self,
        vertex_x: u32,
        vertex_y_from_bottom: u32,
    ) -> Option<TerrainElevationSample> {
        if vertex_x > self.width_tiles || vertex_y_from_bottom > self.height_tiles {
            return None;
        }
        let row_from_top = self.height_tiles - vertex_y_from_bottom;
        let row_width = usize::try_from(self.width_tiles).ok()?.checked_add(1)?;
        let index = usize::try_from(row_from_top)
            .ok()?
            .checked_mul(row_width)?
            .checked_add(usize::try_from(vertex_x).ok()?)?;
        self.sample_at_index(index)
    }

    #[must_use]
    pub fn cliff_level_at(&self, point: SimPoint) -> Option<u8> {
        self.sample(point).map(|sample| sample.cliff_level)
    }

    #[must_use]
    pub fn contains(&self, point: SimPoint) -> bool {
        self.nearest_vertex_index(point).is_some()
    }

    fn sample_at_index(&self, index: usize) -> Option<TerrainElevationSample> {
        Some(TerrainElevationSample {
            cliff_level: *self.cliff_levels.get(index)?,
            ground_height_raw: *self.ground_heights_raw.get(index)?,
        })
    }

    fn nearest_vertex_index(&self, point: SimPoint) -> Option<usize> {
        let relative_x = i64::from(point.x) - i64::from(self.origin.x);
        let relative_y = i64::from(point.y) - i64::from(self.origin.y);
        let tile_size = i64::from(self.tile_size);
        let width_extent = i64::from(self.width_tiles).checked_mul(tile_size)?;
        let height_extent = i64::from(self.height_tiles).checked_mul(tile_size)?;
        if relative_x < 0
            || relative_y < 0
            || relative_x > width_extent
            || relative_y > height_extent
        {
            return None;
        }

        let half_tile = tile_size / 2;
        let vertex_x = ((relative_x + half_tile) / tile_size).min(i64::from(self.width_tiles));
        let vertex_y_from_bottom =
            ((relative_y + half_tile) / tile_size).min(i64::from(self.height_tiles));
        let row_from_top = i64::from(self.height_tiles) - vertex_y_from_bottom;
        let row_width = i64::from(self.width_tiles) + 1;
        let index = row_from_top.checked_mul(row_width)?.checked_add(vertex_x)?;
        usize::try_from(index).ok()
    }
}

#[derive(Debug)]
pub enum TerrainLoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidDimensions,
    InvalidSampleCount {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    InvalidCliffLevel,
    InvalidOffset {
        axis: &'static str,
        value: f64,
    },
    CoordinateOverflow,
}

impl fmt::Display for TerrainLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to read terrain file: {error}"),
            Self::Json(error) => write!(formatter, "failed to parse terrain JSON: {error}"),
            Self::InvalidDimensions => write!(
                formatter,
                "terrain dimensions and tile size must be positive"
            ),
            Self::InvalidSampleCount {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "terrain {field} has {actual} samples, expected {expected}"
            ),
            Self::InvalidCliffLevel => {
                write!(formatter, "terrain cliff level exceeds WC3's 4-bit range")
            }
            Self::InvalidOffset { axis, value } => write!(
                formatter,
                "terrain {axis} offset {value} is not a finite integral world coordinate"
            ),
            Self::CoordinateOverflow => {
                write!(formatter, "terrain coordinate conversion overflowed")
            }
        }
    }
}

impl Error for TerrainLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TerrainLoadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TerrainLoadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Deserialize)]
struct Wc3TerrainJson {
    map: Wc3TerrainMap,
    #[serde(rename = "groundHeight")]
    ground_height: Vec<i32>,
    #[serde(rename = "layerHeight")]
    layer_height: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct Wc3TerrainMap {
    width: u32,
    height: u32,
    offset: Wc3TerrainOffset,
}

#[derive(Debug, Deserialize)]
struct Wc3TerrainOffset {
    x: f64,
    y: f64,
}

fn expected_vertex_count(width_tiles: u32, height_tiles: u32) -> Result<usize, TerrainLoadError> {
    let width = usize::try_from(width_tiles)
        .map_err(|_| TerrainLoadError::CoordinateOverflow)?
        .checked_add(1)
        .ok_or(TerrainLoadError::CoordinateOverflow)?;
    let height = usize::try_from(height_tiles)
        .map_err(|_| TerrainLoadError::CoordinateOverflow)?
        .checked_add(1)
        .ok_or(TerrainLoadError::CoordinateOverflow)?;
    width
        .checked_mul(height)
        .ok_or(TerrainLoadError::CoordinateOverflow)
}

fn wc3_offset_to_subunits(axis: &'static str, value: f64) -> Result<i32, TerrainLoadError> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(TerrainLoadError::InvalidOffset { axis, value });
    }
    let world_units = value as i64;
    let subunits = world_units
        .checked_mul(i64::from(SUBUNITS_PER_WORLD_UNIT))
        .ok_or(TerrainLoadError::CoordinateOverflow)?;
    i32::try_from(subunits).map_err(|_| TerrainLoadError::CoordinateOverflow)
}

fn checked_axis_max(origin: i32, tile_size: i32, tiles: u32) -> i32 {
    let extent = i64::from(tile_size)
        .checked_mul(i64::from(tiles))
        .expect("validated terrain extent overflowed i64");
    let max = i64::from(origin)
        .checked_add(extent)
        .expect("validated terrain maximum overflowed i64");
    i32::try_from(max).expect("validated terrain maximum overflowed i32")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_point(x: i32, y: i32) -> SimPoint {
        SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
    }

    #[test]
    fn wc3_loader_understands_flipped_vertex_rows_and_offsets() {
        let json = r#"{
            "map": {"width": 2, "height": 1, "offset": {"x": -128.0, "y": -64.0}},
            "groundHeight": [10, 11, 12, 20, 21, 22],
            "layerHeight": [3, 3, 4, 1, 1, 2]
        }"#;
        let terrain = TerrainElevationMap::from_wc3_terrain_json(json).unwrap();

        assert_eq!(terrain.width_tiles(), 2);
        assert_eq!(terrain.height_tiles(), 1);
        assert_eq!(terrain.origin(), world_point(-128, -64));
        assert_eq!(
            terrain.sample(world_point(-128, -64)).unwrap().cliff_level,
            1
        );
        assert_eq!(
            terrain.sample(world_point(-128, 64)).unwrap().cliff_level,
            3
        );
        assert_eq!(terrain.sample(world_point(128, 64)).unwrap().cliff_level, 4);
        assert!(!terrain.contains(world_point(129, 64)));
    }

    #[test]
    fn vertex_samples_use_world_bottom_to_top_coordinates() {
        let terrain = TerrainElevationMap::from_vertex_samples(
            world_point(0, 0),
            128 * SUBUNITS_PER_WORLD_UNIT,
            1,
            1,
            vec![3, 4, 1, 2],
            vec![8192, 8196, 8200, 8204],
        )
        .unwrap();

        assert_eq!(terrain.vertex_sample(0, 0).unwrap().cliff_level, 1);
        assert_eq!(terrain.vertex_sample(1, 0).unwrap().ground_height_raw, 8204);
        assert_eq!(terrain.vertex_sample(0, 1).unwrap().cliff_level, 3);
        assert!(terrain.vertex_sample(2, 0).is_none());
    }

    #[test]
    fn display_height_includes_ground_and_cliff_layers() {
        let sample = TerrainElevationSample {
            cliff_level: 4,
            ground_height_raw: 9270,
        };
        assert_eq!(sample.display_height_quarters(), 2102);
    }

    #[test]
    fn committed_castle_fight_terrain_loads_with_elevated_bases() {
        let terrain = TerrainElevationMap::from_wc3_terrain_json(include_str!(
            "../../../docs/original_map/extracted/terrain.json"
        ))
        .unwrap();

        assert_eq!(terrain.width_tiles(), 132);
        assert_eq!(terrain.height_tiles(), 64);
        assert_eq!(terrain.cliff_level_at(world_point(-6_000, 0)), Some(4));
        assert_eq!(terrain.cliff_level_at(world_point(0, 0)), Some(1));
        assert_eq!(terrain.cliff_level_at(world_point(6_000, 0)), Some(4));
    }
}
