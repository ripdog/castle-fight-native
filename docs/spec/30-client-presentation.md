# Client Presentation Architecture

Status: **normative separation, provisional rendering details**

## 1. Purpose

This document defines the client-side presentation layer: rendering, interpolation, animation, audio, effects, camera, and presentation event handling.

The central rule is strict separation from authoritative gameplay. Presentation may lag, interpolate, predict, embellish, or drop cosmetic work as needed, but it MUST NOT alter simulation outcomes.

## 2. Full Bevy client

The client SHOULD use the full Bevy engine for:

- window/input integration;
- rendering;
- assets;
- UI;
- audio;
- presentation scheduling;
- optional render-world extraction and GPU instancing.

The authoritative simulation SHOULD remain a separate crate/module that can run identically in the headless server.

## 3. Simulation/presentation boundary

The client consumes completed authoritative simulation states and events.

Authoritative state examples:

```text
SimId
SimPosition
Health
TargetState
UnitTypeId
BuildingTypeId
status/gameplay animation state where relevant
```

Presentation state examples:

```text
render Transform
mesh/material handle
animation blend state
particle emitter
floating combat text
selection highlight
camera shake
sound instance
```

Presentation state MUST NOT be included in authoritative checksums/snapshots.

## 4. Mapping entities

The client maintains a presentation mapping:

```text
SimId -> presentation/render entity or instance handle
```

The mapping is process-local and rebuildable.

A simulation entity spawn causes presentation creation. Despawn may leave a cosmetic corpse/death animation after the authoritative entity is gone, provided the cosmetic object has no gameplay effect.

## 5. Interpolation

Simulation may run at 20–30 Hz while rendering runs at the display rate.

The presentation layer SHOULD interpolate between completed authoritative samples.

Conceptually:

```text
sim tick N                 sim tick N+1
    |---------------------------|
                 ^
              render
```

Interpolation is visual only. The displayed position may differ slightly from the latest canonical `SimPosition`.

Gameplay hit testing, targeting, and building validation MUST use authoritative simulation coordinates.

## 6. Catch-up and discontinuities

After reconnect/desync snapshot replacement, presentation may observe a large authoritative discontinuity.

The client SHOULD choose sensible visual recovery based on magnitude/context:

- small correction: blend over a short cosmetic interval;
- large correction/snapshot replace: snap or rebuild presentation;
- historical catch-up: do not render every intermediate tick.

Presentation smoothing MUST NOT delay authoritative command processing.

## 7. Rendering high unit counts

High unit count is a primary target.

The renderer SHOULD be designed to avoid one expensive draw submission/material state change per unit.

Potential techniques include:

- GPU instancing by unit type/material;
- texture arrays/atlases;
- GPU-driven culling/indirect draw where beneficial;
- animation state represented compactly per instance;
- level-of-detail simplification;
- reduced update frequency for distant/off-screen presentation;
- pooled presentation entities/resources.

The exact rendering architecture should be measured after a simple playable simulation exists.

## 8. Simulation entity count is not render entity count

The client MAY represent many simulation entities using a more compact rendering representation than one full Bevy render entity each.

For example:

```text
4,000 Footman simulation entities
        ↓
compact extracted instance records
        ↓
several GPU batches
```

This optimization MUST preserve stable mapping for selection/inspection where needed.

## 9. Animation

Animation timing may be cosmetic unless a gameplay rule depends on a named animation phase.

Authoritative attack windup/cooldown uses simulation ticks. Presentation should synchronize an attack animation to authoritative attack events, but the animation event itself must not cause damage.

Correct direction:

```text
simulation AttackIntent/resolution
        ↓
presentation event
        ↓
play swing / projectile visual / sound
```

Forbidden direction:

```text
animation keyframe callback
        ↓
apply authoritative damage
```

## 10. Projectiles

Presentation MUST reflect the authoritative attack delivery mode rather than infer semantics from missile art.

- **Melee:** no gameplay projectile; animation is cosmetic around the authoritative strike timing.
- **Ranged guaranteed-hit:** render/interpolate a missile over the authoritative launch/impact interval toward the retained target. Target movement does not make the authoritative attack miss.
- **Ranged ballistic:** render an arc toward the authoritative captured impact point. The visual projectile MUST NOT visually home after launch; the 2D destination/impact tick comes from simulation state.
- **Bounce:** render each guaranteed impact/jump according to the authoritative bounce sequence and timing.

The vertical shape of a ballistic arc MAY be cosmetic when only its fixed destination and impact tick matter to gameplay.

For any already-resolved attack, presentation MAY create cosmetic travel/effects only when doing so cannot imply a contradictory gameplay result.

Presentation must distinguish authoritative and cosmetic projectile state so a visual effect cannot accidentally become a hidden gameplay timer.

## 11. Audio

Audio is cosmetic and may be culled/aggregated aggressively at high event rates.

A battle with thousands of units MUST NOT attempt to play thousands of simultaneous full-volume samples.

Potential policies:

- per-sound concurrency limits;
- spatial/viewport culling;
- event aggregation;
- randomized cosmetic pitch/variant using a presentation RNG, not authoritative RNG;
- priority classes for important UI/building/castle sounds.

Audio nondeterminism is acceptable.

## 12. Visual randomness

Particles, animation variation, cosmetic offsets, and sound variations MAY use nondeterministic client-local randomness.

Such random values MUST remain confined to presentation.

A useful code-level boundary is separate RNG types/modules for:

- authoritative deterministic RNG;
- cosmetic/presentation RNG.

## 13. Camera

The camera is purely client-local.

Expected RTS controls may include:

- pan;
- zoom;
- optional rotate;
- jump to castle/important event;
- follow selected entity.

Camera movement MUST NOT modify the authoritative simulation.

## 14. Selection/inspection and builder presentation

The user may select/inspect combat units/buildings for information. Ordinary combat-unit selection MUST remain inspection-only and MUST NOT expose direct orders.

The player's builder is the sole directly controlled unit and should be visually/UI-distinct enough that its movement/build/inventory controls are not confused with combat-unit inspection.

Picking may use render/GPU/scene data to identify a `SimId`, but displayed authoritative stats should be resolved from the latest simulation state.

A stale/dead `SimId` must fail gracefully.

## 15. Presentation events

The simulation should expose enough event/state change information to drive presentation without forcing the presentation layer to diff the entire world for every effect.

Examples:

- unit spawned;
- attack fired;
- projectile spawned/impact;
- damage/heal;
- death;
- building constructed/destroyed;
- upgrade purchased;
- ability cast / mana spent / aura state changed where useful for presentation;
- item activated;
- builder moved/build interaction;
- castle damaged;
- victory.

Presentation events are derived from authoritative outcomes. They need not themselves be persisted in snapshots if they can be omitted during catch-up or reconstructed as needed.

## 16. Historical event suppression

During reconnect/replay fast-forward to live state, the client SHOULD suppress or coalesce historical cosmetic events to avoid a burst of delayed sounds/particles.

Events that matter to UI comprehension may be summarized rather than replayed literally.

## 17. Assets and content identity

Presentation assets are referenced from gameplay content through stable logical asset IDs/paths.

Missing presentation assets should produce a clear fallback/error but MUST NOT change authoritative content semantics.

Client cosmetic packs may eventually vary visuals while sharing the same gameplay content hash, provided gameplay-relevant geometry is not inferred from cosmetic assets.

## 18. Debug visualization

The client SHOULD provide optional developer overlays for:

- `SimId`;
- collision radius;
- spatial grid cells;
- target links;
- acquisition/attack ranges;
- navigation integration/flow field;
- disconnected/caged components;
- pathing blockers/building footprints;
- worker/phase profiling;
- authoritative vs interpolated position.

These tools will be important for debugging emergent pathing/target behavior.

## 19. Headless independence test

Any gameplay behavior that changes when the client renderer/audio/UI is disabled is an architecture defect.

Automated tests SHOULD run the same simulation headlessly and inside a minimal client harness and compare authoritative checksums.
