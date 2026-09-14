use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed, window::PrimaryWindow};

use crate::{
    AuthoritativeSimulation, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    build_ui::{
        ActionPanelMode, ActionPanelState, TargetingAction, cursor_over_action_panel,
        placement_footprint,
    },
    demo::order_demo_building,
    inspection::{
        InspectionSelection, cursor_over_inspector_panel, pick_building_at_ground, pick_unit_on_ray,
    },
    presentation::{WorldMetrics, viewport_ground_point, world_to_sim_point},
    terrain::TerrainSurface,
};

pub(crate) struct BuilderControlPlugin;

impl Plugin for BuilderControlPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_selection_commands);
    }
}

#[derive(SystemParam)]
struct SelectionCommandResources<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    fixed_time: Res<'w, Time<Fixed>>,
    terrain: Res<'w, TerrainSurface>,
    metrics: Res<'w, WorldMetrics>,
    selection: Res<'w, InspectionSelection>,
    playback: Res<'w, SimulationPlayback>,
    action_panel: ResMut<'w, ActionPanelState>,
    authoritative: ResMut<'w, AuthoritativeSimulation>,
    presentation: ResMut<'w, PresentationSamples>,
}

fn handle_selection_commands(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut resources: SelectionCommandResources<'_>,
) {
    let Some(actor) = resources.action_panel.actor else {
        return;
    };

    // Castle Fight's live Blink uses D. Enter the same point-targeting mode as clicking Blink on
    // the command card instead of maintaining a separate keyboard-only state machine.
    if resources.action_panel.mode == ActionPanelMode::Actions
        && resources.keys.just_pressed(KeyCode::KeyD)
        && resources.presentation.current.builders.contains_key(&actor)
    {
        resources.action_panel.mode = ActionPanelMode::Targeting(TargetingAction::Blink);
        resources.action_panel.status = "Blink: left-click a destination; Esc cancels.".into();
    }

    // Repair autocast remains a toggle, not a targeting command. The targeted Repair command is
    // exposed by the action panel and uses the common modal path below.
    if resources.action_panel.mode == ActionPanelMode::Actions
        && resources.keys.just_pressed(KeyCode::KeyR)
        && let Some(builder) = resources.presentation.current.builders.get(&actor).copied()
    {
        let enabled = !builder.repair_autocast_enabled;
        match resources
            .authoritative
            .simulation
            .set_builder_repair_autocast(actor, enabled)
        {
            Ok(()) => {
                resources.action_panel.status = format!(
                    "Repair autocast {}.",
                    if enabled { "enabled" } else { "disabled" }
                );
                publish_snapshot(&resources.authoritative, &mut resources.presentation);
            }
            Err(error) => {
                resources.action_panel.status =
                    format!("Repair autocast command rejected: {error:?}.");
            }
        }
    }

    if resources.playback.paused {
        return;
    }

    if resources.mouse_buttons.just_pressed(MouseButton::Left) {
        handle_modal_left_click(*window, *camera, &mut resources);
        return;
    }

    if resources.mouse_buttons.just_pressed(MouseButton::Right)
        && resources.action_panel.mode == ActionPanelMode::Actions
    {
        handle_builder_smart_right_click(*window, *camera, &mut resources);
    }
}

fn handle_modal_left_click(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    resources: &mut SelectionCommandResources<'_>,
) {
    let Some(action) = resources.action_panel.targeting() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width())
    {
        return;
    }
    let (camera, camera_transform) = camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let world = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain);
    let actor = resources
        .action_panel
        .actor
        .expect("targeting mode requires an action-panel actor");

    match action {
        TargetingAction::Move => {
            let Some(world) = world else {
                resources.action_panel.status = "Move rejected: no battlefield point.".into();
                return;
            };
            let destination = world_to_sim_point(world);
            match resources
                .authoritative
                .simulation
                .order_builder_move(actor, destination)
            {
                Ok(()) => finish_modal(resources, "Builder move ordered.".into()),
                Err(error) => {
                    resources.action_panel.status = format!("Move rejected: {error:?}.");
                }
            }
        }
        TargetingAction::Blink => {
            let Some(world) = world else {
                resources.action_panel.status = "Blink rejected: no battlefield point.".into();
                return;
            };
            let destination = world_to_sim_point(world);
            match resources
                .authoritative
                .simulation
                .order_builder_blink(actor, destination)
            {
                Ok(resolved) => finish_modal(
                    resources,
                    format!(
                        "Blinked to {:.0}, {:.0}.",
                        resolved.x as f32 / castle_fight_sim::SUBUNITS_PER_WORLD_UNIT as f32,
                        resolved.y as f32 / castle_fight_sim::SUBUNITS_PER_WORLD_UNIT as f32,
                    ),
                ),
                Err(error) => {
                    resources.action_panel.status = format!("Blink rejected: {error:?}.");
                }
            }
        }
        TargetingAction::Repair => {
            let Some(world) = world else {
                resources.action_panel.status = "Repair rejected: no target.".into();
                return;
            };
            let Some(builder) = resources.presentation.current.builders.get(&actor).copied() else {
                resources.action_panel.status = "Repair rejected: builder is gone.".into();
                return;
            };
            let alpha = resources
                .playback
                .interpolation_alpha(&resources.fixed_time);
            let target = pick_unit_on_ray(
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
            let Some(target) = target else {
                resources.action_panel.status =
                    "Repair requires a friendly building or mechanical unit.".into();
                return;
            };
            match resources
                .authoritative
                .simulation
                .order_builder_repair(actor, target)
            {
                Ok(()) => finish_modal(resources, format!("Builder repairing #{}.", target.0)),
                Err(error) => {
                    resources.action_panel.status = format!("Repair rejected: {error:?}.");
                }
            }
        }
        TargetingAction::Attack => {
            let Some(world) = world else {
                resources.action_panel.status = "Attack rejected: no target.".into();
                return;
            };
            let source_team = resources
                .presentation
                .current
                .buildings
                .get(&actor)
                .map(|building| building.team);
            let Some(source_team) = source_team else {
                resources.action_panel.status = "Attack rejected: tower is gone.".into();
                return;
            };
            let alpha = resources
                .playback
                .interpolation_alpha(&resources.fixed_time);
            let target = pick_unit_on_ray(
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
                    .is_some_and(|unit| unit.team != source_team)
            })
            .or_else(|| {
                pick_building_at_ground(world, &resources.presentation, &resources.metrics).filter(
                    |target| {
                        resources
                            .presentation
                            .current
                            .buildings
                            .get(target)
                            .is_some_and(|building| building.team != source_team)
                    },
                )
            });
            let Some(target) = target else {
                resources.action_panel.status = "Attack requires an enemy target.".into();
                return;
            };
            match resources
                .authoritative
                .simulation
                .order_building_attack_target(actor, target)
            {
                Ok(()) => finish_modal(resources, format!("Tower attacking #{}.", target.0)),
                Err(error) => {
                    resources.action_panel.status = format!("Attack rejected: {error:?}.");
                }
            }
        }
        TargetingAction::Build(kind) => {
            let Some(world) = world else {
                resources.action_panel.status =
                    "Placement rejected: cursor does not intersect the battlefield.".into();
                return;
            };
            let footprint = placement_footprint(&resources.metrics, world, kind);
            if !resources
                .authoritative
                .simulation
                .can_place_building_for_team(resources.action_panel.team, footprint)
            {
                resources.action_panel.status =
                    "Placement rejected: outside this side's build region, blocked, or occupied."
                        .into();
                return;
            }
            match order_demo_building(
                &mut resources.authoritative.simulation,
                resources.action_panel.team,
                footprint,
                kind,
            ) {
                Ok(()) => {
                    resources.action_panel.mode = ActionPanelMode::BuildMenu;
                    resources.action_panel.status = format!(
                        "{} ordered. Builder will move into construction range.",
                        kind.label()
                    );
                    publish_snapshot(&resources.authoritative, &mut resources.presentation);
                }
                Err(error) => {
                    resources.action_panel.status = format!("Build order rejected: {error:?}.");
                }
            }
        }
    }
}

fn handle_builder_smart_right_click(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    resources: &mut SelectionCommandResources<'_>,
) {
    let Some(builder_id) = resources.selection.selected else {
        return;
    };
    let Some(builder) = resources
        .presentation
        .current
        .builders
        .get(&builder_id)
        .copied()
    else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width())
    {
        return;
    }
    let (camera, camera_transform) = camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        return;
    };
    let destination = world_to_sim_point(world);
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
            resources.action_panel.status = status;
            publish_snapshot(&resources.authoritative, &mut resources.presentation);
        }
        Err(error) => {
            resources.action_panel.status = format!("Builder command rejected: {error:?}.");
        }
    }
}

fn finish_modal(resources: &mut SelectionCommandResources<'_>, status: String) {
    resources.action_panel.mode = ActionPanelMode::Actions;
    resources.action_panel.status = status;
    publish_snapshot(&resources.authoritative, &mut resources.presentation);
}

fn publish_snapshot(
    authoritative: &AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
) {
    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
}
