# Golden Shrine of Justice fidelity audit — Castle Fight 9.27 r1

## Identity and retained evidence

- Entity: Golden Shrine of Justice, `h059`, inherited Warcraft base `hbla`.
- Runtime identity: `CastleFightTowerKind::GoldenShrineOfJustice`, building ID
  `0x3000000d`. This registers executable content only; Elf promotion and final
  release/source publication remain separate gates.
- Production source: Elf builder `X00P`, non-production utility building, authored
  registration `AK` / `Urb`. Command-card position, hotkey, legendary allocation,
  costs, construction/repair timing, health, armor and footprint use the existing
  version-aware extraction-backed tower definition APIs.
- Object evidence: rows keyed by `h059` in
  `docs/original_map/extracted/resolved/{units,buildings,production-buildings,protected-unit-stats}.tsv`.
  The protected `jP` life override, not the poisoned static life field, is authoritative.
- System evidence: `runtime-system-mechanics.tsv`, system `golden-shrine-revival`.
- Actual retained Lua: `script/war3map.lua` in the release-pinned extraction tree
  selected by `docs/original_map/releases.json`. Reproducible projection:
  `crates/sim/data/castle-fight/9.27/shrine-system-r1.json`. Its source block retains
  script/table SHA-256 identities, function names and decoded protected-call evidence.

Regenerate or audit without modifying shared native-effect recipes/bindings:

```sh
# Materialize the selected release's script to a disk-backed workspace first.
# <tree> is the extraction tree recorded in docs/original_map/releases.json.
git show <tree>:script/war3map.lua > <disk-workspace>/shrine-source.lua
python3 tools/wc3-map/project-shrine-system.py --source <disk-workspace>/shrine-source.lua
python3 tools/wc3-map/project-shrine-system.py --source <disk-workspace>/shrine-source.lua --check
```

## Base data and complete inventory

- Both ordinary weapons are disabled. There is no production queue, movement,
  mana resource, spell AI, cast range, target acquisition or active cooldown.
- Native always-on building health regeneration is projected from the retained
  unit row and uses the engine's fixed-point accumulator. The accumulator is
  canonical and wire-restorable; regeneration cannot raise a dead handle.
- `A06V` (Legendary Unit / `Asph`) remains in the resolved native ability inventory.
- `A06A` (Critical Strike / `ACct`) remains represented as a dormant native
  dependency. It cannot proc because the building has no enabled weapon. Its
  stable behavior ID `0x43000001` is in a dedicated shrine inventory range, avoiding
  coordinator additions to the ordinary shared spell-ID sequence.
- The utility structure never enters the combat-unit corpse/revival predicate.
  Mechanical/organic classification and collision/pathing data are retained as
  object evidence; building occupancy uses the extraction-backed footprint.

## Script trace and source distinctions

Audited functions: `ME`, `onBuildingFinished`, `acquireElvenShrine`,
`elvenShrineEffectiveReviveChance`, `QE`, `migrateBuildingLifecycleOwner`, `fJ`,
`eJ` / `consumeDontReviveFlag`, `dJ` / `isShrineReviveBlocked`,
`markShrineReviveBlocked`, the delayed `OnUnitDeathHandler` callback and its
`Action_watch_OnUnitDeathHandler_run_watch_OnUnitDeathHandler` round-end watcher.

- **Lifecycle:** completed construction acquires the team contribution; removal,
  death and owner migration release/migrate it. Owner migration within the same
  team does not alter team chance. The simulation derives the capped team chance
  from completed, living canonical buildings instead of keeping a drift-prone
  duplicate counter. Contributions are computed once per combat commit phase.
  Pending construction follows its new owner but contributes only on completion.
- **Trigger eligibility:** actual fatal combat damage to a non-structure combat
  sapper, under `fJ`'s non-nil killer branch. No-killer developer/external deaths
  do not proc. Air, mechanical and hero units are not inherently excluded.
- **Exclusions:** `A07H` legendary marker, `A07G` summoned marker, native summoned,
  native illusion, one-shot suppression and the permanent shrine-revived flag.
  The illusion predicate is an additional source-script constraint beyond the
  TSV's explicit parameter list: the protected call decodes to native
  `__wurst_safe_IsUnitIllusion`. Hero status is not a substitute for `A07H`.
- **Geometry:** no distance or approach condition; the shrine is a team passive.
  Owner, unit type and exact death position are captured at successful death.
- **Resources/chance:** no mana/cooldown commitment. The chance roll bounds,
  contribution, cap and literal delay come from the dedicated projection.
  Deterministic keyed RNG replaces Warcraft RNG without worker/order dependence.
- **Delayed sequence:** the exact source delay commits an authoritative callback.
  It is not conditional on corpse presence, raise eligibility, shrine survival or
  a second chance roll. The callback recreates the original owner's cold unit
  definition at the captured position and removes the original dead handle.
  Native resurrection preserves original-handle identity across new simulation
  IDs and subsequent deaths, so the callback removes its living/corpse incarnation.
  Corpse consumption or expiry does not cancel the replacement.
- **Generation:** `A7` is a **round epoch**, not a per-unit death counter. Additional
  deaths do not invalidate earlier callbacks. Round/match completion advances the
  epoch; a mismatch discards the callback without consuming the corpse or emitting
  a revival visual. An explicit boundary hook supports round controllers.
- **Fresh baseline:** replacement resets health/max health, armor, weapons, mana
  and regeneration remainders, attack/primary/additional ability cooldowns,
  cast sequences, autocast state and statuses. Runtime flags are separate from
  the cold `ResurrectionProfile`; native resurrection preserves handle flags,
  while shrine replacement receives its new permanent one-time block.
- **AI:** no shrine-specific AI or orders. Replacement enters the engine's normal
  fresh unit-spawn lifecycle, not the dead unit's stale target/AI state.

## Authoritative state and presentation

- Dedicated typed `DelayedShrineRevival` ECS component; canonical entity checksum
  kind tag **11**, leaving tag 10 for the coordinator's `NativeAction` component.
- Callback fields, cold definition, round generation and handle flags on live
  units/corpses all participate in canonical checksums and snapshot wire transport.
  Supporting buildings supply explicit script-version context; live/original
  content identities must agree before scheduling. The callback retains that
  version independently of optional cold body identity, and uses it after source
  removal. Wire decoding validates callback version even for an unlabelled body
  before rehydrating any captured content identity.
- Presentation-only `ShrineRevivalEvent` carries replacement identity, exact effect
  origin and the native model path extracted from the callback. The client bridge
  forwards it and the renderer uses the normal timed WC3 visual pool/lifetime.
- Asset catalog generation emits a dedicated `systems/resurrection` binding.
  The client reconciles retained `.mdl` script spelling with the exporter's `.mdx`
  normalization and uses native animation metadata for visual lifetime; visual
  duration does not gate gameplay.
- No native assets are hand-authored or committed. The normal WC3 asset export
  pipeline supplies the model, with existing missing-asset fallback behavior.

## Focused verification

Mechanic fixtures live under `crates/sim/src/simulation/shrine/tests*`:

- team stacking, cap, deterministic success/failure thresholds;
- same-team and cross-team ownership, idempotence, allocated legendary-point
  migration/release, construction completion/removal and mid-construction transfer;
- exact delayed tick, exact captured owner/position/content, corpse consumption
  and absence, fresh primary/additional spellcasting and stat baseline;
- legendary, marker/native summoned, native illusion, non-sapper, alive,
  wrong-team/no-shrine and no-killer negative cases;
- one-shot suppression consumption boundaries and a later successful native
  resurrection/redeath; permanent block through native resurrection;
- original identity through repeated native resurrection/redeath;
- concurrent deaths versus round-epoch invalidation, match completion/freeze;
- pending callback, flags, cold baseline, fractional building regeneration and
  classification transport across wire restoration with different worker counts;
- catalog-wide marker/sapper/native summoned classification agreement with retained
  inventory, rather than per-entity copies of tuning values.

Additional asset/client tests verify projection-sourced catalog art, normalized
model identity lookup and animation-driven (not gameplay-delay-driven) VFX lifetime.

Source-context closure passed all 22 Shrine tests (including five reusable
version cases), four earlier version lifecycle tests, all 384 integrated simulation
tests, strict simulation all-target Clippy, and client all-target checking. Logs
are disk-backed under `castle-fight-native-worktrees/elven-full/shrine-version-*.log`.
The new cases cover version-only checksum/wire rejection without body identity,
source removal and worker continuation, live/original mismatches before allocation,
unsupported count/transfer identities, and activation before native counters/regen.
A further lifecycle matrix checks standalone carrier/control/callback versions.
Compatibility constants are bundle 5/checksum 18/snapshot 15; final publication,
imported visual review and native-executable observations remain open.
