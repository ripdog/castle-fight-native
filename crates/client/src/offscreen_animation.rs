//! Stops evaluating animation poses for players whose meshes are all out of view.
//!
//! Bevy evaluates every animation target each frame, and a clip whose weight is zero
//! contributes no curve work. Zeroing the weights of an offscreen player therefore skips its
//! pose evaluation while its clock keeps advancing in `advance_animations`, so a player that
//! returns to view samples the current time without a catch-up pass. Transition-managed
//! weights are rewritten from scratch by `advance_transitions` every frame, which restores
//! them automatically once the player is onscreen again.
//!
//! Only players with at least one mesh descendant are candidates. Players without meshes can
//! drive attachments and particles, so they always keep evaluating.
//!
//! Visibility is recorded after `CheckVisibility` and applied to the next frame's animation
//! evaluation, so a player that enters view shows its last written pose for one frame.

use bevy::{
    animation::{
        AnimationPlayer, advance_animations, animate_targets, transition::AnimationTransitions,
    },
    camera::visibility::{ViewVisibility, VisibilitySystems},
    prelude::*,
};

use crate::wc3_effects::Wc3VisualAnimationController;

const HAS_MESH: u8 = 1;
const VISIBLE_MESH: u8 = 2;

pub(crate) struct OffscreenAnimationPlugin;

impl Plugin for OffscreenAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationPlayerMeshStates>()
            .add_systems(
                PostUpdate,
                record_animation_player_mesh_states.after(VisibilitySystems::CheckVisibility),
            )
            .add_systems(
                PostUpdate,
                pause_offscreen_animation_weights
                    .after(advance_animations)
                    .before(animate_targets),
            );
    }
}

/// Per-player mesh coverage from the last completed visibility pass, indexed by entity index.
#[derive(Resource, Default)]
struct AnimationPlayerMeshStates {
    recorded: bool,
    states: Vec<u8>,
}

fn record_animation_player_mesh_states(
    mut recorded: ResMut<AnimationPlayerMeshStates>,
    meshes: Query<(Entity, &ViewVisibility), With<Mesh3d>>,
    parents: Query<&ChildOf>,
    players: Query<(), With<AnimationPlayer>>,
) {
    let recorded = &mut *recorded;
    recorded.states.fill(0);
    for (mesh, view) in &meshes {
        let flags = if view.get() {
            HAS_MESH | VISIBLE_MESH
        } else {
            HAS_MESH
        };
        let mut current = mesh;
        loop {
            if players.contains(current) {
                let index = current.index_u32() as usize;
                if index >= recorded.states.len() {
                    recorded.states.resize(index + 1, 0);
                }
                recorded.states[index] |= flags;
                break;
            }
            let Ok(parent) = parents.get(current) else {
                break;
            };
            current = parent.parent();
        }
    }
    recorded.recorded = true;
}

type TransitionPlayers<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut AnimationPlayer),
    (
        With<AnimationTransitions>,
        Without<Wc3VisualAnimationController>,
    ),
>;

fn pause_offscreen_animation_weights(
    recorded: Res<AnimationPlayerMeshStates>,
    mut players: TransitionPlayers,
) {
    if !recorded.recorded {
        return;
    }
    for (entity, mut player) in &mut players {
        let state = recorded
            .states
            .get(entity.index_u32() as usize)
            .copied()
            .unwrap_or(0);
        let offscreen = state & HAS_MESH != 0 && state & VISIBLE_MESH == 0;
        if !offscreen || player.all_paused() {
            continue;
        }
        for (_, animation) in player.playing_animations_mut() {
            animation.set_weight(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::animation::graph::AnimationNodeIndex;

    use super::*;

    fn pause_app() -> App {
        let mut app = App::new();
        app.init_resource::<AnimationPlayerMeshStates>()
            .add_systems(
                Update,
                (
                    record_animation_player_mesh_states,
                    pause_offscreen_animation_weights,
                )
                    .chain(),
            );
        app
    }

    fn spawn_player_with_mesh(app: &mut App, visibility: Option<ViewVisibility>) -> Entity {
        let player = app
            .world_mut()
            .spawn((AnimationPlayer::default(), AnimationTransitions::new()))
            .id();
        if let Some(visibility) = visibility {
            app.world_mut()
                .spawn((Mesh3d::default(), visibility, ChildOf(player)));
        }
        app.world_mut()
            .get_mut::<AnimationPlayer>(player)
            .expect("player")
            .play(AnimationNodeIndex::new(1));
        player
    }

    fn weight(app: &App, player: Entity) -> f32 {
        app.world()
            .get::<AnimationPlayer>(player)
            .and_then(|player| player.animation(AnimationNodeIndex::new(1)))
            .map(|animation| animation.weight())
            .expect("playing animation")
    }

    #[test]
    fn players_with_only_offscreen_meshes_stop_evaluating() {
        let mut app = pause_app();
        let player = spawn_player_with_mesh(&mut app, Some(ViewVisibility::HIDDEN));
        app.update();
        assert_eq!(weight(&app, player), 0.0);
    }

    #[test]
    fn players_with_a_visible_mesh_keep_their_weights() {
        let mut app = pause_app();
        let player = spawn_player_with_mesh(&mut app, Some(ViewVisibility::VISIBLE));
        app.update();
        assert_eq!(weight(&app, player), 1.0);
    }

    #[test]
    fn players_without_meshes_keep_evaluating() {
        let mut app = pause_app();
        let player = spawn_player_with_mesh(&mut app, None);
        app.update();
        assert_eq!(weight(&app, player), 1.0);
    }
}
