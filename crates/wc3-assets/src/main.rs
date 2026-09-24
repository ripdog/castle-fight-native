mod catalog;
mod export;

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::File,
    io::{self, BufWriter},
    path::{Path, PathBuf},
};

use catalog::{
    CATALOG_VERSION, load_embedded_buildings, load_embedded_doodads, load_embedded_ui,
    load_embedded_units, load_embedded_visuals, load_production_units,
};
use export::{EventObjectKindManifest, Exporter, ModelManifest};

#[derive(Debug, serde::Serialize)]
struct CastleFightPackManifest {
    schema_version: u32,
    castle_fight_catalog_version: &'static str,
    wc3_version: Option<String>,
    art_mode: &'static str,
    units: PackSectionManifest,
    buildings: PackSectionManifest,
    doodads: PackSectionManifest,
    effects: PackSectionManifest,
    ui: PackSectionManifest,
    fidelity: ModelFeatureSummary,
}

#[derive(Debug, serde::Serialize)]
struct PackSectionManifest {
    schema_version: u32,
    entries: usize,
    models: usize,
    failures: usize,
    substitutions: usize,
    intentionally_hidden: usize,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FidelityStatus {
    Approximation,
    Unsupported,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
struct FidelityFindingSummary {
    id: String,
    status: FidelityStatus,
    affected_models: usize,
    occurrences: usize,
}

#[derive(Debug, Default, serde::Serialize, PartialEq, Eq)]
struct ModelFeatureSummary {
    model_variants: usize,
    models_with_warnings: usize,
    warning_count: usize,
    multilayer_material_models: usize,
    multilayer_material_count: usize,
    animated_material_alpha_layer_count: usize,
    animated_material_texture_layer_count: usize,
    animated_geoset_alpha_count: usize,
    global_sequence_count: usize,
    attachment_count: usize,
    attachment_models: Vec<String>,
    particle_emitter_count: usize,
    particle_emitter_animated_track_count: usize,
    particle_emitter_2_count: usize,
    particle_emitter_2_animated_track_count: usize,
    ribbon_emitter_count: usize,
    ribbon_emitter_animated_track_count: usize,
    corn_emitter_count: usize,
    corn_emitter_animated_track_count: usize,
    event_object_count: usize,
    event_sound_count: usize,
    event_splat_count: usize,
    event_footprint_count: usize,
    event_spawn_count: usize,
    event_spawn_resolved_count: usize,
    event_uber_splat_count: usize,
    event_unknown_count: usize,
    light_count: usize,
    omni_light_count: usize,
    non_omni_light_count: usize,
    non_inheritance_node_count: usize,
    models_with_more_than_four_classic_skin_influences: usize,
    max_classic_skin_influences: u32,
    approximation_occurrences: usize,
    unsupported_occurrences: usize,
    findings: Vec<FidelityFindingSummary>,
}

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

    if args.castle_fight {
        if args.ui
            || args.effects
            || args.buildings
            || !args.building_filters.is_empty()
            || args.doodads
            || !args.doodad_filters.is_empty()
            || args.production.is_some()
            || args.object_fields.is_some()
            || !args.units.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--castle-fight cannot be combined with individual asset modes, filters, or development catalog overrides",
            )
            .into());
        }
        export_castle_fight_pack(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        return Ok(());
    }

    if args.ui {
        if args.effects
            || args.buildings
            || !args.building_filters.is_empty()
            || args.doodads
            || !args.doodad_filters.is_empty()
            || args.production.is_some()
            || args.object_fields.is_some()
            || !args.units.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--ui cannot be combined with unit, building, doodad, or effect options",
            )
            .into());
        }
        let ui = load_embedded_ui()?;
        println!(
            "Extracting {} Castle Fight/WC3 UI binding(s) from {}",
            ui.assets.len(),
            wc3_install.display()
        );
        let mut exporter = Exporter::open(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        let manifest = exporter.export_ui(&ui)?;
        write_manifest(&output.join("manifest.json"), &manifest)?;
        println!(
            "Exported {} unique WC3 UI texture(s) to {} ({} unresolved texture reference(s))",
            manifest.textures.len(),
            output.display(),
            manifest.failures.len()
        );
        return Ok(());
    }

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
            visuals.assets.len()
                + visuals.status_visuals.len()
                + usize::from(visuals.stun_model_path.is_some()),
            wc3_install.display()
        );
        let mut exporter = Exporter::open(
            &wc3_install,
            args.map_archive.as_deref(),
            &output,
            args.keep_source,
        )?;
        let manifest = exporter.export_visuals(&visuals)?;
        write_manifest(&output.join("manifest.json"), &manifest)?;
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
        write_manifest(&output.join("manifest.json"), &manifest)?;
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
        write_manifest(&output.join("manifest.json"), &manifest)?;
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
                    "requested unit rawcode(s) not in unit asset catalog: {}",
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
    write_manifest(&output.join("manifest.json"), &manifest)?;

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

fn export_castle_fight_pack(
    wc3_install: &Path,
    map_archive: Option<&Path>,
    output: &Path,
    keep_source: bool,
) -> Result<(), Box<dyn Error>> {
    let units = load_embedded_units()?;
    let buildings = load_embedded_buildings()?;
    let doodads = load_embedded_doodads()?;
    let visuals = load_embedded_visuals()?;
    let ui = load_embedded_ui()?;
    let doodad_placements: usize = doodads.iter().map(|doodad| doodad.placements.len()).sum();
    let visual_bindings = visuals.assets.len()
        + visuals.status_visuals.len()
        + usize::from(visuals.stun_model_path.is_some());

    println!(
        "Extracting complete Castle Fight presentation pack from {}: {} unit objects, {} buildings, {} doodad objects / {} placements, {} visual bindings, {} UI bindings",
        wc3_install.display(),
        units.len(),
        buildings.len(),
        doodads.len(),
        doodad_placements,
        visual_bindings,
        ui.assets.len(),
    );

    let units_output = output.join("units");
    let mut exporter = Exporter::open(wc3_install, map_archive, &units_output, keep_source)?;
    let unit_manifest = exporter.export_units(&units)?;
    write_manifest(&units_output.join("manifest.json"), &unit_manifest)?;
    let unit_fallbacks = unit_manifest
        .units
        .iter()
        .filter(|unit| unit.fallback_to_base_art && !unit.intentionally_hidden)
        .count();
    println!(
        "  units: {} object(s), {} unique model(s), {} unresolved model(s), {} base-art substitution(s)",
        unit_manifest.units.len(),
        unit_manifest.models.len(),
        unit_manifest.failures.len(),
        unit_fallbacks
    );

    let buildings_output = output.join("buildings");
    exporter.switch_output(&buildings_output)?;
    let building_manifest = exporter.export_buildings(&buildings)?;
    write_manifest(&buildings_output.join("manifest.json"), &building_manifest)?;
    let building_fallbacks = building_manifest
        .buildings
        .iter()
        .filter(|building| building.fallback_to_base_art)
        .count();
    println!(
        "  buildings: {} object(s), {} unique model(s), {} unresolved model(s), {} base-art substitution(s)",
        building_manifest.buildings.len(),
        building_manifest.models.len(),
        building_manifest.failures.len(),
        building_fallbacks
    );

    let doodads_output = output.join("doodads");
    exporter.switch_output(&doodads_output)?;
    let doodad_manifest = exporter.export_doodads(&doodads)?;
    write_manifest(&doodads_output.join("manifest.json"), &doodad_manifest)?;
    let doodad_fallbacks = doodad_manifest
        .objects
        .iter()
        .flat_map(|object| &object.placements)
        .filter(|placement| placement.fallback_to_base_art)
        .count();
    println!(
        "  doodads: {} object(s), {} unique model(s), {} unresolved model variant(s), {} substituted placement(s)",
        doodad_manifest.objects.len(),
        doodad_manifest.models.len(),
        doodad_manifest.failures.len(),
        doodad_fallbacks
    );

    let effects_output = output.join("effects");
    exporter.switch_output(&effects_output)?;
    let visual_manifest = exporter.export_visuals(&visuals)?;
    write_manifest(&effects_output.join("manifest.json"), &visual_manifest)?;
    println!(
        "  effects: {} unique model(s), {} unresolved model reference(s)",
        visual_manifest.models.len(),
        visual_manifest.failures.len()
    );

    let ui_output = output.join("ui");
    exporter.switch_output(&ui_output)?;
    let ui_manifest = exporter.export_ui(&ui)?;
    write_manifest(&ui_output.join("manifest.json"), &ui_manifest)?;
    println!(
        "  ui: {} unique texture(s), {} unresolved texture reference(s)",
        ui_manifest.textures.len(),
        ui_manifest.failures.len()
    );

    let fidelity = summarize_model_features(
        unit_manifest
            .models
            .iter()
            .chain(building_manifest.models.iter())
            .chain(doodad_manifest.models.iter())
            .chain(visual_manifest.models.iter()),
    );
    println!(
        "  fidelity inventory: {} converted model variant(s), {} warning(s), {} multilayer material(s), {} attachment child model(s), {} PE1 / {} PE2 / {} ribbon / {} CORN emitter(s), {} event object(s), {} parsed light(s)",
        fidelity.model_variants,
        fidelity.warning_count,
        fidelity.multilayer_material_count,
        fidelity.attachment_models.len(),
        fidelity.particle_emitter_count,
        fidelity.particle_emitter_2_count,
        fidelity.ribbon_emitter_count,
        fidelity.corn_emitter_count,
        fidelity.event_object_count,
        fidelity.light_count,
    );
    println!(
        "  fidelity gate: {} unsupported occurrence(s), {} accepted approximation occurrence(s), {} typed finding class(es)",
        fidelity.unsupported_occurrences,
        fidelity.approximation_occurrences,
        fidelity.findings.len(),
    );
    let unsupported_semantics = fidelity.unsupported_occurrences;

    let pack_manifest = CastleFightPackManifest {
        schema_version: 1,
        castle_fight_catalog_version: CATALOG_VERSION,
        wc3_version: unit_manifest.wc3_version.clone(),
        art_mode: "sd",
        units: PackSectionManifest {
            schema_version: unit_manifest.schema_version,
            entries: unit_manifest.units.len(),
            models: unit_manifest.models.len(),
            failures: unit_manifest.failures.len(),
            substitutions: unit_fallbacks,
            intentionally_hidden: unit_manifest
                .units
                .iter()
                .filter(|unit| unit.intentionally_hidden)
                .count(),
        },
        buildings: PackSectionManifest {
            schema_version: building_manifest.schema_version,
            entries: building_manifest.buildings.len(),
            models: building_manifest.models.len(),
            failures: building_manifest.failures.len(),
            substitutions: building_fallbacks,
            intentionally_hidden: 0,
        },
        doodads: PackSectionManifest {
            schema_version: doodad_manifest.schema_version,
            entries: doodad_manifest.objects.len(),
            models: doodad_manifest.models.len(),
            failures: doodad_manifest.failures.len(),
            substitutions: doodad_fallbacks,
            intentionally_hidden: 0,
        },
        effects: PackSectionManifest {
            schema_version: visual_manifest.schema_version,
            entries: visual_bindings,
            models: visual_manifest.models.len(),
            failures: visual_manifest.failures.len(),
            substitutions: 0,
            intentionally_hidden: 0,
        },
        ui: PackSectionManifest {
            schema_version: ui_manifest.schema_version,
            entries: ui_manifest.assets.len(),
            models: ui_manifest.textures.len(),
            failures: ui_manifest.failures.len(),
            substitutions: 0,
            intentionally_hidden: 0,
        },
        fidelity,
    };
    write_manifest(&output.join("manifest.json"), &pack_manifest)?;

    let reference_failures = unit_manifest.failures.len()
        + unit_fallbacks
        + building_manifest.failures.len()
        + building_fallbacks
        + doodad_manifest.failures.len()
        + doodad_fallbacks
        + visual_manifest.failures.len()
        + ui_manifest.failures.len();
    if reference_failures != 0 || unsupported_semantics != 0 {
        return Err(io::Error::other(format!(
            "complete Castle Fight presentation extraction has {reference_failures} unresolved or substituted asset reference(s) and {unsupported_semantics} unsupported presentation-semantic occurrence(s); inspect the root and sub-pack manifests"
        ))
        .into());
    }

    println!(
        "Complete Castle Fight presentation pack exported to {}",
        output.display()
    );
    Ok(())
}

fn summarize_model_features<'a>(
    models: impl Iterator<Item = &'a ModelManifest>,
) -> ModelFeatureSummary {
    let mut summary = ModelFeatureSummary::default();
    let mut attachment_models = BTreeSet::new();
    let mut findings = BTreeMap::<&'static str, (FidelityStatus, usize, usize)>::new();

    for model in models {
        let features = &model.features;
        summary.model_variants += 1;
        if !model.warnings.is_empty() {
            summary.models_with_warnings += 1;
            summary.warning_count += model.warnings.len();
        }
        if features.multilayer_material_count != 0 {
            summary.multilayer_material_models += 1;
        }
        summary.multilayer_material_count += features.multilayer_material_count;
        summary.animated_material_alpha_layer_count += features.animated_material_alpha_layer_count;
        summary.animated_material_texture_layer_count +=
            features.animated_material_texture_layer_count;
        summary.animated_geoset_alpha_count += features.animated_geoset_alpha_count;
        summary.global_sequence_count += features.global_sequence_count;
        summary.attachment_count += features.attachment_count;
        attachment_models.extend(features.attachment_models.iter().cloned());
        summary.particle_emitter_count += features.particle_emitter_count;
        summary.particle_emitter_animated_track_count +=
            features.particle_emitter_animated_track_count;
        summary.particle_emitter_2_count += features.particle_emitter_2_count;
        summary.particle_emitter_2_animated_track_count +=
            features.particle_emitter_2_animated_track_count;
        summary.ribbon_emitter_count += features.ribbon_emitter_count;
        summary.ribbon_emitter_animated_track_count += features.ribbon_emitter_animated_track_count;
        summary.corn_emitter_count += features.corn_emitter_count;
        summary.corn_emitter_animated_track_count += features.corn_emitter_animated_track_count;
        summary.event_object_count += features.event_object_count;
        let mut event_sound_count = 0;
        let mut event_splat_count = 0;
        let mut event_footprint_count = 0;
        let mut event_spawn_resolved_count = 0;
        let mut event_spawn_unresolved_count = 0;
        let mut event_uber_splat_count = 0;
        let mut event_unknown_count = 0;
        for event in &model.event_objects {
            match event.kind {
                EventObjectKindManifest::Sound => event_sound_count += 1,
                EventObjectKindManifest::Splat => event_splat_count += 1,
                EventObjectKindManifest::Footprint => event_footprint_count += 1,
                EventObjectKindManifest::Spawn if event.lookup_resolved => {
                    event_spawn_resolved_count += 1;
                }
                EventObjectKindManifest::Spawn => event_spawn_unresolved_count += 1,
                EventObjectKindManifest::UberSplat => event_uber_splat_count += 1,
                EventObjectKindManifest::Unknown => event_unknown_count += 1,
            }
        }
        summary.event_sound_count += event_sound_count;
        summary.event_splat_count += event_splat_count;
        summary.event_footprint_count += event_footprint_count;
        summary.event_spawn_count += event_spawn_resolved_count + event_spawn_unresolved_count;
        summary.event_spawn_resolved_count += event_spawn_resolved_count;
        summary.event_uber_splat_count += event_uber_splat_count;
        summary.event_unknown_count += event_unknown_count;
        summary.light_count += features.light_count;
        summary.omni_light_count += features.omni_light_count;
        summary.non_omni_light_count += features.non_omni_light_count;
        summary.non_inheritance_node_count += features.non_inheritance_node_count;
        if features.max_classic_skin_influences > 4 {
            summary.models_with_more_than_four_classic_skin_influences += 1;
        }
        summary.max_classic_skin_influences = summary
            .max_classic_skin_influences
            .max(features.max_classic_skin_influences);

        record_fidelity_finding(
            &mut findings,
            "material.multilayer_flattened",
            FidelityStatus::Approximation,
            features.multilayer_material_count,
        );
        record_fidelity_finding(
            &mut findings,
            "material.animated_alpha_approximate",
            FidelityStatus::Approximation,
            features.animated_material_alpha_layer_count,
        );
        record_fidelity_finding(
            &mut findings,
            "material.animated_texture_approximate",
            FidelityStatus::Approximation,
            features.animated_material_texture_layer_count,
        );
        record_fidelity_finding(
            &mut findings,
            "geoset.alpha_binary",
            FidelityStatus::Approximation,
            features.animated_geoset_alpha_count,
        );
        record_fidelity_finding(
            &mut findings,
            "animation.global_sequence_reset",
            FidelityStatus::Approximation,
            features.global_sequence_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.attachment_child_approximate",
            FidelityStatus::Approximation,
            features.attachment_models.len(),
        );
        record_fidelity_finding(
            &mut findings,
            "particle.pe1_approximate",
            FidelityStatus::Approximation,
            features.particle_emitter_count,
        );
        record_fidelity_finding(
            &mut findings,
            "particle.pe2_approximate",
            FidelityStatus::Approximation,
            features.particle_emitter_2_count,
        );
        record_fidelity_finding(
            &mut findings,
            "particle.pe2_animated_tracks_approximate",
            FidelityStatus::Approximation,
            features.particle_emitter_2_animated_track_count,
        );
        record_fidelity_finding(
            &mut findings,
            "ribbon.approximate",
            FidelityStatus::Approximation,
            features.ribbon_emitter_count,
        );
        record_fidelity_finding(
            &mut findings,
            "ribbon.animated_tracks_approximate",
            FidelityStatus::Approximation,
            features.ribbon_emitter_animated_track_count,
        );
        record_fidelity_finding(
            &mut findings,
            "particle.corn_unsupported",
            FidelityStatus::Unsupported,
            features.corn_emitter_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_spawn_approximate",
            FidelityStatus::Approximation,
            event_spawn_resolved_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_spawn_unresolved",
            FidelityStatus::Unsupported,
            event_spawn_unresolved_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_sound_unsupported",
            FidelityStatus::Unsupported,
            event_sound_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_splat_unsupported",
            FidelityStatus::Unsupported,
            event_splat_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_footprint_unsupported",
            FidelityStatus::Unsupported,
            event_footprint_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_uber_splat_unsupported",
            FidelityStatus::Unsupported,
            event_uber_splat_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.event_unknown_unsupported",
            FidelityStatus::Unsupported,
            event_unknown_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.omni_light_approximate",
            FidelityStatus::Approximation,
            features.omni_light_count,
        );
        record_fidelity_finding(
            &mut findings,
            "model.non_omni_light_unsupported",
            FidelityStatus::Unsupported,
            features.non_omni_light_count,
        );
        record_fidelity_finding(
            &mut findings,
            "hierarchy.non_inheritance_approximate",
            FidelityStatus::Approximation,
            features.non_inheritance_node_count,
        );
        record_fidelity_finding(
            &mut findings,
            "skin.more_than_four_influences",
            FidelityStatus::Approximation,
            usize::from(features.max_classic_skin_influences > 4),
        );

        let omitted_v1800_lights = model
            .warnings
            .iter()
            .filter(|warning| {
                warning.contains("omitted v") && warning.contains("embedded light chunk")
            })
            .count();
        record_fidelity_finding(
            &mut findings,
            "model.v1300_plus_light_chunk_unsupported",
            FidelityStatus::Unsupported,
            omitted_v1800_lights,
        );
        record_fidelity_finding(
            &mut findings,
            "parser.post_v1200_compatibility",
            FidelityStatus::Approximation,
            model
                .warnings
                .iter()
                .filter(|warning| warning.starts_with("Unsupported MDX version:"))
                .count(),
        );
    }

    summary.attachment_models = attachment_models.into_iter().collect();
    summary.findings = findings
        .into_iter()
        .map(
            |(id, (status, affected_models, occurrences))| FidelityFindingSummary {
                id: id.to_owned(),
                status,
                affected_models,
                occurrences,
            },
        )
        .collect();
    for finding in &summary.findings {
        match finding.status {
            FidelityStatus::Approximation => {
                summary.approximation_occurrences += finding.occurrences;
            }
            FidelityStatus::Unsupported => {
                summary.unsupported_occurrences += finding.occurrences;
            }
        }
    }
    summary
}

fn record_fidelity_finding(
    findings: &mut BTreeMap<&'static str, (FidelityStatus, usize, usize)>,
    id: &'static str,
    status: FidelityStatus,
    occurrences: usize,
) {
    if occurrences == 0 {
        return;
    }
    let entry = findings.entry(id).or_insert((status, 0, 0));
    debug_assert_eq!(entry.0, status);
    entry.1 += 1;
    entry.2 += occurrences;
}

fn write_manifest<T: serde::Serialize>(path: &Path, manifest: &T) -> Result<(), Box<dyn Error>> {
    let file = File::create(path)?;
    serde_json::to_writer_pretty(BufWriter::new(file), manifest)?;
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
    castle_fight: bool,
    buildings: bool,
    building_filters: Vec<String>,
    doodads: bool,
    doodad_filters: Vec<String>,
    effects: bool,
    ui: bool,
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
            castle_fight: false,
            buildings: false,
            building_filters: Vec::new(),
            doodads: false,
            doodad_filters: Vec::new(),
            effects: false,
            ui: false,
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
                "--castle-fight" => result.castle_fight = true,
                "--buildings" => result.buildings = true,
                "--doodads" => result.doodads = true,
                "--effects" => result.effects = true,
                "--ui" => result.ui = true,
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
  --castle-fight        Export the complete Castle Fight presentation pack
  --unit RAWCODE        Export one non-building unit; repeat for more units
  --buildings           Export every resolved Castle Fight building model
  --building RAWCODE    Export one building; repeat for more buildings
  --doodads             Export every doodad/destructable placed by Castle Fight
  --effects             Export projectile/spell/buff models referenced by Castle Fight
  --ui                  Export resolved unit/ability/buff/item icons and WC3 resource icons
  --doodad RAWCODE      Export one placed doodad type; repeat for more types
  --art sd              Art mode. SD/classic is currently implemented
  --keep-source         Also retain extracted MDX and source texture files
  --production PATH     Development override for production-buildings.tsv
  --object-fields PATH  Development override for resolved object-fields.tsv
  -h, --help            Show this help

Use --castle-fight for the release-quality full pack under units/, buildings/, doodads/,
effects/, and ui/. With no mode or --unit filters, every resolved non-building Castle Fight
unit object is exported. Use --buildings (or --building RAWCODE) for structures and towers,
--doodads (or --doodad RAWCODE) for map decoration assets and exact placements, and
--effects for the visual-effects catalog, and --ui for UI/icon textures. Models or textures
shared by multiple objects are converted once. Particle/ribbon metadata is retained in the model
manifest for native presentation even though glTF has no particle-emitter primitive. When
--map is supplied, map-imported models/textures override install assets and are extracted too.
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_summary_aggregates_model_features_and_deduplicates_child_models() {
        use crate::export::{EventObjectManifest, ModelFeatureManifest};

        let model =
            |source: &str, features: ModelFeatureManifest, warnings: Vec<&str>| ModelManifest {
                source_model: source.to_owned(),
                source_casc_path: source.to_owned(),
                gltf: format!("{source}.gltf"),
                bin: format!("{source}.bin"),
                geosets: 0,
                bones: 0,
                features,
                overhead_position: None,
                animations: Vec::new(),
                textures: Vec::new(),
                materials: Vec::new(),
                geoset_animations: Vec::new(),
                particle_emitters: Vec::new(),
                model_particle_emitters: Vec::new(),
                ribbon_emitters: Vec::new(),
                attachments: Vec::new(),
                event_objects: Vec::new(),
                lights: Vec::new(),
                warnings: warnings.into_iter().map(str::to_owned).collect(),
            };

        let first = model(
            "first",
            ModelFeatureManifest {
                multilayer_material_count: 2,
                animated_material_texture_layer_count: 2,
                global_sequence_count: 1,
                attachment_count: 1,
                attachment_models: vec![r"SharedModels\Child.mdl".to_owned()],
                particle_emitter_count: 2,
                particle_emitter_2_count: 3,
                particle_emitter_2_animated_track_count: 4,
                light_count: 2,
                omni_light_count: 2,
                non_inheritance_node_count: 2,
                max_classic_skin_influences: 5,
                ..Default::default()
            },
            vec!["warning one"],
        );
        let mut second = model(
            "second",
            ModelFeatureManifest {
                attachment_count: 2,
                attachment_models: vec![
                    r"SharedModels\Child.mdl".to_owned(),
                    r"SharedModels\Other.mdl".to_owned(),
                ],
                ribbon_emitter_count: 1,
                corn_emitter_count: 2,
                event_object_count: 3,
                light_count: 1,
                non_omni_light_count: 1,
                ..Default::default()
            },
            vec!["warning two", "warning three"],
        );
        second.event_objects = vec![
            EventObjectManifest {
                object_id: 0,
                name: "SPNxUDIS".to_owned(),
                position: [0.0; 3],
                kind: EventObjectKindManifest::Spawn,
                event_code: Some("UDIS".to_owned()),
                lookup_resolved: true,
                spawn_model: Some("spawn.mdx".to_owned()),
                gltf: Some("models/spawn.gltf".to_owned()),
                global_sequence_id: None,
                event_track_times: vec![100],
                sequence_windows: Vec::new(),
                global_sequence_durations_ms: Vec::new(),
            },
            EventObjectManifest {
                object_id: 1,
                name: "SNDxTEST".to_owned(),
                position: [0.0; 3],
                kind: EventObjectKindManifest::Sound,
                event_code: Some("TEST".to_owned()),
                lookup_resolved: false,
                spawn_model: None,
                gltf: None,
                global_sequence_id: None,
                event_track_times: vec![200],
                sequence_windows: Vec::new(),
                global_sequence_durations_ms: Vec::new(),
            },
            EventObjectManifest {
                object_id: 2,
                name: "Point01".to_owned(),
                position: [0.0; 3],
                kind: EventObjectKindManifest::Unknown,
                event_code: None,
                lookup_resolved: false,
                spawn_model: None,
                gltf: None,
                global_sequence_id: None,
                event_track_times: vec![300],
                sequence_windows: Vec::new(),
                global_sequence_durations_ms: Vec::new(),
            },
        ];

        let summary = summarize_model_features([&first, &second].into_iter());
        assert_eq!(summary.model_variants, 2);
        assert_eq!(summary.models_with_warnings, 2);
        assert_eq!(summary.warning_count, 3);
        assert_eq!(summary.multilayer_material_models, 1);
        assert_eq!(summary.multilayer_material_count, 2);
        assert_eq!(summary.global_sequence_count, 1);
        assert_eq!(summary.attachment_count, 3);
        assert_eq!(
            summary.attachment_models,
            [
                r"SharedModels\Child.mdl".to_owned(),
                r"SharedModels\Other.mdl".to_owned()
            ]
        );
        assert_eq!(summary.particle_emitter_count, 2);
        assert_eq!(summary.particle_emitter_2_count, 3);
        assert_eq!(summary.particle_emitter_2_animated_track_count, 4);
        assert_eq!(summary.ribbon_emitter_count, 1);
        assert_eq!(summary.corn_emitter_count, 2);
        assert_eq!(summary.event_object_count, 3);
        assert_eq!(summary.event_spawn_count, 1);
        assert_eq!(summary.event_spawn_resolved_count, 1);
        assert_eq!(summary.event_sound_count, 1);
        assert_eq!(summary.event_unknown_count, 1);
        assert_eq!(summary.light_count, 3);
        assert_eq!(summary.omni_light_count, 2);
        assert_eq!(summary.non_omni_light_count, 1);
        assert_eq!(
            summary.models_with_more_than_four_classic_skin_influences,
            1
        );
        assert_eq!(summary.max_classic_skin_influences, 5);
        assert_eq!(summary.approximation_occurrences, 24);
        assert_eq!(summary.unsupported_occurrences, 5);
        assert_eq!(summary.findings.len(), 16);
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "material.animated_texture_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 2
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "hierarchy.non_inheritance_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 2
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.attachment_child_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 2
                && finding.occurrences == 3
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.event_spawn_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 1
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.event_sound_unsupported"
                && finding.status == FidelityStatus::Unsupported
                && finding.affected_models == 1
                && finding.occurrences == 1
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.event_unknown_unsupported"
                && finding.status == FidelityStatus::Unsupported
                && finding.affected_models == 1
                && finding.occurrences == 1
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.omni_light_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 2
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "model.non_omni_light_unsupported"
                && finding.status == FidelityStatus::Unsupported
                && finding.affected_models == 1
                && finding.occurrences == 1
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "particle.pe1_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 2
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "particle.pe2_animated_tracks_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.affected_models == 1
                && finding.occurrences == 4
        }));
        assert!(summary.findings.iter().any(|finding| {
            finding.id == "particle.pe2_approximate"
                && finding.status == FidelityStatus::Approximation
                && finding.occurrences == 3
        }));
    }

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
    fn parses_full_castle_fight_export_mode() {
        let args = Args::parse(
            ["--wc3", "/game", "--output", "/out", "--castle-fight"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert!(args.castle_fight);
        assert!(!args.effects);
        assert!(!args.buildings);
        assert!(args.units.is_empty());
    }

    #[test]
    fn parses_ui_export_mode() {
        let args = Args::parse(
            ["--wc3", "/game", "--output", "/out", "--ui"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert!(args.ui);
        assert!(!args.effects);
        assert!(args.units.is_empty());
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
