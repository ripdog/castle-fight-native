//! Spreads WC3 model scene instantiation over frames.
//!
//! Bevy clones a scene template into the world synchronously inside one system, at
//! roughly 150 us per instance plus a few microseconds per template entity, and does
//! it for every root requested that frame. Combat opens with hundreds of aura, spell
//! and unit roots in a single frame, which shows up as 30-40 ms spikes. This gate holds
//! opted-in roots back (by removing `WorldAssetRoot` before the engine queues them) and
//! re-inserts them under a per-frame template-entity budget, nearest to the camera
//! first. A held root simply looks like a scene whose asset has not loaded yet, which
//! every consumer of `WorldInstance` already tolerates.
//!
//! Non-exact (roots appear a few frames late under bursts), so it is only active under
//! `--render-experiment budgeted-scene-spawn`.

use bevy::platform::collections::HashMap;
use bevy::{
    app::{SceneSpawnerSystems, SpawnScene},
    ecs::resource::IS_RESOURCE,
    prelude::*,
};

use crate::render_audit::RenderExperiment;

/// Template entities instantiated per frame before further roots wait (about 4 ms).
const SCENE_SPAWN_ENTITY_BUDGET: u32 = 600;
/// A root held this many frames outranks the camera-distance ordering.
const PROMOTE_AFTER_FRAMES: u32 = 30;

/// How urgently a held scene root is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SceneRootPriority {
    /// Visible gameplay content.
    Gameplay,
    /// Speculative warm-up instances that only fill a reuse pool.
    Warmup,
}

/// Opts a `WorldAssetRoot` entity into frame budgeting.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct BudgetedSceneRoot(pub(crate) SceneRootPriority);

/// Present once a held root was released; excluded from interception.
#[derive(Component)]
struct ReleasedSceneRoot;

#[derive(Debug)]
struct HeldRoot {
    entity: Entity,
    scene: Handle<WorldAsset>,
    priority: SceneRootPriority,
    waited: u32,
}

/// Cumulative gate accounting, for the profile report.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SceneBudgetStats {
    /// Roots that were not instantiated in the frame they were requested.
    pub(crate) delayed_roots: usize,
    /// Roots instantiated under the gate.
    pub(crate) released_roots: usize,
    pub(crate) peak_held: usize,
    pub(crate) max_wait_frames: u32,
    pub(crate) total_wait_frames: u64,
}

#[derive(Resource, Default)]
pub(crate) struct SceneRootBudget {
    held: Vec<HeldRoot>,
    stats: SceneBudgetStats,
}

impl SceneRootBudget {
    pub(crate) fn stats(&self) -> SceneBudgetStats {
        self.stats
    }
}

pub(crate) struct SceneBudgetPlugin;

impl Plugin for SceneBudgetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneRootBudget>()
            .add_systems(
                SpawnScene,
                budget_scene_roots
                    .run_if(budgeting_enabled)
                    .before(SceneSpawnerSystems::WorldInstanceSpawn),
            )
            .add_systems(Last, publish_scene_budget_stats.run_if(budgeting_enabled));
    }
}

fn budgeting_enabled(experiment: Option<Res<RenderExperiment>>) -> bool {
    experiment.is_some_and(|experiment| *experiment == RenderExperiment::BudgetedSceneSpawn)
}

fn publish_scene_budget_stats(
    budget: Res<SceneRootBudget>,
    audit: Option<Res<crate::render_audit::RenderAudit>>,
) {
    if let Some(audit) = audit {
        audit.set_scene_budget_stats(budget.stats());
    }
}

/// Entities in a scene template, cached per asset (the archetype scan is slow).
/// Returns `None` while the asset is not loaded.
pub(crate) fn template_entity_count(
    cache: &mut HashMap<AssetId<WorldAsset>, u32>,
    assets: &Assets<WorldAsset>,
    asset: AssetId<WorldAsset>,
) -> Option<u32> {
    if let Some(&count) = cache.get(&asset) {
        return Some(count);
    }
    let world_asset = assets.get(asset)?;
    let count = world_asset
        .world
        .archetypes()
        .iter()
        .filter(|archetype| !archetype.contains(IS_RESOURCE))
        .map(|archetype| archetype.entities().len() as u32)
        .sum();
    cache.insert(asset, count);
    Some(count)
}

/// One root competing for this frame's instantiation budget.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// Template size; `None` while the asset is still loading.
    cost: Option<u32>,
    priority: SceneRootPriority,
    waited: u32,
    distance_squared: f32,
}

/// Orders candidates for admission: starved roots, then gameplay before warm-up,
/// then nearest to the camera. The sort is stable, so ties keep request order.
fn admission_order(candidates: &[Candidate]) -> Vec<usize> {
    let mut order = (0..candidates.len()).collect::<Vec<_>>();
    order.sort_by(|&left, &right| {
        let (left, right) = (&candidates[left], &candidates[right]);
        // `false` (starved) sorts first.
        let recent = |candidate: &Candidate| candidate.waited < PROMOTE_AFTER_FRAMES;
        recent(left)
            .cmp(&recent(right))
            .then(left.priority.cmp(&right.priority))
            .then(left.distance_squared.total_cmp(&right.distance_squared))
    });
    order
}

/// Chooses which candidates are instantiated this frame. Roots whose asset has not
/// loaded stay held. The first loaded root is always admitted so a large template
/// cannot starve behind the budget.
fn select_admissions(candidates: &[Candidate], budget: u32) -> Vec<bool> {
    let mut admitted = vec![false; candidates.len()];
    let mut spent = 0u32;
    for index in admission_order(candidates) {
        let Some(cost) = candidates[index].cost else {
            continue;
        };
        if spent == 0 || spent.saturating_add(cost) <= budget {
            admitted[index] = true;
            spent = spent.saturating_add(cost);
        }
    }
    admitted
}

type NewSceneRoots<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static WorldAssetRoot, &'static BudgetedSceneRoot),
    (Added<WorldAssetRoot>, Without<ReleasedSceneRoot>),
>;

#[allow(clippy::too_many_arguments)]
fn budget_scene_roots(
    mut commands: Commands,
    mut budget: ResMut<SceneRootBudget>,
    new_roots: NewSceneRoots,
    alive: Query<(), With<BudgetedSceneRoot>>,
    anchors: Query<(&Transform, Option<&ChildOf>)>,
    global_transforms: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    assets: Res<Assets<WorldAsset>>,
    mut template_sizes: Local<HashMap<AssetId<WorldAsset>, u32>>,
) {
    let budget = &mut *budget;
    // Roots entering the gate this frame still hold their `WorldAssetRoot`; roots
    // already held do not. Both compete for the same budget.
    let mut candidates = Vec::with_capacity(budget.held.len());
    let mut sources = Vec::with_capacity(budget.held.len());
    let camera = cameras.iter().next().map(GlobalTransform::translation);
    let distance_squared = |entity: Entity| {
        let Some(camera) = camera else {
            return 0.0;
        };
        let Ok((transform, parent)) = anchors.get(entity) else {
            return 0.0;
        };
        // New roots have no propagated transform yet; an attached root is placed by
        // its parent, a free one by its own translation.
        let position = parent
            .and_then(|parent| global_transforms.get(parent.parent()).ok())
            .map_or(transform.translation, GlobalTransform::translation);
        position.distance_squared(camera)
    };

    for mut held in std::mem::take(&mut budget.held) {
        if !alive.contains(held.entity) {
            continue;
        }
        held.waited += 1;
        candidates.push(Candidate {
            cost: template_entity_count(&mut template_sizes, &assets, held.scene.id()),
            priority: held.priority,
            waited: held.waited,
            distance_squared: distance_squared(held.entity),
        });
        sources.push((held, false));
    }
    for (entity, root, marker) in &new_roots {
        let held = HeldRoot {
            entity,
            scene: root.0.clone(),
            priority: marker.0,
            waited: 0,
        };
        candidates.push(Candidate {
            cost: template_entity_count(&mut template_sizes, &assets, held.scene.id()),
            priority: held.priority,
            waited: 0,
            distance_squared: distance_squared(entity),
        });
        sources.push((held, true));
    }

    let admitted = select_admissions(&candidates, SCENE_SPAWN_ENTITY_BUDGET);
    for ((held, still_attached), admit) in sources.into_iter().zip(admitted) {
        if admit {
            budget.stats.released_roots += 1;
            budget.stats.max_wait_frames = budget.stats.max_wait_frames.max(held.waited);
            budget.stats.total_wait_frames += u64::from(held.waited);
            if !still_attached {
                commands
                    .entity(held.entity)
                    .try_insert((WorldAssetRoot(held.scene), ReleasedSceneRoot));
            }
        } else {
            if still_attached {
                budget.stats.delayed_roots += 1;
                commands.entity(held.entity).remove::<WorldAssetRoot>();
            }
            budget.held.push(held);
        }
    }
    budget.stats.peak_held = budget.stats.peak_held.max(budget.held.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(cost: Option<u32>, waited: u32, distance_squared: f32) -> Candidate {
        Candidate {
            cost,
            priority: SceneRootPriority::Gameplay,
            waited,
            distance_squared,
        }
    }

    #[test]
    fn admissions_respect_the_budget_but_always_make_progress() {
        let candidates = [
            candidate(Some(400), 0, 1.0),
            candidate(Some(400), 0, 2.0),
            candidate(Some(100), 0, 3.0),
        ];
        // The nearest root is admitted, the second no longer fits, the cheap third does.
        assert_eq!(select_admissions(&candidates, 600), [true, false, true]);
        // A template larger than the whole budget is still admitted when it is first.
        let oversized = [candidate(Some(5_000), 0, 1.0), candidate(Some(1), 0, 2.0)];
        assert_eq!(select_admissions(&oversized, 600), [true, false]);
    }

    #[test]
    fn unloaded_assets_stay_held_without_consuming_budget() {
        let candidates = [candidate(None, 0, 1.0), candidate(Some(500), 0, 9.0)];
        assert_eq!(select_admissions(&candidates, 600), [false, true]);
    }

    #[test]
    fn nearer_gameplay_roots_precede_distant_and_warmup_ones() {
        let mut warmup = candidate(Some(10), 0, 0.0);
        warmup.priority = SceneRootPriority::Warmup;
        let candidates = [
            candidate(Some(10), 0, 50.0),
            warmup,
            candidate(Some(10), 0, 5.0),
        ];
        assert_eq!(admission_order(&candidates), [2, 0, 1]);
    }

    #[test]
    fn starved_roots_outrank_camera_distance() {
        let candidates = [
            candidate(Some(10), 0, 1.0),
            candidate(Some(10), PROMOTE_AFTER_FRAMES, 1_000_000.0),
        ];
        assert_eq!(admission_order(&candidates), [1, 0]);
    }
}
