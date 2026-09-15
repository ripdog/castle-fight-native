use bevy::{prelude::*, window::PrimaryWindow};
use castle_fight_sim::{
    BuildingConstructionCancelOutcome, BuildingFootprint, CommandCardPosition, SimId, Team,
    castle_fight_command_card_layout,
};

use crate::{
    AuthoritativeSimulation,
    bridge::{BuildingSample, BuildingVisualKind, PresentationSamples, PresentationSnapshot},
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    demo::{BuildKind, ProductionKind, order_demo_production_upgrade},
    inspection::InspectionSelection,
    presentation::{WorldMetrics, draw_footprint_outline, viewport_ground_point},
    resource_ui::TOP_BAR_HEIGHT,
    terrain::TerrainSurface,
    ui_icons::{CastleFightPresentationCatalog, UiIconAssets, UiIconKey},
    wc3_text::{Wc3Color, parse_wc3_text},
};

const PANEL_LEFT: f32 = 12.0;
const PANEL_BOTTOM: f32 = 12.0;
const PANEL_PADDING: f32 = 8.0;
const GRID_GAP: f32 = 4.0;
const CELL_SIZE: f32 = 66.0;
const GRID_COLUMNS: usize = 4;
const GRID_ROWS: usize = 3;
const SLOT_COUNT: usize = GRID_COLUMNS * GRID_ROWS;
const PANEL_WIDTH: f32 =
    PANEL_PADDING * 2.0 + CELL_SIZE * GRID_COLUMNS as f32 + GRID_GAP * (GRID_COLUMNS as f32 - 1.0);
const PANEL_HEIGHT: f32 =
    PANEL_PADDING * 2.0 + CELL_SIZE * GRID_ROWS as f32 + GRID_GAP * (GRID_ROWS as f32 - 1.0);

const PANEL_BACKGROUND: Color = Color::srgba(0.105, 0.070, 0.040, 0.97);
const PANEL_BORDER: Color = Color::srgb(0.28, 0.25, 0.20);
const SLOT_BORDER: Color = Color::srgb(0.34, 0.34, 0.32);
const AUTOCAST_BORDER: Color = Color::srgb(0.78, 0.70, 0.24);
const BUTTON_NORMAL: Color = Color::srgb(0.095, 0.075, 0.055);
const BUTTON_HOVERED: Color = Color::srgb(0.18, 0.14, 0.095);
const BUTTON_DISABLED: Color = Color::srgb(0.045, 0.043, 0.040);
const BUTTON_DISABLED_BORDER: Color = Color::srgb(0.18, 0.17, 0.16);
const BUTTON_TEXT: Color = Color::srgb(0.92, 0.90, 0.84);
const BUTTON_TEXT_DISABLED: Color = Color::srgb(0.42, 0.40, 0.37);
const BUTTON_ICON_DISABLED: Color = Color::srgb(0.38, 0.38, 0.38);
const BUTTON_LABEL_BACKGROUND: Color = Color::srgba(0.02, 0.015, 0.01, 0.72);
const TOOLTIP_WIDTH: f32 = 500.0;
const TOOLTIP_GAP: f32 = 8.0;
const TOOLTIP_BACKGROUND: Color = Color::srgba(0.025, 0.020, 0.015, 0.98);
const TOOLTIP_BORDER: Color = Color::srgb(0.48, 0.39, 0.22);
const TOOLTIP_TITLE_COLOR: Color = Color::srgb(1.0, 0.82, 0.25);
const TOOLTIP_TEXT_COLOR: Color = Color::srgb(0.95, 0.95, 0.92);

const ALL_BUILD_KINDS: [BuildKind; 8] = [
    BuildKind::Production(ProductionKind::Barracks),
    BuildKind::Production(ProductionKind::Stronghold),
    BuildKind::Production(ProductionKind::RangersHall),
    BuildKind::Production(ProductionKind::OrcishSiegeFactory),
    BuildKind::Production(ProductionKind::IceTrollHut),
    BuildKind::Production(ProductionKind::GryphonRock),
    BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::WatchTower),
    BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::PoofTower),
];

fn command_slot(position: CommandCardPosition) -> usize {
    usize::from(position.y) * GRID_COLUMNS + usize::from(position.x)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetingAction {
    Move,
    Repair,
    Blink,
    Attack,
    Build(BuildKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ActionPanelMode {
    #[default]
    Actions,
    BuildMenu,
    Targeting(TargetingAction),
}

#[derive(Resource)]
pub(crate) struct ActionPanelState {
    pub(crate) team: Team,
    pub(crate) actor: Option<SimId>,
    pub(crate) mode: ActionPanelMode,
    pub(crate) status: String,
}

impl Default for ActionPanelState {
    fn default() -> Self {
        Self {
            team: Team(0),
            actor: None,
            mode: ActionPanelMode::Actions,
            status: "Select a controllable builder, production building, or tower.".into(),
        }
    }
}

impl ActionPanelState {
    pub(crate) const fn targeting(&self) -> Option<TargetingAction> {
        match self.mode {
            ActionPanelMode::Targeting(action) => Some(action),
            ActionPanelMode::Actions | ActionPanelMode::BuildMenu => None,
        }
    }

    pub(crate) fn cancel_modal(&mut self) {
        self.mode = match self.mode {
            ActionPanelMode::Targeting(TargetingAction::Build(_)) => ActionPanelMode::BuildMenu,
            ActionPanelMode::Targeting(_) | ActionPanelMode::BuildMenu => ActionPanelMode::Actions,
            ActionPanelMode::Actions => ActionPanelMode::Actions,
        };
        self.status = "Command cancelled.".into();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductionPanelAction {
    Upgrade(ProductionKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelAction {
    Target(TargetingAction),
    Production(ProductionPanelAction),
    OpenBuildMenu,
    CancelConstruction,
    Cancel,
}

#[derive(Component)]
struct ActionPanel;

#[derive(Component, Debug, Clone, Copy)]
struct CommandSlot(usize);

#[derive(Component, Debug, Clone, Copy, Default)]
struct SlotAction(Option<PanelAction>);

#[derive(Component)]
struct SlotLabel;

#[derive(Component)]
struct SlotIcon;

#[derive(Component)]
struct BuildTooltip;

#[derive(Component)]
struct BuildTooltipTitle;

#[derive(Component)]
struct BuildTooltipBody;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionTooltipKind {
    Build(BuildKind),
    ProductionUpgrade(ProductionKind),
}

impl ActionTooltipKind {
    fn tooltips(self) -> (&'static str, &'static str) {
        match self {
            Self::Build(kind) => kind.tooltips(),
            Self::ProductionUpgrade(kind) => {
                let definition = kind.definition();
                (definition.basic_tooltip, definition.extended_tooltip)
            }
        }
    }
}

#[derive(Resource, Default)]
struct BuildTooltipState(Option<ActionTooltipKind>);

type ActionInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static SlotAction),
    (Changed<Interaction>, With<Button>),
>;

pub(crate) struct BuildUiPlugin;

impl Plugin for BuildUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActionPanelState>()
            .init_resource::<BuildTooltipState>()
            .insert_resource(UiIconAssets::load_default())
            .add_systems(Startup, setup_action_panel)
            .add_systems(
                Update,
                (
                    sync_action_panel_to_selection,
                    handle_escape,
                    populate_action_panel,
                    handle_action_panel_buttons,
                    handle_action_panel_right_click,
                    style_action_panel_buttons,
                    update_build_tooltip,
                    draw_build_preview,
                )
                    .chain(),
            );
    }
}

fn setup_action_panel(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(PANEL_LEFT),
                bottom: px(PANEL_BOTTOM),
                width: px(PANEL_WIDTH),
                height: px(PANEL_HEIGHT),
                padding: UiRect::all(px(PANEL_PADDING)),
                border: UiRect::all(px(3.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(GRID_GAP),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(PANEL_BORDER),
            Visibility::Hidden,
            ActionPanel,
        ))
        .with_children(|panel| {
            for row_index in 0..GRID_ROWS {
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        height: px(CELL_SIZE),
                        column_gap: px(GRID_GAP),
                        ..default()
                    },))
                    .with_children(|row| {
                        for column_index in 0..GRID_COLUMNS {
                            let slot = row_index * GRID_COLUMNS + column_index;
                            row.spawn((
                                Button,
                                Node {
                                    width: px(CELL_SIZE),
                                    height: px(CELL_SIZE),
                                    border: UiRect::all(px(2.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(BUTTON_NORMAL),
                                BorderColor::all(SLOT_BORDER),
                                Visibility::Hidden,
                                CommandSlot(slot),
                                SlotAction::default(),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    ImageNode::default(),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(2.0),
                                        right: px(2.0),
                                        top: px(2.0),
                                        bottom: px(2.0),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                    CommandSlot(slot),
                                    SlotIcon,
                                ));
                                button.spawn((
                                    Text::new(""),
                                    TextFont::from_font_size(9.0),
                                    TextColor(BUTTON_TEXT),
                                    TextLayout::justify(Justify::Center),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(1.0),
                                        right: px(1.0),
                                        bottom: px(1.0),
                                        padding: UiRect::axes(px(2.0), px(1.0)),
                                        justify_content: JustifyContent::Center,
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_LABEL_BACKGROUND),
                                    GlobalZIndex(1),
                                    Pickable::IGNORE,
                                    CommandSlot(slot),
                                    SlotLabel,
                                ));
                            });
                        }
                    });
            }
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(PANEL_LEFT),
                bottom: px(PANEL_BOTTOM + PANEL_HEIGHT + TOOLTIP_GAP),
                width: px(TOOLTIP_WIDTH),
                padding: UiRect::all(px(10.0)),
                border: UiRect::all(px(2.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                ..default()
            },
            BackgroundColor(TOOLTIP_BACKGROUND),
            BorderColor::all(TOOLTIP_BORDER),
            GlobalZIndex(20),
            Pickable::IGNORE,
            Visibility::Hidden,
            BuildTooltip,
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                Text::new(""),
                TextFont::from_font_size(12.0),
                TextColor(TOOLTIP_TITLE_COLOR),
                TextLayout::default(),
                Node {
                    width: percent(100.0),
                    ..default()
                },
                BuildTooltipTitle,
            ));
            tooltip.spawn((
                Text::new(""),
                TextFont::from_font_size(10.0),
                TextColor(TOOLTIP_TEXT_COLOR),
                TextLayout::default(),
                Node {
                    width: percent(100.0),
                    ..default()
                },
                BuildTooltipBody,
            ));
        });
}

fn sync_action_panel_to_selection(
    inspection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    mut state: ResMut<ActionPanelState>,
    mut panel: Single<&mut Visibility, With<ActionPanel>>,
) {
    let selected = inspection.selected;
    let relevant = selected.and_then(|id| {
        if let Some(builder) = samples.current.builders.get(&id) {
            return Some((id, builder.team));
        }
        samples.current.buildings.get(&id).and_then(|building| {
            (building.construction_complete_tick.is_some()
                || building_is_controllable_production(building)
                || building_is_controllable_tower(building))
            .then_some((id, building.team))
        })
    });

    match relevant {
        Some((actor, team)) => {
            if state.actor != Some(actor) {
                state.actor = Some(actor);
                state.team = team;
                state.mode = ActionPanelMode::Actions;
                state.status = "Choose an action.".into();
            } else {
                state.team = team;
            }
            **panel = Visibility::Visible;
        }
        None => {
            state.actor = None;
            state.mode = ActionPanelMode::Actions;
            **panel = Visibility::Hidden;
        }
    }
}

fn handle_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut samples: ResMut<PresentationSamples>,
    mut state: ResMut<ActionPanelState>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if state.mode != ActionPanelMode::Actions {
        state.cancel_modal();
        return;
    }
    let Some(actor) = state.actor else {
        return;
    };
    if authoritative
        .simulation
        .building(actor)
        .is_some_and(|building| building.construction_complete_tick.is_some())
    {
        cancel_selected_construction(&mut authoritative, &mut samples, &mut state, actor);
    }
}

fn cancel_selected_construction(
    authoritative: &mut AuthoritativeSimulation,
    samples: &mut PresentationSamples,
    state: &mut ActionPanelState,
    actor: SimId,
) {
    match authoritative
        .simulation
        .cancel_building_construction(state.team, actor)
    {
        Ok(BuildingConstructionCancelOutcome::RemovedNewBuilding) => {
            samples.publish(PresentationSnapshot::capture(&authoritative.simulation));
            state.actor = None;
            state.status = "Construction cancelled and resources refunded.".into();
        }
        Ok(BuildingConstructionCancelOutcome::RevertedUpgrade) => {
            samples.publish(PresentationSnapshot::capture(&authoritative.simulation));
            state.status = "Upgrade cancelled and the original building restored.".into();
        }
        Err(error) => {
            state.status = format!("Unable to cancel construction: {error:?}.");
        }
    }
}

fn populate_action_panel(
    state: Res<ActionPanelState>,
    authoritative: Res<AuthoritativeSimulation>,
    asset_server: Res<AssetServer>,
    mut icon_assets: ResMut<UiIconAssets>,
    mut buttons: Query<(&CommandSlot, &mut SlotAction, &mut Visibility)>,
    mut labels: Query<(&CommandSlot, &mut Text), With<SlotLabel>>,
    mut icons: Query<(&CommandSlot, &mut ImageNode), With<SlotIcon>>,
) {
    let layout = action_layout(&state, &authoritative);
    let command_card = castle_fight_command_card_layout();
    let presentation = CastleFightPresentationCatalog::for_version(command_card.map_version)
        .expect("active Castle Fight version must have presentation bindings");

    for (slot, mut action, mut visibility) in &mut buttons {
        action.0 = layout[slot.0];
        *visibility = if action.0.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (slot, mut text) in &mut labels {
        text.0 = layout[slot.0].map_or_else(String::new, action_label);
    }
    for (slot, mut image) in &mut icons {
        *image = layout[slot.0]
            .map(|action| action_icon_key(action, &state, &authoritative, presentation))
            .and_then(|key| icon_assets.image(key, &asset_server))
            .map_or_else(ImageNode::default, ImageNode::new);
    }
}

fn action_icon_key(
    action: PanelAction,
    state: &ActionPanelState,
    authoritative: &AuthoritativeSimulation,
    presentation: CastleFightPresentationCatalog,
) -> UiIconKey {
    match action {
        PanelAction::OpenBuildMenu => presentation.build_command,
        PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
            UiIconKey::unit_game_interface(target.definition().rawcode)
        }
        PanelAction::CancelConstruction | PanelAction::Cancel => presentation.cancel_command,
        PanelAction::Target(TargetingAction::Move) => presentation.move_command,
        PanelAction::Target(TargetingAction::Repair) => {
            let autocast_active = state.actor.is_some_and(|actor| {
                authoritative
                    .simulation
                    .builder(actor)
                    .is_some_and(|builder| builder.repair_autocast_enabled)
            });
            if autocast_active {
                presentation.repair_turn_off_command
            } else {
                presentation.repair_command
            }
        }
        PanelAction::Target(TargetingAction::Blink) => presentation.blink_command,
        PanelAction::Target(TargetingAction::Attack) => presentation.attack_command,
        PanelAction::Target(TargetingAction::Build(kind)) => {
            UiIconKey::unit_game_interface(kind.rawcode())
        }
    }
}

fn insert_panel_action(
    slots: &mut [Option<PanelAction>; SLOT_COUNT],
    preferred: usize,
    cancel_slot: usize,
    action: PanelAction,
) {
    let slot = if preferred < SLOT_COUNT && preferred != cancel_slot && slots[preferred].is_none() {
        preferred
    } else {
        (0..SLOT_COUNT)
            .find(|index| *index != cancel_slot && slots[*index].is_none())
            .expect("implemented command card actions must fit the WC3 command card")
    };
    slots[slot] = Some(action);
}

fn action_layout(
    state: &ActionPanelState,
    authoritative: &AuthoritativeSimulation,
) -> [Option<PanelAction>; SLOT_COUNT] {
    let mut slots = [None; SLOT_COUNT];
    let Some(actor) = state.actor else {
        return slots;
    };
    let command_card = castle_fight_command_card_layout();
    let cancel_slot = command_slot(command_card.cancel_command);

    match state.mode {
        ActionPanelMode::Actions => {
            if authoritative.simulation.builder(actor).is_some() {
                slots[command_slot(command_card.move_command)] =
                    Some(PanelAction::Target(TargetingAction::Move));
                slots[command_slot(command_card.repair_ability)] =
                    Some(PanelAction::Target(TargetingAction::Repair));
                slots[command_slot(command_card.blink_ability)] =
                    Some(PanelAction::Target(TargetingAction::Blink));
                slots[command_slot(command_card.build_command)] = Some(PanelAction::OpenBuildMenu);
            } else if let Some(building) = authoritative.simulation.building(actor) {
                if building.construction_complete_tick.is_some() {
                    slots[cancel_slot] = Some(PanelAction::CancelConstruction);
                } else if let Some(kind) = building
                    .content
                    .and_then(|content| ProductionKind::from_rawcode(content.rawcode))
                {
                    for target in kind.upgrade_targets() {
                        insert_panel_action(
                            &mut slots,
                            command_slot(target.definition().command_card_position),
                            cancel_slot,
                            PanelAction::Production(ProductionPanelAction::Upgrade(target)),
                        );
                    }
                } else if building.attack_delivery.is_some()
                    && building.content.is_some_and(|content| {
                        castle_fight_sim::CastleFightTowerKind::from_rawcode(content.rawcode)
                            .is_some()
                    })
                {
                    slots[command_slot(command_card.attack_command)] =
                        Some(PanelAction::Target(TargetingAction::Attack));
                }
            }
        }
        ActionPanelMode::BuildMenu => {
            let Some(builder) = authoritative.simulation.builder(actor) else {
                return slots;
            };
            for kind in ALL_BUILD_KINDS {
                if !builder.configuration.allows_building(kind.rawcode()) {
                    continue;
                }
                // The verification client intentionally exposes a mixed-race catalog. Real
                // race catalogs do not collide here; for this synthetic menu retain authored
                // slots when possible and resolve cross-race collisions deterministically.
                insert_panel_action(
                    &mut slots,
                    command_slot(kind.command_card_position()),
                    cancel_slot,
                    PanelAction::Target(TargetingAction::Build(kind)),
                );
            }
            slots[cancel_slot] = Some(PanelAction::Cancel);
        }
        ActionPanelMode::Targeting(_) => {
            slots[cancel_slot] = Some(PanelAction::Cancel);
        }
    }
    slots
}

fn handle_action_panel_buttons(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut samples: ResMut<PresentationSamples>,
    mut state: ResMut<ActionPanelState>,
    actions: ActionInteractions,
) {
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, action) in &actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(action) = action.0 else {
            continue;
        };
        match action {
            PanelAction::OpenBuildMenu => {
                state.mode = ActionPanelMode::BuildMenu;
                state.status = "Choose a building.".into();
            }
            PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
                let Some(actor) = state.actor else {
                    continue;
                };
                if !can_afford_production_upgrade(&authoritative, &state, target) {
                    state.status =
                        insufficient_upgrade_resources_status(&authoritative, &state, target);
                    continue;
                }
                match order_demo_production_upgrade(&mut authoritative.simulation, actor, target) {
                    Ok(()) => {
                        samples.publish(PresentationSnapshot::capture(&authoritative.simulation));
                        state.status = format!(
                            "Upgrading to {} — construction can be cancelled until completion.",
                            target.definition().name
                        );
                    }
                    Err(error) => {
                        state.status = format!("Unable to start upgrade: {error:?}.");
                    }
                }
            }
            PanelAction::Target(TargetingAction::Build(kind)) => {
                if !can_afford_build_kind(&authoritative, &state, kind) {
                    state.status = insufficient_resources_status(&authoritative, &state, kind);
                    continue;
                }
                state.mode = ActionPanelMode::Targeting(TargetingAction::Build(kind));
                state.status = format!(
                    "{} selected — {} gold / {} lumber. Left-click a build site; Esc cancels this building.",
                    kind.label(),
                    kind.gold_cost(),
                    kind.lumber_cost(),
                );
            }
            PanelAction::Target(action) => {
                state.mode = ActionPanelMode::Targeting(action);
                state.status = match action {
                    TargetingAction::Move => "Move: left-click a destination; Esc cancels.".into(),
                    TargetingAction::Repair => {
                        "Repair: left-click a friendly building or mechanical unit; Esc cancels."
                            .into()
                    }
                    TargetingAction::Blink => {
                        "Blink: left-click a destination; Esc cancels.".into()
                    }
                    TargetingAction::Attack => {
                        "Attack: left-click an enemy unit or building; Esc cancels.".into()
                    }
                    TargetingAction::Build(_) => unreachable!(),
                };
            }
            PanelAction::CancelConstruction => {
                let Some(actor) = state.actor else {
                    continue;
                };
                cancel_selected_construction(&mut authoritative, &mut samples, &mut state, actor);
            }
            PanelAction::Cancel => state.cancel_modal(),
        }
    }
}

fn handle_action_panel_right_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<ActionPanelState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    buttons: Query<(&Interaction, &SlotAction), With<Button>>,
) {
    if !mouse_buttons.just_pressed(MouseButton::Right) || state.mode != ActionPanelMode::Actions {
        return;
    }
    let Some(actor) = state.actor else {
        return;
    };
    let repair_hovered = buttons
        .iter()
        .any(|(interaction, action)| repair_autocast_button_hovered(*interaction, action.0));
    if !repair_hovered {
        return;
    }
    let Some(builder) = presentation.current.builders.get(&actor).copied() else {
        return;
    };

    let enabled = !builder.repair_autocast_enabled;
    match authoritative
        .simulation
        .set_builder_repair_autocast(actor, enabled)
    {
        Ok(()) => {
            state.status = format!(
                "Repair autocast {}.",
                if enabled { "enabled" } else { "disabled" }
            );
            presentation.publish(crate::bridge::PresentationSnapshot::capture(
                &authoritative.simulation,
            ));
        }
        Err(error) => {
            state.status = format!("Repair autocast command rejected: {error:?}.");
        }
    }
}

fn repair_autocast_button_hovered(interaction: Interaction, action: Option<PanelAction>) -> bool {
    matches!(interaction, Interaction::Hovered | Interaction::Pressed)
        && action == Some(PanelAction::Target(TargetingAction::Repair))
}

fn style_action_panel_buttons(
    state: Res<ActionPanelState>,
    authoritative: Res<AuthoritativeSimulation>,
    mut buttons: Query<(
        &CommandSlot,
        &SlotAction,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&CommandSlot, &mut TextColor), With<SlotLabel>>,
    mut icons: Query<(&CommandSlot, &mut ImageNode), With<SlotIcon>>,
) {
    let mut disabled_slots = [false; SLOT_COUNT];
    for (slot, action, interaction, mut background, mut border) in &mut buttons {
        let disabled = action.0.is_some_and(|action| match action {
            PanelAction::Target(TargetingAction::Build(kind)) => {
                !can_afford_build_kind(&authoritative, &state, kind)
            }
            PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
                !can_afford_production_upgrade(&authoritative, &state, target)
            }
            _ => false,
        });
        disabled_slots[slot.0] = disabled;
        *background = BackgroundColor(if disabled {
            BUTTON_DISABLED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        let autocast_active = action.0 == Some(PanelAction::Target(TargetingAction::Repair))
            && state.actor.is_some_and(|actor| {
                authoritative
                    .simulation
                    .builder(actor)
                    .is_some_and(|builder| builder.repair_autocast_enabled)
            });
        *border = BorderColor::all(if disabled {
            BUTTON_DISABLED_BORDER
        } else if autocast_active {
            AUTOCAST_BORDER
        } else {
            SLOT_BORDER
        });
    }
    for (slot, mut color) in &mut labels {
        color.0 = if disabled_slots[slot.0] {
            BUTTON_TEXT_DISABLED
        } else {
            BUTTON_TEXT
        };
    }
    for (slot, mut image) in &mut icons {
        image.color = if disabled_slots[slot.0] {
            BUTTON_ICON_DISABLED
        } else {
            Color::WHITE
        };
    }
}

fn update_build_tooltip(
    mut commands: Commands,
    buttons: Query<(&Interaction, &SlotAction), With<Button>>,
    mut tooltip_state: ResMut<BuildTooltipState>,
    mut tooltip_visibility: Single<&mut Visibility, With<BuildTooltip>>,
    tooltip_title: Single<Entity, With<BuildTooltipTitle>>,
    tooltip_body: Single<Entity, With<BuildTooltipBody>>,
) {
    let hovered = buttons.iter().find_map(|(interaction, action)| {
        matches!(interaction, Interaction::Hovered | Interaction::Pressed)
            .then_some(action.0)
            .flatten()
            .and_then(|action| match action {
                PanelAction::Target(TargetingAction::Build(kind)) => {
                    Some(ActionTooltipKind::Build(kind))
                }
                PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
                    Some(ActionTooltipKind::ProductionUpgrade(target))
                }
                _ => None,
            })
    });

    if tooltip_state.0 == hovered {
        return;
    }
    tooltip_state.0 = hovered;

    let Some(kind) = hovered else {
        **tooltip_visibility = Visibility::Hidden;
        return;
    };

    let (basic, extended) = kind.tooltips();
    set_wc3_text(&mut commands, *tooltip_title, basic, TOOLTIP_TITLE_COLOR);
    set_wc3_text(&mut commands, *tooltip_body, extended, TOOLTIP_TEXT_COLOR);
    **tooltip_visibility = Visibility::Visible;
}

fn set_wc3_text(commands: &mut Commands, entity: Entity, source: &str, default_color: Color) {
    commands.entity(entity).despawn_children();
    commands.entity(entity).with_children(|text| {
        for run in parse_wc3_text(source) {
            let color = run.color.map(wc3_color_to_bevy).unwrap_or(default_color);
            text.spawn((
                TextSpan::new(wc3_text_for_embedded_font(&run.text)),
                TextColor(color),
            ));
        }
    });
}

fn wc3_color_to_bevy(color: Wc3Color) -> Color {
    Color::srgba_u8(color.red, color.green, color.blue, color.alpha)
}

fn wc3_text_for_embedded_font(text: &str) -> String {
    // Bevy's built-in Fira Mono subset contains only ASCII (U+0020..U+007E). The current
    // extracted building tooltip text otherwise stays inside that range except for WC3's U+2022 bullet.
    // Keep the original string in versioned content/parser output and substitute only at the final
    // rendering boundary until the client ships a fuller UI font.
    text.replace('•', "-")
}

fn draw_build_preview(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    authoritative: Res<AuthoritativeSimulation>,
    ui_state: (Res<ActionPanelState>, Res<DebugMenuState>),
    mut gizmos: Gizmos,
) {
    let (state, debug_menu) = ui_state;
    let Some(TargetingAction::Build(kind)) = state.targeting() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), state.actor.is_some())
        || cursor_over_debug_menu(cursor, debug_menu.is_open())
    {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        return;
    };
    let footprint = placement_footprint(&metrics, world, kind);
    let valid = authoritative
        .simulation
        .can_place_building_for_team(state.team, footprint)
        && can_afford_build_kind(&authoritative, &state, kind);
    let color = if valid {
        team_ui_color(state.team)
    } else {
        Color::srgb(1.0, 0.18, 0.15)
    };
    draw_footprint_outline(&mut gizmos, &metrics, &terrain, footprint, color);
}

pub(crate) fn placement_footprint(
    metrics: &WorldMetrics,
    world: Vec3,
    kind: BuildKind,
) -> BuildingFootprint {
    let size = kind.footprint_size();
    metrics.footprint_at_world(world, size, size)
}

fn can_afford_build_kind(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    kind: BuildKind,
) -> bool {
    let Some(actor) = state.actor else {
        return false;
    };
    authoritative
        .simulation
        .can_builder_afford_building(actor, kind.economy())
}

fn can_afford_production_upgrade(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    target: ProductionKind,
) -> bool {
    authoritative
        .simulation
        .can_afford_building(state.team, target.definition().economy)
}

fn insufficient_upgrade_resources_status(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    target: ProductionKind,
) -> String {
    let definition = target.definition();
    let resources = authoritative
        .simulation
        .player_resources(state.team)
        .expect("action panel supports the two Castle Fight players");
    format!(
        "Cannot upgrade to {}: need {} gold / {} lumber; currently {} / {}.",
        definition.name,
        definition.economy.gold_cost,
        definition.economy.lumber_cost,
        resources.gold,
        resources.lumber,
    )
}

fn insufficient_resources_status(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    kind: BuildKind,
) -> String {
    let resources = authoritative
        .simulation
        .player_resources(state.team)
        .expect("action panel supports the two Castle Fight players");
    format!(
        "Cannot afford {}: need {} gold / {} lumber; currently {} / {} committed/free.",
        kind.label(),
        kind.gold_cost(),
        kind.lumber_cost(),
        resources.gold,
        resources.lumber,
    )
}

fn action_label(action: PanelAction) -> String {
    match action {
        PanelAction::OpenBuildMenu => "Build".into(),
        PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
            production_upgrade_button_label(target)
        }
        PanelAction::CancelConstruction | PanelAction::Cancel => "Cancel\nEsc".into(),
        PanelAction::Target(TargetingAction::Move) => "Move".into(),
        PanelAction::Target(TargetingAction::Repair) => "Repair".into(),
        PanelAction::Target(TargetingAction::Blink) => "Blink".into(),
        PanelAction::Target(TargetingAction::Attack) => "Attack".into(),
        PanelAction::Target(TargetingAction::Build(kind)) => build_button_label(kind),
    }
}

fn production_upgrade_button_label(target: ProductionKind) -> String {
    let definition = target.definition();
    if definition.economy.lumber_cost == 0 {
        format!(
            "Upgrade\n{}\n{}g",
            definition.name, definition.economy.gold_cost
        )
    } else {
        format!(
            "Upgrade\n{}\n{}g {}w",
            definition.name, definition.economy.gold_cost, definition.economy.lumber_cost
        )
    }
}

fn build_button_label(kind: BuildKind) -> String {
    let name = match kind {
        BuildKind::Production(ProductionKind::Barracks) => "Barracks",
        BuildKind::Production(ProductionKind::Stronghold) => "Stronghold",
        BuildKind::Production(ProductionKind::RangersHall) => "Rngrs Hall",
        BuildKind::Production(ProductionKind::OrcishSiegeFactory) => "Siege Fac.",
        BuildKind::Production(ProductionKind::IceTrollHut) => "Ice Hut",
        BuildKind::Production(ProductionKind::GryphonRock) => "Gryph Rock",
        BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::WatchTower) => "Watch Tower",
        BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::PoofTower) => "Poof Tower",
    };
    let economy = kind.economy();
    if economy.lumber_cost == 0 {
        format!("{name}\n{}g", economy.gold_cost)
    } else {
        format!("{name}\n{}g {}w", economy.gold_cost, economy.lumber_cost)
    }
}

pub(crate) fn cursor_over_action_panel(
    cursor: Vec2,
    window_height: f32,
    panel_visible: bool,
) -> bool {
    if cursor.y <= TOP_BAR_HEIGHT {
        return true;
    }
    if !panel_visible {
        return false;
    }
    let panel_top = window_height - PANEL_BOTTOM - PANEL_HEIGHT;
    cursor.x >= PANEL_LEFT
        && cursor.x <= PANEL_LEFT + PANEL_WIDTH
        && cursor.y >= panel_top
        && cursor.y <= panel_top + PANEL_HEIGHT
}

fn building_is_attack_capable(kind: BuildingVisualKind) -> bool {
    matches!(
        kind,
        BuildingVisualKind::Attack
            | BuildingVisualKind::ProductionAttack
            | BuildingVisualKind::AttackSpellcaster
            | BuildingVisualKind::ProductionAttackSpellcaster
    )
}

fn building_is_controllable_production(building: &BuildingSample) -> bool {
    building
        .content
        .is_some_and(|content| ProductionKind::from_rawcode(content.rawcode).is_some())
}

fn building_is_controllable_tower(building: &BuildingSample) -> bool {
    building_is_attack_capable(building.visual_kind)
        && building.content.is_some_and(|content| {
            castle_fight_sim::CastleFightTowerKind::from_rawcode(content.rawcode).is_some()
        })
}

fn team_ui_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.20, 0.58, 1.0),
        1 => Color::srgb(1.0, 0.28, 0.22),
        _ => Color::WHITE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::{NavCell, SUBUNITS_PER_WORLD_UNIT, SimPoint, SimulationConfig};

    use crate::demo::create_demo_world;

    #[test]
    fn placement_preview_uses_authoritative_navigation_cell_scale() {
        let config = SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 20),
            team_objective: [SimPoint::new(0, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let metrics = WorldMetrics::from_simulation_config(&config);
        let footprint = placement_footprint(
            &metrics,
            Vec3::new(105.0, 0.0, 75.0),
            BuildKind::Production(ProductionKind::Barracks),
        );
        assert_eq!(footprint, BuildingFootprint::new(8, 5, 4, 4));
    }

    #[test]
    fn panel_capture_matches_visible_bottom_left_panel_bounds() {
        let window_height = 720.0;
        let panel_top = window_height - PANEL_BOTTOM - PANEL_HEIGHT;
        assert!(cursor_over_action_panel(
            Vec2::new(PANEL_LEFT, panel_top),
            window_height,
            true,
        ));
        assert!(!cursor_over_action_panel(
            Vec2::new(PANEL_LEFT + PANEL_WIDTH + 1.0, panel_top),
            window_height,
            true,
        ));
        assert!(cursor_over_action_panel(
            Vec2::new(900.0, TOP_BAR_HEIGHT / 2.0),
            window_height,
            false,
        ));
    }

    #[test]
    fn builder_action_positions_match_wc3_command_card_and_map_abilities() {
        let demo = create_demo_world(1, Some(0));
        let actor = demo.simulation.builder_for_team(Team(0)).unwrap().id;
        let state = ActionPanelState {
            actor: Some(actor),
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation {
            simulation: demo.simulation,
        };
        let layout = action_layout(&state, &authoritative);
        let command_card = castle_fight_command_card_layout();
        assert_eq!(
            layout[command_slot(command_card.move_command)],
            Some(PanelAction::Target(TargetingAction::Move))
        );
        assert_eq!(
            layout[command_slot(command_card.repair_ability)],
            Some(PanelAction::Target(TargetingAction::Repair))
        );
        assert_eq!(
            layout[command_slot(command_card.blink_ability)],
            Some(PanelAction::Target(TargetingAction::Blink))
        );
        assert_eq!(
            layout[command_slot(command_card.build_command)],
            Some(PanelAction::OpenBuildMenu)
        );
    }

    #[test]
    fn tower_action_panel_exposes_attack_in_versioned_wc3_slot() {
        let mut simulation = castle_fight_sim::Simulation::new(SimulationConfig::default(), 1);
        let tower = castle_fight_sim::CastleFightTowerKind::WatchTower.definition();
        let tower_id = simulation.spawn_building_with_properties(
            tower.spawn(Team(0), BuildingFootprint::new(0, 0, 4, 4)),
            tower.gameplay_properties(),
        );
        let state = ActionPanelState {
            actor: Some(tower_id),
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation { simulation };
        let layout = action_layout(&state, &authoritative);
        let command_card = castle_fight_command_card_layout();
        assert_eq!(
            layout[command_slot(command_card.attack_command)],
            Some(PanelAction::Target(TargetingAction::Attack))
        );
    }

    #[test]
    fn build_menu_uses_authored_slots_and_resolves_only_mixed_demo_collisions() {
        let demo = create_demo_world(1, Some(0));
        let actor = demo.simulation.builder_for_team(Team(0)).unwrap().id;
        let state = ActionPanelState {
            actor: Some(actor),
            mode: ActionPanelMode::BuildMenu,
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation {
            simulation: demo.simulation,
        };
        let layout = action_layout(&state, &authoritative);
        let barracks = BuildKind::Production(ProductionKind::Barracks);
        let siege_factory = BuildKind::Production(ProductionKind::OrcishSiegeFactory);
        assert_eq!(
            layout[command_slot(barracks.command_card_position())],
            Some(PanelAction::Target(TargetingAction::Build(barracks)))
        );
        assert_eq!(
            layout[command_slot(siege_factory.command_card_position())],
            Some(PanelAction::Target(TargetingAction::Build(siege_factory)))
        );
        assert_eq!(
            layout[command_slot(castle_fight_command_card_layout().cancel_command)],
            Some(PanelAction::Cancel)
        );
        let build_count = layout
            .iter()
            .filter(|action| matches!(action, Some(PanelAction::Target(TargetingAction::Build(_)))))
            .count();
        assert_eq!(build_count, ALL_BUILD_KINDS.len() - 1);
        assert!(!layout.iter().any(|action| {
            *action
                == Some(PanelAction::Target(TargetingAction::Build(
                    BuildKind::Production(ProductionKind::Stronghold),
                )))
        }));
    }

    #[test]
    fn production_building_panel_exposes_versioned_upgrade_target() {
        let mut simulation = castle_fight_sim::Simulation::new(SimulationConfig::default(), 1);
        let barracks = ProductionKind::Barracks.definition();
        let barracks_id = simulation.spawn_building_with_properties(
            barracks.spawn(Team(0), BuildingFootprint::new(0, 0, 4, 4)),
            barracks.gameplay_properties(),
        );
        let state = ActionPanelState {
            actor: Some(barracks_id),
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation { simulation };
        let stronghold = ProductionKind::Stronghold;
        let layout = action_layout(&state, &authoritative);
        assert_eq!(
            layout[command_slot(stronghold.definition().command_card_position)],
            Some(PanelAction::Production(ProductionPanelAction::Upgrade(
                stronghold
            )))
        );
    }

    #[test]
    fn repair_button_is_the_only_right_click_autocast_action() {
        assert!(repair_autocast_button_hovered(
            Interaction::Hovered,
            Some(PanelAction::Target(TargetingAction::Repair)),
        ));
        assert!(!repair_autocast_button_hovered(
            Interaction::Hovered,
            Some(PanelAction::Target(TargetingAction::Move)),
        ));
        assert!(!repair_autocast_button_hovered(
            Interaction::None,
            Some(PanelAction::Target(TargetingAction::Repair)),
        ));
    }

    #[test]
    fn tooltip_rendering_replaces_wc3_bullet_for_ascii_embedded_font() {
        assert_eq!(
            wc3_text_for_embedded_font("Charged Hammer • Attacks"),
            "Charged Hammer - Attacks"
        );
        assert_eq!(wc3_text_for_embedded_font("ASCII only"), "ASCII only");
    }

    #[test]
    fn escape_semantics_keep_build_menu_after_cancelling_one_building() {
        let mut state = ActionPanelState {
            mode: ActionPanelMode::Targeting(TargetingAction::Build(BuildKind::Production(
                ProductionKind::Barracks,
            ))),
            ..ActionPanelState::default()
        };
        state.cancel_modal();
        assert_eq!(state.mode, ActionPanelMode::BuildMenu);
        state.cancel_modal();
        assert_eq!(state.mode, ActionPanelMode::Actions);

        state.mode = ActionPanelMode::Targeting(TargetingAction::Repair);
        state.cancel_modal();
        assert_eq!(state.mode, ActionPanelMode::Actions);
    }
}
