//! Castle item presentation and input: the six inventory buttons of the console, the castle shop
//! spellbook helpers used by the command card, and the items lying on the battlefield.
//!
//! Every rule (what a button may do, what an item costs, how long it cools down) is read from the
//! authoritative simulation or the version-scoped item catalog; this module only draws and
//! translates clicks into [`PlayerCommand`]s.
use bevy::{prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{
    CASTLE_FIGHT_SIMULATION_HZ, CastleFightContentBundle, CommandCardPosition, INVENTORY_SLOTS,
    ItemCommandError, PlayerCommand, SUBUNITS_PER_WORLD_UNIT, SimId, Simulation,
    items::{ItemCatalog, ItemDefinition, ItemEffect, item_catalog_for_version},
};

use crate::{
    AuthoritativeSimulation, SelectedMatch, SimulationPlayback,
    bridge::PresentationSamples,
    build_ui::{ActionPanelMode, ActionPanelState, TargetingAction},
    builder_controls::BuilderCommandInput,
    console::{ConsoleSlots, INVENTORY_SLOT, wc3, wc3_font},
    debug_menu::DebugMenuState,
    inspection::{cursor_over_inspector_panel, pick_builder_on_ray},
    presentation::{
        WorldMetrics, sim_point_to_terrain_world, viewport_ground_point, world_to_sim_point,
    },
    terrain::TerrainSurface,
    ui_icons::{UiIconAssets, UiIconKey, UiIconRole, UiObjectIconKind},
};

/// Inventory button geometry, in layout units: each button covers the gold frame that the
/// console art draws for its slot. `OFFSET` is the first frame's top-left corner relative to the
/// top-left of `INVENTORY_SLOT`.
const INVENTORY_BUTTON_WIDTH: f32 = 0.036_6;
const INVENTORY_BUTTON_HEIGHT: f32 = 0.036_2;
const INVENTORY_BUTTON_OFFSET: (f32, f32) = (0.000_9, -0.001);
const INVENTORY_COLUMN_PITCH: f32 = 0.04;
const INVENTORY_ROW_PITCH: f32 = 0.038_5;
const DRAG_THRESHOLD: f32 = 6.0;
/// How close, in world units, a right-click must land to an item on the ground to pick it up.
const GROUND_ITEM_PICK_RADIUS: f32 = 70.0;
const GROUND_ITEM_SIZE: f32 = 56.0;
const GROUND_ITEM_HOVER_HEIGHT: f32 = 46.0;
const HOVER_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
const COOLDOWN_TINT: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
const CHARGE_TEXT: Color = Color::srgb(0.95, 0.93, 0.80);
const CHARGE_BACKGROUND: Color = Color::srgba(0.02, 0.015, 0.01, 0.72);
/// Warcraft's default inventory hotkeys, top-left to bottom-right (numpad layout).
const INVENTORY_HOTKEYS: [KeyCode; INVENTORY_SLOTS] = [
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
];

#[must_use]
pub(crate) fn item_catalog(content: &CastleFightContentBundle) -> Option<&'static ItemCatalog> {
    item_catalog_for_version(content.map_version)
}

/// The icon an item shows in the inventory and on the ground.
#[must_use]
pub(crate) const fn item_icon_key(item: u32) -> UiIconKey {
    UiIconKey::Object {
        kind: UiObjectIconKind::Item,
        rawcode: item,
        role: UiIconRole::Interface,
    }
}

/// The command-card cell of the shop spellbook button on a builder's card.
#[must_use]
pub(crate) fn shop_button_cell(catalog: &ItemCatalog) -> CommandCardPosition {
    let position = catalog.shop.spellbook_position;
    CommandCardPosition::new(position.x, position.y)
}

/// The command-card cell of a shop entry inside the opened spellbook.
#[must_use]
pub(crate) fn shop_entry_cell(catalog: &ItemCatalog, entry: usize) -> CommandCardPosition {
    let cell = catalog.shop.card_cell(entry);
    CommandCardPosition::new(cell.x, cell.y)
}

/// Why the shop button cannot be pressed right now, if it cannot.
#[must_use]
pub(crate) fn shop_unavailable(
    simulation: &Simulation,
    controller: castle_fight_sim::PlayerId,
    builder: SimId,
    position: u8,
) -> Option<ItemCommandError> {
    simulation.can_buy_item(controller, builder, position).err()
}

/// Seconds, rounded up, until a tick.
fn seconds_until(ready_tick: u64, now: u64) -> u64 {
    ready_tick
        .saturating_sub(now)
        .div_ceil(CASTLE_FIGHT_SIMULATION_HZ as u64)
}

/// The label under a shop button: price and what is left on the shelf. The icon and tooltip
/// identify the item, as in Warcraft's own shop buttons.
#[must_use]
pub(crate) fn shop_button_label(
    catalog: &ItemCatalog,
    simulation: &Simulation,
    team: castle_fight_sim::Team,
    entry: u8,
) -> String {
    let Some(entry_definition) = catalog.shop.entries.get(usize::from(entry)) else {
        return String::new();
    };
    let Some(item) = catalog.item(castle_fight_sim::items::ItemId(entry_definition.item)) else {
        return String::new();
    };
    let stock = simulation
        .item_shop(team)
        .get(usize::from(entry))
        .copied()
        .unwrap_or_default();
    let price = if item.lumber_cost == 0 {
        format!("{}g", item.gold_cost)
    } else {
        format!("{}g {}w", item.gold_cost, item.lumber_cost)
    };
    if !stock.opened {
        let remaining = entry_definition
            .stock
            .start_delay_seconds
            .saturating_sub(simulation.round_elapsed_seconds());
        return format!("{price}\nin {remaining}s");
    }
    if stock.stock == 0 {
        return stock.restock_tick.map_or_else(
            || format!("{price}\nsold out"),
            |tick| format!("{price}\nback {}s", seconds_until(tick, simulation.tick())),
        );
    }
    format!("{price}\nx{}", stock.stock)
}

/// Looks up the definition behind an inventory slot of the builder.
fn slot_definition<'a>(
    simulation: &Simulation,
    catalog: &'a ItemCatalog,
    builder: SimId,
    slot: usize,
) -> Option<&'a ItemDefinition> {
    let view = simulation.builder(builder)?;
    let stack = view.inventory.slot(slot)?;
    catalog.item(stack.item)
}

#[derive(Component)]
pub(crate) struct InventoryButton(pub(crate) usize);

#[derive(Component)]
struct InventoryIcon(usize);

#[derive(Component)]
struct InventoryCharges(usize);

#[derive(Component)]
struct InventoryCooldown(usize);

#[derive(Component)]
struct InventoryDragGhost;

#[derive(Component)]
struct GroundItemVisual(SimId);

type InventoryCovers<'w, 's> = Query<
    'w,
    's,
    &'static mut Visibility,
    (
        Without<InventoryButton>,
        Without<InventoryCharges>,
        Without<InventoryDragGhost>,
    ),
>;

type InventoryChargeTexts<'w, 's> = Query<
    'w,
    's,
    (
        &'static InventoryCharges,
        &'static mut Text,
        &'static mut Visibility,
    ),
    (Without<InventoryButton>, Without<InventoryDragGhost>),
>;

type DragGhostFilter = (
    With<InventoryDragGhost>,
    Without<InventoryButton>,
    Without<InventoryCharges>,
);

type DragGhostParts = (
    &'static mut Node,
    &'static mut ImageNode,
    &'static mut Visibility,
);

#[derive(Resource, Default)]
struct InventoryDrag {
    slot: Option<usize>,
    origin: Vec2,
    dragging: bool,
}

#[derive(Resource, Default)]
struct GroundItemMeshes {
    quad: Option<Handle<Mesh>>,
}

pub(crate) struct ItemUiPlugin;

impl Plugin for ItemUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleSlots>()
            .init_resource::<InventoryDrag>()
            .init_resource::<GroundItemMeshes>()
            .add_systems(Startup, spawn_inventory_buttons)
            .add_systems(
                Update,
                (
                    handle_inventory_input.before(BuilderCommandInput),
                    sync_inventory_buttons,
                    sync_ground_items,
                )
                    .chain(),
            );
    }
}

/// Six inventory buttons in Warcraft's two-column layout.
fn spawn_inventory_buttons(mut commands: Commands, slots: Res<ConsoleSlots>) {
    commands.entity(slots.inventory).with_children(|inventory| {
        for index in 0..INVENTORY_SLOTS {
            let (column, row) = (index % 2, index / 2);
            inventory
                .spawn((
                    Button,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(
                            (INVENTORY_BUTTON_OFFSET.0 + column as f32 * INVENTORY_COLUMN_PITCH)
                                / INVENTORY_SLOT.width
                                * 100.0,
                        ),
                        top: percent(
                            (INVENTORY_BUTTON_OFFSET.1 + row as f32 * INVENTORY_ROW_PITCH)
                                / INVENTORY_SLOT.height
                                * 100.0,
                        ),
                        width: percent(INVENTORY_BUTTON_WIDTH / INVENTORY_SLOT.width * 100.0),
                        height: percent(INVENTORY_BUTTON_HEIGHT / INVENTORY_SLOT.height * 100.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    Visibility::Hidden,
                    ZIndex(1),
                    InventoryButton(index),
                ))
                .with_children(|button| {
                    let fill = Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        right: px(0.0),
                        top: px(0.0),
                        bottom: px(0.0),
                        ..default()
                    };
                    button.spawn((
                        ImageNode::default(),
                        fill.clone(),
                        Pickable::IGNORE,
                        InventoryIcon(index),
                    ));
                    button.spawn((
                        Node {
                            height: percent(0.0),
                            ..fill
                        },
                        BackgroundColor(COOLDOWN_TINT),
                        Pickable::IGNORE,
                        InventoryCooldown(index),
                    ));
                    button.spawn((
                        Text::new(""),
                        TextFont::from_font_size(wc3_font(0.0075)),
                        TextColor(CHARGE_TEXT),
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(1.0),
                            bottom: px(1.0),
                            padding: UiRect::axes(px(2.0), px(0.0)),
                            ..default()
                        },
                        BackgroundColor(CHARGE_BACKGROUND),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                        InventoryCharges(index),
                    ));
                });
        }
    });
    commands.spawn((
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            width: wc3(INVENTORY_BUTTON_WIDTH),
            height: wc3(INVENTORY_BUTTON_HEIGHT),
            ..default()
        },
        GlobalZIndex(30),
        Visibility::Hidden,
        Pickable::IGNORE,
        InventoryDragGhost,
    ));
}

/// The builder whose inventory the console shows: the controllable builder in the action panel.
fn inventory_owner(panel: &ActionPanelState, simulation: &Simulation) -> Option<SimId> {
    panel
        .actor
        .filter(|actor| simulation.builder(*actor).is_some())
}

#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_inventory_buttons(
    panel: Res<ActionPanelState>,
    authoritative: Res<AuthoritativeSimulation>,
    selected_match: Res<SelectedMatch>,
    slots: Res<ConsoleSlots>,
    asset_server: Res<AssetServer>,
    mut icon_assets: ResMut<UiIconAssets>,
    drag: Res<InventoryDrag>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut covers: InventoryCovers,
    mut buttons: Query<
        (
            &InventoryButton,
            &Interaction,
            &mut Visibility,
            &mut BackgroundColor,
        ),
        Without<InventoryDragGhost>,
    >,
    mut icons: Query<(&InventoryIcon, &mut ImageNode), Without<InventoryDragGhost>>,
    mut charges: InventoryChargeTexts,
    mut cooldowns: Query<(&InventoryCooldown, &mut Node), Without<InventoryDragGhost>>,
    mut ghost: Single<DragGhostParts, DragGhostFilter>,
) {
    let simulation = &authoritative.simulation;
    let catalog = item_catalog(selected_match.content);
    let owner = inventory_owner(&panel, simulation).zip(catalog);
    if let Ok(mut cover) = covers.get_mut(slots.inventory_cover) {
        *cover = if owner.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    let now = simulation.tick();
    let inventory = owner.and_then(|(builder, _)| simulation.builder(builder));
    let stack_at = |slot: usize| {
        inventory
            .as_ref()
            .and_then(|view| view.inventory.slot(slot))
            .copied()
    };
    for (button, interaction, mut visibility, mut background) in &mut buttons {
        *visibility = if owner.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        *background = BackgroundColor(
            if stack_at(button.0).is_some() && *interaction != Interaction::None {
                HOVER_TINT
            } else {
                Color::NONE
            },
        );
    }
    for (icon, mut image) in &mut icons {
        *image = stack_at(icon.0)
            .and_then(|stack| icon_assets.image(item_icon_key(stack.item.0), &asset_server))
            .map_or_else(ImageNode::default, ImageNode::new);
    }
    for (label, mut text, mut visibility) in &mut charges {
        let count = stack_at(label.0).map_or(0, |stack| stack.charges);
        *visibility = if count > 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if count > 0 {
            text.0 = count.to_string();
        }
    }
    for (cooldown, mut node) in &mut cooldowns {
        let fraction = owner
            .zip(inventory.as_ref())
            .and_then(|((_, catalog), view)| {
                let stack = view.inventory.slot(cooldown.0)?;
                let definition = catalog.item(stack.item)?;
                cooldown_fraction(definition, stack, &view.inventory, now)
            })
            .unwrap_or(0.0);
        node.height = percent(fraction * 100.0);
    }

    let (node, image, visibility) = &mut *ghost;
    match (
        drag.slot.filter(|_| drag.dragging),
        window.cursor_position(),
    ) {
        (Some(slot), Some(cursor)) => {
            **image = stack_at(slot)
                .and_then(|stack| icon_assets.image(item_icon_key(stack.item.0), &asset_server))
                .map_or_else(ImageNode::default, ImageNode::new);
            node.left = px(cursor.x - window.height() * 0.015);
            node.top = px(cursor.y - window.height() * 0.015);
            **visibility = Visibility::Inherited;
        }
        _ => **visibility = Visibility::Hidden,
    }
}

/// Remaining share of the item's cooldown, from its authored total.
fn cooldown_fraction(
    definition: &ItemDefinition,
    stack: &castle_fight_sim::ItemStack,
    inventory: &castle_fight_sim::Inventory,
    now: u64,
) -> Option<f32> {
    let (ready_tick, total_seconds) = match definition.use_effect() {
        Some(ItemEffect::OrbChainLightning(orb)) => (
            inventory.cooldown_ready_tick(definition.cooldown_group)?,
            orb.use_cooldown_seconds,
        ),
        _ => {
            let blast = definition.effects.iter().find_map(|effect| match effect {
                ItemEffect::Blast(blast) => Some(blast),
                _ => None,
            })?;
            (stack.ready_tick, blast.cooldown_seconds)
        }
    };
    let total = total_seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ);
    if total <= 0.0 || ready_tick <= now {
        return None;
    }
    Some(((ready_tick - now) as f64 / total).clamp(0.0, 1.0) as f32)
}

/// What clicking an inventory slot does: arm a target for aimed items, nothing for passives.
fn use_inventory_slot(
    slot: usize,
    panel: &mut ActionPanelState,
    simulation: &Simulation,
    catalog: &ItemCatalog,
    builder: SimId,
) {
    let Some(definition) = slot_definition(simulation, catalog, builder, slot) else {
        return;
    };
    let Some(effect) = definition.use_effect() else {
        panel.status = format!("{} has no active ability.", definition.name);
        return;
    };
    let on_cooldown = simulation.builder(builder).is_some_and(|view| {
        view.inventory
            .on_cooldown(definition.cooldown_group, simulation.tick())
    });
    if on_cooldown {
        panel.status = format!("{} is not ready yet.", definition.name);
        return;
    }
    let slot = u8::try_from(slot).expect("inventory slot fits u8");
    panel.mode = ActionPanelMode::Targeting(TargetingAction::UseItem(slot));
    panel.status = match effect {
        ItemEffect::OrbChainLightning(_) => {
            format!(
                "{}: left-click an enemy unit; Esc cancels.",
                definition.name
            )
        }
        _ => format!("{}: left-click a location; Esc cancels.", definition.name),
    };
}

fn submit(
    authoritative: &mut AuthoritativeSimulation,
    panel: &mut ActionPanelState,
    selected_match: &SelectedMatch,
    debug_menu: &DebugMenuState,
    builder: SimId,
    command: PlayerCommand,
    accepted: &str,
) {
    let controller = debug_menu.controller_for_actor(
        &authoritative.simulation,
        selected_match.local_player,
        builder,
    );
    let submission = authoritative.submit_local_command(controller, command);
    panel.status = match crate::builder_controls::command_submission_status(
        submission,
        accepted.to_owned(),
        "Item command rejected",
    ) {
        Ok(status) | Err(status) => status,
    };
}

#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn handle_inventory_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    selected_match: Res<SelectedMatch>,
    debug_menu: Res<DebugMenuState>,
    playback: Res<SimulationPlayback>,
    fixed_time: Res<Time<Fixed>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: (Res<TerrainSurface>, Res<WorldMetrics>),
    samples: Res<PresentationSamples>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut panel: ResMut<ActionPanelState>,
    mut drag: ResMut<InventoryDrag>,
    buttons: Query<(&InventoryButton, &Interaction)>,
) {
    let (terrain, _metrics) = world;
    let Some(catalog) = item_catalog(selected_match.content) else {
        return;
    };
    let Some(builder) = inventory_owner(&panel, &authoritative.simulation) else {
        *drag = InventoryDrag::default();
        return;
    };
    let occupied = |authoritative: &AuthoritativeSimulation, slot: usize| {
        authoritative
            .simulation
            .builder(builder)
            .is_some_and(|view| view.inventory.slot(slot).is_some())
    };
    // A button under the cursor reads `Hovered`; the one the left button went down on stays
    // `Pressed` even after the cursor has left it, so the two roles never collide in a drag.
    let button_in = |state: Interaction| {
        buttons
            .iter()
            .find(|(_, interaction)| **interaction == state)
            .map(|(button, _)| button.0)
    };
    let pressed = button_in(Interaction::Pressed);
    let hovered = button_in(Interaction::Hovered);
    let cursor = window.cursor_position();

    let usable_mode = matches!(
        panel.mode,
        ActionPanelMode::Actions | ActionPanelMode::Targeting(TargetingAction::UseItem(_))
    );
    if usable_mode {
        for (slot, key) in INVENTORY_HOTKEYS.iter().enumerate() {
            if keys.just_pressed(*key) && occupied(&authoritative, slot) {
                use_inventory_slot(
                    slot,
                    &mut panel,
                    &authoritative.simulation,
                    catalog,
                    builder,
                );
            }
        }
        if mouse_buttons.just_pressed(MouseButton::Right)
            && let Some(slot) = hovered
                .or(pressed)
                .filter(|slot| occupied(&authoritative, *slot))
        {
            use_inventory_slot(
                slot,
                &mut panel,
                &authoritative.simulation,
                catalog,
                builder,
            );
        }
    }

    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(slot) = pressed.filter(|slot| occupied(&authoritative, *slot))
        && let Some(cursor) = cursor
    {
        *drag = InventoryDrag {
            slot: Some(slot),
            origin: cursor,
            dragging: false,
        };
    }
    if let (Some(_), Some(cursor)) = (drag.slot, cursor)
        && mouse_buttons.pressed(MouseButton::Left)
        && cursor.distance(drag.origin) > DRAG_THRESHOLD
    {
        drag.dragging = true;
    }
    if !mouse_buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(slot) = drag.slot.take() else {
        return;
    };
    let dragged = std::mem::take(&mut drag.dragging);
    if !occupied(&authoritative, slot) {
        return;
    }
    let slot_byte = u8::try_from(slot).expect("inventory slot fits u8");
    if !dragged || hovered == Some(slot) {
        if usable_mode {
            use_inventory_slot(
                slot,
                &mut panel,
                &authoritative.simulation,
                catalog,
                builder,
            );
        }
        return;
    }
    if let Some(target) = hovered {
        submit(
            &mut authoritative,
            &mut panel,
            &selected_match,
            &debug_menu,
            builder,
            PlayerCommand::MoveItem {
                builder,
                from: slot_byte,
                to: u8::try_from(target).expect("inventory slot fits u8"),
            },
            "Item moved.",
        );
        return;
    }
    let Some(cursor) = cursor else {
        return;
    };
    if cursor_over_inspector_panel(cursor, window.width(), window.height()) || playback.paused {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(ground) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        return;
    };
    let alpha = playback.interpolation_alpha(&fixed_time, &samples);
    let recipient = camera
        .viewport_to_world(camera_transform, cursor)
        .ok()
        .and_then(|ray| pick_builder_on_ray(ray.origin, *ray.direction, &samples, &terrain, alpha))
        .filter(|recipient| *recipient != builder);
    let (command, accepted) = match recipient {
        Some(recipient) => (
            PlayerCommand::GiveItem {
                builder,
                slot: slot_byte,
                recipient,
            },
            "Item hand-over queued.",
        ),
        None => (
            PlayerCommand::DropItem {
                builder,
                slot: slot_byte,
                position: world_to_sim_point(ground),
            },
            "Item drop queued.",
        ),
    };
    submit(
        &mut authoritative,
        &mut panel,
        &selected_match,
        &debug_menu,
        builder,
        command,
        accepted,
    );
}

/// The nearest ground item within reach of a ground point, for right-click pickup.
#[must_use]
pub(crate) fn pick_ground_item(world: Vec3, samples: &PresentationSamples) -> Option<SimId> {
    let scale = SUBUNITS_PER_WORLD_UNIT as f32;
    samples
        .current
        .ground_items
        .iter()
        .map(|(id, item)| {
            let at = Vec2::new(
                item.position.x as f32 / scale,
                item.position.y as f32 / scale,
            );
            (at.distance(Vec2::new(world.x, world.z)), *id)
        })
        .filter(|(distance, _)| *distance <= GROUND_ITEM_PICK_RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// Items on the ground hover as camera-facing icons.
#[expect(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_ground_items(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    terrain: Res<TerrainSurface>,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut icon_assets: ResMut<UiIconAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: ResMut<GroundItemMeshes>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut visuals: Query<(Entity, &GroundItemVisual, &mut Transform)>,
) {
    let facing = camera.rotation();
    let bob = (time.elapsed_secs() * 2.0).sin() * 4.0;
    let mut present = Vec::new();
    for (entity, visual, mut transform) in &mut visuals {
        match samples
            .current
            .ground_items
            .iter()
            .find(|(id, _)| *id == visual.0)
        {
            Some((_, item)) => {
                let ground = sim_point_to_terrain_world(item.position, &terrain);
                transform.translation = ground + Vec3::Y * (GROUND_ITEM_HOVER_HEIGHT + bob);
                transform.rotation = facing;
                present.push(visual.0);
            }
            None => commands.entity(entity).despawn(),
        }
    }
    for (id, item) in &samples.current.ground_items {
        if present.contains(id) {
            continue;
        }
        let quad = shared
            .quad
            .get_or_insert_with(|| meshes.add(Rectangle::new(GROUND_ITEM_SIZE, GROUND_ITEM_SIZE)))
            .clone();
        let texture = icon_assets.image(item_icon_key(item.item.0), &asset_server);
        let ground = sim_point_to_terrain_world(item.position, &terrain);
        commands.spawn((
            Mesh3d(quad),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: texture,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(ground + Vec3::Y * GROUND_ITEM_HOVER_HEIGHT)
                .with_rotation(facing),
            GroundItemVisual(*id),
        ));
    }
}
