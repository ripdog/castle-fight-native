# Unit / Building Implementation Fidelity Checklist

Use this checklist for each Warcraft III-derived unit or building before declaring its gameplay implementation complete. The goal is compatibility with the retained map evidence, not reconstruction from tooltips or from the apparent intent of an ability.

For Castle Fight content, always work against an exact registered map/content revision. Treat resolved object data, map script behavior, and presentation data as separate evidence layers.

## 1. Identity and source revision

- [ ] Record the map version and retained content revision being implemented.
- [ ] Record the unit/building rawcode and production source.
- [ ] Confirm the resolved object inherits from the expected Warcraft base object.
- [ ] Identify the relevant rows/files under `docs/original_map/extracted/resolved/` and any script functions in `docs/original_map/extracted/script/`.
- [ ] Do not copy an unqualified tuning literal into runtime code when it belongs in version-scoped content.

## 2. Base unit/building data

- [ ] Verify life, mana, regeneration, armor amount/type, movement, collision, acquisition range, bounty/cost, build/production time, and relevant classification flags.
- [ ] Verify every ordinary attack independently: target mask, attack type, damage, cooldown, range, projectile behavior, splash/bounce fields, and structure-vs-unit variants.
- [ ] Verify corpse behavior: corpse creation, expiry, raise eligibility, mechanical/organic classification, and any legendary/special exclusions.
- [ ] Verify hidden or apparently marker-only abilities and classifications. Keep them represented even if the engine does not yet implement the hostile mechanic they protect against.

## 3. Complete ability inventory

For every visible, passive, hidden, autocast, proc, aura, upgrade-granted, proxy, dummy, or marker ability:

- [ ] Record the rawcode and whether the behavior is object-data-native, script-native, or a composition of both.
- [ ] Verify mana cost, cooldown, cast range, area, duration, levels, order ID, and target mask from resolved data.
- [ ] Trace any ability added, removed, enabled, disabled, or level-changed by script.
- [ ] Trace every dummy/proxy/carrier ability transitively until the actual gameplay effect is known.
- [ ] Treat tooltips as descriptive evidence only. When tooltip text, object data, and script runtime disagree, implement the runtime behavior and document the discrepancy.

## 4. Scripted spell trace

For each scripted cast, write down the following explicitly before implementing it:

| Question | Required answer |
| --- | --- |
| What causes the AI/script to consider the spell? | Trigger condition and candidate class |
| Which units can be selected as the cast target? | **Trigger eligibility** |
| How close must the caster get? | **Approach/cast geometry** |
| Where is the real effect spawned/cast? | **Effect origin**: caster, selected target, point, projectile impact, dummy position, etc. |
| Which units can the resulting effect affect? | **Effect eligibility**, independently of trigger eligibility |
| What resources are committed? | Mana, charges, cooldowns, secondary ability resources |
| When do effects happen? | Immediate and delayed callbacks, periodic intervals, ordering |
| What persists afterward? | Buffs, permanent stat changes, cooldown state, AI state, spawned units |
| What can make a secondary effect fail? | Missing target/corpse, insufficient secondary mana, cooldown, immunity, classification |
| What does the unit do around the cast? | Stop, approach, retreat, sleep, resume aura/autocast, reacquire |

Do not collapse these columns into one generic target predicate or one AoE position unless the evidence proves they are identical.

## 5. Timing, secondary abilities, and AI state

- [ ] Reproduce script delays literally, including seemingly insignificant sub-second gaps.
- [ ] If one spell invokes another ability, verify whether the invoked ability has its own mana cost, cooldown, target restrictions, and failure path.
- [ ] Verify whether the primary cast succeeds even when a delayed/secondary effect later fails.
- [ ] Verify autocast enable/disable timing and any post-cast retreat, sleep, stop, or reacquisition behavior.
- [ ] Verify aura/passive suspension and restoration timing separately from the visible spell cooldown.
- [ ] Preserve odd timings when they are script-authored rather than rounding them to cleaner values.

## 6. Authoritative state

Any gameplay consequence that survives beyond the current resolution step must be represented authoritatively.

- [ ] Pending delayed callbacks are authoritative state, not presentation timers.
- [ ] Secondary cooldowns, charges, one-time bonuses, spawned-object ownership, toggles, and scripted AI phases are authoritative where applicable.
- [ ] New persistent fields are included in canonical checksums.
- [ ] New persistent fields are included in save/snapshot/rejoin state.
- [ ] Snapshot/checksum schema versions are updated when their serialized authoritative shape changes.
- [ ] Restore tests cover an in-progress delayed/timed effect when practical.

## 7. Presentation boundary

- [ ] Gameplay does not depend on animation, particles, wall-clock timing, or client-only events.
- [ ] Cast/effect presentation uses the same semantic origin as the authoritative effect when that matters visually.
- [ ] Delayed or secondary abilities that need distinct WC3 visuals emit/retain enough presentation information to reproduce them.
- [ ] Visual lifetime is checked against gameplay timing for resurrection, channels, projectiles, spawned movers, and other long effects.
- [ ] Team color, attachment points, model variants, and effect art are presentation concerns unless the extracted data also gives them gameplay meaning.

## 8. Behavioral tests

Prefer end-to-end behavioral assertions over tests that merely verify imported constants.

For every nontrivial ability, add tests covering the relevant items below:

- [ ] A normal successful cast/proc produces the extracted result.
- [ ] Trigger range/cast range boundary behavior is correct.
- [ ] Effect center is proven with positions that distinguish caster-centered from target-centered behavior.
- [ ] Trigger eligibility and effect eligibility are tested separately when they differ.
- [ ] Air/ground, organic/mechanical, unit/building, ally/enemy, living/corpse, and special classifications have negative cases where relevant.
- [ ] Mana and cooldown commitment are exact, including secondary ability costs.
- [ ] Delayed effects occur on the correct tick and not earlier.
- [ ] Failed secondary effects do not consume resources unless the source script does.
- [ ] Permanent/one-time bonuses cannot accidentally stack.
- [ ] Proc chance/deterministic RNG behavior is covered when applicable.
- [ ] AI approach/retreat/sleep/autocast behavior is covered for scripted casters.
- [ ] Corpse selection/consumption and resurrection exclusions are covered where applicable.
- [ ] Save/restore during pending state is covered when the mechanic introduces new persistent state.

When an extracted behavior is surprising, make the regression test demonstrate the surprising distinction rather than merely asserting the final numeric value.

## 9. Per-unit sign-off record

Before declaring the unit/building complete, leave a short audit note in the implementation change or relevant specification containing:

```text
Entity:
Map/content revision:
Rawcode:
Production source:

Ordinary attacks:
Passives/procs/auras:
Active/autocast spells:
Hidden/marker abilities:

Scripted spell traces:
- trigger eligibility:
- approach/cast geometry:
- effect origin:
- effect eligibility:
- mana/cooldowns:
- delayed/periodic sequence:
- AI behavior:

Corpse/classification behavior:
Known tooltip/object/script discrepancies:
Unsupported engine interactions intentionally left dormant:

Tests proving the important distinctions:
```

A unit is not fully audited merely because all of its visible ability icons have corresponding runtime effects. Completion means its attacks, classifications, hidden abilities, scripted control flow, target semantics, timing, persistent state, and relevant presentation hooks have all been reconciled against the retained evidence.
