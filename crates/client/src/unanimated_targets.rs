//! Parks animation targets that no clip of their player's graph animates.
//!
//! WC3 model scenes give every node below an animation player an `AnimationTargetId`, but most
//! of them (attachment points, helpers, static geoset parents) have no curves in any of the
//! model's sequences. `animate_targets` still walks the whole graph for each of them every
//! frame and writes nothing. When a player's graph is set, its targets without curves in that
//! graph trade `AnimatedBy` for [`ParkedAnimationTarget`], which takes them out of
//! `animate_targets` with identical output. Setting another graph on the player re-examines
//! its targets and restores any that the new graph animates.
//!
//! Targets spawned below a player after its graph was set keep `AnimatedBy`.

use bevy::{
    animation::{AnimatedBy, AnimationTargetId, animate_targets, graph::AnimationNodeType},
    asset::AssetEvent,
    platform::collections::{HashMap, HashSet},
    prelude::*,
};

use crate::perf_disable::PerfDisable;

pub(crate) struct UnanimatedTargetsPlugin;

impl Plugin for UnanimatedTargetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerfDisable>().add_systems(
            PostUpdate,
            park_unanimated_targets
                .before(animate_targets)
                .run_if(|disable: Res<PerfDisable>| !disable.unanimated_targets),
        );
    }
}

/// An animation target taken out of `animate_targets` because its player's graph has no curves
/// for it; holds the player its `AnimatedBy` named.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ParkedAnimationTarget(Entity);

#[derive(Default)]
struct ParkingState {
    /// Targets with curves in some clip of each graph.
    animated: HashMap<AssetId<AnimationGraph>, HashSet<AnimationTargetId>>,
    /// Players whose graph or clips had not loaded yet.
    pending: Vec<Entity>,
}

type TargetQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static AnimationTargetId,
        Option<&'static AnimatedBy>,
        Option<&'static ParkedAnimationTarget>,
    ),
    Or<(With<AnimatedBy>, With<ParkedAnimationTarget>)>,
>;

fn park_unanimated_targets(
    mut commands: Commands,
    mut graph_events: MessageReader<AssetEvent<AnimationGraph>>,
    animation_assets: (Res<Assets<AnimationGraph>>, Res<Assets<AnimationClip>>),
    players: Query<(Entity, Ref<AnimationGraphHandle>), With<AnimationPlayer>>,
    children: Query<&Children>,
    targets: TargetQuery,
    mut state: Local<ParkingState>,
) {
    let (graphs, clips) = animation_assets;
    let ParkingState { animated, pending } = &mut *state;
    for event in graph_events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = event {
            animated.remove(id);
        }
    }
    let mut retry = Vec::new();
    let changed = players
        .iter()
        .filter_map(|(player, handle)| handle.is_changed().then_some(player));
    for player in pending.drain(..).chain(changed) {
        let Ok((_, handle)) = players.get(player) else {
            continue;
        };
        let Some(animated) = animated_targets(animated, handle.id(), &graphs, &clips) else {
            retry.push(player);
            continue;
        };
        for entity in std::iter::once(player).chain(children.iter_descendants(player)) {
            let Ok((target, animated_by, parked)) = targets.get(entity) else {
                continue;
            };
            let owned_by_player = animated_by.is_some_and(|by| by.0 == player)
                || parked.is_some_and(|parked| parked.0 == player);
            if !owned_by_player {
                continue;
            }
            let wanted = animated.contains(target);
            if wanted && parked.is_some() {
                commands
                    .entity(entity)
                    .remove::<ParkedAnimationTarget>()
                    .insert(AnimatedBy(player));
            } else if !wanted && animated_by.is_some() {
                commands
                    .entity(entity)
                    .remove::<AnimatedBy>()
                    .insert(ParkedAnimationTarget(player));
            }
        }
    }
    retry.sort_unstable();
    retry.dedup();
    *pending = retry;
}

/// The targets animated by `graph`, or `None` until the graph and all of its clips are loaded.
fn animated_targets<'a>(
    cache: &'a mut HashMap<AssetId<AnimationGraph>, HashSet<AnimationTargetId>>,
    graph: AssetId<AnimationGraph>,
    graphs: &Assets<AnimationGraph>,
    clips: &Assets<AnimationClip>,
) -> Option<&'a HashSet<AnimationTargetId>> {
    if !cache.contains_key(&graph) {
        let mut animated = HashSet::new();
        for node in graphs.get(graph)?.graph.node_weights() {
            if let AnimationNodeType::Clip(clip) = &node.node_type {
                animated.extend(clips.get(clip)?.curves().keys().copied());
            }
        }
        cache.insert(graph, animated);
    }
    cache.get(&graph)
}
