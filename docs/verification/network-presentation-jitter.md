# Network movement and effect flicker investigation

Date: 2026-10-06. Scope: investigate the reported movement jitter and brief disappearance of
ambient doodad fire. No production movement/interpolation/particle changes are included in this
investigation.

## Confirmed movement cause: render time can run backwards

`SimulationPlayback::interpolation_alpha` uses the local `Time<Fixed>::overstep_fraction` for
both offline and network matches. In offline play each unpaused fixed update advances a simulation
tick. Network fixed updates instead drain whatever messages have arrived. A fixed update with no
finalized server tick keeps the same previous/current samples but resets the interpolation alpha.
A smoothly moving unit therefore interpolates backwards within its last interval. The same issue
affects builders, projectile positions, and authoritative action/construction animation phases.
A burst of multiple ticks also overwrites intermediate samples rather than pacing their display.
This requires no pathfinding turn, collision, or change in authoritative speed.

A temporary client test used the actual `process_network_events` path, a canonical forward builder
move, and Bevy's fixed overstep clock. It first applied a finalized tick, then simulated another
local fixed boundary with an empty network poll. Results:

```text
empty network poll: samples=0->1 alpha=0.900->0.080 position.x=-4335.500->-4350.533
non-tick control: samples=1->1 authoritative tick=1
```

The authoritative builder moved forward. Its rendered X moved backwards by about 15 world units
while the simulation remained at tick 1. Unit transforms use the same interpolation factor.
The test passed by asserting these observed defects; it was removed from the production test
suite so the existing behavior is not enshrined as a contract. Its reproducible test body is
retained in `network-presentation-reproduction.rs.txt` beside this report. Insert it into the
`tests` module in `crates/client/src/main.rs` and run:

```text
tools/cargo-interactive test -p castle-fight-client investigate_network_presentation_clock_and_control_publication -- --nocapture
```

## Confirmed second cause: non-tick records replace movement history

The live `ServerMessage::StreamRecord` branch publishes a presentation snapshot even when
`MatchDriver::apply_stream_record` returns no tick result (a boundary control). Publishing the
same tick replaces the previous movement sample with the current sample. The reproduction above
also applies a no-op connection control and demonstrates the 0->1 sample interval collapsing to
1->1 without advancing gameplay. That produces a forward snap and can repeat last-tick cosmetic
events because presentation event lists are still present. Host debug controls now also expose
this existing boundary-publication problem, but it predates that change.

## Fire flicker: separate renderer candidate, not yet tied to the reported frame

Ambient doodad emitters are created by `DoodadPresentationPlugin` and advanced from frame `Time`
in `emit_wc3_particles` / `update_wc3_particles`. They do not depend on canonical network ticks,
`PresentationSamples`, or `SimulationPlayback::interpolation_alpha`. The backwards interpolation
clock does not directly explain stationary doodad particles disappearing.

A concrete suppression path exists in `prepare_wc3_particle_texture_bind_groups` in
`crates/client/src/particle_renderer.rs`. Particle textures are packed into shared slabs, including
ambient and combat effects together. Texture preparation collects all `GpuImage` lookups into
`Option<Vec<_>>`. If one texture is not ready (initial load or reload), it removes the entire slab's
bind group. `DrawWc3BillboardParticleCommand` then skips every batch using that slab, including
otherwise-ready fire particles. When the texture arrives, those particles can reappear. This is
a plausible brief-flicker cause when new combat-effect textures enter the same slab; it is not
proof that the user's particular disappearing frame took this branch.

No visual network session or GPU capture was performed. Additional possibilities include authored
emitter/atlas visibility gaps and frame-to-frame transparent ordering; this investigation has not
established those as causes.

## Recommended follow-up

- Pace network presentation from a monotonic render clock and a bounded queue of finalized
  snapshots. Do not reset display progress when a local fixed update receives no server tick;
  handle bursts, late arrivals, pause/speed, and reconnect explicitly. Consume cosmetic events
  once per displayed tick.
- Apply boundary state changes without discarding the last movement interval or replaying
  its events. Merely skipping every boundary snapshot would hide legitimate host mutations.
- Record missing texture/slab draw skips during a visible fire dropout. Isolate not-ready
  particle textures from ready slabs, or use a safe per-slot placeholder, then verify against
  a GPU/frame capture before claiming the reported fire issue fixed.

The requested attack and debug fixes are verified separately. This report deliberately keeps
movement diagnosis distinct from an unverified rendering symptom.
