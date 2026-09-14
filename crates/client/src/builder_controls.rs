use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed, window::PrimaryWindow};

use crate::{
    AuthoritativeSimulation, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    build_ui::{BuildSelection, cursor_over_build_panel},
    inspection::{
        InspectionSelection, cursor_over_inspector_panel, pick_building_at_ground, pick_unit_on_ray,
    },
    presentation::{WorldMetrics, viewport_ground_point, world_to_sim_point},
    terrain::TerrainSurface,
};

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuilderControlState {
    pub(crate) blink_armed: bool,
}

pub(crate) struct BuilderControlPlugin;

impl Plugin for BuilderControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuilderControlState>().add_systems(
            Update,
            (sync_build_side_to_selected_builder, handle_builder_controls).chain(),
        );
    }
}

fn sync_build_side_to_selected_builder(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    mut build_selection: ResMut<BuildSelection>,
) {
    let Some(selected) = selection.selected else {
        return;
    };
    let Some(builder) = samples.current.builders.get(&selected) else {
        return;
    };
    build_selection.team = builder.team;
}

#[derive(SystemParam)]
struct BuilderControlResources<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    fixed_time: Res<'w, Time<Fixed>>,
    terrain: Res<'w, TerrainSurface>,
    metrics: Res<'w, WorldMetrics>,
    selection: Res<'w, InspectionSelection>,
    playback: Res<'w, SimulationPlayback>,
    controls: ResMut<'w, BuilderControlState>,
    build_selection: ResMut<'w, BuildSelection>,
    authoritative: ResMut<'w, AuthoritativeSimulation>,
    presentation: ResMut<'w, PresentationSamples>,
}

fn handle_builder_controls(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut resources: BuilderControlResources<'_>,
) {
    let Some(builder_id) = resources.selection.selected else {
        resources.controls.blink_armed = false;
        return;
    };
    let Some(builder) = resources
        .presentation
        .current
        .builders
        .get(&builder_id)
        .copied()
    else {
        resources.controls.blink_armed = false;
        return;
    };

    if resources.keys.just_pressed(KeyCode::KeyD) {
        resources.controls.blink_armed = !resources.controls.blink_armed;
        resources.build_selection.status = if resources.controls.blink_armed {
            "Blink armed — right-click a point in or near your base. Press D again to cancel."
                .into()
        } else {
            "Blink targeting cancelled.".into()
        };
    }

    if resources.keys.just_pressed(KeyCode::KeyR) {
        let enabled = !builder.repair_autocast_enabled;
        match resources
            .authoritative
            .simulation
            .set_builder_repair_autocast(builder_id, enabled)
        {
            Ok(()) => {
                resources.build_selection.status = format!(
                    "Repair autocast {}.",
                    if enabled { "enabled" } else { "disabled" }
                );
                publish_snapshot(&resources.authoritative, &mut resources.presentation);
            }
            Err(error) => {
                resources.build_selection.status =
                    format!("Repair autocast command rejected: {error:?}.");
            }
        }
    }

    if resources.playback.paused || !resources.mouse_buttons.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_build_panel(cursor, window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width())
        || resources.build_selection.kind.is_some()
    {
        return;
    }

    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        return;
    };
    let destination = world_to_sim_point(world);

    if resources.controls.blink_armed {
        match resources
            .authoritative
            .simulation
            .order_builder_blink(builder_id, destination)
        {
            Ok(resolved) => {
                resources.controls.blink_armed = false;
                resources.build_selection.status = format!(
                    "Blinked to {:.0}, {:.0}.",
                    resolved.x as f32 / castle_fight_sim::SUBUNITS_PER_WORLD_UNIT as f32,
                    resolved.y as f32 / castle_fight_sim::SUBUNITS_PER_WORLD_UNIT as f32,
                );
                publish_snapshot(&resources.authoritative, &mut resources.presentation);
            }
            Err(error) => {
                resources.build_selection.status = format!("Blink rejected: {error:?}.");
            }
        }
        return;
    }

    let alpha = resources
        .playback
        .interpolation_alpha(&resources.fixed_time);
    let repair_target = pick_unit_on_ray(
        ray.origin,
        *ray.direction,
        &resources.presentation,
        &resources.terrain,
        alpha,
    )
    .filter(|target| {
        resources
            .presentation
            .current
            .units
            .get(target)
            .is_some_and(|unit| unit.team == builder.team && unit.mechanical)
    })
    .or_else(|| {
        pick_building_at_ground(world, &resources.presentation, &resources.metrics).filter(
            |target| {
                resources
                    .presentation
                    .current
                    .buildings
                    .get(target)
                    .is_some_and(|building| building.team == builder.team)
            },
        )
    });

    let result = if let Some(target) = repair_target {
        resources
            .authoritative
            .simulation
            .order_builder_repair(builder_id, target)
            .map(|()| format!("Builder repairing #{}.", target.0))
    } else {
        resources
            .authoritative
            .simulation
            .order_builder_move(builder_id, destination)
            .map(|()| "Builder move ordered.".to_owned())
    };
    match result {
        Ok(status) => {
            resources.build_selection.status = status;
            publish_snapshot(&resources.authoritative, &mut resources.presentation);
        }
        Err(error) => {
            resources.build_selection.status = format!("Builder command rejected: {error:?}.");
        }
    }
}

fn publish_snapshot(
    authoritative: &AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
) {
    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
}
