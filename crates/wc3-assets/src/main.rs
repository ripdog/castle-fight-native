mod catalog;
mod export;

use std::{
    collections::BTreeSet,
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use catalog::{
    load_embedded_buildings, load_embedded_doodads, load_embedded_units, load_embedded_visuals,
    load_production_units,
};
use export::Exporter;

fn main() {
    if let Err(error) = run() {
        eprintln!("cf-wc3-assets: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse(env::args().skip(1))?;
    if args.help {
        print_usage();
        return Ok(());
    }
    if args.art_mode != "sd" {
        return Err(io::Error::other(
            "only --art sd is implemented in the initial exporter; HD/Reforged PBR materials need a separate material mapping",
        )
        .into());
    }

    let wc3_install = args
        .wc3_install
        .or_else(|| env::var_os("WC3_INSTALL").map(PathBuf::from))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing --wc3 PATH (or WC3_INSTALL environment variable)",
            )
        })?;
    let output = args.output.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing required --output PATH",
        )
    })?;
    verify_install(&wc3_install)?;

    if args.effects {
        if args.buildings
            || !args.building_filters.is_empty()
            || args.doodads
            || !args.doodad_filters.is_empty()
            || args.production.is_some()
            || args.object_fields.is_some()
            || !args.units.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--effects cannot be combined with unit, building, or doodad options",
            )
            .into());
        }
        let visuals = load_embedded_visuals()?;
        println!(
            "Extracting {} Castle Fight projectile/effect art reference(s) from {}",
            visuals.assets.len() + usize::from(visuals.stun_model_path.is_some()),
            wc3_install.display()
        );
        let mut exporter = Exporter::open(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        let manifest = exporter.export_visuals(&visuals)?;
        fs::write(
            output.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        println!(
            "Exported {} unique WC3 visual model(s) to {} ({} unresolved map/import reference(s))",
            manifest.models.len(),
            output.display(),
            manifest.failures.len()
        );
        return Ok(());
    }

    if args.buildings || !args.building_filters.is_empty() {
        if args.doodads
            || !args.doodad_filters.is_empty()
            || args.production.is_some()
            || args.object_fields.is_some()
            || !args.units.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--buildings/--building cannot be combined with unit or doodad options",
            )
            .into());
        }
        let mut buildings = load_embedded_buildings()?;
        if !args.building_filters.is_empty() {
            let requested: BTreeSet<_> = args.building_filters.iter().map(String::as_str).collect();
            let available: BTreeSet<_> = buildings
                .iter()
                .map(|building| building.rawcode.as_str())
                .collect();
            let unknown: Vec<_> = requested.difference(&available).copied().collect();
            if !unknown.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "requested building rawcode(s) not in resolved building catalog: {}",
                        unknown.join(", ")
                    ),
                )
                .into());
            }
            buildings.retain(|building| requested.contains(building.rawcode.as_str()));
        }
        println!(
            "Extracting {} Castle Fight building asset definition(s) from {}",
            buildings.len(),
            wc3_install.display()
        );
        let mut exporter = Exporter::open(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        let manifest = exporter.export_buildings(&buildings)?;
        fs::write(
            output.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        println!(
            "Exported {} unique model(s) for {} building(s) to {}",
            manifest.models.len(),
            manifest.buildings.len(),
            output.display()
        );
        if !manifest.failures.is_empty() {
            eprintln!(
                "{} building model(s) could not be exported:",
                manifest.failures.len()
            );
            for failure in &manifest.failures {
                eprintln!(
                    "  {} (buildings {}): {}",
                    failure.source_model,
                    failure.buildings.join(","),
                    failure.error
                );
            }
            return Err(io::Error::other(
                "building asset extraction completed with model failures; see manifest.json",
            )
            .into());
        }
        return Ok(());
    }

    if args.doodads || !args.doodad_filters.is_empty() {
        if args.production.is_some() || args.object_fields.is_some() || !args.units.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--doodads/--doodad cannot be combined with unit catalog/filter options",
            )
            .into());
        }
        let mut doodads = load_embedded_doodads()?;
        if !args.doodad_filters.is_empty() {
            let requested: BTreeSet<_> = args.doodad_filters.iter().map(String::as_str).collect();
            let available: BTreeSet<_> = doodads
                .iter()
                .map(|doodad| doodad.rawcode.as_str())
                .collect();
            let unknown: Vec<_> = requested.difference(&available).copied().collect();
            if !unknown.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "requested doodad rawcode(s) not in placed catalog: {}",
                        unknown.join(", ")
                    ),
                )
                .into());
            }
            doodads.retain(|doodad| requested.contains(doodad.rawcode.as_str()));
        }
        let placements: usize = doodads.iter().map(|doodad| doodad.placements.len()).sum();
        println!(
            "Extracting {placements} Castle Fight doodad placement(s) across {} object definition(s) from {}",
            doodads.len(),
            wc3_install.display()
        );
        let mut exporter = Exporter::open(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        let manifest = exporter.export_doodads(&doodads)?;
        fs::write(
            output.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        println!(
            "Exported {} unique model(s) for {} doodad object(s) to {}",
            manifest.models.len(),
            manifest.objects.len(),
            output.display()
        );
        if !manifest.failures.is_empty() {
            eprintln!(
                "{} doodad model variant(s) could not be exported:",
                manifest.failures.len()
            );
            for failure in &manifest.failures {
                eprintln!(
                    "  {} variation {} ({}): {}",
                    failure.rawcode, failure.variation, failure.source_model, failure.error
                );
            }
            return Err(io::Error::other(
                "doodad asset extraction completed with model failures; see manifest.json",
            )
            .into());
        }
        return Ok(());
    }

    let mut units = match (&args.production, &args.object_fields) {
        (None, None) => load_embedded_units()?,
        (Some(production), Some(object_fields)) => {
            load_production_units(production, object_fields)?
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--production and --object-fields must be supplied together",
            )
            .into());
        }
    };
    if !args.units.is_empty() {
        let requested: BTreeSet<_> = args.units.iter().map(String::as_str).collect();
        let available: BTreeSet<_> = units.iter().map(|unit| unit.rawcode.as_str()).collect();
        let unknown: Vec<_> = requested.difference(&available).copied().collect();
        if !unknown.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "requested unit rawcode(s) not in production catalog: {}",
                    unknown.join(", ")
                ),
            )
            .into());
        }
        units.retain(|unit| requested.contains(unit.rawcode.as_str()));
    }

    println!(
        "Extracting {} Castle Fight unit asset definition(s) from {}",
        units.len(),
        wc3_install.display()
    );
    let mut exporter = Exporter::open(
        &wc3_install,
        args.map_archive.as_deref(),
        &output,
        args.keep_source,
    )?;
    let manifest = exporter.export_units(&units)?;
    fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    println!(
        "Exported {} unique model(s) for {} unit(s) to {}",
        manifest.models.len(),
        manifest.units.len(),
        output.display()
    );
    if !manifest.failures.is_empty() {
        eprintln!(
            "{} model(s) could not be exported:",
            manifest.failures.len()
        );
        for failure in &manifest.failures {
            eprintln!(
                "  {} (units {}): {}",
                failure.source_model,
                failure.units.join(","),
                failure.error
            );
        }
        return Err(io::Error::other(
            "asset extraction completed with model failures; see manifest.json",
        )
        .into());
    }
    Ok(())
}

fn verify_install(path: &Path) -> Result<(), Box<dyn Error>> {
    if !path.join(".build.info").is_file() && !path.join("Data").is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} does not look like a Warcraft III installation (no .build.info or Data directory)",
                path.display()
            ),
        )
        .into());
    }
    Ok(())
}

struct Args {
    wc3_install: Option<PathBuf>,
    map_archive: Option<PathBuf>,
    output: Option<PathBuf>,
    production: Option<PathBuf>,
    object_fields: Option<PathBuf>,
    units: Vec<String>,
    buildings: bool,
    building_filters: Vec<String>,
    doodads: bool,
    doodad_filters: Vec<String>,
    effects: bool,
    art_mode: String,
    keep_source: bool,
    help: bool,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut result = Self {
            wc3_install: None,
            map_archive: None,
            output: None,
            production: None,
            object_fields: None,
            units: Vec::new(),
            buildings: false,
            building_filters: Vec::new(),
            doodads: false,
            doodad_filters: Vec::new(),
            effects: false,
            art_mode: "sd".to_owned(),
            keep_source: false,
            help: false,
        };
        let args: Vec<_> = args.collect();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => result.help = true,
                "--keep-source" => result.keep_source = true,
                "--buildings" => result.buildings = true,
                "--doodads" => result.doodads = true,
                "--effects" => result.effects = true,
                "--wc3" => result.wc3_install = Some(PathBuf::from(value(&args, &mut i, "--wc3")?)),
                "--map" => result.map_archive = Some(PathBuf::from(value(&args, &mut i, "--map")?)),
                "--output" | "-o" => {
                    result.output = Some(PathBuf::from(value(&args, &mut i, "--output")?))
                }
                "--production" => {
                    result.production = Some(PathBuf::from(value(&args, &mut i, "--production")?))
                }
                "--object-fields" => {
                    result.object_fields =
                        Some(PathBuf::from(value(&args, &mut i, "--object-fields")?))
                }
                "--unit" => result
                    .units
                    .push(value(&args, &mut i, "--unit")?.to_owned()),
                "--building" => result
                    .building_filters
                    .push(value(&args, &mut i, "--building")?.to_owned()),
                "--doodad" => result
                    .doodad_filters
                    .push(value(&args, &mut i, "--doodad")?.to_owned()),
                "--art" => result.art_mode = value(&args, &mut i, "--art")?.to_ascii_lowercase(),
                unknown => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown argument {unknown:?}; use --help"),
                    )
                    .into());
                }
            }
            i += 1;
        }
        Ok(result)
    }
}

fn value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, Box<dyn Error>> {
    *i += 1;
    args.get(*i).map(String::as_str).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{flag} requires a value"),
        )
        .into()
    })
}

fn print_usage() {
    println!(
        "\
Castle Fight Warcraft III asset extractor

Usage:
  cf-wc3-assets --wc3 PATH --output PATH [options]

Options:
  --wc3 PATH            Warcraft III install root (or set WC3_INSTALL)
  --map PATH            Optional Warcraft III map archive for map-imported assets
  -o, --output PATH     Destination directory for converted assets
  --unit RAWCODE        Export one production unit; repeat for more units
  --buildings           Export every resolved Castle Fight building model
  --building RAWCODE    Export one building; repeat for more buildings
  --doodads             Export every doodad/destructable placed by Castle Fight
  --effects             Export projectile/spell/buff models referenced by Castle Fight
  --doodad RAWCODE      Export one placed doodad type; repeat for more types
  --art sd              Art mode. SD/classic is currently implemented
  --keep-source         Also retain extracted MDX and source texture files
  --production PATH     Development override for production-buildings.tsv
  --object-fields PATH  Development override for resolved object-fields.tsv
  -h, --help            Show this help

With no --unit filters, every production unit in the resolved Castle Fight
catalog is exported. Use --buildings (or --building RAWCODE) for structures and towers,
--doodads (or --doodad RAWCODE) for map decoration assets and exact placements, and
--effects for the visual-effects catalog. Models shared by multiple objects are converted
once. Particle/ribbon metadata is retained in the model
manifest for native presentation even though glTF has no particle-emitter primitive. When
--map is supplied, map-imported models/textures override install assets and are extracted too.
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repeated_unit_filters() {
        let args = Args::parse(
            [
                "--wc3", "/game", "--output", "/out", "--unit", "hfoo", "--unit", "hrif",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(args.units, ["hfoo", "hrif"]);
        assert_eq!(args.art_mode, "sd");
        assert!(args.map_archive.is_none());
    }

    #[test]
    fn building_filter_implies_building_export_mode() {
        let args = Args::parse(
            [
                "--wc3",
                "/game",
                "--output",
                "/out",
                "--building",
                "h000",
                "--building",
                "h006",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(!args.buildings);
        assert_eq!(args.building_filters, ["h000", "h006"]);
    }

    #[test]
    fn parses_optional_map_archive() {
        let args = Args::parse(
            [
                "--wc3",
                "/game",
                "--map",
                "/maps/castle-fight.w3x",
                "--output",
                "/out",
                "--effects",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            args.map_archive.as_deref(),
            Some(Path::new("/maps/castle-fight.w3x"))
        );
        assert!(args.effects);
    }

    #[test]
    fn doodad_filter_implies_doodad_export_mode() {
        let args = Args::parse(
            ["--wc3", "/game", "--output", "/out", "--doodad", "ATtr"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert!(!args.doodads);
        assert_eq!(args.doodad_filters, ["ATtr"]);
    }
}
