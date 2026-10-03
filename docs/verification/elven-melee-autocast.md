# Elven melee/autocast fixture audit

Map: Castle Fight DE Beta 9.27, retained release r1, extraction tree
`8ea806dca331ff254995e94e6f0baf225a14bf10`; native content revision r12.
These are simulation/debug fixtures, **not race promotion or a Warcraft-executable
conformance sign-off**. Apply the [entity checklist](unit-implementation-checklist.md)
before lifting any remaining fidelity gates.

## Elder Blademaster / Hall of the Eldest

Identities: `n00Y` (base `nbel`), production `h03F` (base `hbla`), reached by the
extracted `h00T` upgrade. Registration supplies identity only. Stats, ordinary
Normal melee attack, ground/structure mask, production/build/repair timing,
regeneration, armor, corpse eligibility and command-card metadata come from the
retained catalog; no tooltip stats are maintained in this audit.

Complete object-authored inventory:

- `A05Y`: native Critical Strike, projected separately from Feedback.
- `A014`: native tower-derived Feedback (`Afbt`). No script registers a combined
  chance-gated Magebane action. Its mana combustion is therefore evaluated on
  landed directed hits, independently of the Critical Strike roll, using live
  target mana at impact. The summoned-target term is retained as well.
- `A03P`: shared native Evasion.
- `A0AN`: native spell resistance; retained source identity also represents the
  map's Power Armor exclusion marker.

The Magebane tooltip describes a combined chance-bearing effect; the actual
inventory has independent Critical Strike and Feedback abilities. Do not apply
its Critical Strike probability or multiplier to Feedback. The current primitive
rejects unequal hero/ordinary Feedback parameters instead of flattening them.

Feedback can coexist with ordinary armor mitigation but uses a separate spell
payload; misses, spell immunity, structures, no mana, drained mana between launch
and impact, and shared live mana between simultaneous hits have synthetic tests.
A killing ordinary strike has no further effect on a dead body. Revived bodies
use their original mana/ability definitions, not dead-source runtime state.

## Bloodthirster / Bloodelf War Academy

Identities: `h00U` (base `hspt`), production `h00V` (base `hbla`). The ordinary
Magic weapon is native `mbounce`, not splash. Its maximum target count, retained
bounce damage and bounce search radius are projected from `utc1`, `udl1`, and
`ua1f` into the release-pinned catalog supplement. Existing directed bounce
runtime supplies successive travel/impact state and the authored ground/air/
structure target mask. Corpse and other base metadata use the retained catalog.

Complete object-authored inventory:

- `A00W`: native creep Faerie Fire (`ACff`). Mana/cooldown use the protected
  overlay, not placeholder static fields. Both ordinary and hero durations are
  projected; no native proxy or scripted recovery handler is registered.
- `A0CV`: retained Channel/assassin-order marker; no invented active spell.

Autocast eligibility is nearest unmarked hostile combat unit in cast range, with
stable identity tie-breaking; non-attacking, idle, allied, mechanical and
spell-immune candidates are excluded. Combat reads the previous targeting/hit
state. Eligibility is checked again at commitment, before mana/cooldown spending.
The debuff reduces numeric armor and retains the revealing team through expiry.
Existing buff identity prevents repeated spending or stacking by another caster.
Cast events and the modifier identity feed the existing ability/status visuals.

## Evidence and reusable verification

Retained evidence: `resolved/units.tsv`, `protected-unit-stats.tsv`,
`production-unit-attacks.tsv`, `production-unit-corpses.tsv`,
`production-unit-abilities.tsv`, `object-fields.tsv`,
`protected-ability-fields.tsv`, and `script/building-upgrades.tsv`.
Neither unit has a registered scripted cast in `script/unit-spell-registrations.tsv`.

Engine semantics references, **not alternate sources of map tuning**:

- [Blizzard Spell Breaker](https://classic.battle.net/war3/human/units/spellbreaker.shtml):
  Feedback burns available mana and cannot affect spell-immune units.
- [Blizzard Spell Basics](https://classic.battle.net/war3/basics/spellbasics.shtml):
  magical spell targeting excludes mechanical units; Faerie Fire is dispellable.
- [Native ability observations](https://www.hiveworkshop.com/threads/alternative-uses-of-abilities.354568/):
  Faerie Fire combat targeting favors nearest battling enemies and excludes
  non-attacking targets.

Verification is in `simulation/native_target_effects.rs`, the generic
production/restoration suites, catalog-wide content checks, and complete generated
artifact comparisons. Projection tests separately exercise protected overrides,
hero durations, and rejection of unsupported Feedback class parameters. These
avoid repeating this map's numerical values in tests.

## Validation of this fixture slice

- Workspace tests: 598 passed, one ignored (291 simulation tests); no server
  timing failure in this serial run.
- Workspace all-target Clippy with warnings denied, formatting and diff checks:
  passed.
- Twelve projection tests and complete supplement/source-manifest/native-tuning
  regeneration comparisons: passed.
- Synthetic 1,000-unit ability/multi-ability release benchmarks preserve checksums
  across one/four workers (`7ffbac1794ef453f` / `eacdb5f98b1fad7c`). Total ms/tick
  were 8.096/5.150 (single) and 10.640/6.396 (multi); these are exploratory samples,
  not a performance threshold or a native-fidelity test.

## Fidelity gates still open

- Warcraft-executable probes are still needed for Feedback's exact damage-class/
  resistance interactions and native Faerie Fire AI cadence/order interruptions.
  The current Feedback payload uses the shared spell-damage path; synthetic tests
  are not an independent oracle for that native classification.
- Bounce target selection remains the engine's documented provisional deterministic
  rule, not a measured reproduction of Warcraft group iteration. Do not claim
  exact native bounce ordering from these imported parameters alone.
- Reveal state is authoritative, but there is no fog-of-war/invisibility system
  yet; its visibility consequences remain dormant until that system consumes it.
- General dispel/cleanse and hostile immunity interaction coverage are not complete.
  The new immunity classification is enforced by these primitives; it does not
  establish every legacy mechanic's immunity semantics.
- Source Channel/Power Armor/assassin markers remain retained but corresponding
  hostile scripted systems are not fully implemented.

Elven stays unselectable until the entire race and these interactions are reconciled.
