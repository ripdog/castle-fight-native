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

A simulation entity spawn causes presentation creation. When a corpse-producing unit dies, presentation should transition from the living unit to the authoritative corpse entity/state rather than replacing gameplay state with a cosmetic-only corpse. The client may still layer non-authoritative death animation, particles, decals, or later visual remains around that corpse. For units/content that do not produce authoritative corpses, despawn may leave cosmetic death remnants provided they have no gameplay effect.

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

### 5.1 Terrain presentation and ground clamping

The standard Castle Fight client SHOULD render the committed Warcraft terrain using the extracted W3E grid's original world bounds and vertex heights rather than an unrelated flat presentation plane. The current imported map spans 132 × 64 terrain tiles at 128 Warcraft world units per tile, from `(-8192, -4096)` through `(8704, 4096)`.

Presentation height uses the Warcraft terrain-display conversion of the extracted vertex data: `groundHeight - 8192 + (layerHeight - 2) * 512`, expressed in quarter-world-unit height samples. The presentation surface MAY use clamped higher-order interpolation and additional tessellation between those exact imported vertices to round abrupt ramp/plateau joins, provided every authored vertex height remains unchanged. This smoothing is presentation-only; authoritative uphill combat continues to use the simulation's discrete cliff-level rule.

Imported Warcraft ground atlases are presentation diffuse assets and SHOULD retain their authored color and brightness relationships across terrain slope. The client SHOULD display flat ground at the authored texture brightness and MAY apply a small, smoothly interpolated presentation-only darkening based on slope steepness to reproduce Warcraft's soft ramp shading. It MUST NOT allow generic scene-light direction to drive steep textured ramps/cliff transitions nearly black merely because the presentation mesh normal faces away from a directional light.

The retained W3E terrain can extend beyond the release's authoritative build regions. For Castle Fight 9.27, terrain west/east of the outer canonical team build-region edges SHOULD be presented as near-black dead space, matching Warcraft's boundary treatment rather than exposing ordinary bright terrain. The presentation mask is derived from the selected release's versioned build-region geometry, not terrain or generic navigation bounds. Those masked side bands are also blocked for ordinary ground and air movement; the mask itself remains presentation-only and MUST NOT enlarge the traversable or buildable map.

The client MUST consume the selected release's validated terrain source and resolved content bundle from shared match setup rather than independently opening an implicit default-version extraction. Generated unit/building models should be loaded only for content promoted into that resolved bundle (plus explicitly required shared objective/presentation assets), so adding another retained archive does not eagerly load or accidentally expose unrelated roster art. Cosmetic camera limits remain client-owned even when authoritative navigation/build bounds come from shared match definitions.

Warcraft player colour is presentation keyed by the entity owner's stable `PlayerId`/WC3 slot, not by `Team`. Allied players therefore retain distinct model tint, team-glow, corpse/remnant, health-bar, and UI colours while `Team` continues to express allegiance. Replaceable Team Glow (`ReplaceableId=2`) resolves `TeamGlowNN` using that owning slot; generated packs that contain team-glow materials SHOULD include the full Warcraft player-colour glow set needed by supported slots rather than assuming a red/blue two-side palette.

Ground-bound render entities such as units, buildings, corpses, selection markers, and building-placement previews SHOULD sample this same presentation heightfield. Flat rigid building models and their placement previews use the highest presentation-height sample covered by their authoritative footprint as their support elevation, so terrain variation inside the footprint cannot clip low model geosets; the preview and completed model MUST use the same support rule. Rendered unit positions MUST be clamped to the imported terrain bounds/height so interpolation or cosmetic motion cannot leave a unit visibly below, above, or outside the terrain. This clamping MUST NOT feed presentation Y coordinates back into authoritative movement or combat.

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

### 9.1 Imported building lifecycle animation

Generated Warcraft building assets SHOULD resolve their presentation lifecycle clips during extraction, using the building object's versioned required-animation properties together with the exact sequences present in the shared source model. The generated manifest is the client contract for the selected `Birth`, ambient `Stand`, and `Death` sequence names; the runtime SHOULD NOT maintain per-building animation-name tables or independently guess upgrade variants.

An authoritative building in the `constructing` state SHOULD display its selected `Birth` sequence at the phase implied by the simulation's construction start/completion ticks. Birth playback MUST be driven by authoritative construction progress rather than by the source clip's natural duration: a two-second construction window therefore consumes the full Birth sequence in two simulation seconds regardless of the MDX sequence length. Presentation initialization/rebuild during construction MUST seek to the current construction phase; a building already completed when presentation is initialized starts from the selected looping `Stand` sequence instead of replaying historical Birth. When construction completes, presentation transitions immediately to `Stand`. Cancelling an unfinished construction site removes that presentation without playing the completed building's `Death` sequence. When a completed authoritative building is destroyed, the client MAY retain its render entity as a presentation-only remnant long enough to play the selected `Death` sequence and associated sequence-scoped effects, then remove it.

These transitions are presentation of authoritative lifecycle state, not authority themselves. A construction site blocks according to the simulation as soon as construction starts but remains functionally inactive until its authoritative completion tick; pausing the simulation also freezes Birth progress. Authoritative destruction immediately removes occupancy, economy, targeting, production, and other gameplay effects even while the client is still displaying a completed building's Death animation. Presentation animation duration MUST NOT extend construction or destruction semantics.

## 10. Projectiles

Presentation MUST reflect the authoritative attack delivery mode rather than infer semantics from missile art.

- **Melee:** no gameplay projectile; animation is cosmetic around the authoritative strike timing.
- **Ranged guaranteed-hit:** render/interpolate a missile over the authoritative launch/impact interval toward the retained target. Target movement does not make the authoritative attack miss.
- **Ranged ballistic:** render an arc toward the authoritative captured impact point. The visual projectile MUST NOT visually home after launch; the 2D destination/impact tick comes from simulation state.
- **Bounce:** render each guaranteed impact/jump according to the authoritative bounce sequence and timing.

The vertical shape of a ballistic arc MAY be cosmetic when only its fixed destination and impact tick matter to gameplay.

Authoritative staged Chain Lightning events SHOULD retain the bounce index needed for presentation to distinguish Warcraft's primary and secondary lightning primitives. When the original lightning texture/data is available, the client SHOULD render that textured primitive instead of substituting an unrelated solid-color line. The transient bolt SHOULD decay its additive intensity over its short display lifetime rather than remaining fully opaque until an abrupt despawn.

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

- keyboard pan;
- screen-edge pan while the client window is focused;
- middle-mouse drag pan;
- zoom;
- optional rotate;
- jump to castle/important event;
- follow selected entity.

Screen-edge panning uses a narrow logical-pixel band at each viewport edge, combines both axes at corners, and follows the same camera-relative pan directions/speed as keyboard panning. It is disabled while middle-mouse drag panning is active and when the window is not focused. Camera movement MUST NOT modify the authoritative simulation.

## 14. Selection/inspection and builder presentation

The user may select/inspect combat units/buildings for information. Ordinary combat-unit selection MUST remain inspection-only and MUST NOT expose direct orders.

Air units SHOULD be presented at an obvious visual altitude above the sampled terrain, with presentation-only motion or silhouettes that distinguish them from ground units. Unit picking SHOULD test the rendered 3D unit volume rather than only the terrain point beneath it, so clicking an airborne model selects that unit at its visible altitude.

The player's builder is the sole directly controlled unit and should be visually/UI-distinct enough that its movement/build/inventory controls are not confused with combat-unit inspection.

Picking may use render/GPU/scene data to identify a `SimId`, but displayed authoritative stats should be resolved from the latest simulation state. When authoritative content identity is available, inspection UI SHOULD show the canonical imported unit/building name rather than only a presentation-role label.

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

Persistent Warcraft buff/status art MUST follow authoritative status state rather than a presentation-side timer. Generated effect metadata binds an ability modifier and status category (for example movement or armor) to the corresponding WC3 target model; the client creates that visual only while the matching authoritative modifier is active and keeps it attached to the affected unit as it moves. This covers effects such as the Ice Troll Priest's Frozen target art, Frost Armor shell, and the reactive frost slow without duplicating their gameplay duration or proc logic in presentation.

An authoritative ordinary-attack miss SHOULD produce clear transient client feedback. The current client renders `MISS` above the missed target for approximately one second when the corresponding authoritative `AttackEvent` has `missed = true`. This text is cosmetic and MUST NOT perform or infer its own accuracy roll.

## 16. Historical event suppression

During reconnect/replay fast-forward to live state, the client SHOULD suppress or coalesce historical cosmetic events to avoid a burst of delayed sounds/particles.

Events that matter to UI comprehension may be summarized rather than replayed literally.

## 17. Assets and content identity

Presentation assets are referenced from gameplay content through stable logical asset IDs/paths.

Missing presentation assets should produce a clear fallback/error but MUST NOT change authoritative content semantics.

Imported Warcraft doodad presentation SHOULD preserve the source model's normal `Stand` visibility state and ambient ParticleEmitter2/ribbon metadata where the extractor can represent them. Particles/ribbons MUST resolve textures from the same generated asset pack as their owning model rather than assuming all emitters come from the transient-effects pack. Generated doodad packs are versioned presentation data; a client MAY reject an older schema when exporter changes are required to prevent stale geometry/visibility semantics from rendering incorrectly.

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
