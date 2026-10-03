# Content scalability implementation audit

This is implementation status and follow-up work, not a gameplay specification or a tuning catalog.

## Landed foundations

- `crates/sim/src/content/roster.rs` is an identity-only promotion registry. It generates stable enum IDs, retained rawcode mappings, reverse lookup, and iteration without repeating registrations across methods. Names, production relationships, corpse eligibility, and builder command lists come from retained evidence. Client labels consume the content definitions.
- `tools/wc3-map/build_runtime_catalog.py` generates the complete runtime supplement, including object-authored builder command lists. Missing repair metadata stays optional. Catalog-wide validation checks promoted identity uniqueness and lookup round trips.
- `tools/wc3-map/build_native_tuning.py` generates native tuning from the release-pinned extraction tree. Versioned recipes select source identities/primitive translations, not values. Protected mana/cooldown overlays and script-derived Defend activation timing are consumed directly. Whole-artifact reproducibility tests replace manually copied tuning expectations.
- Version-aware unit definitions and production properties carry the complete translated automatic-ability set. The native importer collects multiple supported profiles, rejects mana conflicts, and composes native/scripted definitions without silently selecting one. Explicit primary identity is retained; unordered imports choose the lowest ID. Current promoted entities still have the same actual ability inventories; this is infrastructure, not new content promotion.
- Production, companion spawning, construction, upgrade cancellation/completion, and wire restoration preserve additional definitions. New bodies initialize fresh slots directly at birth without a global entity-ID scan or temporary profile vectors. Client effect prewarming iterates the complete automatic set.
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
| Single ability | 1 | 7.819 | 3.659 |
| Single ability | 4 | 5.354 | 1.741 |
| Multiple abilities | 1 | 9.568 | 5.352 |
| Multiple abilities | 4 | 5.743 | 2.297 |

These numbers were recorded after the production-definition integration. Each scenario produced identical final checksums across worker counts. This is a single exploratory run, not a regression threshold or a before/after optimization claim. Target candidate work remains substantial; this supports profiling target selection next rather than assuming parallelism eliminates its cost.

## Production-integration verification

Content bundle schema 3 / checksum schema 13 / snapshot schema 10 carry the expanded definitions. Synthetic coverage checks profile composition and rejection, cold definition hashing before the first birth, wire continuation across worker counts, fresh child state, and pending-upgrade cancellation/completion. Catalog-wide tests verify definition transport without re-encoding entity tuning.

Formatting, workspace Clippy, projection tests, artifact checks, and the workspace excluding the server passed. Full workspace runs encountered the already-observed TCP timing failures (`TCP server did not reach expected state` / disconnect control). The duplicate-command case passed in isolation and its readiness failure also reproduced on the previous commit (`2cca441`) in a clean detached worktree. No unrelated server change was folded into this integration.

## Remaining work before broad roster expansion

- Translate additional extracted automatic spell primitives and scripted handlers. The definition/production path now transports multiple supported profiles, but it does not make unsupported mechanics executable or establish race playability. Active building content definitions still need a full multi-ability declaration path (their runtime supports explicitly configured additional slots).
- Migrate remaining scripted Human composite profiles and recovery timing to source-driven projections. They remain version-scoped but some are still hand-authored. Do not treat the native-tuning generator as complete coverage of all scripted map mechanics.
- Generalize triggered attack payloads: multiple spell-proc/Burning Oil guards still exist. Multiple automatic abilities do not remove these separate impact-delivery limitations.
- Replace remaining dedicated delayed-action/status paths with typed mechanic state as new semantics require it. The current secondary-resurrection change is deliberately scoped; there is no general-purpose map-script interpreter or universal delayed-effect queue.
- Profile local target selection before replacing whole-unit scans. Preserve canonical candidate ordering and spatial broad-phase correctness. The benchmark makes cost visible but does not prove every mechanic scales well.
- Audit passive/status/projectile capacities against the complete extracted roster and define rejection/overflow policies for each family.
- Keep entity-specific regressions only for demonstrated integration failures or control-flow ambiguities; continue preferring reusable synthetic suites and catalog/extraction closure checks.

Elven remains incomplete and unselectable. See `elven-implementation.md` for promotion blockers; engine refactoring does not waive fidelity requirements.
