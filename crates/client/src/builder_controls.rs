use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed, window::PrimaryWindow};

use castle_fight_sim::{BuildPosition, BuilderConfiguration, CommandSubmission, PlayerCommand};

use crate::{
    AuthoritativeSimulation, ClientCommandSubmission, SelectedMatch, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    build_ui::{
        ActionPanelMode, ActionPanelState, TargetingAction, cursor_over_action_panel,
        hotkey_just_pressed, placement_footprint, production_upgrade_hotkey_target,
        queue_production_upgrade, try_arm_build_target,
    },
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    demo::BuildKind,
    inspection::{cursor_over_inspector_panel, pick_building_at_ground, pick_unit_on_ray},
    presentation::{
        BuildingGridSnapState, WorldMetrics, viewport_ground_point, world_to_sim_point,
    },
    resource_ui::{BuilderShortcutState, cursor_over_builder_shortcuts, cursor_over_map_controls},
    terrain::TerrainSurface,
};

pub(crate) struct BuilderControlPlugin;

impl Plugin for BuilderControlPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            handle_selection_commands.after(crate::inspection::handle_world_selection),
        );
    }
}

#[derive(SystemParam)]
struct SelectionCommandResources<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    fixed_time: Res<'w, Time<Fixed>>,
    terrain: Res<'w, TerrainSurface>,
    metrics: Res<'w, WorldMetrics>,
    grid_snap: Res<'w, BuildingGridSnapState>,
    playback: Res<'w, SimulationPlayback>,
    debug_menu: Res<'w, DebugMenuState>,
    builder_shortcuts: Res<'w, BuilderShortcutState>,
    action_panel: ResMut<'w, ActionPanelState>,
    selected_match: Res<'w, SelectedMatch>,
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

    let command_card = resources.selected_match.content.command_card;

    let opened_build_menu = try_open_build_menu_hotkey(
        &resources.keys,
        command_card.build_hotkey,
        resources.presentation.current.builders.contains_key(&actor),
        &mut resources.action_panel,
    );

    if !opened_build_menu
        && resources.action_panel.mode == ActionPanelMode::BuildMenu
        && let Some(builder) = resources.authoritative.simulation.builder(actor)
        && let Some(kind) = build_menu_hotkey_target(
            &resources.keys,
            &resources.selected_match.direct_buildings,
            &builder.configuration,
            resources.selected_match.content,
        )
    {
        try_arm_build_target(
            &resources.authoritative,
            &mut resources.action_panel,
            kind,
            resources.selected_match.content,
        );
    }

    if !opened_build_menu
        && resources.action_panel.mode == ActionPanelMode::Actions
        && let Some(target) = production_upgrade_hotkey_target(
            &resources.keys,
            &resources.action_panel,
            &resources.authoritative,
            &resources.selected_match,
        )
    {
        queue_production_upgrade(
            &mut resources.authoritative,
            &mut resources.action_panel,
            target,
            &resources.selected_match,
            &resources.debug_menu,
        );
    }

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
        let controller = resources.debug_menu.controller_for_actor(
            &resources.authoritative.simulation,
            resources.selected_match.local_player,
            actor,
        );
        let submission = resources.authoritative.submit_local_command(
            controller,
            PlayerCommand::SetBuilderRepairAutocast {
                builder: actor,
                enabled,
            },
        );
        resources.action_panel.status = match command_submission_status(
            submission,
            format!(
                "Repair autocast {} queued.",
                if enabled { "enable" } else { "disable" }
            ),
            "Repair autocast command rejected",
        ) {
            Ok(status) | Err(status) => status,
        };
    }

    if resources.mouse_buttons.just_pressed(MouseButton::Right)
        && cancel_build_cursor_for_right_click(&mut resources.action_panel)
    {
        return;
    }

    if resources.playback.paused {
        return;
    }

    if resources.mouse_buttons.just_pressed(MouseButton::Left) {
        handle_modal_left_click(*window, *camera, &mut resources);
        return;
    }

    if resources.mouse_buttons.just_pressed(MouseButton::Right) {
        handle_smart_right_click(*window, *camera, &mut resources);
    }
}

fn cancel_build_cursor_for_right_click(action_panel: &mut ActionPanelState) -> bool {
    if !matches!(
        action_panel.mode,
        ActionPanelMode::Targeting(TargetingAction::Build(_))
    ) {
        return false;
    }
    action_panel.cancel_modal();
    true
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
    if cursor_over_action_panel(cursor, window.width(), window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width(), window.height())
        || cursor_over_debug_menu(cursor, resources.debug_menu.is_open())
        || cursor_over_builder_shortcuts(cursor, &resources.builder_shortcuts)
        || cursor_over_map_controls(cursor, window.width())
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
            let controller = resources.debug_menu.controller_for_actor(
                &resources.authoritative.simulation,
                resources.selected_match.local_player,
                actor,
            );
            let submission = resources.authoritative.submit_local_command(
                controller,
                PlayerCommand::MoveBuilder {
                    builder: actor,
                    destination,
                },
            );
            match command_submission_status(
                submission,
                "Builder move queued.".into(),
                "Move rejected",
            ) {
                Ok(status) => finish_modal(resources, status),
                Err(status) => resources.action_panel.status = status,
            }
        }
        TargetingAction::Blink => {
            let Some(world) = world else {
                resources.action_panel.status = "Blink rejected: no battlefield point.".into();
                return;
            };
            let destination = world_to_sim_point(world);
            let controller = resources.debug_menu.controller_for_actor(
                &resources.authoritative.simulation,
                resources.selected_match.local_player,
                actor,
            );
            let submission = resources.authoritative.submit_local_command(
                controller,
                PlayerCommand::BlinkBuilder {
                    builder: actor,
                    destination,
                },
            );
            match command_submission_status(
                submission,
                "Builder blink queued.".into(),
                "Blink rejected",
            ) {
                Ok(status) => finish_modal(resources, status),
                Err(status) => resources.action_panel.status = status,
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
            let controller = resources.debug_menu.controller_for_actor(
                &resources.authoritative.simulation,
                resources.selected_match.local_player,
                actor,
            );
            let submission = resources.authoritative.submit_local_command(
                controller,
                PlayerCommand::RepairWithBuilder {
                    builder: actor,
                    target,
                },
            );
            match command_submission_status(
                submission,
                format!("Builder repair of #{} queued.", target.0),
                "Repair rejected",
            ) {
                Ok(status) => finish_modal(resources, status),
                Err(status) => resources.action_panel.status = status,
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
            let controller = resources.debug_menu.controller_for_actor(
                &resources.authoritative.simulation,
                resources.selected_match.local_player,
                actor,
            );
            let submission = resources.authoritative.submit_local_command(
                controller,
                PlayerCommand::AttackWithBuilding {
                    building: actor,
                    target,
                },
            );
            match command_submission_status(
                submission,
                format!("Tower attack on #{} queued.", target.0),
                "Attack rejected",
            ) {
                Ok(status) => finish_modal(resources, status),
                Err(status) => resources.action_panel.status = status,
            }
        }
        TargetingAction::Build(kind) => {
            let Some(world) = world else {
                resources.action_panel.status =
                    "Placement rejected: cursor does not intersect the battlefield.".into();
                return;
            };
            let footprint = placement_footprint(
                &resources.metrics,
                world,
                resources.action_panel.team,
                kind,
                resources.selected_match.content,
                resources.grid_snap.enabled,
            );
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
            let controller = resources.debug_menu.controller_for_actor(
                &resources.authoritative.simulation,
                resources.selected_match.local_player,
                actor,
            );
            let submission = resources.authoritative.submit_local_command(
                controller,
                PlayerCommand::PlaceBuilding {
                    builder: actor,
                    building: kind.shared().stable_id(),
                    position: BuildPosition::new(footprint.min_x, footprint.min_y),
                },
            );
            match command_submission_status(
                submission,
                format!(
                    "{} queued. Builder will move into construction range after execution.",
                    kind.label(resources.selected_match.content)
                ),
                "Build order rejected",
            ) {
                Ok(status) => {
                    resources.action_panel.mode = ActionPanelMode::BuildMenu;
                    resources.action_panel.status = status;
                }
                Err(status) => resources.action_panel.status = status,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmartTarget {
    Unit(castle_fight_sim::SimId),
    Building(castle_fight_sim::SimId),
}

impl SmartTarget {
    const fn id(self) -> castle_fight_sim::SimId {
        match self {
            Self::Unit(id) | Self::Building(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmartActor {
    Builder { team: castle_fight_sim::Team },
    Tower { team: castle_fight_sim::Team },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SmartTargetInfo {
    target: SmartTarget,
    team: castle_fight_sim::Team,
    repairable: bool,
    damaged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmartRightClickAction {
    BuilderMove(castle_fight_sim::SimPoint),
    BuilderFollow(castle_fight_sim::SimId),
    BuilderRepair(castle_fight_sim::SimId),
    TowerAttack(castle_fight_sim::SimId),
    None,
}

fn handle_smart_right_click(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    resources: &mut SelectionCommandResources<'_>,
) {
    let Some(actor) = resources.action_panel.actor else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_action_panel(cursor, window.width(), window.height(), true)
        || cursor_over_inspector_panel(cursor, window.width(), window.height())
        || cursor_over_debug_menu(cursor, resources.debug_menu.is_open())
        || cursor_over_builder_shortcuts(cursor, &resources.builder_shortcuts)
        || cursor_over_map_controls(cursor, window.width())
    {
        return;
    }

    let (camera, camera_transform) = camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
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
    .map(SmartTarget::Unit)
    .or_else(|| {
        pick_building_at_ground(world, &resources.presentation, &resources.metrics)
            .map(SmartTarget::Building)
    });

    dismiss_action_mode_for_smart_order(&mut resources.action_panel);
    let actor_kind = smart_actor(actor, &resources.presentation.current);
    let target_info =
        target.and_then(|target| smart_target_info(target, &resources.presentation.current));
    let action = resolve_smart_right_click(actor_kind, world_to_sim_point(world), target_info);
    let (command, accepted, rejected_prefix) = match action {
        SmartRightClickAction::BuilderMove(destination) => (
            PlayerCommand::MoveBuilder {
                builder: actor,
                destination,
            },
            "Builder move queued.".to_owned(),
            "Builder move rejected",
        ),
        SmartRightClickAction::BuilderFollow(target) => (
            PlayerCommand::FollowWithBuilder {
                builder: actor,
                target,
            },
            format!("Builder follow of #{} queued.", target.0),
            "Builder follow rejected",
        ),
        SmartRightClickAction::BuilderRepair(target) => (
            PlayerCommand::RepairWithBuilder {
                builder: actor,
                target,
            },
            format!("Builder repair of #{} queued.", target.0),
            "Builder repair rejected",
        ),
        SmartRightClickAction::TowerAttack(target) => (
            PlayerCommand::AttackWithBuilding {
                building: actor,
                target,
            },
            format!("Tower attack on #{} queued.", target.0),
            "Tower attack rejected",
        ),
        SmartRightClickAction::None => return,
    };
    let controller = resources.debug_menu.controller_for_actor(
        &resources.authoritative.simulation,
        resources.selected_match.local_player,
        actor,
    );
    let submission = resources
        .authoritative
        .submit_local_command(controller, command);
    resources.action_panel.status =
        match command_submission_status(submission, accepted, rejected_prefix) {
            Ok(status) | Err(status) => status,
        };
}

fn smart_actor(
    actor: castle_fight_sim::SimId,
    snapshot: &PresentationSnapshot,
) -> Option<SmartActor> {
    if let Some(builder) = snapshot.builders.get(&actor) {
        return Some(SmartActor::Builder { team: builder.team });
    }
    snapshot.buildings.get(&actor).and_then(|building| {
        building
            .cooldown_remaining
            .is_some()
            .then_some(SmartActor::Tower {
                team: building.team,
            })
    })
}

fn smart_target_info(
    target: SmartTarget,
    snapshot: &PresentationSnapshot,
) -> Option<SmartTargetInfo> {
    match target {
        SmartTarget::Unit(id) => snapshot.units.get(&id).map(|unit| SmartTargetInfo {
            target,
            team: unit.team,
            repairable: unit.mechanical,
            damaged: unit.health > 0 && unit.health < unit.health_max,
        }),
        SmartTarget::Building(id) => snapshot.buildings.get(&id).map(|building| SmartTargetInfo {
            target,
            team: building.team,
            repairable: true,
            damaged: building.health > 0 && building.health < building.health_max,
        }),
    }
}

fn resolve_smart_right_click(
    actor: Option<SmartActor>,
    destination: castle_fight_sim::SimPoint,
    target: Option<SmartTargetInfo>,
) -> SmartRightClickAction {
    match actor {
        Some(SmartActor::Builder { team }) => match target {
            Some(target) if target.team == team && target.repairable && target.damaged => {
                SmartRightClickAction::BuilderRepair(target.target.id())
            }
            Some(target) => SmartRightClickAction::BuilderFollow(target.target.id()),
            None => SmartRightClickAction::BuilderMove(destination),
        },
        Some(SmartActor::Tower { team }) => match target {
            Some(target) if target.team != team => {
                SmartRightClickAction::TowerAttack(target.target.id())
            }
            _ => SmartRightClickAction::None,
        },
        None => SmartRightClickAction::None,
    }
}

fn dismiss_action_mode_for_smart_order(action_panel: &mut ActionPanelState) {
    action_panel.mode = ActionPanelMode::Actions;
    action_panel.status = "Choose an action.".into();
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

fn build_menu_hotkey_target(
    keys: &ButtonInput<KeyCode>,
    direct_buildings: &[BuildKind],
    builder: &BuilderConfiguration,
    content: &castle_fight_sim::CastleFightContentBundle,
) -> Option<BuildKind> {
    let mut selected = None;
    for &kind in direct_buildings {
        if builder.allows_building(kind.rawcode(content))
            && hotkey_just_pressed(keys, kind.hotkey(content))
        {
            selected = Some(kind);
        }
    }
    selected
}

fn command_submission_status(
    submission: ClientCommandSubmission,
    accepted: String,
    rejected_prefix: &str,
) -> Result<String, String> {
    match submission {
        ClientCommandSubmission::Local(CommandSubmission::Scheduled(command)) => {
            Ok(format!("{accepted} [tick {}]", command.tick))
        }
        ClientCommandSubmission::Local(CommandSubmission::DuplicateScheduled(command)) => Ok(
            format!("{accepted} [already scheduled for tick {}]", command.tick),
        ),
        ClientCommandSubmission::Local(
            CommandSubmission::Rejected(error) | CommandSubmission::DuplicateRejected(error),
        ) => Err(format!("{rejected_prefix}: {error:?}.")),
        ClientCommandSubmission::Submitted { client_sequence } => {
            Ok(format!("{accepted} [submitted #{client_sequence}]"))
        }
        ClientCommandSubmission::Failed => Err(format!("{rejected_prefix}: network unavailable.")),
    }
}

fn finish_modal(resources: &mut SelectionCommandResources<'_>, status: String) {
    resources.action_panel.mode = ActionPanelMode::Actions;
    resources.action_panel.status = status;
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
    fn build_menu_hotkey_collision_uses_last_eligible_building() {
        let demo = crate::demo::create_demo_world(1, None);
        let builder = demo
            .simulation
            .builder_for_team(castle_fight_sim::Team(0))
            .unwrap();
        let collision_hotkey = demo
            .direct_buildings
            .iter()
            .copied()
            .map(|kind| kind.hotkey(demo.content))
            .find(|hotkey| {
                demo.direct_buildings
                    .iter()
                    .filter(|kind| kind.hotkey(demo.content) == *hotkey)
                    .count()
                    > 1
            })
            .expect("mixed demo catalog should exercise a hotkey collision");
        let expected = demo
            .direct_buildings
            .iter()
            .copied()
            .rfind(|kind| {
                builder
                    .configuration
                    .allows_building(kind.rawcode(demo.content))
                    && kind.hotkey(demo.content) == collision_hotkey
            })
            .expect("collision hotkey must have an eligible claimant");

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(crate::build_ui::key_code_for_hotkey(collision_hotkey).unwrap());
        assert_eq!(
            build_menu_hotkey_target(
                &keys,
                &demo.direct_buildings,
                &builder.configuration,
                demo.content,
            ),
            Some(expected)
        );
    }

    #[test]
    fn right_click_with_build_ghost_only_clears_the_build_cursor() {
        let mut state = ActionPanelState {
            mode: ActionPanelMode::Targeting(TargetingAction::Build(
                crate::demo::BuildKind::Production(crate::demo::ProductionKind::Barracks),
            )),
            ..ActionPanelState::default()
        };
        assert!(cancel_build_cursor_for_right_click(&mut state));
        assert_eq!(state.mode, ActionPanelMode::BuildMenu);

        assert!(!cancel_build_cursor_for_right_click(&mut state));
        assert_eq!(state.mode, ActionPanelMode::BuildMenu);
    }

    #[test]
    fn right_click_smart_order_dismisses_submenus_and_targeting() {
        let mut state = ActionPanelState {
            mode: ActionPanelMode::BuildMenu,
            ..ActionPanelState::default()
        };
        dismiss_action_mode_for_smart_order(&mut state);
        assert_eq!(state.mode, ActionPanelMode::Actions);

        state.mode = ActionPanelMode::Targeting(TargetingAction::Build(
            crate::demo::BuildKind::Production(crate::demo::ProductionKind::Barracks),
        ));
        dismiss_action_mode_for_smart_order(&mut state);
        assert_eq!(state.mode, ActionPanelMode::Actions);
    }

    #[test]
    fn smart_right_click_matches_wc3_context_for_supported_castle_fight_actions() {
        use castle_fight_sim::{SimId, SimPoint, Team};

        let destination = SimPoint::new(123, 456);
        let damaged_mech = SmartTargetInfo {
            target: SmartTarget::Unit(SimId(10)),
            team: Team(0),
            repairable: true,
            damaged: true,
        };
        assert_eq!(
            resolve_smart_right_click(
                Some(SmartActor::Builder { team: Team(0) }),
                destination,
                Some(damaged_mech),
            ),
            SmartRightClickAction::BuilderRepair(SimId(10))
        );

        let healthy_mech = SmartTargetInfo {
            damaged: false,
            ..damaged_mech
        };
        assert_eq!(
            resolve_smart_right_click(
                Some(SmartActor::Builder { team: Team(0) }),
                destination,
                Some(healthy_mech),
            ),
            SmartRightClickAction::BuilderFollow(SimId(10))
        );
        assert_eq!(
            resolve_smart_right_click(
                Some(SmartActor::Builder { team: Team(0) }),
                destination,
                None,
            ),
            SmartRightClickAction::BuilderMove(destination)
        );

        let enemy = SmartTargetInfo {
            target: SmartTarget::Building(SimId(20)),
            team: Team(1),
            repairable: true,
            damaged: true,
        };
        assert_eq!(
            resolve_smart_right_click(
                Some(SmartActor::Tower { team: Team(0) }),
                destination,
                Some(enemy),
            ),
            SmartRightClickAction::TowerAttack(SimId(20))
        );
        assert_eq!(
            resolve_smart_right_click(Some(SmartActor::Tower { team: Team(0) }), destination, None,),
            SmartRightClickAction::None
        );
    }
}
