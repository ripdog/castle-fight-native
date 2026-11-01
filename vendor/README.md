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
