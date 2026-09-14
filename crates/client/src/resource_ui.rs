use bevy::prelude::*;

use crate::{
    bridge::PresentationSamples, build_ui::BuildSelection, inspection::InspectionSelection,
};

pub(crate) const TOP_BAR_HEIGHT: f32 = 58.0;

const BAR_BACKGROUND: Color = Color::srgba(0.055, 0.048, 0.036, 0.98);
const BAR_BORDER: Color = Color::srgb(0.33, 0.27, 0.16);
const SLOT_BACKGROUND: Color = Color::srgba(0.025, 0.023, 0.020, 0.95);
const SLOT_BORDER: Color = Color::srgb(0.24, 0.21, 0.15);
const LABEL_COLOR: Color = Color::srgb(0.67, 0.63, 0.52);
const GOLD_COLOR: Color = Color::srgb(0.96, 0.78, 0.16);
const LUMBER_COLOR: Color = Color::srgb(0.26, 0.78, 0.34);
const LEGENDARY_COLOR: Color = Color::srgb(0.72, 0.56, 0.96);
const PROGRESS_TRACK: Color = Color::srgb(0.11, 0.09, 0.055);

#[derive(Component)]
struct SelectedPlayerText;

#[derive(Component)]
struct GoldText;

#[derive(Component)]
struct GoldIncomeText;

#[derive(Component)]
struct GoldIncomeProgress;

#[derive(Component)]
struct LumberText;

#[derive(Component)]
struct LegendaryText;

pub(crate) struct ResourceUiPlugin;

impl Plugin for ResourceUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_resource_bar)
            .add_systems(Update, update_resource_bar);
    }
}

fn setup_resource_bar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                right: px(0.0),
                top: px(0.0),
                height: px(TOP_BAR_HEIGHT),
                padding: UiRect::horizontal(px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(8.0),
                border: UiRect::bottom(px(2.0)),
                ..default()
            },
            BackgroundColor(BAR_BACKGROUND),
            BorderColor::all(BAR_BORDER),
            ZIndex(1000),
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    width: px(124.0),
                    height: px(42.0),
                    padding: UiRect::horizontal(px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(SLOT_BACKGROUND),
                BorderColor::all(SLOT_BORDER),
            ))
            .with_child((
                Text::new("BLUE PLAYER"),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.35, 0.67, 1.0)),
                SelectedPlayerText,
            ));

            spawn_resource_slot(bar, "G", "GOLD", GOLD_COLOR, GoldText, true, true);
            spawn_resource_slot(bar, "W", "LUMBER", LUMBER_COLOR, LumberText, false, false);
            spawn_resource_slot(
                bar,
                "L",
                "LEGENDARY",
                LEGENDARY_COLOR,
                LegendaryText,
                false,
                false,
            );
        });
}

fn spawn_resource_slot<M: Component>(
    parent: &mut ChildSpawnerCommands,
    icon: &'static str,
    label: &'static str,
    accent: Color,
    value_marker: M,
    show_income: bool,
    has_progress: bool,
) {
    parent
        .spawn((
            Node {
                width: px(154.0),
                height: px(42.0),
                padding: UiRect::axes(px(6.0), px(4.0)),
                align_items: AlignItems::Center,
                column_gap: px(7.0),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(SLOT_BACKGROUND),
            BorderColor::all(SLOT_BORDER),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(29.0),
                    height: px(29.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.18)),
                BorderColor::all(accent.with_alpha(0.72)),
            ))
            .with_child((
                Text::new(icon),
                TextFont::from_font_size(18.0),
                TextColor(accent),
            ));

            slot.spawn((Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                flex_grow: 1.0,
                row_gap: px(1.0),
                ..default()
            },))
                .with_children(|column| {
                    column.spawn((
                        Text::new(label),
                        TextFont::from_font_size(9.0),
                        TextColor(LABEL_COLOR),
                    ));
                    column.spawn((
                        Text::new("0"),
                        TextFont::from_font_size(17.0),
                        TextColor(Color::WHITE),
                        value_marker,
                    ));
                    if has_progress {
                        column
                            .spawn((
                                Node {
                                    width: percent(100.0),
                                    height: px(4.0),
                                    ..default()
                                },
                                BackgroundColor(PROGRESS_TRACK),
                            ))
                            .with_child((
                                Node {
                                    width: percent(0.0),
                                    height: percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(GOLD_COLOR),
                                GoldIncomeProgress,
                            ));
                    }
                    if show_income {
                        column.spawn((
                            Text::new("income +0"),
                            TextFont::from_font_size(8.0),
                            TextColor(GOLD_COLOR.with_alpha(0.82)),
                            GoldIncomeText,
                        ));
                    }
                });
        });
}

fn update_resource_bar(
    selection: Res<BuildSelection>,
    inspection: Res<InspectionSelection>,
    presentation: Res<PresentationSamples>,
    mut player_text: Single<(&mut Text, &mut TextColor), With<SelectedPlayerText>>,
    mut gold_text: Single<&mut Text, (With<GoldText>, Without<SelectedPlayerText>)>,
    mut gold_income_text: Single<&mut Text, (With<GoldIncomeText>, Without<GoldText>)>,
    mut progress: Single<&mut Node, With<GoldIncomeProgress>>,
    mut lumber_text: Single<&mut Text, (With<LumberText>, Without<GoldText>)>,
    mut legendary_text: Single<&mut Text, (With<LegendaryText>, Without<LumberText>)>,
) {
    let selected_team = inspection.selected.and_then(|selected| {
        presentation
            .current
            .builders
            .get(&selected)
            .map(|builder| builder.team)
            .or_else(|| {
                presentation
                    .current
                    .units
                    .get(&selected)
                    .map(|unit| unit.team)
            })
            .or_else(|| {
                presentation
                    .current
                    .buildings
                    .get(&selected)
                    .map(|building| building.team)
            })
    });
    let team_index = usize::from(selected_team.unwrap_or(selection.team).0.min(1));
    let economy = presentation.current.player_economy[team_index];
    let resources = economy.resources;

    gold_text.0 = resources.gold.to_string();
    gold_income_text.0 = format!("income +{}", economy.income);
    progress.width = percent(f32::from(economy.income_progress_per_10k) / 100.0);
    lumber_text.0 = resources.lumber.to_string();
    legendary_text.0 = format!(
        "{} / {}",
        resources.legendary_points_used, resources.legendary_points_cap
    );

    let (label, color) = if team_index == 0 {
        ("BLUE PLAYER", Color::srgb(0.35, 0.67, 1.0))
    } else {
        ("RED PLAYER", Color::srgb(1.0, 0.38, 0.31))
    };
    player_text.0.0 = label.into();
    player_text.1.0 = color;
}
