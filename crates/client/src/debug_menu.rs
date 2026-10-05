use bevy::{prelude::*, time::Fixed};
use castle_fight_protocol::{DebugRequest, DebugSpeed, WireDebugCommand};
use castle_fight_sim::{CASTLE_FIGHT_SIMULATION_HZ, PlayerId, SimId, Simulation};

use crate::{
    AuthoritativeSimulation, SimulationPlayback, advance_authoritative_simulation_once,
    bridge::{PresentationSamples, PresentationSnapshot},
    resource_ui::TOP_BAR_HEIGHT,
};

const PANEL_LEFT: f32 = 12.0;
const PANEL_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const PANEL_WIDTH: f32 = 360.0;
const PANEL_HEIGHT: f32 = 496.0;
const PANEL_PADDING: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 38.0;
const BUTTON_GAP: f32 = 6.0;

const PANEL_BACKGROUND: Color = Color::srgba(0.030, 0.035, 0.045, 0.97);
const PANEL_BORDER: Color = Color::srgb(0.42, 0.33, 0.17);
const BUTTON_NORMAL: Color = Color::srgb(0.09, 0.10, 0.12);
const BUTTON_HOVERED: Color = Color::srgb(0.17, 0.18, 0.21);
const BUTTON_SELECTED: Color = Color::srgb(0.20, 0.16, 0.08);
const BUTTON_DISABLED: Color = Color::srgb(0.055, 0.058, 0.065);
const BORDER_NORMAL: Color = Color::srgb(0.31, 0.33, 0.38);
const BORDER_SELECTED: Color = Color::srgb(0.88, 0.70, 0.22);
const TEXT_NORMAL: Color = Color::srgb(0.90, 0.91, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.56, 0.58, 0.63);
const STATUS_COLOR: Color = Color::srgb(0.75, 0.78, 0.84);

#[derive(Resource, Debug)]
pub(crate) struct DebugMenuState {
    open: bool,
    speed: DebugSpeed,
    control_all_players: bool,
    buildings_invulnerable: bool,
    status: String,
}

impl Default for DebugMenuState {
    fn default() -> Self {
        Self {
            open: false,
            speed: DebugSpeed::Normal,
            control_all_players: false,
            buildings_invulnerable: false,
            status: "F8 closes this menu.".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebugAction {
    GrantResources,
    KillAllUnits,
    ToggleControlAllPlayers,
    ToggleBuildingsInvulnerable,
    PopulateBuildings,
    TogglePause,
    StepOneTick,
    SetSpeed(DebugSpeed),
}

#[derive(Component)]
struct DebugMenuRoot;

#[derive(Component, Debug, Clone, Copy)]
struct DebugMenuButton(DebugAction);

#[derive(Component)]
struct DebugMenuButtonLabel;

#[derive(Component)]
struct DebugStatusText;

pub(crate) struct DebugMenuPlugin;

impl Plugin for DebugMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugMenuState>()
            .add_systems(Startup, setup_debug_menu)
            .add_systems(
                Update,
                (
                    toggle_debug_menu,
                    handle_debug_buttons,
                    update_debug_menu,
                    style_debug_buttons,
                )
                    .chain(),
            );
    }
}

fn setup_debug_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(PANEL_LEFT),
                top: px(PANEL_TOP),
                width: px(PANEL_WIDTH),
                height: px(PANEL_HEIGHT),
                padding: UiRect::all(px(PANEL_PADDING)),
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::all(px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(PANEL_BORDER),
            Visibility::Hidden,
            ZIndex(1100),
            DebugMenuRoot,
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("DEBUG / CHEATS   [F8]"),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(1.0, 0.82, 0.30)),
            ));

            spawn_debug_button(
                panel,
                DebugAction::GrantResources,
                "Give all players +1,000,000 gold / lumber",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::KillAllUnits,
                "Kill all units (9999 damage)",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::ToggleControlAllPlayers,
                "Control all players: OFF",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::ToggleBuildingsInvulnerable,
                "Buildings invulnerable: OFF",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::PopulateBuildings,
                "Populate implemented building lines",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::TogglePause,
                "Pause simulation",
                percent(100.0),
            );
            spawn_debug_button(
                panel,
                DebugAction::StepOneTick,
                "Step one tick (pause first)",
                percent(100.0),
            );

            panel.spawn((
                Text::new("SIMULATION SPEED"),
                TextFont::from_font_size(12.0),
                TextColor(TEXT_MUTED),
            ));
            panel
                .spawn((Node {
                    width: percent(100.0),
                    height: px(BUTTON_HEIGHT),
                    column_gap: px(BUTTON_GAP),
                    ..default()
                },))
                .with_children(|row| {
                    for speed in DebugSpeed::ALL {
                        spawn_debug_button(
                            row,
                            DebugAction::SetSpeed(speed),
                            speed.label(),
                            percent(20.0),
                        );
                    }
                });

            panel.spawn((
                Text::new("F8 closes this menu."),
                TextFont::from_font_size(12.0),
                TextColor(STATUS_COLOR),
                Node {
                    width: percent(100.0),
                    ..default()
                },
                DebugStatusText,
            ));
        });
}

fn spawn_debug_button(
    parent: &mut ChildSpawnerCommands,
    action: DebugAction,
    label: &'static str,
    width: Val,
) {
    parent
        .spawn((
            Button,
            Node {
                width,
                height: px(BUTTON_HEIGHT),
                padding: UiRect::horizontal(px(8.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 1.0,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(BORDER_NORMAL),
            DebugMenuButton(action),
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(12.0),
            TextColor(TEXT_NORMAL),
            TextLayout::justify(Justify::Center),
            DebugMenuButtonLabel,
        ));
}

fn toggle_debug_menu(
    authoritative: Res<AuthoritativeSimulation>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DebugMenuState>,
    mut visibility: Single<&mut Visibility, With<DebugMenuRoot>>,
) {
    if !authoritative.debug_available() {
        state.open = false;
        state.control_all_players = false;
        **visibility = Visibility::Hidden;
        return;
    }
    if !keys.just_pressed(KeyCode::F8) {
        return;
    }
    state.open = !state.open;
    **visibility = if state.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

fn handle_debug_buttons(
    mut buttons: Query<(&Interaction, &DebugMenuButton), Changed<Interaction>>,
    mut state: ResMut<DebugMenuState>,
    mut playback: ResMut<SimulationPlayback>,
    mut fixed_time: ResMut<Time<Fixed>>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    if !state.open || !authoritative.debug_available() {
        return;
    }

    for (interaction, button) in &mut buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let request = match button.0 {
            DebugAction::GrantResources => DebugRequest::Command {
                command: WireDebugCommand::GrantResources,
            },
            DebugAction::KillAllUnits => DebugRequest::Command {
                command: WireDebugCommand::KillAllUnits,
            },
            DebugAction::ToggleControlAllPlayers => {
                state.control_all_players = !state.control_all_players;
                state.status = if state.control_all_players {
                    "Debug control enabled for every player-owned builder and building.".into()
                } else {
                    "Debug control restored to normal player authority.".into()
                };
                continue;
            }
            DebugAction::ToggleBuildingsInvulnerable => DebugRequest::Command {
                command: WireDebugCommand::SetBuildingsInvulnerable {
                    enabled: !authoritative.simulation.debug_buildings_invulnerable(),
                },
            },
            DebugAction::PopulateBuildings => DebugRequest::Command {
                command: WireDebugCommand::PopulateBuildings,
            },
            DebugAction::TogglePause => DebugRequest::SetPlayback {
                paused: !playback.paused,
                speed: state.speed,
            },
            DebugAction::StepOneTick => DebugRequest::StepOneTick,
            DebugAction::SetSpeed(speed) => DebugRequest::SetPlayback {
                paused: playback.paused,
                speed,
            },
        };
        if authoritative.is_networked() {
            state.status = match authoritative.submit_debug_request(request) {
                Ok(()) => "Debug command sent to the host server.".into(),
                Err(error) => error,
            };
            continue;
        }
        match request {
            DebugRequest::Command { command } => {
                match authoritative.apply_local_debug_command(command.into()) {
                    Ok(()) => {
                        presentation
                            .publish(PresentationSnapshot::capture(&authoritative.simulation));
                        state.status = "Debug command applied.".into();
                    }
                    Err(error) => {
                        state.status = format!("Unable to apply debug command: {error:?}.")
                    }
                }
            }
            DebugRequest::SetPlayback { paused, speed } => {
                playback.paused = paused;
                state.speed = speed;
                apply_debug_speed(&mut fixed_time, speed);
                state.status = if paused {
                    "Simulation paused. Single-step is now available.".into()
                } else {
                    format!("Simulation speed set to {}.", speed.label())
                };
            }
            DebugRequest::StepOneTick if playback.paused => {
                state.status = match advance_authoritative_simulation_once(
                    &mut authoritative,
                    &mut presentation,
                ) {
                    Ok(_) => format!(
                        "Advanced one canonical tick. Current tick: {}.",
                        authoritative.simulation.tick()
                    ),
                    Err(error) => format!("Unable to advance canonical tick: {error:?}."),
                };
            }
            DebugRequest::StepOneTick => {
                state.status = "Pause the simulation before single-stepping.".into()
            }
        }
    }
}

fn apply_debug_speed(fixed_time: &mut Time<Fixed>, speed: DebugSpeed) {
    fixed_time.set_timestep_hz(f64::from(CASTLE_FIGHT_SIMULATION_HZ) * speed.multiplier());
}

fn update_debug_menu(
    authoritative: Res<AuthoritativeSimulation>,
    playback: Res<SimulationPlayback>,
    mut state: ResMut<DebugMenuState>,
    buttons: Query<(&DebugMenuButton, &Children)>,
    mut labels: Query<&mut Text, With<DebugMenuButtonLabel>>,
    mut status: Single<&mut Text, (With<DebugStatusText>, Without<DebugMenuButtonLabel>)>,
) {
    state.buildings_invulnerable = authoritative.simulation.debug_buildings_invulnerable();
    if let Some(status) = &authoritative.network_lobby_status {
        state.speed = status.debug_speed;
    }

    if !state.open {
        return;
    }

    for (button, children) in &buttons {
        let desired = match button.0 {
            DebugAction::TogglePause => Some(if playback.paused {
                "Resume simulation"
            } else {
                "Pause simulation"
            }),
            DebugAction::ToggleControlAllPlayers => Some(if state.control_all_players {
                "Control all players: ON"
            } else {
                "Control all players: OFF"
            }),
            DebugAction::ToggleBuildingsInvulnerable => Some(if state.buildings_invulnerable {
                "Buildings invulnerable: ON"
            } else {
                "Buildings invulnerable: OFF"
            }),
            _ => None,
        };
        let Some(desired) = desired else {
            continue;
        };
        if let Some(child) = children.first()
            && let Ok(mut label) = labels.get_mut(*child)
            && label.0 != desired
        {
            label.0 = desired.into();
        }
    }

    if status.0 != state.status {
        status.0.clone_from(&state.status);
    }
}

fn style_debug_buttons(
    playback: Res<SimulationPlayback>,
    state: Res<DebugMenuState>,
    mut buttons: Query<(
        &DebugMenuButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut labels: Query<&mut TextColor, With<DebugMenuButtonLabel>>,
) {
    if !state.open {
        return;
    }

    for (button, interaction, mut background, mut border, children) in &mut buttons {
        let disabled = button.0 == DebugAction::StepOneTick && !playback.paused;
        let selected = matches!(button.0, DebugAction::SetSpeed(speed) if speed == state.speed)
            || (button.0 == DebugAction::ToggleControlAllPlayers && state.control_all_players)
            || (button.0 == DebugAction::ToggleBuildingsInvulnerable
                && state.buildings_invulnerable);
        background.0 = if disabled {
            BUTTON_DISABLED
        } else if selected {
            BUTTON_SELECTED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        };
        *border = BorderColor::all(if selected {
            BORDER_SELECTED
        } else {
            BORDER_NORMAL
        });

        if let Some(child) = children.first()
            && let Ok(mut color) = labels.get_mut(*child)
        {
            color.0 = if disabled { TEXT_MUTED } else { TEXT_NORMAL };
        }
    }
}

pub(crate) fn cursor_over_debug_menu(cursor: Vec2, menu_open: bool) -> bool {
    menu_open
        && cursor.x >= PANEL_LEFT
        && cursor.x <= PANEL_LEFT + PANEL_WIDTH
        && cursor.y >= PANEL_TOP
        && cursor.y <= PANEL_TOP + PANEL_HEIGHT
}

impl DebugMenuState {
    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) const fn controls_all_players(&self) -> bool {
        self.control_all_players
    }

    pub(crate) fn can_control_builder(
        &self,
        simulation: &Simulation,
        local_player: PlayerId,
        builder: SimId,
    ) -> bool {
        self.control_all_players && simulation.builder(builder).is_some()
            || simulation.can_player_control_builder(local_player, builder)
    }

    pub(crate) fn can_control_building(
        &self,
        simulation: &Simulation,
        local_player: PlayerId,
        building: SimId,
    ) -> bool {
        self.control_all_players
            && simulation
                .building(building)
                .is_some_and(|building| building.owner.is_some())
            || simulation.can_player_control_building(local_player, building)
    }

    pub(crate) fn controller_for_actor(
        &self,
        simulation: &Simulation,
        local_player: PlayerId,
        actor: SimId,
    ) -> PlayerId {
        if !self.control_all_players {
            return local_player;
        }
        simulation
            .builder(actor)
            .map(|builder| builder.owner)
            .or_else(|| {
                simulation
                    .building(actor)
                    .and_then(|building| building.owner)
            })
            .unwrap_or(local_player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::{BuildingFootprint, Team};

    fn footprints_overlap(a: BuildingFootprint, b: BuildingFootprint) -> bool {
        a.min_x <= b.max_x() && b.min_x <= a.max_x() && a.min_y <= b.max_y() && b.min_y <= a.max_y()
    }

    fn footprint_contains(region: BuildingFootprint, footprint: BuildingFootprint) -> bool {
        footprint.min_x >= region.min_x
            && footprint.max_x() <= region.max_x()
            && footprint.min_y >= region.min_y
            && footprint.max_y() <= region.max_y()
    }

    #[test]
    fn guests_cannot_open_debug_menu_and_lost_host_access_closes_it() {
        let demo = crate::demo::create_demo_world(1, None);
        let client = crate::network::NetworkClient::connected_test_fixture(
            crate::compatibility_identity_for_demo(&demo),
        );
        let mut authoritative = AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            PlayerId(0),
            0,
        );
        authoritative.network_lobby_status = Some(castle_fight_protocol::LobbyStatus {
            host_player_id: 6,
            debug_paused: false,
            debug_speed: DebugSpeed::Normal,
            connected_player_ids: vec![0, 6],
            required_players: 2,
            participants: Vec::new(),
            started: true,
        });
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::F8);
        let mut app = App::new();
        app.insert_resource(authoritative)
            .insert_resource(keys)
            .init_resource::<DebugMenuState>()
            .add_systems(Update, toggle_debug_menu);
        let root = app
            .world_mut()
            .spawn((DebugMenuRoot, Visibility::Hidden))
            .id();
        app.update();
        assert!(!app.world().resource::<DebugMenuState>().open);
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        app.world_mut()
            .resource_mut::<AuthoritativeSimulation>()
            .network_lobby_status
            .as_mut()
            .unwrap()
            .host_player_id = 0;
        app.update();
        assert!(app.world().resource::<DebugMenuState>().open);
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Visible
        );
        app.world_mut()
            .resource_mut::<DebugMenuState>()
            .control_all_players = true;
        app.world_mut()
            .resource_mut::<AuthoritativeSimulation>()
            .network_lobby_status
            .as_mut()
            .unwrap()
            .host_player_id = 6;
        app.update();
        let state = app.world().resource::<DebugMenuState>();
        assert!(!state.open);
        assert!(!state.control_all_players);
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
    }

    fn assert_fixed_hz(fixed_time: &Time<Fixed>, expected_hz: f64) {
        assert_eq!(
            fixed_time.timestep(),
            std::time::Duration::from_secs_f64(1.0 / expected_hz),
        );
    }

    #[test]
    fn speed_presets_change_only_fixed_update_cadence() {
        let mut fixed_time = Time::<Fixed>::from_hz(f64::from(CASTLE_FIGHT_SIMULATION_HZ));
        apply_debug_speed(&mut fixed_time, DebugSpeed::Quarter);
        assert_fixed_hz(&fixed_time, 7.5);
        apply_debug_speed(&mut fixed_time, DebugSpeed::Quadruple);
        assert_fixed_hz(&fixed_time, 120.0);
        apply_debug_speed(&mut fixed_time, DebugSpeed::Normal);
        assert_fixed_hz(&fixed_time, 30.0);
    }

    #[test]
    fn closed_menu_never_blocks_world_cursor_and_open_menu_blocks_its_rect() {
        let inside = Vec2::new(PANEL_LEFT + 10.0, PANEL_TOP + 10.0);
        let outside = Vec2::new(PANEL_LEFT + PANEL_WIDTH + 1.0, PANEL_TOP + 10.0);
        assert!(!cursor_over_debug_menu(inside, false));
        assert!(cursor_over_debug_menu(inside, true));
        assert!(!cursor_over_debug_menu(outside, true));
    }

    #[test]
    fn control_all_players_uses_the_selected_actors_owner_for_commands() {
        let mut demo = crate::demo::create_demo_world(1, None);
        let (spawned, _) = castle_fight_sim::debug::populate_debug_building_lines(
            &mut demo.simulation,
            demo.content,
        );
        assert!(spawned > 0);
        let simulation = &demo.simulation;
        let other_builder = simulation.builder_for_team(Team(1)).unwrap();
        let other_building = simulation
            .buildings()
            .into_iter()
            .find(|building| building.owner == Some(other_builder.owner))
            .unwrap();
        let local = PlayerId(0);
        let normal = DebugMenuState::default();
        assert!(!normal.can_control_builder(simulation, local, other_builder.id));
        assert!(!normal.can_control_building(simulation, local, other_building.id));

        let debug = DebugMenuState {
            control_all_players: true,
            ..DebugMenuState::default()
        };
        assert!(debug.can_control_builder(simulation, local, other_builder.id));
        assert!(debug.can_control_building(simulation, local, other_building.id));
        assert_eq!(
            debug.controller_for_actor(simulation, local, other_builder.id),
            other_builder.owner
        );
        assert!(simulation.can_player_control_builder(other_builder.owner, other_builder.id));
        assert_eq!(
            debug.controller_for_actor(simulation, local, other_building.id),
            other_builder.owner
        );
        assert!(simulation.can_player_control_building(other_builder.owner, other_building.id));
    }

    #[test]
    fn debug_population_builds_complete_bounded_rosters_behind_both_castles() {
        let mut demo = crate::demo::create_demo_world(1, None);
        let content = demo.content;
        let implemented_rawcodes = content
            .production_building_definitions()
            .map(|definition| definition.rawcode)
            .chain(
                content
                    .tower_definitions()
                    .map(|definition| definition.rawcode),
            )
            .collect::<Vec<_>>();
        let expected_spawned = implemented_rawcodes.len() * 2;
        let initial_buildings = demo.simulation.building_count();

        let (spawned, skipped) =
            castle_fight_sim::debug::populate_debug_building_lines(&mut demo.simulation, content);

        assert_eq!(skipped, 0);
        assert_eq!(spawned, expected_spawned);
        assert_eq!(
            demo.simulation.building_count(),
            initial_buildings + expected_spawned
        );

        for team in [Team(0), Team(1)] {
            let castle = demo
                .simulation
                .team_objective(team)
                .and_then(|objective| demo.simulation.building(objective))
                .expect("development match must have both castles");
            let castle_center_x2 =
                castle.footprint.min_x * 2 + i32::from(castle.footprint.width) - 1;
            let mut line = demo
                .simulation
                .buildings()
                .into_iter()
                .filter(|building| building.team == team)
                .filter(|building| {
                    building
                        .content
                        .is_some_and(|content| implemented_rawcodes.contains(&content.rawcode))
                })
                .collect::<Vec<_>>();
            line.sort_unstable_by_key(|building| std::cmp::Reverse(building.footprint.min_y));
            assert_eq!(line.len(), implemented_rawcodes.len());

            for (index, building) in line.iter().enumerate() {
                assert!(
                    demo.simulation
                        .team_build_regions(team)
                        .iter()
                        .any(|region| footprint_contains(*region, building.footprint))
                );
                let center_x2 =
                    building.footprint.min_x * 2 + i32::from(building.footprint.width) - 1;
                assert!(if team == Team(0) {
                    center_x2 < castle_center_x2
                } else {
                    center_x2 > castle_center_x2
                });
                assert!(
                    line[index + 1..]
                        .iter()
                        .all(|other| !footprints_overlap(building.footprint, other.footprint))
                );
            }
            let mut actual_rawcodes = line
                .iter()
                .map(|building| building.content.unwrap().rawcode)
                .collect::<Vec<_>>();
            actual_rawcodes.sort_unstable();
            let mut expected = implemented_rawcodes.clone();
            expected.sort_unstable();
            assert_eq!(actual_rawcodes, expected);
        }
    }
}
