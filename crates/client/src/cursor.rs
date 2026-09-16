use crate::{
    AuthoritativeSimulation, SelectedMatch,
    bridge::PresentationSamples,
    build_ui::{ActionPanelState, TargetingAction, cursor_over_action_panel, placement_footprint},
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    inspection::cursor_over_inspector_panel,
    presentation::{WorldMetrics, viewport_ground_point, world_to_sim_point},
    resource_ui::{BuilderShortcutState, cursor_over_builder_shortcuts},
    terrain::TerrainSurface,
    ui_icons::{CastleFightPresentationCatalog, UiIconAssets},
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow},
};

const WC3_CURSOR_COLUMNS: u32 = 8;
const WC3_CURSOR_ROWS: u32 = 4;
const WC3_REFERENCE_FRAME_SIZE: u32 = 32;

/// Stock Warcraft cursor sprite-sheet cells used by the cursor model's static sequences.
///
/// Warcraft keeps animated hand/crosshair sequences in rows 0 and 2. Row 3 contains the static
/// Normal, Target, and InvalidTarget cells used while no cursor animation is playing. Native only
/// needs those static states for now; the extracted atlas remains complete so animated feedback
/// can be added later without changing the asset pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wc3CursorState {
    Normal,
    Target,
    InvalidTarget,
}

impl Wc3CursorState {
    const fn atlas_cell(self) -> UVec2 {
        match self {
            Self::Normal => UVec2::new(0, 3),
            Self::Target => UVec2::new(1, 3),
            Self::InvalidTarget => UVec2::new(2, 3),
        }
    }

    const fn reference_hotspot(self) -> UVec2 {
        match self {
            // The Warcraft cursor model places the normal hand slightly left of its origin. Its
            // Normal sequence extents put the click point about 5/32 across the frame and at the
            // top edge. Target/InvalidTarget are centered on the crosshair.
            Self::Normal => UVec2::new(5, 0),
            Self::Target | Self::InvalidTarget => UVec2::new(16, 16),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wc3CursorFrame {
    rect: URect,
    hotspot: (u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppliedCursor {
    state: Wc3CursorState,
    image_size: UVec2,
}

#[derive(Resource, Default)]
struct CursorPresentationState {
    applied: Option<AppliedCursor>,
    warned_bad_atlas: bool,
}

pub(crate) struct CursorPresentationPlugin;

#[derive(SystemParam)]
struct CursorPresentationResources<'w> {
    action_panel: Res<'w, ActionPanelState>,
    debug_menu: Res<'w, DebugMenuState>,
    builder_shortcuts: Res<'w, BuilderShortcutState>,
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    presentation: Res<'w, PresentationSamples>,
    authoritative: Res<'w, AuthoritativeSimulation>,
    selected_match: Res<'w, SelectedMatch>,
    asset_server: Res<'w, AssetServer>,
    images: Res<'w, Assets<Image>>,
    ui_assets: ResMut<'w, UiIconAssets>,
    cursor_state: ResMut<'w, CursorPresentationState>,
}

impl Plugin for CursorPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorPresentationState>()
            .add_systems(Update, update_wc3_cursor);
    }
}

fn update_wc3_cursor(
    mut commands: Commands,
    primary_window: Single<(Entity, &Window), With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut resources: CursorPresentationResources<'_>,
) {
    let Some(presentation) =
        CastleFightPresentationCatalog::for_version(resources.selected_match.content.map_version)
    else {
        return;
    };
    let Some(atlas) = resources
        .ui_assets
        .cursor_atlas(presentation.cursor_theme, &resources.asset_server)
    else {
        return;
    };
    let Some(image) = resources.images.get(&atlas) else {
        return;
    };

    let (window_entity, window) = *primary_window;
    let desired = desired_cursor_state(window, *camera, &resources);
    let image_size = image.size();
    if resources.cursor_state.applied
        == Some(AppliedCursor {
            state: desired,
            image_size,
        })
    {
        return;
    }

    let Some(frame) = cursor_frame(image_size, desired) else {
        if !resources.cursor_state.warned_bad_atlas {
            eprintln!(
                "warning: generated WC3 cursor atlas has unsupported dimensions {}x{}; expected an 8x4 grid of square frames",
                image_size.x, image_size.y
            );
            resources.cursor_state.warned_bad_atlas = true;
        }
        return;
    };

    commands
        .entity(window_entity)
        .insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: atlas,
            texture_atlas: None,
            flip_x: false,
            flip_y: false,
            rect: Some(frame.rect),
            hotspot: frame.hotspot,
        })));
    resources.cursor_state.applied = Some(AppliedCursor {
        state: desired,
        image_size,
    });
}

fn desired_cursor_state(
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    resources: &CursorPresentationResources<'_>,
) -> Wc3CursorState {
    let Some(targeting) = resources.action_panel.targeting() else {
        return Wc3CursorState::Normal;
    };
    let Some(cursor) = window.cursor_position() else {
        return Wc3CursorState::Normal;
    };
    if cursor_over_action_panel(
        cursor,
        window.height(),
        resources.action_panel.actor.is_some(),
    ) || cursor_over_inspector_panel(cursor, window.width())
        || cursor_over_debug_menu(cursor, resources.debug_menu.is_open())
        || cursor_over_builder_shortcuts(cursor, &resources.builder_shortcuts)
    {
        return Wc3CursorState::Normal;
    }

    let world = viewport_ground_point(camera.0, camera.1, cursor, &resources.terrain);
    if targeting == TargetingAction::Blink {
        let Some(world) = world else {
            return Wc3CursorState::InvalidTarget;
        };
        let Some(builder) = resources
            .action_panel
            .actor
            .and_then(|actor| resources.presentation.current.builders.get(&actor))
        else {
            return Wc3CursorState::InvalidTarget;
        };
        let destination = world_to_sim_point(world);
        let range = i64::from(builder.blink_range);
        return if builder.position.distance_sq(destination) <= (range * range) as u64 {
            Wc3CursorState::Target
        } else {
            Wc3CursorState::InvalidTarget
        };
    }

    let TargetingAction::Build(kind) = targeting else {
        // All other point/entity targeting commands use Warcraft's crosshair. Keeping this branch
        // generic means newly exposed point-target abilities (such as Rescue Strike) get the same
        // cursor automatically when they enter the shared targeting mode.
        return Wc3CursorState::Target;
    };
    let Some(world) = world else {
        return Wc3CursorState::InvalidTarget;
    };
    let footprint = placement_footprint(
        &resources.metrics,
        world,
        kind,
        resources.selected_match.content,
    );
    let affordable = resources.action_panel.actor.is_some_and(|actor| {
        resources
            .authoritative
            .simulation
            .can_builder_afford_building(actor, kind.economy(resources.selected_match.content))
    });
    if affordable
        && resources
            .authoritative
            .simulation
            .can_place_building_for_team(resources.action_panel.team, footprint)
    {
        Wc3CursorState::Target
    } else {
        Wc3CursorState::InvalidTarget
    }
}

fn cursor_frame(image_size: UVec2, state: Wc3CursorState) -> Option<Wc3CursorFrame> {
    if image_size.x == 0
        || image_size.y == 0
        || !image_size.x.is_multiple_of(WC3_CURSOR_COLUMNS)
        || !image_size.y.is_multiple_of(WC3_CURSOR_ROWS)
    {
        return None;
    }
    let frame_size = UVec2::new(
        image_size.x / WC3_CURSOR_COLUMNS,
        image_size.y / WC3_CURSOR_ROWS,
    );
    if frame_size.x != frame_size.y {
        return None;
    }

    let cell = state.atlas_cell();
    let min = cell * frame_size;
    let max = min + frame_size;
    let reference_hotspot = state.reference_hotspot();
    let hotspot_x = scale_hotspot(reference_hotspot.x, frame_size.x)?;
    let hotspot_y = scale_hotspot(reference_hotspot.y, frame_size.y)?;
    Some(Wc3CursorFrame {
        rect: URect::from_corners(min, max),
        hotspot: (hotspot_x, hotspot_y),
    })
}

fn scale_hotspot(reference: u32, frame_size: u32) -> Option<u16> {
    let scaled = u64::from(reference)
        .checked_mul(u64::from(frame_size))?
        .checked_add(u64::from(WC3_REFERENCE_FRAME_SIZE / 2))?
        / u64::from(WC3_REFERENCE_FRAME_SIZE);
    u16::try_from(scaled).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_sd_cursor_frames_match_warcraft_sprite_sheet() {
        assert_eq!(
            cursor_frame(UVec2::new(256, 128), Wc3CursorState::Normal),
            Some(Wc3CursorFrame {
                rect: URect::from_corners(UVec2::new(0, 96), UVec2::new(32, 128)),
                hotspot: (5, 0),
            })
        );
        assert_eq!(
            cursor_frame(UVec2::new(256, 128), Wc3CursorState::Target),
            Some(Wc3CursorFrame {
                rect: URect::from_corners(UVec2::new(32, 96), UVec2::new(64, 128)),
                hotspot: (16, 16),
            })
        );
        assert_eq!(
            cursor_frame(UVec2::new(256, 128), Wc3CursorState::InvalidTarget),
            Some(Wc3CursorFrame {
                rect: URect::from_corners(UVec2::new(64, 96), UVec2::new(96, 128)),
                hotspot: (16, 16),
            })
        );
    }

    #[test]
    fn cursor_frame_scales_with_reforged_cursor_atlas_resolution() {
        assert_eq!(
            cursor_frame(UVec2::new(1024, 512), Wc3CursorState::Target),
            Some(Wc3CursorFrame {
                rect: URect::from_corners(UVec2::new(128, 384), UVec2::new(256, 512)),
                hotspot: (64, 64),
            })
        );
    }

    #[test]
    fn rejects_malformed_cursor_atlas_layout() {
        assert_eq!(
            cursor_frame(UVec2::new(255, 128), Wc3CursorState::Normal),
            None
        );
        assert_eq!(
            cursor_frame(UVec2::new(256, 256), Wc3CursorState::Normal),
            None
        );
    }
}
