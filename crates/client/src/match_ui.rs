//! Match HUD and Rescue Strike targeting/presentation.
use crate::{
    AuthoritativeSimulation, SelectedMatch,
    build_ui::{ActionPanelState, TargetingAction, cursor_over_action_panel},
    debug_menu::{DebugMenuState, cursor_over_debug_menu},
    inspection::cursor_over_inspector_panel,
    lobby::LobbyState,
    presentation::{sim_point_to_terrain_world, viewport_ground_point, world_to_sim_point},
    resource_ui::{
        BuilderShortcutState, TOP_BAR_HEIGHT, cursor_over_builder_shortcuts,
        cursor_over_map_controls,
    },
    terrain::TerrainSurface,
    wc3_effects::{Wc3RibbonSource, Wc3VisualSet},
};
use bevy::{
    ecs::system::SystemParam, prelude::*, window::PrimaryWindow,
    world_serialization::WorldAssetRoot,
};
use castle_fight_sim::{
    MatchLifecycle, MatchOutcome, RescueStrikeRules, SUBUNITS_PER_WORLD_UNIT, SimPoint, Team,
};

pub(crate) struct MatchUiPlugin;

#[derive(Component)]
struct MatchHud;

#[derive(Component)]
struct MatchHudText;

#[derive(Resource, Default)]
struct RescueImpactDisplay {
    last_tick: Option<u64>,
    impacts: Vec<RescueImpact>,
}

#[derive(Resource, Default)]
struct RescueArtTracker {
    active: Vec<(castle_fight_sim::SimId, SimPoint)>,
}

#[derive(Component)]
struct RescueArtLifetime(f32);

struct RescueImpact {
    center: SimPoint,
    targets: Vec<castle_fight_sim::SimId>,
    life: f32,
}

impl Plugin for MatchUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RescueImpactDisplay>()
            .init_resource::<RescueArtTracker>()
            .add_systems(Startup, spawn_match_hud)
            .add_systems(
                Update,
                (
                    update_match_hud,
                    capture_rescue_impacts,
                    draw_rescue_targeting,
                    sync_rescue_wc3_art,
                ),
            );
    }
}

fn spawn_match_hud(mut commands: Commands) {
    commands
        .spawn((
            MatchHud,
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP_BAR_HEIGHT + 5.0),
                left: Val::Percent(33.0),
                width: Val::Percent(34.0),
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(12.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.055, 0.048, 0.036, 0.83)),
            BorderColor::all(Color::srgb(0.33, 0.27, 0.16)),
            Visibility::Hidden,
            ZIndex(15),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent.spawn((
                MatchHudText,
                Text::new(""),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.96, 0.89, 0.72)),
                TextLayout::justify(Justify::Center),
            ));
        });
}

fn update_match_hud(
    authoritative: Res<AuthoritativeSimulation>,
    lobby: Option<Res<LobbyState>>,
    mut container: Query<&mut Visibility, With<MatchHud>>,
    mut text: Query<&mut Text, With<MatchHudText>>,
) {
    let Ok(mut visibility) = container.single_mut() else {
        return;
    };
    if lobby.as_ref().is_some_and(|lobby| lobby.active()) {
        *visibility = Visibility::Hidden;
        return;
    }
    *visibility = Visibility::Visible;
    let sim = &authoritative.simulation;
    let wins = sim.round_wins();
    let strikes = [
        sim.rescue_strikes_left(Team(0)),
        sim.rescue_strikes_left(Team(1)),
    ];
    let timer = if let Some(seconds) = sim.review_seconds_remaining() {
        format!("Next round in {seconds}s")
    } else {
        sim.match_rules()
            .duration_ticks()
            .map_or("No time limit".to_owned(), |limit| {
                let remaining = limit.saturating_sub(sim.round_elapsed_ticks());
                let seconds =
                    remaining.div_ceil(castle_fight_sim::CASTLE_FIGHT_SIMULATION_HZ as u64);
                format!("{:02}:{:02} remaining", seconds / 60, seconds % 60)
            })
    };
    let result = match sim.lifecycle() {
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(Team(0)),
            ..
        } => " · WEST WINS MATCH",
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(Team(1)),
            ..
        } => " · EAST WINS MATCH",
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(_),
            ..
        } => " · MATCH FINISHED",
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Draw,
            ..
        } => " · DRAW",
        MatchLifecycle::PausedForDisconnect { .. } => " · PAUSED FOR DISCONNECT",
        MatchLifecycle::RoundReview { .. } => " · ROUND COMPLETE",
        MatchLifecycle::Running => "",
    };
    if let Ok(mut text) = text.single_mut() {
        text.0 = format!(
            "Round {} · West {}–{} East · {timer} · Rescue {}/{} · F9: Scores {result}",
            sim.round_index(),
            wins[0],
            wins[1],
            strikes[0],
            strikes[1]
        );
    }
}

fn capture_rescue_impacts(
    authoritative: Res<AuthoritativeSimulation>,
    time: Res<Time>,
    mut display: ResMut<RescueImpactDisplay>,
) {
    for impact in &mut display.impacts {
        impact.life -= time.delta_secs();
    }
    display.impacts.retain(|impact| impact.life > 0.0);
    let tick = authoritative.simulation.tick();
    if display.last_tick == Some(tick) {
        return;
    }
    display.last_tick = Some(tick);
    for event in authoritative.simulation.rescue_strike_visuals_last_tick() {
        display.impacts.push(RescueImpact {
            center: event.center,
            targets: event.targets.clone(),
            life: 1.3,
        });
    }
}

/// Use the same converted classic models requested by the map's Rescue Strike script.
/// The original spawns a Tome of Retraining cast effect and three Wisp explosions.
fn sync_rescue_wc3_art(
    mut commands: Commands,
    authoritative: Res<AuthoritativeSimulation>,
    terrain: Res<TerrainSurface>,
    visuals: Res<Wc3VisualSet>,
    time: Res<Time>,
    mut tracker: ResMut<RescueArtTracker>,
    mut existing: Query<(Entity, &mut RescueArtLifetime)>,
) {
    for (entity, mut lifetime) in &mut existing {
        lifetime.0 -= time.delta_secs();
        if lifetime.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    let pending = authoritative.simulation.pending_rescue_strikes();
    for &(builder, center) in &pending {
        if tracker.active.contains(&(builder, center)) {
            continue;
        }
        if let Some(model) = visuals.system_model("Units\\\\NightElf\\\\Wisp\\\\WispExplode.mdl") {
            for _ in 0..3 {
                spawn_rescue_model(
                    &mut commands,
                    model,
                    sim_point_to_terrain_world(center, &terrain),
                );
            }
        }
        if let Some(model) = visuals.system_model(
            "Abilities\\\\Spells\\\\Items\\\\TomeOfRetraining\\\\TomeOfRetrainingCaster.mdl",
        ) && let Some(caster) = authoritative.simulation.builder(builder)
        {
            spawn_rescue_model(
                &mut commands,
                model,
                sim_point_to_terrain_world(caster.position, &terrain),
            );
        }
    }
    tracker.active = pending;
}

fn spawn_rescue_model(
    commands: &mut Commands,
    model: &crate::wc3_effects::Wc3VisualModel,
    position: Vec3,
) {
    let mut entity = commands.spawn((
        WorldAssetRoot(model.scene.clone()),
        Transform::from_translation(position),
        model.emitter_source(),
        Wc3RibbonSource::new(&model.ribbons),
        RescueArtLifetime(model.effect_lifetime_seconds(1.8)),
    ));
    if let Some(animation) = model.animation_source() {
        entity.insert(animation);
    }
}

#[derive(SystemParam)]
struct RescueTargetWorld<'w, 's> {
    window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform), With<Camera3d>>,
    terrain: Res<'w, TerrainSurface>,
    selected: Res<'w, SelectedMatch>,
}

fn draw_rescue_targeting(
    world: RescueTargetWorld<'_, '_>,
    authoritative: Res<AuthoritativeSimulation>,
    panel: Res<ActionPanelState>,
    debug_menu: Res<DebugMenuState>,
    shortcuts: Res<BuilderShortcutState>,
    display: Res<RescueImpactDisplay>,
    mut gizmos: Gizmos,
) {
    let RescueTargetWorld {
        window,
        camera,
        terrain,
        selected,
    } = world;
    let Some(rules) = RescueStrikeRules::for_version(selected.content.map_version) else {
        return;
    };
    let radius = rules.radius_subunits as f32 / SUBUNITS_PER_WORLD_UNIT as f32;
    let orientation = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
    let sim = &authoritative.simulation;
    let mut cursor_target = None;
    if panel.targeting() == Some(TargetingAction::RescueStrike)
        && let Some(cursor) = window.cursor_position()
        && !cursor_over_action_panel(cursor, window.width(), window.height(), true)
        && !cursor_over_inspector_panel(cursor, window.width(), window.height())
        && !cursor_over_debug_menu(cursor, debug_menu.is_open())
        && !cursor_over_builder_shortcuts(cursor, &shortcuts)
        && !cursor_over_map_controls(cursor, window.width())
        && let Some(world) = viewport_ground_point(camera.0, camera.1, cursor, &terrain)
    {
        let center = world_to_sim_point(world);
        cursor_target = Some(center);
        let location = sim_point_to_terrain_world(center, &terrain) + Vec3::Y * 2.0;
        gizmos.circle(
            Isometry3d::new(location, orientation),
            radius,
            Color::srgb(0.98, 0.82, 0.24),
        );
        gizmos.circle(
            Isometry3d::new(location, orientation),
            radius * 0.05,
            Color::srgb(1.0, 0.28, 0.15),
        );
    }
    if let Some(center) = cursor_target {
        let radius_sq = i64::from(rules.radius_subunits).pow(2);
        let team = panel
            .actor
            .and_then(|actor| sim.builder(actor))
            .map(|builder| builder.team);
        for unit in sim.units() {
            if team == Some(unit.team) || unit.classifications.invulnerable || unit.health <= 0 {
                continue;
            }
            let dx = i64::from(unit.position.x) - i64::from(center.x);
            let dy = i64::from(unit.position.y) - i64::from(center.y);
            if dx * dx + dy * dy > radius_sq {
                continue;
            }
            let ground = sim_point_to_terrain_world(unit.position, &terrain) + Vec3::Y * 2.0;
            let marker = Color::srgb(1.0, 0.26, 0.16);
            gizmos.circle(Isometry3d::new(ground, orientation), 24.0, marker);
            gizmos.line(ground, ground + Vec3::Y * 65.0, marker.with_alpha(0.8));
        }
    }
    for center in sim.pending_rescue_strike_centers() {
        let ground = sim_point_to_terrain_world(center, &terrain) + Vec3::Y * 3.0;
        gizmos.circle(
            Isometry3d::new(ground, orientation),
            radius,
            Color::srgb(1.0, 0.41, 0.11),
        );
    }
    for impact in &display.impacts {
        let fraction = impact.life / 1.3;
        let ground = sim_point_to_terrain_world(impact.center, &terrain) + Vec3::Y * 4.0;
        gizmos.circle(
            Isometry3d::new(ground, orientation),
            radius * (1.0 + (1.0 - fraction) * 0.2),
            Color::srgba(1.0, 0.3, 0.06, fraction),
        );
        gizmos.circle(
            Isometry3d::new(ground + Vec3::Y * 30.0, orientation),
            radius * fraction,
            Color::srgba(1.0, 0.73, 0.14, fraction),
        );
        for target in &impact.targets {
            if let Some(unit) = sim.unit(*target) {
                let ground = sim_point_to_terrain_world(unit.position, &terrain);
                gizmos.line(
                    ground,
                    ground + Vec3::Y * 135.0 * fraction,
                    Color::srgba(1.0, 0.35, 0.1, fraction),
                );
            }
        }
    }
}
