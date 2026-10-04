use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{
    ArmorType, AttackProfile, CASTLE_FIGHT_SIMULATION_HZ, CastleFightBuildingKind,
    CastleFightContentBundle, DamageType, PlayerId, SUBUNITS_PER_WORLD_UNIT, SimId, Team,
};

use crate::{
    SelectedMatch, SimulationPlayback,
    bridge::{
        BuilderSample, BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample,
        UnitVisualKind,
    },
    build_ui::{ActionPanelState, cursor_over_action_panel},
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    presentation::{
        DebugPresentation, WorldMetrics, draw_footprint_outline, sim_point_to_terrain_world,
        sim_point_to_terrain_world_lerp, sim_point_to_world, unit_height, unit_visual_altitude,
        unit_visual_center_lerp, viewport_ground_point,
    },
    resource_ui::{BuilderShortcutState, cursor_over_builder_shortcuts, cursor_over_map_controls},
    terrain::TerrainSurface,
    ui_icons::{UiCommandIcon, UiIconAssets, UiIconKey, UiIconRole, UiStatusIconRole},
};

const CONSOLE_HEIGHT: f32 = 300.0;
const MAP_SLOT_WIDTH: f32 = 280.0;
const ACTION_SLOT_WIDTH: f32 = 310.0;
const TILE_SIZE: f32 = 43.0;
const MIN_UNIT_PICK_RADIUS: f32 = 6.0;
// Human Builder X00C inherits the stock Peasant's 100x100 shadow footprint and selection scale 1.
// Use that authored footprint for the native click target instead of the old tiny placeholder size.
const BUILDER_PICK_RADIUS: f32 = 50.0;
const BUILDER_PICK_HEIGHT: f32 = 100.0;
const SELECTION_RING_PADDING: f32 = 2.5;
const SELECTION_COLOR: Color = Color::srgb(0.24, 0.95, 0.28);
const PANEL_BACKGROUND: Color = Color::srgba(0.035, 0.045, 0.060, 0.94);
const MAX_SELECTION: usize = 24;
const DRAG_THRESHOLD: f32 = 6.0;
const DOUBLE_CLICK_SECONDS: f64 = 0.35;

#[derive(Resource, Debug, Clone)]
pub(crate) struct InspectionSelection {
    pub(crate) selected: Option<SimId>,
    pub(crate) members: Vec<SimId>,
    groups: [Vec<SimId>; 10],
    last_click: Option<(SimId, f64)>,
    last_group: Option<(usize, f64)>,
}

impl Default for InspectionSelection {
    fn default() -> Self {
        Self {
            selected: None,
            members: Vec::new(),
            groups: std::array::from_fn(|_| Vec::new()),
            last_click: None,
            last_group: None,
        }
    }
}

impl InspectionSelection {
    pub(crate) fn replace(&mut self, ids: impl IntoIterator<Item = SimId>) {
        self.members.clear();
        self.selected = None;
        self.add(ids);
    }

    fn add(&mut self, ids: impl IntoIterator<Item = SimId>) {
        for id in ids {
            if self.members.len() == MAX_SELECTION {
                break;
            }
            if !self.members.contains(&id) {
                self.members.push(id);
            }
        }
        if self.selected.is_none() {
            self.selected = self.members.first().copied();
        }
    }

    fn toggle(&mut self, id: SimId) {
        if let Some(index) = self.members.iter().position(|member| *member == id) {
            self.members.remove(index);
            if self.selected == Some(id) {
                self.selected = self.members.first().copied();
            }
        } else {
            self.add([id]);
        }
    }

    fn retain_present(&mut self, samples: &PresentationSamples) {
        let present = |id: &SimId| {
            samples.current.builders.contains_key(id)
                || samples.current.units.contains_key(id)
                || samples.current.buildings.contains_key(id)
        };
        self.members.retain(present);
        for group in &mut self.groups {
            group.retain(present);
        }
        if self.selected.is_some_and(|id| !self.members.contains(&id)) {
            self.selected = self.members.first().copied();
        }
    }

    fn focus(&mut self, id: SimId) {
        if self.members.contains(&id) {
            self.selected = Some(id);
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct SelectionDrag {
    start: Option<Vec2>,
    current: Option<Vec2>,
}

#[derive(Component)]
struct InspectionText;

#[derive(Component)]
struct InspectionHeading;

#[derive(Component)]
struct ProductionTooltipButton;

#[derive(Component)]
struct ProductionTooltipPanel;

#[derive(Component)]
struct ProductionTooltipTitle;

#[derive(Component)]
struct ProductionTooltipBody;

#[derive(Component)]
struct ActiveEffectIcon(usize);

#[derive(Component)]
struct ActiveEffectButton(usize);

#[derive(Component)]
struct ActiveEffectTooltip;

#[derive(Resource, Default)]
pub(crate) struct PortraitCameraHold(pub(crate) Option<SimId>);

#[derive(Component)]
struct ProductionUiRoot;

#[derive(Component)]
struct ProductionProgressText;

#[derive(Component)]
struct ProductionProgressFill;

#[derive(Component)]
struct ProductionQueueIcon(usize);

#[derive(Clone, Copy, PartialEq, Eq)]
enum CombatTooltipKind {
    Attack(usize),
    Armor,
}

#[derive(Component)]
struct CombatTypeButton(CombatTooltipKind);

#[derive(Component)]
struct CombatTypeLabel(CombatTooltipKind);

#[derive(Clone, Copy)]
enum AttackStatKind {
    Range,
    Cooldown,
}

#[derive(Component)]
struct AttackStatLabel(usize, AttackStatKind);

#[derive(Component)]
struct ArmorReductionLabel;

#[derive(Component)]
struct CombatTypeIcon(CombatTooltipKind);

#[derive(Component)]
struct CombatTooltip;

#[derive(SystemParam)]
struct CombatBadgeIcons<'w, 's> {
    asset_server: Res<'w, AssetServer>,
    icon_assets: ResMut<'w, UiIconAssets>,
    images: Query<'w, 's, (&'static CombatTypeIcon, &'static mut ImageNode)>,
}

#[derive(SystemParam)]
struct ProductionTooltipUi<'w, 's> {
    button:
        Single<'w, 's, (&'static Interaction, &'static mut Node), With<ProductionTooltipButton>>,
    panel: Single<'w, 's, &'static mut Visibility, With<ProductionTooltipPanel>>,
    title: Single<'w, 's, Entity, With<ProductionTooltipTitle>>,
    body: Single<'w, 's, Entity, With<ProductionTooltipBody>>,
}

#[derive(SystemParam)]
struct ActiveEffectUi<'w, 's> {
    icon_assets: ResMut<'w, UiIconAssets>,
    buttons: Query<
        'w,
        's,
        (
            &'static ActiveEffectButton,
            &'static Interaction,
            &'static mut Node,
            &'static mut BorderColor,
        ),
    >,
    images: Query<'w, 's, (&'static ActiveEffectIcon, &'static mut ImageNode)>,
    tooltip:
        Single<'w, 's, (&'static mut Text, &'static mut Visibility), With<ActiveEffectTooltip>>,
}

#[derive(Component)]
struct DebugInspectionPanel;

#[derive(Component)]
struct DebugInspectionText;

#[derive(Component)]
struct SelectionTile(usize);

#[derive(Component)]
struct SelectionTileRoot;

#[derive(Component)]
struct InventoryPlaceholder;

#[derive(Component)]
struct SelectionTileIcon(usize);

#[derive(Component)]
struct SelectionTileHealth(usize);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum PortraitResource {
    Health,
    Mana,
}

#[derive(Component)]
struct PortraitResourceFill(PortraitResource);

#[derive(Component)]
struct PortraitResourceText(PortraitResource);

#[derive(Component)]
struct SelectionRectangle;

pub(crate) struct InspectionPlugin;

#[derive(SystemParam)]
pub(crate) struct WorldSelectionState<'w> {
    samples: Res<'w, PresentationSamples>,
    action_panel: Res<'w, ActionPanelState>,
    playback: Res<'w, SimulationPlayback>,
    debug_menu: Res<'w, DebugMenuState>,
    builder_shortcuts: Res<'w, BuilderShortcutState>,
}

#[derive(SystemParam)]
pub(crate) struct SelectionInput<'w> {
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    time: Res<'w, Time>,
    selected_match: Res<'w, SelectedMatch>,
    drag: ResMut<'w, SelectionDrag>,
}

type TileRootQuery<'w, 's> = Single<
    'w,
    's,
    &'static mut Node,
    (
        With<SelectionTileRoot>,
        Without<SelectionTile>,
        Without<SelectionTileHealth>,
    ),
>;
type InventoryQuery<'w, 's> =
    Single<'w, 's, &'static mut Visibility, (With<InventoryPlaceholder>, Without<SelectionTile>)>;
type SelectionTilesQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static SelectionTile,
        &'static mut Node,
        &'static mut Visibility,
        &'static mut BorderColor,
    ),
    (
        Without<SelectionTileRoot>,
        Without<SelectionTileHealth>,
        Without<InventoryPlaceholder>,
    ),
>;
type SelectionBarsQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SelectionTileHealth, &'static mut Node),
    (Without<SelectionTileRoot>, Without<SelectionTile>),
>;
type PortraitBarsQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PortraitResource,
        &'static mut Visibility,
        &'static mut Node,
    ),
    (Without<SelectionTileIcon>, Without<PortraitResourceFill>),
>;
type PortraitFillsQuery<'w, 's> = Query<
    'w,
    's,
    (&'static PortraitResourceFill, &'static mut Node),
    (Without<SelectionTileIcon>, Without<PortraitResource>),
>;
type PortraitIconsQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SelectionTileIcon, &'static mut Node),
    (Without<PortraitResourceFill>, Without<PortraitResource>),
>;

#[derive(SystemParam)]
struct SelectionTileUi<'w, 's> {
    icon_assets: ResMut<'w, UiIconAssets>,
    tile_root: TileRootQuery<'w, 's>,
    inventory: InventoryQuery<'w, 's>,
    tiles: SelectionTilesQuery<'w, 's>,
    icons: Query<'w, 's, (&'static SelectionTileIcon, &'static mut ImageNode)>,
    bars: SelectionBarsQuery<'w, 's>,
}

#[derive(SystemParam)]
struct ProductionUi<'w, 's> {
    icon_assets: ResMut<'w, UiIconAssets>,
    root: ProductionRootQuery<'w, 's>,
    label: Single<'w, 's, &'static mut Text, With<ProductionProgressText>>,
    fill: Single<
        'w,
        's,
        &'static mut Node,
        (With<ProductionProgressFill>, Without<ProductionUiRoot>),
    >,
    icons: Query<
        'w,
        's,
        (
            &'static ProductionQueueIcon,
            &'static mut ImageNode,
            &'static mut Visibility,
        ),
        Without<ProductionUiRoot>,
    >,
}

type ProductionRootQuery<'w, 's> = Single<
    'w,
    's,
    (&'static mut Node, &'static mut Visibility),
    (
        With<ProductionUiRoot>,
        Without<ProductionQueueIcon>,
        Without<ProductionProgressFill>,
    ),
>;

type CombatTooltipQuery<'w, 's> = Single<
    'w,
    's,
    (&'static mut Text, &'static mut Visibility),
    (
        With<CombatTooltip>,
        Without<CombatTypeButton>,
        Without<CombatTypeLabel>,
    ),
>;

type CombatLabelsQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        Option<&'static CombatTypeLabel>,
        Option<&'static AttackStatLabel>,
        Option<&'static ArmorReductionLabel>,
    ),
    (
        Without<CombatTooltip>,
        Or<(
            With<CombatTypeLabel>,
            With<AttackStatLabel>,
            With<ArmorReductionLabel>,
        )>,
    ),
>;

type SelectionTileInteractionQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SelectionTile, &'static Interaction),
    (Changed<Interaction>, With<Button>),
>;

impl Plugin for InspectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectionSelection>()
            .init_resource::<SelectionDrag>()
            .init_resource::<PortraitCameraHold>()
            .add_systems(Startup, setup_inspector_ui)
            .add_systems(
                Update,
                (
                    handle_world_selection,
                    handle_selection_hotkeys,
                    clear_stale_selection,
                    update_inspector_text,
                    update_production_tooltip,
                    update_production_ui,
                    update_combat_tooltip,
                    update_active_effect_icons,
                    update_debug_inspector_text,
                    update_selection_tiles,
                    update_portrait_resources,
                    handle_selection_tile_click,
                    update_selection_rectangle,
                    draw_selection_highlight,
                )
                    .chain(),
            );
    }
}

fn setup_inspector_ui(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(0.0),
            bottom: px(0.0),
            width: px(ACTION_SLOT_WIDTH),
            height: px(CONSOLE_HEIGHT),
            border: UiRect::all(px(3.0)),
            ..default()
        },
        BackgroundColor(PANEL_BACKGROUND),
        BorderColor::all(Color::srgb(0.32, 0.27, 0.16)),
        ZIndex(-1),
        Pickable::IGNORE,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                bottom: px(0.0),
                width: px(MAP_SLOT_WIDTH),
                height: px(CONSOLE_HEIGHT),
                border: UiRect::all(px(3.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(Color::srgb(0.32, 0.27, 0.16)),
        ))
        .with_child((
            Text::new("MINIMAP"),
            TextFont::from_font_size(17.0),
            TextColor(Color::srgb(0.45, 0.42, 0.35)),
            Pickable::IGNORE,
        ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(MAP_SLOT_WIDTH),
                right: px(ACTION_SLOT_WIDTH),
                bottom: px(0.0),
                height: px(CONSOLE_HEIGHT),
                padding: UiRect::all(px(12.0)),
                border: UiRect::all(px(3.0)),
                flex_direction: FlexDirection::Row,
                column_gap: px(12.0),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(Color::srgb(0.32, 0.27, 0.16)),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: px(8.0 * (TILE_SIZE + 3.0)),
                        height: percent(100.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(3.0),
                        row_gap: px(3.0),
                        align_content: AlignContent::FlexStart,
                        ..default()
                    },
                    SelectionTileRoot,
                ))
                .with_children(|tiles| {
                    for index in 0..MAX_SELECTION {
                        tiles
                            .spawn((
                                Button,
                                Node {
                                    width: px(TILE_SIZE),
                                    height: px(TILE_SIZE),
                                    border: UiRect::all(px(2.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.10, 0.09, 0.06)),
                                BorderColor::all(Color::srgb(0.26, 0.24, 0.19)),
                                Visibility::Hidden,
                                SelectionTile(index),
                            ))
                            .with_children(|tile| {
                                tile.spawn((
                                    ImageNode::default(),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(1.0),
                                        right: px(1.0),
                                        top: px(1.0),
                                        bottom: px(5.0),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                    SelectionTileIcon(index),
                                ));
                                tile.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(1.0),
                                        bottom: px(1.0),
                                        width: percent(100.0),
                                        height: px(4.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.10, 0.72, 0.16)),
                                    Pickable::IGNORE,
                                    SelectionTileHealth(index),
                                ));
                                if index == 0 {
                                    for (resource, bottom, color) in [
                                        (
                                            PortraitResource::Health,
                                            15.0,
                                            Color::srgb(0.08, 0.68, 0.12),
                                        ),
                                        (
                                            PortraitResource::Mana,
                                            1.0,
                                            Color::srgb(0.12, 0.34, 0.88),
                                        ),
                                    ] {
                                        tile.spawn((
                                            Node {
                                                position_type: PositionType::Absolute,
                                                left: px(1.0),
                                                right: px(1.0),
                                                bottom: px(bottom),
                                                height: px(13.0),
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgb(0.06, 0.07, 0.10)),
                                            Visibility::Hidden,
                                            Pickable::IGNORE,
                                            resource,
                                        ))
                                        .with_children(
                                            |bar| {
                                                bar.spawn((
                                                    Node {
                                                        width: percent(100.0),
                                                        height: percent(100.0),
                                                        ..default()
                                                    },
                                                    BackgroundColor(color),
                                                    PortraitResourceFill(resource),
                                                ));
                                                bar.spawn((
                                                    Text::new(""),
                                                    TextFont::from_font_size(11.0),
                                                    TextColor(Color::WHITE),
                                                    Node {
                                                        position_type: PositionType::Absolute,
                                                        width: percent(100.0),
                                                        height: percent(100.0),
                                                        justify_content: JustifyContent::Center,
                                                        align_items: AlignItems::Center,
                                                        ..default()
                                                    },
                                                    PortraitResourceText(resource),
                                                ));
                                            },
                                        );
                                    }
                                }
                            });
                    }
                });
            panel
                .spawn((Node {
                    flex_grow: 1.0,
                    height: percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10.0),
                    ..default()
                },))
                .with_children(|details| {
                    details
                        .spawn((Node {
                            align_items: AlignItems::Center,
                            column_gap: px(8.0),
                            ..default()
                        },))
                        .with_children(|heading| {
                            heading.spawn((
                                Text::new("No selection"),
                                TextFont::from_font_size(19.0),
                                TextColor(Color::srgb(0.92, 0.73, 0.25)),
                                InspectionHeading,
                            ));
                            heading.spawn((
                                Button,
                                Node {
                                    width: px(24.0),
                                    height: px(24.0),
                                    display: Display::None,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(px(1.0)),
                                    ..default()
                                },
                                BorderColor::all(Color::srgb(0.72, 0.56, 0.17)),
                                Text::new("i"),
                                TextFont::from_font_size(17.0),
                                TextColor(Color::srgb(0.95, 0.78, 0.30)),
                                ProductionTooltipButton,
                            ));
                        });
                    details.spawn((
                        Text::new("No selection."),
                        TextFont::from_font_size(15.0),
                        TextColor(Color::srgb(0.82, 0.81, 0.75)),
                        Node {
                            width: percent(100.0),
                            ..default()
                        },
                        InspectionText,
                    ));
                    details
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(12.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                        .with_children(|combat_row| {
                            for kind in [
                                CombatTooltipKind::Attack(0),
                                CombatTooltipKind::Attack(1),
                                CombatTooltipKind::Armor,
                            ] {
                                combat_row
                                    .spawn((
                                        Button,
                                        Node {
                                            width: px(
                                                if matches!(kind, CombatTooltipKind::Armor) {
                                                    124.0
                                                } else {
                                                    188.0
                                                },
                                            ),
                                            height: px(64.0),
                                            display: Display::None,
                                            align_items: AlignItems::Center,
                                            column_gap: px(5.0),
                                            ..default()
                                        },
                                        CombatTypeButton(kind),
                                    ))
                                    .with_children(|badge| {
                                        badge.spawn((
                                            ImageNode::default(),
                                            Node {
                                                width: px(48.0),
                                                height: px(48.0),
                                                ..default()
                                            },
                                            Pickable::IGNORE,
                                            CombatTypeIcon(kind),
                                        ));
                                        badge
                                            .spawn((Node {
                                                flex_direction: FlexDirection::Column,
                                                row_gap: px(2.0),
                                                ..default()
                                            },))
                                            .with_children(|stats| {
                                                stats.spawn((
                                                    Text::new(""),
                                                    TextFont::from_font_size(18.0),
                                                    TextColor(Color::srgb(0.96, 0.86, 0.56)),
                                                    Pickable::IGNORE,
                                                    CombatTypeLabel(kind),
                                                ));
                                                match kind {
                                                    CombatTooltipKind::Attack(slot) => {
                                                        for (stat, icon_path) in [
                                                            (AttackStatKind::Range, "ui/range.png"),
                                                            (
                                                                AttackStatKind::Cooldown,
                                                                "ui/cooldown.png",
                                                            ),
                                                        ] {
                                                            stats
                                                                .spawn((Node {
                                                                    align_items: AlignItems::Center,
                                                                    column_gap: px(3.0),
                                                                    ..default()
                                                                },))
                                                                .with_children(|line| {
                                                                    line.spawn((
                                                                        ImageNode::new(
                                                                            asset_server
                                                                                .load(icon_path),
                                                                        ),
                                                                        Node {
                                                                            width: px(15.0),
                                                                            height: px(15.0),
                                                                            ..default()
                                                                        },
                                                                        Pickable::IGNORE,
                                                                    ));
                                                                    line.spawn((
                                                                        Text::new(""),
                                                                        TextFont::from_font_size(
                                                                            12.0,
                                                                        ),
                                                                        TextColor(Color::srgb(
                                                                            0.82, 0.81, 0.75,
                                                                        )),
                                                                        Pickable::IGNORE,
                                                                        AttackStatLabel(slot, stat),
                                                                    ));
                                                                });
                                                        }
                                                    }
                                                    CombatTooltipKind::Armor => {
                                                        stats.spawn((
                                                            Text::new(""),
                                                            TextFont::from_font_size(12.0),
                                                            TextColor(Color::srgb(
                                                                0.82, 0.81, 0.75,
                                                            )),
                                                            Pickable::IGNORE,
                                                            ArmorReductionLabel,
                                                        ));
                                                    }
                                                }
                                            });
                                    });
                            }
                        });
                    details
                        .spawn((Node {
                            position_type: PositionType::Absolute,
                            left: px(0.0),
                            bottom: px(0.0),
                            width: percent(100.0),
                            column_gap: px(4.0),
                            ..default()
                        },))
                        .with_children(|effects| {
                            for index in 0..32 {
                                effects
                                    .spawn((
                                        Button,
                                        Node {
                                            width: px(30.0),
                                            height: px(30.0),
                                            display: Display::None,
                                            border: UiRect::all(px(1.0)),
                                            ..default()
                                        },
                                        BorderColor::all(Color::srgb(0.56, 0.47, 0.27)),
                                        ActiveEffectButton(index),
                                    ))
                                    .with_child((
                                        ImageNode::default(),
                                        Node {
                                            width: percent(100.0),
                                            height: percent(100.0),
                                            ..default()
                                        },
                                        Pickable::IGNORE,
                                        ActiveEffectIcon(index),
                                    ));
                            }
                        });
                });
            panel
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        width: px(200.0),
                        display: Display::None,
                        ..default()
                    },
                    Visibility::Hidden,
                    ProductionUiRoot,
                ))
                .with_children(|training| {
                    training.spawn((
                        Text::new("Training"),
                        TextFont::from_font_size(16.0),
                        TextColor(Color::srgb(0.96, 0.77, 0.18)),
                        ProductionProgressText,
                    ));
                    training
                        .spawn((
                            Node {
                                width: percent(100.0),
                                height: px(13.0),
                                border: UiRect::all(px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.06, 0.05, 0.03)),
                            BorderColor::all(Color::srgb(0.70, 0.55, 0.14)),
                        ))
                        .with_child((
                            Node {
                                width: percent(0.0),
                                height: percent(100.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.92, 0.69, 0.13)),
                            ProductionProgressFill,
                        ));
                    training
                        .spawn((Node {
                            column_gap: px(5.0),
                            ..default()
                        },))
                        .with_children(|queue| {
                            for index in 0..2 {
                                queue.spawn((
                                    ImageNode::default(),
                                    Node {
                                        width: px(32.0),
                                        height: px(32.0),
                                        border: UiRect::all(px(2.0)),
                                        ..default()
                                    },
                                    BorderColor::all(Color::srgb(0.70, 0.55, 0.14)),
                                    Visibility::Hidden,
                                    ProductionQueueIcon(index),
                                ));
                            }
                        });
                });
            panel
                .spawn((
                    Node {
                        width: px(99.0),
                        height: percent(100.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(3.0),
                        row_gap: px(3.0),
                        align_content: AlignContent::FlexStart,
                        ..default()
                    },
                    InventoryPlaceholder,
                ))
                .with_children(|inventory| {
                    for _ in 0..6 {
                        inventory.spawn((
                            Node {
                                width: px(46.0),
                                height: px(46.0),
                                border: UiRect::all(px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.07, 0.065, 0.055)),
                            BorderColor::all(Color::srgb(0.22, 0.21, 0.18)),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: px(0.0),
            height: px(0.0),
            border: UiRect::all(px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.15, 0.90, 0.22, 0.12)),
        BorderColor::all(Color::srgb(0.15, 0.90, 0.22)),
        Visibility::Hidden,
        Pickable::IGNORE,
        GlobalZIndex(1000),
        SelectionRectangle,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(ACTION_SLOT_WIDTH + 12.0),
            bottom: px(CONSOLE_HEIGHT + 8.0),
            width: px(300.0),
            padding: UiRect::all(px(10.0)),
            border: UiRect::all(px(2.0)),
            ..default()
        },
        BackgroundColor(PANEL_BACKGROUND),
        BorderColor::all(Color::srgb(0.72, 0.56, 0.17)),
        Visibility::Hidden,
        GlobalZIndex(1001),
        Pickable::IGNORE,
        CombatTooltip,
        Text::new(""),
        TextFont::from_font_size(16.0),
        TextColor(Color::WHITE),
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(ACTION_SLOT_WIDTH + 12.0),
                bottom: px(CONSOLE_HEIGHT + 8.0),
                width: px(440.0),
                padding: UiRect::all(px(10.0)),
                border: UiRect::all(px(2.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(Color::srgb(0.72, 0.56, 0.17)),
            Visibility::Hidden,
            GlobalZIndex(1002),
            Pickable::IGNORE,
            ProductionTooltipPanel,
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                Text::new(""),
                TextFont::from_font_size(17.0),
                TextColor(Color::srgb(0.96, 0.77, 0.18)),
                ProductionTooltipTitle,
            ));
            tooltip.spawn((
                Text::new(""),
                TextFont::from_font_size(14.0),
                TextColor(Color::WHITE),
                ProductionTooltipBody,
            ));
        });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(ACTION_SLOT_WIDTH + 12.0),
            bottom: px(CONSOLE_HEIGHT + 8.0),
            padding: UiRect::all(px(8.0)),
            border: UiRect::all(px(2.0)),
            ..default()
        },
        BackgroundColor(PANEL_BACKGROUND),
        BorderColor::all(Color::srgb(0.72, 0.56, 0.17)),
        Visibility::Hidden,
        GlobalZIndex(1003),
        Pickable::IGNORE,
        Text::new(""),
        TextFont::from_font_size(15.0),
        TextColor(Color::WHITE),
        ActiveEffectTooltip,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(12.0),
                top: px(70.0),
                width: px(350.0),
                padding: UiRect::all(px(10.0)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            Visibility::Hidden,
            Pickable::IGNORE,
            DebugInspectionPanel,
        ))
        .with_child((
            Text::new(""),
            TextFont::from_font_size(12.0),
            TextColor(Color::WHITE),
            DebugInspectionText,
        ));
}

pub(crate) fn handle_world_selection(
    mut input: SelectionInput<'_>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: (Res<Time<Fixed>>, Res<WorldMetrics>, Res<TerrainSurface>),
    state: WorldSelectionState<'_>,
    mut selection: ResMut<InspectionSelection>,
) {
    let (fixed_time, metrics, terrain) = world;
    if state.action_panel.targeting().is_some() {
        input.drag.start = None;
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let action_panel_visible = state.action_panel.actor.is_some();
    let over_ui = cursor_over_action_panel(
        cursor,
        window.width(),
        window.height(),
        action_panel_visible,
    ) || cursor_over_inspector_panel(cursor, window.width(), window.height())
        || cursor_over_debug_menu(cursor, state.debug_menu.is_open())
        || cursor_over_builder_shortcuts(cursor, &state.builder_shortcuts)
        || cursor_over_map_controls(cursor, window.width());
    if input.mouse_buttons.just_pressed(MouseButton::Left) && !over_ui {
        input.drag.start = Some(cursor);
    }
    input.drag.current = Some(cursor);
    if !input.mouse_buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(start) = input.drag.start.take() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let alpha = state.playback.interpolation_alpha(&fixed_time);
    let selection_view = SelectionView {
        camera,
        transform: camera_transform,
        samples: &state.samples,
        metrics: &metrics,
        terrain: &terrain,
        alpha,
        viewport: Vec2::new(window.width(), window.height()),
    };
    let shift = input.keys.pressed(KeyCode::ShiftLeft) || input.keys.pressed(KeyCode::ShiftRight);
    if start.distance(cursor) >= DRAG_THRESHOLD {
        let ids = box_select(
            start,
            cursor,
            &selection_view,
            input.selected_match.local_player,
            input.selected_match.content,
        );
        if shift {
            selection.add(ids);
        } else if !ids.is_empty() {
            selection.replace(ids);
        }
        return;
    }
    if over_ui {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    if let Some(id) =
        pick_world_actor_on_ray(ray.origin, *ray.direction, &state.samples, &terrain, alpha)
    {
        select_click(
            id,
            &input.keys,
            &input.time,
            &selection_view,
            &mut selection,
        );
        return;
    }
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        return;
    };
    if let Some(building) = pick_building_at_ground(world, &state.samples, &metrics) {
        select_click(
            building,
            &input.keys,
            &input.time,
            &selection_view,
            &mut selection,
        );
    }
}

fn pick_world_actor_on_ray(
    ray_origin: Vec3,
    ray_direction: Vec3,
    samples: &PresentationSamples,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<SimId> {
    let mut nearest: Option<(f32, SimId)> = None;
    let mut consider = |id: SimId, center: Vec3, radius: f32| {
        if let Some(distance) = ray_sphere_hit_distance(ray_origin, ray_direction, center, radius)
            && nearest
                .is_none_or(|(best, best_id)| distance < best || (distance == best && id < best_id))
        {
            nearest = Some((distance, id));
        }
    };
    for builder in samples.current.builders.values() {
        let previous = samples
            .previous
            .builders
            .get(&builder.id)
            .unwrap_or(builder);
        let center =
            sim_point_to_terrain_world_lerp(previous.position, builder.position, alpha, terrain)
                + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5);
        consider(
            builder.id,
            center,
            BUILDER_PICK_RADIUS.max(BUILDER_PICK_HEIGHT * 0.55),
        );
    }
    for unit in samples.current.units.values() {
        let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
        let center =
            unit_visual_center_lerp(previous.position, unit.position, unit, alpha, terrain);
        consider(
            unit.id,
            center,
            unit_pick_radius(unit).max(unit_height(unit) * 0.55),
        );
    }
    nearest.map(|(_, id)| id)
}

fn clear_stale_selection(
    samples: Res<PresentationSamples>,
    mut selection: ResMut<InspectionSelection>,
) {
    selection.retain_present(&samples);
}

fn select_click(
    id: SimId,
    keys: &ButtonInput<KeyCode>,
    time: &Time,
    view: &SelectionView<'_>,
    selection: &mut InspectionSelection,
) {
    let now = time.elapsed_secs_f64();
    let same_type = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || selection
            .last_click
            .is_some_and(|(previous, at)| previous == id && now - at <= DOUBLE_CLICK_SECONDS);
    selection.last_click = Some((id, now));
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if same_type {
        let ids = same_type_selection(id, view);
        if shift {
            selection.add(ids);
        } else {
            selection.replace(ids);
        }
    } else if shift {
        selection.toggle(id);
    } else {
        selection.replace([id]);
    }
}

struct SelectionView<'a> {
    camera: &'a Camera,
    transform: &'a GlobalTransform,
    samples: &'a PresentationSamples,
    metrics: &'a WorldMetrics,
    terrain: &'a TerrainSurface,
    alpha: f32,
    viewport: Vec2,
}

impl SelectionView<'_> {
    fn visible(&self, id: SimId) -> bool {
        let position = if let Some(builder) = self.samples.current.builders.get(&id) {
            let previous = self.samples.previous.builders.get(&id).unwrap_or(builder);
            sim_point_to_terrain_world_lerp(
                previous.position,
                builder.position,
                self.alpha,
                self.terrain,
            ) + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5)
        } else if let Some(building) = self.samples.current.buildings.get(&id) {
            let (mut center, _) = self.metrics.footprint_center_size(building.footprint);
            center.y = self.terrain.height_at_world(center.xz());
            center
        } else if let Some(unit) = self.samples.current.units.get(&id) {
            let previous = self.samples.previous.units.get(&id).unwrap_or(unit);
            unit_visual_center_lerp(
                previous.position,
                unit.position,
                unit,
                self.alpha,
                self.terrain,
            )
        } else {
            return false;
        };
        self.camera
            .world_to_viewport(self.transform, position)
            .is_ok_and(|screen| screen.cmpge(Vec2::ZERO).all() && screen.cmple(self.viewport).all())
    }
}

fn same_type_selection(id: SimId, view: &SelectionView<'_>) -> Vec<SimId> {
    let samples = view.samples;
    if let Some(builder) = samples.current.builders.get(&id) {
        return samples
            .current
            .builders
            .values()
            .filter(|candidate| {
                candidate.owner == builder.owner
                    && candidate.appearance.rawcode == builder.appearance.rawcode
            })
            .map(|candidate| candidate.id)
            .filter(|candidate| *candidate == id || view.visible(*candidate))
            .take(MAX_SELECTION)
            .collect();
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        let selected_owner = building.owner;
        return samples
            .current
            .buildings
            .values()
            .filter(|candidate| {
                candidate.owner == selected_owner
                    && candidate.content.map(|content| content.rawcode)
                        == building.content.map(|content| content.rawcode)
            })
            .map(|candidate| candidate.id)
            .filter(|candidate| *candidate == id || view.visible(*candidate))
            .take(MAX_SELECTION)
            .collect();
    }
    if let Some(unit) = samples.current.units.get(&id) {
        return samples
            .current
            .units
            .values()
            .filter(|candidate| {
                candidate.owner == unit.owner
                    && candidate.content.map(|content| content.rawcode)
                        == unit.content.map(|content| content.rawcode)
            })
            .map(|candidate| candidate.id)
            .filter(|candidate| *candidate == id || view.visible(*candidate))
            .take(MAX_SELECTION)
            .collect();
    }
    vec![id]
}

fn box_select(
    start: Vec2,
    end: Vec2,
    view: &SelectionView<'_>,
    owner: PlayerId,
    content: &CastleFightContentBundle,
) -> Vec<SimId> {
    let samples = view.samples;
    let min = start.min(end);
    let max = start.max(end);
    let inside = |position: Vec3| {
        view.camera
            .world_to_viewport(view.transform, position)
            .is_ok_and(|screen| screen.cmpge(min).all() && screen.cmple(max).all())
    };
    let builders: Vec<_> = samples
        .current
        .builders
        .values()
        .filter(|builder| builder.owner == owner)
        .filter(|builder| {
            let previous = samples
                .previous
                .builders
                .get(&builder.id)
                .unwrap_or(builder);
            inside(
                sim_point_to_terrain_world_lerp(
                    previous.position,
                    builder.position,
                    view.alpha,
                    view.terrain,
                ) + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5),
            )
        })
        .map(|builder| builder.id)
        .take(MAX_SELECTION)
        .collect();
    let buildings: Vec<_> = samples
        .current
        .buildings
        .values()
        .filter(|building| {
            building.owner == Some(owner)
                && building.content.is_some_and(|identity| {
                    matches!(
                        content.building_kind_for_rawcode(identity.rawcode),
                        Some(CastleFightBuildingKind::Production(_))
                    )
                })
        })
        .filter(|building| {
            let (mut center, _) = view.metrics.footprint_center_size(building.footprint);
            center.y = view.terrain.height_at_world(center.xz());
            inside(center)
        })
        .map(|building| building.id)
        .take(MAX_SELECTION)
        .collect();
    let units: Vec<_> = samples
        .current
        .units
        .values()
        .filter(|unit| unit.owner == owner)
        .filter(|unit| {
            let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
            inside(unit_visual_center_lerp(
                previous.position,
                unit.position,
                unit,
                view.alpha,
                view.terrain,
            ))
        })
        .map(|unit| unit.id)
        .take(MAX_SELECTION)
        .collect();
    let local = prioritized_box_selection(builders, buildings, units);
    if !local.is_empty() {
        return local;
    }
    let other_builders: Vec<_> = samples
        .current
        .builders
        .values()
        .filter(|builder| builder.owner != owner)
        .filter(|builder| {
            let previous = samples
                .previous
                .builders
                .get(&builder.id)
                .unwrap_or(builder);
            inside(
                sim_point_to_terrain_world_lerp(
                    previous.position,
                    builder.position,
                    view.alpha,
                    view.terrain,
                ) + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5),
            )
        })
        .map(|builder| builder.id)
        .take(MAX_SELECTION)
        .collect();
    let other_buildings: Vec<_> = samples
        .current
        .buildings
        .values()
        .filter(|building| building.owner.is_some_and(|player| player != owner))
        .filter(|building| {
            let (mut center, _) = view.metrics.footprint_center_size(building.footprint);
            center.y = view.terrain.height_at_world(center.xz());
            inside(center)
        })
        .map(|building| building.id)
        .take(MAX_SELECTION)
        .collect();
    let other_units: Vec<_> = samples
        .current
        .units
        .values()
        .filter(|unit| unit.owner != owner)
        .filter(|unit| {
            let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
            inside(unit_visual_center_lerp(
                previous.position,
                unit.position,
                unit,
                view.alpha,
                view.terrain,
            ))
        })
        .map(|unit| unit.id)
        .take(MAX_SELECTION)
        .collect();
    prioritized_box_selection(other_builders, other_buildings, other_units)
}

fn prioritized_box_selection(
    builders: Vec<SimId>,
    buildings: Vec<SimId>,
    units: Vec<SimId>,
) -> Vec<SimId> {
    if !builders.is_empty() {
        builders
    } else if !buildings.is_empty() {
        buildings
    } else {
        units
    }
}

fn handle_selection_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    samples: Res<PresentationSamples>,
    selected_match: Res<SelectedMatch>,
    mut selection: ResMut<InspectionSelection>,
    mut camera_focus: ResMut<crate::presentation::CameraFocusRequest>,
) {
    if keys.just_pressed(KeyCode::Tab) && selection.members.len() > 1 {
        let mut types = Vec::new();
        for &id in &selection.members {
            if let Some(kind) = selection_type_key(id, &samples)
                && !types.contains(&kind)
            {
                types.push(kind);
            }
        }
        if types.len() > 1 {
            let active = selection
                .selected
                .and_then(|id| selection_type_key(id, &samples));
            let index = active
                .and_then(|kind| types.iter().position(|candidate| *candidate == kind))
                .unwrap_or(0);
            let backwards = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let next = if backwards {
                (index + types.len() - 1) % types.len()
            } else {
                (index + 1) % types.len()
            };
            if let Some(id) = selection
                .members
                .iter()
                .copied()
                .find(|id| selection_type_key(*id, &samples) == Some(types[next]))
            {
                selection.focus(id);
            }
        }
    }
    if keys.just_pressed(KeyCode::Backquote) {
        let idle: Vec<_> = samples
            .current
            .builders
            .values()
            .filter(|builder| builder.owner == selected_match.local_player)
            .filter(|builder| {
                builder.destination.is_none()
                    && builder.follow_target.is_none()
                    && builder.repair_target.is_none()
                    && builder.build_footprint.is_none()
            })
            .map(|builder| builder.id)
            .collect();
        if !idle.is_empty() {
            let next = idle
                .iter()
                .position(|id| Some(*id) == selection.selected)
                .map_or(0, |index| (index + 1) % idle.len());
            selection.replace([idle[next]]);
            camera_focus.0 = Some(idle[next]);
        }
    }
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (index, key) in DIGITS.into_iter().enumerate() {
        if !keys.just_pressed(key) {
            continue;
        }
        if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
            selection.groups[index] = selection.members.clone();
        } else if !selection.groups[index].is_empty() {
            let now = time.elapsed_secs_f64();
            if selection
                .last_group
                .is_some_and(|(last, at)| last == index && now - at <= DOUBLE_CLICK_SECONDS)
            {
                camera_focus.0 = selection.groups[index].first().copied();
            }
            selection.last_group = Some((index, now));
            let group = selection.groups[index].clone();
            selection.replace(group);
        }
    }
}

fn selection_type_key(id: SimId, samples: &PresentationSamples) -> Option<(u8, u32)> {
    samples
        .current
        .builders
        .get(&id)
        .map(|builder| (0, builder.appearance.rawcode))
        .or_else(|| {
            samples
                .current
                .units
                .get(&id)
                .map(|unit| (1, unit.content.map_or(0, |content| content.rawcode)))
        })
        .or_else(|| {
            samples
                .current
                .buildings
                .get(&id)
                .map(|building| (2, building.content.map_or(0, |content| content.rawcode)))
        })
}

fn update_inspector_text(
    samples: Res<PresentationSamples>,
    selection: Res<InspectionSelection>,
    mut heading: Single<&mut Text, (With<InspectionHeading>, Without<InspectionText>)>,
    mut text: Single<&mut Text, (With<InspectionText>, Without<InspectionHeading>)>,
) {
    let next_heading = if selection.members.len() > 1 {
        format!("{} selected", selection.members.len())
    } else {
        selection
            .selected
            .and_then(|id| selected_entity_name(id, &samples))
            .unwrap_or("No selection")
            .to_owned()
    };
    if heading.0 != next_heading {
        heading.0 = next_heading;
    }
    let next = match selection.selected {
        None => "No selection.".into(),
        Some(_) if selection.members.len() > 1 => {
            let constructing = selection
                .members
                .iter()
                .filter(|id| {
                    samples
                        .current
                        .buildings
                        .get(id)
                        .is_some_and(|building| building.construction_complete_tick.is_some())
                })
                .count();
            format!(
                "{} selected\n{} under construction\n\nShift-click an icon to remove it.",
                selection.members.len(),
                constructing
            )
        }
        Some(id) => selection_summary(id, &samples),
    };
    if text.0 != next {
        text.0 = next;
    }
}

fn selected_entity_name(id: SimId, samples: &PresentationSamples) -> Option<&'static str> {
    samples
        .current
        .builders
        .get(&id)
        .map(|builder| builder.appearance.name)
        .or_else(|| {
            samples
                .current
                .units
                .get(&id)
                .and_then(|unit| unit.content.map(|content| content.name))
        })
        .or_else(|| {
            samples
                .current
                .buildings
                .get(&id)
                .and_then(|building| building.content.map(|content| content.name))
        })
}

fn update_production_tooltip(
    mut commands: Commands,
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    selected_match: Res<SelectedMatch>,
    mut ui: ProductionTooltipUi<'_, '_>,
    mut shown: Local<Option<SimId>>,
) {
    let id = (selection.members.len() == 1)
        .then_some(selection.selected)
        .flatten();
    let tooltips = id.and_then(|id| {
        let rawcode = samples
            .current
            .builders
            .get(&id)
            .map(|builder| builder.appearance.rawcode)
            .or_else(|| {
                samples
                    .current
                    .units
                    .get(&id)
                    .and_then(|unit| unit.content.map(|content| content.rawcode))
            })
            .or_else(|| {
                samples
                    .current
                    .buildings
                    .get(&id)
                    .and_then(|building| building.content.map(|content| content.rawcode))
            })?;
        selected_match
            .content
            .building_kind_for_rawcode(rawcode)
            .and_then(|kind| kind.tooltips(selected_match.content))
            .or_else(|| selected_match.content.unit_tooltips_for_rawcode(rawcode))
            .filter(|(basic, extended)| !basic.is_empty() || !extended.is_empty())
    });
    ui.button.1.display = if tooltips.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    let hovered = if tooltips.is_some()
        && matches!(*ui.button.0, Interaction::Hovered | Interaction::Pressed)
    {
        id
    } else {
        None
    };
    **ui.panel = if hovered.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if *shown == hovered {
        return;
    }
    *shown = hovered;
    if let Some((basic, extended)) = tooltips.filter(|_| hovered.is_some()) {
        set_inspection_wc3_text(
            &mut commands,
            *ui.title,
            basic,
            Color::srgb(0.96, 0.77, 0.18),
        );
        set_inspection_wc3_text(&mut commands, *ui.body, extended, Color::WHITE);
    }
}

fn set_inspection_wc3_text(commands: &mut Commands, entity: Entity, source: &str, color: Color) {
    commands.entity(entity).despawn_children();
    commands.entity(entity).with_children(|text| {
        for run in crate::wc3_text::parse_wc3_text(source) {
            let color = run.color.map_or(color, |color| {
                Color::srgba_u8(color.red, color.green, color.blue, color.alpha)
            });
            text.spawn((TextSpan::new(run.text.replace('•', "-")), TextColor(color)));
        }
    });
}

fn update_production_ui(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    selected_match: Res<SelectedMatch>,
    asset_server: Res<AssetServer>,
    mut ui: ProductionUi<'_, '_>,
) {
    let building = (selection.members.len() == 1)
        .then(|| selection.selected)
        .flatten()
        .and_then(|id| samples.current.buildings.get(&id))
        .filter(|building| building.production_queue.is_some());
    let Some(building) = building else {
        ui.root.0.display = Display::None;
        *ui.root.1 = Visibility::Hidden;
        return;
    };
    ui.root.0.display = Display::Flex;
    *ui.root.1 = Visibility::Visible;
    let queued = building.production_queue.unwrap_or(0);
    let interval = u64::from(building.production_interval_ticks.unwrap_or(0));
    let remaining = building
        .next_spawn_tick
        .unwrap_or(0)
        .saturating_sub(samples.current.tick);
    let hz = CASTLE_FIGHT_SIMULATION_HZ as u64;
    ui.label.0 = if queued == 0 {
        "Production stopped".into()
    } else {
        format!("Training ({}s remaining)", remaining.div_ceil(hz))
    };
    ui.fill.width = percent(if queued == 0 || interval == 0 {
        0.0
    } else {
        (1.0 - remaining.min(interval) as f32 / interval as f32) * 100.0
    });
    let unit_rawcode = building.content.and_then(|identity| {
        let CastleFightBuildingKind::Production(kind) = selected_match
            .content
            .building_kind_for_rawcode(identity.rawcode)?
        else {
            return None;
        };
        selected_match
            .content
            .production_building(kind)
            .map(|definition| definition.produced_unit.rawcode)
    });
    let handle = unit_rawcode.and_then(|rawcode| {
        ui.icon_assets
            .image(UiIconKey::unit_game_interface(rawcode), &asset_server)
    });
    for (slot, mut image, mut visibility) in &mut ui.icons {
        *visibility = if slot.0 < usize::from(queued) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        *image = handle
            .clone()
            .map_or_else(ImageNode::default, ImageNode::new);
    }
}

fn update_combat_tooltip(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    mut badge_icons: CombatBadgeIcons<'_, '_>,
    mut buttons: Query<(&CombatTypeButton, &Interaction, &mut Node), Without<CombatTooltip>>,
    mut labels: CombatLabelsQuery<'_, '_>,
    mut tooltip: CombatTooltipQuery<'_, '_>,
) {
    let selected = (selection.members.len() == 1)
        .then_some(selection.selected)
        .flatten();
    let (attacks, armor) =
        selected.map_or(([None; 2], None), |id| selected_combat_badges(id, &samples));
    let mut hovered = None;
    for (button, interaction, mut node) in &mut buttons {
        let visible = match button.0 {
            CombatTooltipKind::Attack(slot) => attacks[slot].is_some(),
            CombatTooltipKind::Armor => armor.is_some(),
        };
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if visible && matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            hovered = Some(button.0);
        }
    }
    for (mut text, kind, stat, armor_reduction) in &mut labels {
        text.0 = if let Some(label) = kind {
            match label.0 {
                CombatTooltipKind::Attack(slot) => attacks[slot]
                    .map_or_else(String::new, |attack| attack.profile.damage.to_string()),
                CombatTooltipKind::Armor => armor.map_or_else(String::new, |armor| {
                    armor_points_label(armor.points_per_100)
                }),
            }
        } else if let Some(stat) = stat {
            attacks[stat.0].map_or_else(String::new, |attack| match stat.1 {
                AttackStatKind::Range => format!(
                    "{:.0}",
                    attack.profile.range as f32 / SUBUNITS_PER_WORLD_UNIT as f32
                ),
                AttackStatKind::Cooldown => format!(
                    "{:.1}s",
                    f32::from(attack.profile.cooldown_ticks) / CASTLE_FIGHT_SIMULATION_HZ as f32
                ),
            })
        } else if armor_reduction.is_some() {
            armor.map_or_else(String::new, |armor| {
                armor_reduction_label(
                    armor.points_per_100,
                    samples.current.damage_rules.armor_factor_per_10k(),
                )
            })
        } else {
            String::new()
        };
    }
    for (slot, mut image) in &mut badge_icons.images {
        let key = match slot.0 {
            CombatTooltipKind::Attack(index) => {
                attacks[index].map(|attack| UiIconKey::InfoDamage(attack.damage_type))
            }
            CombatTooltipKind::Armor => armor.map(|armor| UiIconKey::InfoArmor(armor.armor_type)),
        };
        *image = key
            .and_then(|key| {
                badge_icons
                    .icon_assets
                    .image(key, &badge_icons.asset_server)
                    .or_else(|| {
                        // Older local UI packs predate the info-panel bindings. Keep a visible
                        // Warcraft icon until the player re-runs UI extraction for exact art.
                        let fallback = match key {
                            UiIconKey::InfoDamage(_) => UiIconKey::Command(UiCommandIcon::Attack),
                            UiIconKey::InfoArmor(_) => {
                                UiIconKey::ability(u32::from_be_bytes(*b"AM08"), UiIconRole::Normal)
                            }
                            _ => return None,
                        };
                        badge_icons
                            .icon_assets
                            .image(fallback, &badge_icons.asset_server)
                    })
            })
            .map_or_else(ImageNode::default, ImageNode::new);
    }
    let (text, visibility) = &mut *tooltip;
    text.0 = match hovered {
        Some(CombatTooltipKind::Attack(slot)) => attacks[slot].map_or_else(String::new, |attack| {
            format!(
                "{} damage, {:.0} range, {:.2}s cooldown\n{}",
                attack.profile.damage,
                attack.profile.range as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
                f32::from(attack.profile.cooldown_ticks) / CASTLE_FIGHT_SIMULATION_HZ as f32,
                attack_matchup_tooltip(attack.damage_type, &samples)
            )
        }),
        Some(CombatTooltipKind::Armor) => armor.map_or_else(String::new, |armor| {
            format!(
                "{} armor: {}\n{}",
                armor_type_name(armor.armor_type),
                armor_points_label(armor.points_per_100),
                armor_matchup_tooltip(armor.armor_type, &samples)
            )
        }),
        None => String::new(),
    };
    **visibility = if hovered.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

#[derive(Clone, Copy)]
struct AttackBadgeData {
    profile: AttackProfile,
    damage_type: DamageType,
}

#[derive(Clone, Copy)]
struct ArmorBadgeData {
    armor_type: ArmorType,
    points_per_100: i32,
}

fn selected_combat_badges(
    id: SimId,
    samples: &PresentationSamples,
) -> ([Option<AttackBadgeData>; 2], Option<ArmorBadgeData>) {
    if let Some(unit) = samples.current.units.get(&id) {
        return (
            [
                Some(AttackBadgeData {
                    profile: unit.attack,
                    damage_type: unit.damage_type,
                }),
                unit.secondary_attack.map(|attack| AttackBadgeData {
                    profile: attack.attack,
                    damage_type: attack.damage_type,
                }),
            ],
            Some(ArmorBadgeData {
                armor_type: unit.armor.armor_type,
                points_per_100: unit.status.effective_armor_points_per_100(unit.armor),
            }),
        );
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        return (
            [
                building
                    .attack
                    .zip(building.damage_type)
                    .map(|(profile, damage_type)| AttackBadgeData {
                        profile,
                        damage_type,
                    }),
                None,
            ],
            Some(ArmorBadgeData {
                armor_type: building.armor.armor_type,
                points_per_100: i32::from(building.armor.armor_points) * 100,
            }),
        );
    }
    ([None; 2], None)
}

fn armor_reduction_label(points_per_100: i32, factor_per_10k: u16) -> String {
    let factor = f64::from(factor_per_10k) / 10_000.0;
    let armor = f64::from(points_per_100) / 100.0;
    let reduction = if armor >= 0.0 {
        1.0 - 1.0 / (1.0 + factor * armor)
    } else {
        // The authoritative negative-armor rule compounds per point, not linearly.
        (1.0 - factor).powf(-armor) - 1.0
    };
    if reduction >= 0.0 {
        format!("{:.0}% less", reduction * 100.0)
    } else {
        format!("{:.0}% more", -reduction * 100.0)
    }
}

fn attack_matchup_tooltip(kind: DamageType, samples: &PresentationSamples) -> String {
    let mut lines = vec![format!("{} damage against:", damage_type_name(kind))];
    for armor in [
        ArmorType::Small,
        ArmorType::Unarmored,
        ArmorType::Medium,
        ArmorType::Large,
        ArmorType::Hero,
        ArmorType::Fortified,
        ArmorType::Divine,
        ArmorType::Normal,
    ] {
        lines.push(format!(
            "{}: {}",
            armor_type_name(armor),
            percent_label(
                i32::from(samples.current.damage_rules.bonus_per_10k(kind, armor)),
                false
            )
        ));
    }
    lines.join("\n")
}

fn armor_matchup_tooltip(kind: ArmorType, samples: &PresentationSamples) -> String {
    let mut lines = vec![format!("{} armor receives:", armor_type_name(kind))];
    for damage in [
        DamageType::Normal,
        DamageType::Pierce,
        DamageType::Siege,
        DamageType::Magic,
        DamageType::Chaos,
        DamageType::Spells,
        DamageType::Hero,
    ] {
        lines.push(format!(
            "{}: {}",
            damage_type_name(damage),
            percent_label(
                i32::from(samples.current.damage_rules.bonus_per_10k(damage, kind)),
                false
            )
        ));
    }
    lines.join("\n")
}

fn update_debug_inspector_text(
    samples: Res<PresentationSamples>,
    selection: Res<InspectionSelection>,
    debug: Res<crate::presentation::DebugPresentation>,
    mut panel: Single<&mut Visibility, With<DebugInspectionPanel>>,
    mut text: Single<&mut Text, With<DebugInspectionText>>,
) {
    **panel = if debug.overlays && selection.selected.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Some(id) = selection.selected {
        text.0 = inspector_text(id, &samples);
    }
}

fn selection_summary(id: SimId, samples: &PresentationSamples) -> String {
    if let Some(builder) = samples.current.builders.get(&id) {
        return format!(
            "Player {} builder\n\n{}",
            builder.owner.0 + 1,
            if builder.build_footprint.is_some() {
                "Constructing"
            } else if builder.repair_target.is_some() {
                "Repairing"
            } else if builder.follow_target.is_some() {
                "Following"
            } else if builder.destination.is_some() {
                "Moving"
            } else {
                "Idle"
            }
        );
    }
    if let Some(unit) = samples.current.units.get(&id) {
        return format!("Player {}", unit.owner.0 + 1);
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        let status = if let Some(complete_tick) = building.construction_complete_tick {
            format!(
                "Constructing: {} ticks remaining",
                complete_tick.saturating_sub(samples.current.tick)
            )
        } else if building.production_queue == Some(0) {
            "Production stopped".into()
        } else if building.production_queue.is_some() {
            String::new()
        } else {
            "Ready".into()
        };
        let training_status =
            building
                .production_interval_ticks
                .map_or_else(String::new, |interval| {
                    let remaining = if building.production_queue == Some(0) {
                        0
                    } else {
                        building
                            .next_spawn_tick
                            .unwrap_or(0)
                            .saturating_sub(samples.current.tick)
                    };
                    format!(
                        "\nTraining: {} / {}s",
                        remaining.div_ceil(CASTLE_FIGHT_SIMULATION_HZ as u64),
                        u64::from(interval).div_ceil(CASTLE_FIGHT_SIMULATION_HZ as u64)
                    )
                });
        let status = if status.is_empty() {
            String::new()
        } else {
            format!("\n{status}")
        };
        let stun = building
            .stunned_until_tick
            .filter(|until| *until > samples.current.tick)
            .map_or_else(String::new, |until| {
                format!(
                    "\nDebuffs: Stunned ({}s)",
                    until
                        .saturating_sub(samples.current.tick)
                        .div_ceil(CASTLE_FIGHT_SIMULATION_HZ as u64)
                )
            });
        return format!(
            "{}{}{}{}",
            building.owner.map_or_else(
                || "Neutral".to_owned(),
                |owner| format!("Player {}", owner.0 + 1)
            ),
            training_status,
            status,
            stun
        );
    }
    "No selection.".into()
}

struct ActiveEffectBadgeData {
    icon: UiIconKey,
    description: String,
    beneficial: bool,
}

fn active_effect_badges(
    id: SimId,
    samples: &PresentationSamples,
    catalog: crate::ui_icons::CastleFightPresentationCatalog,
) -> Vec<ActiveEffectBadgeData> {
    let tick = samples.current.tick;
    let mut effects = Vec::new();
    let remaining = |expires_tick: u64| {
        expires_tick
            .saturating_sub(tick)
            .div_ceil(CASTLE_FIGHT_SIMULATION_HZ as u64)
    };
    if let Some(building) = samples.current.buildings.get(&id) {
        if let Some(until) = building.stunned_until_tick.filter(|until| *until > tick) {
            effects.push(ActiveEffectBadgeData {
                icon: catalog.stun_buff,
                description: format!("Stunned ({}s)", remaining(until)),
                beneficial: false,
            });
        }
        return effects;
    }
    let Some(unit) = samples.current.units.get(&id) else {
        return effects;
    };
    if unit.status.stunned_until_tick > tick {
        effects.push(ActiveEffectBadgeData {
            icon: catalog.stun_buff,
            description: format!("Stunned ({}s)", remaining(unit.status.stunned_until_tick)),
            beneficial: false,
        });
    }
    if let Some(ability) = unit.active_defend_ability {
        effects.push(ActiveEffectBadgeData {
            icon: UiIconKey::ability(ability.0, UiIconRole::Normal),
            description: "Defend".to_owned(),
            beneficial: true,
        });
    }
    if unit.status.permanent_holy_health_bonus {
        effects.push(ActiveEffectBadgeData {
            icon: catalog.permanent_holy_health,
            description: "Permanent Holy health bonus".to_owned(),
            beneficial: true,
        });
    }
    for modifier in unit.status.movement_modifiers
        [..usize::from(unit.status.movement_modifier_count)]
        .iter()
        .filter(|modifier| modifier.expires_tick > tick)
    {
        let label = format!(
            "Move {:+}% ({}s)",
            modifier.percent_delta,
            remaining(modifier.expires_tick)
        );
        effects.push(ActiveEffectBadgeData {
            icon: UiIconKey::StatusEffect {
                ability: modifier.id.0,
                role: UiStatusIconRole::Secondary,
            },
            description: label,
            beneficial: modifier.percent_delta >= 0,
        });
    }
    for modifier in unit.status.attack_speed_modifiers
        [..usize::from(unit.status.attack_speed_modifier_count)]
        .iter()
        .filter(|modifier| modifier.expires_tick > tick)
    {
        let label = format!(
            "Attack speed {:+}% ({}s)",
            modifier.percent_delta,
            remaining(modifier.expires_tick)
        );
        effects.push(ActiveEffectBadgeData {
            icon: UiIconKey::StatusEffect {
                ability: modifier.id.0,
                role: UiStatusIconRole::Secondary,
            },
            description: label,
            beneficial: modifier.percent_delta >= 0,
        });
    }
    for modifier in unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)]
        .iter()
        .filter(|modifier| modifier.expires_tick > tick)
    {
        let label = format!(
            "Armor {}{} ({}s)",
            if modifier.armor_bonus_per_100 >= 0 {
                "+"
            } else {
                ""
            },
            armor_points_label(i32::from(modifier.armor_bonus_per_100)),
            remaining(modifier.expires_tick)
        );
        effects.push(ActiveEffectBadgeData {
            icon: UiIconKey::StatusEffect {
                ability: modifier.id.0,
                role: UiStatusIconRole::Primary,
            },
            description: label,
            beneficial: modifier.armor_bonus_per_100 >= 0,
        });
    }
    for modifier in unit.status.damage_over_time[..usize::from(unit.status.damage_over_time_count)]
        .iter()
        .filter(|modifier| modifier.expires_tick > tick)
    {
        effects.push(ActiveEffectBadgeData {
            icon: UiIconKey::StatusEffect {
                ability: modifier.id.0,
                role: UiStatusIconRole::Primary,
            },
            description: format!(
                "{} damage/pulse ({}s)",
                modifier.damage_per_pulse,
                remaining(modifier.expires_tick)
            ),
            beneficial: false,
        });
    }
    effects
}

fn update_active_effect_icons(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    selected_match: Res<SelectedMatch>,
    asset_server: Res<AssetServer>,
    mut ui: ActiveEffectUi<'_, '_>,
) {
    let effects = (selection.members.len() == 1)
        .then_some(selection.selected)
        .flatten()
        .and_then(|id| {
            crate::ui_icons::CastleFightPresentationCatalog::for_version(
                selected_match.content.map_version,
            )
            .map(|catalog| active_effect_badges(id, &samples, catalog))
        })
        .unwrap_or_default();
    let mut hovered = None;
    for (slot, interaction, mut node, mut border) in &mut ui.buttons {
        let effect = effects.get(slot.0);
        node.display = if effect.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(effect) = effect {
            *border = BorderColor::all(if effect.beneficial {
                Color::srgb(0.25, 0.72, 0.35)
            } else {
                Color::srgb(0.78, 0.28, 0.25)
            });
            if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
                hovered = Some(effect.description.as_str());
            }
        }
    }
    for (slot, mut image) in &mut ui.images {
        *image = effects
            .get(slot.0)
            .and_then(|effect| match effect.icon {
                UiIconKey::StatusEffect { ability, role } => {
                    ui.icon_assets.status_image(ability, role, &asset_server)
                }
                icon => ui.icon_assets.image(icon, &asset_server),
            })
            .map_or_else(ImageNode::default, ImageNode::new);
    }
    let (text, visibility) = &mut *ui.tooltip;
    text.0 = hovered.unwrap_or_default().to_owned();
    **visibility = if hovered.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

fn update_selection_tiles(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    asset_server: Res<AssetServer>,
    mut ui: SelectionTileUi<'_, '_>,
) {
    let single = selection.members.len() <= 1;
    ui.tile_root.width = px(if single {
        134.0
    } else {
        8.0 * (TILE_SIZE + 3.0)
    });
    **ui.inventory = if single {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for (slot, mut node, mut visibility, mut border) in &mut ui.tiles {
        let size = if single && slot.0 == 0 {
            132.0
        } else {
            TILE_SIZE
        };
        node.width = px(size);
        node.height = px(size);
        *visibility = if slot.0 < selection.members.len() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        *border = BorderColor::all(
            if selection.members.get(slot.0).copied() == selection.selected {
                Color::srgb(0.95, 0.76, 0.22)
            } else {
                Color::srgb(0.40, 0.37, 0.25)
            },
        );
    }
    for (slot, mut image) in &mut ui.icons {
        let rawcode = selection.members.get(slot.0).and_then(|id| {
            samples
                .current
                .builders
                .get(id)
                .map(|builder| builder.appearance.rawcode)
                .or_else(|| {
                    samples
                        .current
                        .units
                        .get(id)
                        .and_then(|unit| unit.content.map(|content| content.rawcode))
                })
                .or_else(|| {
                    samples
                        .current
                        .buildings
                        .get(id)
                        .and_then(|building| building.content.map(|content| content.rawcode))
                })
        });
        *image = rawcode
            .and_then(|code| {
                ui.icon_assets
                    .image(UiIconKey::unit_game_interface(code), &asset_server)
            })
            .map_or_else(ImageNode::default, ImageNode::new);
    }
    for (slot, mut node) in &mut ui.bars {
        if single && slot.0 == 0 {
            node.width = px(0.0);
            continue;
        }
        let fraction = selection
            .members
            .get(slot.0)
            .and_then(|id| {
                samples
                    .current
                    .units
                    .get(id)
                    .map(|unit| (unit.health, unit.health_max))
                    .or_else(|| {
                        samples
                            .current
                            .buildings
                            .get(id)
                            .map(|building| (building.health, building.health_max))
                    })
            })
            .map_or(1.0, |(current, maximum)| {
                if maximum > 0 {
                    (current as f32 / maximum as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            });
        node.width = percent(fraction * 100.0);
    }
}

fn update_portrait_resources(
    selection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    mut bars: PortraitBarsQuery<'_, '_>,
    mut fills: PortraitFillsQuery<'_, '_>,
    mut labels: Query<(&PortraitResourceText, &mut Text)>,
    mut icons: PortraitIconsQuery<'_, '_>,
) {
    let resources = selection
        .members
        .first()
        .filter(|_| selection.members.len() == 1)
        .and_then(|id| {
            samples
                .current
                .units
                .get(id)
                .map(|unit| {
                    (
                        (unit.health, unit.health_max),
                        unit.mana_current.zip(unit.mana_maximum),
                    )
                })
                .or_else(|| {
                    samples.current.buildings.get(id).map(|building| {
                        (
                            (building.health, building.health_max),
                            (building.production_queue.is_none())
                                .then(|| building.mana_current.zip(building.mana_maximum))
                                .flatten(),
                        )
                    })
                })
        });
    for (slot, mut icon) in &mut icons {
        if slot.0 == 0 {
            icon.bottom = px(match resources {
                Some((_, Some(_))) => 29.0,
                Some(_) => 15.0,
                None => 5.0,
            });
        }
    }
    for (resource, mut visibility, mut node) in &mut bars {
        let values = resources.and_then(|(health, mana)| match resource {
            PortraitResource::Health => Some(health),
            PortraitResource::Mana => mana,
        });
        if *resource == PortraitResource::Health {
            node.bottom = px(if resources.is_some_and(|(_, mana)| mana.is_some()) {
                15.0
            } else {
                1.0
            });
        }
        *visibility = if values.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (resource, mut fill) in &mut fills {
        let values = resources.and_then(|(health, mana)| match resource.0 {
            PortraitResource::Health => Some(health),
            PortraitResource::Mana => mana,
        });
        if let Some((current, maximum)) = values {
            fill.width = percent(if maximum > 0 {
                (current as f32 / maximum as f32).clamp(0.0, 1.0) * 100.0
            } else {
                0.0
            });
        }
    }
    for (resource, mut text) in &mut labels {
        let values = resources.and_then(|(health, mana)| match resource.0 {
            PortraitResource::Health => Some(health),
            PortraitResource::Mana => mana,
        });
        if let Some((current, maximum)) = values {
            text.0 = format!("{current} / {maximum}");
        }
    }
}

fn handle_selection_tile_click(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    interactions: SelectionTileInteractionQuery<'_, '_>,
    mut selection: ResMut<InspectionSelection>,
    mut camera_focus: ResMut<crate::presentation::CameraFocusRequest>,
    mut portrait_hold: ResMut<PortraitCameraHold>,
) {
    if !mouse.pressed(MouseButton::Left) {
        portrait_hold.0 = None;
    }
    for (slot, interaction) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(&id) = selection.members.get(slot.0) else {
            continue;
        };
        if selection.members.len() == 1 && slot.0 == 0 {
            camera_focus.0 = Some(id);
            portrait_hold.0 = Some(id);
        }
        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            selection.toggle(id);
        } else {
            selection.focus(id);
        }
    }
}

fn update_selection_rectangle(
    drag: Res<SelectionDrag>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut rectangle: Single<(&mut Node, &mut Visibility), With<SelectionRectangle>>,
) {
    let (node, visibility) = &mut *rectangle;
    if let (Some(start), Some(current)) = (drag.start, drag.current)
        && mouse.pressed(MouseButton::Left)
        && start.distance(current) >= DRAG_THRESHOLD
    {
        let min = start.min(current);
        let max = start.max(current);
        node.left = px(min.x);
        node.top = px(min.y);
        node.width = px(max.x - min.x);
        node.height = px(max.y - min.y);
        **visibility = Visibility::Visible;
    } else {
        **visibility = Visibility::Hidden;
    }
}

fn draw_selection_highlight(
    world: (Res<Time<Fixed>>, Res<WorldMetrics>, Res<TerrainSurface>),
    playback: Res<SimulationPlayback>,
    samples: Res<PresentationSamples>,
    selection: Res<InspectionSelection>,
    debug: Res<DebugPresentation>,
    mut gizmos: Gizmos,
) {
    let (fixed_time, metrics, terrain) = world;
    let alpha = playback.interpolation_alpha(&fixed_time);
    for &id in &selection.members {
        if let Some(builder) = samples.current.builders.get(&id) {
            let previous = samples.previous.builders.get(&id).unwrap_or(builder);
            let center = sim_point_to_terrain_world_lerp(
                previous.position,
                builder.position,
                alpha,
                &terrain,
            );
            gizmos.circle(
                Isometry3d::new(
                    center + Vec3::Y * 0.35,
                    Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
                ),
                BUILDER_PICK_RADIUS + SELECTION_RING_PADDING,
                SELECTION_COLOR,
            );
            if debug.overlays
                && let Some(target) = builder.repair_target
                && let Some(target_position) =
                    current_entity_position(target, &samples, &metrics, &terrain)
            {
                gizmos.line(
                    center + Vec3::Y * 4.0,
                    target_position + Vec3::Y * 4.0,
                    SELECTION_COLOR.with_alpha(0.55),
                );
            }
            if debug.overlays
                && let Some(footprint) = builder.build_footprint
            {
                draw_footprint_outline(
                    &mut gizmos,
                    &metrics,
                    &terrain,
                    footprint,
                    SELECTION_COLOR.with_alpha(0.75),
                );
            }
            continue;
        }

        if let Some(unit) = samples.current.units.get(&id) {
            let previous = samples.previous.units.get(&id).unwrap_or(unit);
            let center =
                sim_point_to_terrain_world_lerp(previous.position, unit.position, alpha, &terrain)
                    + Vec3::Y * unit_visual_altitude(unit.movement_class);
            let radius = unit_pick_radius(unit) + SELECTION_RING_PADDING;
            gizmos.circle(
                Isometry3d::new(
                    center + Vec3::Y * 0.35,
                    Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
                ),
                radius,
                SELECTION_COLOR,
            );
            if debug.overlays
                && let Some(target) = unit.target
                && let Some(target_position) =
                    current_entity_position(target, &samples, &metrics, &terrain)
            {
                gizmos.line(
                    center + Vec3::Y * 4.0,
                    target_position + Vec3::Y * 4.0,
                    SELECTION_COLOR.with_alpha(0.55),
                );
            }
            continue;
        }

        if let Some(building) = samples.current.buildings.get(&id) {
            draw_footprint_outline(
                &mut gizmos,
                &metrics,
                &terrain,
                building.footprint,
                SELECTION_COLOR,
            );
            if debug.overlays
                && let Some(target) = building.target
                && let Some(target_position) =
                    current_entity_position(target, &samples, &metrics, &terrain)
            {
                let (mut center, _) = metrics.footprint_center_size(building.footprint);
                center.y = terrain.height_at_world(center.xz());
                gizmos.line(
                    center + Vec3::Y * 6.0,
                    target_position + Vec3::Y * 4.0,
                    SELECTION_COLOR.with_alpha(0.55),
                );
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn pick_builder_on_ray(
    ray_origin: Vec3,
    ray_direction: Vec3,
    samples: &PresentationSamples,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<SimId> {
    let mut nearest: Option<(f32, SimId)> = None;
    for builder in samples.current.builders.values() {
        let previous = samples
            .previous
            .builders
            .get(&builder.id)
            .unwrap_or(builder);
        let ground =
            sim_point_to_terrain_world_lerp(previous.position, builder.position, alpha, terrain);
        let center = ground + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5);
        let Some(distance) = ray_sphere_hit_distance(
            ray_origin,
            ray_direction,
            center,
            BUILDER_PICK_RADIUS.max(BUILDER_PICK_HEIGHT * 0.55),
        ) else {
            continue;
        };
        match nearest {
            Some((nearest_distance, _)) if nearest_distance <= distance => {}
            _ => nearest = Some((distance, builder.id)),
        }
    }
    nearest.map(|(_, id)| id)
}

pub(crate) fn pick_unit_on_ray(
    ray_origin: Vec3,
    ray_direction: Vec3,
    samples: &PresentationSamples,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<SimId> {
    let mut nearest: Option<(f32, SimId)> = None;
    for unit in samples.current.units.values() {
        let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
        let center =
            unit_visual_center_lerp(previous.position, unit.position, unit, alpha, terrain);
        let radius = unit_pick_radius(unit).max(unit_height(unit) * 0.55);
        let Some(distance) = ray_sphere_hit_distance(ray_origin, ray_direction, center, radius)
        else {
            continue;
        };
        match nearest {
            Some((nearest_distance, _)) if nearest_distance <= distance => {}
            _ => nearest = Some((distance, unit.id)),
        }
    }
    nearest.map(|(_, id)| id)
}

pub(crate) fn pick_building_at_ground(
    world: Vec3,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
) -> Option<SimId> {
    samples
        .current
        .buildings
        .values()
        .find(|building| point_inside_building(world, building, metrics))
        .map(|building| building.id)
}

fn ray_sphere_hit_distance(
    ray_origin: Vec3,
    ray_direction: Vec3,
    center: Vec3,
    radius: f32,
) -> Option<f32> {
    let direction = ray_direction.normalize_or_zero();
    if direction == Vec3::ZERO {
        return None;
    }
    let offset = ray_origin - center;
    let projected = offset.dot(direction);
    let discriminant = projected * projected - (offset.length_squared() - radius * radius);
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let near = -projected - root;
    let far = -projected + root;
    if near >= 0.0 {
        Some(near)
    } else if far >= 0.0 {
        Some(far)
    } else {
        None
    }
}

fn point_inside_building(world: Vec3, building: &BuildingSample, metrics: &WorldMetrics) -> bool {
    let (center, size) = metrics.footprint_center_size(building.footprint);
    let half = size * 0.5;
    world.x >= center.x - half.x
        && world.x <= center.x + half.x
        && world.z >= center.z - half.y
        && world.z <= center.z + half.y
}

fn unit_pick_radius(unit: &UnitSample) -> f32 {
    (unit.collision_radius as f32 / SUBUNITS_PER_WORLD_UNIT as f32).max(MIN_UNIT_PICK_RADIUS)
}

fn current_entity_position(
    id: SimId,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
) -> Option<Vec3> {
    if let Some(builder) = samples.current.builders.get(&id) {
        return Some(sim_point_to_terrain_world(builder.position, terrain));
    }
    if let Some(unit) = samples.current.units.get(&id) {
        return Some(
            sim_point_to_terrain_world(unit.position, terrain)
                + Vec3::Y * unit_visual_altitude(unit.movement_class),
        );
    }
    samples.current.buildings.get(&id).map(|building| {
        let (mut center, _) = metrics.footprint_center_size(building.footprint);
        center.y = terrain.height_at_world(center.xz());
        center
    })
}

fn inspector_text(id: SimId, samples: &PresentationSamples) -> String {
    if let Some(builder) = samples.current.builders.get(&id) {
        return format_builder_inspector(builder);
    }
    if let Some(unit) = samples.current.units.get(&id) {
        return format_unit_inspector(unit, samples.current.tick, samples);
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        return format_building_inspector(building, samples.current.tick);
    }
    format!("SimId {} is no longer present.", id.0)
}

fn format_builder_inspector(builder: &BuilderSample) -> String {
    let position = sim_point_to_world(builder.position);
    let order = if let Some(footprint) = builder.build_footprint {
        format!(
            "Build footprint {},{} {}x{}",
            footprint.min_x, footprint.min_y, footprint.width, footprint.height
        )
    } else if let Some(destination) = builder.destination {
        let destination = sim_point_to_world(destination);
        format!("Move to {:.1}, {:.1}", destination.x, destination.z)
    } else if let Some(target) = builder.repair_target {
        format!("Repair #{}", target.0)
    } else if let Some(target) = builder.follow_target {
        format!("Follow #{}", target.0)
    } else {
        "Idle".into()
    };
    [
        format!("BUILDER #{}", builder.id.0),
        format!("Name: {}", builder.appearance.name),
        format!("Side: {}", team_label(builder.team)),
        format!("Locomotion: {:?}", builder.locomotion),
        format!("Position: {:.1}, {:.1}", position.x, position.z),
        format!("Order: {order}"),
        format!(
            "Repair autocast: {}",
            if builder.repair_autocast_enabled {
                "On"
            } else {
                "Off"
            }
        ),
        format!("Build menu: {} entries", builder.build_catalog_len),
        format!(
            "Blink range: {:.0}",
            builder.blink_range as f32 / SUBUNITS_PER_WORLD_UNIT as f32
        ),
    ]
    .join("\n")
}

fn format_unit_inspector(unit: &UnitSample, tick: u64, samples: &PresentationSamples) -> String {
    let position = sim_point_to_world(unit.position);
    let mut lines = vec![
        format!("UNIT #{}", unit.id.0),
        format!(
            "Name: {}",
            unit.content.map_or("Unknown", |content| content.name)
        ),
        format!("Side: {}", team_label(unit.team)),
        format!("Type: {}", unit_kind_label(unit.visual_kind)),
        format!("Movement: {:?}", unit.movement_class),
        format!("Health: {}", unit.health),
        format!("Position: {:.1}, {:.1}", position.x, position.z),
        format!("Order: {}", unit_order_label(unit, tick)),
        format!("Target: {}", target_label(unit.target)),
        format!(
            "Direct retaliation lock: {}",
            if unit.direct_retaliation_lock {
                "Yes"
            } else {
                "No"
            }
        ),
        format!(
            "Ally defense lock: {}",
            if unit.ally_defense_lock { "Yes" } else { "No" }
        ),
        attack_type_label(unit, samples),
        defense_type_label(unit, samples),
        format!("Attack cooldown: {} ticks", unit.cooldown_remaining),
        format!("State: {}", stun_label(unit.stunned_until_tick, tick)),
    ];
    if let (Some(current), Some(maximum)) = (unit.mana_current, unit.mana_maximum) {
        lines.push(format!("Mana: {current}/{maximum}"));
    }
    lines.join("\n")
}

fn format_building_inspector(building: &BuildingSample, tick: u64) -> String {
    let mut lines = vec![
        format!("BUILDING #{}", building.id.0),
        format!(
            "Name: {}",
            building.content.map_or("Unknown", |content| content.name)
        ),
        format!("Side: {}", team_label(building.team)),
        format!("Type: {}", building_kind_label(building.visual_kind)),
        format!("Health: {}", building.health),
        format!(
            "Footprint: {}x{} cells at {}, {}",
            building.footprint.width,
            building.footprint.height,
            building.footprint.min_x,
            building.footprint.min_y
        ),
        format!("Target: {}", target_label(building.target)),
    ];
    if let (Some(started_tick), Some(complete_tick)) = (
        building.construction_started_tick,
        building.construction_complete_tick,
    ) {
        let duration = complete_tick.saturating_sub(started_tick).max(1);
        let elapsed = tick.saturating_sub(started_tick).min(duration);
        lines.push(format!(
            "Construction: {}% ({} ticks remaining)",
            elapsed.saturating_mul(100) / duration,
            complete_tick.saturating_sub(tick)
        ));
    }
    if let Some(next_spawn_tick) = building.next_spawn_tick {
        lines.push(format!(
            "Next spawn: {} ticks",
            next_spawn_tick.saturating_sub(tick)
        ));
    }
    if let Some(cooldown) = building.cooldown_remaining {
        lines.push(format!("Attack cooldown: {cooldown} ticks"));
    }
    if let (Some(current), Some(maximum)) = (building.mana_current, building.mana_maximum) {
        lines.push(format!("Mana: {current}/{maximum}"));
    }
    if let Some(enabled) = building.ability_autocast_enabled {
        lines.push(format!(
            "Spell autocast: {}",
            if enabled { "On" } else { "Off" }
        ));
    }
    if let Some(ready_tick) = building.ability_ready_tick {
        lines.push(format!(
            "Ability ready: {} ticks",
            ready_tick.saturating_sub(tick)
        ));
    }
    if let Some(stunned_until) = building.stunned_until_tick {
        lines.push(format!("State: {}", stun_label(stunned_until, tick)));
    }
    lines.join("\n")
}

fn attack_type_label(unit: &UnitSample, samples: &PresentationSamples) -> String {
    let attack_type = damage_type_name(unit.damage_type);
    let Some(target) = unit.target else {
        return format!("Attack type: {attack_type}");
    };
    let Some(defense_type) = target_armor_type(target, samples) else {
        return format!("Attack type: {attack_type} -> target: unknown defense");
    };
    let multiplier = samples
        .current
        .damage_rules
        .bonus_per_10k(unit.damage_type, defense_type);
    format!(
        "Attack type: {attack_type} -> {}: {}",
        armor_type_name(defense_type),
        matchup_multiplier_label(multiplier)
    )
}

fn defense_type_label(unit: &UnitSample, samples: &PresentationSamples) -> String {
    let defense_type = unit.armor.armor_type;
    let armor_points = armor_points_label(unit.status.effective_armor_points_per_100(unit.armor));
    let defense_name = format!("{} ({armor_points})", armor_type_name(defense_type));
    let Some(target) = unit.target else {
        return format!("Defense type: {defense_name}");
    };
    let Some(attack_type) = target_damage_type(target, samples) else {
        return format!("Defense type: {defense_name} <- target: no attack");
    };
    let multiplier = samples
        .current
        .damage_rules
        .bonus_per_10k(attack_type, defense_type);
    format!(
        "Defense type: {defense_name} <- {}: {} incoming",
        damage_type_name(attack_type),
        matchup_multiplier_label(multiplier)
    )
}

fn target_armor_type(target: SimId, samples: &PresentationSamples) -> Option<ArmorType> {
    samples
        .current
        .units
        .get(&target)
        .map(|unit| unit.armor.armor_type)
        .or_else(|| {
            samples
                .current
                .buildings
                .get(&target)
                .map(|building| building.armor.armor_type)
        })
}

fn target_damage_type(target: SimId, samples: &PresentationSamples) -> Option<DamageType> {
    samples
        .current
        .units
        .get(&target)
        .map(|unit| unit.damage_type)
        .or_else(|| {
            samples
                .current
                .buildings
                .get(&target)
                .and_then(|building| building.damage_type)
        })
}

fn matchup_multiplier_label(multiplier_per_10k: u16) -> String {
    let multiplier = percent_label(i32::from(multiplier_per_10k), false);
    let delta = percent_label(i32::from(multiplier_per_10k) - 10_000, true);
    format!("{multiplier} ({delta})")
}

fn armor_points_label(points_per_100: i32) -> String {
    let sign = if points_per_100 < 0 { "-" } else { "" };
    let magnitude = points_per_100.unsigned_abs();
    let whole = magnitude / 100;
    let hundredths = magnitude % 100;
    if hundredths == 0 {
        format!("{sign}{whole}")
    } else if hundredths.is_multiple_of(10) {
        format!("{sign}{whole}.{}", hundredths / 10)
    } else {
        format!("{sign}{whole}.{hundredths:02}")
    }
}

fn percent_label(per_10k: i32, force_sign: bool) -> String {
    let sign = if per_10k < 0 {
        "-"
    } else if force_sign && per_10k > 0 {
        "+"
    } else {
        ""
    };
    let magnitude = per_10k.unsigned_abs();
    let whole = magnitude / 100;
    let hundredths = magnitude % 100;
    if hundredths == 0 {
        format!("{sign}{whole}%")
    } else if hundredths.is_multiple_of(10) {
        format!("{sign}{whole}.{}%", hundredths / 10)
    } else {
        format!("{sign}{whole}.{hundredths:02}%")
    }
}

fn damage_type_name(damage_type: DamageType) -> &'static str {
    match damage_type {
        DamageType::Normal => "Normal",
        DamageType::Pierce => "Pierce",
        DamageType::Siege => "Siege",
        DamageType::Magic => "Magic",
        DamageType::Chaos => "Chaos",
        DamageType::Spells => "Spell",
        DamageType::Hero => "Hero",
    }
}

fn armor_type_name(armor_type: ArmorType) -> &'static str {
    match armor_type {
        ArmorType::Small => "Light",
        ArmorType::Medium => "Medium",
        ArmorType::Large => "Heavy",
        ArmorType::Fortified => "Fortified",
        ArmorType::Normal => "Normal",
        ArmorType::Hero => "Hero",
        ArmorType::Divine => "Divine",
        ArmorType::Unarmored => "Unarmored",
    }
}

fn unit_order_label(unit: &UnitSample, tick: u64) -> String {
    if unit.stunned_until_tick > tick {
        return "Disabled/stunned".into();
    }
    match (
        unit.target,
        unit.direct_retaliation_lock,
        unit.ally_defense_lock,
    ) {
        (Some(target), true, _) => format!("Direct retaliation against #{}", target.0),
        (Some(target), false, true) => format!("Defending ally against #{}", target.0),
        (Some(target), false, false) => format!("Engaging target #{}", target.0),
        (None, _, _) => "Advancing toward enemy objective".into(),
    }
}

fn target_label(target: Option<SimId>) -> String {
    target.map_or_else(|| "None".into(), |target| format!("#{}", target.0))
}

fn stun_label(stunned_until_tick: u64, tick: u64) -> String {
    if stunned_until_tick > tick {
        format!("Stunned ({} ticks)", stunned_until_tick - tick)
    } else {
        "Active".into()
    }
}

fn unit_kind_label(kind: UnitVisualKind) -> &'static str {
    match kind {
        UnitVisualKind::Melee => "Melee",
        UnitVisualKind::Ranged => "Ranged",
        UnitVisualKind::Ballistic => "Ballistic ranged",
        UnitVisualKind::Bounce => "Bounce ranged",
        UnitVisualKind::MeleeCaster => "Melee caster",
        UnitVisualKind::RangedCaster => "Ranged caster",
        UnitVisualKind::BallisticCaster => "Ballistic caster",
        UnitVisualKind::BounceCaster => "Bounce caster",
    }
}

fn building_kind_label(kind: BuildingVisualKind) -> &'static str {
    match kind {
        BuildingVisualKind::Structure => "Structure",
        BuildingVisualKind::Production => "Production",
        BuildingVisualKind::Attack => "Attack",
        BuildingVisualKind::Spellcaster => "Spellcaster",
        BuildingVisualKind::ProductionAttack => "Production + attack",
        BuildingVisualKind::ProductionSpellcaster => "Production + spellcaster",
        BuildingVisualKind::AttackSpellcaster => "Attack + spellcaster",
        BuildingVisualKind::ProductionAttackSpellcaster => "Production + attack + spellcaster",
    }
}

fn team_label(team: Team) -> &'static str {
    match team.0 {
        0 => "Blue",
        1 => "Red",
        _ => "Unknown",
    }
}

pub(crate) fn cursor_over_inspector_panel(
    cursor: Vec2,
    window_width: f32,
    window_height: f32,
) -> bool {
    cursor.x >= 0.0
        && cursor.x <= window_width
        && cursor.y >= window_height - CONSOLE_HEIGHT
        && cursor.y <= window_height
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bevy::ecs::system::SystemState;
    use castle_fight_sim::{
        BuilderLocomotion, BuildingFootprint, ContentIdentity, MovementClass, NavCell,
        PlayerConnectionStatus, PlayerEconomyView, PlayerId, PlayerResources, PlayerView, SimPoint,
        SimulationConfig, Team, TerrainElevationMap,
    };

    use super::*;
    use crate::bridge::PresentationSnapshot;

    #[test]
    fn selection_tile_ui_queries_are_disjoint_at_runtime() {
        let mut world = World::new();
        let _state = SystemState::<SelectionTileUi<'_, '_>>::new(&mut world);
        let _production = SystemState::<ProductionUi<'_, '_>>::new(&mut world);
        let _combat = SystemState::<(
            Query<(&CombatTypeButton, &Interaction, &mut Visibility), Without<CombatTooltip>>,
            Query<(&CombatTypeLabel, &mut Text), Without<CombatTooltip>>,
            CombatTooltipQuery<'_, '_>,
        )>::new(&mut world);
    }

    #[test]
    fn combat_tooltip_uses_active_castle_fight_damage_rules() {
        let mut samples = empty_samples();
        samples.current.damage_rules = castle_fight_sim::castle_fight_damage_rules();
        let attack = attack_matchup_tooltip(DamageType::Pierce, &samples);
        assert!(attack.contains("Light: 175%"));
        let armor = armor_matchup_tooltip(ArmorType::Small, &samples);
        assert!(armor.contains("Pierce: 175%"));
    }

    #[test]
    fn selection_caps_at_twenty_four_and_shift_toggle_preserves_order() {
        let mut selection = InspectionSelection::default();
        selection.replace((1..=30).map(SimId));
        assert_eq!(selection.members.len(), MAX_SELECTION);
        assert_eq!(selection.selected, Some(SimId(1)));
        assert_eq!(selection.members.last(), Some(&SimId(24)));

        selection.toggle(SimId(1));
        assert_eq!(selection.selected, Some(SimId(2)));
        selection.toggle(SimId(1));
        assert_eq!(selection.members.last(), Some(&SimId(1)));
        selection.add([SimId(2), SimId(31)]);
        assert_eq!(selection.members.len(), MAX_SELECTION);
        assert!(!selection.members.contains(&SimId(31)));
    }

    #[test]
    fn drag_selection_falls_back_from_builders_to_buildings_to_units() {
        assert_eq!(
            prioritized_box_selection(vec![SimId(1)], vec![SimId(2)], vec![SimId(3)]),
            vec![SimId(1)]
        );
        assert_eq!(
            prioritized_box_selection(Vec::new(), vec![SimId(2)], vec![SimId(3)]),
            vec![SimId(2)]
        );
        assert_eq!(
            prioritized_box_selection(Vec::new(), Vec::new(), vec![SimId(3)]),
            vec![SimId(3)]
        );
    }

    fn metrics() -> WorldMetrics {
        WorldMetrics::from_simulation_config(&SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(100, 100),
            ..SimulationConfig::default()
        })
    }

    fn flat_terrain() -> TerrainSurface {
        TerrainSurface::new(
            TerrainElevationMap::from_vertex_samples(
                SimPoint::new(0, 0),
                10 * SUBUNITS_PER_WORLD_UNIT,
                20,
                20,
                vec![2; 21 * 21],
                vec![0x2000; 21 * 21],
            )
            .unwrap(),
        )
    }

    fn test_attack() -> AttackProfile {
        AttackProfile {
            delivery: castle_fight_sim::AttackDelivery::Melee,
            damage: 25,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 500 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 30,
        }
    }

    fn empty_samples() -> PresentationSamples {
        let snapshot = PresentationSnapshot {
            tick: 10,
            damage_rules: castle_fight_sim::DamageRules::warcraft_frozen_throne(),
            players: BTreeMap::from([(
                PlayerId(0),
                PlayerView {
                    id: PlayerId(0),
                    team: Team(0),
                    resources: PlayerResources::default(),
                    connection: PlayerConnectionStatus::Connected,
                },
            )]),
            player_economy: BTreeMap::from([(
                PlayerId(0),
                PlayerEconomyView {
                    resources: PlayerResources::default(),
                    income: 0,
                    income_interval_ticks: 0,
                    income_progress_per_10k: 0,
                    ticks_until_income: 0,
                },
            )]),
            units: BTreeMap::new(),
            builders: BTreeMap::new(),
            buildings: BTreeMap::new(),
            corpses: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            attacks: Vec::new(),
            ability_casts: Vec::new(),
            chain_lightnings: Vec::new(),
            shrine_revivals: Vec::new(),
        };
        PresentationSamples::new(snapshot)
    }

    #[test]
    fn picking_and_inspection_include_builder() {
        let mut samples = empty_samples();
        samples.current.builders.insert(
            SimId(5),
            BuilderSample {
                id: SimId(5),
                owner: PlayerId(0),
                team: Team(0),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                appearance: ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"X00C"),
                    name: "Human Builder",
                },
                locomotion: BuilderLocomotion::Foot,
                destination: None,
                follow_target: None,
                repair_target: None,
                build_footprint: None,
                repair_autocast_enabled: true,
                blink_range: 10_000 * SUBUNITS_PER_WORLD_UNIT,
                build_catalog_len: 7,
            },
        );
        let terrain = flat_terrain();
        let center =
            sim_point_to_terrain_world(samples.current.builders[&SimId(5)].position, &terrain)
                + Vec3::Y * (BUILDER_PICK_HEIGHT * 0.5);
        assert_eq!(
            pick_builder_on_ray(
                // A click 40 world units off the model center should still land inside the
                // Peasant-sized selection footprint.
                Vec3::new(center.x + 40.0, center.y, 0.0),
                Vec3::Z,
                &samples,
                &terrain,
                1.0,
            ),
            Some(SimId(5))
        );
        let text = inspector_text(SimId(5), &samples);
        assert!(text.contains("Human Builder"));
        assert!(text.contains("Repair autocast: On"));
        assert!(text.contains("Build menu: 7 entries"));
        assert!(!text.contains("Controls:"));
        assert!(!text.contains("D Blink"));
    }

    #[test]
    fn picking_prefers_nearby_unit() {
        let mut samples = empty_samples();
        samples.current.units.insert(
            SimId(7),
            UnitSample {
                id: SimId(7),
                content: Some(ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"hfoo"),
                    name: "Footman",
                }),
                owner: PlayerId(0),
                team: Team(0),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                collision_radius: 4 * SUBUNITS_PER_WORLD_UNIT,
                movement_class: castle_fight_sim::MovementClass::Ground,
                mechanical: false,
                health: 50,
                health_max: 100,
                attack: test_attack(),
                secondary_attack: None,
                damage_type: DamageType::Normal,
                armor: castle_fight_sim::ArmorProfile::new(ArmorType::Medium, 2),
                target: None,
                direct_retaliation_lock: false,
                ally_defense_lock: false,
                cooldown_remaining: 0,
                stunned_until_tick: 0,
                status: castle_fight_sim::StatusState::default(),
                mana_current: None,
                mana_maximum: None,
                visual_kind: UnitVisualKind::Melee,
                active_defend_ability: None,
            },
        );
        let terrain = flat_terrain();
        assert_eq!(
            pick_unit_on_ray(Vec3::new(103.0, 5.0, 0.0), Vec3::Z, &samples, &terrain, 1.0,),
            Some(SimId(7))
        );
        assert!(
            format_unit_inspector(&samples.current.units[&SimId(7)], 10, &samples)
                .contains("Name: Footman")
        );

        let mut target = samples.current.units[&SimId(7)];
        target.id = SimId(8);
        target.owner = PlayerId(6);
        target.team = Team(1);
        target.damage_type = DamageType::Normal;
        target.armor = castle_fight_sim::ArmorProfile::new(ArmorType::Small, 0);
        target.target = Some(SimId(7));
        samples.current.units.insert(SimId(8), target);
        let selected = samples.current.units.get_mut(&SimId(7)).unwrap();
        selected.damage_type = DamageType::Pierce;
        selected.armor = castle_fight_sim::ArmorProfile::new(ArmorType::Small, 2);
        selected.status.armor_modifiers[0].armor_bonus_per_100 = 100;
        selected.status.armor_modifiers[0].expires_tick = 20;
        selected.status.armor_modifier_count = 1;
        selected.target = Some(SimId(8));
        let mut second = test_attack();
        second.damage = 13;
        selected.secondary_attack = Some(castle_fight_sim::SecondaryAttackProfile {
            primary_targets: castle_fight_sim::AttackTargetMask::GROUND_UNITS,
            attack: second,
            targets: castle_fight_sim::AttackTargetMask::BUILDINGS,
            damage_type: DamageType::Siege,
        });

        let (attacks, armor) = selected_combat_badges(SimId(7), &samples);
        assert_eq!(attacks[0].unwrap().profile.damage, 25);
        assert_eq!(attacks[0].unwrap().damage_type, DamageType::Pierce);
        assert_eq!(attacks[1].unwrap().profile.damage, 13);
        assert_eq!(attacks[1].unwrap().damage_type, DamageType::Siege);
        assert_eq!(armor.unwrap().points_per_100, 300);

        let text = format_unit_inspector(&samples.current.units[&SimId(7)], 10, &samples);
        assert!(text.contains("Attack type: Pierce -> Light: 200% (+100%)"));
        assert!(text.contains("Defense type: Light (3) <- Normal: 100% (0%) incoming"));
        assert!(!text.contains("Last attacker:"));
        assert!(!text.contains("Last attacked:"));
        assert_eq!(selected_entity_name(SimId(7), &samples), Some("Footman"));
        assert!(!selection_summary(SimId(7), &samples).contains("Footman"));

        let enemy = samples.current.units.get_mut(&SimId(8)).unwrap();
        enemy.status.stunned_until_tick = 16;
        enemy.status.movement_modifiers[0].percent_delta = -25;
        enemy.status.movement_modifiers[0].expires_tick = 40;
        enemy.status.movement_modifier_count = 1;
        let summary = selection_summary(SimId(8), &samples);
        assert!(summary.contains("Player 7"));
        let effects = active_effect_badges(
            SimId(8),
            &samples,
            crate::ui_icons::CastleFightPresentationCatalog::for_version(
                castle_fight_sim::MapVersion::CASTLE_FIGHT_9_27,
            )
            .unwrap(),
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.description == "Stunned (1s)")
        );
        assert!(
            effects
                .iter()
                .any(|effect| effect.description == "Move -25% (1s)")
        );
    }

    #[test]
    fn combat_type_labels_use_castle_fight_player_facing_names() {
        assert_eq!(armor_type_name(ArmorType::Small), "Light");
        assert_eq!(armor_type_name(ArmorType::Medium), "Medium");
        assert_eq!(armor_type_name(ArmorType::Large), "Heavy");
        assert_eq!(armor_type_name(ArmorType::Fortified), "Fortified");
        assert_eq!(armor_type_name(ArmorType::Normal), "Normal");
        assert_eq!(armor_type_name(ArmorType::Hero), "Hero");
        assert_eq!(armor_type_name(ArmorType::Divine), "Divine");
        assert_eq!(armor_type_name(ArmorType::Unarmored), "Unarmored");

        assert_eq!(damage_type_name(DamageType::Normal), "Normal");
        assert_eq!(damage_type_name(DamageType::Pierce), "Pierce");
        assert_eq!(damage_type_name(DamageType::Siege), "Siege");
        assert_eq!(damage_type_name(DamageType::Magic), "Magic");
        assert_eq!(damage_type_name(DamageType::Chaos), "Chaos");
        assert_eq!(damage_type_name(DamageType::Spells), "Spell");
        assert_eq!(damage_type_name(DamageType::Hero), "Hero");

        assert_eq!(armor_points_label(300), "3");
        assert_eq!(armor_points_label(350), "3.5");
        assert_eq!(armor_points_label(-125), "-1.25");
        assert_eq!(armor_reduction_label(0, 600), "0% less");
        assert_eq!(armor_reduction_label(300, 600), "15% less");
        assert_eq!(armor_reduction_label(-100, 600), "6% more");
    }

    #[test]
    fn picking_building_uses_authoritative_footprint() {
        let mut samples = empty_samples();
        samples.current.buildings.insert(
            SimId(9),
            BuildingSample {
                id: SimId(9),
                content: Some(ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"h000"),
                    name: "Barracks",
                }),
                owner: Some(PlayerId(6)),
                team: Team(1),
                footprint: BuildingFootprint::new(10, 20, 4, 4),
                health: 1_000,
                health_max: 1_000,
                construction_started_tick: None,
                construction_complete_tick: None,
                attack: None,
                damage_type: None,
                armor: castle_fight_sim::ArmorProfile::new(ArmorType::Fortified, 5),
                target: None,
                next_spawn_tick: Some(20),
                production_queue: Some(2),
                production_interval_ticks: Some(20),
                cooldown_remaining: None,
                mana_current: None,
                mana_maximum: None,
                ability_ready_tick: None,
                ability_autocast_enabled: None,
                stunned_until_tick: None,
                visual_kind: BuildingVisualKind::Production,
            },
        );
        assert_eq!(
            pick_building_at_ground(Vec3::new(120.0, 0.0, 220.0), &samples, &metrics()),
            Some(SimId(9))
        );
        assert_eq!(
            pick_building_at_ground(Vec3::new(145.0, 0.0, 220.0), &samples, &metrics()),
            None
        );
        assert!(
            format_building_inspector(&samples.current.buildings[&SimId(9)], 10)
                .contains("Name: Barracks")
        );
        assert!(selection_summary(SimId(9), &samples).contains("Player 7"));
        let (attacks, armor) = selected_combat_badges(SimId(9), &samples);
        assert!(attacks.into_iter().all(|attack| attack.is_none()));
        assert_eq!(armor.unwrap().armor_type, ArmorType::Fortified);
    }

    #[test]
    fn ray_picking_hits_flying_unit_at_rendered_altitude() {
        let mut samples = empty_samples();
        samples.current.units.insert(
            SimId(11),
            UnitSample {
                id: SimId(11),
                content: Some(ContentIdentity {
                    rawcode: u32::from_be_bytes(*b"h016"),
                    name: "Gryphon Rider",
                }),
                owner: PlayerId(6),
                team: Team(1),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                collision_radius: 4 * SUBUNITS_PER_WORLD_UNIT,
                movement_class: MovementClass::Air,
                mechanical: false,
                health: 50,
                health_max: 100,
                attack: test_attack(),
                secondary_attack: None,
                damage_type: DamageType::Normal,
                armor: castle_fight_sim::ArmorProfile::new(ArmorType::Large, 4),
                target: None,
                direct_retaliation_lock: false,
                ally_defense_lock: false,
                cooldown_remaining: 0,
                stunned_until_tick: 0,
                status: castle_fight_sim::StatusState::default(),
                mana_current: None,
                mana_maximum: None,
                visual_kind: UnitVisualKind::Melee,
                active_defend_ability: None,
            },
        );
        let terrain = flat_terrain();
        let center = unit_visual_center_lerp(
            samples.current.units[&SimId(11)].position,
            samples.current.units[&SimId(11)].position,
            &samples.current.units[&SimId(11)],
            1.0,
            &terrain,
        );

        assert_eq!(
            pick_unit_on_ray(
                Vec3::new(center.x, center.y, 0.0),
                Vec3::Z,
                &samples,
                &terrain,
                1.0,
            ),
            Some(SimId(11))
        );
        assert_eq!(
            pick_world_actor_on_ray(
                Vec3::new(center.x, center.y, 0.0),
                Vec3::Z,
                &samples,
                &terrain,
                1.0,
            ),
            Some(SimId(11))
        );
        assert_eq!(
            pick_unit_on_ray(
                Vec3::new(center.x, 0.0, 0.0),
                Vec3::Z,
                &samples,
                &terrain,
                1.0,
            ),
            None
        );
    }

    #[test]
    fn console_capture_covers_minimap_and_selection_space() {
        assert!(!cursor_over_inspector_panel(
            Vec2::new(100.0, 400.0),
            1440.0,
            720.0
        ));
        assert!(cursor_over_inspector_panel(
            Vec2::new(100.0, 500.0),
            1440.0,
            720.0
        ));
        assert!(cursor_over_inspector_panel(
            Vec2::new(1129.0, 700.0),
            1440.0,
            720.0
        ));
        assert!(!cursor_over_inspector_panel(
            Vec2::new(1441.0, 700.0),
            1440.0,
            720.0
        ));
    }
}
