//! "Your base is under attack" alerts and the jump-to-last-alert key.
//!
//! The alert is derived from the presentation snapshots (a building of the local team losing
//! health between two ticks), so it is presentation-only and identical for every viewer of the
//! same match.
use bevy::prelude::*;
use castle_fight_sim::SimId;

use crate::{
    AuthoritativeSimulation, SelectedMatch,
    bridge::PresentationSamples,
    game_menu::MatchSessionReset,
    notifications::{Notifications, Severity},
    presentation::CameraFocusRequest,
};

/// Real seconds between alerts, so a long siege does not spam the screen.
const ALERT_COOLDOWN_SECONDS: f32 = 10.0;

pub(crate) struct AlertsPlugin;

impl Plugin for AlertsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AlertState>()
            .add_message::<MatchSessionReset>()
            .add_systems(
                Update,
                (reset_alerts, detect_attacks_on_base, jump_to_last_alert).chain(),
            );
    }
}

#[derive(Resource, Default)]
struct AlertState {
    /// The building that was hit most recently, where the jump key takes the camera.
    last_target: Option<SimId>,
    cooldown: f32,
}

fn reset_alerts(mut resets: MessageReader<MatchSessionReset>, mut state: ResMut<AlertState>) {
    if resets.read().count() > 0 {
        *state = AlertState::default();
    }
}

fn detect_attacks_on_base(
    time: Res<Time>,
    samples: Res<PresentationSamples>,
    selected: Res<SelectedMatch>,
    authoritative: Res<AuthoritativeSimulation>,
    mut notifications: ResMut<Notifications>,
    mut state: ResMut<AlertState>,
) {
    state.cooldown = (state.cooldown - time.delta_secs()).max(0.0);
    if !samples.is_changed() || !samples.tick_advanced() {
        return;
    }
    let sim = &authoritative.simulation;
    let Some(team) = sim.player(selected.local_player).map(|player| player.team) else {
        return;
    };
    let hit = samples
        .current
        .buildings
        .values()
        .filter(|building| building.team == team && !building.remembered)
        .find(|building| {
            building.health > 0
                && samples
                    .previous
                    .buildings
                    .get(&building.id)
                    .is_some_and(|before| building.health < before.health)
        });
    let Some(building) = hit else {
        return;
    };
    state.last_target = Some(building.id);
    if state.cooldown > 0.0 {
        return;
    }
    state.cooldown = ALERT_COOLDOWN_SECONDS;
    let text = if sim.team_objective(team) == Some(building.id) {
        "Your castle is under attack! (. to look)"
    } else {
        "Your base is under attack! (. to look)"
    };
    notifications.push(Severity::Warning, text);
}

/// `.` moves the camera to the last building that was attacked.
fn jump_to_last_alert(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<AlertState>,
    mut camera_focus: ResMut<CameraFocusRequest>,
) {
    if keys.just_pressed(KeyCode::Period)
        && let Some(target) = state.last_target
    {
        camera_focus.0 = Some(target);
    }
}
