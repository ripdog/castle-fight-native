use bevy::{prelude::*, time::Fixed};
use castle_fight_sim::{CASTLE_FIGHT_SIMULATION_HZ, Team};

use crate::{
    AuthoritativeSimulation, SimulationPlayback, advance_authoritative_simulation_once,
    bridge::{PresentationSamples, PresentationSnapshot},
    resource_ui::TOP_BAR_HEIGHT,
};

const PANEL_LEFT: f32 = 12.0;
const PANEL_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const PANEL_WIDTH: f32 = 360.0;
const PANEL_HEIGHT: f32 = 364.0;
const PANEL_PADDING: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 38.0;
const BUTTON_GAP: f32 = 6.0;
const DEBUG_RESOURCE_GRANT: u32 = 1_000_000;
const DEBUG_KILL_DAMAGE: i32 = 9_999;

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
    status: String,
}

impl Default for DebugMenuState {
    fn default() -> Self {
        Self {
            open: false,
            speed: DebugSpeed::Normal,
            status: "F8 closes this menu.".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DebugAction {
    GrantResources,
    KillAllUnits,
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
                for team in [Team(0), Team(1)] {
                    let granted = authoritative.simulation.debug_grant_player_resources(
                        team,
                        DEBUG_RESOURCE_GRANT,
                        DEBUG_RESOURCE_GRANT,
                    );
                    debug_assert!(granted, "the debug client expects two valid player teams");
                }
                presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
                state.status = "Granted every player +1,000,000 gold and +1,000,000 lumber.".into();
            }
            DebugAction::KillAllUnits => {
                let affected = authoritative
                    .simulation
                    .debug_damage_all_units(DEBUG_KILL_DAMAGE);
                presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
                state.status =
                    format!("Dealt {DEBUG_KILL_DAMAGE} damage to {affected} combat units.");
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
                    advance_authoritative_simulation_once(&mut authoritative, &mut presentation);
                    state.status = format!(
                        "Advanced one tick. Current tick: {}.",
                        authoritative.simulation.tick()
                    );
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
        if button.0 != DebugAction::TogglePause {
            continue;
        }
        if let Some(child) = children.first()
            && let Ok(mut label) = labels.get_mut(*child)
        {
            let desired = if playback.paused {
                "Resume simulation"
            } else {
                "Pause simulation"
            };
            if label.0 != desired {
                label.0 = desired.into();
            }
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
        let selected = matches!(button.0, DebugAction::SetSpeed(speed) if speed == state.speed);
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
}
