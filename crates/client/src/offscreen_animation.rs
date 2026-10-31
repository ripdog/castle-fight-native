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
//! drive attachments and particles, so they always keep evaluating. Meshes hidden below the
//! player (geosets whose animated visibility is off) are ignored: the player's own animation
//! is what unhides them, so counting them as offscreen would freeze the model hidden.
//!
//! Visibility is recorded after `CheckVisibility` and applied to the next frame's animation
//! evaluation, so a player that enters view shows its last written pose for one frame.

use bevy::{
    animation::{
        AnimationPlayer, advance_animations, animate_targets, transition::AnimationTransitions,
    },
    camera::visibility::{ViewVisibility, VisibilitySystems},
    prelude::*,
    utils::Parallel,
};

use crate::{
    perf_disable::PerfDisable, render_audit::RenderExperiment,
    wc3_effects::Wc3VisualAnimationController,
};

const HAS_MESH: u8 = 1;
const VISIBLE_MESH: u8 = 2;

pub(crate) struct OffscreenAnimationPlugin;

impl Plugin for OffscreenAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerfDisable>()
            .init_resource::<AnimationPlayerMeshStates>()
            .add_systems(
                PostUpdate,
                record_animation_player_mesh_states.after(VisibilitySystems::CheckVisibility),
            )
            .add_systems(
                PostUpdate,
                pause_offscreen_animation_weights
                    .after(advance_animations)
                    .before(animate_targets)
                    .run_if(|disable: Res<PerfDisable>| !disable.offscreen_animation),
            );
    }
}

/// Per-player mesh coverage from the last completed visibility pass, indexed by entity index.
#[derive(Resource, Default)]
struct AnimationPlayerMeshStates {
    recorded: bool,
    states: Vec<u8>,
    /// Player entity indices and mesh flags found by each thread of the parallel walk.
    found: Parallel<Vec<(usize, u8)>>,
}

/// Each entity's parent, whether it is an animation player and its visibility, fetched together
/// so the walk from a mesh to its player does one lookup per ancestor.
type PlayerSearchTree<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static ChildOf>,
        Has<AnimationPlayer>,
        Option<&'static Visibility>,
    ),
>;

fn record_animation_player_mesh_states(
    mut recorded: ResMut<AnimationPlayerMeshStates>,
    meshes: Query<(Entity, &ViewVisibility), With<Mesh3d>>,
    tree: PlayerSearchTree,
) {
    let AnimationPlayerMeshStates {
        recorded,
        states,
        found,
    } = &mut *recorded;
    // Walks are independent per mesh and the flags merge by OR, so order does not matter.
    meshes.par_iter().for_each(|(mesh, view)| {
        let flags = if view.get() {
            HAS_MESH | VISIBLE_MESH
        } else {
            HAS_MESH
        };
        let mut current = mesh;
        while let Ok((parent, is_player, visibility)) = tree.get(current) {
            if is_player {
                found
                    .borrow_local_mut()
                    .push((current.index_u32() as usize, flags));
                break;
            }
            if visibility == Some(&Visibility::Hidden) {
                break;
            }
            let Some(parent) = parent else {
                break;
            };
            current = parent.parent();
        }
    });
    states.fill(0);
    for (index, flags) in found.iter_mut().flat_map(|found| found.drain(..)) {
        if index >= states.len() {
            states.resize(index + 1, 0);
        }
        states[index] |= flags;
    }
    *recorded = true;
}

type TransitionPlayers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut AnimationPlayer,
        Has<Wc3VisualAnimationController>,
    ),
    With<AnimationTransitions>,
>;

fn pause_offscreen_animation_weights(
    recorded: Res<AnimationPlayerMeshStates>,
    experiment: Option<Res<RenderExperiment>>,
    mut players: TransitionPlayers,
) {
    if !recorded.recorded {
        return;
    }
    // Effect players also drive emitters and attachments, so freezing their offscreen poses is
    // not exact; it stays behind an experiment until it is proven visually negligible.
    let include_effects = experiment
        .is_some_and(|experiment| *experiment == RenderExperiment::OffscreenEffectAnimation);
    for (entity, mut player, is_effect) in &mut players {
        if is_effect && !include_effects {
            continue;
        }
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
    fn players_whose_meshes_are_all_geoset_hidden_keep_evaluating() {
        let mut app = pause_app();
        let player = spawn_player_with_mesh(&mut app, None);
        let geoset = app
            .world_mut()
            .spawn((Visibility::Hidden, ChildOf(player)))
            .id();
        app.world_mut()
            .spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(geoset)));
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

    #[test]
    fn effect_players_pause_only_under_the_offscreen_effect_experiment() {
        for (experiment, expected) in [
            (RenderExperiment::Baseline, 1.0),
            (RenderExperiment::OffscreenEffectAnimation, 0.0),
        ] {
            let mut app = pause_app();
            app.insert_resource(experiment);
            let player = spawn_player_with_mesh(&mut app, Some(ViewVisibility::HIDDEN));
            app.world_mut()
                .entity_mut(player)
                .insert(Wc3VisualAnimationController);
            app.update();
            assert_eq!(weight(&app, player), expected);
        }
    }
}
