//! Opt-in, presentation-only experiments. Some deliberately trade visual correctness
//! for attribution; all remain separate from ordinary gameplay configuration.

use std::{
    collections::HashSet,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bevy::{
    camera::{primitives::Aabb, visibility::DynamicSkinnedMeshBounds},
    core_pipeline::core_3d::Transparent3d,
    diagnostic::DiagnosticsStore,
    ecs::system::SystemParam,
    mesh::skinning::SkinnedMesh,
    pbr::SkinUniforms,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, batching::NoAutomaticBatching,
        pipelined_rendering::RenderExtractApp, render_phase::ViewSortedRenderPhases,
    },
};

use crate::wc3_effects::{
    Wc3AnimatedAlphaMaterial, Wc3AnimatedMaterialTexture, Wc3BillboardParticles, Wc3Particle,
    Wc3SplatMaterial, Wc3TeamColorMaterial,
};

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RenderExperiment {
    #[default]
    Baseline,
    FreezeBounds,
    HideSkinned,
    HideParticles,
    HideTransparent,
    LegacyTeamColor,
    LegacyGeosetVisibility,
    LegacyAttachmentSearch,
    LegacyAttachmentIndex,
    LegacyEffectPooling,
    LegacySplatUpdates,
    LegacySplatMaterialState,
    LegacyAnimatedAlphaState,
    LegacyAnimatedTextureState,
    FreezeMaterials,
    FreezePoses,
    BindlessAuto,
    Bindless64,
    Bindless128,
    Bindless256,
    NoBindless,
    ParticleSharedView,
    ParticleCull,
    ParticlePartialBindings,
}

impl RenderExperiment {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "baseline" => Ok(Self::Baseline),
            "freeze-bounds" => Ok(Self::FreezeBounds),
            "hide-skinned" => Ok(Self::HideSkinned),
            "hide-particles" => Ok(Self::HideParticles),
            "hide-transparent" => Ok(Self::HideTransparent),
            "legacy-team-color" => Ok(Self::LegacyTeamColor),
            "legacy-geoset-visibility" => Ok(Self::LegacyGeosetVisibility),
            "legacy-attachment-search" => Ok(Self::LegacyAttachmentSearch),
            "legacy-attachment-index" => Ok(Self::LegacyAttachmentIndex),
            "legacy-effect-pooling" => Ok(Self::LegacyEffectPooling),
            "legacy-splat-updates" => Ok(Self::LegacySplatUpdates),
            "legacy-splat-material-state" => Ok(Self::LegacySplatMaterialState),
            "legacy-animated-alpha-state" => Ok(Self::LegacyAnimatedAlphaState),
            "legacy-animated-texture-state" => Ok(Self::LegacyAnimatedTextureState),
            "freeze-materials" => Ok(Self::FreezeMaterials),
            "freeze-poses" => Ok(Self::FreezePoses),
            "bindless-auto" => Ok(Self::BindlessAuto),
            "bindless-64" => Ok(Self::Bindless64),
            "bindless-128" => Ok(Self::Bindless128),
            "bindless-256" => Ok(Self::Bindless256),
            "no-bindless" => Ok(Self::NoBindless),
            "particle-shared-view" => Ok(Self::ParticleSharedView),
            "particle-cull" => Ok(Self::ParticleCull),
            "particle-partial-bindings" => Ok(Self::ParticlePartialBindings),
            _ => Err(format!(
                "unknown render experiment {value:?}; expected baseline, freeze-bounds, hide-skinned, hide-particles, hide-transparent, legacy-team-color, legacy-geoset-visibility, legacy-attachment-search, legacy-attachment-index, legacy-effect-pooling, legacy-splat-updates, legacy-splat-material-state, legacy-animated-alpha-state, legacy-animated-texture-state, freeze-materials, freeze-poses, bindless-auto, bindless-64, bindless-128, bindless-256, no-bindless, particle-shared-view, particle-cull, or particle-partial-bindings"
            )),
        }
    }

    pub(crate) fn standard_material_bindless_slots(self) -> Option<u32> {
        match self {
            Self::BindlessAuto => None,
            Self::Bindless128 => Some(128),
            Self::Bindless256 => Some(256),
            _ => Some(crate::render_tuning::DEFAULT_STANDARD_MATERIAL_BINDLESS_SLOTS),
        }
    }
}

const STAGES: [(&str, Option<RenderSystems>); 14] = [
    ("handoff incl wait/extract", None),
    ("world sync/extract", None),
    ("extract commands", Some(RenderSystems::ExtractCommands)),
    ("prepare assets", Some(RenderSystems::PrepareAssets)),
    ("prepare meshes", Some(RenderSystems::PrepareMeshes)),
    ("create views", Some(RenderSystems::CreateViews)),
    ("specialize", Some(RenderSystems::Specialize)),
    ("prepare views", Some(RenderSystems::PrepareViews)),
    ("queue", Some(RenderSystems::Queue)),
    ("phase sort", Some(RenderSystems::PhaseSort)),
    ("prepare resources", Some(RenderSystems::Prepare)),
    ("render/submit/present", Some(RenderSystems::Render)),
    ("cleanup", Some(RenderSystems::Cleanup)),
    ("post cleanup", Some(RenderSystems::PostCleanup)),
];

#[derive(Clone, Copy, Default)]
struct StageTiming {
    calls: u64,
    total: Duration,
    max: Duration,
}

impl StageTiming {
    fn record(&mut self, duration: Duration) {
        self.calls += 1;
        self.total += duration;
        self.max = self.max.max(duration);
    }
}

#[derive(Default)]
struct Measurements {
    started: Option<Instant>,
    stages: [StageTiming; STAGES.len()],
    transparent: Option<(usize, usize, usize)>,
    particles: Option<crate::particle_renderer::Wc3ParticleQueueStats>,
}

#[derive(Resource, Clone, Default)]
pub(crate) struct RenderAudit(Arc<Mutex<Measurements>>);

impl RenderAudit {
    pub(crate) fn start_capture(&self) {
        *self.0.lock().expect("render audit mutex poisoned") = Measurements {
            started: Some(Instant::now()),
            ..default()
        };
    }

    fn record(&self, stage: usize, started: Instant) {
        let finished = Instant::now();
        let mut measurements = self.0.lock().expect("render audit mutex poisoned");
        // Do not import a warm-up frame straddling the capture boundary.
        if measurements
            .started
            .is_some_and(|capture| started >= capture)
        {
            measurements.stages[stage].record(finished.duration_since(started));
        }
    }

    pub(crate) fn format(&self) -> String {
        let measurements = self.0.lock().expect("render audit mutex poisoned");
        let mut output =
            String::from("\nRENDER CPU  avg/call, max, calls (overlapping scopes; do not sum)\n");
        for ((name, _), timing) in STAGES.iter().zip(&measurements.stages) {
            if timing.calls > 0 {
                writeln!(
                    output,
                    "  {name:<25} {:>8.3}ms {:>8.3}ms {}",
                    timing.total.as_secs_f64() * 1000.0 / timing.calls as f64,
                    timing.max.as_secs_f64() * 1000.0,
                    timing.calls
                )
                .unwrap();
            }
        }
        if let Some((items, draws, palette_bytes)) = measurements.transparent {
            writeln!(output, "  transparent phase: {items} items, {draws} draw-function calls (latest sampled frame, all views)").unwrap();
            writeln!(
                output,
                "  skin palette staging: {palette_bytes} bytes uploaded/frame (latest sample)"
            )
            .unwrap();
        }
        if let Some(particles) = measurements.particles {
            writeln!(output,
                "  particle queue: {} candidates, {} queued, {} zero-alpha rejects, {} frustum rejects; texture slots {}/{} populated/bound (latest sample, all views)",
                particles.candidates, particles.queued, particles.zero_alpha,
                particles.outside, particles.populated_slots, particles.bound_slots,
            ).unwrap();
        }
        output
    }
}

pub(crate) struct RenderAuditPlugin;

impl Plugin for RenderAuditPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderAudit>().add_systems(
            PostUpdate,
            apply_render_experiment
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
        app.add_systems(
            PostUpdate,
            freeze_animation_poses
                .after(crate::wc3_effects::skip_unchanged_paused_animation_poses)
                .before(bevy::animation::animate_targets),
        );
    }

    fn finish(&self, app: &mut App) {
        let audit = app.world().resource::<RenderAudit>().clone();
        if let Some(extract_app) = app.get_sub_app_mut(RenderExtractApp)
            && let Some(mut extract) = extract_app.take_extract()
        {
            let audit = audit.clone();
            extract_app.set_extract(move |main, render| {
                let started = Instant::now();
                extract(main, render);
                audit.record(0, started);
            });
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        if let Some(device) = render_app
            .world()
            .get_resource::<bevy::render::renderer::RenderDevice>()
        {
            println!(
                "client-profile standard_material_bindless={}",
                bevy::pbr::material_uses_bindless_resources::<StandardMaterial>(device)
            );
        }
        render_app.insert_resource(audit.clone()).add_systems(
            Render,
            sample_transparent_batches
                .after(RenderSystems::Prepare)
                .before(RenderSystems::Render),
        );
        if let Some(mut extract) = render_app.take_extract() {
            let audit = audit.clone();
            render_app.set_extract(move |main, render| {
                let started = Instant::now();
                extract(main, render);
                audit.record(1, started);
            });
        }
        // Bracket the engine's existing ordered sets without serializing systems
        // inside them. The handoff includes extraction and may wait for rendering,
        // so the report's scopes must not be added together.
        for (index, (_, phase)) in STAGES.iter().enumerate() {
            let Some(phase) = phase else { continue };
            let started = Arc::new(Mutex::new(None::<Instant>));
            let start_slot = started.clone();
            let audit = audit.clone();
            let mut begin =
                (move || *start_slot.lock().unwrap() = Some(Instant::now())).before(phase.clone());
            let mut end = (move || {
                if let Some(started) = started.lock().unwrap().take() {
                    audit.record(index, started);
                }
            })
            .after(phase.clone());
            if let Some(previous) = STAGES[index - 1].1.as_ref() {
                begin = begin.after(previous.clone());
            }
            if let Some((_, Some(next))) = STAGES.get(index + 1) {
                end = end.before(next.clone());
            }
            render_app.add_systems(Render, (begin, end));
        }
    }
}

// Attribution only: clocks keep advancing, but no joint curves are applied.
// Root movement and the authoritative simulation continue normally.
fn freeze_animation_poses(
    experiment: Res<RenderExperiment>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if *experiment != RenderExperiment::FreezePoses {
        return;
    }
    for mut player in &mut players {
        for (_, animation) in player.playing_animations_mut() {
            animation.set_weight(0.0);
        }
    }
}

fn sample_transparent_batches(
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    skins: Res<SkinUniforms>,
    particles: Res<crate::particle_renderer::Wc3ParticleQueueStats>,
    audit: Res<RenderAudit>,
    mut last_sample: Local<Option<Instant>>,
) {
    let now = Instant::now();
    if last_sample.is_some_and(|last| now.duration_since(last) < Duration::from_secs(1)) {
        return;
    }
    *last_sample = Some(now);
    let mut items = 0;
    let mut draws = 0;
    for phase in phases.values() {
        items += phase.items.len();
        // Follow SortedRenderPhase::render_range: a batch's representative
        // advances over the instances it draws; empty ranges don't issue a call.
        let mut index = 0;
        while index < phase.items.len() {
            let count = phase.items[index].batch_range.len();
            draws += usize::from(count > 0);
            index += count.max(1);
        }
    }
    let mut measurements = audit.0.lock().expect("render audit mutex poisoned");
    measurements.transparent = Some((
        items,
        draws,
        skins.current_staging_buffer.len() * size_of::<Mat4>(),
    ));
    measurements.particles = Some(*particles);
}

type ExperimentMeshes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Visibility,
        Option<&'static MeshMaterial3d<StandardMaterial>>,
        Option<&'static MeshMaterial3d<Wc3AnimatedAlphaMaterial>>,
        Option<&'static MeshMaterial3d<Wc3SplatMaterial>>,
        Has<SkinnedMesh>,
        Has<Wc3Particle>,
        Has<DynamicSkinnedMeshBounds>,
        Has<Aabb>,
    ),
    With<Mesh3d>,
>;

fn apply_render_experiment(
    mut commands: Commands,
    experiment: Res<RenderExperiment>,
    materials: Res<Assets<StandardMaterial>>,
    animated_alpha_materials: Option<Res<Assets<Wc3AnimatedAlphaMaterial>>>,
    mut meshes: ExperimentMeshes,
) {
    if matches!(
        *experiment,
        RenderExperiment::Baseline
            | RenderExperiment::LegacyTeamColor
            | RenderExperiment::LegacyGeosetVisibility
            | RenderExperiment::LegacyAttachmentSearch
            | RenderExperiment::LegacyAttachmentIndex
            | RenderExperiment::LegacyEffectPooling
            | RenderExperiment::LegacySplatUpdates
            | RenderExperiment::LegacySplatMaterialState
            | RenderExperiment::LegacyAnimatedAlphaState
            | RenderExperiment::LegacyAnimatedTextureState
            | RenderExperiment::FreezeMaterials
            | RenderExperiment::FreezePoses
            | RenderExperiment::BindlessAuto
            | RenderExperiment::Bindless64
            | RenderExperiment::Bindless128
            | RenderExperiment::Bindless256
            | RenderExperiment::NoBindless
    ) {
        return;
    }
    for (
        entity,
        mut visibility,
        material,
        animated_alpha_material,
        splat_material,
        skin,
        particle,
        dynamic,
        has_bounds,
    ) in &mut meshes
    {
        let hide = match *experiment {
            RenderExperiment::Baseline
            | RenderExperiment::LegacyTeamColor
            | RenderExperiment::LegacyGeosetVisibility
            | RenderExperiment::LegacyAttachmentSearch
            | RenderExperiment::LegacyAttachmentIndex
            | RenderExperiment::LegacyEffectPooling
            | RenderExperiment::LegacySplatUpdates
            | RenderExperiment::LegacySplatMaterialState
            | RenderExperiment::LegacyAnimatedAlphaState
            | RenderExperiment::LegacyAnimatedTextureState
            | RenderExperiment::FreezeMaterials
            | RenderExperiment::FreezePoses
            | RenderExperiment::BindlessAuto
            | RenderExperiment::Bindless64
            | RenderExperiment::Bindless128
            | RenderExperiment::Bindless256
            | RenderExperiment::NoBindless
            | RenderExperiment::ParticleSharedView
            | RenderExperiment::ParticleCull
            | RenderExperiment::ParticlePartialBindings => false,
            RenderExperiment::FreezeBounds => {
                if dynamic && has_bounds {
                    commands.entity(entity).remove::<DynamicSkinnedMeshBounds>();
                }
                false
            }
            RenderExperiment::HideSkinned => skin,
            RenderExperiment::HideParticles => particle,
            RenderExperiment::HideTransparent => {
                splat_material.is_some()
                    || animated_alpha_material
                        .and_then(|handle| {
                            animated_alpha_materials
                                .as_ref()
                                .and_then(|materials| materials.get(&handle.0))
                        })
                        .is_some_and(|material| is_transparent(material.base.alpha_mode))
                    || material
                        .and_then(|handle| materials.get(&handle.0))
                        .is_some_and(|material| is_transparent(material.alpha_mode))
            }
        };
        if hide {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

fn is_transparent(alpha: AlphaMode) -> bool {
    matches!(
        alpha,
        AlphaMode::Blend | AlphaMode::Premultiplied | AlphaMode::Add | AlphaMode::Multiply
    )
}

type CensusMeshes<'w, 's> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        &'static ViewVisibility,
        Option<&'static MeshMaterial3d<StandardMaterial>>,
        Option<&'static MeshMaterial3d<Wc3AnimatedAlphaMaterial>>,
        Option<&'static MeshMaterial3d<Wc3SplatMaterial>>,
        Option<&'static MeshMaterial3d<Wc3TeamColorMaterial>>,
        Option<&'static SkinnedMesh>,
        Has<DynamicSkinnedMeshBounds>,
        Has<Wc3Particle>,
        Has<NoAutomaticBatching>,
        &'static GlobalTransform,
    ),
>;

#[derive(SystemParam)]
pub(crate) struct SceneCensus<'w, 's> {
    meshes: CensusMeshes<'w, 's>,
    materials: Res<'w, Assets<StandardMaterial>>,
    animated_alpha_materials: Res<'w, Assets<Wc3AnimatedAlphaMaterial>>,
    billboard_particles: Res<'w, Wc3BillboardParticles>,
    players: Query<'w, 's, (), With<AnimationPlayer>>,
    animated_textures: Query<'w, 's, (), With<Wc3AnimatedMaterialTexture>>,
    entities: Query<'w, 's, Entity>,
    windows: Query<'w, 's, &'static Window>,
    cameras: Query<'w, 's, &'static GlobalTransform, With<Camera3d>>,
}

impl SceneCensus<'_, '_> {
    pub(crate) fn format(&self) -> String {
        let mut total = 0;
        let mut visible = 0;
        let mut skins = 0;
        let mut dynamic = 0;
        let mut particles = self.billboard_particles.len();
        let mut transparent = self.billboard_particles.len();
        let mut no_batch = 0;
        let mut joint_references = 0;
        let mut collapsed_visible = 0;
        let mut joints = HashSet::new();
        let mut mesh_assets = HashSet::new();
        let mut material_assets = HashSet::new();
        let mut visible_pairs = HashSet::new();
        let mut animated_alpha_material_assets = HashSet::new();
        let mut animated_alpha_visible_pairs = HashSet::new();
        let mut team_material_assets = HashSet::new();
        let mut team_visible_pairs = HashSet::new();
        for (
            mesh,
            visibility,
            material,
            animated_alpha_material,
            splat_material,
            team_material,
            skin,
            bounds,
            particle,
            unbatched,
            transform,
        ) in &self.meshes
        {
            total += 1;
            visible += usize::from(visibility.get());
            dynamic += usize::from(bounds);
            particles += usize::from(particle);
            no_batch += usize::from(unbatched);
            mesh_assets.insert(mesh.id());
            collapsed_visible +=
                usize::from(visibility.get() && transform.affine().matrix3.determinant() == 0.0);
            if let Some(material) = material {
                material_assets.insert(material.id());
                if visibility.get() {
                    visible_pairs.insert((mesh.id(), material.id()));
                    transparent += usize::from(
                        self.materials
                            .get(material.id())
                            .is_some_and(|material| is_transparent(material.alpha_mode)),
                    );
                }
            } else if let Some(material) = animated_alpha_material {
                animated_alpha_material_assets.insert(material.id());
                if visibility.get() {
                    animated_alpha_visible_pairs.insert((mesh.id(), material.id()));
                    transparent += usize::from(
                        self.animated_alpha_materials
                            .get(material.id())
                            .is_some_and(|material| is_transparent(material.base.alpha_mode)),
                    );
                }
            } else if visibility.get() && splat_material.is_some() {
                transparent += 1;
            }
            if let Some(material) = team_material {
                team_material_assets.insert(material.id());
                if visibility.get() {
                    team_visible_pairs.insert((mesh.id(), material.id()));
                }
            }
            if let Some(skin) = skin {
                skins += 1;
                joint_references += skin.joints.len();
                joints.extend(skin.joints.iter().copied());
            }
        }
        let mut output = format!(
            "\nSCENE  entities={} meshes={} visible={} skinned={} dynamic_bounds={} particles={} transparent_visible={} no_auto_batch={}\n  mesh_assets={} material_assets={} visible_mesh_material_pairs={} animation_players={} animated_texture_tracks={} joint_references={} unique_joints={}\n",
            self.entities.iter().count(),
            total,
            visible,
            skins,
            dynamic,
            particles,
            transparent,
            no_batch,
            mesh_assets.len(),
            material_assets.len()
                + animated_alpha_material_assets.len()
                + team_material_assets.len(),
            visible_pairs.len() + animated_alpha_visible_pairs.len() + team_visible_pairs.len(),
            self.players.iter().count(),
            self.animated_textures.iter().count(),
            joint_references,
            joints.len()
        );
        for window in &self.windows {
            writeln!(
                output,
                "  window={}x{} present={:?} visible={}",
                window.physical_width(),
                window.physical_height(),
                window.present_mode,
                window.visible
            )
            .unwrap();
        }
        for camera in &self.cameras {
            writeln!(
                output,
                "  camera_position={:?} camera_forward={:?}",
                camera.translation(),
                camera.forward()
            )
            .unwrap();
        }
        writeln!(output, "  collapsed_visible={collapsed_visible} (zero determinant; candidates for geoset visibility)").unwrap();
        output
    }
}

pub(crate) fn format_render_passes(diagnostics: &DiagnosticsStore) -> String {
    let mut rows = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let path = diagnostic.path().as_str();
            (path.starts_with("render/")
                && (path.ends_with("/elapsed_cpu") || path.ends_with("/elapsed_gpu")))
            .then(|| diagnostic.average().map(|ms| (path, ms)))
            .flatten()
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(name, _)| *name);
    let mut output =
        String::from("\nRENDER PASSES  recent diagnostic average (not whole capture)\n");
    for (name, ms) in rows {
        writeln!(output, "  {name} {ms:.3}ms").unwrap();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_experiments_reject_misspelled_names() {
        assert_eq!(
            RenderExperiment::parse("freeze-bounds"),
            Ok(RenderExperiment::FreezeBounds)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-team-color"),
            Ok(RenderExperiment::LegacyTeamColor)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-geoset-visibility"),
            Ok(RenderExperiment::LegacyGeosetVisibility)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-splat-material-state"),
            Ok(RenderExperiment::LegacySplatMaterialState)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-attachment-index"),
            Ok(RenderExperiment::LegacyAttachmentIndex)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-effect-pooling"),
            Ok(RenderExperiment::LegacyEffectPooling)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-animated-alpha-state"),
            Ok(RenderExperiment::LegacyAnimatedAlphaState)
        );
        assert_eq!(
            RenderExperiment::parse("legacy-animated-texture-state"),
            Ok(RenderExperiment::LegacyAnimatedTextureState)
        );
        assert_eq!(
            RenderExperiment::parse("bindless-auto"),
            Ok(RenderExperiment::BindlessAuto)
        );
        assert_eq!(
            RenderExperiment::parse("bindless-64"),
            Ok(RenderExperiment::Bindless64)
        );
        assert_eq!(
            RenderExperiment::parse("bindless-128"),
            Ok(RenderExperiment::Bindless128)
        );
        assert_eq!(
            RenderExperiment::parse("bindless-256"),
            Ok(RenderExperiment::Bindless256)
        );
        assert_eq!(
            RenderExperiment::Baseline.standard_material_bindless_slots(),
            Some(64)
        );
        assert_eq!(
            RenderExperiment::BindlessAuto.standard_material_bindless_slots(),
            None
        );
        assert_eq!(
            RenderExperiment::Bindless128.standard_material_bindless_slots(),
            Some(128)
        );
        assert!(RenderExperiment::parse("hide-everything").is_err());
    }

    #[test]
    fn masking_is_not_classified_as_blended_transparency() {
        assert!(!is_transparent(AlphaMode::Opaque));
        assert!(!is_transparent(AlphaMode::Mask(0.5)));
        assert!(!is_transparent(AlphaMode::AlphaToCoverage));
        assert!(is_transparent(AlphaMode::Blend));
        assert!(is_transparent(AlphaMode::Add));
    }

    #[test]
    fn capture_drops_warmup_and_resets_totals() {
        let audit = RenderAudit::default();
        let warmup = Instant::now();
        audit.start_capture();
        audit.record(1, warmup);
        assert_eq!(audit.0.lock().unwrap().stages[1].calls, 0);
        audit.record(1, Instant::now());
        assert_eq!(audit.0.lock().unwrap().stages[1].calls, 1);
        audit.start_capture();
        assert_eq!(audit.0.lock().unwrap().stages[1].calls, 0);
    }

    #[test]
    fn hiding_skin_preserves_static_geometry_and_baseline_visibility() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .insert_resource(RenderExperiment::Baseline)
            .add_systems(Update, apply_render_experiment);
        let skin = app
            .world_mut()
            .spawn((
                Mesh3d::default(),
                Visibility::Inherited,
                SkinnedMesh {
                    inverse_bindposes: Handle::default(),
                    joints: Vec::new(),
                },
            ))
            .id();
        let terrain = app
            .world_mut()
            .spawn((Mesh3d::default(), Visibility::Inherited))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(skin),
            Some(&Visibility::Inherited)
        );
        app.insert_resource(RenderExperiment::HideSkinned);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(skin),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(terrain),
            Some(&Visibility::Inherited)
        );
        assert!(app.world().get::<SkinnedMesh>(skin).is_some());
    }

    #[test]
    fn bounds_experiment_waits_for_bounds_and_keeps_culling_enabled() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .insert_resource(RenderExperiment::FreezeBounds)
            .add_systems(Update, apply_render_experiment);
        let mesh = app
            .world_mut()
            .spawn((
                Mesh3d::default(),
                Visibility::Inherited,
                DynamicSkinnedMeshBounds,
            ))
            .id();
        app.update();
        assert!(app.world().get::<DynamicSkinnedMeshBounds>(mesh).is_some());
        app.world_mut().entity_mut(mesh).insert(Aabb::default());
        app.update();
        assert!(app.world().get::<DynamicSkinnedMeshBounds>(mesh).is_none());
        assert!(app.world().get::<Aabb>(mesh).is_some());
        assert!(
            app.world()
                .get::<bevy::camera::visibility::NoFrustumCulling>(mesh)
                .is_none()
        );
    }
}
