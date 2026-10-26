//! The Warcraft III console: themed art tiles along the top and bottom of the screen, plus the
//! slot rectangles the HUD widgets are placed in.
//!
//! Warcraft lays its interface out in a virtual 0.8 x 0.6 space that is scaled to the window
//! height and centred, with the art tiles at either end spilling into widescreen margins. Every
//! rectangle here is in those units (taken from `UI\FrameDef\UI\ConsoleUI.fdf` and the tile
//! alpha), so the console scales with the window instead of using fixed pixels.
use bevy::{prelude::*, text::FontSize};

use crate::{
    client_settings::ClientSettings,
    ui_icons::{ConsolePiece, ConsoleTheme, UiIconAssets, UiIconKey},
};

/// Height of the virtual Warcraft screen, in layout units.
const SCREEN_HEIGHT: f32 = 0.6;
/// Width of the centred 4:3 layout area, in layout units.
pub(crate) const AREA_WIDTH: f32 = 0.8;
/// Every console tile texture is 512 pixels square and 0.256 units wide.
const UNITS_PER_TEXEL: f32 = 0.256 / 512.0;
const TOP_TEXELS: f32 = 64.0;
pub(crate) const TOP_BAR_UNITS: f32 = TOP_TEXELS * UNITS_PER_TEXEL;

/// Converts Warcraft layout units to a window-height-relative length.
#[must_use]
pub(crate) fn wc3(units: f32) -> Val {
    vh(units / SCREEN_HEIGHT * 100.0)
}

/// A font height in Warcraft layout units (the info panel title is 0.011).
#[must_use]
pub(crate) fn wc3_font(units: f32) -> FontSize {
    FontSize::Vh(units / SCREEN_HEIGHT * 100.0)
}

/// A length that is `pixels` at 1080p and scales with the window height. Lets panels that are
/// not part of Warcraft's layout (tooltips, dock text) keep their proportions at any size.
#[must_use]
pub(crate) fn vpx(pixels: f32) -> Val {
    vh(pixels / 10.8)
}

/// A font size that is `pixels` at 1080p and scales with the window height.
#[must_use]
pub(crate) fn vfont(pixels: f32) -> FontSize {
    FontSize::Vh(pixels / 10.8)
}

/// A rectangle in layout units measured from the bottom-left of the 4:3 area.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotRect {
    pub(crate) left: f32,
    pub(crate) bottom: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl SlotRect {
    const fn new(left: f32, bottom: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            bottom,
            width,
            height,
        }
    }

    fn node(self) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: wc3(self.left),
            bottom: wc3(self.bottom),
            width: wc3(self.width),
            height: wc3(self.height),
            ..default()
        }
    }
}

pub(crate) const MINIMAP_SLOT: SlotRect = SlotRect::new(0.009_375, 0.006_875, 0.139_375, 0.138_125);
pub(crate) const PORTRAIT_SLOT: SlotRect = SlotRect::new(0.216, 0.0025, 0.0735, 0.1085);
pub(crate) const INFO_SLOT: SlotRect = SlotRect::new(0.301, 0.0, 0.2025, 0.1155);
pub(crate) const INVENTORY_SLOT: SlotRect = SlotRect::new(0.512, 0.006_55, 0.071_25, 0.107_55);
pub(crate) const COMMAND_SLOT: SlotRect = SlotRect::new(0.617, 0.0065, 0.1692, 0.127);

/// Right edge of the console art that is outside the 4:3 area on each side.
const WING_WIDTH: f32 = 0.256;

/// One bottom tile: left edge, source rows cropped from the top, and width.
struct BottomTile {
    piece: u8,
    left: f32,
    crop_texels: f32,
    width_texels: f32,
}

const BOTTOM_TILES: [BottomTile; 6] = [
    BottomTile {
        piece: 5,
        left: -0.256,
        crop_texels: 128.0,
        width_texels: 512.0,
    },
    BottomTile {
        piece: 1,
        left: 0.0,
        crop_texels: 160.0,
        width_texels: 512.0,
    },
    BottomTile {
        piece: 2,
        left: 0.256,
        crop_texels: 192.0,
        width_texels: 512.0,
    },
    BottomTile {
        piece: 3,
        left: 0.512,
        crop_texels: 160.0,
        width_texels: 512.0,
    },
    BottomTile {
        piece: 4,
        left: 0.768,
        crop_texels: 160.0,
        width_texels: 64.0,
    },
    BottomTile {
        piece: 6,
        left: 0.8,
        crop_texels: 160.0,
        width_texels: 512.0,
    },
];

/// Top bar pieces: (tile, left, first texel column, last texel column). Tile 02 is split around
/// the gap that Warcraft leaves open above the map.
const TOP_TILES: [(u8, f32, f32, f32); 7] = [
    (5, -0.256, 0.0, 512.0),
    (1, 0.0, 0.0, 512.0),
    (2, 0.256, 0.0, 174.0),
    (2, 0.459, 406.0, 512.0),
    (3, 0.512, 0.0, 512.0),
    (4, 0.768, 0.0, 64.0),
    (6, 0.8, 0.0, 512.0),
];

/// Vertical extent of the sockets cut into the top bar art (texel rows 2..38 of the tiles).
pub(crate) const TOP_SOCKET_TOP: f32 = 2.0 * UNITS_PER_TEXEL;
pub(crate) const TOP_SOCKET_HEIGHT: f32 = 36.0 * UNITS_PER_TEXEL;

/// Resource frame at the right of the top bar: its left edge is the first resource socket's.
/// The four sockets repeat every `RESOURCE_SOCKET_PITCH` and are `RESOURCE_SOCKET_WIDTH` wide.
pub(crate) const RESOURCE_FRAME_LEFT: f32 = 0.46;
pub(crate) const RESOURCE_FRAME_WIDTH: f32 = 0.34;
pub(crate) const RESOURCE_SOCKET_PITCH: f32 = 0.087;
pub(crate) const RESOURCE_SOCKET_WIDTH: f32 = 0.077_5;

/// Dark panel behind slots whose art is open to the map (portrait, info panel, command card).
const SLOT_BACKDROP: Color = Color::srgba(0.025, 0.025, 0.03, 0.95);

/// The four menu sockets in the top bar's left tile.
pub(crate) const MENU_BAR_LEFT: f32 = 0.003;
pub(crate) const MENU_BAR_TOP: f32 = TOP_SOCKET_TOP;
pub(crate) const MENU_BUTTON_WIDTH: f32 = 0.081_5;
pub(crate) const MENU_BUTTON_HEIGHT: f32 = TOP_SOCKET_HEIGHT;
pub(crate) const MENU_BUTTON_PITCH: f32 = 0.085;

/// The inventory cover texture is right-aligned at x=0.6; its first opaque row is 195 of 512.
const INVENTORY_COVER_LEFT: f32 = 0.6 - 256.0 * UNITS_PER_TEXEL;
/// (width, first opaque row, rows) in texels.
const INVENTORY_COVER_TEXELS: (f32, f32, f32) = (256.0, 195.0, 512.0);

/// Width and fixed height of the scoreboard dock, in layout units.
pub(crate) const DOCK_WIDTH: f32 = 0.205;
pub(crate) const DOCK_HEIGHT: f32 = 0.062;
const DOCK_MARGIN: f32 = 0.004;

/// Entities that exist from plugin build so any HUD module can parent into them regardless of
/// startup order. [`spawn_console`] fills in their layout.
#[derive(Resource, Clone, Copy)]
pub(crate) struct ConsoleSlots {
    root: Entity,
    art: Entity,
    pub(crate) minimap: Entity,
    pub(crate) portrait: Entity,
    pub(crate) info: Entity,
    pub(crate) inventory: Entity,
    pub(crate) command: Entity,
    pub(crate) resources: Entity,
    /// Zero-sized anchor at the console's top-right corner; tooltips hang their bottom-right
    /// corner from it.
    pub(crate) tooltips: Entity,
    pub(crate) dock: Entity,
    /// Top-left row of buttons that sit in the four sockets of the top bar.
    pub(crate) menu_bar: Entity,
    /// Stonework hiding the inventory sockets while the selection has no inventory.
    pub(crate) inventory_cover: Entity,
    /// Dock rows, top to bottom.
    pub(crate) dock_header: Entity,
    pub(crate) dock_match: Entity,
    pub(crate) dock_buttons: Entity,
}

impl FromWorld for ConsoleSlots {
    fn from_world(world: &mut World) -> Self {
        let mut allocate = || world.spawn_empty().id();
        Self {
            root: allocate(),
            art: allocate(),
            minimap: allocate(),
            portrait: allocate(),
            info: allocate(),
            inventory: allocate(),
            command: allocate(),
            resources: allocate(),
            tooltips: allocate(),
            dock: allocate(),
            menu_bar: allocate(),
            inventory_cover: allocate(),
            dock_header: allocate(),
            dock_match: allocate(),
            dock_buttons: allocate(),
        }
    }
}

pub(crate) struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleSlots>()
            .add_systems(Startup, spawn_console)
            .add_systems(Update, sync_console_art);
    }
}

fn spawn_console(mut commands: Commands, slots: Res<ConsoleSlots>) {
    commands.entity(slots.root).insert((
        Node {
            position_type: PositionType::Absolute,
            left: percent(50.0),
            margin: UiRect::left(wc3(-AREA_WIDTH / 2.0)),
            top: px(0.0),
            bottom: px(0.0),
            width: wc3(AREA_WIDTH),
            ..default()
        },
        ZIndex(900),
        Pickable::IGNORE,
    ));
    commands.entity(slots.art).insert((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            right: px(0.0),
            top: px(0.0),
            bottom: px(0.0),
            ..default()
        },
        ZIndex(0),
        Pickable::IGNORE,
        ChildOf(slots.root),
    ));
    let root = slots.root;
    let place = |commands: &mut Commands, entity: Entity, node: Node| {
        commands
            .entity(entity)
            .insert((node, ZIndex(1), ChildOf(root)));
    };
    place(
        &mut commands,
        slots.minimap,
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..MINIMAP_SLOT.node()
        },
    );
    place(&mut commands, slots.portrait, PORTRAIT_SLOT.node());
    place(&mut commands, slots.info, INFO_SLOT.node());
    place(&mut commands, slots.inventory, INVENTORY_SLOT.node());
    place(&mut commands, slots.command, COMMAND_SLOT.node());
    place(
        &mut commands,
        slots.resources,
        Node {
            position_type: PositionType::Absolute,
            left: wc3(RESOURCE_FRAME_LEFT),
            top: wc3(TOP_SOCKET_TOP),
            width: wc3(RESOURCE_FRAME_WIDTH),
            height: wc3(TOP_SOCKET_HEIGHT),
            ..default()
        },
    );
    place(
        &mut commands,
        slots.tooltips,
        Node {
            position_type: PositionType::Absolute,
            right: px(0.0),
            bottom: wc3(0.182),
            width: px(0.0),
            height: px(0.0),
            ..default()
        },
    );
    for backed in [slots.minimap, slots.portrait, slots.info, slots.command] {
        commands
            .entity(backed)
            .insert(BackgroundColor(SLOT_BACKDROP));
    }
    place(
        &mut commands,
        slots.menu_bar,
        Node {
            position_type: PositionType::Absolute,
            left: wc3(MENU_BAR_LEFT),
            top: wc3(MENU_BAR_TOP),
            column_gap: wc3(MENU_BUTTON_PITCH - MENU_BUTTON_WIDTH),
            ..default()
        },
    );
    commands.entity(slots.inventory_cover).insert((
        Node {
            position_type: PositionType::Absolute,
            left: wc3(INVENTORY_COVER_LEFT - INVENTORY_SLOT.left),
            bottom: wc3(-INVENTORY_SLOT.bottom),
            width: wc3(INVENTORY_COVER_TEXELS.0 * UNITS_PER_TEXEL),
            height: wc3((INVENTORY_COVER_TEXELS.2 - INVENTORY_COVER_TEXELS.1) * UNITS_PER_TEXEL),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(slots.inventory),
    ));
    commands.entity(slots.dock).insert((
        Node {
            position_type: PositionType::Absolute,
            right: wc3(DOCK_MARGIN),
            top: wc3(TOP_BAR_UNITS + DOCK_MARGIN),
            width: wc3(DOCK_WIDTH),
            height: wc3(DOCK_HEIGHT),
            padding: UiRect::all(wc3(0.004)),
            border: UiRect::all(px(1.0)),
            flex_direction: FlexDirection::Column,
            row_gap: wc3(0.002),
            ..default()
        },
        BackgroundColor(Color::srgba(0.045, 0.04, 0.032, 0.86)),
        BorderColor::all(Color::srgb(0.33, 0.27, 0.16)),
        ZIndex(901),
        Pickable::IGNORE,
    ));
    for row in [slots.dock_header, slots.dock_match, slots.dock_buttons] {
        commands.entity(row).insert((
            Node {
                width: percent(100.0),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                column_gap: wc3(0.008),
                ..default()
            },
            ChildOf(slots.dock),
            Pickable::IGNORE,
        ));
    }
}

fn tile_image(
    parent: &mut ChildSpawnerCommands,
    image: Handle<Image>,
    left: f32,
    columns: (f32, f32),
    rows: (f32, f32),
    from_top: bool,
) {
    let mut node = Node {
        position_type: PositionType::Absolute,
        left: wc3(left),
        width: wc3((columns.1 - columns.0) * UNITS_PER_TEXEL),
        height: wc3((rows.1 - rows.0) * UNITS_PER_TEXEL),
        ..default()
    };
    if from_top {
        node.top = px(0.0);
    } else {
        node.bottom = px(0.0);
    }
    let mut image = ImageNode::new(image);
    image.rect = Some(Rect::new(columns.0, rows.0, columns.1, rows.1));
    parent.spawn((node, image, Pickable::IGNORE));
}

/// Rebuilds the console art whenever the chosen theme changes.
fn sync_console_art(
    mut commands: Commands,
    slots: Res<ConsoleSlots>,
    settings: Res<ClientSettings>,
    asset_server: Res<AssetServer>,
    mut icons: ResMut<UiIconAssets>,
    mut shown: Local<Option<ConsoleTheme>>,
) {
    if *shown == Some(settings.ui_theme) {
        return;
    }
    *shown = Some(settings.ui_theme);
    let theme = settings.ui_theme;
    if let Some(cover) = icons.image(
        UiIconKey::Console(theme, ConsolePiece::InventoryCover),
        &asset_server,
    ) {
        let mut image = ImageNode::new(cover);
        image.rect = Some(Rect::new(
            0.0,
            INVENTORY_COVER_TEXELS.1,
            INVENTORY_COVER_TEXELS.0,
            INVENTORY_COVER_TEXELS.2,
        ));
        commands.entity(slots.inventory_cover).insert(image);
    }
    let mut load = |piece| icons.image(UiIconKey::Console(theme, piece), &asset_server);
    let tiles: Vec<Option<Handle<Image>>> = (1..=6)
        .map(|index| load(ConsolePiece::Tile(index)))
        .collect();
    let overlays = (
        load(ConsolePiece::Tile05Overlay),
        load(ConsolePiece::Tile06Overlay),
    );
    let complete =
        tiles.iter().all(Option::is_some) && overlays.0.is_some() && overlays.1.is_some();
    commands.entity(slots.art).despawn_children();
    commands.entity(slots.art).with_children(|art| {
        if !complete {
            // No extracted pack: draw plain bars so the slots still read as a console.
            for (from_top, height) in [(false, 0.176), (true, TOP_BAR_UNITS)] {
                let mut node = Node {
                    position_type: PositionType::Absolute,
                    left: wc3(-WING_WIDTH),
                    width: wc3(AREA_WIDTH + 2.0 * WING_WIDTH),
                    height: wc3(height),
                    border: UiRect::all(px(2.0)),
                    ..default()
                };
                if from_top {
                    node.top = px(0.0);
                } else {
                    node.bottom = px(0.0);
                }
                art.spawn((
                    node,
                    BackgroundColor(Color::srgba(0.07, 0.06, 0.045, 0.97)),
                    BorderColor::all(Color::srgb(0.32, 0.27, 0.16)),
                    Pickable::IGNORE,
                ));
            }
            return;
        }
        for tile in &BOTTOM_TILES {
            let image = tiles[usize::from(tile.piece) - 1].clone().unwrap();
            tile_image(
                art,
                image,
                tile.left,
                (0.0, tile.width_texels),
                (tile.crop_texels, 512.0),
                false,
            );
        }
        for &(piece, left, first, last) in &TOP_TILES {
            let image = tiles[usize::from(piece) - 1].clone().unwrap();
            tile_image(art, image, left, (first, last), (0.0, TOP_TEXELS), true);
        }
        // The overlays complete the two wing tiles (stonework over the otherwise empty tile).
        for (overlay, left) in [(overlays.0.clone(), -0.256), (overlays.1.clone(), 0.8)] {
            let image = overlay.unwrap();
            tile_image(
                art,
                image.clone(),
                left,
                (0.0, 512.0),
                (128.0, 512.0),
                false,
            );
            tile_image(art, image, left, (0.0, 512.0), (0.0, TOP_TEXELS), true);
        }
    });
}

/// Screen-space description of the console for pointer hit-testing.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConsoleMetrics {
    unit: f32,
    area_left: f32,
}

impl ConsoleMetrics {
    #[must_use]
    pub(crate) fn new(window_width: f32, window_height: f32) -> Self {
        let unit = window_height / SCREEN_HEIGHT;
        Self {
            unit,
            area_left: (window_width - AREA_WIDTH * unit) / 2.0,
        }
    }

    #[must_use]
    pub(crate) fn top_bar_height(self) -> f32 {
        TOP_BAR_UNITS * self.unit
    }

    /// Screen rectangle (y down) of a slot inside the 4:3 area.
    #[must_use]
    pub(crate) fn slot(self, window_height: f32, slot: SlotRect) -> Rect {
        let left = self.area_left + slot.left * self.unit;
        let bottom = window_height - slot.bottom * self.unit;
        Rect::new(
            left,
            bottom - slot.height * self.unit,
            left + slot.width * self.unit,
            bottom,
        )
    }

    #[must_use]
    pub(crate) fn over_bottom_console(self, cursor: Vec2, window_height: f32) -> bool {
        BOTTOM_TILES.iter().any(|tile| {
            let left = self.area_left + tile.left * self.unit;
            let width = tile.width_texels * UNITS_PER_TEXEL * self.unit;
            let height = (512.0 - tile.crop_texels) * UNITS_PER_TEXEL * self.unit;
            cursor.x >= left && cursor.x <= left + width && cursor.y >= window_height - height
        })
    }

    #[must_use]
    pub(crate) fn over_top_bar(self, cursor: Vec2) -> bool {
        cursor.y <= self.top_bar_height()
    }

    #[must_use]
    pub(crate) fn over_dock(self, cursor: Vec2, window_width: f32) -> bool {
        let right = window_width - DOCK_MARGIN * self.unit;
        let left = right - DOCK_WIDTH * self.unit;
        let top = (TOP_BAR_UNITS + DOCK_MARGIN) * self.unit;
        cursor.x >= left
            && cursor.x <= right
            && cursor.y >= top
            && cursor.y <= top + DOCK_HEIGHT * self.unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_tile_is_shorter_than_its_neighbours() {
        let (width, height) = (1920.0, 1080.0);
        let metrics = ConsoleMetrics::new(width, height);
        let middle = metrics.area_left + 0.3 * metrics.unit;
        assert!(
            !metrics.over_bottom_console(Vec2::new(middle, height - 0.165 * metrics.unit), height)
        );
        assert!(
            metrics.over_bottom_console(Vec2::new(middle, height - 0.155 * metrics.unit), height)
        );
        assert!(!metrics.over_bottom_console(Vec2::new(middle, height / 2.0), height));
    }
}
