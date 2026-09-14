use bevy::{prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{SUBUNITS_PER_WORLD_UNIT, SimId, Team};

use crate::{
    SimulationPlayback,
    bridge::{
        BuilderSample, BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample,
        UnitVisualKind,
    },
    build_ui::{BuildSelection, cursor_over_build_panel},
    presentation::{
        WorldMetrics, draw_footprint_outline, sim_point_to_terrain_world,
        sim_point_to_terrain_world_lerp, sim_point_to_world, unit_height, unit_visual_altitude,
        unit_visual_center_lerp, viewport_ground_point,
    },
    terrain::TerrainSurface,
};

const PANEL_RIGHT: f32 = 16.0;
const PANEL_TOP: f32 = 16.0;
const PANEL_WIDTH: f32 = 340.0;
const PANEL_HEIGHT: f32 = 410.0;
const MIN_UNIT_PICK_RADIUS: f32 = 6.0;
const BUILDER_PICK_RADIUS: f32 = 8.0;
const BUILDER_VISUAL_HEIGHT: f32 = 10.0;
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
                Text::new("Left-click a builder, unit, or building to inspect it."),
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
    world: (Res<Time<Fixed>>, Res<WorldMetrics>, Res<TerrainSurface>),
    state: (
        Res<PresentationSamples>,
        Res<BuildSelection>,
        Res<SimulationPlayback>,
    ),
    mut selection: ResMut<InspectionSelection>,
) {
    let (fixed_time, metrics, terrain) = world;
    let (samples, build_selection, playback) = state;
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
    let alpha = playback.interpolation_alpha(&fixed_time);
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        selection.selected = None;
        return;
    };
    if let Some(builder) =
        pick_builder_on_ray(ray.origin, *ray.direction, &samples, &terrain, alpha)
    {
        selection.selected = Some(builder);
        return;
    }
    if let Some(unit) = pick_unit_on_ray(ray.origin, *ray.direction, &samples, &terrain, alpha) {
        selection.selected = Some(unit);
        return;
    }
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        selection.selected = None;
        return;
    };
    selection.selected = pick_building_at_ground(world, &samples, &metrics);
}

fn clear_stale_selection(
    samples: Res<PresentationSamples>,
    mut selection: ResMut<InspectionSelection>,
) {
    let Some(id) = selection.selected else {
        return;
    };
    if !samples.current.builders.contains_key(&id)
        && !samples.current.units.contains_key(&id)
        && !samples.current.buildings.contains_key(&id)
    {
        selection.selected = None;
    }
}

fn update_inspector_text(
    samples: Res<PresentationSamples>,
    selection: Res<InspectionSelection>,
    mut text: Single<&mut Text, With<InspectionText>>,
) {
    let next = match selection.selected {
        None => "Left-click a builder, unit, or building to inspect it.\n\nBuilders are controllable; combat-unit inspection remains read-only.".into(),
        Some(id) => inspector_text(id, &samples),
    };
    if text.0 != next {
        text.0 = next;
    }
}

fn draw_selection_highlight(
    fixed_time: Res<Time<Fixed>>,
    playback: Res<SimulationPlayback>,
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    selection: Res<InspectionSelection>,
    mut gizmos: Gizmos,
) {
    let Some(id) = selection.selected else {
        return;
    };
    let alpha = playback.interpolation_alpha(&fixed_time);

    if let Some(builder) = samples.current.builders.get(&id) {
        let previous = samples.previous.builders.get(&id).unwrap_or(builder);
        let center =
            sim_point_to_terrain_world_lerp(previous.position, builder.position, alpha, &terrain);
        gizmos.circle(
            Isometry3d::new(
                center + Vec3::Y * 0.35,
                Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
            ),
            BUILDER_PICK_RADIUS + SELECTION_RING_PADDING,
            SELECTION_COLOR,
        );
        if let Some(target) = builder.repair_target
            && let Some(target_position) =
                current_entity_position(target, &samples, &metrics, &terrain)
        {
            gizmos.line(
                center + Vec3::Y * 4.0,
                target_position + Vec3::Y * 4.0,
                SELECTION_COLOR.with_alpha(0.55),
            );
        }
        return;
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
        if let Some(target) = unit.target
            && let Some(target_position) =
                current_entity_position(target, &samples, &metrics, &terrain)
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
        draw_footprint_outline(
            &mut gizmos,
            &metrics,
            &terrain,
            building.footprint,
            SELECTION_COLOR,
        );
        if let Some(target) = building.target
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
        let center = ground + Vec3::Y * (BUILDER_VISUAL_HEIGHT * 0.5);
        let Some(distance) = ray_sphere_hit_distance(
            ray_origin,
            ray_direction,
            center,
            BUILDER_PICK_RADIUS.max(BUILDER_VISUAL_HEIGHT * 0.55),
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
        return format_unit_inspector(unit, samples.current.tick);
    }
    if let Some(building) = samples.current.buildings.get(&id) {
        return format_building_inspector(building, samples.current.tick);
    }
    format!("SimId {} is no longer present.", id.0)
}

fn format_builder_inspector(builder: &BuilderSample) -> String {
    let position = sim_point_to_world(builder.position);
    let order = match (builder.destination, builder.repair_target) {
        (Some(destination), _) => {
            let destination = sim_point_to_world(destination);
            format!("Move to {:.1}, {:.1}", destination.x, destination.z)
        }
        (_, Some(target)) => format!("Repair #{}", target.0),
        (None, None) => "Idle".into(),
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
        "Controls: Right-click move/repair • D then right-click Blink • R toggle repair autocast"
            .into(),
    ]
    .join("\n")
}

fn format_unit_inspector(unit: &UnitSample, tick: u64) -> String {
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
        format!("Last attacker: {}", target_label(unit.last_attacker)),
        format!(
            "Last attacked: {}",
            attacked_tick_label(unit.last_attacked_tick, tick)
        ),
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

fn attacked_tick_label(attacked_tick: Option<u64>, tick: u64) -> String {
    attacked_tick.map_or_else(
        || "Never".into(),
        |attacked_tick| {
            let age = tick.saturating_sub(attacked_tick);
            format!("tick {attacked_tick} ({age} ticks ago)")
        },
    )
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

pub(crate) fn cursor_over_inspector_panel(cursor: Vec2, window_width: f32) -> bool {
    let left = window_width - PANEL_RIGHT - PANEL_WIDTH;
    cursor.x >= left
        && cursor.x <= window_width - PANEL_RIGHT
        && cursor.y >= PANEL_TOP
        && cursor.y <= PANEL_TOP + PANEL_HEIGHT
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use castle_fight_sim::{
        BuilderLocomotion, BuildingFootprint, ContentIdentity, MovementClass, NavCell, SimPoint,
        SimulationConfig, TerrainElevationMap,
    };

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

    fn empty_samples() -> PresentationSamples {
        let snapshot = PresentationSnapshot {
            tick: 10,
            units: BTreeMap::new(),
            builders: BTreeMap::new(),
            buildings: BTreeMap::new(),
            corpses: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            attacks: Vec::new(),
            ability_casts: Vec::new(),
            chain_lightnings: Vec::new(),
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
                repair_target: None,
                repair_autocast_enabled: true,
                blink_range: 10_000 * SUBUNITS_PER_WORLD_UNIT,
                build_catalog_len: 7,
            },
        );
        let terrain = flat_terrain();
        let center =
            sim_point_to_terrain_world(samples.current.builders[&SimId(5)].position, &terrain)
                + Vec3::Y * (BUILDER_VISUAL_HEIGHT * 0.5);
        assert_eq!(
            pick_builder_on_ray(
                Vec3::new(center.x, center.y, 0.0),
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
        assert!(text.contains("D then right-click Blink"));
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
                team: Team(0),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                collision_radius: 4 * SUBUNITS_PER_WORLD_UNIT,
                movement_class: castle_fight_sim::MovementClass::Ground,
                mechanical: false,
                health: 50,
                target: None,
                direct_retaliation_lock: false,
                ally_defense_lock: false,
                last_attacker: None,
                last_attacked_tick: None,
                cooldown_remaining: 0,
                stunned_until_tick: 0,
                mana_current: None,
                mana_maximum: None,
                visual_kind: UnitVisualKind::Melee,
            },
        );
        let terrain = flat_terrain();
        assert_eq!(
            pick_unit_on_ray(Vec3::new(103.0, 5.0, 0.0), Vec3::Z, &samples, &terrain, 1.0,),
            Some(SimId(7))
        );
        assert!(
            format_unit_inspector(&samples.current.units[&SimId(7)], 10).contains("Name: Footman")
        );
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
                team: Team(0),
                position: SimPoint::new(
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    100 * SUBUNITS_PER_WORLD_UNIT,
                ),
                collision_radius: 4 * SUBUNITS_PER_WORLD_UNIT,
                movement_class: MovementClass::Air,
                mechanical: false,
                health: 50,
                target: None,
                direct_retaliation_lock: false,
                ally_defense_lock: false,
                last_attacker: None,
                last_attacked_tick: None,
                cooldown_remaining: 0,
                stunned_until_tick: 0,
                mana_current: None,
                mana_maximum: None,
                visual_kind: UnitVisualKind::Melee,
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
