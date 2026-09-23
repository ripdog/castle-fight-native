use bevy::{prelude::*, time::Fixed};
use castle_fight_sim::{
    BuildingFootprint, CASTLE_FIGHT_SIMULATION_HZ, CastleFightContentBundle,
    CastleFightProductionKind, CastleFightTowerKind, PlayerId, SimId, Simulation, Team,
};

use crate::{
    AuthoritativeSimulation, SelectedMatch, SimulationPlayback,
    advance_authoritative_simulation_once,
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
const DEBUG_RESOURCE_GRANT: u32 = 1_000_000;
const DEBUG_KILL_DAMAGE: i32 = 9_999;
const DEBUG_BUILDING_LINE_MARGIN_CELLS: i32 = 4;
const DEBUG_BUILDING_LINE_GAP_CELLS: i32 = 2;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum DebugSpeed {
    Quarter,
    Half,
    #[default]
    Normal,
    Double,
    Quadruple,
}

impl DebugSpeed {
    const ALL: [Self; 5] = [
        Self::Quarter,
        Self::Half,
        Self::Normal,
        Self::Double,
        Self::Quadruple,
    ];

    const fn multiplier(self) -> f64 {
        match self {
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::Normal => 1.0,
            Self::Double => 2.0,
            Self::Quadruple => 4.0,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::Normal => "1x",
            Self::Double => "2x",
            Self::Quadruple => "4x",
        }
    }
}

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
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DebugMenuState>,
    mut visibility: Single<&mut Visibility, With<DebugMenuRoot>>,
) {
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
    selected_match: Res<SelectedMatch>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    if !state.open {
        return;
    }

    for (interaction, button) in &mut buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.0 {
            DebugAction::GrantResources => {
                let players = authoritative.simulation.players();
                for player in &players {
                    let granted = authoritative.simulation.debug_grant_player_resources_for(
                        player.id,
                        DEBUG_RESOURCE_GRANT,
                        DEBUG_RESOURCE_GRANT,
                    );
                    debug_assert!(granted, "listed debug player must have economy state");
                }
                presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
                state.status = format!(
                    "Granted all {} players +1,000,000 gold and +1,000,000 lumber.",
                    players.len()
                );
            }
            DebugAction::KillAllUnits => {
                let affected = authoritative
                    .simulation
                    .debug_damage_all_units(DEBUG_KILL_DAMAGE);
                presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
                state.status =
                    format!("Dealt {DEBUG_KILL_DAMAGE} damage to {affected} combat units.");
            }
            DebugAction::ToggleControlAllPlayers => {
                if authoritative.is_networked() {
                    state.status =
                        "Control-all-players is offline-only; the server owns network authority."
                            .into();
                    continue;
                }
                state.control_all_players = !state.control_all_players;
                state.status = if state.control_all_players {
                    "Debug control enabled for every player-owned builder and building.".into()
                } else {
                    "Debug control restored to normal player authority.".into()
                };
            }
            DebugAction::ToggleBuildingsInvulnerable => {
                if authoritative.is_networked() {
                    state.status =
                        "Building invulnerability is offline-only; the server owns network state."
                            .into();
                    continue;
                }
                state.buildings_invulnerable = !state.buildings_invulnerable;
                authoritative
                    .simulation
                    .debug_set_buildings_invulnerable(state.buildings_invulnerable);
                state.status = if state.buildings_invulnerable {
                    "All buildings are targetable but take zero damage.".into()
                } else {
                    "Building damage restored to normal.".into()
                };
            }
            DebugAction::PopulateBuildings => {
                if authoritative.is_networked() {
                    state.status =
                        "Building population is offline-only; the server owns network state."
                            .into();
                    continue;
                }
                let (spawned, skipped) = populate_debug_building_lines(
                    &mut authoritative.simulation,
                    selected_match.content,
                );
                presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
                state.status = if skipped == 0 {
                    format!("Spawned {spawned} completed buildings in top-to-bottom debug lines.")
                } else {
                    format!("Spawned {spawned} completed buildings; {skipped} could not be placed.")
                };
            }
            DebugAction::TogglePause => {
                playback.paused = !playback.paused;
                state.status = if playback.paused {
                    "Simulation paused. Single-step is now available.".into()
                } else {
                    "Simulation resumed.".into()
                };
            }
            DebugAction::StepOneTick => {
                if playback.paused {
                    match advance_authoritative_simulation_once(
                        &mut authoritative,
                        &mut presentation,
                    ) {
                        Ok(_) => {
                            state.status = format!(
                                "Advanced one canonical tick. Current tick: {}.",
                                authoritative.simulation.tick()
                            );
                        }
                        Err(error) => {
                            state.status = format!("Unable to advance canonical tick: {error:?}.");
                        }
                    }
                } else {
                    state.status = "Pause the simulation before single-stepping.".into();
                }
            }
            DebugAction::SetSpeed(speed) => {
                state.speed = speed;
                apply_debug_speed(&mut fixed_time, speed);
                state.status = format!("Simulation speed set to {}.", speed.label());
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DebugBuildingKind {
    Production(CastleFightProductionKind),
    Tower(CastleFightTowerKind),
}

impl DebugBuildingKind {
    fn footprint_size(self, content: &CastleFightContentBundle) -> u16 {
        match self {
            Self::Production(kind) => {
                content
                    .production_building(kind)
                    .expect("debug production kind must belong to selected content")
                    .footprint_size_cells
            }
            Self::Tower(kind) => {
                content
                    .tower(kind)
                    .expect("debug tower kind must belong to selected content")
                    .footprint_size_cells
            }
        }
    }
}

fn populate_debug_building_lines(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
) -> (usize, usize) {
    let definitions = CastleFightProductionKind::ALL
        .into_iter()
        .filter(|kind| content.production_building(*kind).is_some())
        .map(DebugBuildingKind::Production)
        .chain(
            CastleFightTowerKind::ALL
                .into_iter()
                .filter(|kind| content.tower(*kind).is_some())
                .map(DebugBuildingKind::Tower),
        )
        .collect::<Vec<_>>();
    if definitions.is_empty() {
        return (0, 0);
    }

    let max_size = definitions
        .iter()
        .map(|definition| definition.footprint_size(content))
        .max()
        .expect("non-empty debug building list");
    let max_size_i32 = i32::from(max_size);
    let mut spawned = 0;
    let mut skipped = 0;

    for team in [Team(0), Team(1)] {
        let Some(castle) = simulation
            .team_objective(team)
            .and_then(|objective| simulation.building(objective))
        else {
            skipped += definitions.len();
            continue;
        };
        let owner = castle.owner.or_else(|| {
            simulation
                .players()
                .into_iter()
                .filter(|player| player.team == team)
                .map(|player| player.id)
                .min()
        });
        let Some(owner) = owner else {
            skipped += definitions.len();
            continue;
        };

        let region = simulation
            .team_build_regions(team)
            .iter()
            .copied()
            .find(|region| footprint_contains(*region, castle.footprint))
            .or_else(|| {
                simulation
                    .team_build_regions(team)
                    .iter()
                    .copied()
                    .max_by_key(|region| u32::from(region.width) * u32::from(region.height))
            });
        let Some(region) = region else {
            skipped += definitions.len();
            continue;
        };

        let castle_center_x2 = castle.footprint.min_x * 2 + i32::from(castle.footprint.width) - 1;
        let region_center_x2 = region.min_x * 2 + i32::from(region.width) - 1;
        let outer_side_is_left = castle_center_x2 < region_center_x2;
        let line_min_x = if outer_side_is_left {
            region.min_x + DEBUG_BUILDING_LINE_MARGIN_CELLS
        } else {
            region.max_x() - DEBUG_BUILDING_LINE_MARGIN_CELLS - max_size_i32 + 1
        };
        let minimum_y = region.min_y + DEBUG_BUILDING_LINE_MARGIN_CELLS;
        let mut cursor_max_y = region.max_y() - DEBUG_BUILDING_LINE_MARGIN_CELLS;

        for definition in &definitions {
            let definition = *definition;
            let size = definition.footprint_size(content);
            let size_i32 = i32::from(size);
            let min_x = line_min_x + (max_size_i32 - size_i32) / 2;
            let mut candidate_max_y = cursor_max_y;
            let mut placed = false;

            while candidate_max_y - size_i32 + 1 >= minimum_y {
                let min_y = candidate_max_y - size_i32 + 1;
                let footprint = BuildingFootprint::new(min_x, min_y, size, size);
                if simulation.can_place_building_for_team(team, footprint) {
                    match definition {
                        DebugBuildingKind::Production(kind) => {
                            let definition = content
                                .production_building(kind)
                                .expect("debug production kind must belong to selected content");
                            simulation.spawn_building_for_player_with_properties(
                                owner,
                                definition.spawn(team, footprint),
                                definition.gameplay_properties(),
                            );
                        }
                        DebugBuildingKind::Tower(kind) => {
                            let definition = content
                                .tower(kind)
                                .expect("debug tower kind must belong to selected content");
                            simulation.spawn_building_for_player_with_properties(
                                owner,
                                definition.spawn(team, footprint),
                                definition.gameplay_properties(),
                            );
                        }
                    }
                    cursor_max_y = min_y - DEBUG_BUILDING_LINE_GAP_CELLS - 1;
                    spawned += 1;
                    placed = true;
                    break;
                }
                candidate_max_y -= 1;
            }

            if !placed {
                skipped += 1;
            }
        }
    }

    (spawned, skipped)
}

const fn footprint_contains(region: BuildingFootprint, footprint: BuildingFootprint) -> bool {
    footprint.min_x >= region.min_x
        && footprint.max_x() <= region.max_x()
        && footprint.min_y >= region.min_y
        && footprint.max_y() <= region.max_y()
}

fn apply_debug_speed(fixed_time: &mut Time<Fixed>, speed: DebugSpeed) {
    fixed_time.set_timestep_hz(f64::from(CASTLE_FIGHT_SIMULATION_HZ) * speed.multiplier());
}

fn update_debug_menu(
    playback: Res<SimulationPlayback>,
    state: Res<DebugMenuState>,
    buttons: Query<(&DebugMenuButton, &Children)>,
    mut labels: Query<&mut Text, With<DebugMenuButtonLabel>>,
    mut status: Single<&mut Text, (With<DebugStatusText>, Without<DebugMenuButtonLabel>)>,
) {
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

    pub(crate) const fn buildings_invulnerable(&self) -> bool {
        self.buildings_invulnerable
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
        let (spawned, _) = populate_debug_building_lines(&mut demo.simulation, demo.content);
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
    fn debug_population_builds_complete_vertical_rosters_behind_both_castles() {
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

        let (spawned, skipped) = populate_debug_building_lines(&mut demo.simulation, content);

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

            let line_center_x2 =
                line[0].footprint.min_x * 2 + i32::from(line[0].footprint.width) - 1;
            assert!(line.iter().all(|building| {
                let center_x2 =
                    building.footprint.min_x * 2 + i32::from(building.footprint.width) - 1;
                (center_x2 - line_center_x2).abs() <= 1
            }));
            assert!(
                line.windows(2)
                    .all(|pair| { pair[0].footprint.min_y > pair[1].footprint.min_y })
            );
            if team == Team(0) {
                assert!(line_center_x2 < castle_center_x2);
            } else {
                assert!(line_center_x2 > castle_center_x2);
            }
        }
    }
}
