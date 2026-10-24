//! WC3-inspired live match scoreboard and authoritative round-review statistics.
use bevy::{app::AppExit, ecs::system::SystemParam, prelude::*};
use castle_fight_sim::{MatchLifecycle, MatchOutcome, PlayerId, PlayerRoundScore, Team};

use crate::{
    AuthoritativeSimulation, SelectedMatch, debug_menu::DebugMenuState,
    game_menu::LeaveMatchRequest, lobby::LobbyState, main_menu::MainMenuState,
};

const PANEL: Color = Color::srgba(0.038, 0.040, 0.055, 0.97);
const GOLD: Color = Color::srgb(0.99, 0.84, 0.46);
const MUTED: Color = Color::srgb(0.65, 0.72, 0.80);
const WEST: Color = Color::srgb(0.50, 0.74, 1.0);
const EAST: Color = Color::srgb(1.0, 0.59, 0.53);
const VICTORY: Color = Color::srgb(0.99, 0.84, 0.46);
const DEFEAT: Color = Color::srgb(0.95, 0.38, 0.33);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const BUTTON_BORDER: Color = Color::srgb(0.32, 0.43, 0.52);

pub(crate) struct ScoreboardPlugin;

#[derive(Resource, Default)]
struct ScoreboardState {
    expanded: bool,
    /// The player chose to look at the battlefield instead of the final result.
    result_dismissed: bool,
}
/// Headline of the finished-match result (Victory / Defeat / Draw).
#[derive(Component)]
struct ScoreboardBanner;
/// Row of actions offered once the match has finished.
#[derive(Component)]
struct ScoreboardResultActions;
#[derive(Component, Clone, Copy)]
enum ResultAction {
    ViewBattlefield,
    MainMenu,
    ExitGame,
}
#[derive(Component)]
struct ScoreboardRoot;
#[derive(Component)]
struct ScoreboardTitle;
#[derive(Component)]
struct ScoreboardSubtitle;
#[derive(Component)]
struct ScoreboardFooter;
#[derive(Component)]
struct TeamHeader(Team);
#[derive(Component)]
struct PlayerRow(PlayerId);
#[derive(Component)]
struct PlayerCell {
    player: PlayerId,
    column: ScoreColumn,
}
#[derive(Clone, Copy)]
enum ScoreColumn {
    Player,
    Gold,
    Lumber,
    Income,
    Units,
    Buildings,
    Strikes,
}

impl Plugin for ScoreboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScoreboardState>()
            .add_message::<crate::game_menu::MatchSessionReset>()
            .add_message::<LeaveMatchRequest>()
            .add_systems(
                Update,
                reset_scoreboard_for_new_match.before(toggle_scoreboard),
            )
            .add_systems(Startup, setup_scoreboard)
            .add_systems(
                Update,
                (toggle_scoreboard, handle_result_buttons, update_scoreboard).chain(),
            );
    }
}

fn reset_scoreboard_for_new_match(
    mut resets: MessageReader<crate::game_menu::MatchSessionReset>,
    mut state: ResMut<ScoreboardState>,
) {
    if resets.read().count() > 0 {
        *state = ScoreboardState::default();
    }
}

fn scoreboard_text(
    parent: &mut ChildSpawnerCommands,
    text: impl Into<String>,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(text),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn spawn_cell(parent: &mut ChildSpawnerCommands, label: &str, width: f32, size: f32, color: Color) {
    parent
        .spawn(Node {
            width: Val::Percent(width),
            ..default()
        })
        .with_children(|cell| {
            scoreboard_text(cell, label, size, color);
        });
}

fn setup_scoreboard(mut commands: Commands, main_menu: Option<Res<MainMenuState>>) {
    commands
        .spawn((
            ScoreboardRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(16.0),
                left: Val::Percent(13.0),
                width: Val::Percent(74.0),
                max_width: px(1120.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.0),
                padding: UiRect::axes(px(26.0), px(23.0)),
                border: UiRect::all(px(2.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(Color::srgb(0.59, 0.53, 0.39)),
            Visibility::Hidden,
            ZIndex(35),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent.spawn((
                ScoreboardBanner,
                Node {
                    display: Display::None,
                    ..default()
                },
                Text::new(""),
                TextFont::from_font_size(44.0),
                TextColor(VICTORY),
            ));
            parent.spawn((
                ScoreboardTitle,
                Text::new("MATCH SCOREBOARD"),
                TextFont::from_font_size(28.0),
                TextColor(GOLD),
            ));
            parent.spawn((
                ScoreboardSubtitle,
                Text::new(""),
                TextFont::from_font_size(17.0),
                TextColor(MUTED),
            ));
            let columns = [
                ("PLAYER", 24.0),
                ("GOLD", 12.0),
                ("LUMBER", 13.0),
                ("INCOME", 14.0),
                ("UNITS", 12.0),
                ("BUILDS", 13.0),
                ("RS", 12.0),
            ];
            parent
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|row| {
                    for (name, width) in columns {
                        spawn_cell(row, name, width, 13.0, MUTED);
                    }
                });
            for team in [Team(0), Team(1)] {
                parent.spawn((
                    TeamHeader(team),
                    Text::new(""),
                    TextFont::from_font_size(19.0),
                    TextColor(if team == Team(0) { WEST } else { EAST }),
                ));
                for slot in 0..6u8 {
                    let player = PlayerId(team.0 * 6 + slot);
                    parent
                        .spawn((
                            PlayerRow(player),
                            Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                padding: UiRect::axes(px(4.0), px(4.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.18, 0.23, 0.30, 0.30)),
                            Visibility::Hidden,
                        ))
                        .with_children(|row| {
                            for (column, width) in [
                                (ScoreColumn::Player, 24.0),
                                (ScoreColumn::Gold, 12.0),
                                (ScoreColumn::Lumber, 13.0),
                                (ScoreColumn::Income, 14.0),
                                (ScoreColumn::Units, 12.0),
                                (ScoreColumn::Buildings, 13.0),
                                (ScoreColumn::Strikes, 12.0),
                            ] {
                                row.spawn(Node {
                                    width: Val::Percent(width),
                                    ..default()
                                })
                                .with_children(|cell| {
                                    cell.spawn((
                                        PlayerCell { player, column },
                                        Text::new(""),
                                        TextFont::from_font_size(16.0),
                                        TextColor(Color::srgb(0.91, 0.93, 0.96)),
                                    ));
                                });
                            }
                        });
                }
            }
            parent
                .spawn((
                    ScoreboardResultActions,
                    Node {
                        display: Display::None,
                        flex_direction: FlexDirection::Row,
                        column_gap: px(10.0),
                        ..default()
                    },
                ))
                .with_children(|actions| {
                    result_button(actions, "VIEW BATTLEFIELD", ResultAction::ViewBattlefield);
                    if main_menu.is_some() {
                        result_button(actions, "MAIN MENU", ResultAction::MainMenu);
                    }
                    result_button(actions, "EXIT GAME", ResultAction::ExitGame);
                });
            parent.spawn((
                ScoreboardFooter,
                Text::new("F9: close scoreboard"),
                TextFont::from_font_size(13.0),
                TextColor(MUTED),
            ));
        });
}

fn result_button(parent: &mut ChildSpawnerCommands, value: &str, action: ResultAction) {
    parent
        .spawn((
            Button,
            action,
            Node {
                flex_grow: 1.0,
                min_height: px(38.0),
                padding: UiRect::axes(px(14.0), px(7.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(BUTTON),
            BorderColor::all(BUTTON_BORDER),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(16.0),
            TextColor(Color::srgb(0.92, 0.94, 0.97)),
        ));
}

type ResultButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static ResultAction,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

fn handle_result_buttons(
    mut buttons: ResultButtonQuery<'_, '_>,
    mut state: ResMut<ScoreboardState>,
    mut leave: MessageWriter<LeaveMatchRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action, mut background) in &mut buttons {
        match interaction {
            Interaction::Hovered => background.0 = BUTTON_HOVER,
            Interaction::None => background.0 = BUTTON,
            Interaction::Pressed => match action {
                ResultAction::ViewBattlefield => state.result_dismissed = true,
                ResultAction::MainMenu => {
                    leave.write(LeaveMatchRequest);
                }
                ResultAction::ExitGame => {
                    exit.write(AppExit::Success);
                }
            },
        }
    }
}

/// What the finished match means to the viewer. A single-player session controls both teams, so
/// it can only name the winner; otherwise the result is relative to the local player's team.
fn result_banner(
    outcome: MatchOutcome,
    local_team: Option<Team>,
    controls_all_players: bool,
) -> (&'static str, Color) {
    match outcome {
        MatchOutcome::Draw => ("DRAW", GOLD),
        MatchOutcome::Victory(winner) if controls_all_players || local_team.is_none() => {
            match winner {
                Team(0) => ("WEST WINS", WEST),
                Team(1) => ("EAST WINS", EAST),
                Team(_) => ("MATCH WON", VICTORY),
            }
        }
        MatchOutcome::Victory(winner) if Some(winner) == local_team => ("VICTORY", VICTORY),
        MatchOutcome::Victory(_) => ("DEFEAT", DEFEAT),
    }
}

fn toggle_scoreboard(
    keys: Res<ButtonInput<KeyCode>>,
    lobby: Option<Res<LobbyState>>,
    authoritative: Res<AuthoritativeSimulation>,
    mut state: ResMut<ScoreboardState>,
) {
    if keys.just_pressed(KeyCode::F9) && !lobby.as_ref().is_some_and(|lobby| lobby.active()) {
        if matches!(
            authoritative.simulation.lifecycle(),
            MatchLifecycle::Finished { .. }
        ) {
            // The final result stays reachable after choosing to look at the battlefield.
            state.result_dismissed = !state.result_dismissed;
        } else {
            state.expanded = !state.expanded;
        }
    }
}

fn summary(outcome: MatchOutcome) -> &'static str {
    match outcome {
        MatchOutcome::Victory(Team(0)) => "West wins the round",
        MatchOutcome::Victory(Team(1)) => "East wins the round",
        MatchOutcome::Victory(_) => "Round won",
        MatchOutcome::Draw => "Round drawn",
    }
}

fn score_cell(score: &PlayerRoundScore, column: ScoreColumn) -> String {
    match column {
        ScoreColumn::Player => format!("Player {}", score.player.0 + 1),
        ScoreColumn::Gold => score.gold.to_string(),
        ScoreColumn::Lumber => score.lumber.to_string(),
        ScoreColumn::Income => score.income.to_string(),
        ScoreColumn::Units => score.units.to_string(),
        ScoreColumn::Buildings => score.buildings.to_string(),
        ScoreColumn::Strikes => score.rescue_strikes.to_string(),
    }
}

type ScoreboardTextParts = (
    Option<&'static ScoreboardTitle>,
    Option<&'static ScoreboardSubtitle>,
    Option<&'static ScoreboardFooter>,
    Option<&'static TeamHeader>,
    Option<&'static PlayerCell>,
    &'static mut Text,
);

type ScoreboardBannerParts = (&'static mut Text, &'static mut TextColor, &'static mut Node);
type BannerFilter = (With<ScoreboardBanner>, Without<ScoreboardResultActions>);

#[derive(SystemParam)]
struct ScoreboardQueries<'w, 's> {
    root: Query<'w, 's, &'static mut Visibility, With<ScoreboardRoot>>,
    rows: Query<'w, 's, (&'static PlayerRow, &'static mut Visibility), Without<ScoreboardRoot>>,
    text: Query<'w, 's, ScoreboardTextParts, Without<ScoreboardBanner>>,
    banner: Query<'w, 's, ScoreboardBannerParts, BannerFilter>,
    actions: Query<
        'w,
        's,
        &'static mut Node,
        (With<ScoreboardResultActions>, Without<ScoreboardBanner>),
    >,
}

fn update_scoreboard(
    state: Res<ScoreboardState>,
    lobby: Option<Res<LobbyState>>,
    authoritative: Res<AuthoritativeSimulation>,
    selected: Res<SelectedMatch>,
    debug_menu: Res<DebugMenuState>,
    mut ui: ScoreboardQueries<'_, '_>,
) {
    let sim = &authoritative.simulation;
    let in_review = sim.review_seconds_remaining().is_some();
    let finished = matches!(sim.lifecycle(), MatchLifecycle::Finished { .. });
    let visible = !lobby.as_ref().is_some_and(|lobby| lobby.active())
        && (in_review
            || if finished {
                !state.result_dismissed
            } else {
                state.expanded
            });
    if let Ok(mut root) = ui.root.single_mut() {
        *root = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !visible {
        return;
    }
    if let Ok((mut text, mut color, mut node)) = ui.banner.single_mut() {
        if let MatchLifecycle::Finished { outcome, .. } = sim.lifecycle() {
            let local_team = sim.player(selected.local_player).map(|player| player.team);
            let (headline, headline_color) =
                result_banner(outcome, local_team, debug_menu.controls_all_players());
            text.0 = headline.into();
            color.0 = headline_color;
            node.display = Display::Flex;
        } else {
            node.display = Display::None;
        }
    }
    if let Ok(mut node) = ui.actions.single_mut() {
        node.display = if finished {
            Display::Flex
        } else {
            Display::None
        };
    }

    let round = sim.last_round_scoreboard();
    let historical = in_review || finished;
    let scores = if historical {
        round.map_or_else(|| sim.current_round_scores(), |r| r.players.clone())
    } else {
        sim.current_round_scores()
    };
    let wins = sim.round_wins();
    for (row, mut visibility) in &mut ui.rows {
        *visibility = if scores.iter().any(|score| score.player == row.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (title, subtitle, footer, header, cell, mut text) in &mut ui.text {
        text.0 = if title.is_some() {
            if finished {
                "MATCH RESULT".into()
            } else if in_review {
                format!(
                    "ROUND {} COMPLETE",
                    round.map_or(sim.round_index(), |r| r.round)
                )
            } else {
                "MATCH SCOREBOARD".into()
            }
        } else if subtitle.is_some() {
            if finished {
                format!(
                    "{} · Final score: West {} – {} East",
                    match sim.lifecycle() {
                        MatchLifecycle::Finished {
                            outcome: MatchOutcome::Victory(Team(0)),
                            ..
                        } => "West wins the match",
                        MatchLifecycle::Finished {
                            outcome: MatchOutcome::Victory(Team(1)),
                            ..
                        } => "East wins the match",
                        MatchLifecycle::Finished {
                            outcome: MatchOutcome::Victory(_),
                            ..
                        } => "Match won",
                        MatchLifecycle::Finished {
                            outcome: MatchOutcome::Draw,
                            ..
                        } => "Match drawn",
                        _ => "Match ended",
                    },
                    wins[0],
                    wins[1]
                )
            } else if let Some(seconds) = sim.review_seconds_remaining() {
                format!(
                    "{} · Next round begins in {seconds} seconds",
                    round.map_or("Round complete", |r| summary(r.outcome))
                )
            } else {
                format!(
                    "Round {} · First to {} wins · F9 to hide",
                    sim.round_index(),
                    sim.match_rules().rounds_to_win
                )
            }
        } else if footer.is_some() {
            if in_review {
                "Game paused during the review. Orders resume when the countdown ends.".into()
            } else if finished {
                "F9 shows or hides this result.".into()
            } else {
                "F9: close scoreboard".into()
            }
        } else if let Some(header) = header {
            let team_scores = scores.iter().filter(|score| score.team == header.0);
            let income: u64 = team_scores
                .clone()
                .map(|score| u64::from(score.income))
                .sum();
            let strikes: u32 = team_scores.map(|score| score.rescue_strikes).sum();
            let name = if header.0 == Team(0) { "WEST" } else { "EAST" };
            format!(
                "{name}  ·  {} round wins  ·  {income} income  ·  {strikes} rescue strikes",
                wins[usize::from(header.0.0)]
            )
        } else if let Some(cell) = cell {
            scores
                .iter()
                .find(|score| score.player == cell.player)
                .map_or_else(String::new, |score| score_cell(score, cell.column))
        } else {
            continue;
        };
    }
}
