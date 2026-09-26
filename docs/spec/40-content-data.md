# Gameplay Content and Data Definitions

Status: **normative data principles, provisional schema**

## 1. Purpose

This document defines how units, buildings, attacks, factions, maps, upgrades, and other gameplay content are represented without hard-coding every rule into simulation systems.

Content is authoritative input to deterministic simulation and therefore must be versioned, validated, and hashed canonically.

## 2. Content vs runtime state

Content definitions describe immutable rules/templates.

Runtime ECS state stores mutable instance state and compact references to content definitions.

Example:

```text
UnitDefinition "footman"
  movement speed
  max health
  attack definitions
  collision radius
  target rules

Unit entity #18422
  UnitTypeId = footman
  current health = 317
  position = (...)
  target = SimId(...)
  cooldown = ...
```

Static definitions SHOULD NOT be duplicated into every entity unless profiling justifies a hot cached subset.

## 3. Stable content identifiers

Gameplay content MUST use stable explicit IDs.

Human-readable names/paths may be used for source files, but runtime/network/replay representations should use validated stable identifiers or canonical hashes.

IDs MUST NOT depend on filesystem enumeration order or hash-map insertion order.

Examples:

```rust
pub struct UnitTypeId(pub u32);
pub struct BuildingTypeId(pub u32);
pub struct AttackId(pub u32);
pub struct UpgradeId(pub u32);
pub struct FactionId(pub u32);
pub struct MapId(pub u32);
```

Exact widths/types are provisional.

## 4. Canonical content bundle

Before a match starts, source content is loaded and validated into a canonical immutable bundle.

The simulation consumes only this validated representation.

The bundle has a canonical hash used for:

- multiplayer compatibility;
- replay compatibility;
- snapshot validation;
- server/client mismatch detection;
- diagnostics.

Cosmetic-only assets SHOULD be separable from gameplay content so harmless visual differences do not necessarily alter gameplay compatibility. Versioned presentation bindings therefore live alongside, but outside the authoritative gameplay hash: UI art is addressed by stable semantic keys such as object rawcode plus icon role, stock command identity, resource identity, or cursor theme/atlas identity, and a generated asset manifest resolves those keys to converted files. Gameplay/client callsites MUST NOT embed converted PNG paths. Selecting a different supported Castle Fight version selects that version's presentation bindings, while re-converting identical authored art does not by itself change replay or multiplayer gameplay compatibility.

### 4.1 User-selected map versions and retained history

Match setup MUST allow the user to select an exact registered Castle Fight `(map version, release revision)` from the releases known to the installed simulation/content. Historical versions are playable content when their required runtime bundle is supported, not merely migration inputs for the newest version. The selected release determines the map geometry, stats, economy, production, abilities, items, and other authoritative rules used throughout that match.

Extracted map data and validated content MUST retain independently addressable snapshots for each supported map version. Extracting a newer map MUST NOT overwrite the data needed to play an older version. Each snapshot MUST record its source map identity/digest, relevant Warcraft base-data version, extraction schema/tool revision, and content revision. Correcting an extraction for an existing map version produces a new content revision/hash; existing replay/snapshot identities MUST NOT silently resolve to the corrected data.

Storage MAY deduplicate unchanged data or use generated Rust tables, provided each version resolves to a complete, reproducible content snapshot. A single mutable extraction directory or an implicit "latest" lookup is insufficient as the runtime source of historical match content.

The repository registry for retained source/extraction revisions is `docs/original_map/releases.json`. A registered release key is the exact pair `(map_version, revision)`; callers MUST NOT silently substitute another revision. Runtime bundle lookup likewise resolves the release's exact content revision rather than a map-version-only "current" bundle. A retained extraction may be stored by immutable Git tree identity plus a working-tree alias, allowing historical revisions to remain addressable without duplicating very large generated trees on disk. Any runtime data still compiled from such a working alias MUST validate the consumed bytes against immutable retained-tree/revision metadata before match creation; checkout-only CRLF/LF differences are not content changes. `archived` means source evidence is retained but does not imply executable native behavior coverage. `supported-development-subset` is explicitly restricted content for development and MUST NOT be presented as a complete historical ruleset.

Map version, content revision/hash, and simulation compatibility version are distinct identities. A map-version label alone does not establish multiplayer or replay compatibility. The canonical bundle MUST include the resolved native behavior bindings described in section 14.1 as well as numeric data, so changing a selected implementation changes compatibility identity even when its tuning is unchanged.

## 5. Source format

Source format is open.

Candidates include:

- RON;
- TOML;
- JSON/JSON5;
- custom editor-generated format.

Selection criteria:

- readable diffs;
- good Rust tooling/serde support;
- strict schema validation;
- convenient inheritance/composition if required;
- explicit numeric representation;
- modding potential.

Runtime correctness must not depend on parser map ordering.

## 6. Numeric content values

Authors may want ergonomic decimal values, but authoritative runtime representation uses deterministic integer/fixed-point quantities.

Content loading MUST convert/quantize values deterministically and reject out-of-range/ambiguous data.

For example:

```text
source: speed = 2.75 tiles/sec
validated runtime: exact fixed-point/tick movement value
```

The conversion algorithm is part of content/simulation semantics.

## 7. Unit definitions

A unit definition may include:

```text
identity/name/localization key
presentation asset references
max health
movement profile
collision/separation radius
unit classifications/tags
attack list
armor/resistance profile
target rule set
production-related metadata
bounty/reward if applicable
abilities/passives
status immunities
```

`collision/separation radius` is imported gameplay geometry and MUST remain distinct from presentation mesh size or selection scale. Where the original content exposes this value reliably, conversion SHOULD preserve it as an explicit authoritative radius rather than collapsing all units to one simulation-wide spacing constant.

The simulation should consume compact validated IDs/structures rather than dynamically interpret arbitrary scripts in hot loops.

## 8. Attack definitions

Every ordinary attack definition MUST declare an authoritative delivery kind rather than deriving behavior from projectile art.

Initial required kinds:

```text
Melee
RangedGuaranteedHit
RangedBallistic
Bounce
```

An attack definition may include:

```text
damage / damage distribution rule
damage type
cooldown
windup / backswing if relevant
attack range
acquisition/retention modifiers
target filters
delivery kind
projectile speed / travel-time rule where applicable
ballistic impact-zone radius/effect where applicable
bounce count/range/repeat policy/damage scaling where applicable
splash/area behavior
random proc definitions
on-hit/on-impact effects
```

`RangedGuaranteedHit` retains its intended target and cannot become missable merely because the target moves after launch. `RangedBallistic` captures a fixed impact position and may miss the original target or hit other eligible units present in the impact zone. `Bounce` guarantees its first hit and selects later bounce targets according to explicit deterministic-random rules.

All values that influence gameplay are authoritative content. In particular, imported ordinary attacks MUST carry their extracted Castle Fight attack/damage type, while every damageable unit/building MUST carry its extracted defense/armor type and base armor value. The native content layer currently centralizes these fields for the imported Footman, Ranger, Catapult, Ice Troll Shadow Priest, Gryphon Rider, Watch Tower, Poof Tower, and their production buildings. Type-versus-type multipliers come from the committed extracted `war3mapMisc.txt`; object classifications and base armor values come from resolved unit/building object data and protected runtime-stat extraction where required.

Targeting policy should reference explicit rule sets rather than deriving behavior from presentation asset type.

## 9. Building definitions

Buildings are not limited to production. One building may combine several deterministic roles.

A building definition may include:

```text
footprint
placement constraints
cost
health/armor
production definition
attack definitions if attack building
mana pool / mana regeneration if spellcasting
ability definitions/references
activation mode for abilities (automatic/passive/player-targeted)
legendary classification/limits if applicable
upgrade relationships
sell behavior
spawn location/offset policy
build/construction time if present
```

The footprint is authoritative and MUST NOT be derived from the visual mesh bounds.

Legendary point cost is versioned building economy data. A point is reserved when an order or upgrade is purchased, transferred when an upgrade replaces its precursor, and released when that order is canceled or the building is removed. Affordability MUST include the point cap as well as gold and lumber.

This is critical for deterministic placement/navigation/caging.

Spellcasting and legendary building behavior is specified in `16-abilities-spellcasting.md`.

## 10. Production definitions

Production content must define deterministic spawn timing and produced-unit selection.

Potential fields:

```text
unit type or deterministic selection table
spawn interval in ticks
initial delay
spawn count
spawn offset/pattern
upgrade modifiers
random-selection purpose/key rules if randomized
```

Production timing uses simulation ticks only.

## 11. Factions/races

Faction definitions may organize:

- available buildings;
- tech/upgrades;
- random pools;
- UI grouping;
- starting state;
- faction-specific rules.

The core engine SHOULD avoid special-casing factions by Rust type where data/effect composition suffices.

Unique mechanics may still require dedicated deterministic systems.

## 12. Upgrades

Upgrades should express deterministic modifications to validated gameplay values. Building-to-building production upgrades additionally carry an explicit versioned precursor/successor graph recovered from map content. For 9.27 the importer-facing `production-buildings.tsv` `upgrade_from`/`upgrade_to` fields are the source of truth, including branching successor sets; simulation and UI code must consume version-aware content lookups rather than infer edges from rawcode names or race-specific conditionals.

A builder's direct-placement catalog contains building roots, not upgrade descendants. When both ends of an extracted edge are implemented, the descendant is exposed from the selected precursor production building instead. During incremental native-content development, an implemented building whose extracted precursor has not yet been implemented may remain exposed as a temporary verification root; this is a content-coverage accommodation, not a rewritten map relationship.

The engine needs an explicit policy for modification order to avoid data-order-dependent outcomes.

Possible approach:

```text
base definition
+ additive modifiers sorted by stable modifier ID
+ multiplicative fixed-point modifiers in defined stage/order
+ clamps/overrides in defined stage
```

Exact semantics are open and should be kept simple initially.

## 13. Tags and categories

Targeting/effects will likely need stable gameplay categories such as:

- ground;
- air;
- building;
- mechanical;
- summoned;
- hero/boss;
- organic/biological;
- corpse-producing;
- magic immune;
- etc.

These should be explicit stable bitsets/IDs, not string comparisons in hot loops. Corpse production SHOULD be authored explicitly in the unit definition (for example by an optional corpse definition/reference) rather than inferred only from a presentation model; biological units are expected to produce corpses unless original/imported content says otherwise.

The actual set depends on the intended Castle Fight rules/content.

## 14. Effect and ability definitions

Reusable deterministic effects may represent:

- direct damage/healing;
- apply/remove status;
- area damage;
- spawn unit;
- modify resource or mana;
- knockback if supported;
- timed aura/status;
- map-wide aura/modifier;
- corpse selection/consumption;
- corpse-derived effects such as explosion damage or spawning raised units;
- stun/disable;
- chained/bounce attack;
- spawn attack/ability projectile.

Ability definitions compose target policy, activation mode, cost/cooldown/charges, and one or more effects. Automatic building abilities and player-targeted legendary abilities use the same validated vocabulary rather than separate scripting paths.

The effect system SHOULD be constrained and explicitly ordered rather than becoming an unconstrained scripting VM prematurely.

### 14.1 Versioned native-effect implementations

Imported Warcraft/Castle Fight behavior MUST distinguish **behavior identity** from **per-map-version tuning**. A native effect implementation has a stable implementation ID and an inclusive map-version range for which its semantics have been verified. Numeric fields such as chance, damage, radius, duration, mana cost, cooldown, or armor bonus are loaded from the selected map version's extracted/validated tuning snapshot rather than copied into the native implementation.

If a later map changes only numeric tuning, that map version SHOULD reuse the same native implementation while supplying different tuning data. If the original map rewrites the mechanic's semantics, the engine MUST add a distinct native implementation ID and assign it a non-overlapping validity range. The runtime MUST NOT silently select the nearest implementation when the requested map version has a gap; missing behavior coverage is a content error.

Bindings associate a stable source kind/key with an implementation ID and verified map-version applicability. One implementation MAY serve multiple versions or multiple disjoint validity ranges; retaining an older implementation MUST allow older maps to continue selecting it after a newer implementation is introduced. Numeric similarity, unchanged rawcodes, or a later version number alone MUST NOT extend verified applicability. A behavior-changing edit to an existing implementation requires a new implementation/compatibility identity rather than silently changing historical behavior under the old identity.

Before match creation, content loading MUST automatically resolve a deterministic set of native implementations for the selected map version:

1. Load that version's content snapshot and enumerate every behavior required by the configured mode and reachable content, including upgrades, summons, proc-triggered child effects, and other indirect references.
2. For each required source kind/key, select exactly one binding whose verified applicability includes the selected map version. Multiple matching bindings are an ambiguity error, not a priority or registration-order decision.
3. Verify that the selected implementation exists in the installed simulation, accepts the version's tuning schema/parameters, and has compatible implementations/data for all required dependencies. A matching version range alone is insufficient.
4. Reject configuration before the match starts if required coverage is missing, ambiguous, or incompatible. Report the source key, dependency, and selected map version responsible. A required ability MUST NOT silently disappear or fall back to another map's implementation/tuning. An explicitly verified no-runtime marker binding is valid coverage.
5. Freeze the resolved source-to-implementation mapping and tuning into the immutable canonical content bundle in stable order. Runtime entities consume that resolved bundle rather than repeating version selection in gameplay phases.

A version with archived extraction but incomplete required native behavior is not yet a supported playable version for that match configuration. Development fixtures MAY explicitly select a restricted content subset, but MUST NOT present silently reduced content as the complete historical ruleset.

Extraction ingestion and executable-content promotion are deliberately separate. A retained version snapshot MAY contain units, buildings, abilities, items, upgrade edges, or fields that the current simulation cannot execute yet; the content index MUST preserve such trustworthy evidence instead of rejecting the whole snapshot merely because an unsupported row is present. Promotion of an object into a playable bundle is stricter: it requires a stable native content ID, every authoritative field required by the native primitive, and complete reachable behavior coverage through the resolver. Therefore expanding or correcting an existing retained extraction can feed additional numeric/source data into the versioned catalog without changing ordinary simulation callsites, while newly discovered mechanics remain non-playable until explicitly covered. A corrected retained extraction receives a new revision/content identity rather than silently changing an older saved identity.

Each runtime catalog revision MUST pin the retained source evidence from which it was built. The current 9.27 development catalog records the exact retained extraction tree plus a deterministic aggregate hash of every extraction file it consumes; catalog initialization rejects evidence drift under that revision. This allows the repository's working extraction alias to continue serving tooling without letting later extractor output silently redefine an already named runtime content revision.

Production definitions and every produced unit/effect MUST resolve against the match's selected version rather than falling back independently to a default version. The selected version and resolved bundle identity MUST be recorded in multiplayer handshakes, snapshots, and replays. Loading or rejoining MUST require that exact compatible bundle, not rerun a "best available" selection against a newer registry.

Native-effect coverage is derived from two sources: extracted effect/ability inventories define what exists, while the native binding registry defines which stable source keys have implementations for a requested version. Unbound inventory entries are explicitly **unimplemented** until either a native behavior binding is added or later compatibility work classifies the extracted row as requiring no native runtime behavior. Coverage tooling MUST make gaps queryable per map version.

The first executable content slice is the currently exposed Castle Fight 9.27 production-unit roster. Footman has no extra extracted ability behavior. Its Stronghold (`h039`) upgrade produces Defender (`h03A`), which binds the same 15% Evasion (`A00U`) plus Defend (`A03G`): Castle Fight's extracted script activates Defend after 0.7 seconds and maintains that state, while the versioned ability tuning supplies 40% ranged damage taken, 50% spell damage taken, and 50% Pierce deflection with zero damage on a successfully deflected Pierce hit. Ranger binds 15% Evasion (`A00U`) and explicitly classifies its Channel marker (`A0CV`) and zero-damage Barrage (`A03N`) as requiring no runtime effect. Catapult binds Burning Oil (`A02J`). Ice Troll Shadow Priest binds its 10% orb proc (`A049`) to Entangling Roots (`A03W`) and Frost Armor autocast (`A03Z`), including exact 1.5 mana/sec regeneration via deterministic fixed-point state. Gryphon Rider binds Bash (`A05K`) and its 10% orb proc (`A01B`) to Chain Lightning (`A05X`). Imported per-unit hit-point regeneration is also authoritative fixed-point content rather than presentation metadata; the current slice includes Footman 0.25 HP/s, Defender 1.75, Ranger/Catapult 0.5, and Ice Troll/Gryphon Rider 1.0. Numeric chance/damage/radius/duration/mana/cooldown fields remain in the 9.27 tuning snapshot while the behavior IDs/ranges remain in the binding registry. Guaranteed-hit/reflected projectiles carry the damage type and any applicable payload/state through impact, and persistent/timed native-effect state participates in canonical checksums.

The playable Human builder catalog for the 9.27 development slice exposes Q Barracks (`h000`), W Sniper Nest (`h003`), E Weapon Lab (`h004`), R Gryphon Rock (`h015`), A Chapel (`h037`), and S Hjordhejmen (`h00K`) in command-card order. Stronghold (`h039`) remains the Barracks upgrade; Sniper Nest branches to Gunner's Hall (`h05D`) and Marksmen's Encampment (`h0A1`); Chapel upgrades through Church (`h038`) to the legendary Holy Altar (`h072`). These produce Footman/Defender, Sniper/Heavy Gunner/Marksman, Mortar, Gryphon Rider, Crusader/Paladin/Holy Warrior, and Warlock respectively. Ranger, Catapult, Ice Troll Shadow Priest, and the two towers remain verification content but are not assigned to the playable Human builder. Sniper, Heavy Gunner, and Marksman Critical Strike parameters are versioned by ability (`A02L`, `A06L`, `A06F`); Heavy Gunner and Mortar splash bands are extracted from their unit attacks. Marksman has a separate structure attack with its own range, damage, and target mask. Instant ranged attacks resolve damage on the attack tick without a travelling gameplay projectile.

The 9.27 Human support abilities are version-scoped composite profiles. Crusader and Paladin Heal select a wounded ally, restore health, and apply timed Inner Fire armor and regeneration; the Paladin profile grants the one-time maximum-health bonus and revives one eligible friendly corpse near the caster. Holy Warrior Prayer heals and restores mana to allies around its target, applies timed armor and attack damage bonuses, and revives up to three eligible friendly corpses. The Warlock's registered Parasite order delivers the extracted Frost Nova area damage and suspends its self Brilliance Aura during that spell's cooldown. Devotion Auras refresh timed armor on allies in range. Cleave applies a fraction of melee attack damage to other nearby ground enemies; legendary spell resistance scales incoming spell damage. Corpse resurrection carries the resolved unit template through snapshot serialization, so replay or restore cannot change the revived unit's type or combat properties.

Retained 9.27 evidence for the not-yet-promoted Elemental of Lightning line records Lightning Attack as two target-class-specific orb/Forked Lightning pairs, and this distinction is authoritative content rather than an implementation convenience. Unit-primary attacks use `A0CH` → `A0CJ` for Elemental of Lightning and `A0CI` → `A0CK` for Greater Elemental of Lightning; the Forked Lightning profiles target hostile air/ground unit-like targets, deal 50 / 80 spell damage, and allow 3 / 5 total targets. Structure-primary attacks use the separate `A0F7` → `A0F8` and `A0F0` → `A0F6` pairs; those Forked Lightning profiles target hostile structures only, deal 25 / 40 spell damage, and have exactly one target. This is the extracted source of the tooltip's 50% building-damage rule and means a structure near a unit-primary victim is not a valid additional lightning target, while a building chosen as the primary target is struck through the single-target structure profile.

### 14.2 Item definitions

Builder-held item content may include:

```text
inventory/equip rules
passive or activation mode
automatic trigger cadence
cooldown / charges
range or global scope
target filters and target policy
ability/effect references
stacking rules
```

Items may provide map-wide friendly auras, automatically fire at nearby enemies, or expose player-targeted area effects. Item behavior MUST NOT make the builder an ordinary targetable/blocking combat unit.

## 15. Scripting

General gameplay scripting is **open** and SHOULD be deferred until concrete content cannot be expressed cleanly through data + deterministic systems.

If scripting is introduced, it MUST satisfy:

- deterministic execution;
- deterministic iteration APIs;
- bounded/resource-controlled server execution;
- no wall clock/filesystem/network/OS randomness access;
- explicit versioning;
- snapshot-safe script state;
- replay-safe semantics.

A native Rust effect/system implementation is preferable for early milestones.

## 16. Map definitions

Authoritative map content includes:

- map dimensions/coordinate system;
- static navigation/buildability grid/geometry;
- objectives/castles;
- player start/build regions;
- spawn-related anchors if needed;
- lane/objective connectivity semantics;
- terrain movement costs/classes;
- authoritative terrain combat elevation/cliff-level data used by rules such as uphill miss;
- game-mode parameters.

Authoritative combat elevation SHOULD be represented in a deterministic queryable form appropriate to the imported map (for example discrete cliff/elevation levels or regions), rather than recomputed from presentation mesh geometry at runtime. The standard Castle Fight bases are elevated above the lane, so imported map data must preserve that relationship for combat even if the presentation terrain is rebuilt differently.

The current Warcraft importer consumes the committed W3E-derived `terrain.json`. Its `width`/`height` are terrain-tile dimensions while `groundHeight` and `layerHeight` are vertex arrays of exactly `(width + 1) × (height + 1)` samples. The extraction translator stores those rows north-to-south even though Warcraft world Y increases from the map's bottom edge; native loading must preserve that row-orientation convention. Combat uses the discrete `layerHeight` cliff level, while the raw `groundHeight` samples remain available for future terrain/presentation work without silently becoming combat elevation. Retained terrain outside the authoritative navigation rectangle is presentation/background only and is non-traversable for ordinary ground and air combat movement.

Decorative terrain/props MAY be client-only where they do not affect navigation/visibility/gameplay.

## 17. Content validation

Loading must fail before match start for invalid authoritative content.

The version/binding resolution checks in section 14.1 are mandatory for every match configuration.

Validation SHOULD catch:

- duplicate IDs;
- missing references;
- impossible/overflowing numeric values;
- invalid footprint geometry;
- empty target rule sets where prohibited;
- zero/negative production intervals;
- invalid projectile speeds/lifetimes;
- cyclic upgrade dependencies unless intentionally supported;
- unsupported effect combinations;
- map objectives outside bounds;
- inconsistent build/navigation grids;
- missing/invalid combat-elevation data where terrain-height combat rules are enabled.

## 18. Hot reload

Content hot reload is useful during development but MUST NOT modify authoritative gameplay definitions mid-network-match unless the change is represented as a deliberate synchronized simulation operation/version transition.

The simplest rule is:

- development sandbox: hot reload allowed with simulation reset;
- live match: authoritative content bundle immutable.

Presentation assets may hot reload independently if gameplay geometry/state is unaffected.

## 19. Modding

Modding is not required for the first playable version, but data-driven content and canonical hashes should make it possible later.

A server hosting modded authoritative content must advertise the exact bundle identity and ensure joining clients possess compatible gameplay definitions.

Asset distribution/security/licensing is outside this specification for now.

## 20. Original-game compatibility data

If the project aims to reproduce particular Castle Fight behaviors, observed rules should be recorded as explicit content/behavioral fixtures rather than scattered magic constants.

Where original behavior is uncertain, tests/content should label the choice as:

- verified original behavior;
- inferred behavior;
- intentional divergence.

This will prevent accidental changes while reverse-engineering game feel.
