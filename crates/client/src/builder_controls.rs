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

    let command_card = castle_fight_sim::castle_fight_command_card_layout();

    try_open_build_menu_hotkey(
        &resources.keys,
        command_card.build_hotkey,
        resources.presentation.current.builders.contains_key(&actor),
        &mut resources.action_panel,
    );

    // Castle Fight's live Blink uses D. Enter the same point-targeting mode as clicking Blink on
    // the command card instead of maintaining a separate keyboard-only state machine.
    if resources.action_panel.mode == ActionPanelMode::Actions
        && hotkey_just_pressed(&resources.keys, command_card.blink_hotkey)
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

    if resources.mouse_buttons.just_pressed(MouseButton::Right) {
        handle_builder_move_right_click(*window, *camera, &mut resources);
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

fn handle_builder_move_right_click(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    resources: &mut SelectionCommandResources<'_>,
) {
    let Some(builder_id) = resources.selection.selected else {
        return;
    };
    if !resources
        .presentation
        .current
        .builders
        .contains_key(&builder_id)
    {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width())
    {
        return;
    }
    let (camera, camera_transform) = camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        return;
    };
    let destination = world_to_sim_point(world);

    // Warcraft right-click movement takes precedence over any open command submenu or targeting
    // mode. Cancelling the UI mode first also makes a rejected move behave like an ordinary
    // right-click cancel rather than leaving the old targeting cursor armed.
    dismiss_action_mode_for_move(&mut resources.action_panel);
    let result = resources
        .authoritative
        .simulation
        .order_builder_move(builder_id, destination)
        .map(|()| "Builder move ordered.".to_owned());
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

fn dismiss_action_mode_for_move(action_panel: &mut ActionPanelState) {
    action_panel.mode = ActionPanelMode::Actions;
}

fn try_open_build_menu_hotkey(
    keys: &ButtonInput<KeyCode>,
    build_hotkey: char,
    actor_is_builder: bool,
    action_panel: &mut ActionPanelState,
) -> bool {
    if action_panel.mode != ActionPanelMode::Actions
        || !actor_is_builder
        || !hotkey_just_pressed(keys, build_hotkey)
    {
        return false;
    }
    action_panel.mode = ActionPanelMode::BuildMenu;
    action_panel.status = "Choose a building.".into();
    true
}

fn hotkey_just_pressed(keys: &ButtonInput<KeyCode>, hotkey: char) -> bool {
    let key_code = match hotkey.to_ascii_uppercase() {
        'A' => KeyCode::KeyA,
        'B' => KeyCode::KeyB,
        'C' => KeyCode::KeyC,
        'D' => KeyCode::KeyD,
        'E' => KeyCode::KeyE,
        'F' => KeyCode::KeyF,
        'G' => KeyCode::KeyG,
        'H' => KeyCode::KeyH,
        'I' => KeyCode::KeyI,
        'J' => KeyCode::KeyJ,
        'K' => KeyCode::KeyK,
        'L' => KeyCode::KeyL,
        'M' => KeyCode::KeyM,
        'N' => KeyCode::KeyN,
        'O' => KeyCode::KeyO,
        'P' => KeyCode::KeyP,
        'Q' => KeyCode::KeyQ,
        'R' => KeyCode::KeyR,
        'S' => KeyCode::KeyS,
        'T' => KeyCode::KeyT,
        'U' => KeyCode::KeyU,
        'V' => KeyCode::KeyV,
        'W' => KeyCode::KeyW,
        'X' => KeyCode::KeyX,
        'Y' => KeyCode::KeyY,
        'Z' => KeyCode::KeyZ,
        _ => return false,
    };
    keys.just_pressed(key_code)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_build_hotkey_opens_the_build_menu() {
        let command_card = castle_fight_sim::castle_fight_command_card_layout();
        assert_eq!(command_card.build_hotkey, 'B');

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyB);
        let mut state = ActionPanelState::default();
        assert!(try_open_build_menu_hotkey(
            &keys,
            command_card.build_hotkey,
            true,
            &mut state,
        ));
        assert_eq!(state.mode, ActionPanelMode::BuildMenu);

        state.mode = ActionPanelMode::Actions;
        assert!(!try_open_build_menu_hotkey(
            &keys,
            command_card.build_hotkey,
            false,
            &mut state,
        ));
        assert_eq!(state.mode, ActionPanelMode::Actions);
    }

    #[test]
    fn right_click_move_dismisses_submenus_and_targeting() {
        let mut state = ActionPanelState {
            mode: ActionPanelMode::BuildMenu,
            ..ActionPanelState::default()
        };
        dismiss_action_mode_for_move(&mut state);
        assert_eq!(state.mode, ActionPanelMode::Actions);

        state.mode = ActionPanelMode::Targeting(TargetingAction::Build(
            crate::demo::BuildKind::Production(crate::demo::ProductionKind::Barracks),
        ));
        dismiss_action_mode_for_move(&mut state);
        assert_eq!(state.mode, ActionPanelMode::Actions);
    }
}
