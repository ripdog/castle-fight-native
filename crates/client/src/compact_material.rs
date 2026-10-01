//! Profiling-only compact bindings for base-colour-only WC3 material extensions.
//!
//! Main-world material handles stay authoritative for presentation setup and
//! pooling. Render extraction substitutes a prepared proxy, retaining Bevy's
//! material uniform, specialization, mesh/skin ownership and WC3 extension data.

use std::{any::TypeId, collections::HashMap};

use bevy::{
    asset::{AssetEvent, AssetEventSystems, AssetId, uuid_handle},
    ecs::system::SystemParamItem,
    material::OpaqueRendererMethod,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{
        ExtendedMaterial, Material, MaterialExtractionSystems, MaterialPipeline,
        MaterialPipelineKey, MaterialPlugin, MeshExtractionSystems, MeshesToReextractNextFrame,
        PreparedMaterial, RenderMaterialInstances, StandardMaterialKey,
    },
    prelude::*,
    reflect::ReflectRef,
    render::{
        Extract, ExtractSchedule, RenderApp,
        camera::{DirtySpecializationSystems, DirtySpecializations},
        erased_render_asset::ErasedRenderAssets,
        render_resource::{
            AsBindGroup, AsBindGroupError, BindGroupLayout, BindGroupLayoutEntry,
            RenderPipelineDescriptor, SpecializedMeshPipelineError, UnpreparedBindGroup,
        },
        renderer::RenderDevice,
        sync_world::MainEntity,
    },
    shader::{ShaderDefVal, ShaderImport, ShaderRef, Source},
};

use crate::{
    render_audit::RenderExperiment,
    wc3_effects::{
        Wc3AnimatedAlphaExtension, Wc3AnimatedAlphaMaterial, Wc3TeamColorExtension,
        Wc3TeamColorMaterial,
    },
};

const COMPACT_FRAGMENT: Handle<Shader> = uuid_handle!("237198f2-cf10-4ba1-9020-47a858b94901");
const COMPACT_ALPHA: Handle<Shader> = uuid_handle!("237198f2-cf10-4ba1-9020-47a858b94902");
const COMPACT_TEAM: Handle<Shader> = uuid_handle!("237198f2-cf10-4ba1-9020-47a858b94903");
const COMPACT_IMPORT: &str = "castle_fight::compact_pbr_fragment";

pub(crate) struct CompactMaterialPlugin;

impl Plugin for CompactMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CompactShaderState>()
            .init_resource::<CompactMaterialCache<Wc3AnimatedAlphaMaterial>>()
            .init_resource::<CompactMaterialCache<Wc3TeamColorMaterial>>()
            .add_plugins((
                MaterialPlugin::<CompactAlphaMaterial>::default(),
                MaterialPlugin::<CompactTeamMaterial>::default(),
            ))
            .add_systems(
                PostUpdate,
                (
                    prepare_compact_shader,
                    sync_compact_materials::<Wc3AnimatedAlphaMaterial>,
                    sync_compact_materials::<Wc3TeamColorMaterial>,
                )
                    .chain()
                    .after(AssetEventSystems),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CompactMaterialStats>()
            .add_systems(
                ExtractSchedule,
                reset_compact_stats.in_set(MaterialExtractionSystems),
            )
            .add_systems(
                ExtractSchedule,
                (
                    extract_compact_materials::<Wc3AnimatedAlphaMaterial>,
                    extract_compact_materials::<Wc3TeamColorMaterial>,
                )
                    .after(MaterialExtractionSystems)
                    .before(MeshExtractionSystems)
                    .in_set(DirtySpecializationSystems::CheckForChanges),
            );
    }
}

#[derive(Resource, Default)]
enum CompactShaderState {
    #[default]
    Loading,
    Ready,
    Unsupported,
}

/// Specialize only texture branches known to be false for eligible materials.
/// All remaining PBR calculations come from the loaded engine shader verbatim.
/// Refuse unfamiliar source structure instead of producing a partial shader.
fn compact_fragment_source(source: &str) -> Option<String> {
    let header = "#define_import_path bevy_pbr::pbr_fragment";
    if source.matches(header).count() != 1 {
        return None;
    }
    let mut source = source.replace(header, &format!("#define_import_path {COMPACT_IMPORT}"));
    for flag in [
        "DEPTH_MAP",
        "EMISSIVE_TEXTURE",
        "METALLIC_ROUGHNESS_TEXTURE",
        "OCCLUSION_TEXTURE",
        "SPECULAR_TEXTURE",
        "SPECULAR_TINT_TEXTURE",
        "CLEARCOAT_TEXTURE",
        "CLEARCOAT_ROUGHNESS_TEXTURE",
        "SPECULAR_TRANSMISSION_TEXTURE",
        "THICKNESS_TEXTURE",
        "DIFFUSE_TRANSMISSION_TEXTURE",
        "ANISOTROPY_TEXTURE",
    ] {
        let marker =
            format!("if ((flags & pbr_types::STANDARD_MATERIAL_FLAGS_{flag}_BIT) != 0u) {{");
        let start = source.find(&marker)?;
        let open = start + marker.len() - 1;
        let mut depth = 0;
        let end = source[open..]
            .char_indices()
            .find_map(|(offset, character)| {
                match character {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(open + offset + 1)
            })?;
        source.insert_str(end, "\n#endif // WC3_COMPACT_BASE_COLOR\n");
        source.insert_str(start, "\n#ifndef WC3_COMPACT_BASE_COLOR\n");
    }
    Some(source)
}

fn prepare_compact_shader(
    experiment: Res<RenderExperiment>,
    mut state: ResMut<CompactShaderState>,
    mut shaders: ResMut<Assets<Shader>>,
) {
    if *experiment != RenderExperiment::CompactMaterial
        || !matches!(*state, CompactShaderState::Loading)
    {
        return;
    }
    let Some((_, original)) = shaders.iter().find(|(_, shader)| {
        shader.import_path == ShaderImport::Custom("bevy_pbr::pbr_fragment".into())
    }) else {
        return;
    };
    let Source::Wgsl(source) = &original.source else {
        *state = CompactShaderState::Unsupported;
        return;
    };
    let Some(source) = compact_fragment_source(source) else {
        warn!("compact material: unfamiliar Bevy PBR source; retaining original materials");
        *state = CompactShaderState::Unsupported;
        return;
    };
    shaders
        .insert(
            COMPACT_FRAGMENT.id(),
            Shader::from_wgsl(source, "generated/compact_pbr_fragment.wgsl"),
        )
        .unwrap();
    for (handle, source, path) in [
        (
            COMPACT_ALPHA,
            include_str!("../../../assets/shaders/wc3_animated_alpha.wgsl"),
            "generated/compact_alpha.wgsl",
        ),
        (
            COMPACT_TEAM,
            include_str!("../../../assets/shaders/wc3_team_color.wgsl"),
            "generated/compact_team.wgsl",
        ),
    ] {
        shaders
            .insert(
                handle.id(),
                Shader::from_wgsl(
                    format!(
                        "#import {COMPACT_IMPORT}::pbr_input_from_standard_material\n{}",
                        source.replace("pbr_fragment::pbr_input_from_standard_material,", ""),
                    ),
                    path,
                ),
            )
            .unwrap();
    }
    *state = CompactShaderState::Ready;
}

#[derive(Asset, Reflect, Clone, Debug)]
pub(crate) struct CompactStandardMaterial(#[dependency] StandardMaterial);

impl AsBindGroup for CompactStandardMaterial {
    type Data = StandardMaterialKey;
    type Param = <StandardMaterial as AsBindGroup>::Param;

    fn label() -> &'static str {
        "wc3 compact standard material"
    }

    fn bind_group_data(&self) -> Self::Data {
        self.0.bind_group_data()
    }

    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        param: &mut SystemParamItem<'_, '_, Self::Param>,
        _force_no_bindless: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let mut group = self.0.unprepared_bind_group(layout, device, param, true)?;
        group.bindings.retain(|(binding, _)| *binding <= 2);
        Ok(group)
    }

    fn bind_group_layout_entries(
        device: &RenderDevice,
        _force_no_bindless: bool,
    ) -> Vec<BindGroupLayoutEntry> {
        let mut entries = StandardMaterial::bind_group_layout_entries(device, true);
        entries.retain(|entry| entry.binding <= 2);
        entries
    }
}

impl Material for CompactStandardMaterial {
    fn fragment_shader() -> ShaderRef {
        StandardMaterial::fragment_shader()
    }
    fn deferred_fragment_shader() -> ShaderRef {
        StandardMaterial::deferred_fragment_shader()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        StandardMaterial::prepass_fragment_shader()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.0.alpha_mode()
    }
    fn opaque_render_method(&self) -> OpaqueRendererMethod {
        self.0.opaque_render_method()
    }
    fn depth_bias(&self) -> f32 {
        self.0.depth_bias()
    }
    fn reads_view_transmission_texture(&self) -> bool {
        self.0.reads_view_transmission_texture()
    }

    fn specialize(
        pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        StandardMaterial::specialize(
            pipeline,
            descriptor,
            layout,
            MaterialPipelineKey {
                mesh_key: key.mesh_key,
                bind_group_data: key.bind_group_data,
            },
        )?;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if let Some(path) = fragment.shader.path() {
                match path.path().to_str() {
                    Some("shaders/wc3_animated_alpha.wgsl") => fragment.shader = COMPACT_ALPHA,
                    Some("shaders/wc3_team_color.wgsl") => fragment.shader = COMPACT_TEAM,
                    _ => {}
                }
            }
            fragment
                .shader_defs
                .push(ShaderDefVal::Bool("WC3_COMPACT_BASE_COLOR".into(), true));
        }
        Ok(())
    }
}

type CompactAlphaMaterial = ExtendedMaterial<CompactStandardMaterial, Wc3AnimatedAlphaExtension>;
type CompactTeamMaterial = ExtendedMaterial<CompactStandardMaterial, Wc3TeamColorExtension>;

trait CompactSource: Material {
    type Compact: Material;
    fn base(&self) -> &StandardMaterial;
    fn compact(&self) -> Self::Compact;
}

impl CompactSource for Wc3AnimatedAlphaMaterial {
    type Compact = CompactAlphaMaterial;
    fn base(&self) -> &StandardMaterial {
        &self.base
    }
    fn compact(&self) -> Self::Compact {
        ExtendedMaterial {
            base: CompactStandardMaterial(self.base.clone()),
            extension: self.extension.clone(),
        }
    }
}

impl CompactSource for Wc3TeamColorMaterial {
    type Compact = CompactTeamMaterial;
    fn base(&self) -> &StandardMaterial {
        &self.base
    }
    fn compact(&self) -> Self::Compact {
        ExtendedMaterial {
            base: CompactStandardMaterial(self.base.clone()),
            extension: self.extension.clone(),
        }
    }
}

fn supports_compact_material(material: &StandardMaterial) -> bool {
    let ReflectRef::Struct(fields) = material.reflect_ref() else {
        return false;
    };
    // Include feature-gated and future image fields without enabling those
    // engine features or silently dropping a texture when they are present.
    fields.iter_fields().all(|(name, field)| {
        name == "base_color_texture"
            || field
                .try_downcast_ref::<Option<Handle<Image>>>()
                .is_none_or(Option::is_none)
    })
}

#[derive(Resource)]
struct CompactMaterialCache<M: CompactSource> {
    materials: HashMap<AssetId<M>, Handle<M::Compact>>,
    initialized: bool,
}

impl<M: CompactSource> Default for CompactMaterialCache<M> {
    fn default() -> Self {
        Self {
            materials: HashMap::new(),
            initialized: false,
        }
    }
}

fn sync_compact_materials<M: CompactSource>(
    experiment: Res<RenderExperiment>,
    state: Res<CompactShaderState>,
    mut events: MessageReader<AssetEvent<M>>,
    originals: Res<Assets<M>>,
    mut proxies: ResMut<Assets<M::Compact>>,
    mut cache: ResMut<CompactMaterialCache<M>>,
) {
    if *experiment != RenderExperiment::CompactMaterial
        || !matches!(*state, CompactShaderState::Ready)
    {
        return;
    }
    if !cache.initialized {
        for (id, original) in originals
            .iter()
            .filter(|(_, original)| supports_compact_material(original.base()))
        {
            cache.materials.insert(id, proxies.add(original.compact()));
        }
        cache.initialized = true;
        events.clear();
        return;
    }
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                cache.materials.remove(&id);
                if let Some(original) = originals.get(id)
                    && supports_compact_material(original.base())
                {
                    // A fresh ID prevents an older prepared proxy being used
                    // while the replacement material is still being uploaded.
                    cache.materials.insert(id, proxies.add(original.compact()));
                }
            }
            AssetEvent::Removed { id } => {
                cache.materials.remove(&id);
            }
            AssetEvent::Unused { .. } | AssetEvent::LoadedWithDependencies { .. } => {}
        }
    }
}

#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct CompactMaterialStats {
    pub(crate) selected: usize,
    pub(crate) pending: usize,
    pub(crate) unsupported: usize,
}

fn reset_compact_stats(mut stats: ResMut<CompactMaterialStats>) {
    *stats = default();
}

type CompactExtractionSources<'w, 's, M> = (
    Extract<'w, 's, Res<'static, RenderExperiment>>,
    Extract<'w, 's, Res<'static, CompactMaterialCache<M>>>,
    Extract<
        'w,
        's,
        Query<
            'static,
            'static,
            (Entity, &'static ViewVisibility, &'static MeshMaterial3d<M>),
            With<Mesh3d>,
        >,
    >,
);

fn extract_compact_materials<M: CompactSource>(
    sources: CompactExtractionSources<M>,
    prepared: Res<ErasedRenderAssets<PreparedMaterial>>,
    mut instances: ResMut<RenderMaterialInstances>,
    mut dirty: ResMut<DirtySpecializations>,
    mut reextract: Option<ResMut<MeshesToReextractNextFrame>>,
    mut stats: ResMut<CompactMaterialStats>,
) {
    let (experiment, cache, meshes) = sources;
    if **experiment != RenderExperiment::CompactMaterial {
        return;
    }
    instances.instances.retain(|entity, instance| {
        instance.asset_id.type_id() != TypeId::of::<M::Compact>()
            || meshes
                .get(entity.id())
                .is_ok_and(|(_, visibility, _)| visibility.get())
    });
    let tick = instances.current_change_tick;
    for (entity, visibility, original) in &meshes {
        if !visibility.get() {
            continue;
        }
        let selected = match cache.materials.get(&original.id()) {
            Some(proxy) if prepared.get(proxy.id()).is_some() => {
                stats.selected += 1;
                proxy.id().untyped()
            }
            Some(_) => {
                stats.pending += 1;
                original.id().untyped()
            }
            None => {
                stats.unsupported += 1;
                original.id().untyped()
            }
        };
        let main_entity = MainEntity::from(entity);
        if let Some(instance) = instances.instances.get_mut(&main_entity)
            && instance.asset_id != selected
        {
            instance.asset_id = selected;
            instance.last_change_tick = tick;
            dirty.changed_renderables.insert(main_entity);
            if let Some(reextract) = reextract.as_mut() {
                reextract.insert(main_entity);
            }
        }
    }
}
