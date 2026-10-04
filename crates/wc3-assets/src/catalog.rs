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
    pub tint_rgb: Option<[u8; 3]>,
    pub attached_visuals: Vec<AttachedVisualSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttachedVisualSpec {
    pub ability_rawcode: String,
    pub attachment_point: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildingAssetSpec {
    pub rawcode: String,
    pub base_rawcode: String,
    pub name: String,
    pub model_path: Option<String>,
    pub scale: Option<f32>,
    pub animation_properties: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VisualAssetSpec {
    pub owner_kind: String,
    pub owner_rawcode: String,
    pub source_unit_rawcode: Option<String>,
    pub role: String,
    pub model_path: String,
    pub missile_arc: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusVisualSpec {
    pub ability_rawcode: String,
    pub status_kind: String,
    pub model_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VisualAssetCatalog {
    pub assets: Vec<VisualAssetSpec>,
    pub status_visuals: Vec<StatusVisualSpec>,
    pub chain_lightning_abilities: Vec<String>,
    pub stun_model_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiAssetSpec {
    pub owner_kind: String,
    pub owner_rawcode: String,
    pub role: String,
    pub texture_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiAssetCatalog {
    pub assets: Vec<UiAssetSpec>,
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

pub fn load_embedded_buildings() -> Result<Vec<BuildingAssetSpec>, Box<dyn Error>> {
    let json = include_str!(concat!(env!("OUT_DIR"), "/building-assets.json"));
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

pub fn load_embedded_ui() -> Result<UiAssetCatalog, Box<dyn Error>> {
    let json = include_str!(concat!(env!("OUT_DIR"), "/ui-assets.json"));
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
    let mut tints = BTreeMap::<String, [u8; 3]>::new();
    let mut unit_abilities = BTreeMap::<String, Vec<String>>::new();
    let mut ability_base = BTreeMap::<String, String>::new();
    let mut ability_target_art = std::collections::BTreeSet::<String>::new();
    let mut ability_attachment = BTreeMap::<String, String>::new();
    for row in fields.records() {
        let row = row?;
        let Some(rawcode) = row.get(rawcode_col) else {
            continue;
        };
        if row.get(category) == Some("abilities") {
            if let Some(base) = row.get(base_rawcode_col) {
                ability_base.insert(rawcode.to_owned(), base.to_owned());
            }
            match row.get(field_id) {
                Some("atat")
                    if parse_json_string(row.get(recovered).unwrap_or_default()).is_some() =>
                {
                    ability_target_art.insert(rawcode.to_owned());
                }
                Some("ata0") => {
                    if let Some(point) = parse_json_string(row.get(recovered).unwrap_or_default()) {
                        ability_attachment.insert(rawcode.to_owned(), point);
                    }
                }
                _ => {}
            }
            continue;
        }
        if row.get(category) != Some("units") {
            continue;
        }
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
            Some("uabi") => {
                unit_abilities.insert(
                    rawcode.to_owned(),
                    parse_json_comma_list(row.get(recovered).unwrap_or_default()),
                );
            }
            Some("uclr" | "uclg" | "uclb") => {
                let channel = match row.get(field_id) {
                    Some("uclr") => 0,
                    Some("uclg") => 1,
                    _ => 2,
                };
                if let Some(value) = parse_json_u8(row.get(recovered).unwrap_or_default()) {
                    tints.entry(rawcode.to_owned()).or_insert([255; 3])[channel] = value;
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
            tint_rgb: tints.remove(&rawcode).filter(|tint| *tint != [255; 3]),
            attached_visuals: unit_abilities
                .remove(&rawcode)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|ability_rawcode| {
                    (ability_base
                        .get(&ability_rawcode)
                        .is_some_and(|base| base == "Aasl")
                        && ability_target_art.contains(&ability_rawcode))
                    .then(|| AttachedVisualSpec {
                        attachment_point: ability_attachment
                            .get(&ability_rawcode)
                            .cloned()
                            .unwrap_or_else(|| "origin".to_owned()),
                        ability_rawcode,
                    })
                })
                .collect(),
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

fn parse_json_u8(value: &str) -> Option<u8> {
    let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
    value
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn parse_json_comma_list(value: &str) -> Vec<String> {
    parse_json_string(value)
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
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
    fn shrine_system_visual_uses_retained_callback_art_projection() {
        let projection: serde_json::Value = serde_json::from_str(include_str!(
            "../../sim/data/castle-fight/9.27/shrine-system-r1.json"
        ))
        .unwrap();
        let catalog = load_embedded_visuals().unwrap();
        let bindings: Vec<_> = catalog
            .assets
            .iter()
            .filter(|asset| asset.owner_kind == "systems" && asset.role == "resurrection")
            .collect();
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0].model_path,
            projection["resurrection_model"].as_str().unwrap()
        );
        assert_eq!(
            bindings[0].owner_rawcode.as_bytes(),
            (projection["parameters"]["golden_shrine_unit_id"]
                .as_u64()
                .unwrap() as u32)
                .to_be_bytes()
        );
    }

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
    fn embedded_unit_catalog_covers_resolved_non_building_objects() {
        let units = load_embedded_units().expect("embedded unit catalog loads");
        let rawcodes = units
            .iter()
            .map(|unit| unit.rawcode.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(rawcodes.len(), units.len(), "unit rawcodes must be unique");
        assert!(
            rawcodes.contains("h030"),
            "runtime Greater Fire Elemental body is outside the production-building roster"
        );
        assert!(
            rawcodes.contains("e008"),
            "intentionally invisible helper units still belong to the asset inventory"
        );
        assert!(
            !rawcodes.contains("h000"),
            "building rawcodes must remain in the building asset catalog"
        );

        let builder = units
            .iter()
            .find(|unit| unit.rawcode == "X00C")
            .expect("Human Builder should be exportable as a unit asset");
        assert_eq!(builder.base_rawcode, "hpea");
        assert_eq!(builder.name, "Human Builder");
        assert_eq!(
            builder.model_path.as_deref(),
            Some(r"units\human\Peasant\Peasant")
        );
        assert_eq!(builder.scale, Some(1.0));

        let defender = units
            .iter()
            .find(|unit| unit.rawcode == "h03A")
            .expect("Defender should be exportable as a unit asset");
        assert_eq!(defender.base_rawcode, "hfoo");
        assert_eq!(defender.name, "Defender");
        assert_eq!(
            defender.model_path.as_deref(),
            Some(r"units\human\TheCaptain\TheCaptain.mdl")
        );
    }

    #[test]
    fn embedded_building_catalog_contains_current_native_slice() {
        let buildings = load_embedded_buildings().expect("embedded building catalog loads");
        let building = |rawcode: &str| {
            buildings
                .iter()
                .find(|building| building.rawcode == rawcode)
                .unwrap_or_else(|| panic!("missing current building {rawcode}"))
        };
        let expected = [
            ("hcas", r"buildings\human\TownHall\TownHall", 1.2),
            (
                "h000",
                r"buildings\human\HumanBarracks\HumanBarracks.mdl",
                0.5,
            ),
            ("h039", r"buildings\human\TownHall\TownHall.mdl", 0.37),
            (
                "h03D",
                r"buildings\nightelf\HuntersHall\HuntersHall.mdl",
                0.52,
            ),
            ("h02I", r"buildings\orc\WarMill\WarMill.mdl", 0.5),
            (
                "h03K",
                r"buildings\other\IceTrollHut1\IceTrollHut1.mdl",
                0.6,
            ),
            (
                "h015",
                r"buildings\human\GryphonAviary\GryphonAviary.mdl",
                0.5,
            ),
            ("h006", r"buildings\human\HumanTower\HumanTower", 0.8),
            ("h07P", r"war3mapImported\PandarenTower.mdl", 0.8),
        ];
        for (rawcode, model, scale) in expected {
            let actual = building(rawcode);
            assert_eq!(actual.model_path.as_deref(), Some(model), "{rawcode} model");
            assert_eq!(actual.scale, Some(scale), "{rawcode} scale");
        }
        assert_eq!(
            building("hcas").animation_properties,
            vec!["upgrade".to_owned(), "second".to_owned()]
        );
        assert_eq!(
            building("h006").animation_properties,
            vec!["upgrade".to_owned(), "first".to_owned()]
        );
        assert_eq!(
            building("h07P").animation_properties,
            vec!["upgrade".to_owned(), "second".to_owned()]
        );
        assert!(building("h000").animation_properties.is_empty());
        assert_eq!(
            building("h039").animation_properties,
            vec!["upgrade".to_owned(), "second".to_owned()]
        );
    }

    #[test]
    fn embedded_ui_catalog_contains_object_and_resource_icons() {
        let catalog = load_embedded_ui().expect("embedded UI catalog parses");
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "abilities"
                && asset.owner_rawcode == "A03W"
                && asset.role == "normal"
                && asset.texture_path
                    == r"ReplaceableTextures\PassiveButtons\PASBTNFreezingBreath.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "buffs"
                && asset.owner_rawcode == "B005"
                && asset.role == "buff"
                && asset.texture_path == r"ReplaceableTextures\CommandButtons\BTNStun.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "resources"
                && asset.owner_rawcode == "gold"
                && asset.role == "bar"
                && asset.texture_path == r"UI\Feedback\Resources\ResourceGold.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "resources"
                && asset.owner_rawcode == "lumber"
                && asset.role == "bar"
                && asset.texture_path == r"UI\Feedback\Resources\ResourceLumber.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "commands"
                && asset.owner_rawcode == "move"
                && asset.role == "command"
                && asset.texture_path == r"ReplaceableTextures\CommandButtons\BTNMove.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "commands"
                && asset.owner_rawcode == "cancel"
                && asset.role == "command"
                && asset.texture_path == r"ReplaceableTextures\CommandButtons\BTNCancel.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "feedback"
                && asset.owner_rawcode == "autocast"
                && asset.role == "particle"
                && asset.texture_path == r"Textures\HeroLevel-Particle.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "info_panel"
                && asset.owner_rawcode == "damage_pierce"
                && asset.role == "icon"
                && asset.texture_path
                    == r"UI\Widgets\Console\Human\infocard-neutral-attack-piercing.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "info_panel"
                && asset.owner_rawcode == "armor_hero"
                && asset.role == "icon"
                && asset.texture_path == r"war3mapimported\infocard-neutral-armor-hero.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "info_panel"
                && asset.owner_rawcode == "armor_medium"
                && asset.role == "icon"
                && asset.texture_path
                    == r"UI\Widgets\Console\Human\infocard-neutral-armor-medium.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "status_effects"
                && asset.owner_rawcode == "A03M"
                && asset.role == "primary"
                && asset.texture_path == r"ReplaceableTextures\CommandButtons\BTNInnerFire.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "cursors"
                && asset.owner_rawcode == "human"
                && asset.role == "atlas"
                && asset.texture_path == r"UI\Cursor\HumanCursor.blp"
        }));
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "cursors"
                && asset.owner_rawcode == "night_elf"
                && asset.role == "atlas"
                && asset.texture_path == r"UI\Cursor\NightElfCursor.blp"
        }));
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
        assert!(catalog.assets.iter().any(|asset| {
            asset.owner_kind == "abilities"
                && asset.owner_rawcode == "A03G"
                && asset.role == "caster"
                && asset.model_path == r"Abilities\Spells\Human\Defend\DefendCaster.mdl"
        }));
        for role in ["caster", "target"] {
            assert!(catalog.assets.iter().any(|asset| {
                asset.owner_kind == "abilities"
                    && asset.owner_rawcode == "A0HN"
                    && asset.source_unit_rawcode.as_deref() == Some("h07U")
                    && asset.role == role
                    && asset.model_path == r"HolyBlast.mdx"
            }));
        }
        assert!(catalog.status_visuals.iter().any(|visual| {
            visual.ability_rawcode == "A03W"
                && visual.status_kind == "movement"
                && visual.model_path
                    == r"Abilities\Spells\Undead\FreezingBreath\FreezingBreathTargetArt.mdl"
        }));
        assert!(catalog.status_visuals.iter().any(|visual| {
            visual.ability_rawcode == "A03Z"
                && visual.status_kind == "armor"
                && visual.model_path == r"Abilities\Spells\Undead\FrostArmor\FrostArmorTarget.mdl"
        }));
        assert!(catalog.status_visuals.iter().any(|visual| {
            visual.ability_rawcode == "A03Z"
                && visual.status_kind == "movement"
                && visual.model_path == r"Abilities\Spells\Other\FrostDamage\FrostDamage.mdl"
        }));
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
