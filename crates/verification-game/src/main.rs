use std::collections::{HashMap, HashSet};

use bevy::{camera::ScalingMode, prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile,
    AutomaticAbilityProfile, BuildingFootprint, BuildingSpawn, BuildingView, ManaProfile,
    MovementProfile, NavCell, ProductionProfile, ProjectileViewKind, SUBUNITS_PER_WORLD_UNIT,
    SimId, SimPoint, Simulation, SimulationConfig, SpellcastingProfile, Team, UnitTemplate,
    UnitView,
};

const SIMULATION_HZ: f64 = 30.0;
const SIMULATION_HZ_I32: i32 = 30;
const MAP_WIDTH: f32 = 2_000.0;
const MAP_HEIGHT: f32 = 750.0;
const NAV_CELL_WORLD: i32 = 10;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const NAV_MAX_X: i32 = 199;
const NAV_MAX_Y: i32 = 74;
const PLAYER_BASE_MAX_X: i32 = 66;
const MIDDLE_MIN_X: i32 = 67;
const MIDDLE_MAX_X: i32 = 132;
const LANE_MIN_Y: i32 = 20;
const LANE_MAX_Y: i32 = 54;
const PRODUCTION_BUILDING_SIZE: u16 = 4;
const PRODUCTION_INTERVAL_TICKS: u16 = 300;
const ATTACK_COOLDOWN_TICKS: u16 = 30;
const RANGED_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 300;
const ARTILLERY_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 90;
const TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 110;
const ATTACK_TRACE_SECONDS: f32 = 0.18;
const PLAYER_CASTLE: BuildingFootprint = BuildingFootprint::new(30, 34, 7, 7);
const ENEMY_CASTLE: BuildingFootprint = BuildingFootprint::new(163, 34, 7, 7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductionKind {
    Melee,
    Ranged,
    Artillery,
    Spellcaster,
}

impl ProductionKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Melee => "melee production",
            Self::Ranged => "ranged production",
            Self::Artillery => "artillery production",
            Self::Spellcaster => "spellcaster production",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildKind {
    Production(ProductionKind),
    GuaranteedTower,
    ProjectileTower,
    GlobalAreaSpell,
}

impl BuildKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Production(kind) => kind.label(),
            Self::GuaranteedTower => "guaranteed-hit tower",
            Self::ProjectileTower => "projectile splash tower",
            Self::GlobalAreaSpell => "global-random AOE spell building",
        }
    }

    const fn footprint_size(self) -> u16 {
        match self {
            Self::GuaranteedTower | Self::ProjectileTower => 3,
            Self::Production(_) | Self::GlobalAreaSpell => PRODUCTION_BUILDING_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingPlacement {
    kind: BuildKind,
    footprint: BuildingFootprint,
}

#[derive(Resource, Default)]
struct PendingPlacements(Vec<PendingPlacement>);

#[derive(Resource)]
struct UiStatus {
    text: String,
    selected: BuildKind,
}

impl Default for UiStatus {
    fn default() -> Self {
        Self {
            text: "1 melee • 2 ranged • 3 artillery • 4 caster • 5 hit tower • 6 splash tower • 7 global AOE • LMB place • RMB quick ranged"
                .into(),
            selected: BuildKind::Production(ProductionKind::Melee),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AttackTrace {
    start: Vec2,
    end: Vec2,
    delivery: AttackDelivery,
    remaining: f32,
}

#[derive(Resource, Default)]
struct AttackVisuals(Vec<AttackTrace>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchOutcome {
    PlayerVictory,
    EnemyVictory,
    Draw,
}

impl MatchOutcome {
    const fn label(self) -> &'static str {
        match self {
            Self::PlayerVictory => "PLAYER VICTORY",
            Self::EnemyVictory => "ENEMY VICTORY",
            Self::Draw => "DRAW",
        }
    }
}

#[derive(Resource)]
struct GameState {
    simulation: Simulation,
    presented_units: HashMap<SimId, Entity>,
    presented_buildings: HashMap<SimId, Entity>,
    player_castle: SimId,
    enemy_castle: SimId,
    outcome: Option<MatchOutcome>,
}

impl GameState {
    fn step_match(&mut self) -> bool {
        if self.outcome.is_some() {
            return false;
        }
        self.simulation.step();
        let player_alive = self.simulation.building(self.player_castle).is_some();
        let enemy_alive = self.simulation.building(self.enemy_castle).is_some();
        self.outcome = match (player_alive, enemy_alive) {
            (true, true) => None,
            (true, false) => Some(MatchOutcome::PlayerVictory),
            (false, true) => Some(MatchOutcome::EnemyVictory),
            (false, false) => Some(MatchOutcome::Draw),
        };
        true
    }
}

#[derive(Component)]
struct VerificationCamera;

#[derive(Component)]
struct PresentedUnit;

#[derive(Component)]
struct PresentedBuilding;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .init_resource::<PendingPlacements>()
        .init_resource::<AttackVisuals>()
        .init_resource::<UiStatus>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Castle Fight Native — playable verification".into(),
                resolution: (1440, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, apply_placements_and_step)
        .add_systems(
            Update,
            (
                queue_build_input,
                sync_presentation,
                draw_cursor_preview,
                update_window_title,
            ),
        )
        .add_systems(Update, age_attack_traces.before(draw_attack_traces))
        .add_systems(Update, (draw_attack_traces, draw_authoritative_projectiles))
        .run();
}

fn setup(mut commands: Commands) {
    spawn_map_visuals(&mut commands);
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 1_020.0,
            },
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0, 0.0),
        VerificationCamera,
    ));

    let mut simulation = Simulation::new(verification_config(), default_worker_count());
    let player_castle = simulation.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: PLAYER_CASTLE,
        health: 500,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let enemy_castle = simulation.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: ENEMY_CASTLE,
        health: 500,
        production: None,
        attack: None,
        spellcasting: None,
    });

    commands.insert_resource(GameState {
        simulation,
        presented_units: HashMap::new(),
        presented_buildings: HashMap::new(),
        player_castle,
        enemy_castle,
        outcome: None,
    });
}

fn verification_config() -> SimulationConfig {
    SimulationConfig {
        match_seed: 0,
        spatial_cell_size: 40 * SUBUNITS_PER_WORLD_UNIT,
        navigation_cell_size: NAV_CELL_SUBUNITS,
        navigation_min: NavCell::new(0, 0),
        navigation_max: NavCell::new(NAV_MAX_X, NAV_MAX_Y),
        target_pursuit_extra_range: 30 * SUBUNITS_PER_WORLD_UNIT,
        unit_separation_distance: 8 * SUBUNITS_PER_WORLD_UNIT,
        max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT,
        static_blockers: vec![
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                0,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                LANE_MIN_Y as u16,
            ),
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                LANE_MAX_Y + 1,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                (NAV_MAX_Y - LANE_MAX_Y) as u16,
            ),
        ],
        // These are approach cells immediately outside the opposing castle footprints.
        team_objective: [cell_center(162, 37), cell_center(37, 37)],
    }
}

fn spawn_map_visuals(commands: &mut Commands) {
    spawn_rect(
        commands,
        Vec2::new(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0),
        Vec2::new(MAP_WIDTH, MAP_HEIGHT),
        Color::srgb(0.10, 0.11, 0.12),
        -20.0,
    );

    spawn_rect(
        commands,
        Vec2::new(335.0, 375.0),
        Vec2::new(670.0, 750.0),
        Color::srgb(0.075, 0.11, 0.16),
        -19.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_665.0, 375.0),
        Vec2::new(670.0, 750.0),
        Color::srgb(0.16, 0.075, 0.075),
        -19.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 375.0),
        Vec2::new(660.0, 350.0),
        Color::srgb(0.16, 0.16, 0.145),
        -18.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 100.0),
        Vec2::new(660.0, 200.0),
        Color::srgb(0.035, 0.04, 0.045),
        -18.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 650.0),
        Vec2::new(660.0, 200.0),
        Color::srgb(0.035, 0.04, 0.045),
        -18.0,
    );

    let border = Color::srgb(0.35, 0.37, 0.39);
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 0.0),
        Vec2::new(2_004.0, 4.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 750.0),
        Vec2::new(2_004.0, 4.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(0.0, 375.0),
        Vec2::new(4.0, 754.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(2_000.0, 375.0),
        Vec2::new(4.0, 754.0),
        border,
        -10.0,
    );

    let division = Color::srgba(0.8, 0.82, 0.85, 0.25);
    spawn_rect(
        commands,
        Vec2::new(670.0, 375.0),
        Vec2::new(2.0, 750.0),
        division,
        -9.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_330.0, 375.0),
        Vec2::new(2.0, 750.0),
        division,
        -9.0,
    );
}

fn spawn_rect(commands: &mut Commands, center: Vec2, size: Vec2, color: Color, z: f32) {
    commands.spawn((
        Sprite::from_color(color, size),
        Transform::from_xyz(center.x, center.y, z),
    ));
}

fn queue_build_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform), With<VerificationCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    state: Res<GameState>,
    mut pending: ResMut<PendingPlacements>,
    mut status: ResMut<UiStatus>,
) {
    if let Some(kind) = build_kind_hotkey(&keys) {
        status.selected = kind;
        status.text = format!("Selected {}", kind.label());
    }

    if let Some(outcome) = state.outcome {
        if buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right) {
            status.text = format!("{} — simulation stopped", outcome.label());
        }
        return;
    }
    let kind = if buttons.just_pressed(MouseButton::Left) {
        Some(status.selected)
    } else if buttons.just_pressed(MouseButton::Right) {
        Some(BuildKind::Production(ProductionKind::Ranged))
    } else {
        None
    };
    let Some(kind) = kind else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };
    let Some(footprint) = player_footprint_at(world, kind.footprint_size()) else {
        status.text = "Placement rejected: click inside the blue player base".into();
        return;
    };

    pending.0.push(PendingPlacement { kind, footprint });
    status.text = format!("Queued mirrored {}", kind.label());
}

fn build_kind_hotkey(keys: &ButtonInput<KeyCode>) -> Option<BuildKind> {
    [
        (
            KeyCode::Digit1,
            BuildKind::Production(ProductionKind::Melee),
        ),
        (
            KeyCode::Digit2,
            BuildKind::Production(ProductionKind::Ranged),
        ),
        (
            KeyCode::Digit3,
            BuildKind::Production(ProductionKind::Artillery),
        ),
        (
            KeyCode::Digit4,
            BuildKind::Production(ProductionKind::Spellcaster),
        ),
        (KeyCode::Digit5, BuildKind::GuaranteedTower),
        (KeyCode::Digit6, BuildKind::ProjectileTower),
        (KeyCode::Digit7, BuildKind::GlobalAreaSpell),
    ]
    .into_iter()
    .find_map(|(key, kind)| keys.just_pressed(key).then_some(kind))
}

fn apply_placements_and_step(
    mut state: ResMut<GameState>,
    mut pending: ResMut<PendingPlacements>,
    mut status: ResMut<UiStatus>,
    mut attacks: ResMut<AttackVisuals>,
) {
    if state.outcome.is_some() {
        pending.0.clear();
        return;
    }

    for placement in pending.0.drain(..) {
        let enemy_footprint = mirror_footprint(placement.footprint);
        if !state.simulation.can_place_building(placement.footprint)
            || !state.simulation.can_place_building(enemy_footprint)
        {
            status.text = "Placement rejected: footprint is occupied or blocked".into();
            continue;
        }

        spawn_verification_building(
            &mut state.simulation,
            Team(0),
            placement.footprint,
            placement.kind,
        );
        spawn_verification_building(
            &mut state.simulation,
            Team(1),
            enemy_footprint,
            placement.kind,
        );
        status.text = format!("Placed mirrored {}", placement.kind.label());
    }

    let stepped = state.step_match();
    debug_assert!(
        stepped,
        "running verification match unexpectedly refused a tick"
    );
    attacks.0.extend(
        state
            .simulation
            .attacks_last_tick()
            .iter()
            .copied()
            .filter(|event| matches!(event.delivery, AttackDelivery::Melee))
            .map(|event| AttackTrace {
                start: sim_point_to_world(event.source_position),
                end: sim_point_to_world(event.target_position),
                delivery: event.delivery,
                remaining: ATTACK_TRACE_SECONDS,
            }),
    );

    if let Some(outcome) = state.outcome {
        pending.0.clear();
        status.text = format!("{} — simulation stopped", outcome.label());
    }
}

fn spawn_verification_building(
    simulation: &mut Simulation,
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> SimId {
    match kind {
        BuildKind::Production(ProductionKind::Spellcaster) => simulation
            .spawn_building_with_production_spellcasting(
                production_building(team, footprint, ProductionKind::Spellcaster),
                short_range_spellcaster_profile(),
            ),
        BuildKind::Production(kind) => {
            simulation.spawn_building(production_building(team, footprint, kind))
        }
        BuildKind::GuaranteedTower => {
            simulation.spawn_building(attack_building(team, footprint, guaranteed_tower_attack()))
        }
        BuildKind::ProjectileTower => {
            simulation.spawn_building(attack_building(team, footprint, projectile_tower_attack()))
        }
        BuildKind::GlobalAreaSpell => {
            simulation.spawn_building(spell_building(team, footprint, global_area_spell_profile()))
        }
    }
}

fn production_building(
    team: Team,
    footprint: BuildingFootprint,
    kind: ProductionKind,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 100,
        production: Some(ProductionProfile {
            initial_delay_ticks: PRODUCTION_INTERVAL_TICKS,
            interval_ticks: PRODUCTION_INTERVAL_TICKS,
            search_radius_cells: 12,
            unit: unit_template(kind),
        }),
        attack: None,
        spellcasting: None,
    }
}

fn attack_building(
    team: Team,
    footprint: BuildingFootprint,
    attack: AttackProfile,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 160,
        production: None,
        attack: Some(attack),
        spellcasting: None,
    }
}

fn spell_building(
    team: Team,
    footprint: BuildingFootprint,
    spellcasting: SpellcastingProfile,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 140,
        production: None,
        attack: None,
        spellcasting: Some(spellcasting),
    }
}

fn unit_template(kind: ProductionKind) -> UnitTemplate {
    let speed_per_tick = 40 * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32;
    match kind {
        ProductionKind::Melee => UnitTemplate {
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 14 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
        ProductionKind::Ranged => UnitTemplate {
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: RANGED_PROJECTILE_SPEED_WORLD_PER_SECOND
                        * SUBUNITS_PER_WORLD_UNIT
                        / SIMULATION_HZ_I32,
                },
                damage: 1,
                range: 120 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 180 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
        ProductionKind::Artillery => UnitTemplate {
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedBallistic {
                    speed_per_tick: ARTILLERY_PROJECTILE_SPEED_WORLD_PER_SECOND
                        * SUBUNITS_PER_WORLD_UNIT
                        / SIMULATION_HZ_I32,
                    impact_radius: 35 * SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 2,
                range: 260 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 340 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 2 * ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
        ProductionKind::Spellcaster => UnitTemplate {
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 14 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
    }
}

fn guaranteed_tower_attack() -> AttackProfile {
    AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: RANGED_PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                / SIMULATION_HZ_I32,
        },
        damage: 2,
        range: 300 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 300 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: ATTACK_COOLDOWN_TICKS,
    }
}

fn projectile_tower_attack() -> AttackProfile {
    AttackProfile {
        delivery: AttackDelivery::RangedBallistic {
            speed_per_tick: TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                / SIMULATION_HZ_I32,
            impact_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 4,
        range: 340 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 340 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 2 * ATTACK_COOLDOWN_TICKS,
    }
}

fn global_area_spell_profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 180,
            starting: 0,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1_001),
            mana_cost: 180,
            cooldown_ticks: 1,
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
            effect: AbilityEffect::AreaDamage {
                amount: 3,
                radius: 50 * SUBUNITS_PER_WORLD_UNIT,
            },
        },
    }
}

fn short_range_spellcaster_profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 120,
            starting: 0,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1_002),
            mana_cost: 120,
            cooldown_ticks: 1,
            range: 90 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::AreaDamage {
                amount: 2,
                radius: 30 * SUBUNITS_PER_WORLD_UNIT,
            },
        },
    }
}

fn sync_presentation(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut unit_transforms: Query<&mut Transform, (With<PresentedUnit>, Without<PresentedBuilding>)>,
    mut building_transforms: Query<
        &mut Transform,
        (With<PresentedBuilding>, Without<PresentedUnit>),
    >,
) {
    let units = state.simulation.units();
    let live_units: HashSet<_> = units.iter().map(|unit| unit.id).collect();
    let stale_units: Vec<_> = state
        .presented_units
        .iter()
        .filter(|(id, _)| !live_units.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale_units {
        commands.entity(entity).despawn();
        state.presented_units.remove(&id);
    }

    for unit in units {
        let position = sim_point_to_world(unit.position).extend(5.0);
        if let Some(&entity) = state.presented_units.get(&unit.id) {
            if let Ok(mut transform) = unit_transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(unit_color(&unit), unit_size(&unit)),
                Transform::from_translation(position),
                PresentedUnit,
            ))
            .id();
        state.presented_units.insert(unit.id, entity);
    }

    let buildings = state.simulation.buildings();
    let live_buildings: HashSet<_> = buildings.iter().map(|building| building.id).collect();
    let stale_buildings: Vec<_> = state
        .presented_buildings
        .iter()
        .filter(|(id, _)| !live_buildings.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale_buildings {
        commands.entity(entity).despawn();
        state.presented_buildings.remove(&id);
    }

    for building in buildings {
        let (center, size) = footprint_world_rect(building.footprint);
        let position = center.extend(3.0);
        if let Some(&entity) = state.presented_buildings.get(&building.id) {
            if let Ok(mut transform) = building_transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(building_color(&building), size - Vec2::splat(2.0)),
                Transform::from_translation(position),
                PresentedBuilding,
            ))
            .id();
        state.presented_buildings.insert(building.id, entity);
    }
}

fn age_attack_traces(time: Res<Time>, mut attacks: ResMut<AttackVisuals>) {
    let elapsed = time.delta_secs();
    for trace in &mut attacks.0 {
        trace.remaining -= elapsed;
    }
    attacks.0.retain(|trace| trace.remaining > 0.0);
}

fn draw_attack_traces(mut gizmos: Gizmos, attacks: Res<AttackVisuals>) {
    for trace in &attacks.0 {
        let color = match trace.delivery {
            AttackDelivery::Melee => Color::srgba(1.0, 0.92, 0.62, 0.9),
            AttackDelivery::RangedGuaranteedHit { .. } => Color::srgba(0.72, 0.95, 1.0, 0.95),
            AttackDelivery::RangedBallistic { .. } => Color::srgba(1.0, 0.78, 0.38, 0.95),
            AttackDelivery::Bounce { .. } => Color::srgba(0.72, 1.0, 0.45, 0.95),
        };
        gizmos.line_2d(trace.start, trace.end, color);
    }
}

fn draw_authoritative_projectiles(state: Res<GameState>, mut gizmos: Gizmos) {
    let tick = state.simulation.tick();
    for projectile in state.simulation.projectiles() {
        let (target, color) = match projectile.kind {
            ProjectileViewKind::GuaranteedHit { target } => {
                let target = if let Some(unit) = state.simulation.unit(target) {
                    sim_point_to_world(unit.position)
                } else if let Some(building) = state.simulation.building(target) {
                    footprint_world_rect(building.footprint).0
                } else {
                    continue;
                };
                (target, Color::srgba(0.72, 0.95, 1.0, 0.95))
            }
            ProjectileViewKind::Ballistic { destination, .. } => (
                sim_point_to_world(destination),
                Color::srgba(1.0, 0.78, 0.38, 0.95),
            ),
            ProjectileViewKind::Bounce { target, .. } => {
                let target = if let Some(unit) = state.simulation.unit(target) {
                    sim_point_to_world(unit.position)
                } else if let Some(building) = state.simulation.building(target) {
                    footprint_world_rect(building.footprint).0
                } else {
                    continue;
                };
                (target, Color::srgba(0.72, 1.0, 0.45, 0.95))
            }
        };
        let start = sim_point_to_world(projectile.launch_position);
        let travel_ticks = projectile
            .impact_tick
            .saturating_sub(projectile.launch_tick)
            .max(1);
        let elapsed_ticks = tick
            .saturating_sub(projectile.launch_tick)
            .min(travel_ticks);
        let progress = elapsed_ticks as f32 / travel_ticks as f32;
        let position = start.lerp(target, progress);
        let direction = (target - start).normalize_or_zero();
        let half_length = 3.0;
        gizmos.line_2d(
            position - direction * half_length,
            position + direction * half_length,
            color,
        );
    }
}

fn draw_cursor_preview(
    camera: Single<(&Camera, &GlobalTransform), With<VerificationCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    state: Res<GameState>,
    status: Res<UiStatus>,
    mut gizmos: Gizmos,
) {
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };
    let Some(player) = player_footprint_at(world, status.selected.footprint_size()) else {
        return;
    };
    let enemy = mirror_footprint(player);
    let valid =
        state.simulation.can_place_building(player) && state.simulation.can_place_building(enemy);
    let color = if valid {
        Color::srgba(0.55, 1.0, 0.65, 0.9)
    } else {
        Color::srgba(1.0, 0.35, 0.35, 0.9)
    };
    draw_footprint_outline(&mut gizmos, player, color);
    draw_footprint_outline(&mut gizmos, enemy, color.with_alpha(0.45));
}

fn draw_footprint_outline(gizmos: &mut Gizmos, footprint: BuildingFootprint, color: Color) {
    let (center, size) = footprint_world_rect(footprint);
    let half = size / 2.0;
    let min = center - half;
    let max = center + half;
    gizmos.line_2d(Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), color);
    gizmos.line_2d(Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), color);
    gizmos.line_2d(Vec2::new(max.x, max.y), Vec2::new(min.x, max.y), color);
    gizmos.line_2d(Vec2::new(min.x, max.y), Vec2::new(min.x, min.y), color);
}

fn update_window_title(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    state: Res<GameState>,
    status: Res<UiStatus>,
) {
    let seconds = state.simulation.tick() as f64 / SIMULATION_HZ;
    let player_castle_hp = state
        .simulation
        .building(state.player_castle)
        .map_or(0, |castle| castle.health);
    let enemy_castle_hp = state
        .simulation
        .building(state.enemy_castle)
        .map_or(0, |castle| castle.health);
    let outcome = state
        .outcome
        .map(|outcome| format!(" | {}", outcome.label()))
        .unwrap_or_default();
    window.title = format!(
        "Castle Fight verification | selected: {} | 1-7 select • LMB place • RMB quick ranged | t={seconds:.1}s • units={} • buildings={} • castles {player_castle_hp}/{enemy_castle_hp}{outcome} | {}",
        status.selected.label(),
        state.simulation.unit_count(),
        state.simulation.building_count(),
        status.text,
    );
}

fn player_footprint_at(world: Vec2, size: u16) -> Option<BuildingFootprint> {
    if !(0.0..MAP_WIDTH).contains(&world.x) || !(0.0..MAP_HEIGHT).contains(&world.y) {
        return None;
    }
    let cell_x = (world.x / NAV_CELL_WORLD as f32).floor() as i32;
    let cell_y = (world.y / NAV_CELL_WORLD as f32).floor() as i32;
    let footprint = BuildingFootprint::new(
        cell_x - i32::from(size / 2),
        cell_y - i32::from(size / 2),
        size,
        size,
    );
    (footprint.min_x >= 0
        && footprint.max_x() <= PLAYER_BASE_MAX_X
        && footprint.min_y >= 0
        && footprint.max_y() <= NAV_MAX_Y)
        .then_some(footprint)
}

fn mirror_footprint(footprint: BuildingFootprint) -> BuildingFootprint {
    BuildingFootprint::new(
        NAV_MAX_X - footprint.max_x(),
        footprint.min_y,
        footprint.width,
        footprint.height,
    )
}

fn cell_center(x: i32, y: i32) -> SimPoint {
    SimPoint::new(
        x * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
        y * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
    )
}

fn sim_point_to_world(point: SimPoint) -> Vec2 {
    Vec2::new(
        point.x as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
        point.y as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
    )
}

fn footprint_world_rect(footprint: BuildingFootprint) -> (Vec2, Vec2) {
    let width = f32::from(footprint.width) * NAV_CELL_WORLD as f32;
    let height = f32::from(footprint.height) * NAV_CELL_WORLD as f32;
    let min_x = footprint.min_x as f32 * NAV_CELL_WORLD as f32;
    let min_y = footprint.min_y as f32 * NAV_CELL_WORLD as f32;
    (
        Vec2::new(min_x + width / 2.0, min_y + height / 2.0),
        Vec2::new(width, height),
    )
}

fn unit_color(unit: &UnitView) -> Color {
    if unit.mana_maximum.is_some() {
        return if unit.team.0 == 0 {
            Color::srgb(0.72, 0.40, 1.0)
        } else {
            Color::srgb(1.0, 0.38, 0.76)
        };
    }
    match (unit.team.0, unit.attack_delivery) {
        (0, AttackDelivery::Melee) => Color::srgb(0.22, 0.55, 1.0),
        (0, AttackDelivery::RangedGuaranteedHit { .. }) => Color::srgb(0.42, 0.88, 1.0),
        (0, AttackDelivery::RangedBallistic { .. }) => Color::srgb(0.94, 0.78, 0.28),
        (1, AttackDelivery::Melee) => Color::srgb(1.0, 0.30, 0.26),
        (1, AttackDelivery::RangedGuaranteedHit { .. }) => Color::srgb(1.0, 0.60, 0.32),
        (1, AttackDelivery::RangedBallistic { .. }) => Color::srgb(1.0, 0.78, 0.22),
        _ => Color::WHITE,
    }
}

fn unit_size(unit: &UnitView) -> Vec2 {
    match unit.attack_delivery {
        AttackDelivery::Melee => Vec2::splat(9.0),
        AttackDelivery::RangedGuaranteedHit { .. } => Vec2::splat(7.0),
        AttackDelivery::RangedBallistic { .. } => Vec2::splat(8.0),
        AttackDelivery::Bounce { .. } => Vec2::splat(7.0),
    }
}

fn building_color(building: &BuildingView) -> Color {
    if building.mana_maximum.is_some() {
        return if building.team.0 == 0 {
            Color::srgb(0.58, 0.25, 0.86)
        } else {
            Color::srgb(0.86, 0.24, 0.62)
        };
    }
    if let Some(delivery) = building.attack_delivery {
        return match (building.team.0, delivery) {
            (0, AttackDelivery::RangedGuaranteedHit { .. }) => Color::srgb(0.18, 0.86, 0.92),
            (0, AttackDelivery::RangedBallistic { .. }) => Color::srgb(0.88, 0.66, 0.18),
            (1, AttackDelivery::RangedGuaranteedHit { .. }) => Color::srgb(0.96, 0.48, 0.48),
            (1, AttackDelivery::RangedBallistic { .. }) => Color::srgb(0.96, 0.68, 0.18),
            _ => Color::srgb(0.6, 0.6, 0.6),
        };
    }
    match (
        building.team.0,
        building
            .production
            .map(|profile| profile.unit.attack.delivery),
    ) {
        (0, None) => Color::srgb(0.45, 0.70, 1.0),
        (1, None) => Color::srgb(1.0, 0.48, 0.42),
        (0, Some(AttackDelivery::Melee)) => Color::srgb(0.12, 0.36, 0.72),
        (0, Some(AttackDelivery::RangedGuaranteedHit { .. })) => Color::srgb(0.20, 0.62, 0.78),
        (0, Some(AttackDelivery::RangedBallistic { .. })) => Color::srgb(0.72, 0.58, 0.12),
        (1, Some(AttackDelivery::Melee)) => Color::srgb(0.72, 0.18, 0.16),
        (1, Some(AttackDelivery::RangedGuaranteedHit { .. })) => Color::srgb(0.82, 0.40, 0.16),
        (1, Some(AttackDelivery::RangedBallistic { .. })) => Color::srgb(0.82, 0.56, 0.12),
        _ => Color::srgb(0.6, 0.6, 0.6),
    }
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_map_matches_requested_dimensions() {
        assert_eq!(NAV_MAX_X + 1, 200);
        assert_eq!(NAV_MAX_Y + 1, 75);
        assert_eq!((NAV_MAX_X + 1) * NAV_CELL_WORLD, 2_000);
        assert_eq!((NAV_MAX_Y + 1) * NAV_CELL_WORLD, 750);
        assert_eq!((LANE_MAX_Y - LANE_MIN_Y + 1) * NAV_CELL_WORLD, 350);
        assert_eq!(verification_config().static_blockers.len(), 2);
    }

    #[test]
    fn mirrored_placement_preserves_size_and_y() {
        let player = BuildingFootprint::new(12, 18, 4, 4);
        let enemy = mirror_footprint(player);
        assert_eq!(enemy, BuildingFootprint::new(184, 18, 4, 4));
        assert_eq!(mirror_footprint(enemy), player);
    }

    #[test]
    fn verification_units_are_ten_hp_and_one_base_dps() {
        for kind in [
            ProductionKind::Melee,
            ProductionKind::Ranged,
            ProductionKind::Artillery,
            ProductionKind::Spellcaster,
        ] {
            let unit = unit_template(kind);
            assert_eq!(unit.health, 10);
            assert_eq!(
                i32::from(unit.attack.cooldown_ticks),
                unit.attack.damage * SIMULATION_HZ_I32,
                "{kind:?} should keep the one-DPS base-attack baseline"
            );
        }
        assert_eq!(
            unit_template(ProductionKind::Ranged).attack.delivery,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: RANGED_PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                    / SIMULATION_HZ_I32,
            }
        );
        assert_eq!(
            unit_template(ProductionKind::Artillery).attack.delivery,
            AttackDelivery::RangedBallistic {
                speed_per_tick: ARTILLERY_PROJECTILE_SPEED_WORLD_PER_SECOND
                    * SUBUNITS_PER_WORLD_UNIT
                    / SIMULATION_HZ_I32,
                impact_radius: 35 * SUBUNITS_PER_WORLD_UNIT,
            }
        );
    }

    #[test]
    fn verification_towers_cover_guaranteed_and_ballistic_delivery() {
        assert!(matches!(
            guaranteed_tower_attack().delivery,
            AttackDelivery::RangedGuaranteedHit { .. }
        ));
        assert_eq!(
            projectile_tower_attack().delivery,
            AttackDelivery::RangedBallistic {
                speed_per_tick: TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                    / SIMULATION_HZ_I32,
                impact_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
            }
        );
        assert!(projectile_tower_attack().range > guaranteed_tower_attack().range);
    }

    #[test]
    fn verification_area_spells_cast_only_when_full_with_requested_scope() {
        let global = global_area_spell_profile();
        assert_eq!(global.mana.starting, 0);
        assert_eq!(global.ability.mana_cost, global.mana.maximum);
        assert_eq!(
            global.ability.target_policy,
            AbilityTargetPolicy::RandomEnemyUnitGlobal
        );
        assert!(matches!(
            global.ability.effect,
            AbilityEffect::AreaDamage { .. }
        ));

        let local = short_range_spellcaster_profile();
        assert_eq!(local.mana.starting, 0);
        assert_eq!(local.ability.mana_cost, local.mana.maximum);
        assert_eq!(
            local.ability.target_policy,
            AbilityTargetPolicy::RandomEnemyUnit
        );
        assert!(local.ability.range > 0);
        assert!(matches!(
            local.ability.effect,
            AbilityEffect::AreaDamage { .. }
        ));
    }

    #[test]
    fn verification_buildings_produce_every_ten_seconds() {
        let building = production_building(
            Team(0),
            BuildingFootprint::new(10, 10, 4, 4),
            ProductionKind::Melee,
        );
        let production = building
            .production
            .expect("production building missing profile");
        assert_eq!(production.initial_delay_ticks, 300);
        assert_eq!(production.interval_ticks, 300);
    }

    #[test]
    fn long_running_production_congestion_remains_non_overlapping() {
        let config = verification_config();
        let minimum_distance = config.unit_separation_distance;
        let minimum_distance_sq =
            (i64::from(minimum_distance) * i64::from(minimum_distance)) as u64;
        let mut simulation = Simulation::new(config, 4);

        for row in 0..5 {
            for column in 0..10 {
                simulation.spawn_building(production_building(
                    Team(0),
                    BuildingFootprint::new(2 + column * 6, 2 + row * 6, 4, 4),
                    ProductionKind::Melee,
                ));
            }
        }

        for _ in 0..1_800 {
            simulation.step();
        }

        let units = simulation.units();
        assert!(units.len() >= 200, "stress fixture produced too few units");
        for (index, unit) in units.iter().enumerate() {
            for other in &units[index + 1..] {
                assert!(
                    unit.position.distance_sq(other.position) >= minimum_distance_sq,
                    "units {:?} and {:?} overlap after long-running production",
                    unit.id,
                    other.id
                );
            }
        }
    }

    #[test]
    fn terminal_match_does_not_advance_after_victory() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let player_castle = simulation.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(10, 0, 2, 2),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        });
        let mut state = GameState {
            simulation,
            presented_units: HashMap::new(),
            presented_buildings: HashMap::new(),
            player_castle,
            enemy_castle: SimId(u64::MAX),
            outcome: None,
        };

        assert!(state.step_match());
        assert_eq!(state.outcome, Some(MatchOutcome::PlayerVictory));
        let terminal_tick = state.simulation.tick();
        assert!(!state.step_match());
        assert_eq!(state.simulation.tick(), terminal_tick);
    }
}
