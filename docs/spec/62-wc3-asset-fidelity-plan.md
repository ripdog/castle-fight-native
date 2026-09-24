# WC3 Asset Conversion Fidelity Plan

Status: **active implementation plan**

Prepared: **2026-09-24**, after auditing the Castle Fight Native WC3 asset extractor and client presentation pipeline.

## Objective

Make Warcraft III presentation conversion a Castle Fight-wide capability rather than a per-unit onboarding task.

A supported Castle Fight release MUST have one deterministic, version-scoped presentation asset inventory derived from the retained map extraction. The normal asset-generation workflow SHOULD convert that entire inventory. Unit/building filters remain development accelerators only.

The target end state is that enabling another already-extracted Castle Fight unit or building in gameplay requires no model-specific conversion work unless that object introduces a genuinely new Warcraft presentation primitive. Unsupported or approximated primitives must be explicit and measurable; producing a glTF file is not by itself a successful fidelity result.

## Current baseline

The retained 9.27 extraction already provides broad source inventories:

- `units.tsv` contains 613 map-relevant unit objects, of which 288 are non-building units and 325 are buildings.
- `buildings.tsv` already drives the embedded building asset catalog, so `--buildings` covers all 325 resolved building objects.
- placed doodads/destructables already drive the doodad catalog.
- model-valued object fields across units, abilities, and buffs already drive the projectile/effect catalog.
- icon-valued object fields already drive the UI catalog.
- the remaining unit-art whitelist is the important exception: the embedded unit catalog currently starts from production-spawn units plus race builders instead of every resolved non-building unit object.

The audit also found that successful conversion can still discard visible WC3 features: model-local particles on units, PE2 atlas/color/alpha/head-tail semantics, animated emitter-node transforms, legacy model particles, model attachments, CORN emitters, event objects, lights, material layers/tracks, partial geoset alpha, global-sequence timing, and some hierarchy/skin semantics. These losses currently appear mostly as manifest warnings or are not surfaced at all.

## Implementation progress — 2026-09-24

Steps 1–3 are implemented in the current worktree: the embedded unit catalog covers all 288 resolved non-building unit objects, `--castle-fight` generates the complete unit/building/doodad/effect/UI pack, nested model dependencies are traversed, and the root manifest carries typed fidelity findings that fail on unsupported presentation semantics. The extractor also preserves PE2 scalar tracks, PE1/ribbon tracks, event timelines, material alpha/texture-selection tracks, and geoset alpha/color tracks for later native runtime consumption.

The parser baseline is now `whiteoutlib 0.2.1`. Its native MDX parser supports Warcraft III 3.0/v1800 directly, so the old camera-size rewriting, v1300+ light suppression, and v1400+ SKIN narrowing have been deleted. On the full 9.27 Castle Fight closure this recovered 98 embedded lights instead of 10, exposed 13 additional child-model dependencies, and removed all 755 unsupported-v1800, 144 camera-rewrite, and 87 omitted-light warning occurrences from the old 0.1.7 run. One upstream Rust-binding defect remains explicit: `GeosetAnimation::flags()` is generated with the wrong enum type, so those diagnostic flag values are temporarily marked unavailable rather than guessed; rendering does not currently consume that field.

The current full-pack gate after the parser upgrade is being burned down against the complete 9.27 closure rather than a hand-picked sample. After the PE2, ribbon, animated-material-alpha, animated-material-texture, model-light, attachment-child, and legacy-model-particle milestones, unsupported presentation semantics have fallen from 5,768 to 2,068 occurrences. The remaining unsupported buckets are 2,006 event objects and 62 non-inheritance occurrences. The two unresolved/substituted asset references remain the known `Progressbar.mdx` dummy and missing `sandshield.mdx` source reference.

Step 4 is substantially implemented. Unit and building loaders retain model-local PE2/ribbon metadata; units, buildings, corpses, doodads, projectiles, and effect models use the common runtime emitter/ribbon components; exported WC3 object IDs bind emitters/ribbons to the corresponding animated glTF scene node; unit animation state switches active emitter sequences; and particle/ribbon origin, orientation, dimensions, velocity, and gravity inherit the resolved source-node transform scale. The remaining Step 4 cleanup is to consolidate the still-duplicated per-domain manifest/runtime descriptor types, not to add more per-model rendering exceptions.

Step 5 is in progress. PE2 extraction/runtime now preserves and uses authored emitter width/length, lifecycle `Time`, head/tail UV interval triplets, interval repeat counts, animated speed/variation/latitude/gravity/emission-rate/width/length/visibility tracks, and global-sequence timing. Head particles spawn across the authored WC3 emitter rectangle, atlas frames advance through the authored life/decay ranges, and scale/color/alpha interpolate through all three authored segments using `Time` rather than a hardcoded midpoint. Animated ribbons now evaluate height, alpha, color, texture-slot, and visibility tracks against the selected WC3 sequence/global clock and emit atlas-selected, vertex-colored geometry. Animated material alpha and diffuse-texture selection are also runtime-driven: exported textured layers preserve static alpha in glTF, alpha and texture-ID tracks evaluate against a model-root WC3 sequence/global clock, converted diffuse-frame PNGs are selected without per-model rules, and each animated mesh gets an instance-local material so one model cannot change another's material state. PE2's 2,735, ribbon's 253, material-alpha's 535, and material-texture's 22 track occurrences have all moved from unsupported to documented approximation. They remain approximations while PE2 tail/squirt/replaceable-texture semantics, ribbon exactness, multilayer material flattening, and the remaining global-clock duplication differ from the engine.

Step 6 is also in progress. All 98 model-embedded lights in the complete 9.27 closure are omni lights. The exporter now preserves their static and animated light parameters on model nodes, and the client reconstructs them as model-local Bevy point lights using the shared WC3 sequence/global clock. Authored color, intensity, attenuation-end radius, visibility, node motion, and model scale are runtime-driven; a stock WC3 intensity of 20 maps to Bevy's 1,000,000-lumen default point-light power. The 98 occurrences are classified as approximation rather than unsupported because WC3 attenuation-start, ambient contribution, Reforged falloff/shadow fields, and exact engine attenuation law are not yet reproduced.

The 34 attachment-child occurrences are also runtime-driven now. The exporter records each resolved child glTF plus the parent's attachment-visibility track and sequence/global timing directly on the authored attachment node. The client resolves that child through a pack-wide converted-model registry, spawns it beneath the animated attachment node only while the parent sequence makes it visible, and gives the child its own preferred animation plus PE2/ribbon runtime descriptor. The current 9.27 closure reduces to four shared construction-effect models (`NEBirth`, `UBirth`, `NagaBirth`, and `NagaBirth_Small`). This remains an approximation because child playback restarts when an attachment becomes visible again rather than preserving an independently advancing hidden child-model clock.

Legacy model-particle (PE1) emitters are now restored through the same converted-model registry. All 23 occurrences in the 9.27 closure resolve to child models and animate only emitter visibility; the client evaluates that visibility against the parent WC3 sequence, emits the authored child model at the animated emitter node, and applies emission rate, lifespan, initial velocity, gravity, longitude/latitude cone spread, and source-node/model scale. PE1 remains an approximation because its random angular distribution and child orientation have not yet been validated against Warcraft frame-for-frame.

## Invariants

1. **Castle Fight closure, not Warcraft-wide dumping.** The normal pack is derived from the retained Castle Fight extraction and its referenced stock/imported dependencies. It must not crawl and convert unrelated CASC content merely because it exists in the Warcraft install.
2. **Version scoped.** A pack records the Castle Fight extraction/catalog revision and Warcraft art source. Adding another retained map release must not silently alter an older pack's inventory.
3. **No convincing wrong fallbacks.** Explicit invisible/no-model objects remain invisible. Missing imported custom art is a classified failure unless a source-proven fallback is correct.
4. **Shared model semantics.** Units, buildings, doodads, projectiles, and spell/status effects should consume the same converted-model metadata and native WC3 presentation primitives rather than independently approximating them.
5. **Every visible primitive is classified.** A source feature is either faithfully implemented, intentionally approximated with a documented rule, intentionally irrelevant for Castle Fight, or unsupported. Silent omission is a bug.
6. **Filters are diagnostic only.** `--unit`, `--building`, etc. may speed iteration, but release-quality extraction and fidelity gates run over the complete Castle Fight presentation closure.

## 1. Canonical Castle Fight presentation inventory

Replace the unit production whitelist with the resolved non-building unit inventory from `units.tsv`. Preserve rawcode, base rawcode, authored model path, scale, tint, abilities/attachment art, and explicit no-model state.

Keep the already-broad building, doodad, effect, and UI inventories. Add tests that pin inventory relationships rather than fragile exact counts where possible:

- every non-building resolved unit has exactly one unit asset-spec row;
- no building rawcode appears in the unit model catalog;
- every resolved building has one building asset-spec row;
- intentionally invisible models such as `no_model.mdl` do not fall back to unrelated base art;
- shared source models remain deduplicated after resolution.

The asset inventory should eventually expose source provenance for why each asset is reachable (object field, placed doodad, attachment child model, legacy particle child model, or another converted-model dependency).

**Acceptance:** the default unit catalog is no longer tied to the currently implemented production roster, and expanding the gameplay roster cannot reveal a unit whose authored base model was never considered by extraction.

## 2. One full-pack workflow

Add a first-class full Castle Fight extraction command, rooted at a common output directory, that generates the normal client layout in one run:

- `units/`
- `buildings/`
- `doodads/`
- `effects/`
- `ui/`
- terrain/presentation inputs that belong to the same release where practical

The command should reuse one WC3/map source configuration but keep per-domain manifests so client loading remains modular. Existing narrow modes remain available for debugging.

A top-level pack manifest should record the Castle Fight catalog/revision, Warcraft build/art mode, sub-pack schemas, inventory counts, unresolved assets, and fidelity summary.

**Acceptance:** regenerating the normal Castle Fight presentation pack does not require maintaining a hand-written list of currently enabled units/buildings.

## 3. Fidelity inventory and gating

Extend converted model metadata with a source-feature inventory. At minimum count/classify:

- geosets, skin influence widths, geoset alpha tracks;
- material layer counts, filter modes, animated alpha and texture IDs;
- bones/helpers and non-inheritance flags;
- global sequences;
- attachments and attachment child-model paths;
- ParticleEmitter/ParticleEmitter2/CORN emitters;
- ribbons;
- event objects;
- lights;
- other parsed MDX chunks that affect presentation.

Warnings must become typed fidelity findings rather than free-form strings where possible. The full-pack command prints a summary and can fail a release-quality fidelity gate on unclassified visible losses.

Approximations that remain temporarily acceptable must have stable IDs and tests. A new source primitive appearing in a later map/Warcraft build should therefore fail loudly instead of silently joining an existing warning string.

**Acceptance:** a clean exit means "all encountered visible primitives are implemented or explicitly accepted approximations", not merely "all files parsed."

## 4. Unify model-local VFX consumption

Move ParticleEmitter2/ribbon/model-local metadata into a shared converted-model runtime descriptor consumed by units, buildings, doodads, projectiles, and spell/status effects.

Unit models must consume emitter/ribbon metadata already present in their manifests. Emitters and ribbons must bind to their authored MDX/glTF object node rather than a static root-space position. Sequence selection must control both mesh animation and emitter activation.

Transform semantics must be correct under authored unit/building scale, including dimensions, velocity, gravity, and explicit WC3 non-inheritance behavior.

**Acceptance:** model-local effects no longer disappear solely because the model is being rendered as a unit, and attached effects follow animated bones/helpers through stand/walk/attack/death sequences.

## 5. Complete ParticleEmitter2 and ribbon semantics

Implement the PE2 fields already preserved by extraction instead of discarding them:

- source width/length;
- head/tail/both and tail length;
- texture atlas lifecycle;
- three-stage color, alpha, and scale;
- animated visibility/emission and other relevant tracks;
- authored squirt behavior;
- replaceable texture handling where encountered;
- correct local/source transform scale.

Improve ribbons similarly: animated source node, authored color/alpha behavior, texture-slot/atlas semantics, transform scale, and track-driven parameters.

**Acceptance:** representative PE2/ribbon-heavy WC3 models match reference captures closely enough that size, origin, lifetime, color/opacity, and motion are recognizably the same effect without per-model tuning.

## 6. Restore composed/triggered model features

Implement or explicitly classify:

- legacy model-particle emitters, including recursive child-model dependencies;
- model attachment child paths and visibility tracks;
- CORN emitters;
- presentation-relevant event objects;
- model-embedded lights;
- any additional v1800 chunks encountered by the Castle Fight closure.

Dependency traversal must be cycle-safe and deduplicate shared child assets.

**Acceptance:** a model that visually composes itself from child models/effects is not reduced to only its base geosets.

## 7. Material and animation fidelity

Replace the one-representative-layer material flattening where Castle Fight assets rely on multiple layers. Preserve/filter/order additive, alpha, modulate, team-color/glow, animated alpha, and animated texture selection with WC3-compatible semantics.

Improve animation fidelity for:

- partial geoset alpha instead of binary node scaling;
- global-sequence clocks that continue across clip changes;
- hierarchy non-inheritance;
- skin influence widths beyond Bevy's directly consumed four influences, using a representation that does not visibly deform affected SD models.

**Acceptance:** multilayer/glowing/fading models do not require model-specific material patches, and models with global sequences do not visibly reset secondary animation whenever their main sequence changes.

## 8. Close extracted-art to runtime-event coverage

The effect manifest is an inventory, not proof that the client uses every binding. Add explicit runtime coverage for every relevant role, including:

- attack 1 and attack 2 projectile/impact art;
- ability missile art with authoritative travel paths;
- ability caster/effect/target/special art;
- buff effect/target/special art tied to authoritative status lifetime;
- passive/always-on unit ability art without hardcoding one ability base family.

Unconsumed bindings must be reported by the fidelity/coverage gate rather than falling through a wildcard match.

**Acceptance:** adding a gameplay implementation for an already-extracted ability cannot silently omit its known visual bindings.

## 9. Reference validation

Build a small reference suite of representative models/effects covering each supported primitive. Prefer deterministic camera/animation snapshots or structured render diagnostics over manual memory.

At minimum keep fixtures for:

- ordinary skinned unit + team color;
- multi-layer unit material;
- scaled unit with model-local particles;
- PE2 atlas/color/alpha/head-tail model;
- ribbon projectile;
- model attachment child;
- legacy model-particle child;
- global-sequence model;
- non-inheritance model;
- status/buff visual;
- birth/stand/death building lifecycle.

For a newly retained map release, run the full pack and fidelity gate before calling its presentation assets supported.

## Execution order

Work in this order unless a discovered dependency requires adjustment:

1. canonical full unit/building/object inventory;
2. full-pack command + top-level manifest;
3. typed fidelity inventory/report;
4. shared model-local VFX runtime and unit consumption;
5. PE2/ribbon fidelity;
6. child-model/attachment/CORN/event/light support;
7. material/animation fidelity;
8. complete runtime binding coverage;
9. reference rendering gate and cleanup of remaining accepted approximations.

Early steps deliberately expose more broken assets. That is desirable: the goal is to discover the finite Castle Fight problem set now, then burn it down once, instead of discovering one missing renderer feature every time a gameplay unit is added.

## Definition of done

For a supported Castle Fight release:

- the full presentation pack is generated from retained map data without a hand-maintained gameplay slice;
- every Castle Fight-relevant unit/building/doodad/effect/UI asset is present or explicitly classified as intentionally invisible/unresolved;
- every visible MDX primitive encountered by that closure is faithfully handled or carries an intentional, reviewed approximation;
- the client has no silent wildcard drop of extracted presentation bindings;
- fidelity reports contain no unclassified visible losses;
- enabling an already-extracted unit/building in gameplay normally requires no asset-conversion or renderer changes.
