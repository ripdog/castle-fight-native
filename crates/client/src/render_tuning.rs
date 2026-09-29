//! Renderer configuration that preserves Bevy material semantics while tuning
//! bindless resource-group sizing for Castle Fight's workload.

use std::any::TypeId;

use bevy::{
    pbr::{
        MaterialBindGroupAllocator, MaterialBindGroupAllocators, material_uses_bindless_resources,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{AsBindGroup, BindlessSlabResourceLimit},
        renderer::RenderDevice,
    },
};

pub(crate) const DEFAULT_STANDARD_MATERIAL_BINDLESS_SLOTS: u32 = 64;

#[derive(Resource, Clone, Copy)]
struct StandardMaterialBindlessSlabLimit(u32);

pub(crate) struct StandardMaterialBindlessSlabPlugin {
    slot_count: Option<u32>,
}

impl StandardMaterialBindlessSlabPlugin {
    pub(crate) fn new(slot_count: Option<u32>) -> Self {
        Self { slot_count }
    }
}

impl Plugin for StandardMaterialBindlessSlabPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let Some(slot_count) = self.slot_count else {
            return;
        };
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(StandardMaterialBindlessSlabLimit(slot_count))
            .add_systems(
                Render,
                override_standard_material_bindless_allocator.before(RenderSystems::PrepareAssets),
            );
    }
}

fn override_standard_material_bindless_allocator(
    limit: Res<StandardMaterialBindlessSlabLimit>,
    render_device: Res<RenderDevice>,
    mut allocators: ResMut<MaterialBindGroupAllocators>,
    mut applied: Local<bool>,
) {
    if *applied {
        return;
    }
    *applied = true;

    if !material_uses_bindless_resources::<StandardMaterial>(&render_device) {
        return;
    }

    allocators.insert(
        TypeId::of::<StandardMaterial>(),
        MaterialBindGroupAllocator::new(
            &render_device,
            <StandardMaterial as AsBindGroup>::label(),
            <StandardMaterial as AsBindGroup>::bindless_descriptor(),
            <StandardMaterial as AsBindGroup>::bind_group_layout_descriptor(&render_device),
            Some(BindlessSlabResourceLimit::Custom(limit.0)),
        ),
    );
}
