use bevy::{prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{SUBUNITS_PER_WORLD_UNIT, SimId, Team};

use crate::{
    bridge::{BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample, UnitVisualKind},
    build_ui::{BuildSelection, cursor_over_build_panel},
    presentation::{
        WorldMetrics, draw_footprint_outline, sim_point_to_world, sim_point_to_world_lerp,
        viewport_ground_point,
    },
};

const PANEL_RIGHT: f32 = 16.0;
const PANEL_TOP: f32 = 16.0;
const PANEL_WIDTH: f32 = 340.0;
const PANEL_HEIGHT: f32 = 330.0;
const MIN_UNIT_PICK_RADIUS: f32 = 6.0;
const SELECTION_RING_PADDING: f32 = 2.5;
const SELECTION_COLOR: Color = Color::srgb(1.0, 0.88, 0.22);
const PANEL_BACKGROUND: Color = Color::srgba(0.035, 0.045, 0.060, 0.94);

#[derive(Resource, Default, Debug, Clone, Copy)]
pub(crate) struct InspectionSelection {
    pub(crate) selected: Option<SimId>,
}

#[derive(Component)]
struct InspectionText;

pub(crate) struct InspectionPlugin;

impl Plugin for InspectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectionSelection>()
            .add_systems(Startup, setup_inspector_ui)
            .add_systems(
                Update,
                (
                    handle_world_selection,
                    clear_stale_selection,
                    update_inspector_text,
                    draw_selection_highlight,
                )
                    .chain(),
            );
    }
}

fn setup_inspector_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(PANEL_RIGHT),
                top: px(PANEL_TOP),
                width: px(PANEL_WIDTH),
                height: px(PANEL_HEIGHT),
                padding: UiRect::all(px(14.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                border_radius: BorderRadius::all(px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("INSPECTOR"),
                TextFont::from_font_size(22.0),
                TextColor(Color::WHITE),
            ));
            panel.spawn((
                Text::new("Left-click a unit or building to inspect it."),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.72, 0.76, 0.82)),
                Node {
                    width: percent(100.0),
                    ..default()
                },
                InspectionText,
            ));
        });
}

fn handle_world_selection(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    context: (
        Res<Time<Fixed>>,
        Res<WorldMetrics>,
        Res<PresentationSamples>,
        Res<BuildSelection>,
    ),
    mut selection: ResMut<InspectionSelection>,
) {
    let (fixed_time, metrics, samples, build_selection) = context;
    if keys.just_pressed(KeyCode::Escape) && build_selection.kind.is_none() {
        selection.selected = None;
    }

    if !mouse_buttons.just_pressed(MouseButton::Left) || build_selection.kind.is_some() {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_build_panel(cursor) || cursor_over_inspector_panel(cursor, window.width()) {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor) else {
        selection.selected = None;
        return;
    };
    selection.selected = pick_entity(world, &samples, &metrics, fixed_time.overstep_fraction());
}

fn clear_stale_selection(
    samples: Res<PresentationSamples>,
    mut selection: ResMut<InspectionSelection>,
) {
    let Some(id) = selection.selected else {
        return;
    };
    if !samples.current.units.contains_key(&id) && !samples.current.buildings.contains_key(&id) {
        selection.selected = None;
    }
}

fn update_inspector_text(
    samples: Res<PresentationSamples>,
    selection: Res<InspectionSelection>,
    mut text: Single<&mut Text, With<InspectionText>>,
) {
    let next = match selection.selected {
        None => "Left-click a unit or building to inspect it.\n\nCombat-unit inspection is read-only; no direct orders are exposed.".into(),
        Some(id) => inspector_text(id, &samples),
    };
    if text.0 != next {
        text.0 = next;
    }
}

fn draw_selection_highlight(
    fixed_time: Res<Time<Fixed>>,
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    selection: Res<InspectionSelection>,
    mut gizmos: Gizmos,
) {
    let Some(id) = selection.selected else {
        return;
    };
    let alpha = fixed_time.overstep_fraction();

    if let Some(unit) = samples.current.units.get(&id) {
        let previous = samples.previous.units.get(&id).unwrap_or(unit);
        let center = sim_point_to_world_lerp(previous.position, unit.position, alpha);
        let radius = unit_pick_radius(unit) + SELECTION_RING_PADDING;
        gizmos.circle(
            Isometry3d::new(
                center + Vec3::Y * 0.35,
                Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
            ),
            radius,
            SELECTION_COLOR,
        );
        if let Some(target) = unit.target
            && let Some(target_position) = current_entity_position(target, &samples, &metrics)
        {
            gizmos.line(
                center + Vec3::Y * 4.0,
                target_position + Vec3::Y * 4.0,
                SELECTION_COLOR.with_alpha(0.55),
            );
        }
        return;
    }

    if let Some(building) = samples.current.buildings.get(&id) {
        draw_footprint_outline(&mut gizmos, &metrics, building.footprint, SELECTION_COLOR);
        if let Some(target) = building.target
            && let Some(target_position) = current_entity_position(target, &samples, &metrics)
        {
            let (center, _) = metrics.footprint_center_size(building.footprint);
            gizmos.line(
                center + Vec3::Y * 6.0,
                target_position + Vec3::Y * 4.0,
                SELECTION_COLOR.with_alpha(0.55),
            );
        }
    }
}

fn pick_entity(
    world: Vec3,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    alpha: f32,
) -> Option<SimId> {
    let mut nearest_unit: Option<(f32, SimId)> = None;
    for unit in samples.current.units.values() {
        let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
        let position = sim_point_to_world_lerp(previous.position, unit.position, alpha);
        let distance_sq = position.xz().distance_squared(world.xz());
        let radius = unit_pick_radius(unit);
        if distance_sq > radius * radius {
            continue;
        }
        match nearest_unit {
            Some((nearest_sq, _)) if nearest_sq <= distance_sq => {}
            _ => nearest_unit = Some((distance_sq, unit.id)),
        }
    }
    if let Some((_, id)) = nearest_unit {
        return Some(id);
    }

    samples
        .current
        .buildings
        .values()
        .find(|building| point_inside_building(world, building, metrics))
        .map(|building| building.id)
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
) -> Option<Vec3> {
    if let Some(unit) = samples.current.units.get(&id) {
        return Some(sim_point_to_world(unit.position));
    }
    samples.current.buildings.get(&id).map(|building| {
        let (center, _) = metrics.footprint_center_size(building.footprint);
        center
    })
}

fn inspector_text(id: SimId, samples: &PresentationSamples) -> String {
    if let Some(unit) = samples.current.units.get(&id) {
        return format_unit_inspector(unit, samples.current.tick);
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        return format_building_inspector(building, samples.current.tick);
    }
    format!("SimId {} is no longer present.", id.0)
}

fn format_unit_inspector(unit: &UnitSample, tick: u64) -> String {
    let position = sim_point_to_world(unit.position);
    format!(
        "UNIT #{}\nSide: {}\nType: {}\nHealth: {}\nPosition: {:.1}, {:.1}\nTarget: {}\nAttack cooldown: {} ticks\nState: {}",
        unit.id.0,
        team_label(unit.team),
        unit_kind_label(unit.visual_kind),
        unit.health,
        position.x,
        position.z,
        target_label(unit.target),
        unit.cooldown_remaining,
        stun_label(unit.stunned_until_tick, tick),
    )
}

fn format_building_inspector(building: &BuildingSample, tick: u64) -> String {
    let mut lines = vec![
        format!("BUILDING #{}", building.id.0),
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
    }
}

fn building_kind_label(kind: BuildingVisualKind) -> &'static str {
    match kind {
        BuildingVisualKind::Structure => "Structure",
        BuildingVisualKind::Production => "Production",
        BuildingVisualKind::Attack => "Attack",
        BuildingVisualKind::Spellcaster => "Spellcaster",
    }
}

fn team_label(team: Team) -> &'static str {
    match team.0 {
        0 => "Blue",
        1 => "Red",
        _ => "Unknown",
    }
}

fn cursor_over_inspector_panel(cursor: Vec2, window_width: f32) -> bool {
    let left = window_width - PANEL_RIGHT - PANEL_WIDTH;
    cursor.x >= left
        && cursor.x <= window_width - PANEL_RIGHT
        && cursor.y >= PANEL_TOP
        && cursor.y <= PANEL_TOP + PANEL_HEIGHT
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use castle_fight_sim::{BuildingFootprint, NavCell, SimPoint, SimulationConfig};

    use super::*;
    use crate::bridge::PresentationSnapshot;

    fn metrics() -> WorldMetrics {
        WorldMetrics::from_simulation_config(&SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(100, 100),
            ..SimulationConfig::default()
        })
    }

    fn empty_samples() -> PresentationSamples {
        let snapshot = PresentationSnapshot {
            tick: 10,
            units: BTreeMap::new(),
            buildings: BTreeMap::new(),
            corpses: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            attacks: Vec::new(),
        };
        PresentationSamples::new(snapshot)
    }

    #[test]
    fn picking_prefers_nearby_unit() {
        let mut samples = empty_samples();
        samples.current.units.insert(
            SimId(7),
            UnitSample {
                id: SimId(7),
                team: Team(0),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                collision_radius: 4 * SUBUNITS_PER_WORLD_UNIT,
                health: 50,
                target: None,
                cooldown_remaining: 0,
                stunned_until_tick: 0,
                visual_kind: UnitVisualKind::Melee,
            },
        );
        assert_eq!(
            pick_entity(Vec3::new(103.0, 0.0, 100.0), &samples, &metrics(), 1.0,),
            Some(SimId(7))
        );
    }

    #[test]
    fn picking_building_uses_authoritative_footprint() {
        let mut samples = empty_samples();
        samples.current.buildings.insert(
            SimId(9),
            BuildingSample {
                id: SimId(9),
                team: Team(1),
                footprint: BuildingFootprint::new(10, 20, 4, 4),
                health: 1_000,
                target: None,
                next_spawn_tick: Some(20),
                cooldown_remaining: None,
                mana_current: None,
                mana_maximum: None,
                ability_ready_tick: None,
                stunned_until_tick: None,
                visual_kind: BuildingVisualKind::Production,
            },
        );
        assert_eq!(
            pick_entity(Vec3::new(120.0, 0.0, 220.0), &samples, &metrics(), 1.0,),
            Some(SimId(9))
        );
        assert_eq!(
            pick_entity(Vec3::new(145.0, 0.0, 220.0), &samples, &metrics(), 1.0,),
            None
        );
    }

    #[test]
    fn inspector_panel_capture_tracks_right_edge() {
        assert!(cursor_over_inspector_panel(Vec2::new(1424.0, 16.0), 1440.0));
        assert!(cursor_over_inspector_panel(
            Vec2::new(1084.0, 346.0),
            1440.0
        ));
        assert!(!cursor_over_inspector_panel(
            Vec2::new(1083.0, 200.0),
            1440.0
        ));
    }
}
