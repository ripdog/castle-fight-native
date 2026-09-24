use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs,
    path::PathBuf,
};

use csv::StringRecord;
use serde::Serialize;

#[derive(Serialize)]
struct UnitAssetSpec {
    rawcode: String,
    base_rawcode: String,
    name: String,
    model_path: Option<String>,
    scale: Option<f32>,
    tint_rgb: Option<[u8; 3]>,
    attached_visuals: Vec<AttachedVisualSpec>,
}

#[derive(Clone, Serialize)]
struct AttachedVisualSpec {
    ability_rawcode: String,
    attachment_point: String,
}

#[derive(Serialize)]
struct BuildingAssetSpec {
    rawcode: String,
    base_rawcode: String,
    name: String,
    model_path: Option<String>,
    scale: Option<f32>,
    animation_properties: Vec<String>,
}

#[derive(Serialize)]
struct DoodadAssetSpec {
    rawcode: String,
    base_rawcode: String,
    object_kind: String,
    name: String,
    model_path: Option<String>,
    num_variations: Option<u32>,
    placements: Vec<DoodadPlacementSpec>,
}

#[derive(Serialize)]
struct VisualAssetSpec {
    owner_kind: String,
    owner_rawcode: String,
    role: String,
    model_path: String,
    missile_arc: Option<f32>,
}

#[derive(Serialize)]
struct StatusVisualSpec {
    ability_rawcode: String,
    status_kind: String,
    model_path: String,
}

#[derive(Serialize)]
struct VisualAssetCatalog {
    assets: Vec<VisualAssetSpec>,
    status_visuals: Vec<StatusVisualSpec>,
    chain_lightning_abilities: Vec<String>,
    stun_model_path: Option<String>,
}

#[derive(Serialize)]
struct UiAssetSpec {
    owner_kind: String,
    owner_rawcode: String,
    role: String,
    texture_path: String,
}

#[derive(Serialize)]
struct UiAssetCatalog {
    assets: Vec<UiAssetSpec>,
}

#[derive(Serialize)]
struct DoodadPlacementSpec {
    editor_id: u32,
    position: [f32; 3],
    angle_degrees: f32,
    scale: [f32; 3],
    visible: bool,
    solid: bool,
    fixed_z: bool,
    variation: u32,
}

struct DoodadBuilder {
    rawcode: String,
    base_rawcode: String,
    object_kind: String,
    name: String,
    model_path: Option<String>,
    num_variations: Option<u32>,
    placements: Vec<DoodadPlacementSpec>,
}

fn main() {
    if let Err(error) = build_catalog() {
        panic!("failed to build embedded WC3 asset catalogs: {error}");
    }
}

fn build_catalog() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let original_map = manifest_dir.join("../../docs/original_map");
    let resolved = original_map.join("extracted/resolved");
    let units_path = resolved.join("units.tsv");
    let buildings_path = resolved.join("buildings.tsv");
    let object_fields_path = resolved.join("object-fields.tsv");
    let placed_doodads_path = resolved.join("placed-doodads.tsv");
    println!("cargo:rerun-if-changed={}", units_path.display());
    println!("cargo:rerun-if-changed={}", buildings_path.display());
    println!("cargo:rerun-if-changed={}", object_fields_path.display());
    println!("cargo:rerun-if-changed={}", placed_doodads_path.display());
    let map_readme = original_map.join("README.md");
    println!("cargo:rerun-if-changed={}", map_readme.display());
    let catalog_version = parse_catalog_version(&fs::read_to_string(&map_readme)?)?;
    println!("cargo:rustc-env=CF_ASSET_CATALOG_VERSION={catalog_version}");

    let units = load_unit_assets(&units_path, &object_fields_path)?;
    let buildings = load_buildings(&buildings_path, &object_fields_path)?;
    let doodads = load_placed_doodads(&placed_doodads_path, &object_fields_path)?;
    let visuals = load_visual_assets(&object_fields_path)?;
    let ui = load_ui_assets(&object_fields_path)?;
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(
        out_dir.join("unit-assets.json"),
        serde_json::to_vec(&units)?,
    )?;
    fs::write(
        out_dir.join("building-assets.json"),
        serde_json::to_vec(&buildings)?,
    )?;
    fs::write(
        out_dir.join("doodad-assets.json"),
        serde_json::to_vec(&doodads)?,
    )?;
    fs::write(
        out_dir.join("visual-assets.json"),
        serde_json::to_vec(&visuals)?,
    )?;
    fs::write(out_dir.join("ui-assets.json"), serde_json::to_vec(&ui)?)?;
    Ok(())
}

fn load_unit_assets(
    units_path: &std::path::Path,
    object_fields_path: &std::path::Path,
) -> Result<Vec<UnitAssetSpec>, Box<dyn Error>> {
    let mut unit_rows = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(units_path)?;
    let unit_headers = unit_rows.headers()?.clone();
    let unit_rawcode = header_index(&unit_headers, "rawcode")?;
    let unit_base_rawcode = header_index(&unit_headers, "base_rawcode")?;
    let unit_name = header_index(&unit_headers, "name")?;
    let unit_is_building = header_index(&unit_headers, "is_building")?;

    let mut units = BTreeMap::<String, (String, String)>::new();
    for row in unit_rows.records() {
        let row = row?;
        if row.get(unit_is_building) == Some("1") {
            continue;
        }
        let rawcode = row.get(unit_rawcode).unwrap_or_default().trim();
        if rawcode.is_empty() {
            continue;
        }
        let base_rawcode = row.get(unit_base_rawcode).unwrap_or_default().trim();
        if base_rawcode.is_empty() {
            return Err(format!("unit {rawcode} has no resolved base rawcode").into());
        }
        let name = row.get(unit_name).unwrap_or_default().trim();
        units.insert(
            rawcode.to_owned(),
            (base_rawcode.to_owned(), name.to_owned()),
        );
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

    let mut model_paths = BTreeMap::<String, String>::new();
    let mut scales = BTreeMap::<String, f32>::new();
    let mut tints = BTreeMap::<String, [u8; 3]>::new();
    let mut unit_abilities = BTreeMap::<String, Vec<String>>::new();
    let mut ability_base = BTreeMap::<String, String>::new();
    let mut ability_target_art = BTreeSet::<String>::new();
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
    for (rawcode, (base_rawcode, name)) in units {
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

fn load_buildings(
    buildings_path: &std::path::Path,
    object_fields_path: &std::path::Path,
) -> Result<Vec<BuildingAssetSpec>, Box<dyn Error>> {
    let mut buildings = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(buildings_path)?;
    let building_headers = buildings.headers()?.clone();
    let rawcode_col = header_index(&building_headers, "rawcode")?;
    let base_rawcode_col = header_index(&building_headers, "base_rawcode")?;
    let name_col = header_index(&building_headers, "name")?;

    let mut specs = BTreeMap::<String, BuildingAssetSpec>::new();
    for row in buildings.records() {
        let row = row?;
        let rawcode = required(&row, rawcode_col, "rawcode")?.to_owned();
        specs
            .entry(rawcode.clone())
            .or_insert_with(|| BuildingAssetSpec {
                base_rawcode: row
                    .get(base_rawcode_col)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                name: row.get(name_col).unwrap_or_default().trim().to_owned(),
                rawcode,
                model_path: None,
                scale: None,
                animation_properties: Vec::new(),
            });
    }

    let mut fields = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(object_fields_path)?;
    let headers = fields.headers()?.clone();
    let category = header_index(&headers, "category")?;
    let rawcode_field = header_index(&headers, "rawcode")?;
    let field_id = header_index(&headers, "field_id")?;
    let recovered = header_index(&headers, "recovered_value_json")?;
    for row in fields.records() {
        let row = row?;
        if row.get(category) != Some("units") {
            continue;
        }
        let Some(rawcode) = row.get(rawcode_field) else {
            continue;
        };
        let Some(spec) = specs.get_mut(rawcode) else {
            continue;
        };
        match row.get(field_id) {
            Some("umdl") => {
                spec.model_path = parse_json_string(row.get(recovered).unwrap_or_default());
            }
            Some("usca") => {
                spec.scale = parse_json_f32(row.get(recovered).unwrap_or_default());
            }
            Some("uani") => {
                spec.animation_properties =
                    parse_json_comma_list(row.get(recovered).unwrap_or_default());
            }
            _ => {}
        }
    }

    for building in specs.values() {
        if building.base_rawcode.is_empty() {
            return Err(format!(
                "building {} ({}) has no resolved base rawcode",
                building.rawcode, building.name
            )
            .into());
        }
    }
    Ok(specs.into_values().collect())
}

fn load_placed_doodads(
    placed_path: &std::path::Path,
    object_fields_path: &std::path::Path,
) -> Result<Vec<DoodadAssetSpec>, Box<dyn Error>> {
    let mut placed = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(placed_path)?;
    let headers = placed.headers()?.clone();
    let rawcode = header_index(&headers, "rawcode")?;
    let object_kind = header_index(&headers, "object_kind")?;
    let name = header_index(&headers, "name")?;
    let editor_id = header_index(&headers, "editor_id")?;
    let x = header_index(&headers, "x")?;
    let y = header_index(&headers, "y")?;
    let z = header_index(&headers, "z")?;
    let angle = header_index(&headers, "angle_degrees")?;
    let scale_x = header_index(&headers, "scale_x")?;
    let scale_y = header_index(&headers, "scale_y")?;
    let scale_z = header_index(&headers, "scale_z")?;
    let visible = header_index(&headers, "visible")?;
    let solid = header_index(&headers, "solid")?;
    let fixed_z = header_index(&headers, "fixed_z")?;
    let variation = header_index(&headers, "variation")?;

    let mut objects = BTreeMap::<String, DoodadBuilder>::new();
    for row in placed.records() {
        let row = row?;
        let rawcode = required(&row, rawcode, "rawcode")?.to_owned();
        let kind = required(&row, object_kind, "object_kind")?.to_owned();
        let object = objects
            .entry(rawcode.clone())
            .or_insert_with(|| DoodadBuilder {
                rawcode: rawcode.clone(),
                base_rawcode: rawcode.clone(),
                object_kind: kind.clone(),
                name: row.get(name).unwrap_or_default().to_owned(),
                model_path: None,
                num_variations: None,
                placements: Vec::new(),
            });
        if object.object_kind != kind {
            return Err(
                format!("placed rawcode {rawcode} appears as multiple object kinds").into(),
            );
        }
        object.placements.push(DoodadPlacementSpec {
            editor_id: parse_required(&row, editor_id, "editor_id")?,
            position: [
                parse_required(&row, x, "x")?,
                parse_required(&row, y, "y")?,
                parse_required(&row, z, "z")?,
            ],
            angle_degrees: parse_required(&row, angle, "angle_degrees")?,
            scale: [
                parse_required(&row, scale_x, "scale_x")?,
                parse_required(&row, scale_y, "scale_y")?,
                parse_required(&row, scale_z, "scale_z")?,
            ],
            visible: parse_flag(&row, visible, "visible")?,
            solid: parse_flag(&row, solid, "solid")?,
            fixed_z: parse_flag(&row, fixed_z, "fixed_z")?,
            variation: parse_required(&row, variation, "variation")?,
        });
    }

    let mut fields = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(object_fields_path)?;
    let field_headers = fields.headers()?.clone();
    let category = header_index(&field_headers, "category")?;
    let rawcode_col = header_index(&field_headers, "rawcode")?;
    let base_rawcode_col = header_index(&field_headers, "base_rawcode")?;
    let field_id = header_index(&field_headers, "field_id")?;
    let recovered = header_index(&field_headers, "recovered_value_json")?;
    for row in fields.records() {
        let row = row?;
        let Some(rawcode) = row.get(rawcode_col) else {
            continue;
        };
        let Some(object) = objects.get_mut(rawcode) else {
            continue;
        };
        let expected_category = if object.object_kind == "destructable" {
            "destructables"
        } else {
            "doodads"
        };
        if row.get(category) != Some(expected_category) {
            continue;
        }
        if let Some(base) = row.get(base_rawcode_col).filter(|value| !value.is_empty()) {
            object.base_rawcode = base.to_owned();
        }
        match row.get(field_id) {
            Some("dfil") | Some("bfil") => {
                if let Some(path) = parse_json_string(row.get(recovered).unwrap_or_default()) {
                    object.model_path = Some(path);
                }
            }
            Some("dvar") | Some("bvar") => {
                if let Some(count) = parse_json_u32(row.get(recovered).unwrap_or_default()) {
                    object.num_variations = Some(count);
                }
            }
            _ => {}
        }
    }

    Ok(objects
        .into_values()
        .map(|object| DoodadAssetSpec {
            rawcode: object.rawcode,
            base_rawcode: object.base_rawcode,
            object_kind: object.object_kind,
            name: object.name,
            model_path: object.model_path,
            num_variations: object.num_variations,
            placements: object.placements,
        })
        .collect())
}

fn load_visual_assets(
    object_fields_path: &std::path::Path,
) -> Result<VisualAssetCatalog, Box<dyn Error>> {
    let mut fields = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(object_fields_path)?;
    let headers = fields.headers()?.clone();
    let category = header_index(&headers, "category")?;
    let rawcode_col = header_index(&headers, "rawcode")?;
    let base_rawcode_col = header_index(&headers, "base_rawcode")?;
    let field_id = header_index(&headers, "field_id")?;
    let value_type = header_index(&headers, "value_type")?;
    let base_value = header_index(&headers, "base_value_json")?;
    let recovered = header_index(&headers, "recovered_value_json")?;

    let mut assets = BTreeSet::<(String, String, String, String)>::new();
    let mut missile_arcs = BTreeMap::<(String, String), f32>::new();
    let mut ability_base_rawcodes = BTreeMap::<String, String>::new();
    let mut ability_buff_ids = BTreeMap::<String, Vec<String>>::new();
    let mut buff_target_art = BTreeMap::<String, String>::new();
    let mut chain_lightning_abilities = BTreeMap::<String, ()>::new();
    let mut stun_model_path = None;
    for row in fields.records() {
        let row = row?;
        let Some(kind) = row.get(category) else {
            continue;
        };
        let Some(rawcode) = row.get(rawcode_col) else {
            continue;
        };
        let base_rawcode = row.get(base_rawcode_col).unwrap_or_default();
        let Some(field) = row.get(field_id) else {
            continue;
        };

        if kind == "abilities" {
            ability_base_rawcodes
                .entry(rawcode.to_owned())
                .or_insert_with(|| base_rawcode.to_owned());
            if field == "abuf" {
                ability_buff_ids.insert(
                    rawcode.to_owned(),
                    parse_json_comma_list(row.get(recovered).unwrap_or_default()),
                );
            }
        }
        if kind == "buffs"
            && field == "ftat"
            && let Some(path) = parse_json_model_paths(row.get(recovered).unwrap_or_default())
                .into_iter()
                .find(|path| is_renderable_model_path(path))
        {
            buff_target_art.insert(rawcode.to_owned(), path);
        }

        let role = match (kind, field) {
            ("units", "ua1m") => Some("attack1_projectile"),
            ("units", "ua2m") => Some("attack2_projectile"),
            ("abilities", "amat") => Some("missile"),
            ("abilities", "acat") => Some("caster"),
            ("abilities", "aeat") => Some("effect"),
            ("abilities", "atat") => Some("target"),
            ("abilities", "asat") => Some("special"),
            ("buffs", "feat") => Some("effect"),
            ("buffs", "ftat") => Some("target"),
            ("buffs", "fsat") => Some("special"),
            _ => None,
        };
        if kind == "units" {
            let projectile_role = match field {
                "uma1" => Some("attack1_projectile"),
                "uma2" => Some("attack2_projectile"),
                _ => None,
            };
            if let Some(projectile_role) = projectile_role
                && let Some(arc) = parse_json_f32(row.get(recovered).unwrap_or_default())
            {
                missile_arcs.insert((rawcode.to_owned(), projectile_role.to_owned()), arc);
            }
        }

        if let Some(role) = role
            && matches!(row.get(value_type), Some("model") | Some("modelList"))
        {
            for path in parse_json_model_paths(row.get(recovered).unwrap_or_default())
                .into_iter()
                .filter(|path| is_renderable_model_path(path))
            {
                assets.insert((kind.to_owned(), rawcode.to_owned(), role.to_owned(), path));
            }
        }

        if kind == "abilities" && matches!(base_rawcode, "ACcl" | "AOcl") {
            chain_lightning_abilities.insert(rawcode.to_owned(), ());
        }
        if kind == "buffs" && base_rawcode == "BPSE" && field == "ftat" && stun_model_path.is_none()
        {
            stun_model_path = parse_json_string(row.get(base_value).unwrap_or_default())
                .filter(|path| !path.trim().is_empty());
        }
    }

    let mut status_visuals = Vec::new();
    for (ability_rawcode, buffs) in ability_buff_ids {
        let Some(base_rawcode) = ability_base_rawcodes
            .get(&ability_rawcode)
            .map(String::as_str)
        else {
            continue;
        };
        let status_kinds: &[&str] = match base_rawcode {
            // Entangling Roots-family effects hold the target in place for the buff lifetime.
            "Aenr" => &["movement"],
            // Frost Armor applies one persistent shield buff and one reactive slow buff.
            "ACf2" => &["armor", "movement"],
            _ => continue,
        };
        for (buff_rawcode, status_kind) in buffs.iter().zip(status_kinds.iter().copied()) {
            let model_path = buff_target_art
                .get(buff_rawcode)
                .cloned()
                .or_else(|| stock_buff_target_art(buff_rawcode));
            let Some(model_path) = model_path else {
                continue;
            };
            status_visuals.push(StatusVisualSpec {
                ability_rawcode: ability_rawcode.clone(),
                status_kind: status_kind.to_owned(),
                model_path,
            });
        }
    }
    status_visuals.sort_by(|left, right| {
        (&left.ability_rawcode, &left.status_kind, &left.model_path).cmp(&(
            &right.ability_rawcode,
            &right.status_kind,
            &right.model_path,
        ))
    });

    Ok(VisualAssetCatalog {
        assets: assets
            .into_iter()
            .map(|(owner_kind, owner_rawcode, role, model_path)| {
                let missile_arc = missile_arcs
                    .get(&(owner_rawcode.clone(), role.clone()))
                    .copied();
                VisualAssetSpec {
                    owner_kind,
                    owner_rawcode,
                    role,
                    model_path,
                    missile_arc,
                }
            })
            .collect(),
        status_visuals,
        chain_lightning_abilities: chain_lightning_abilities.into_keys().collect(),
        stun_model_path,
    })
}

fn load_ui_assets(object_fields_path: &std::path::Path) -> Result<UiAssetCatalog, Box<dyn Error>> {
    let mut fields = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_path(object_fields_path)?;
    let headers = fields.headers()?.clone();
    let category = header_index(&headers, "category")?;
    let rawcode_col = header_index(&headers, "rawcode")?;
    let field_id = header_index(&headers, "field_id")?;
    let value_type = header_index(&headers, "value_type")?;
    let recovered = header_index(&headers, "recovered_value_json")?;

    let mut assets = BTreeSet::<(String, String, String, String)>::new();
    for row in fields.records() {
        let row = row?;
        if row.get(value_type) != Some("icon") {
            continue;
        }
        let Some(owner_kind) = row.get(category) else {
            continue;
        };
        let Some(owner_rawcode) = row.get(rawcode_col) else {
            continue;
        };
        let Some(field) = row.get(field_id) else {
            continue;
        };
        let Some(texture_path) = parse_json_string(row.get(recovered).unwrap_or_default()) else {
            continue;
        };
        let texture_path = texture_path.trim();
        if texture_path.is_empty() || texture_path.eq_ignore_ascii_case("none") {
            continue;
        }
        let role = match field {
            "aart" => "normal",
            "arar" => "research",
            "auar" => "turn_off",
            "fart" => "buff",
            "uico" => "game_interface",
            "iico" => "interface",
            "ucua" => "caster_upgrade",
            _ => field,
        };
        assets.insert((
            owner_kind.to_owned(),
            owner_rawcode.to_owned(),
            role.to_owned(),
            texture_path.to_owned(),
        ));
    }

    for (resource, texture_path) in [
        ("gold", r"UI\Feedback\Resources\ResourceGold.blp"),
        ("lumber", r"UI\Feedback\Resources\ResourceLumber.blp"),
        ("supply", r"UI\Feedback\Resources\ResourceSupply.blp"),
        ("upkeep", r"UI\Feedback\Resources\ResourceUpkeep.blp"),
    ] {
        assets.insert((
            "resources".to_owned(),
            resource.to_owned(),
            "bar".to_owned(),
            texture_path.to_owned(),
        ));
    }

    // Stock engine commands are not object-editor rows, but they are still part of the
    // Warcraft command card presentation. Keep them in the same generated UI catalog so
    // clients do not grow a second set of handwritten asset paths.
    for (command, texture_path) in [
        ("move", r"ReplaceableTextures\CommandButtons\BTNMove.blp"),
        (
            "attack",
            r"ReplaceableTextures\CommandButtons\BTNAttack.blp",
        ),
        (
            "build_human",
            r"ReplaceableTextures\CommandButtons\BTNHumanBuild.blp",
        ),
        (
            "cancel",
            r"ReplaceableTextures\CommandButtons\BTNCancel.blp",
        ),
    ] {
        assets.insert((
            "commands".to_owned(),
            command.to_owned(),
            "command".to_owned(),
            texture_path.to_owned(),
        ));
    }

    // Warcraft cursor art is a fixed-layout sprite sheet. Keep every stock race atlas in the
    // generated presentation catalog even though Castle Fight 9.27 currently uses the Human
    // cursor theme. The client selects semantic frames from the atlas rather than embedding
    // texture paths or copying cursor pixels into source code.
    for (cursor, texture_path) in [
        ("human", r"UI\Cursor\HumanCursor.blp"),
        ("orc", r"UI\Cursor\OrcCursor.blp"),
        ("undead", r"UI\Cursor\UndeadCursor.blp"),
        ("night_elf", r"UI\Cursor\NightElfCursor.blp"),
    ] {
        assets.insert((
            "cursors".to_owned(),
            cursor.to_owned(),
            "atlas".to_owned(),
            texture_path.to_owned(),
        ));
    }

    Ok(UiAssetCatalog {
        assets: assets
            .into_iter()
            .map(
                |(owner_kind, owner_rawcode, role, texture_path)| UiAssetSpec {
                    owner_kind,
                    owner_rawcode,
                    role,
                    texture_path,
                },
            )
            .collect(),
    })
}

fn stock_buff_target_art(rawcode: &str) -> Option<String> {
    let path = match rawcode {
        // These are stock Warcraft III buff objects referenced by Frost Armor's inherited
        // BUfa/Bfro buff list. They do not appear as standalone rows in Castle Fight's custom
        // object-data delta, so retain the stock presentation lookup alongside the extractor.
        "BUfa" => r"Abilities\Spells\Undead\FrostArmor\FrostArmorTarget.mdl",
        "Bfro" => r"Abilities\Spells\Other\FrostDamage\FrostDamage.mdl",
        _ => return None,
    };
    Some(path.to_owned())
}

fn parse_catalog_version(readme: &str) -> Result<String, Box<dyn Error>> {
    const MARKER: &str = "Castle Fight DE Beta ";
    let after = readme
        .split_once(MARKER)
        .map(|(_, after)| after)
        .ok_or("original map README does not contain Castle Fight DE Beta version")?;
    let version: String = after
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    if version.is_empty() {
        return Err("original map README contains an empty Castle Fight version".into());
    }
    Ok(version)
}

fn header_index(headers: &StringRecord, name: &str) -> Result<usize, Box<dyn Error>> {
    headers
        .iter()
        .position(|header| header == name)
        .ok_or_else(|| format!("missing TSV column {name:?}").into())
}

fn required<'a>(
    row: &'a StringRecord,
    index: usize,
    name: &str,
) -> Result<&'a str, Box<dyn Error>> {
    row.get(index)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing required TSV value {name:?}").into())
}

fn parse_required<T>(row: &StringRecord, index: usize, name: &str) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = required(row, index, name)?;
    value
        .parse()
        .map_err(|error| format!("invalid {name} value {value:?}: {error}").into())
}

fn parse_flag(row: &StringRecord, index: usize, name: &str) -> Result<bool, Box<dyn Error>> {
    match required(row, index, name)? {
        "0" => Ok(false),
        "1" => Ok(true),
        value => Err(format!("invalid {name} flag {value:?}").into()),
    }
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
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_json_model_paths(value: &str) -> Vec<String> {
    parse_json_string(value)
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_renderable_model_path(path: &str) -> bool {
    !matches!(
        path.trim().to_ascii_lowercase().as_str(),
        ".mdl" | ".mdx" | "none" | "none.mdl" | "none.mdx"
    )
}

fn parse_json_f32(value: &str) -> Option<f32> {
    let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
    if let Some(number) = value.as_f64() {
        return Some(number as f32);
    }
    value.as_str()?.parse().ok()
}

fn parse_json_u32(value: &str) -> Option<u32> {
    let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
    if let Some(number) = value.as_u64() {
        return u32::try_from(number).ok();
    }
    value.as_str()?.parse().ok()
}
