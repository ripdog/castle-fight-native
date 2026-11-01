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
