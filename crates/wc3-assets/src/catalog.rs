use std::{collections::BTreeMap, error::Error, path::Path};

use csv::StringRecord;
use serde::{Deserialize, Serialize};

pub const CATALOG_VERSION: &str = env!("CF_ASSET_CATALOG_VERSION");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnitAssetSpec {
    pub rawcode: String,
    pub base_rawcode: String,
    pub name: String,
    pub model_path: Option<String>,
    pub scale: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VisualAssetSpec {
    pub owner_kind: String,
    pub owner_rawcode: String,
    pub role: String,
    pub model_path: String,
    pub missile_arc: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VisualAssetCatalog {
    pub assets: Vec<VisualAssetSpec>,
    pub chain_lightning_abilities: Vec<String>,
    pub stun_model_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DoodadAssetSpec {
    pub rawcode: String,
    pub base_rawcode: String,
    pub object_kind: String,
    pub name: String,
    pub model_path: Option<String>,
    pub num_variations: Option<u32>,
    pub placements: Vec<DoodadPlacementSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DoodadPlacementSpec {
    pub editor_id: u32,
    pub position: [f32; 3],
    pub angle_degrees: f32,
    pub scale: [f32; 3],
    pub visible: bool,
    pub solid: bool,
    pub fixed_z: bool,
    pub variation: u32,
}

pub fn load_embedded_units() -> Result<Vec<UnitAssetSpec>, Box<dyn Error>> {
    let json = include_str!(concat!(env!("OUT_DIR"), "/unit-assets.json"));
    Ok(serde_json::from_str(json)?)
}

pub fn load_embedded_doodads() -> Result<Vec<DoodadAssetSpec>, Box<dyn Error>> {
    let json = include_str!(concat!(env!("OUT_DIR"), "/doodad-assets.json"));
    Ok(serde_json::from_str(json)?)
}

pub fn load_embedded_visuals() -> Result<VisualAssetCatalog, Box<dyn Error>> {
    let json = include_str!(concat!(env!("OUT_DIR"), "/visual-assets.json"));
    Ok(serde_json::from_str(json)?)
}

pub fn load_production_units(
    production_path: &Path,
    object_fields_path: &Path,
) -> Result<Vec<UnitAssetSpec>, Box<dyn Error>> {
    let mut production = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(production_path)?;
    let prod_headers = production.headers()?.clone();
    let unit_rawcode = header_index(&prod_headers, "unit_rawcode")?;
    let unit_names = header_index(&prod_headers, "unit_names")?;

    let mut units = BTreeMap::<String, String>::new();
    for row in production.records() {
        let row = row?;
        let rawcode = row.get(unit_rawcode).unwrap_or_default().trim();
        if rawcode.is_empty() {
            continue;
        }
        units
            .entry(rawcode.to_owned())
            .or_insert_with(|| row.get(unit_names).unwrap_or_default().trim().to_owned());
    }

    let mut fields = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(object_fields_path)?;
    let headers = fields.headers()?.clone();
    let category = header_index(&headers, "category")?;
    let rawcode_col = header_index(&headers, "rawcode")?;
    let base_rawcode_col = header_index(&headers, "base_rawcode")?;
    let field_id = header_index(&headers, "field_id")?;
    let recovered = header_index(&headers, "recovered_value_json")?;

    let mut base_rawcodes = BTreeMap::<String, String>::new();
    let mut model_paths = BTreeMap::<String, String>::new();
    let mut scales = BTreeMap::<String, f32>::new();
    for row in fields.records() {
        let row = row?;
        if row.get(category) != Some("units") {
            continue;
        }
        let Some(rawcode) = row.get(rawcode_col) else {
            continue;
        };
        if !units.contains_key(rawcode) {
            continue;
        }
        if let Some(base_rawcode) = row.get(base_rawcode_col) {
            base_rawcodes
                .entry(rawcode.to_owned())
                .or_insert_with(|| base_rawcode.to_owned());
        }
        match row.get(field_id) {
            Some("umdl") => {
                if let Some(path) = parse_json_string(row.get(recovered).unwrap_or_default()) {
                    model_paths.insert(rawcode.to_owned(), path);
                }
            }
            Some("usca") => {
                if let Some(scale) = parse_json_f32(row.get(recovered).unwrap_or_default()) {
                    scales.insert(rawcode.to_owned(), scale);
                }
            }
            _ => {}
        }
    }

    let mut result = Vec::with_capacity(units.len());
    for (rawcode, name) in units {
        let base_rawcode = base_rawcodes.remove(&rawcode).ok_or_else(|| {
            format!("production unit {rawcode} ({name}) has no resolved base rawcode")
        })?;
        result.push(UnitAssetSpec {
            model_path: model_paths.remove(&rawcode),
            scale: scales.remove(&rawcode),
            rawcode,
            base_rawcode,
            name,
        });
    }
    Ok(result)
}

fn header_index(headers: &StringRecord, name: &str) -> Result<usize, Box<dyn Error>> {
    headers
        .iter()
        .position(|header| header == name)
        .ok_or_else(|| format!("missing TSV column {name:?}").into())
}

fn parse_json_string(value: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()?
        .as_str()
        .map(str::to_owned)
}

fn parse_json_f32(value: &str) -> Option<f32> {
    let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
    if let Some(number) = value.as_f64() {
        return Some(number as f32);
    }
    value.as_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_helpers_accept_resolver_values() {
        assert_eq!(
            parse_json_string(r#""units\\human\\Footman\\Footman""#).as_deref(),
            Some(r"units\human\Footman\Footman")
        );
        assert_eq!(parse_json_f32("0.65"), Some(0.65));
        assert_eq!(parse_json_f32(r#""0.9""#), Some(0.9));
    }

    #[test]
    fn embedded_visual_catalog_contains_current_combat_art() {
        let catalog = load_embedded_visuals().expect("embedded visual catalog parses");
        let projectile = |rawcode: &str| {
            catalog.assets.iter().find(|asset| {
                asset.owner_kind == "units"
                    && asset.owner_rawcode == rawcode
                    && asset.role == "attack1_projectile"
            })
        };
        assert_eq!(
            projectile("e003").map(|asset| asset.model_path.as_str()),
            Some(r"Abilities\Weapons\Arrow\ArrowMissile.mdl")
        );
        assert_eq!(
            projectile("n015").map(|asset| asset.model_path.as_str()),
            Some(r"Abilities\Weapons\LichMissile\LichMissile.mdl")
        );
        assert_eq!(
            projectile("o001").and_then(|asset| asset.missile_arc),
            Some(0.4)
        );
        assert!(
            catalog
                .chain_lightning_abilities
                .iter()
                .any(|rawcode| rawcode == "A05X")
        );
        assert_eq!(
            catalog.stun_model_path.as_deref(),
            Some(r"Abilities\Spells\Human\Thunderclap\ThunderclapTarget.mdl")
        );
        let flame_strike_special: Vec<_> = catalog
            .assets
            .iter()
            .filter(|asset| {
                asset.owner_kind == "abilities"
                    && asset.owner_rawcode == "A01I"
                    && asset.role == "special"
            })
            .map(|asset| asset.model_path.as_str())
            .collect();
        assert_eq!(flame_strike_special.len(), 3);
        assert!(
            catalog
                .assets
                .iter()
                .all(|asset| !asset.model_path.contains("CommandButtons"))
        );
        assert!(catalog.assets.iter().all(|asset| {
            !matches!(
                asset.model_path.trim().to_ascii_lowercase().as_str(),
                ".mdl" | ".mdx" | "none" | "none.mdl" | "none.mdx"
            )
        }));
    }
}
