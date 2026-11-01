//! Evaluates animation targets in place of Bevy's `animate_targets`, with identical output.
//!
//! Bevy walks every node of a target's graph for each target each frame, looking each clip up
//! in the player's active animations and blending every child of every blend node. Unit graphs
//! hold a dozen clips of which one or two play, and pose throttling and offscreen pausing zero
//! the weights of most players on most frames, so nearly all of that walk produces nothing.
//! This evaluator resolves once per player which nodes contribute this frame (clips that are
//! active with a nonzero weight, and the blend nodes above them) and evaluates each target
//! over only those nodes, in Bevy's order and through the same curve evaluators. The nodes it
//! leaves out change nothing in Bevy either: an inactive clip pushes no value, blending a
//! child that pushed none leaves the stack alone, and pushing an empty blend register does
//! nothing.
//!
//! Clip events targeted at animation targets are not triggered; the glTF clips this client
//! plays carry none. Player-level events stay with Bevy's `trigger_untargeted_animation_events`.

use std::{any::TypeId, ops::Range};

use bevy::{
    animation::{
        AnimatedBy, AnimationEntityMut, AnimationPlayer, AnimationTargetId, VariableCurve,
        advance_animations, animate_targets,
        animation_curves::{AnimationCurveEvaluator, EvaluatorId},
        graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex, AnimationNodeType},
    },
    app::AnimationSystems,
    asset::AssetEvent,
    ecs::{resource::IsResource, schedule::ScheduleCleanupPolicy},
    platform::collections::HashMap,
    prelude::*,
    transform::TransformSystems,
    utils::Parallel,
};

use crate::perf_disable::PerfDisable;

pub(crate) struct AnimationEvaluationPlugin;

impl Plugin for AnimationEvaluationPlugin {
    fn build(&self, app: &mut App) {
        if disabled(app) {
            return;
        }
        app.add_systems(
            PostUpdate,
            evaluate_animation_targets
                .after(advance_animations)
                .after(animate_targets)
                .in_set(AnimationSystems)
                .before(TransformSystems::Propagate)
                // Like Bevy's evaluator, this can write any animated component.
                .ambiguous_with_all(),
        );
    }

    fn finish(&self, app: &mut App) {
        if disabled(app) {
            return;
        }
        // The `animate_targets` set stays, so systems ordered around it keep their order.
        let removed = app
            .remove_systems_in_set(
                PostUpdate,
                animate_targets,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .expect("Bevy's AnimationPlugin adds animate_targets to PostUpdate");
        assert_eq!(removed, 1, "exactly one animate_targets system to replace");
    }
}

fn disabled(app: &App) -> bool {
    app.world()
        .get_resource::<PerfDisable>()
        .is_some_and(|disable| disable.animation_evaluation)
}

/// A graph in Bevy's evaluation order: nodes in postorder from the root with siblings
/// descending, each node's children ascending, and masks accumulated from the root.
struct ThreadedGraph {
    order: Vec<AnimationNodeIndex>,
    children: Vec<Range<u32>>,
    edges: Vec<AnimationNodeIndex>,
    masks: Vec<u64>,
}

impl ThreadedGraph {
    fn new(graph: &AnimationGraph) -> Self {
        let nodes = graph.graph.node_count();
        let mut threaded = Self {
            order: Vec::with_capacity(nodes),
            children: vec![0..0; nodes],
            edges: Vec::with_capacity(graph.graph.edge_count()),
            masks: vec![0; nodes],
        };
        threaded.build_from(graph, graph.root, 0);
        threaded
    }

    /// Mirrors Bevy's `ThreadedAnimationGraph::build_from`.
    fn build_from(&mut self, graph: &AnimationGraph, node: AnimationNodeIndex, mut mask: u64) {
        mask |= graph.graph[node].mask;
        self.masks[node.index()] = mask;
        let mut kids: Vec<_> = graph.graph.neighbors(node).collect();
        kids.sort_unstable();
        let start = self.edges.len() as u32;
        self.edges.extend_from_slice(&kids);
        self.children[node.index()] = start..self.edges.len() as u32;
        for &kid in kids.iter().rev() {
            self.build_from(graph, kid, mask);
        }
        self.order.push(node);
    }
}

/// One contributing graph node of a player's frame.
#[derive(Clone, Copy)]
enum Step {
    Clip {
        node: AnimationNodeIndex,
        clip: AssetId<AnimationClip>,
        /// Active animation weight times node weight.
        weight: f32,
        seek_time: f32,
        mask: u64,
    },
    Blend {
        node: AnimationNodeIndex,
        weight: f32,
        additive: bool,
        /// The node's contributing children, ascending, relative to the plan's first child.
        children: (u32, u32),
    },
}

/// Where one player's contributing nodes for this frame sit in the step and child buffers.
struct PlayerPlan {
    player: Entity,
    frame: u32,
    graph: AssetId<AnimationGraph>,
    masked: bool,
    steps: Range<u32>,
    first_child: u32,
}

/// Working space for planning players, one per thread; its buffers are moved into
/// [`EvaluationState`] once all players are planned.
#[derive(Default)]
struct PlanScratch {
    /// Active weight and seek time of each clip node that plays with a nonzero weight.
    active: Vec<Option<(f32, f32)>>,
    /// Whether each node contributes.
    live: Vec<bool>,
    steps: Vec<Step>,
    children: Vec<AnimationNodeIndex>,
    plans: Vec<(u32, PlayerPlan)>,
    missing_graph: Vec<Entity>,
}

#[derive(Default)]
struct EvaluationState {
    graphs: HashMap<AssetId<AnimationGraph>, ThreadedGraph>,
    scratch: Parallel<PlanScratch>,
    /// Scratch for players whose graph is threaded on the main thread.
    serial: PlanScratch,
    /// Plans indexed by player entity index; entries from earlier frames are stale.
    plans: Vec<Option<PlayerPlan>>,
    steps: Vec<Step>,
    children: Vec<AnimationNodeIndex>,
    frame: u32,
}

impl EvaluationState {
    fn plan(&self, player: Entity) -> Option<&PlayerPlan> {
        self.plans
            .get(player.index_u32() as usize)?
            .as_ref()
            .filter(|plan| plan.frame == self.frame && plan.player == player)
    }
}

/// Moves a scratch's plans into the shared buffers.
fn merge_plans(
    scratch: &mut PlanScratch,
    plans: &mut Vec<Option<PlayerPlan>>,
    steps: &mut Vec<Step>,
    children: &mut Vec<AnimationNodeIndex>,
) {
    let step_base = steps.len() as u32;
    let child_base = children.len() as u32;
    steps.append(&mut scratch.steps);
    children.append(&mut scratch.children);
    for (index, mut plan) in scratch.plans.drain(..) {
        plan.steps = plan.steps.start + step_base..plan.steps.end + step_base;
        plan.first_child += child_base;
        let index = index as usize;
        if index >= plans.len() {
            plans.resize_with(index + 1, || None);
        }
        plans[index] = Some(plan);
    }
}

/// Appends the nodes of `graph` that contribute to `player` this frame to the scratch, and
/// records the player's plan there if any do.
fn plan_player(
    player: Entity,
    animation: &AnimationPlayer,
    graph_id: AssetId<AnimationGraph>,
    graph: &AnimationGraph,
    threaded: &ThreadedGraph,
    frame: u32,
    scratch: &mut PlanScratch,
) {
    let nodes = threaded.masks.len();
    scratch.active.clear();
    scratch.active.resize(nodes, None);
    let mut any = false;
    for (node, active) in animation.playing_animations() {
        if active.weight() == 0.0 {
            continue;
        }
        if let Some(slot) = scratch.active.get_mut(node.index()) {
            *slot = Some((active.weight(), active.seek_time()));
            any = true;
        }
    }
    if !any {
        return;
    }
    scratch.live.clear();
    scratch.live.resize(nodes, false);
    let first_step = scratch.steps.len() as u32;
    let first_child = scratch.children.len() as u32;
    for &node in &threaded.order {
        let Some(graph_node) = graph.get(node) else {
            continue;
        };
        match &graph_node.node_type {
            AnimationNodeType::Clip(clip) => {
                let Some((weight, seek_time)) = scratch.active[node.index()] else {
                    continue;
                };
                scratch.live[node.index()] = true;
                scratch.steps.push(Step::Clip {
                    node,
                    clip: clip.id(),
                    weight: weight * graph_node.weight,
                    seek_time,
                    mask: threaded.masks[node.index()],
                });
            }
            node_type @ (AnimationNodeType::Blend | AnimationNodeType::Add) => {
                let start = scratch.children.len();
                let Range { start: kids, end } = threaded.children[node.index()];
                let live = &scratch.live;
                scratch.children.extend(
                    threaded.edges[kids as usize..end as usize]
                        .iter()
                        .copied()
                        .filter(|kid| live[kid.index()]),
                );
                if scratch.children.len() == start {
                    continue;
                }
                scratch.live[node.index()] = true;
                scratch.steps.push(Step::Blend {
                    node,
                    weight: graph_node.weight,
                    additive: matches!(node_type, AnimationNodeType::Add),
                    children: (
                        start as u32 - first_child,
                        scratch.children.len() as u32 - first_child,
                    ),
                });
            }
        }
    }
    scratch.plans.push((
        player.index_u32(),
        PlayerPlan {
            player,
            frame,
            graph: graph_id,
            masked: !graph.mask_groups.is_empty(),
            steps: first_step..scratch.steps.len() as u32,
            first_child,
        },
    ));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvaluatorKey {
    Field(TypeId, usize),
    Type(TypeId),
}

impl From<EvaluatorId<'_>> for EvaluatorKey {
    fn from(id: EvaluatorId<'_>) -> Self {
        match id {
            EvaluatorId::ComponentField(field) => {
                let &(component, index) = &**field;
                Self::Field(component, index)
            }
            EvaluatorId::Type(type_id) => Self::Type(type_id),
        }
    }
}

/// Per-thread curve evaluators, one per animated property, reused across targets and frames.
#[derive(Default)]
struct CurveEvaluators {
    evaluators: Vec<(EvaluatorKey, Box<dyn AnimationCurveEvaluator>)>,
    /// Indices of the evaluators the current target has used.
    current: Vec<usize>,
}

impl CurveEvaluators {
    fn apply(
        &mut self,
        curve: &VariableCurve,
        seek_time: f32,
        weight: f32,
        node: AnimationNodeIndex,
    ) {
        let key = EvaluatorKey::from(curve.0.evaluator_id());
        let index = match self
            .evaluators
            .iter()
            .position(|(existing, _)| *existing == key)
        {
            Some(index) => index,
            None => {
                self.evaluators.push((key, curve.0.create_evaluator()));
                self.evaluators.len() - 1
            }
        };
        if !self.current.contains(&index) {
            self.current.push(index);
        }
        if let Err(err) = curve
            .0
            .apply(&mut *self.evaluators[index].1, seek_time, weight, node)
        {
            warn!("Animation application failed: {err:?}");
        }
    }

    fn combine(&mut self, child: AnimationNodeIndex, additive: bool) {
        for &index in &self.current {
            let evaluator = &mut self.evaluators[index].1;
            let result = if additive {
                evaluator.add(child)
            } else {
                evaluator.blend(child)
            };
            if let Err(err) = result {
                warn!("Failed to blend animation: {err:?}");
            }
        }
    }

    fn push_blend_register(&mut self, weight: f32, node: AnimationNodeIndex) {
        for &index in &self.current {
            if let Err(err) = self.evaluators[index].1.push_blend_register(weight, node) {
                warn!("Animation blending failed: {err:?}");
            }
        }
    }

    fn commit(&mut self, mut entity: AnimationEntityMut) {
        for index in self.current.drain(..) {
            if let Err(err) = self.evaluators[index].1.commit(entity.reborrow()) {
                warn!("Animation application failed: {err:?}");
            }
        }
    }
}

type AnimationTargets<'w, 's> = Query<
    'w,
    's,
    (
        &'static AnimationTargetId,
        &'static AnimatedBy,
        AnimationEntityMut<'static, 'static>,
    ),
    Without<IsResource>,
>;

fn evaluate_animation_targets(
    mut graph_events: MessageReader<AssetEvent<AnimationGraph>>,
    animation_assets: (Res<Assets<AnimationGraph>>, Res<Assets<AnimationClip>>),
    players: Query<(Entity, &AnimationPlayer, &AnimationGraphHandle)>,
    mut targets: AnimationTargets,
    mut state: Local<EvaluationState>,
    evaluators: Local<Parallel<CurveEvaluators>>,
) {
    let (graphs, clips) = animation_assets;
    let state = &mut *state;
    for event in graph_events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = event {
            state.graphs.remove(id);
        }
    }
    state.frame = state.frame.wrapping_add(1);
    let EvaluationState {
        graphs: threaded_graphs,
        scratch,
        serial,
        plans,
        steps,
        children,
        frame,
    } = &mut *state;
    let frame = *frame;

    let cached = &*threaded_graphs;
    let shared = &*scratch;
    players.par_iter().for_each(|(player, animation, handle)| {
        let Some(graph) = graphs.get(handle) else {
            return;
        };
        let mut scratch = shared.borrow_local_mut();
        match cached.get(&handle.id()) {
            Some(threaded) => {
                plan_player(
                    player,
                    animation,
                    handle.id(),
                    graph,
                    threaded,
                    frame,
                    &mut scratch,
                );
            }
            None => scratch.missing_graph.push(player),
        }
    });
    steps.clear();
    children.clear();
    let mut missing = Vec::new();
    for scratch in scratch.iter_mut() {
        merge_plans(scratch, plans, steps, children);
        missing.append(&mut scratch.missing_graph);
    }
    // Players whose graph has not been threaded yet; rare, so they are planned here.
    for player in missing {
        let Ok((player, animation, handle)) = players.get(player) else {
            continue;
        };
        let Some(graph) = graphs.get(handle) else {
            continue;
        };
        let threaded = threaded_graphs
            .entry(handle.id())
            .or_insert_with(|| ThreadedGraph::new(graph));
        plan_player(
            player,
            animation,
            handle.id(),
            graph,
            threaded,
            frame,
            serial,
        );
    }
    merge_plans(serial, plans, steps, children);

    let state = &*state;
    targets
        .par_iter_mut()
        .for_each(|(&target, &AnimatedBy(player), entity)| {
            let Some(plan) = state.plan(player) else {
                return;
            };
            let target_mask = if plan.masked {
                graphs
                    .get(plan.graph)
                    .and_then(|graph| graph.mask_groups.get(&target).copied())
                    .unwrap_or_default()
            } else {
                0
            };
            let mut evaluators = evaluators.borrow_local_mut();
            let plan_children = &state.children[plan.first_child as usize..];
            for step in &state.steps[plan.steps.start as usize..plan.steps.end as usize] {
                match *step {
                    Step::Clip {
                        node,
                        clip,
                        weight,
                        seek_time,
                        mask,
                    } => {
                        if target_mask & mask != 0 {
                            continue;
                        }
                        let Some(curves) = clips
                            .get(clip)
                            .and_then(|clip| clip.curves_for_target(target))
                        else {
                            continue;
                        };
                        for curve in curves {
                            evaluators.apply(curve, seek_time, weight, node);
                        }
                    }
                    Step::Blend {
                        node,
                        weight,
                        additive,
                        children: (start, end),
                    } => {
                        for &child in &plan_children[start as usize..end as usize] {
                            evaluators.combine(child, additive);
                        }
                        evaluators.push_blend_register(weight, node);
                    }
                }
            }
            evaluators.commit(entity);
        });
}

#[cfg(test)]
mod tests {
    use bevy::{
        animation::{
            AnimationPlugin, animated_field,
            animation_curves::{AnimatableCurve, AnimatedField},
        },
        math::curve::UnevenSampleAutoCurve,
        time::TimePlugin,
    };

    use super::*;

    fn translation_clip(targets: &[AnimationTargetId], scale: f32) -> AnimationClip {
        let mut clip = AnimationClip::default();
        for (index, &target) in targets.iter().enumerate() {
            let offset = scale * (index as f32 + 1.0);
            let curve = UnevenSampleAutoCurve::new([
                (0.0, Vec3::new(offset, -offset, 0.5)),
                (1.0, Vec3::new(-offset, 2.0 * offset, 1.5)),
            ])
            .expect("curve samples");
            clip.add_curve_to_target(
                target,
                AnimatableCurve::new(animated_field!(Transform::translation), curve),
            );
        }
        clip
    }

    /// Poses a graph of clips, nested blends, an additive blend, a masked clip and a
    /// zero-weight clip, with Bevy's evaluator or this one.
    fn evaluated_translations(replace_bevy: bool) -> Vec<Vec3> {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            AssetPlugin::default(),
            AnimationPlugin,
        ));
        if replace_bevy {
            app.add_plugins(AnimationEvaluationPlugin);
        }
        app.finish();
        app.cleanup();

        let targets: Vec<AnimationTargetId> = (0..4)
            .map(|index| AnimationTargetId::from_name(&Name::new(format!("bone{index}"))))
            .collect();
        let mut clips = app.world_mut().resource_mut::<Assets<AnimationClip>>();
        let clip_handles: Vec<Handle<AnimationClip>> = (0..5)
            .map(|index| clips.add(translation_clip(&targets[index % 2..], index as f32 + 0.5)))
            .collect();

        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let walk = graph.add_clip(clip_handles[0].clone(), 1.0, root);
        let stand = graph.add_clip(clip_handles[1].clone(), 0.8, root);
        let blend = graph.add_blend(0.6, root);
        let nested = graph.add_clip(clip_handles[2].clone(), 1.0, blend);
        let silent = graph.add_clip(clip_handles[3].clone(), 1.0, blend);
        let additive = graph.add_additive_blend(0.5, root);
        let masked = graph.add_clip_with_mask(clip_handles[4].clone(), 1, 1.0, additive);
        graph.add_target_to_mask_group(targets[3], 0);
        let graph = app
            .world_mut()
            .resource_mut::<Assets<AnimationGraph>>()
            .add(graph);

        let mut player = AnimationPlayer::default();
        for (node, weight, seek) in [
            (walk, 0.7, 0.25),
            (stand, 0.3, 0.6),
            (nested, 0.9, 0.1),
            (silent, 0.0, 0.9),
            (masked, 1.0, 0.4),
        ] {
            player
                .play(node)
                .set_weight(weight)
                .set_speed(0.0)
                .seek_to(seek);
        }
        let player = app
            .world_mut()
            .spawn((player, AnimationGraphHandle(graph)))
            .id();
        let target_entities: Vec<Entity> = targets
            .iter()
            .map(|&target| {
                app.world_mut()
                    .spawn((target, AnimatedBy(player), Transform::default()))
                    .id()
            })
            .collect();

        app.update();
        app.update();
        target_entities
            .iter()
            .map(|&entity| {
                app.world()
                    .get::<Transform>(entity)
                    .expect("target")
                    .translation
            })
            .collect()
    }

    #[test]
    fn poses_match_bevys_evaluator() {
        let bevy = evaluated_translations(false);
        let replaced = evaluated_translations(true);
        assert_ne!(bevy[0], Vec3::ZERO, "the graph should animate the targets");
        assert_eq!(
            bevy.iter()
                .map(|value| value.to_array().map(f32::to_bits))
                .collect::<Vec<_>>(),
            replaced
                .iter()
                .map(|value| value.to_array().map(f32::to_bits))
                .collect::<Vec<_>>()
        );
    }
}
