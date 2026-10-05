# Elven caster fixture fidelity audit

Scope: Dragonhawk Rider `n01Y` / Dragonhawk Portal `h06Y`, Sorceress
`h07B` / School of Wizardry `h09X`, and Wizard `h00W` / Tower of Supreme
Magic `h00X`. These are native verification fixtures, **not race exposure**.
No release/source-manifest revision is published by this change.

## Evidence identity and reproducibility

Map: Castle Fight 9.27. Source identity: registered retained r1 extraction tree
`8ea806dca331ff254995e94e6f0baf225a14bf10`, as selected by
`docs/original_map/releases.json`. Runtime tuning belongs to the versioned 9.27
content directory; recipes contain source identities only.

Reproduce/check:

```sh
python3 tools/wc3-map/build_native_tuning.py --map-version 9.27 --revision r1 \
  --check crates/sim/data/castle-fight/9.27/native-effect-tuning.json
python3 -m unittest discover -s tools/wc3-map -p 'test_build_native_tuning.py'
```

Use `git show <tree>:<path>` to read retained files (not the mutable working
extraction alias). Relevant paths:

- `resolved/units.tsv`, `production-buildings.tsv`, `protected-unit-stats.tsv`:
  inherited base identities, protected life/armor/movement and production data.
- `resolved/production-unit-attacks.tsv`, `production-unit-corpses.tsv`:
  ordinary weapon/delivery/target masks, corpse creation and eligibility.
- `resolved/production-unit-abilities.tsv`, `object-fields.tsv`,
  `protected-ability-fields.tsv`: complete inventory and child-native tuning.
- `resolved/unit-spells.tsv`, `unit-spell-semantics.tsv`,
  `unit-spell-mechanics.tsv`: AI registration, trigger/effect distinction,
  helper sites, protected resources and scheduled callbacks.
- `script/war3map.lua`: actual minified control flow. Extract functions by
  locating `function <name>(` and the following `function ` boundary; do not
  treat the entire one-line file as a single useful line-number reference.

Native (not map tuning) corroboration:
[WC3 editor ability insight, 2023-11-14](https://raw.githubusercontent.com/Cokemonkey11/wc3-ability-doc/main/sources/screwthetrees-wc3-editor-ability-insight-document/2023-11-14.md),
sections `Phoenix Fire` and `Chain Lightning / Healing Wave` (anchors 21 and
79). Read the source itself: quarter-second hops, Healing Wave least-current-HP
priority, and Phoenix Fire buff-on-impact/one-second damage are documented there.
This is observational evidence, not a substitute for the map script.

## Checklist reconciliation

### Base data and ordinary attacks

All three fixtures consume the existing extracted unit/production importer,
including protected stats, target masks, imported missile delivery, classification,
health regeneration, corpse and cost/build/production profiles. Base objects are
`nws1`, `hsor`, and `hmpr` respectively. No independent hand-authored stat table
was introduced. Production and direct spawning share resolved definitions.

The source `sapper` flag is now available as authoritative `combat_sapper`, and
`Avul` is represented separately from spell immunity. Synthetic entities must
explicitly opt into the sapper class; not every flying entity is a combat sapper.
The integrated Shrine/caster content shares one authoritative classification field; runtime mask consumers still require the negative-case audit below.

### Dragonhawk Rider

Inventory: `A08V` trigger → `A08U` native bolt. The parent is a Parasite-derived
registration, **not a damaging parasite implementation inferred from its tooltip**.

Script trace:

- Trigger: enemy flying combat sapper in imported parent cast range; respect the
  native parent's nonhero/magic-immunity restrictions.
- Effect search: `solarStrikeSpell` copies `unit_getPos(Dqr)` (selected target),
  then calls `forUnitsInRange` around that point. It does **not** search around
  the caster.
- Effect eligibility: callback
  `ForGroupCallback_forUnitsInRange_RaceElvenAbilities_callback_forUnitsInRange_RaceElvenAbilities`
  requires flying, `isAliveCombatSapper`, enemy, and missing `Avul`. Native magic
  immunity can make a selected child cast fail; it is not a script search filter.
- Delivery origin: `unit_getPos(Cqr)` (caster), passed to
  `dummyCastTargetWithVision`. Homing missiles retain that initial origin and
  advance authoritative positions against the live target, not a fixed arrival
  timer to an obsolete position.
- Capacity counts attempted dummy casts, including native failure, exactly as the
  script increments its counter before issuing the order. Canonical SimId order
  supplies deterministic enumeration where native group ordering is unspecified.
- Parent mana/cooldown, child damage, native normal/hero stun, speed and search
  limits are generated from retained evidence. Child protected resources are
  checked to be free, rather than blindly suppressing a non-free ability.

### Sorceress

Inventory: `A0CV` selector marker; `A0A7` Heal trigger → `A0A8` native Healing
Wave; `A0A9` orb proc → `A0AA` native Chain Lightning.

Healing trace:

- Trigger: wounded friendly organic ground unit in the imported trigger range.
- Native trigger Heal's object-derived healing still resolves; it is separate
  from the proxy's bounce healing and must not be multiplied into later hops.
- Child originates at the caster through `dummyCastTargetFrom`; later hops are
  centered on the previous live target (last position if it disappeared).
- Secondary eligibility: wounded friendly organic unvisited units, including
  air and the real caster. Prefer least current HP, then canonical SimId, not
  nearest or greatest missing HP.
- Quarter-second cumulative deadlines use ceiling at 30 Hz (alternating gaps),
  never repeated rounded intervals. Native child resources are independently
  checked against protected zero cost/cooldown.
- Script `doAfter` recovery from the retained mechanics table is an authoritative
  order-disable interval, then ordinary attack reacquisition resumes. No client
  timer controls the recovery. Healing does not bypass external stuns.

### Wizard

Inventory: `A0CV`, `A07H`, `A06V`, `A070`, and zero-factor `A013` remain represented
as source markers; `A0AH` shared native spell resistance; `A00X` → `A00Y` Healing
Wave; `A00Z` → `A00L` native Chain Lightning; `A010` Phoenix Fire.

Healing follows the same reusable trace as Sorceress. The `A0AH` generated/native
binding and legacy human passive path are deduplicated so resistance cannot be
applied twice merely because both content import paths recognize the same effect.
Legendary/anti-negative consumers are owned by the City/Shrine work, not replaced
with tooltip-based rules here.

Phoenix Fire is an independent native periodic shooter, not an ordinary weapon
proc and not blocked by active-cast recovery or stun. Ground, air and structures
are supported. Its deterministic random target rank is independent of worker
scheduling. A target with its live native buff is excluded until expiry; travelling
missiles do not count as an already-applied buff. Impact commits native buff/DOT
and emits effect identity only for a successful native hit. Magic immune and Avul
targets do not acquire damage/buff effects. Periodic damage respects unit spell
resistance and immunity. The final one-second pulse lands at exact buff expiry,
then the target becomes eligible again that same authoritative tick. A dedicated
DOT boundary flag avoids silently changing other DOT mechanics' exclusive ends.

### Chain Lightning reconciliation

Both orb children use the existing native Chain Lightning primitive, with
source-generated damage/falloff/target/area profiles. The inherited branch already
contains the corrected quarter-second cumulative deadline in
`chain_lightning_jump_due_tick`, and reusable tests
`triggered_chain_lightning_hits_nearest_valid_jump_targets` and
`staged_chain_lightning_is_worker_count_independent` assert its staged behavior.
There is no remaining 150 ms substitute in this branch. The native insight's
nearest-unvisited rule is distinct from Healing Wave's least-HP rule.

## Authoritative and presentation boundary

`NativeAction::{HealingWave,Bolt}` is snapshot/canonical state, including complete
profiles, native target history and healing falloff, original launch identity,
current homing position/tick and live projected arrival. Native DOT final-pulse
policy and new intrinsic classifications are also canonical/snapshotted.
The integrated compatibility constants are bundle schema 5, checksum schema 18,
snapshot schema 15. Independent post-cast order recovery is canonical status,
not removable native stun. Final content/release/source publication remains open.

Presentation transport includes child ability IDs on staged healing-lightning
events, ability IDs on native projectile views, and successful native impact
casts for both units and structures. No generic model/rawcode placeholder is
encoded by the simulation. Actual child missile, caster/target/buff and lightning
art comes from the frontend worker's retained visual projection.

## Integrated contracts and remaining promotion gates

1. **Interruption and native firing — integrated:** primary and additional
   Phoenix Fire slots ignore ordinary stun, independent cast recovery, and script
   order suspension at evaluation and commitment. Both still honor the projected
   Hex ability-disable flag. An earlier same-tick Hex cancels an evaluated passive
   intent without consuming resources or sequences; a committed missile survives
   source disable. Healing Wave and the existing Warlock retreat/sleep use a
   separate order-recovery deadline. Native cleansing clears native stun but
   preserves that deadline, script callbacks and other independent controls.
   Four reusable mixed-feature recovery tests, three cleanse regressions and the
   existing retreat regression passed; all 378 integrated simulation tests,
   strict all-target simulation Clippy and client all-target checking also passed
   (`casting-recovery-{focused2,cleanse,warlock,sim,clippy,client}.log`). This is
   synthetic continuation evidence, not native-executable conformance.
2. **Mana API consolidation — integrated:** generated profiles use
   `ManaProfile::per_second` and `regeneration_at_tick`; the temporary
   `native_mana_increment` bridge is removed. Untagged legacy rates remain
   per-tick. Arbitrary-phase/snapshot regressions remain, and the generator
   rejects negative/out-of-range rates and non-free child resources.
3. **Frontend transport — integrated, rendered fidelity open:** projectile
   selection calls `wc3_visuals.projectile_for(source_rawcode, ability_rawcode)`
   with independent child identity rather than weapon fallback. Parent/impact
   visuals, Healing Wave lightning endpoints, and unit/structure native buff
   visuals still need the final asset-loading/attachment and rendered review.
4. **Sub-tick Parasite carrier corner:** the parent's retained Parasite buff has a
   sub-tick lifetime and an inherited `nfbr` death summon. Native proxy missiles
   cannot normally hit during that window, but same-native-frame unrelated death
   remains an unprobed interaction. This is not implemented as a guessed long-lived
   parasite/summon. It is an explicit remaining native-engine observation gate.
5. **AI/native frame fidelity:** current healing target selection/recovery is
   deterministic and tested; native frame-level order cadence, visibility and
   simultaneous native-event ordering still need an in-game comparison. Full
   rendered VFX fidelity cannot be signed off until the frontend merge is tested.

## Reusable behavioral verification

`simulation/native_actions_tests.rs` covers:

- target-centered search vs caster-origin launch;
- script search eligibility, attempted-cast cap, native failure and hero secondary
  duration versus primary eligibility;
- ground-only healing triggers and air/self secondary targets; organic/team/life
  negative cases; independent trigger healing and bounce falloff;
- lowest-current-HP selection and cumulative quarter-second deadlines;
- independent passive shooting during ordinary stun/recovery, but not Hex ability
  disable; primary/additional evaluation and same-tick commitment; ground/air/structure delivery;
- native buff exclusion, all DOT pulses including the expiry boundary;
- homing target movement, target disappearance and impact immunity;
- save/restore during healing, native flight, unit/structure DOT and mana remainder,
  with continued checksum equality at different worker counts;
- exact per-second mana clock across arbitrary phases and legacy compatibility.

Generator tests validate identity-only recipes, reproducibility, exact rate units,
protected free child resources and native delivery masks using synthetic values.
Catalog-wide existing tests cover imported attacks, corpse/classification data,
production/direct parity and behavior-root completeness. The synthetic typed-battle
fixture now derives its spawn width from roster length, rather than putting newly
added units outside legal collision bounds.

## Branch validation and handoff

- Existing final simulation log `/tmp/elven-caster-tests3.log`: **301 passed**,
  zero failures; exit status 0. No simulation/client source changed after that run.
- Existing final simulation Clippy log `/tmp/elven-caster-clippy2.log`: **passed**,
  exit status 0, no warnings.
- Final `cargo fmt --all -- --check` and `git diff --check`: **passed**.
- Retained-r1 tuning reproduction command above: **passed**, byte-for-byte check.
- Focused native tuning generator suite: **9 passed**.
- Full map-tool unittest discovery: **96 passed, 1 failed**. The sole failure is
  `test_repository_manifest_registers_927_and_verified_932_archive`: the existing
  release manifest references commit `1d04aaf87bf3f9bfb69550f9cdb2200ee1232860`
  with mismatched tuning/binding digests. Release metadata is coordinator-owned;
  this branch intentionally does not rewrite manifests or publish a revision.
  Full output: `/tmp/elven-caster-python-final.log`.

The caster engine work is ready for merge. This is not a claim that the Hex,
consolidated mana API, rendered frontend VFX, or native-observation gates listed
above have passed. The coordinator must close those gates on the integrated tree
before full entity fidelity sign-off. Elven race exposure remains disabled.
