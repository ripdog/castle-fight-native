use std::{collections::BTreeMap, env, error::Error, fs, path::PathBuf};

use csv::StringRecord;
use serde::Serialize;

#[derive(Serialize)]
struct UnitAssetSpec {
    rawcode: String,
    base_rawcode: String,
    name: String,
    model_path: Option<String>,
    scale: Option<f32>,
}

fn main() {
    if let Err(error) = build_catalog() {
        panic!("failed to build embedded WC3 unit asset catalog: {error}");
    }
}

fn build_catalog() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let original_map = manifest_dir.join("../../docs/original_map");
    let resolved = original_map.join("extracted/resolved");
    let production_path = resolved.join("production-buildings.tsv");
    let object_fields_path = resolved.join("object-fields.tsv");
    println!("cargo:rerun-if-changed={}", production_path.display());
    println!("cargo:rerun-if-changed={}", object_fields_path.display());
    let map_readme = original_map.join("README.md");
    println!("cargo:rerun-if-changed={}", map_readme.display());
    let catalog_version = parse_catalog_version(&fs::read_to_string(&map_readme)?)?;
    println!("cargo:rustc-env=CF_ASSET_CATALOG_VERSION={catalog_version}");

    let units = load_production_units(&production_path, &object_fields_path)?;
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("unit-assets.json");
    fs::write(out, serde_json::to_vec(&units)?)?;
    Ok(())
}

fn load_production_units(
    production_path: &std::path::Path,
    object_fields_path: &std::path::Path,
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
        let base_rawcode = base_rawcodes
            .remove(&rawcode)
            .ok_or_else(|| format!("production unit {rawcode} ({name}) has no base rawcode"))?;
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
