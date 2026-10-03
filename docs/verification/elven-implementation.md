# Elven Union implementation status (9.27)

**Status: incomplete; do not expose as a playable race.** The requested completion
criterion is the full roster and special buildings, not a reduced Elven build menu.
The lobby and authoritative match's Human-only development catalog remain unchanged.

## Evidence and identity

- Map: Castle Fight DE Beta 9.27, retained release `r1`.
- Extraction tree: `8ea806dca331ff254995e94e6f0baf225a14bf10`.
- Native development content revision: `cf-native-dev-slice-r9`.
- Roster: `script/race-buildings.tsv`, builder `X00P`, race index 5.
- Base/attack/corpse/ability inventories: `resolved/units.tsv`,
  `protected-unit-stats.tsv`, `production-unit-attacks.tsv`,
  `production-unit-corpses.tsv`, `production-unit-abilities.tsv`.
- Repair timing and command positions: `resolved/object-fields.tsv` (including
  inherited fields), pinned by `catalog-supplement-r1.json`.
- Script callbacks and protected tuning: `script/war3map.lua`,
  `unit-spell-registrations.tsv`, `unit-spell-mechanics.tsv`,
  `runtime-system-mechanics.tsv`, `protected-ability-fields.tsv`.

## Native development fixtures added

These fixtures are available for simulation/debug verification, **not** a complete
race sign-off. Numeric stats, construction costs/timing, production, ordinary
weapons, target masks, movement, collision and corpse profiles are resolved through
the existing versioned importer, not copied from tooltip prose.

| Entity / production source | Ordinary weapon | Complete extracted ability inventory |
| --- | --- | --- |
| Archer `n022` / Archery Range `h08X` | Pierce missile; air/ground/structures; 450 range | `A0CV` Channel marker, `A08I` Bash, `A00U` 15% Evasion |
| Master Archer `n023` / Archery Tower `h08Y` | Pierce missile; air/ground/structures; 500 range | `A08J` Bash, `A03P` 25% Evasion |
| Blademaster `n006` / Hall of Honor `h00T` | Normal melee; ground/structures; 100 range | `A0FK` non-target Channel marker, `A0AL` 15% spell resistance, `A00U` 15% Evasion, `A00V` 25% chance at 2.5x Critical Strike |

Archer and Master Archer Headshot uses Bash's attack damage type (Pierce), not a
separate spell-damage packet. Their chances are 10% / 20%, bonus damage 50 / 125,
and ordinary-unit stun duration 0.5 seconds (15 ticks). The ability target mask
excludes structures even though their ordinary attacks can hit structures. Damage
and stun are delivered at projectile impact and survive an in-flight snapshot.
The separate hero duration is 0.25 seconds in the extraction; hero classification
is not currently represented by the native unit primitive and must be addressed
before enabling hero content.

All three organic fixtures retain WC3 corpse creation, death-animation delay,
expiry and raise eligibility. Neither Channel marker is treated as an active spell.
Blademaster's `A0AL` remains an identified passive ability, which is important for
the future Power Armor exclusion even though Power Plant is not playable yet.
Spell resistance uses the existing authoritative passive effect and scales spell
damage only, not ordinary attacks.

`h08X` upgrades to `h08Y` through the extracted production graph. Hall of Honor's
unimplemented `h03F` successor is not substituted with an ordinary Blademaster.
Production and direct spawning consume identical resolved unit properties.
Archer train hotkeys resolve to Q; their inherited creep profiles omit Buttonpos.
The development supplement explicitly supplies the native default (0, 0) train
slot, which is a verification-fixture UI policy, not a claimed authored override.

### Tests

`crates/sim/tests/elven_content.rs` covers:

- versioned identities, production properties and the Archery Range upgrade;
- imported Headshot success and failure using unmodified deterministic chances;
- no early damage/stun before projectile impact;
- exact stun expiry and in-flight snapshot continuation across worker counts;
- structures never receiving Headshot despite being valid ordinary targets;
- imported Blademaster spell resistance and ordinary-attack negative cases;
- no partial Elven structures leaking into the playable Human catalog.

Existing generic simulation tests cover Evasion, Critical Strike, corpse timing,
versioned ordinary weapons, production, armor and worker-count determinism.
No new persistent state or snapshot/checksum shape is introduced by these fixtures;
the content revision and canonical bundle hash change.

## Remaining full-fidelity work

Do not mark the race complete or add a lobby option until these are implemented
and audited with `unit-implementation-checklist.md`:

| Entity / source | Required mechanics and distinctions |
| --- | --- |
| Elder Blademaster `n00Y` / `h03F` | `A0AN` 25% spell resistance, `A05Y` 30%/3x Critical Strike, `A03P` Evasion, `A014` Feedback including mana commitment, summoned-unit bonus and effect damage type |
| Bloodthirster `h00U` / `h00V` | `A00W` native Faerie Fire autocast, protected 20 mana/6-second cooldown, armor reduction and native duration/target mask; marker `A0CV` |
| Dragonhawk Rider `n01Y` / `h06Y` | `A08V` registered enemy-flying trigger at 800 range, 50 mana/14-second cooldown; `solarStrikeSpell` effects must follow script control flow rather than treating it as generic target-centered AoE |
| Ballista `e005` / `h070` | Native `mline` piercing projectile delivery; preserve its line collision/continuation geometry and distinguish the disabled `aline` second weapon. Do not downgrade this to ballistic splash or guaranteed single-target delivery |
| Sorceress `h07B` / `h09X` | `A0A7` ally-ground trigger, 50 mana/8-second cooldown; caster-origin dummy `A0A8` Healing Wave (150, 2 targets, 40% reduction, 500 jump radius), 0.8-second attack reengagement; `A0A9` orb proc and linked child effect |
| Wizard `h00W` / `h00X` | `A00X` ally-ground trigger, 70 mana/12-second cooldown; caster-origin dummy `A00Y` Healing Wave (300, 6 targets, 20% reduction, 500 jump radius), 0.8-second reengagement; `A00Z` orb proc, `A010` Phoenix Fire, all legendary/protection markers and 70% spell resistance |
| City of Magic `h00Z` | `A017` random living enemy trigger, shield check, proxy `A018` Hex; disabled abilities/attack and minimum speed, expiry and reengagement (including Defender's special restoration sequence) |
| Obelisk of Light `h005` | One persistent `A000` Phoenix Fire carrier per building; owner/origin and carrier cleanup on building death/leave; positive damage triggers full native positive/negative buff cleanse and the script's persistent-ability removal list |
| Arcane Tower `h014` | `A015` multishot plus `A09A` range helper; reconcile actual native extra-target count and radius with tooltip (object data's `Maximum Number of Targets=3` is not sufficient evidence for the tooltip's four additional targets) |
| Golden Shrine of Justice `h059` | Team-stacked 20% chance per shrine capped at 40%; excludes summoned, legendary and already revived units; authoritative 2-second delayed replacement with same-death-generation validation, original owner/position/template, one-time revival flag and corpse-removal behavior |

Solar Strike's extracted callback selects living hostile **flying combat sappers**
lacking `Avul`. The helper enumerates at **400 units around the caster**, caps at
four selected units, and invokes `A08U` via `dummyCastTargetWithVision`; the selected
trigger unit must not be assumed to be the center of the enumeration. The precise
proxy origin, effect-native target restrictions, resource fields and ordering still
need the full per-spell audit.

Healing Wave trigger eligibility and bounce eligibility differ: the registered
trigger is ally-ground, whereas the linked native waves permit organic allied air
and ground units (including self). Tests must distinguish those sets, enforce
non-repeated bounce selection and preserve any delayed native jump sequence.

Before promotion, wire all authored Elven build roots/shared structures through
race-aware match setup and lobby selection, and verify the imported models/icons,
archery building attachment variants and active-effect presentation origins. Do
not reuse `playable_human_direct_building_kinds()` for an Elven participant.
