# Vendored crates

## bevy_pbr 0.19.1

`bevy_pbr/` is the crates.io release of `bevy_pbr` 0.19.1 with one change,
wired in through `[patch.crates-io]` in the root `Cargo.toml`. The crate is
excluded from the workspace, so workspace `fmt`/`clippy` do not touch it.

The change is in `src/render/skin.rs`. Upstream `extract_skins` walks every
skinned mesh's joints on a single thread, and at late-game unit counts that walk
was the largest system in the render app's extract schedule (about 4 ms), which
stalls the main world while it runs. Each skin owns a disjoint range of
`SkinUniforms::current_staging_buffer`, so the patch gathers the skins, sorts
them by offset, splits the buffer into those ranges with `split_at_mut` and
writes the changed joint matrices on the compute task pool. Below
`PARALLEL_JOINT_THRESHOLD` joints it stays serial. The writes are the same as
upstream's, so results do not depend on scheduling.

The allocator and skin bookkeeping it needs are private to `bevy_pbr`, which is
why this is a fork rather than a system in the client.

### Re-applying on a Bevy upgrade

1. Replace `bevy_pbr/` with the new release from
   `~/.cargo/registry/src/*/bevy_pbr-<version>` (drop `.cargo-ok`,
   `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`).
2. Apply `bevy_pbr-parallel-skin-extract.patch` from inside `bevy_pbr/`
   (`patch -p1 < ../bevy_pbr-parallel-skin-extract.patch`), resolving by hand
   if `extract_skins` changed, then regenerate the patch.
3. If upstream has made the joint walk parallel itself, delete the fork and the
   `[patch.crates-io]` entry instead.

## bevy_transform 0.19.1

`bevy_transform/` is the crates.io release of `bevy_transform` 0.19.1 with one
change, wired in through `[patch.crates-io]` like `bevy_pbr`.

The change is in `src/systems.rs`. Upstream's parallel `propagation_worker`
busy-spins on the work queue until every busy worker finishes, and every
compute-pool thread joins. In the 700-unit benchmark that spin was 4.2% of all
process CPU samples, about as much as the propagation work itself, and it ran
while the render and simulation threads competed for the same cores. The patch
lets a worker spin for `WorkQueue::SPIN_POLLS` empty polls, then yield its
thread between polls. Work hand-off and propagation results are unchanged; a
frame-pointer profile shows propagation CPU down 21% with the same work samples.

### Re-applying on a Bevy upgrade

1. Replace `bevy_transform/` with the new release from
   `~/.cargo/registry/src/*/bevy_transform-<version>` (drop `.cargo-ok`,
   `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`).
2. Apply `bevy_transform-yielding-propagation-workers.patch` from inside
   `bevy_transform/` (`patch -p1 < ../bevy_transform-yielding-propagation-workers.patch`),
   resolving by hand if `propagation_worker` changed, then regenerate the patch.
3. If upstream workers stop busy-spinning, delete the fork and its
   `[patch.crates-io]` entry instead.

## bevy_render 0.19.1

`bevy_render/` is the crates.io release of `bevy_render` 0.19.1 with one
change, wired in through `[patch.crates-io]` like `bevy_pbr`.

The change is in `src/render_phase/draw.rs` and `src/render_phase/mod.rs`. A
draw function keeps per-call ECS state (`SystemState::get` needs `&mut`), and
`SortedRenderPhase::render_range` holds the `DrawFunctions` write lock, so a
phase can record into only one render pass at a time. The client's chunked
transparent pass spent about 6 ms per late-game frame recording ~5k draws on
one thread. The patch:

- records a factory for every draw function registered through
  `add_render_command` (`DrawFunctionsInternal::add_with_factory`);
- adds `DrawFunctions::instantiate`, which builds an independent set of every
  draw function under the same ids (or `None` if one has no factory);
- adds `SortedRenderPhase::render_range_with`, which draws a range with a given
  set instead of the world's.

Draw results are unchanged; only which `SystemState` instance fetches the
parameters differs. In the 700-unit benchmark render/submit/present dropped
from 9.9 to 8.6 ms.

### Re-applying on a Bevy upgrade

1. Replace `bevy_render/` with the new release from
   `~/.cargo/registry/src/*/bevy_render-<version>` (drop `.cargo-ok`,
   `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`).
2. Apply `bevy_render-draw-function-lanes.patch` from inside `bevy_render/`
   (`patch -p1 < ../bevy_render-draw-function-lanes.patch`), resolving by hand
   if `DrawFunctionsInternal` or `render_range` changed, then regenerate the
   patch.
3. If upstream can record one phase into several passes concurrently, delete
   the fork and its `[patch.crates-io]` entry instead.

## bevy_light 0.19.1

`bevy_light/` is the crates.io release of `bevy_light` 0.19.1 with two changes,
wired in through `[patch.crates-io]` like `bevy_pbr`.

The first change is in `src/lib.rs`. `check_dir_light_mesh_visibility` defers marking
shadow-casting entities visible to a command, so it can run alongside
`check_point_light_mesh_visibility`, and that command built a fresh
`QueryState<&mut ViewVisibility>` every frame. Building one visits every
archetype in the world; with ~1,300 archetypes in the 700-unit benchmark it was
about 2% of main-thread samples. The patch keeps the query state in a `Local`
shared with the command, so later frames only visit new archetypes. Which
entities are marked is unchanged.

The second is in `src/point_light.rs` and `src/spot_light.rs`.
`update_point_light_bounding_spheres` queued a command inserting a fresh
`Sphere` for every light whose transform changed, and WC3 model lights move
with their units every frame. The patch writes the sphere in place when the
light already has one and only queues the insert for new lights. Both run before
`VisibilitySystems::UpdateFrusta`, ahead of the visibility check that reads the
sphere, so the value it sees is unchanged.

### Re-applying on a Bevy upgrade

1. Replace `bevy_light/` with the new release from
   `~/.cargo/registry/src/*/bevy_light-<version>` (drop `.cargo-ok`,
   `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`).
2. Apply `bevy_light-cached-deferred-view-visibility.patch` and
   `bevy_light-in-place-light-spheres.patch` from inside `bevy_light/`
   (`patch -p1 < ../<name>.patch`), resolving by hand if
   `check_dir_light_mesh_visibility` or the bounding-sphere systems changed,
   then regenerate the patches.
3. Drop a patch once upstream does the same (its `TODO` in
   `check_dir_light_mesh_visibility` asks for a cached query), and delete the
   fork and its `[patch.crates-io]` entry when neither is left.

## bevy_world_serialization 0.19.1

`bevy_world_serialization/` is the crates.io release of
`bevy_world_serialization` 0.19.1 with one change, wired in through
`[patch.crates-io]` like `bevy_pbr`.

The change is in `src/world_asset.rs`. Upstream `WorldAsset::write_to_world_with`
copies a scene's entities one component at a time: for each component it looks
up the type registration, clones the value through reflection, converts it again
with `from_reflect`, and inserts it, which moves the entity to a new archetype
and table row every time and builds and finishes an entity mapper per component.
WC3 unit and effect scenes spawn every frame in the 700-unit benchmark (about
170 entities a frame, with spikes of 7,000), so this was about 8% of main-thread
samples. The patch resolves registrations once per source archetype, maps
entities once per entity, and inserts every non-relationship component of an
entity together with `insert_by_ids`, so each entity moves archetype once.
Relationship components (`ChildOf`, `Children`) and values that cannot be
cloned into their concrete type still go through the upstream path, after the
batch, with relationship hooks skipped as before. The components written are the
same; hooks and observers for the batched components run once, after all of
them are present.

The batch insert is the crate's only `unsafe`: each value is a leaked `Box`
whose concrete type id was checked against the destination component, and its
allocation is freed without dropping once `insert_by_ids` has moved it out.

### Re-applying on a Bevy upgrade

1. Replace `bevy_world_serialization/` with the new release from
   `~/.cargo/registry/src/*/bevy_world_serialization-<version>` (drop
   `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`).
2. Apply `bevy_world_serialization-batched-scene-inserts.patch` from inside
   `bevy_world_serialization/` (`patch -p1 < ../bevy_world_serialization-batched-scene-inserts.patch`),
   resolving by hand if `write_to_world_with` changed, then regenerate the patch.
3. If upstream spawns scenes with batched or typed inserts, delete the fork and
   its `[patch.crates-io]` entry instead.
