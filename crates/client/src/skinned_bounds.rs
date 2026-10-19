//! Freezes skinned-mesh bounds once a mesh has settled after spawn.
//!
//! Bevy's `DynamicSkinnedMeshBounds` rebuilds each skinned mesh's local `Aabb` from
//! joint transforms every frame. Castle Fight models stay within a bounded envelope
//! around their rest pose, so after a short settle period the joint-derived bounds are
//! replaced by the last computed `Aabb`, inflated to cover animation poses. The `Aabb`
//! is local to the mesh entity, so it remains valid as the owning entity moves.

use bevy::{
    camera::{
        primitives::Aabb,
        visibility::{DynamicSkinnedMeshBounds, VisibilitySystems},
    },
    mesh::skinning::SkinnedMesh,
    prelude::*,
};

/// Frames a skinned mesh must keep a computed `Aabb` before its bounds are frozen.
const SETTLE_FRAMES: u8 = 8;
/// Fractional growth applied to every `Aabb` half-extent when bounds are frozen.
const FROZEN_BOUNDS_MARGIN: f32 = 0.3;

pub(crate) struct SkinnedMeshBoundsPlugin;

impl Plugin for SkinnedMeshBoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            freeze_settled_skinned_mesh_bounds
                .after(VisibilitySystems::CalculateBounds)
                .before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

/// Counts frames during which a skinned mesh has held a computed (non-zero) `Aabb`.
#[derive(Component, Debug, Clone, Copy, Default)]
struct SkinnedBoundsSettle(u8);

type SkinnedBoundsMeshes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Aabb,
        Option<&'static mut SkinnedBoundsSettle>,
    ),
    (With<DynamicSkinnedMeshBounds>, With<SkinnedMesh>),
>;

fn freeze_settled_skinned_mesh_bounds(mut commands: Commands, mut meshes: SkinnedBoundsMeshes) {
    for (entity, mut aabb, settle) in &mut meshes {
        // A zero-extent `Aabb` means joint bounds have not been computed yet.
        if aabb.half_extents == Vec3A::ZERO {
            continue;
        }
        let Some(mut settle) = settle else {
            commands.entity(entity).insert(SkinnedBoundsSettle(1));
            continue;
        };
        settle.0 = settle.0.saturating_add(1);
        if settle.0 < SETTLE_FRAMES {
            continue;
        }
        aabb.half_extents *= Vec3A::splat(1.0 + FROZEN_BOUNDS_MARGIN);
        commands
            .entity(entity)
            .remove::<(DynamicSkinnedMeshBounds, SkinnedBoundsSettle)>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skinned_mesh() -> SkinnedMesh {
        SkinnedMesh {
            inverse_bindposes: Handle::default(),
            joints: Vec::new(),
        }
    }

    fn settle_app() -> App {
        let mut app = App::new();
        app.add_systems(Update, freeze_settled_skinned_mesh_bounds);
        app
    }

    #[test]
    fn unset_bounds_are_never_frozen() {
        let mut app = settle_app();
        let mesh = app
            .world_mut()
            .spawn((skinned_mesh(), DynamicSkinnedMeshBounds, Aabb::default()))
            .id();
        for _ in 0..(SETTLE_FRAMES as usize * 2) {
            app.update();
        }
        assert!(app.world().get::<DynamicSkinnedMeshBounds>(mesh).is_some());
        assert!(app.world().get::<SkinnedBoundsSettle>(mesh).is_none());
    }

    #[test]
    fn settled_bounds_freeze_with_margin_after_settle_period() {
        let mut app = settle_app();
        let mesh = app
            .world_mut()
            .spawn((
                skinned_mesh(),
                DynamicSkinnedMeshBounds,
                Aabb {
                    center: Vec3A::new(0.5, 0.0, 0.0),
                    half_extents: Vec3A::new(1.0, 2.0, 4.0),
                },
            ))
            .id();
        for _ in 0..(SETTLE_FRAMES - 1) {
            app.update();
        }
        assert!(app.world().get::<DynamicSkinnedMeshBounds>(mesh).is_some());
        assert_eq!(
            app.world().get::<Aabb>(mesh).map(|aabb| aabb.half_extents),
            Some(Vec3A::new(1.0, 2.0, 4.0))
        );
        app.update();
        assert!(app.world().get::<DynamicSkinnedMeshBounds>(mesh).is_none());
        assert!(app.world().get::<SkinnedBoundsSettle>(mesh).is_none());
        let aabb = app.world().get::<Aabb>(mesh).expect("aabb kept");
        assert_eq!(aabb.center, Vec3A::new(0.5, 0.0, 0.0));
        assert!(
            aabb.half_extents
                .abs_diff_eq(Vec3A::new(1.3, 2.6, 5.2), 1e-5)
        );
    }
}
