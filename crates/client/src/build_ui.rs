use std::collections::HashMap;

use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use castle_fight_sim::{
    BuildingConstructionCancelOutcome, BuildingFootprint, CastleFightBuildingKind,
    CastleFightContentBundle, CommandCardPosition, NavCell, SimId, Team,
};

#[cfg(test)]
use castle_fight_sim::PlayerId;

use crate::{
    AuthoritativeSimulation, SelectedMatch,
    bridge::{BuildingSample, BuildingVisualKind, PresentationSamples, PresentationSnapshot},
    building_models::BuildingModelSet,
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    demo::{BuildKind, ProductionKind, order_demo_production_upgrade},
    inspection::{InspectionSelection, cursor_over_inspector_panel},
    presentation::{
        WC3_MODEL_FACING_OFFSET, WorldMetrics, draw_footprint_outline, player_color,
        viewport_ground_point,
    },
    resource_ui::TOP_BAR_HEIGHT,
    terrain::TerrainSurface,
    ui_icons::{CastleFightPresentationCatalog, UiIconAssets, UiIconKey},
    wc3_effects::{Wc3MaterialProcessed, Wc3TeamTint, fix_wc3_scene_materials},
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
const BUILD_PREVIEW_VALID_COLOR: Color = Color::srgba(0.18, 1.0, 0.24, 0.82);
const BUILD_PREVIEW_INVALID_COLOR: Color = Color::srgba(1.0, 0.12, 0.10, 0.88);
const BUILD_GHOST_VALID_COLOR: Color = Color::srgba(0.48, 1.0, 0.52, 0.82);
const BUILD_GHOST_INVALID_COLOR: Color = Color::srgba(1.0, 0.30, 0.24, 0.86);

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
    fn tooltips(self, content: &CastleFightContentBundle) -> (&'static str, &'static str) {
        match self {
            Self::Build(kind) => kind.tooltips(content),
            Self::ProductionUpgrade(kind) => {
                let definition = content
                    .production_building(kind)
                    .expect("upgrade target must belong to selected content bundle");
                (definition.basic_tooltip, definition.extended_tooltip)
            }
        }
    }
}

#[derive(Resource, Default)]
struct BuildTooltipState(Option<ActionTooltipKind>);

#[derive(Component, Clone)]
struct BuildGhostMaterialPair {
    valid: Handle<StandardMaterial>,
    invalid: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct BuildPreviewMaterials {
    textured: HashMap<AssetId<StandardMaterial>, BuildGhostMaterialPair>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuildPlacementGhost {
    rawcode: u32,
    valid: bool,
}

#[derive(SystemParam)]
struct BuildPreviewResources<'w> {
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    authoritative: Res<'w, AuthoritativeSimulation>,
    state: Res<'w, ActionPanelState>,
    debug_menu: Res<'w, DebugMenuState>,
    selected_match: Res<'w, SelectedMatch>,
    building_models: Res<'w, BuildingModelSet>,
}

type ActionInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static SlotAction),
    (Changed<Interaction>, With<Button>),
>;

pub(crate) struct BuildUiPlugin;

impl Plugin for BuildUiPlugin {
    fn build(&self, app: &mut App) {
        let map_version = app.world().resource::<SelectedMatch>().content.map_version;
        app.init_resource::<ActionPanelState>()
            .init_resource::<BuildTooltipState>()
            .init_resource::<BuildPreviewMaterials>()
            .insert_resource(UiIconAssets::load_for_version(map_version))
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
                    update_build_preview,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                sync_build_preview_ghost_materials
                    .after(update_build_preview)
                    .after(fix_wc3_scene_materials),
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
    authoritative: Res<AuthoritativeSimulation>,
    selected_match: Res<SelectedMatch>,
    mut state: ResMut<ActionPanelState>,
    mut panel: Single<&mut Visibility, With<ActionPanel>>,
) {
    let selected = inspection.selected;
    let relevant = selected.and_then(|id| {
        if let Some(builder) = samples.current.builders.get(&id) {
            return authoritative
                .simulation
                .can_player_control_builder(selected_match.local_player, id)
                .then_some((id, builder.team));
        }
        samples.current.buildings.get(&id).and_then(|building| {
            (authoritative
                .simulation
                .can_player_control_building(selected_match.local_player, id)
                && (building.construction_complete_tick.is_some()
                    || building_is_controllable_production(building, selected_match.content)
                    || building_is_controllable_tower(building, selected_match.content)))
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
    selected_match: Res<SelectedMatch>,
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
        cancel_selected_construction(
            &mut authoritative,
            &mut samples,
            &mut state,
            selected_match.local_player,
            actor,
        );
    }
}

fn cancel_selected_construction(
    authoritative: &mut AuthoritativeSimulation,
    samples: &mut PresentationSamples,
    state: &mut ActionPanelState,
    controller: castle_fight_sim::PlayerId,
    actor: SimId,
) {
    match authoritative
        .simulation
        .cancel_building_construction_for_player(controller, actor)
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
    match_state: (
        Res<ActionPanelState>,
        Res<AuthoritativeSimulation>,
        Res<SelectedMatch>,
    ),
    asset_server: Res<AssetServer>,
    mut icon_assets: ResMut<UiIconAssets>,
    mut buttons: Query<(&CommandSlot, &mut SlotAction, &mut Visibility)>,
    mut labels: Query<(&CommandSlot, &mut Text), With<SlotLabel>>,
    mut icons: Query<(&CommandSlot, &mut ImageNode), With<SlotIcon>>,
) {
    let (state, authoritative, selected_match) = match_state;
    let layout = action_layout(&state, &authoritative, &selected_match);
    let presentation =
        CastleFightPresentationCatalog::for_version(selected_match.content.map_version)
            .expect("selected Castle Fight version must have presentation bindings");

    for (slot, mut action, mut visibility) in &mut buttons {
        action.0 = layout[slot.0];
        *visibility = if action.0.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (slot, mut text) in &mut labels {
        text.0 = layout[slot.0].map_or_else(String::new, |action| {
            action_label(action, selected_match.content)
        });
    }
    for (slot, mut image) in &mut icons {
        *image = layout[slot.0]
            .map(|action| {
                action_icon_key(
                    action,
                    &state,
                    &authoritative,
                    selected_match.content,
                    presentation,
                )
            })
            .and_then(|key| icon_assets.image(key, &asset_server))
            .map_or_else(ImageNode::default, ImageNode::new);
    }
}

fn action_icon_key(
    action: PanelAction,
    state: &ActionPanelState,
    authoritative: &AuthoritativeSimulation,
    content: &CastleFightContentBundle,
    presentation: CastleFightPresentationCatalog,
) -> UiIconKey {
    match action {
        PanelAction::OpenBuildMenu => presentation.build_command,
        PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
            let rawcode = content
                .production_building(target)
                .expect("upgrade target must belong to selected content bundle")
                .rawcode;
            UiIconKey::unit_game_interface(rawcode)
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
            UiIconKey::unit_game_interface(kind.rawcode(content))
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
    selected_match: &SelectedMatch,
) -> [Option<PanelAction>; SLOT_COUNT] {
    let mut slots = [None; SLOT_COUNT];
    let Some(actor) = state.actor else {
        return slots;
    };
    let command_card = selected_match.content.command_card;
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
                } else if let Some(kind) =
                    selected_production_kind_from_content(building.content, selected_match.content)
                {
                    for target in kind
                        .upgrade_targets_for_version(selected_match.content.map_version)
                        .expect("selected bundle must support its production upgrade graph")
                    {
                        let target_definition = selected_match
                            .content
                            .production_building(target)
                            .expect("upgrade target must belong to selected bundle");
                        insert_panel_action(
                            &mut slots,
                            command_slot(target_definition.command_card_position),
                            cancel_slot,
                            PanelAction::Production(ProductionPanelAction::Upgrade(target)),
                        );
                    }
                } else if building.attack_delivery.is_some()
                    && building.content.is_some_and(|identity| {
                        matches!(
                            selected_match
                                .content
                                .building_kind_for_rawcode(identity.rawcode),
                            Some(CastleFightBuildingKind::Tower(_))
                        )
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
            for &kind in &selected_match.direct_buildings {
                if !builder
                    .configuration
                    .allows_building(kind.rawcode(selected_match.content))
                {
                    continue;
                }
                // The development subset intentionally exposes a mixed-race catalog. Preserve
                // authored command-card positions when possible and resolve collisions in stable
                // bundle order.
                insert_panel_action(
                    &mut slots,
                    command_slot(kind.command_card_position(selected_match.content)),
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
    selected_match: Res<SelectedMatch>,
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
                if !can_afford_production_upgrade(
                    &authoritative,
                    &state,
                    target,
                    selected_match.content,
                ) {
                    state.status = insufficient_upgrade_resources_status(
                        &authoritative,
                        &state,
                        target,
                        selected_match.content,
                    );
                    continue;
                }
                match order_demo_production_upgrade(
                    &mut authoritative.simulation,
                    selected_match.content,
                    selected_match.local_player,
                    actor,
                    target,
                ) {
                    Ok(()) => {
                        samples.publish(PresentationSnapshot::capture(&authoritative.simulation));
                        state.status = format!(
                            "Upgrading to {} — construction can be cancelled until completion.",
                            selected_match
                                .content
                                .production_building(target)
                                .expect("upgrade target must belong to selected bundle")
                                .name
                        );
                    }
                    Err(error) => {
                        state.status = format!("Unable to start upgrade: {error:?}.");
                    }
                }
            }
            PanelAction::Target(TargetingAction::Build(kind)) => {
                if !can_afford_build_kind(&authoritative, &state, kind, selected_match.content) {
                    state.status = insufficient_resources_status(
                        &authoritative,
                        &state,
                        kind,
                        selected_match.content,
                    );
                    continue;
                }
                state.mode = ActionPanelMode::Targeting(TargetingAction::Build(kind));
                state.status = format!(
                    "{} selected — {} gold / {} lumber. Left-click a build site; Esc cancels this building.",
                    kind.label(selected_match.content),
                    kind.gold_cost(selected_match.content),
                    kind.lumber_cost(selected_match.content),
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
                cancel_selected_construction(
                    &mut authoritative,
                    &mut samples,
                    &mut state,
                    selected_match.local_player,
                    actor,
                );
            }
            PanelAction::Cancel => state.cancel_modal(),
        }
    }
}

fn handle_action_panel_right_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    selected_match: Res<SelectedMatch>,
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
    match authoritative.simulation.set_builder_repair_autocast_as(
        selected_match.local_player,
        actor,
        enabled,
    ) {
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
    selected_match: Res<SelectedMatch>,
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
                !can_afford_build_kind(&authoritative, &state, kind, selected_match.content)
            }
            PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
                !can_afford_production_upgrade(
                    &authoritative,
                    &state,
                    target,
                    selected_match.content,
                )
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
    selected_match: Res<SelectedMatch>,
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

    let (basic, extended) = kind.tooltips(selected_match.content);
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

fn build_ghost_material(mut source: StandardMaterial, tint: Color) -> StandardMaterial {
    // Derive the placement material from the already-processed Warcraft building material so
    // team-colour flattening, alpha/filter-mode fixes, and the diffuse texture are all retained.
    source.base_color = tint;
    source.alpha_mode = AlphaMode::Blend;
    source.unlit = true;
    source.emissive = LinearRgba::BLACK;
    source.cull_mode = None;
    source
}

fn update_build_preview(
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut ghosts: Query<(Entity, &mut BuildPlacementGhost, &mut Transform)>,
    resources: BuildPreviewResources<'_>,
    mut gizmos: Gizmos,
) {
    let Some(TargetingAction::Build(kind)) = resources.state.targeting() else {
        for (entity, ..) in &mut ghosts {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        for (entity, ..) in &mut ghosts {
            commands.entity(entity).despawn();
        }
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), resources.state.actor.is_some())
        || cursor_over_inspector_panel(cursor, window.width())
        || cursor_over_debug_menu(cursor, resources.debug_menu.is_open())
    {
        for (entity, ..) in &mut ghosts {
            commands.entity(entity).despawn();
        }
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        for (entity, ..) in &mut ghosts {
            commands.entity(entity).despawn();
        }
        return;
    };
    let footprint = placement_footprint(
        &resources.metrics,
        world,
        kind,
        resources.selected_match.content,
    );
    let valid = resources
        .authoritative
        .simulation
        .can_place_building_for_team(resources.state.team, footprint)
        && can_afford_build_kind(
            &resources.authoritative,
            &resources.state,
            kind,
            resources.selected_match.content,
        );

    for y in footprint.min_y..=footprint.max_y() {
        for x in footprint.min_x..=footprint.max_x() {
            let cell = NavCell::new(x, y);
            let cell_valid = resources
                .authoritative
                .simulation
                .can_place_building_cell_for_team(resources.state.team, cell);
            draw_footprint_outline(
                &mut gizmos,
                &resources.metrics,
                &resources.terrain,
                BuildingFootprint::new(x, y, 1, 1),
                if cell_valid {
                    BUILD_PREVIEW_VALID_COLOR
                } else {
                    BUILD_PREVIEW_INVALID_COLOR
                },
            );
        }
    }

    let rawcode = kind.rawcode(resources.selected_match.content);
    let Some(model) = resources.building_models.get(rawcode) else {
        for (entity, ..) in &mut ghosts {
            commands.entity(entity).despawn();
        }
        return;
    };
    let (mut center, _) = resources.metrics.footprint_center_size(footprint);
    center.y = resources.terrain.height_at_world(center.xz()) + 0.05;
    let transform = Transform {
        translation: center,
        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
        scale: Vec3::splat(model.scale),
    };

    let mut found_matching_ghost = false;
    for (entity, mut ghost, mut ghost_transform) in &mut ghosts {
        if found_matching_ghost || ghost.rawcode != rawcode {
            commands.entity(entity).despawn();
            continue;
        }
        found_matching_ghost = true;
        *ghost_transform = transform;
        if ghost.valid != valid {
            ghost.valid = valid;
        }
    }
    if !found_matching_ghost {
        commands.spawn((
            Name::new(format!(
                "WC3 build placement ghost {}",
                String::from_utf8_lossy(&rawcode.to_be_bytes())
            )),
            WorldAssetRoot(model.scene.clone()),
            Wc3TeamTint::new(
                resources.selected_match.local_player.0,
                player_color(resources.selected_match.local_player),
                "wc3/buildings",
            ),
            transform,
            BuildPlacementGhost { rawcode, valid },
        ));
    }
}

fn sync_build_preview_ghost_materials(
    mut commands: Commands,
    ghosts: Query<(Entity, &BuildPlacementGhost)>,
    children: Query<&Children>,
    mut mesh_materials: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        Option<&BuildGhostMaterialPair>,
        Has<Wc3MaterialProcessed>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut preview_materials: ResMut<BuildPreviewMaterials>,
) {
    for (entity, ghost) in &ghosts {
        for child in children.iter_descendants(entity) {
            let Ok((mut mesh_material, existing_pair, wc3_material_processed)) =
                mesh_materials.get_mut(child)
            else {
                continue;
            };

            if let Some(pair) = existing_pair {
                // Reassert the preview-owned material every frame. This makes the ghost immune to
                // later presentation systems replacing a mesh material because of scene timing.
                mesh_material.0 = if ghost.valid {
                    pair.valid.clone()
                } else {
                    pair.invalid.clone()
                };
                continue;
            }

            // Never derive a ghost material from the raw glTF material. The normal Warcraft pass
            // first has to resolve filter modes and building team colour exactly as it does for a
            // real building; otherwise translucent team-colour layers can make the preview vanish.
            if !wc3_material_processed {
                continue;
            }

            let source = mesh_material.0.clone();
            let source_id = source.id();
            let pair = if let Some(pair) = preview_materials.textured.get(&source_id) {
                pair.clone()
            } else {
                let Some(source_material) = materials.get(&source).cloned() else {
                    continue;
                };
                let pair = BuildGhostMaterialPair {
                    valid: materials.add(build_ghost_material(
                        source_material.clone(),
                        BUILD_GHOST_VALID_COLOR,
                    )),
                    invalid: materials.add(build_ghost_material(
                        source_material,
                        BUILD_GHOST_INVALID_COLOR,
                    )),
                };
                preview_materials.textured.insert(source_id, pair.clone());
                pair
            };

            mesh_material.0 = if ghost.valid {
                pair.valid.clone()
            } else {
                pair.invalid.clone()
            };
            commands.entity(child).insert(pair);
        }
    }
}

pub(crate) fn placement_footprint(
    metrics: &WorldMetrics,
    world: Vec3,
    kind: BuildKind,
    content: &CastleFightContentBundle,
) -> BuildingFootprint {
    let size = kind.footprint_size(content);
    metrics.footprint_at_world(world, size, size)
}

fn can_afford_build_kind(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    kind: BuildKind,
    content: &CastleFightContentBundle,
) -> bool {
    let Some(actor) = state.actor else {
        return false;
    };
    authoritative
        .simulation
        .can_builder_afford_building(actor, kind.economy(content))
}

fn can_afford_production_upgrade(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    target: ProductionKind,
    content: &CastleFightContentBundle,
) -> bool {
    let definition = content
        .production_building(target)
        .expect("upgrade target must belong to selected bundle");
    let Some(owner) = state
        .actor
        .and_then(|actor| authoritative.simulation.building(actor))
        .and_then(|building| building.owner)
    else {
        return false;
    };
    authoritative
        .simulation
        .can_afford_building_for_player(owner, definition.economy)
}

fn insufficient_upgrade_resources_status(
    authoritative: &AuthoritativeSimulation,
    state: &ActionPanelState,
    target: ProductionKind,
    content: &CastleFightContentBundle,
) -> String {
    let definition = content
        .production_building(target)
        .expect("upgrade target must belong to selected bundle");
    let owner = state
        .actor
        .and_then(|actor| authoritative.simulation.building(actor))
        .and_then(|building| building.owner)
        .expect("controllable production building must have an owner");
    let resources = authoritative
        .simulation
        .player_resources_for(owner)
        .expect("controllable building owner must have economy state");
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
    content: &CastleFightContentBundle,
) -> String {
    let owner = state
        .actor
        .and_then(|actor| authoritative.simulation.builder(actor))
        .map(|builder| builder.owner)
        .expect("build action requires an owned builder");
    let resources = authoritative
        .simulation
        .player_resources_for(owner)
        .expect("controllable builder owner must have economy state");
    format!(
        "Cannot afford {}: need {} gold / {} lumber; currently {} / {} committed/free.",
        kind.label(content),
        kind.gold_cost(content),
        kind.lumber_cost(content),
        resources.gold,
        resources.lumber,
    )
}

fn action_label(action: PanelAction, content: &CastleFightContentBundle) -> String {
    match action {
        PanelAction::OpenBuildMenu => "Build".into(),
        PanelAction::Production(ProductionPanelAction::Upgrade(target)) => {
            production_upgrade_button_label(target, content)
        }
        PanelAction::CancelConstruction | PanelAction::Cancel => "Cancel\nEsc".into(),
        PanelAction::Target(TargetingAction::Move) => "Move".into(),
        PanelAction::Target(TargetingAction::Repair) => "Repair".into(),
        PanelAction::Target(TargetingAction::Blink) => "Blink".into(),
        PanelAction::Target(TargetingAction::Attack) => "Attack".into(),
        PanelAction::Target(TargetingAction::Build(kind)) => build_button_label(kind, content),
    }
}

fn production_upgrade_button_label(
    target: ProductionKind,
    content: &CastleFightContentBundle,
) -> String {
    let definition = content
        .production_building(target)
        .expect("upgrade target must belong to selected bundle");
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

fn build_button_label(kind: BuildKind, content: &CastleFightContentBundle) -> String {
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
    let economy = kind.economy(content);
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

fn selected_production_kind_from_content(
    identity: Option<castle_fight_sim::ContentIdentity>,
    content: &CastleFightContentBundle,
) -> Option<ProductionKind> {
    identity
        .and_then(|identity| content.building_kind_for_rawcode(identity.rawcode))
        .and_then(|kind| match kind {
            CastleFightBuildingKind::Production(kind) => Some(kind),
            CastleFightBuildingKind::Tower(_) => None,
        })
}

fn building_is_controllable_production(
    building: &BuildingSample,
    content: &CastleFightContentBundle,
) -> bool {
    selected_production_kind_from_content(building.content, content).is_some()
}

fn building_is_controllable_tower(
    building: &BuildingSample,
    content: &CastleFightContentBundle,
) -> bool {
    building_is_attack_capable(building.visual_kind)
        && building.content.is_some_and(|identity| {
            matches!(
                content.building_kind_for_rawcode(identity.rawcode),
                Some(CastleFightBuildingKind::Tower(_))
            )
        })
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
        let demo = create_demo_world(1, Some(0));
        let footprint = placement_footprint(
            &metrics,
            Vec3::new(105.0, 0.0, 75.0),
            BuildKind::Production(ProductionKind::Barracks),
            demo.content,
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
        let selected_match = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings.clone(),
            local_player: PlayerId(0),
        };
        let authoritative = AuthoritativeSimulation {
            simulation: demo.simulation,
        };
        let layout = action_layout(&state, &authoritative, &selected_match);
        let command_card = selected_match.content.command_card;
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
        let demo = create_demo_world(1, Some(0));
        let tower = demo
            .content
            .tower(castle_fight_sim::CastleFightTowerKind::WatchTower)
            .unwrap();
        let mut simulation = demo.simulation;
        let tower_id = simulation.spawn_building_with_properties(
            tower.spawn(Team(0), BuildingFootprint::new(-120, 0, 4, 4)),
            tower.gameplay_properties(),
        );
        let state = ActionPanelState {
            actor: Some(tower_id),
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation { simulation };
        let selected_match = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        };
        let layout = action_layout(&state, &authoritative, &selected_match);
        let command_card = selected_match.content.command_card;
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
        let selected_match = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings.clone(),
            local_player: PlayerId(0),
        };
        let authoritative = AuthoritativeSimulation {
            simulation: demo.simulation,
        };
        let layout = action_layout(&state, &authoritative, &selected_match);
        let barracks = BuildKind::Production(ProductionKind::Barracks);
        let siege_factory = BuildKind::Production(ProductionKind::OrcishSiegeFactory);
        assert_eq!(
            layout[command_slot(barracks.command_card_position(selected_match.content))],
            Some(PanelAction::Target(TargetingAction::Build(barracks)))
        );
        assert_eq!(
            layout[command_slot(siege_factory.command_card_position(selected_match.content))],
            Some(PanelAction::Target(TargetingAction::Build(siege_factory)))
        );
        assert_eq!(
            layout[command_slot(selected_match.content.command_card.cancel_command)],
            Some(PanelAction::Cancel)
        );
        let build_count = layout
            .iter()
            .filter(|action| matches!(action, Some(PanelAction::Target(TargetingAction::Build(_)))))
            .count();
        assert_eq!(build_count, selected_match.direct_buildings.len());
        assert!(!layout.iter().any(|action| {
            *action
                == Some(PanelAction::Target(TargetingAction::Build(
                    BuildKind::Production(ProductionKind::Stronghold),
                )))
        }));
    }

    #[test]
    fn production_building_panel_exposes_versioned_upgrade_target() {
        let demo = create_demo_world(1, Some(0));
        let barracks = demo
            .content
            .production_building(ProductionKind::Barracks)
            .unwrap();
        let mut simulation = demo.simulation;
        let barracks_id = simulation.spawn_building_with_properties(
            barracks.spawn(Team(0), BuildingFootprint::new(-120, 0, 4, 4)),
            barracks.gameplay_properties(),
        );
        let state = ActionPanelState {
            actor: Some(barracks_id),
            ..ActionPanelState::default()
        };
        let authoritative = AuthoritativeSimulation { simulation };
        let selected_match = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        };
        let stronghold = ProductionKind::Stronghold;
        let stronghold_definition = selected_match
            .content
            .production_building(stronghold)
            .unwrap();
        let layout = action_layout(&state, &authoritative, &selected_match);
        assert_eq!(
            layout[command_slot(stronghold_definition.command_card_position)],
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
    fn ghost_material_wraps_processed_building_material_without_losing_texture() {
        let texture = Handle::<Image>::default();
        let source = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Opaque,
            emissive: LinearRgba::WHITE,
            depth_bias: 7.0,
            ..default()
        };
        let ghost = build_ghost_material(source, BUILD_GHOST_VALID_COLOR);

        assert_eq!(ghost.base_color_texture, Some(texture));
        assert_eq!(ghost.base_color, BUILD_GHOST_VALID_COLOR);
        assert_eq!(ghost.alpha_mode, AlphaMode::Blend);
        assert!(ghost.unlit);
        assert_eq!(ghost.emissive, LinearRgba::BLACK);
        assert_eq!(ghost.depth_bias, 7.0);
        assert_eq!(ghost.cull_mode, None);
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
