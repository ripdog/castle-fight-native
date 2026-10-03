# Content scalability implementation audit

This is implementation status and follow-up work, not a gameplay specification or a tuning catalog.

## Landed foundations

- `crates/sim/src/content/roster.rs` is an identity-only promotion registry. It generates stable enum IDs, retained rawcode mappings, reverse lookup, and iteration without repeating registrations across methods. Names, production relationships, corpse eligibility, and builder command lists come from retained evidence. Client labels consume the content definitions.
- `tools/wc3-map/build_runtime_catalog.py` generates the complete runtime supplement, including object-authored builder command lists. Missing repair metadata stays optional. Catalog-wide validation checks promoted identity uniqueness and lookup round trips.
- `tools/wc3-map/build_native_tuning.py` generates native tuning from the release-pinned extraction tree. Versioned recipes select source identities/primitive translations, not values. Protected mana/cooldown overlays and script-derived Defend activation timing are consumed directly. Whole-artifact reproducibility tests replace manually copied tuning expectations.
- `Simulation::configure_additional_automatic_abilities` configures bounded multiple abilities. Slots share mana but retain independent cooldowns, sequences, control flags, and delayed secondary-resurrection state. Registration order is irrelevant; duplicate IDs, capacity overflow, unsupported delayed source kinds, and in-place definition changes are rejected before source mutation.
- Additional runtime state is an optional ECS component: single-ability entities and the hot per-tick unit snapshot do not acquire a full ability array. Logical/wire snapshots, checksums, and saved upgrade-precursor runtime include additional profiles and state. Definition replacement discards obsolete slots; cancellation restores the precursor.
- Resolved unit/corpse resurrection metadata retains additional definitions separately from runtime state. A synthetic live-stat regression also exposed the old snapshot reconstruction losing the original resurrection baseline: it is now preserved, referencing current unit fields in the common case and storing an explicit original only when needed. A revived body restores the complete ability set with fresh cooldown/sequence/delayed state; encoded restoration preserves this metadata. The additional definition payload is cold ECS data, not part of the hot per-tick unit snapshot.
- Delayed secondary resurrection retains its originating ability identity and uses per-slot deadlines/readiness. Due actions are ordered by source/ability ID; additional slots cannot overwrite the primary pending action or one another.
- Synthetic mechanic regressions cover mana contention, independent cooldowns, definition/capacity rejection, registration/worker independence, delayed-slot isolation, spell-versus-ordinary-attack resistance, resurrection definition/state separation, and encoded snapshot continuation. These are engine tests, not numeric assertions about individual map entities.
- `castle-fight-sim-bench --scenarios ability,multi-ability` compares single-ability and mixed damage/stun multi-ability caster density with worker-independent final checksums.

## Exploratory performance probe

Reproduce with `tools/cargo-interactive run --release -p castle-fight-sim-bench -- --scenarios ability,multi-ability --units 1000 --workers 1,4 --ticks 100 --warmup 20`. This probe uses synthetic profiles and a fixed battlefield with 250 building casters; multi-ability casters carry damage and stun slots in addition to the primary damage ability.

| Scenario | Workers | Total ms/tick | Ability phase ms/tick |
| --- | ---: | ---: | ---: |
| Single ability | 1 | 6.040 | 3.172 |
| Single ability | 4 | 3.509 | 1.168 |
| Multiple abilities | 1 | 7.664 | 4.714 |
| Multiple abilities | 4 | 4.089 | 1.729 |

Each scenario produced identical final checksums across worker counts. This is a single exploratory run, not a regression threshold or a before/after optimization claim. Target candidate work remains substantial; this supports profiling target selection next rather than assuming parallelism eliminates its cost.

## Remaining work before broad roster expansion

- Wire multi-ability definitions through production/content metadata. Existing promoted native profiles still use the legacy primary profile, and the native importer continues to fail loudly on multiple translated automatic profiles rather than silently dropping them. The runtime capability is not a claim that every extracted multi-spell unit is promoted.
- Migrate remaining scripted Human composite profiles and recovery timing to source-driven projections. They remain version-scoped but some are still hand-authored. Do not treat the native-tuning generator as complete coverage of all scripted map mechanics.
- Generalize triggered attack payloads: multiple spell-proc/Burning Oil guards still exist. Multiple automatic abilities do not remove these separate impact-delivery limitations.
- Replace remaining dedicated delayed-action/status paths with typed mechanic state as new semantics require it. The current secondary-resurrection change is deliberately scoped; there is no general-purpose map-script interpreter or universal delayed-effect queue.
- Profile local target selection before replacing whole-unit scans. Preserve canonical candidate ordering and spatial broad-phase correctness. The benchmark makes cost visible but does not prove every mechanic scales well.
- Audit passive/status/projectile capacities against the complete extracted roster and define rejection/overflow policies for each family.
- Keep entity-specific regressions only for demonstrated integration failures or control-flow ambiguities; continue preferring reusable synthetic suites and catalog/extraction closure checks.

Elven remains incomplete and unselectable. See `elven-implementation.md` for promotion blockers; engine refactoring does not waive fidelity requirements.
