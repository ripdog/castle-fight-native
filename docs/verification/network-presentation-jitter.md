# Network movement and effect flicker

Date: 2026-10-06. Investigation and authorized follow-up fixes.

## Confirmed movement causes

Before the fix, `SimulationPlayback::interpolation_alpha` used the local
`Time<Fixed>::overstep_fraction` for both offline and network matches. Offline fixed updates
advance a simulation tick; network fixed updates can receive no finalized tick. The local
interpolation alpha then wrapped while the previous/current movement interval stayed unchanged.
A reproduction through `process_network_events` recorded:

```text
empty network poll: samples=0->1 alpha=0.900->0.080 position.x=-4335.500->-4350.533
non-tick control: samples=1->1 authoritative tick=1
```

The first line demonstrates backwards visual movement despite forward authoritative movement.
The second demonstrates boundary records replacing the preceding movement endpoint without
advancing gameplay. Those publications could also repeat last-tick cosmetics, and a multi-tick
network burst replaced intermediate snapshots and their events. This affected units, builders,
projectiles, and authoritative action/construction animation phases.

The old defect-asserting reproduction is retained in commit `73565cb`; permanent regressions now
assert the corrected behavior instead.

## Movement changes applied

`bridge/network_timeline.rs` owns a presentation-only queue and monotonic frame-driven clock.
Canonical records, command execution, feedback, and checksum reporting still apply immediately.
Offline interpolation keeps its fixed-update clock.

- Normal network display buffers two confirmed snapshots, adding approximately one simulation
  interval to ordinary presentation latency. Empty polls cannot reset progress. Missing state
  holds the last confirmed endpoint; gameplay positions are never extrapolated.
- After starvation, display buffers again. A lone final snapshot is released after a bounded
  two-interval wait so a stopped server does not leave the last state invisible.
- Larger bursts drain with bounded cosmetic catch-up (up to twice the selected server rate),
  returning to ordinary pacing as the backlog shrinks. Every crossed tick's event lists are
  concatenated in chronological order for consumption in that display frame.
- The queue is capped at 64 snapshots. Overflow logs a warning and explicitly rebases to recent
  confirmed state without replaying historical effects. This is severe-stall recovery.
- Boundary controls patch the latest queued state while preserving its pending tick events.
  Already displayed boundaries preserve the previous endpoint and interpolation progress,
  publish their persistent mutations, clear stale event lists, and do not restart attack poses.
- Queue and alpha changes bypass Bevy resource change detection. Only a published display state
  marks the samples changed, so empty polls and ordinary interpolation cannot replay cosmetics.
- Pause flushes confirmed state; a paused single-step appears immediately. Server speed presets
  scale the display clock. Disconnect freezes display progress and discards queued cosmetics;
  verified reconnect/snapshot handoff resets the queue and clock.
- Rendering, health bars, selection/picking, and portrait tracking use the same network alpha.
  Selection runs after the frame's presentation-clock update.

The normative presentation contract is in `docs/spec/30-client-presentation.md`. No deterministic
simulation, protocol, map tuning, command timing, or gameplay checksum schema changed.

## Fire suppression path fixed; reported frame remains unconfirmed

Ambient doodad emitters advance from frame `Time` in `emit_wc3_particles` /
`update_wc3_particles`. They do not use the network interpolation clock, so backwards movement
interpolation does not directly explain a stationary fire disappearing.

A concrete suppression path existed in `prepare_wc3_particle_texture_bind_groups`: a single
missing `GpuImage` removed its entire shared texture slab's bind group. The draw command then
skipped ready ambient particles sharing that slab with a loading/reloading combat texture.

The renderer now checks readiness once per texture slot during Queue and omits only that slot's
particles. Every slab retains valid bindings: a pending slot binds Bevy's safe fallback until its
image is ready. Its particles stay omitted during that time, preventing placeholder rectangles.
Ready textures keep drawing, including during another slot's hot reload. GPU resource/sampler
identity still invalidates the binding cache when the replacement is ready. This applies to both
texture arrays and the single-texture device fallback.

The opt-in render audit reports pending slots and omitted particles. It also counts affected
frames and capture peaks every frame, independently of the once-per-second transparent census,
so a one-frame loading gap cannot vanish between census samples.

No visible network session or GPU/frame capture was performed. The isolated GUI workspace tools
and a local Xvfb runtime were unavailable in this session. The fix removes the identified shared-
slab suppression path; it does not establish that the user's specific fire dropout took that
path. Authored emitter gaps and transparent ordering remain unconfirmed possibilities.

## Verification

Permanent mechanic regressions cover empty polls through the actual network event and Bevy
change-detection path, monotonic starvation/rebuffering, lone final ticks, burst catch-up while
new ticks continue, chronological event consumption, persistent boundary mutations, pause and
single-step, speed changes, disconnect/reset, and bounded overflow. The integration regression
also confirms replica and source checksums remain equal with different worker counts.

Particle tests cover readiness isolation, untextured slots, highest array slot, single-texture
fallback, and existing cache invalidation for image/sampler replacement and slot reordering.

Validation passed: 238 client tests, 3 existing asset-pack tests ignored; workspace/all-target
Clippy with warnings denied; formatting and diff checks; debug client executable build.

Validation commands:

```text
cargo fmt --all -- --check
tools/cargo-interactive test -p castle-fight-client
tools/cargo-interactive clippy --workspace --all-targets -- -D warnings
tools/cargo-interactive build -p castle-fight-client
```
