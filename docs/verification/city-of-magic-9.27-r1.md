# City of Magic: retained Hex and negative-building-spell control

This audit covers `h00Z` (City of Magic, base `hbla`) and its `A017` registration / `A018` native Hex proxy in Castle Fight 9.27, retained release r1. This branch does not promote the race; final race exposure requires the integrated roster and presentation audit.

## Source identity and reproducibility

The projection reads the release registry's retained extraction tree `8ea806dca331ff254995e94e6f0baf225a14bf10`, not the mutable extraction alias. `building-mechanics-r1.json` records that tree, source revision, source-file digests, selected resolved fields, and exact script function traces with byte offsets.

```sh
python3 tools/wc3-map/build_building_mechanics.py --map-version 9.27 --revision r1
python3 tools/wc3-map/build_building_mechanics.py --map-version 9.27 --revision r1 --check
```

Relevant retained evidence:

- `resolved/units.tsv`, `protected-unit-stats.tsv`, and the existing building catalog: inherited identity, life, armor, cost, construction/repair timing and classification.
- `resolved/building-spell-mechanics.tsv`: parent registration, actual native child, target selector, script recovery sequence and ordinary/hero duration distinction.
- `resolved/protected-ability-fields.tsv`: runtime mana/cooldown overrides, rather than conspicuous editor placeholders.
- `resolved/object-fields.tsv`: parent/native child, shield, critter form, building mana and attachment fields.
- `resolved/runtime-system-mechanics.tsv`: targeted-negative-effect shield precedence and Overheat behavior.
- `script/war3map.lua`: `hexSpell`, `randomAliveEnemy`, `hasShield`, `checkForShield`, combat-sapper helpers, dummy helpers, and all three nested `ReengageRuntime` callbacks. The projection retains these exact bodies; minified-script line numbers are not a useful reference.

## Complete inventory and ordinary building behavior

The source building has no ordinary damaging attack. `A017` is the building-spell registration; it is not a substitute damaging Parasite inferred from its inherited editor base. The actual effect is `A018`, and both source identities are represented in native-effect requirements and stable content behavior declarations. Existing versioned import supplies the building's base data, economy and command-card metadata.

The active profile uses one shared mana state. Per-second regeneration remains exact through the tagged `ManaRegeneration` API and cumulative authoritative tick clock; dividing regeneration into rounded per-tick constants is not permitted. Protected source values determine commitment and cadence.

## Script trace

| Concern | Implemented interpretation |
| --- | --- |
| Trigger | Building spell scheduler with live mana/cooldown checks. |
| Selection | Global random living hostile combat unit, with retained selector exclusions; no invented ground-only or organic-only condition. |
| Shield handling | Resources are committed before interception. Active shield precedes the special Overheat roll, which precedes the anti-negative marker. A failed Overheat roll does not fall through to that marker. |
| Effect origin | Dummy created at the selected target's position, with temporary vision through the retained helper. |
| Effect eligibility | Native Hex failure is separate from script selection. Mechanical and flying units remain selectable; spell immunity can make the native child fail. |
| Native body | Separate ground/air critter forms, imported movement/collision/armor, ordinary/hero lifetime, and disabled attacks, abilities and passive inventory. Logical source identity and resurrection baseline remain original. |
| Immediate control | Successful morph suspends current orders and toggled Defend. Native failure is not an implicit undefend order. |
| Reengagement | The first callback is independent of native buff lifetime and still exists after a native child failure. Defender then receives the separately authored Defend and attack-resume callbacks. |
| Early dispel | Removes the native morph without cancelling unrelated script callbacks. |
| Shield side effects | Interception consumes one level; the stronger level heals before leaving the weaker one. Shield expiry and explicit dispel are separate from interception. |

The native child and script control clocks are separate authoritative state. Restoration must not derive callbacks from remaining Hex duration or restart a fresh timer. Multiple callbacks retain their own deadlines and stable canonical order.

## Hidden markers and permanent effects

Selector/anti-negative markers are resolved from retained inventories, not inferred from unit names, armor or legendary labels. Synthetic setup APIs can explicitly configure equivalent flags. The shared shield helper preserves the source's special Overheat progression, attack-speed grant and death-splash distinction. That reusable infrastructure does not register an otherwise unfinished Goblin roster as playable.

Native Hex suppresses projected gameplay properties without overwriting immutable entity baselines. After expiry or dispel, ordinary armor, collision, movement and passive definitions return; original content identity never becomes a critter definition. A dedicated cached control component is rebuilt from canonical target state, not serialized as a second authoritative value.

## Authoritative state and presentation

Canonical entity tag 12 stores target identity, version, shield level/lifetime, selector and anti-negative markers, Overheat level, Hex form/expiry, Defend/order flags and the full pending callback list. Wire restoration reconstructs control caches before continuation. Cross-worker schema publication is owned by final integration.

The client receives the imported critter rawcode and native lifetime, native transform/restore events, and shield/Overheat events. Model selection uses the native form only for the visual body; command/ownership identity remains original. Shader/attachment art is projected from the same objects and literal script model paths. Gameplay is independent of animation and presentation timers.

## Verification

Reusable tests cover:

- shield precedence, resource spending, stronger-level healing and non-stacking transitions;
- selector markers versus native immunity, air/mechanical targets, and ally exclusions;
- native body projection and original-baseline restoration;
- early dispel versus continuing Defender callbacks;
- cancellation of already-evaluated primary/additional intents after a committed Hex;
- exact mana cadence and fractional-clock wire continuation;
- pending Hex/shield/callback continuation across worker counts;
- retained source profile/dependency closure, independent native/control deadlines, and special shield death effects.

Source-profile fixtures use legal navigation bounds for imported collision footprints. Tiny synthetic collision bodies must also use a navigation-resolution lower bound for dense reservation buckets; otherwise a test can allocate billions of buckets. The integrated engine has a separate focused regression for that memory bound.

## Final integration checks

- Consolidate the exact mana API with caster profiles and remove their temporary bridge; retain arbitrary-phase clock tests.
- Apply the caster branch's new intrinsic `combat_sapper` / `invulnerable` classifications to global Hex selection. The source helper explicitly requires a living non-structure sapper and excludes `Avul`; ordinary synthetic combat fixtures must opt into that class after integration.
- Independent native Phoenix Fire must ignore ordinary recovery/stun but respect Hex's ability disable at evaluation and commitment.
- Obelisk cleansing must remove native Hex and active shield state through these control APIs while preserving separate script callbacks and order-recovery timers.
- Reconcile the client visual catalogs and authoritative schemas with the other completed branches, then run mixed-race lifecycle/wire/presentation checks.
- Passing these native-contract tests is not a claim that a Warcraft executable comparison or rendered screenshot inspection was performed.
