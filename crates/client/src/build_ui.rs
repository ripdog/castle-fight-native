use std::{collections::HashMap, time::Duration};

use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use castle_fight_sim::{
    BuildingFootprint, CastleFightBuildingKind, CastleFightContentBundle, CommandCardPosition,
    CommandSubmission, NavCell, PlayerCommand, SimId, Team,
};

#[cfg(test)]
use castle_fight_sim::PlayerId;

use crate::{
    AuthoritativeSimulation, ClientCommandSubmission, SelectedMatch,
    bridge::{BuildingSample, BuildingVisualKind, PresentationSamples},
    building_models::BuildingModelSet,
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    demo::{BuildKind, ProductionKind},
    inspection::{InspectionSelection, cursor_over_inspector_panel},
    presentation::{
        WC3_BUILDING_AMBIENT_ANIMATION_SPEED, WC3_MODEL_FACING_OFFSET, WorldMetrics,
        building_terrain_height, draw_footprint_outline, player_color, viewport_ground_point,
    },
    resource_ui::{
        BuilderShortcutState, TOP_BAR_HEIGHT, cursor_over_builder_shortcuts,
        cursor_over_map_grid_toggle,
    },
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
const BUILD_GHOST_VALID_COLOR: Color = Color::srgb(0.48, 1.0, 0.52);

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
struct BuildGhostMaterial(Handle<StandardMaterial>);

#[derive(Resource, Default)]
struct BuildPreviewMaterials {
    textured: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuildPlacementGhost {
    rawcode: u32,
}

#[derive(Component)]
struct BuildPlacementGhostAnimationController;

#[derive(SystemParam)]
struct BuildPreviewResources<'w> {
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    authoritative: Res<'w, AuthoritativeSimulation>,
    state: Res<'w, ActionPanelState>,
    debug_menu: Res<'w, DebugMenuState>,
    builder_shortcuts: Res<'w, BuilderShortcutState>,
    selected_match: Res<'w, SelectedMatch>,
    building_models: Res<'w, BuildingModelSet>,
}

#[derive(SystemParam)]
struct BuildPreviewMaterialResources<'w> {
    materials: ResMut<'w, Assets<StandardMaterial>>,
    preview_materials: ResMut<'w, BuildPreviewMaterials>,
}

type ActionInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static SlotAction),
    (Changed<Interaction>, With<Button>),
>;

type BuildPlacementGhosts<'w, 's> = Query<
    'w,
    's,
    (
        &'static BuildPlacementGhost,
        &'static mut Transform,
        &'static mut Visibility,
    ),
>;

type BuildGhostMeshMaterials<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut MeshMaterial3d<StandardMaterial>,
        Has<Wc3MaterialProcessed>,
        Option<&'static BuildGhostMaterial>,
    ),
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
                (
                    sync_build_preview_ghost_materials
                        .after(update_build_preview)
                        .after(fix_wc3_scene_materials),
                    setup_build_preview_ghost_animation_players.after(update_build_preview),
                ),
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
    debug_menu: Res<DebugMenuState>,
    mut state: ResMut<ActionPanelState>,
    mut panel: Single<&mut Visibility, With<ActionPanel>>,
) {
    let selected = inspection.selected;
    let relevant = selected.and_then(|id| {
        if let Some(builder) = samples.current.builders.get(&id) {
            return debug_menu
                .can_control_builder(&authoritative.simulation, selected_match.local_player, id)
                .then_some((id, builder.team));
        }
        samples.current.buildings.get(&id).and_then(|building| {
            (debug_menu.can_control_building(
                &authoritative.simulation,
                selected_match.local_player,
                id,
            ) && (building.construction_complete_tick.is_some()
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
    debug_menu: Res<DebugMenuState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
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
        let controller = debug_menu.controller_for_actor(
            &authoritative.simulation,
            selected_match.local_player,
            actor,
        );
        cancel_selected_construction(&mut authoritative, &mut state, controller, actor);
    }
}

fn cancel_selected_construction(
    authoritative: &mut AuthoritativeSimulation,
    state: &mut ActionPanelState,
    controller: castle_fight_sim::PlayerId,
    actor: SimId,
) {
    let submission = authoritative.submit_local_command(
        controller,
        PlayerCommand::CancelBuildingConstruction { building: actor },
    );
    state.status = submission_status(
        submission,
        "Construction cancellation queued.",
        "Unable to cancel construction",
    );
}

fn submission_status(
    submission: ClientCommandSubmission,
    accepted: &str,
    rejected_prefix: &str,
) -> String {
    match submission {
        ClientCommandSubmission::Local(CommandSubmission::Scheduled(command)) => {
            format!("{accepted} [tick {}]", command.tick)
        }
        ClientCommandSubmission::Local(CommandSubmission::DuplicateScheduled(command)) => {
            format!("{accepted} [already scheduled for tick {}]", command.tick)
        }
        ClientCommandSubmission::Local(
            CommandSubmission::Rejected(error) | CommandSubmission::DuplicateRejected(error),
        ) => format!("{rejected_prefix}: {error:?}."),
        ClientCommandSubmission::Submitted { client_sequence } => {
            format!("{accepted} [submitted #{client_sequence}]")
        }
        ClientCommandSubmission::Failed => format!("{rejected_prefix}: network unavailable."),
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
    debug_menu: Res<DebugMenuState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
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
                let target_definition = selected_match
                    .content
                    .production_building(target)
                    .expect("upgrade target must belong to selected bundle");
                let controller = debug_menu.controller_for_actor(
                    &authoritative.simulation,
                    selected_match.local_player,
                    actor,
                );
                let submission = authoritative.submit_local_command(
                    controller,
                    PlayerCommand::UpgradeBuilding {
                        building: actor,
                        target: target.stable_id(),
                    },
                );
                state.status = submission_status(
                    submission,
                    &format!(
                        "Upgrade to {} queued — construction can be cancelled after execution.",
                        target_definition.name
                    ),
                    "Unable to start upgrade",
                );
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
                let controller = debug_menu.controller_for_actor(
                    &authoritative.simulation,
                    selected_match.local_player,
                    actor,
                );
                cancel_selected_construction(&mut authoritative, &mut state, controller, actor);
            }
            PanelAction::Cancel => state.cancel_modal(),
        }
    }
}

fn handle_action_panel_right_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    selected_match: Res<SelectedMatch>,
    debug_menu: Res<DebugMenuState>,
    mut state: ResMut<ActionPanelState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    presentation: Res<PresentationSamples>,
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
    let controller = debug_menu.controller_for_actor(
        &authoritative.simulation,
        selected_match.local_player,
        actor,
    );
    let submission = authoritative.submit_local_command(
        controller,
        PlayerCommand::SetBuilderRepairAutocast {
            builder: actor,
            enabled,
        },
    );
    state.status = submission_status(
        submission,
        if enabled {
            "Repair autocast enable queued."
        } else {
            "Repair autocast disable queued."
        },
        "Repair autocast command rejected",
    );
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

fn build_ghost_material(mut source: StandardMaterial) -> StandardMaterial {
    // Derive the preview from the already-processed Warcraft building material so team-colour
    // flattening, filter modes, lighting, culling, depth testing, and the diffuse texture all match
    // a completed building. Only the color multiplier changes.
    source.base_color = BUILD_GHOST_VALID_COLOR;
    source
}

fn hide_build_preview_ghosts(ghosts: &mut BuildPlacementGhosts<'_, '_>) {
    // Keep the loaded scene hierarchy alive. The material/animation setup systems can queue
    // deferred commands for ghost descendants later in the same Update schedule; despawning here
    // races those commands and can make Bevy apply an insert to an entity whose generation already
    // changed. Hidden roots are also cheaper to reuse when the cursor crosses validity boundaries.
    for (_, _, mut visibility) in ghosts {
        *visibility = Visibility::Hidden;
    }
}

fn update_build_preview(
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut ghosts: BuildPlacementGhosts<'_, '_>,
    resources: BuildPreviewResources<'_>,
    mut gizmos: Gizmos,
) {
    let Some(TargetingAction::Build(kind)) = resources.state.targeting() else {
        hide_build_preview_ghosts(&mut ghosts);
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        hide_build_preview_ghosts(&mut ghosts);
        return;
    };
    if cursor_over_action_panel(cursor, window.height(), resources.state.actor.is_some())
        || cursor_over_inspector_panel(cursor, window.width())
        || cursor_over_debug_menu(cursor, resources.debug_menu.is_open())
        || cursor_over_builder_shortcuts(cursor, &resources.builder_shortcuts)
        || cursor_over_map_grid_toggle(cursor, window.width())
    {
        hide_build_preview_ghosts(&mut ghosts);
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &resources.terrain)
    else {
        hide_build_preview_ghosts(&mut ghosts);
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

    if !valid {
        hide_build_preview_ghosts(&mut ghosts);
        return;
    }

    let rawcode = kind.rawcode(resources.selected_match.content);
    let Some(model) = resources.building_models.get(rawcode) else {
        hide_build_preview_ghosts(&mut ghosts);
        return;
    };
    let (mut center, _) = resources.metrics.footprint_center_size(footprint);
    center.y = building_terrain_height(&resources.metrics, &resources.terrain, footprint);
    let transform = Transform {
        translation: center,
        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
        scale: Vec3::splat(model.scale),
    };

    let mut found_matching_ghost = false;
    for (ghost, mut ghost_transform, mut visibility) in &mut ghosts {
        if ghost.rawcode == rawcode && !found_matching_ghost {
            found_matching_ghost = true;
            *ghost_transform = transform;
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
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
            Visibility::Visible,
            BuildPlacementGhost { rawcode },
        ));
    }
}

fn setup_build_preview_ghost_animation_players(
    mut commands: Commands,
    building_models: Res<BuildingModelSet>,
    parents: Query<&ChildOf>,
    ghosts: Query<&BuildPlacementGhost>,
    mut players: Query<
        (Entity, &mut AnimationPlayer),
        Without<BuildPlacementGhostAnimationController>,
    >,
) {
    for (entity, mut player) in &mut players {
        let Some(rawcode) = build_preview_ghost_rawcode(entity, &parents, &ghosts) else {
            continue;
        };
        let Some(animations) = building_models.animations(rawcode) else {
            continue;
        };
        let Some(stand) = animations.stand.clone() else {
            commands
                .entity(entity)
                .insert(BuildPlacementGhostAnimationController);
            continue;
        };

        let mut transitions = AnimationTransitions::new();
        transitions
            .play(&mut player, stand.node, Duration::ZERO)
            .repeat()
            .set_speed(WC3_BUILDING_AMBIENT_ANIMATION_SPEED);
        commands.entity(entity).insert((
            AnimationGraphHandle(animations.graph.clone()),
            transitions,
            BuildPlacementGhostAnimationController,
        ));
    }
}

fn build_preview_ghost_rawcode(
    entity: Entity,
    parents: &Query<&ChildOf>,
    ghosts: &Query<&BuildPlacementGhost>,
) -> Option<u32> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(ghost) = ghosts.get(current) {
            return Some(ghost.rawcode);
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

fn sync_build_preview_ghost_materials(
    mut commands: Commands,
    ghosts: Query<Entity, With<BuildPlacementGhost>>,
    children: Query<&Children>,
    mut mesh_materials: BuildGhostMeshMaterials<'_, '_>,
    mut resources: BuildPreviewMaterialResources<'_>,
) {
    for entity in &ghosts {
        for child in children.iter_descendants(entity) {
            let Ok((mesh_entity, mut mesh_material, wc3_material_processed, ghost_material)) =
                mesh_materials.get_mut(child)
            else {
                continue;
            };

            if let Some(ghost_material) = ghost_material {
                mesh_material.0 = ghost_material.0.clone();
                continue;
            }

            // Never derive a preview material from the raw glTF material. The normal Warcraft pass
            // first has to resolve filter modes and building team colour exactly as it does for a
            // real building.
            if !wc3_material_processed {
                continue;
            }

            let source = mesh_material.0.clone();
            let source_id = source.id();
            let tinted =
                if let Some(material) = resources.preview_materials.textured.get(&source_id) {
                    material.clone()
                } else {
                    let Some(source_material) = resources.materials.get(&source).cloned() else {
                        continue;
                    };
                    let material = resources
                        .materials
                        .add(build_ghost_material(source_material));
                    resources
                        .preview_materials
                        .textured
                        .insert(source_id, material.clone());
                    material
                };

            mesh_material.0 = tinted.clone();
            commands
                .entity(mesh_entity)
                .insert(BuildGhostMaterial(tinted));
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
        let authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
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
        let authoritative = AuthoritativeSimulation::new(simulation, demo.content);
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
        let authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
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
        let authoritative = AuthoritativeSimulation::new(simulation, demo.content);
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
    fn ghost_material_tints_processed_building_material_without_changing_render_semantics() {
        let texture = Handle::<Image>::default();
        let source = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(texture.clone()),
            alpha_mode: AlphaMode::Opaque,
            emissive: LinearRgba::WHITE,
            depth_bias: 7.0,
            ..default()
        };
        let ghost = build_ghost_material(source.clone());

        assert_eq!(ghost.base_color_texture, Some(texture));
        assert_eq!(ghost.base_color, BUILD_GHOST_VALID_COLOR);
        assert_eq!(ghost.alpha_mode, source.alpha_mode);
        assert_eq!(ghost.unlit, source.unlit);
        assert_eq!(ghost.emissive, source.emissive);
        assert_eq!(ghost.depth_bias, source.depth_bias);
        assert_eq!(ghost.cull_mode, source.cull_mode);
    }

    #[test]
    fn processed_ghost_mesh_remains_a_normally_depth_tested_standard_material() {
        let mut world = World::new();
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(BuildPreviewMaterials::default());

        let source = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                depth_bias: 7.0,
                ..default()
            });
        let root = world
            .spawn((
                BuildPlacementGhost {
                    rawcode: u32::from_be_bytes(*b"h03K"),
                },
                Transform::default(),
                Visibility::Visible,
            ))
            .id();
        let mesh = world
            .spawn((MeshMaterial3d(source.clone()), Wc3MaterialProcessed))
            .id();
        world.entity_mut(root).add_child(mesh);

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_build_preview_ghost_materials);
        schedule.run(&mut world);

        let tinted_handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .expect("processed preview mesh should remain a standard material")
            .0
            .clone();
        let marker = world
            .get::<BuildGhostMaterial>(mesh)
            .expect("processed preview mesh should retain its tinted material");
        assert_eq!(tinted_handle, marker.0);
        assert_ne!(tinted_handle, source);

        let tinted = world
            .resource::<Assets<StandardMaterial>>()
            .get(&tinted_handle)
            .expect("tinted preview material should exist");
        assert_eq!(tinted.base_color, BUILD_GHOST_VALID_COLOR);
        assert_eq!(tinted.alpha_mode, AlphaMode::Opaque);
        assert_eq!(tinted.depth_bias, 7.0);
        assert!(!tinted.unlit);
    }

    #[test]
    fn hiding_preview_keeps_scene_entities_alive_for_deferred_material_work() {
        let mut world = World::new();
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(BuildPreviewMaterials::default());

        let source = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let root = world
            .spawn((
                BuildPlacementGhost {
                    rawcode: u32::from_be_bytes(*b"h03K"),
                },
                Transform::default(),
                Visibility::Visible,
            ))
            .id();
        let mesh = world
            .spawn((MeshMaterial3d(source), Wc3MaterialProcessed))
            .id();
        world.entity_mut(root).add_child(mesh);

        let mut state =
            bevy::ecs::system::SystemState::<BuildPlacementGhosts<'_, '_>>::new(&mut world);
        {
            let mut ghosts = state
                .get_mut(&mut world)
                .expect("preview ghost query should validate in test world");
            hide_build_preview_ghosts(&mut ghosts);
        }
        state.apply(&mut world);

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_build_preview_ghost_materials);
        schedule.run(&mut world);

        assert_eq!(world.get::<Visibility>(root), Some(&Visibility::Hidden));
        assert!(world.get::<BuildPlacementGhost>(root).is_some());
        assert!(world.get::<BuildGhostMaterial>(mesh).is_some());
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
