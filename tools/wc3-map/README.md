# Warcraft III map extraction tooling

This directory contains the reproducible extraction pipeline for the original Castle Fight Warcraft III map. The goal is to preserve as much source evidence as possible in searchable, diffable text before translating it into Castle Fight Native content definitions.

## Run it

From the repository root, first cache the matching Warcraft III base object data from a local install, then regenerate the map extraction:

```sh
WC3_INSTALL=/mnt/gamessd_linux/Games/Warcraft3 tools/wc3-map/extract-base-data.sh
tools/wc3-map/extract.sh
```

`WC3_INSTALL` defaults to `/mnt/gamessd_linux/Games/Warcraft3`. The default map input is `docs/original_map/5329_Castle_Fight_DE_beta9.27_w3p.w3x` and committed output is regenerated under `docs/original_map/extracted/`.

The pipeline uses three ignored working locations:

- `.local-tools/` for source checkouts, builds, and locally built executables;
- `.map-work/` for extracted map members and intermediate translations;
- `.wc3-base/` for the small subset of locally installed Blizzard data needed to resolve inherited object fields and pathing masks.

None of these directories is required in git. `.local-tools/` and `.map-work/` are reproducible from the checked-in scripts/map. `.wc3-base/` must be repopulated from a Warcraft III install before inherited data can be regenerated.

## Terrain presentation assets

The committed `terrain.json` also provides the exact ground/cliff palette rawcodes used by Castle Fight. To copy only those terrain atlases from a local Warcraft III installation and convert them for the native client, run:

```sh
WC3_INSTALL=/mnt/gamessd_linux/Games/Warcraft3 python tools/wc3-map/extract_terrain_textures.py
```

The terrain asset extractor resolves each palette rawcode through Warcraft's `TerrainArt/Terrain.slk` or `TerrainArt/CliffTypes.slk`, then copies only the referenced classic terrain textures. It does not scan unit/model directories and is intentionally separate from the general unit/model asset extraction work. Current Warcraft installations expose the classic terrain atlases as DDS; the tool also probes BLP/TGA/PNG and uses ImageMagick or ffmpeg for conversion without adding a second in-repository BLP decoder.

Generated files live under ignored `assets/wc3/terrain/`: PNG ground/cliff atlases plus `manifest.json` recording the rawcode-to-CASC provenance and atlas shape. No Blizzard texture is committed. When this generated directory is present, the 3D client uses the ground atlases with Warcraft's tilepoint blend masks and variation selection; without it, the existing procedural solid-color terrain remains the fallback. Cliff atlases are extracted and recorded now, while cliff-face mesh/texturing remains separate from the smooth authoritative height surface used by the current client.

## Doodad presentation assets

`resolved/placed-doodads.tsv` is the authoritative placement catalog for the original map: rawcode/kind/name, exact X/Y/Z, angle, per-axis scale, visibility/solid/fixed-Z flags, variation, and resolved pathing footprint. The shared Rust WC3 asset exporter embeds this placement catalog and resolves the corresponding classic doodad/destructable skin records directly from the user's Warcraft III installation.

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --doodads \
  --output assets/wc3/doodads
```

The generated ignored bundle contains `manifest.json`, glTF/bin models, and PNG textures. Only model variations that are actually placed by Castle Fight are exported. Editor-only LOS/pathing blockers are deliberately retained as unresolved render placements rather than shown in the client. Destructable replaceable textures (notably tree skins) are resolved from Warcraft's `texID`/`texFile` metadata. The native client automatically instantiates every resolved visible placement when this bundle is present; if it is absent, doodad presentation is simply skipped.

MDX particle/ribbon emitters are not yet converted, so the two emitter-only bubble-geyser doodads currently have empty glTF scenes. This does not affect the geometry-backed trees, shrubs, rocks, walls, ruins, props, water plants, fish, etc.

## Source-built third-party tools

Only open-source tooling is fetched, pinned, license-checked, and built locally.

### StormLib

- project: `ladislav-zezula/StormLib`
- revision: `44ebfbfc109d76e2a85bbd5d8b0c949df7e65c6f`
- license: MIT
- bootstrap: `bootstrap-stormlib.sh`

StormLib is compiled from source with CMake/Ninja and bundled dependencies. `mpq_extract.cpp` is our small archive wrapper around StormLib.

### CascLib

- project: `ladislav-zezula/CascLib`
- revision: `2a280f5a231966dc5d1b534978dd9f9f04a374cd`
- license: MIT
- bootstrap: `bootstrap-casclib.sh`

CascLib is compiled from source with CMake/Ninja. `casc_extract.cpp` is our small read-only CASC wrapper. It is used only to copy the required base-game data out of the user's local Warcraft III installation into `.wc3-base/`; no CascLib binaries or Blizzard source data are committed.

### WC3MapTranslator

- project: `ChiefOfGxBxL/WC3MapTranslator`
- revision: `7d477ebb5cea445bee7915fe72cd93ee399252b7`
- license: MIT
- bootstrap: `bootstrap-translator.sh`

Its TypeScript source is installed from the pinned lockfile and compiled locally with `tsc`. It is used for the binary terrain, doodad-placement, and object-editor formats it understands.

WC3MapTranslator currently expects `war3map.w3i` format 33, while this map uses format 31. `decode_map.py` therefore parses this map's W3I directly. Its WTS path also misinterprets this map's UTF-8 strings, so `decode_map.py` parses `war3map.wts` itself and repairs equivalent UTF-8-as-Latin-1 text from translated object tables.

## Base-game data selection

This map records Warcraft III version `2.0.4.23745`, W3I `game_data_set_version = 0` (`Default`), and game-data version `1` (`TFT`). WC3MapTranslator's W3I enum is `Default=0`, `Custom101=1`, `MeleeLatestPatch=2`; for TFT the installed default/custom balance data is the `custom_v1` overlay and `:custom,V1` profile variant:

```text
war3.w3mod:_balance\custom_v1.w3mod
```

`extract-base-data.sh` caches all three selectable balance overlays (`custom_v0`, `custom_v1`, `melee_v0`) plus the standard editor metadata, en-US editor/game strings, doodad/destructable data, and all 207 standard pathing TGAs. `resolve-base-data.py` selects the overlay/profile variant from W3I instead of assuming one data set. It intentionally does not copy the whole `war3.w3mod:units` asset tree: only the data tables needed for inheritance are retained.

This distinction is gameplay-significant: resolving this TFT map against `custom_v0` incorrectly produced, for example, Mortar speed `220` instead of the live/default `270`. The generated `xO` effective-unit table provided an independent cross-check that exposed the selection error. The resolver emits the selected W3I label/profile/CASC overlay and exact cache hashes with the committed resolution.

## Protected archive behavior

The source map is W3P-protected. It has a normal Warcraft III `HM3W` wrapper with the MPQ archive beginning at byte `0x200`, but its MPQ file table has been deliberately poisoned with tens of thousands of anonymous pseudo-files.

The normal extraction path therefore opens only canonical Warcraft III map-member names by MPQ hash. This recovers the useful map data without trusting the sabotaged file listing.

`mpq_extract` has an explicit `--scan-unknown` mode for forensic work. Do not use it casually: on this 15.9 MB map the poisoned table expanded into roughly 2.4 GB of overlapping/garbage output during investigation. The scan is intentionally not part of `extract.sh`.

## Generated plaintext

`docs/original_map/extracted/` contains:

- `summary.json` — concise source/archive/content inventory;
- `archive-extract.tsv`, `archive-members.json` — recovered canonical members and hashes;
- `map-info.json` — W3I map metadata, players, forces, bounds, environment, mode data;
- `terrain.json` — W3E terrain dimensions, tile palette, heights, textures, cliff data;
- `pathing.json`, `pathing-grid.hex.txt`, `walkability.txt`, `buildability.txt` — WPM navigation/buildability data;
- `shadow.json`, `shadow-grid.hex.txt` — SHD shadow data;
- `doodads.json` — placed doodads/destructables from `war3map.doo`;
- `minimap-icons.json` — MMP minimap markers;
- `strings.json`, `war3map.wts` — UTF-8 trigger strings/tooltips;
- `obj-*.json` — decoded object-editor override tables;
- `catalog/object-fields.tsv` — every map object modification in file order, including duplicate/conflict metadata;
- `catalog/objects.tsv` — compact map-override object index;
- `catalog/units.tsv`, `catalog/buildings.tsv`, `catalog/abilities.tsv` — map-override convenience views only;
- `resolved/object-fields.tsv` — full base + map effective editor fields, with base value, every map candidate, last-write value, recovered value, metadata label/source, and protection-conflict provenance;
- `resolved/base-source-fields.tsv` — inherited SLK source columns for every map object, including computed/non-editor fields such as `realHP`, min/max damage, and source DPS;
- `resolved/items.tsv` — normalized definitions for every authored map item, including helper/result items that are not directly sold by the Castle shop; costs, stock/use flags, cooldown group and attached ability multiplicity are preserved (notably Multi Blast Staff's four `A02D` links);
- `resolved/castle-shop-items.tsv` — exact 10-slot Castle inventory joined to normalized item fields plus fully resolved attached ability levels with protected cooldown/mana overrides applied where known;
- `resolved/item-mechanics.tsv` — importer-facing script-only item behavior layered on top of the item definitions: Gold scaling, Cheese legendary-slot/refund logic, four-Blast-Staff conversion, Double/Quad aura carriers, Orb of Lightning round-scaled casts, and Scroll of Stone/Speed hidden effect abilities;
- `resolved/units.tsv` — normalized **static object-data** unit/building stats including HP/mana/regen, armor, movement, collision, acquisition, abilities, both attacks, damage ranges, cooldowns, AoE and derived DPS; W3P poisons many spawned-unit combat fields even without duplicate modifications, so use the effective runtime catalog below for covered production units;
- `resolved/protected-unit-stats.tsv` — 549 protected `UnitStat` rows decoded from generated initializer `jP`, with every explicit HP/armor/defense/speed/attack-1/attack-2 override applied on top of the static object-data fallback, derived attack min/max/average/DPS, and both encoded-row and per-field provenance retained;
- `resolved/effective-unit-stats.tsv` — 162 building→spawned-unit runtime/effective catalog rows from generated Lua, joined to static unit values for HP, armor, DPS, attack range, and movement speed with per-field comparison labels;
- `resolved/production-unit-attacks.tsv` — two explicit weapon-profile rows per production unit, combining decoded `UnitStat` damage/dice/cooldown/range with static attack type/weapon/targets and structurally detected `Agra`/War Club attack-index switching; `xO` DPS/range are retained only as cross-checks;
- `resolved/production-unit-special-mechanics.tsv` — importer-facing runtime mechanics that bypass the ordinary scripted unit-spell registry. Current exact rows normalize Mountain Giant's automatic temporary-tree/War Club setup; Echofoot Mystic's damage-triggered Echo Step/remnant; Gnoll anti-air retaliation; Defender's automatic native Defend maintenance; Greater Fire Elemental splitting including the `u00F → h030` spawn-building provenance handoff consumed by spawn-building-sensitive perks; Avatar/Avenging Spirit death/kill effects; Vampire Eternal Servitude; Troll-family kill Berserk; Winged Riptide Serpent's +45% qualifying damage against flying targets; Forest Troll Trapper's persistent +30/+60/+90 low-HP attack tiers; Ironpaw Guardian's 20% 160-radius Whirlwind proc with the script-authoritative 175 universal damage despite its 150-damage tooltip; Dryad/Keeper/Ancient Keeper attack-proc dispels; Bear/Ancient Bear Feral Rage + scripted hibernation; Razormane reactive Razor Spray; Emerald Dragon stacking corrosion; Greater Water Mirror Image; Greater Wind's damage-to-mana Kaboom charge; Earth Elemental health-scaled Aftershock; Lightning Elemental melee-retaliation Thunderbolt; Paladin summon-time mana reset; Mine Layer train-time random initial mana; Goblin Rocketeer's runtime exploded flag plus native death explosion; Lich King's exact two-branch Mastery over Death roll; and Vampire Lord's five-level Blood Corrosion stack;
- `resolved/production-unit-runtime-coverage.tsv` — strict closure audit over production-unit rawcodes referenced by the core train/summon/damage/death/order runtime plus explicitly audited marker/listener hooks such as Earth, Lightning, Riptide, Troll Blood and Whirlwind. Every audited unit must resolve to importer-ready special mechanics, scripted unit-spell semantics, Greater Fire's already-proven `h02Z` child endpoint, or Shadow Drake's strictly asserted visual-only vertex tint; otherwise extraction fails instead of silently leaving a runtime branch unmodeled;
- `resolved/perk-mechanics.tsv` — importer-facing semantics for **all 19/19 live draft perks** proven through the protected registry. Besides the eight damage-listener perks, it normalizes Arcane Overcharge, Cloudchaser, Feathered Friend, Towerless, Glass Cannon, Bulwark March, Forward Line, Longline Formation, Production Enchantment, Rampant Growth and Spell Enchantment, joining the relevant item/unit/ability object data. Less-obvious runtime details are retained explicitly: Feathered Friend's cumulative food schedule and 5-minute damage steps, Towerless's Y-tier tech caps, Glass Cannon only modifying weapon 0, Forward Line's +3/-2 castle-boundary armor swap, Production Enchantment's multiplicative 0.95× then 1.15× spawn scaling, Spell Enchantment's direct mana-regen deltas, and Longline Formation's script quirk that computes `weapon0 + 90` but writes the range delta to weapon index 1 while raising acquisition range;
- `resolved/runtime-ai-mechanics.tsv` — importer-facing **AI-only** observation/decision semantics kept separate from authoritative combat rules. It normalizes the damage-weighted engagement/structure-pressure signal with its 2-second 0.8 decay; the complete damage-triggered Rescue Strike controller (`<65%` structure gate, HP-scaled required enemy count, 700/900 geometry, tower-vs-siege `-4`, 28/75-second throttles, 3-second coordination lock, protected `A005` `0` mana / `60s` cooldown); and the team strategic-aura purchase observer that tracks both human and AI purchases of `I00A` Bassline Generator and `I003` Bass Drums and removes already-bought auras from future AI candidates;
- `resolved/runtime-session-mechanics.tsv` — importer-facing player-session mechanics that affect authoritative control or match flow: No-AFK automatic 1-second idle checks with 20/30-second round-start and 60/120-second normal warning/AWAY thresholds, allied advanced-unit control sharing/restoration, and the three leave-autobalance modes (asset redistribution, dependent-slot sharing, AI takeover) plus delayed team-empty match resolution;
- `resolved/runtime-mode-mechanics.tsv` — complete importer-facing 9.27 mode-selection registry and host chat parser: all 44 registered mode IDs/names/descriptions, exact integer ranges/choice sets, generated closure-class→callback mappings, callback direct-call/simple-assignment evidence, Start Resource `g/l/u` validation, reserved parser commands and Ultimate Draft conflict gating;
- `resolved/runtime-campaign-mechanics.tsv` — exact campaign star-restriction event semantics for tracked player-built building loss (`challenge_no_buildings_lost`) and challenge-bound item purchases (`challenge_no_items`), with the protected restriction IDs statically decoded from the W3P string layer;
- `resolved/runtime-system-mechanics.tsv` — importer-facing cross-system runtime mechanics that do not belong to one production unit or spell row. The 9.27 baseline currently normalizes Power Plant/Power Surge including spawned-unit augmentation and periodic freeze cleanup; Heroic Shrine companion rolls plus Murloc/Twins transformations; Golden Shrine one-time delayed revival; Blood Fiend body/trait randomization; Elemental Linker's building-count-scaled death heal; Treasure Box's exact nonlinear income multiplier; Human Artillery's random enemy-base attack-ground controller joined to its protected 300–400 siege weapon; Raise Dead carrier replacement into the exact random skeleton tables including Lich King +12 damage/+3 armor; Gjallarhorn's separate team construction counter that drives its 40/45/50/55% buff levels; the shared Assassin/Royal Assassin/Gobbo support-order controller; the player-issued combat-unit order suppression/restoration system with exact stop/move/attack exceptions and 5/15/30/60/120/240/300-second escalating control penalties; the 4-second global idle-unit re-engage sweep started each round; builder `A0-1` Blink with exact 64-unit own-castle clamp; Rescue Strike `A005`; Obelisk of Elements' actual A0F1/A03Q 50%-damage 250-radius cleave including its compacted-index quirk; Locust Harpy's 50% structure-damage rewrite; Celestial Chi Tower's impact-vision batching; first-15-second castle protection; Eye of Corruption's `B00Q`-gated 1.12× non-attack damage multiplier; and Obelisk of Light's persistent cleanse carrier. Resolved rows retain script-vs-tooltip/object discrepancies rather than silently flattening them;
- `resolved/production-unit-abilities.tsv` — all initial production-unit ability links, retaining map/inherited definition provenance, initial level, effective protected cooldown/mana, range/area/targets/buffs, labeled ability-specific Data fields, and direct Lua rawcode-reference counts/functions; unmodified Blizzard utility abilities are kept rather than silently omitted;
- `resolved/unit-spells.tsv` — all 37 generated scripted unit-spell registrations joined to resolved unit/ability definitions, protected cooldown/mana, production source building when applicable, base order, exact target-mode semantics, expected immediate unit for the one inlined special registration, concrete handler provenance, and numeric order IDs resolved from canonical Warcraft order constants (`parasite=852601`, `heal=852063`, `absorb=852529`) while retaining the protected registry expression separately;
- `resolved/unit-spell-mechanics.tsv` — complete implementation-evidence profiles for those 37 handlers: normalized primitive kind, direct/helper calls, exact generated delayed/group/periodic callback targets, scheduled/random timing literals, semantic effect call sites, per-function numeric literals, and bounded reachable map-object evidence enriched with resolved ability/unit object data;
- `resolved/unit-spell-semantics.tsv` — stricter native-import normalization for all 37 scripted unit spells. **37/37 are implementation-ready** (`17` object-effect driven, `20` script-native); Master of Elements' protected global Frost target filter `SX` is statically resolved to generated predicate `vL` (`alive-combat-sapper;enemy-of-mIb`). Mana Generator is normalized as the actual live mechanic: a permanent A0EL mana-regeneration improvement on one allied mana-using non-production building, level 1 normally and level 2 on Elemental buildings. The superficially related A0EL check inside `setupUnit` is separately classified as unreachable in legal Mana Generator gameplay because `improveSpecialBuilding` rejects every production building (`hIb[...] > 0`) while every `setupUnit` caller is a production companion-spawn path. Ready rows therefore do not import that dead branch as a Mana Generator combat mechanic;
- `resolved/element-building-buckets.tsv` — the 12 Elemental-race buildings grouped into the exact Fire/Earth/Lightning/Water/Wind counter buckets consumed by Master of Elements scaling formulas;
- `resolved/production-unit-corpses.tsv` — all 167 authored production-unit death profiles with Warcraft `Death Type` capability bits (`can_raise`/`does_decay`), per-unit `Death Time`, mechanical/campaign flags, effective flesh/bone decay durations, and the death+flesh+bones arithmetic only for units that actually decay;
- `resolved/death-decay-constants.tsv` — base Warcraft and map-override values for flesh, bone and structure decay. This map keeps flesh at the build default `2`, overrides bones `88 → 25`, and structures `30 → 0.1`;
- `resolved/production-buildings.tsv` — all 240 authored race/building definitions joined from generated `UnitObjectMeta`, race membership, race-wrapper semantics, upgrade/precursor edges and resolved footprints; its 167 production rows split cleanly into 162 normal-play `xO` buildings plus five campaign-only Critter productions. `spawn_time` is the recurring Castle Fight production interval; every production row separately has WC3 object `build_time=2`, the short construction/cancellation window rather than a spawn timer;
- `resolved/building-spells.tsv` — all **43** generated scripted building-spell registrations joined to effective ability fields and exact concrete Lua handlers: 15 protected registry calls plus 28 direct `EVENT_PLAYER_UNIT_SPELL_EFFECT` listeners. Forty-two use Castle Fight's mana timer (`effective mana cost / building mana regeneration`); Tidal Guardian is the explicit exception and uses its protected 15-second WC3 ability cooldown;
- `resolved/building-spell-evidence.tsv` — complete implementation-evidence profiles for all 43 building spells, following exact named helpers and generated delayed/group/periodic callbacks and enriching reachable rawcodes with resolved ability/unit fields;
- `resolved/building-spell-mechanics.tsv` — stricter importer-facing semantic rows for **all 43/43 scripted building spells** across both registration representations, joining script-proven targeting/delivery/constants to separately sourced linked ability/unit/buff definitions. Direct-listener mechanics include the full Magic Ruin 10-way branch table, Eraser same-type battlefield wipe, Desert utility spells, Elemental/Naga/Nature/Nelf utility mechanics, shield generators and charge systems. All fixed protected target filters used by these rows are now resolved; tooltip/object disagreements such as Chilling Mushroom `150` vs linked `A0AK Damage=200` are retained rather than silently chosen;
- `resolved/corpse-building-mechanics.tsv` — importer-facing normalization of Skull Pile, Skull Shrine and Vessel of Purity corpse logic: exact dying/dead predicate, WC3 `can_raise` independence, consumption mode/radius, effect radius/damage, auxiliary ability, proven summon probabilities, and the joined building-spell cadence;
- `resolved/buildings.tsv` — resolved building stats plus exact pathing texture dimensions and per-cell walk/fly/build masks;
- `resolved/abilities.tsv` — one row per statically resolved ability level with object-data costs/cooldowns/range/area/targets and labeled Data fields; for W3P-protected cooldown/mana values, cross-check the runtime table below;
- `resolved/protected-ability-fields.tsv` — the generated Lua protection table joined back to resolved static ability data, preserving runtime cooldown/mana values beside the static values and an explicit `static-match`/`static-differs` comparison;
- `resolved/protected-ability-jass-add-restores.tsv` — the smaller hard-coded restore path used when abilities are added through the JASS-compatible wrapper, cross-checked both against the canonical protected table and static object data;
- `resolved/placed-doodads.tsv` — all 1,780 placed doodads/destructables joined to base/custom names and exact pathing masks;
- `resolved/pathing-textures.json` — decoded standard TGA pathing masks; each pixel is one 32-world-unit pathing cell;
- `resolved/protection-conflicts.tsv` — the small set of repeated W3P modifications whose candidate values disagree;
- `resolved/base-data-manifest.json`, `resolved/summary.json` — exact local base-data hashes/build identity and resolution coverage;
- `script/war3map.lua` — verbatim protected runtime Lua;
- `script/functions.tsv`, `script/function-spans.tsv`, `script/readable-function-names.txt` — Lua-aware named-function indexes with true source byte offsets/spans;
- `script/calls.tsv`, `script/call-graph.tsv` — lexical call counts plus caller→callee edges, excluding function declarations and tokens hidden inside strings/comments;
- `script/function-aliases.tsv` — exact simple named-function assignments used heavily by Wurst's generated prototype/virtual-dispatch tables; `slot-name-prefix` marks the structural case where the slot name itself prefixes the assigned implementation name, while `assigned-other-target` preserves inherited/alternate assignments without interpreting them;
- `script/function-value-arguments.tsv` — named functions passed as values to another call (for this protected script, currently the generated `xpcall` initialization layer), indexed separately rather than pretending the value reference is an ordinary direct call;
- `script/castle-shop-items.tsv` — exact generated Castle slot→item mapping plus the integer item-value table used by pickup logic;
- `script/item-mechanics.tsv` — direct pickup/inventory transformations recovered from `ZH`/`YH`/`VH`/`WH`, kept separate from object-editor abilities;
- `script/item-spell-mechanics.tsv` — exact OnCast routes for Castle item spells whose visible item ability is only a trigger: Orb of Lightning (`A02H→cfOL`) and Scroll of Stone/Speed (`A08Y→A02G`, `A0HJ→A0HB`), including dummy/order/lifetime and round-scaling parameters;
- `script/building-spell-registrations.tsv` — exact generated `building rawcode + ability rawcode + concrete handler` triples from both registration representations. It joins closure construction and `BuildingSpellClosure_cast` prototype assignments to either protected registry calls or direct spell-effect `EventListener` fields, snapshotting reused Wurst listener locals at each `EventListener_add` call;
- `script/building-spell-evidence.tsv` — bounded lexical mechanics evidence for every registered building spell: direct/helper calls, generated callback dispatch, delays/periods/random ranges, reachable map rawcodes, semantic effect-call arguments and source literals;
- `script/unit-spell-registrations.tsv` — exact generated scripted-unit registry rows recovered from `UnitSpellClosure_cast1` dispatch plus the unit/ability/target-mode/order/callback registry. It captures 36 protected registry calls and the one explicitly inlined Mine Layer registration without executing Lua; the protected order expression is preserved here verbatim as evidence, while the resolved layer independently maps each ability's canonical base-order string to its Warcraft numeric order ID;
- `script/unit-spell-mechanics.tsv` — static mechanics scaffold for every registered unit spell, preserving direct primitives/helper functions, exact generated `doAfter`/`ForGroupCallback`/`CallbackPeriodic` dispatch, calls made by lexically contained anonymous timer callbacks, semantic effect-call arguments, source numeric literals and bounded map-object evidence without executing callback state machines;
- `script/protected-filter-bindings.tsv` — static W3P Filter-wrapper bindings resolved only when use sites and generated function structure make the mapping unique. All 21 fixed wrappers are now resolved, including the Wurst `ClosureForGroups` dispatcher `Gib=kG`, Assassin target priorities, Desert Replenishment/Peyote filters, Withering mana priorities, Gobbo repair targets, round-cleanup `e0`, Snowveil/Master `SX`, Elemental-linker side-effect filter `QX`, Echo Remnant AoE `cHb=UC`, round-stat unit filter `dHb=TC`, builder selector `ZGb=VC`, AI repair selector `RX=AC`, and Blizzard Marketplace stock infrastructure `vFb=IC` (always true). Generic `registerPlayerUnitEvent` local `SCr` is correctly classified as a dynamic caller-supplied wrapper, so there are no unresolved fixed filter globals. The resolver deliberately no longer pairs filters by independently skipping to the next vaguely matching predicate;
- `script/protected-perk-registry-audit.tsv` — exact readable inventory of the 19 authored draft-perk factories (`perk_01` through `perk_19`) cross-checked against the live normal-draft path into protected `initPerks__w3p_vmProtect`. The extractor statically reverses the visible W3P byte/string transforms for VM block 65, resolves `addRegisteredPerk__w3p_vmProtect`, and proves each factory→slot call individually, including the three factories registered directly on the VM stack instead of through a local;
- `script/perk-mechanics.tsv` — strict script-level normalization of **all 19 proven-live draft perks**, combining exact protected factory registration with readable apply/index/periodic/event/cleanup handlers, numeric factors, item/rawcode links and known runtime quirks. Extraction requires one semantic row for every proven protected registration, so a newly reachable or structurally changed perk cannot silently remain outside importer coverage;
- `script/runtime-ai-mechanics.tsv` — exact readable AI-runtime control flow for three systems: decayed engagement/structure-pressure observation, Rescue Strike target/threshold/throttle selection, and strategic-aura purchase tracking for `I00A`/`I003`. AI observer rows are kept separate from ordinary combat mechanics;
- `script/runtime-session-mechanics.tsv` — exact readable player-session control flow for AWAY/idle detection and player-leave/autobalance/team-empty resolution. These rows preserve control-sharing and match-flow behavior without mixing it into unit/spell mechanics;
- `script/runtime-mode-mechanics.tsv` — exact readable `ModeParser` registration/controller evidence. It inventories all 44 registered modes from `ModeParser_initialize__w3p_vmProtect`, resolves each generated callback object's `.call` alias to the concrete readable implementation, and preserves direct-call/simple-assignment evidence without executing protected parser code;
- `script/runtime-campaign-mechanics.tsv` — strict campaign challenge evidence for the two authoritative event-backed restriction failures: tracked-building death and challenge-bound item purchase. The extractor decodes `arb`/`crb` to `challenge_no_buildings_lost`/`challenge_no_items` and fails if those protected IDs or listener control flow change;
- `script/damage-listener-coverage.tsv` — strict closure audit over all 20 generated `DamageListener` callbacks. Sixteen are referenced by normalized gameplay mechanics, the remaining two live AI listeners are referenced by normalized AI-runtime semantics, one is round-stat telemetry only, and one is building-catalog E2E code. Extraction fails if a new listener appears without one of these explicit classifications;
- `script/event-listener-coverage.tsv` — strict closure audit over all **70** generated `EventListener_*_onEvent_*` callbacks. It follows at most three exact named call edges to recognize thin dispatchers into normalized gameplay/AI/session/mode/campaign semantics and explicitly classifies the remainder as presentation, E2E, generic gameplay framework, generic command framework or telemetry. The 9.27 inventory now has **no live-unmodeled event listener**; extraction fails if the inventory or classification counts change;
- `script/production-unit-special-mechanics.tsv` — strict script-level rows for runtime production-unit mechanics outside the unit-spell registry, retaining rawcode-linked objects, exact handler/callback functions, constants and filter provenance before resolved production-building joins;
- `script/building-improvement-spawn-mechanics.tsv` — reachability audit for cross-runtime code keyed by permanent building-improvement markers. In 9.27 it records the dormant A0EL `setupUnit` branch (30% A0EZ+A0C5 one-shot trait, including attacked-event order 852601 and the `<128 hp → 5000 hp` path) but proves it is **unreachable from a legal Mana Generator target**: A0EL targeting rejects production buildings, and all five `setupUnit` call sites are production companion-spawn paths. This evidence is retained for version diffs but should not be implemented as live 9.27 gameplay;
- `script/runtime-system-mechanics.tsv` — strict script-level evidence for map-wide mechanics implemented across generic lifecycle handlers rather than one unit/spell registration. It currently captures Power Plant/Power Surge, Heroic Shrine companion spawning, Golden Shrine revival, Blood Fiend procedural body/trait generation, Elemental Linker death healing, Treasure Box income scaling, Human Artillery order maintenance, Raise Dead skeleton replacement, Gjallarhorn team-count scaling, the shared Assassin/Royal Assassin/Gobbo support-order state machine, player external-order suppression/restoration and escalating control locks, the global idle-unit attack re-engage timer, builder Blink clamp/teleport handling, Rescue Strike's full cast/damage/refund lifecycle, Forge Weapon cleave dispatch, Locust Harpy's structure-damage penalty, Celestial Chi Tower impact-vision batching, first-15-second castle protection, Eye of Corruption's buff-gated non-attack damage multiplier, and Obelisk of Light's persistent carrier/cleanse lifecycle with exact probabilities/formulas, rawcodes, predicates, callbacks, order IDs and timing;
- `script/element-building-buckets.tsv` — exact generated `vtb[buildingTypeIndex(rawcode)] = bucket` assignments used by Elemental building lifecycle counters and Master of Elements;
- `script/building-spell-mechanics.tsv` — structurally normalized mechanics for all 43 registered building spells: target selector/predicate, linked rawcodes, script constants, source functions and evidence classification. Direct-listener rows preserve protected filter symbols (for example Desert `Y0/X0/W0/V0` and Withering `G6/F6`) instead of promoting the current coarse filter-pairing heuristic into gameplay truth;
- `script/corpse-building-mechanics.tsv` — structurally parsed corpse-dependent building handlers: two `raiseFromCorpse` registrations with proven 10/30/30/30 summon branches and Vessel of Purity's dead-unit predicate, 220 consume radius, 300 effect radius, 150 universal damage and auxiliary Far Sight rawcode;
- `script/rawcode-summary.tsv`, `script/rawcode-reference-sites.tsv`, `script/function-rawcodes.tsv` — direct decimal rawcode references cross-linked back to map object IDs/categories/names and the exact function/call context where they occur;
- `script/runtime-mutators.tsv` — calls that can mutate unit/ability object state at runtime (`BlzSetUnit*`, `BlzSetAbility*`, unit ability add/remove/level, movement/state setters), annotated with map rawcodes directly referenced by the same function;
- `script/protected-ability-fields.tsv` — 452 structurally parsed cooldown/mana assignments from the protected `xD` initializer, covering 259 ability-level rows across 254 abilities; overlapping JASS-add restores are required to agree exactly;
- `script/protected-ability-jass-add-restores.tsv` — 17 explicit guarded restores from `applyProtectedAbilityFieldsForJassAdd`, including the one JASS-only assignment whose ordinary static value is already correct;
- `script/protected-unit-stats.tsv` — 549 structurally parsed `jP` source rows. The visible `cP`/`dP` decoder is mirrored exactly: rawcode + source fingerprint + 14 encoded fields become optional HP/armor/defense/speed and two attacks' base damage/dice/cooldown/range overrides; `2147483647` is the no-override sentinel and cooldowns are stored in integer microseconds;
- `script/unit-object-metadata.tsv` — 240 exact generated `UnitObjectMeta` gameplay rows consumed by `CFBuilding_setup`: spawned unit ID, gold/lumber/food, spawn interval, attack/defense indexes, and air/melee/mechanical/caster flags; presentation strings are deliberately skipped and rejoined from object data downstream;
- `script/race-buildings.tsv` — the complete 15-race/240-building generated partition with builder rawcode, source order and campaign-only classification;
- `script/building-upgrades.tsv` — all 90 authored building upgrade edges from `ensureUnitObjectUpgradeMetadataRegistered`;
- `script/race-building-semantics.tsv` — one exact wrapper-semantic row per authored building: income factor, precursor, tier-assignment presence, legendary/AA/siege/artillery/mode/AI flags, multi-target/cage/placement/spell/tower/power values and explicit tag lists. Its 90 precursor relationships are required to agree exactly with `building-upgrades.tsv`;
- `script/effective-unit-stats.tsv` — 162 structurally parsed rows from generated initializer `xO`, keyed by production building and spawned unit, with effective HP/armor/DPS/range/speed/spawns-per-cycle/air-target capability;
- `script/rawcode-mutator-traces.tsv` — deterministic shortest exact-named-call paths from a function that directly references a map rawcode to a function containing a runtime mutator, including hop count and the full lexical call path;
- `script/rawcode-mutator-summary.tsv` — compact per-rawcode coverage of those static mutation paths, including direct-vs-indirect evidence and reachable mutator kinds/sites;
- `war3mapMisc.txt`, `war3mapSkin.txt` — already-plaintext map configuration.

The ASCII pathing maps use `#` for blocked and `.` for allowed. WPM bit meanings currently decoded are `0x02` no-walk, `0x04` no-fly, `0x08` no-build, `0x20` blight, `0x40` no-water, with unknown/unused bits retained in the hex grid and histogram.

## Important interpretation limits

The inherited object-data gap is now resolved against the exact installed `2.0.4.23745` **Default (TFT)** data set (`custom_v1` / `custom,V1`) selected by W3I. `resolved/object-fields.tsv` is the complete editor-field view for map objects, while `resolved/base-source-fields.tsv` deliberately also retains source-table columns that do not map one-to-one to editable fields.

The official Castle Fight compendium at `castlefight.cfd/compendium/` exposes structured live data through `/api/meta/...` and is useful as an independent semantic/discovery cross-check. It is **not** an extraction input: this repository's protected map is Beta 9.27, while the live compendium can advance to newer official versions, so map object/runtime evidence remains authoritative for reproduced 9.27 values. For example the live compendium corroborates Echo Step's 15-second cooldown/1200-unit blink and Mountain Giant's 10-hit Grab Tree behavior, while its display-oriented damage fields are not always identical to WC3 dice semantics.

W3P protection produces repeated object-field modifications in 29 unit definitions; six objects have 14 fields whose repeated values actually disagree. This is no longer an unresolved gameplay-value ambiguity. All 13 numeric conflicts are independently confirmed by the decoded protected `UnitStat` runtime table—for example Mortar HP candidates `280,1,1` resolve to the same runtime HP `280`. The sole string conflict is Snowveil Fountain's ability list, whose three writes end `A0HO,AM0{`, `A0HO,AM0{`; the resolver therefore classifies that repeated final value as stable. `resolved/protection-conflicts.tsv` still preserves every candidate, ordinary last-write value, selected value and provenance so the protection artifacts remain auditable.

Building and static-doodad footprints no longer rely on filename hints. The resolver decodes the actual Blizzard pathing TGAs. TGA red/green/blue channels become unwalkable/unflyable/unbuildable bits respectively, and each TGA pixel is a 32-world-unit pathing cell. Irregular footprints are kept cell-for-cell as hexadecimal bit rows.

The W3P runtime script remains obfuscated/minified. Its source is retained verbatim and thousands of Wurst-generated function names are still searchable, but encrypted string constants and higher-level control flow are only decoded where the generated protection structure can be validated exactly. The script indexer is deliberately lexical: it skips strings/comments, understands named/method/anonymous function scope, records exact byte offsets, and never executes map code. Direct same-function rawcode↔setter links are useful evidence. `rawcode-mutator-traces.tsv` additionally follows only exact calls whose callee resolves to a named function in the same script and keeps one deterministic shortest path; it does **not** infer function-value/callback targets, prove that a branch executes, or prove that the rawcode is passed as an argument to the eventual setter. `function-aliases.tsv` separately recovers exact Wurst dispatch/prototype assignments, but an alias assignment still does not prove which virtual receiver is instantiated or invoked at runtime; the `slot-name-prefix` label is only a structural naming relation, not a semantic confidence score. The draft-perk initializer is now a stronger exception: its W3P VM bytecode and encrypted global-name table are statically reversed from the visible decoder, and all 19 factory→`addRegisteredPerk` slot calls are proven without executing map code. Other protected VM blocks remain unresolved unless similarly decoded and cross-checked. These indexes are therefore static evidence, not execution claims.

One important protection layer is stronger than generic reachability evidence: the generated `xD` initializer explicitly constructs the table consumed by `applyProtectedAbilityFields`, while `checkProtectedAbilityFieldsOnUnit` and `checkAllProtectedAbilityAdds` compare live Warcraft ability fields against that same table. The extractor structurally parses 452 cooldown/mana assignments from it. On this map every one of those 452 values differs from the statically resolved object-data value, commonly the protection sentinels `99` cooldown and `9999` mana. The runtime values are therefore retained separately in `resolved/protected-ability-fields.tsv`; the resolver does not erase or rewrite the static object evidence.

Unit combat data has a similar but broader protection layer. Generated initializer `jP` contains 549 protected `UnitStat` rows and the visible `cP`/`dP` helpers fully define their encoding, so no VM execution is required to decode them. The table contains 1,887 explicit overrides across HP, armor, defense type, movement speed, and both attacks' base damage/dice/cooldown/range. `UnitStat_applyTo` applies exactly those fields to live units, falling back to ordinary object data where a row carries the no-override sentinel. `resolved/protected-unit-stats.tsv` therefore provides the strongest source for those attack primitives while preserving the fallback and source of each value.

Generated initializer `xO` separately contains one complete effective summary row for each of 162 production buildings and 162 distinct spawned unit IDs, and `applyEffectiveUnitStat` copies those rows into the building catalog. With the corrected W3I-selected `custom_v1` fallback, applying the decoded UnitStat overlay reproduces **all 162 HP, all 162 armor, all 162 movement-speed values, and 160/162 DPS summaries**; the two remaining DPS rows are special/no-base-attack cases. Attack range matches 159/161 comparable rows. The two range exceptions are Mountain Giant `e00F` (`xO=277`, weapon ranges `128/150`) and Roterothopter `h06Q` (`xO=999`, weapon ranges `500/500`); both exhibit an exact `sum(weapon ranges)-1` pattern, which the normalized attack table records as a pattern only rather than promoting it into weapon semantics. This cross-check is strong evidence that ordinary production-unit combat inheritance and UnitStat decoding are correct. For example Faerie Dragon `e000` is statically HP `1` / armor `0` / range `1`; its decoded UnitStat overlay is HP `460`, armor `2`, attack base/dice `49 + 1d1`, cooldown `1.75`, range `350`, which yields the `xO` DPS summary of approximately `28.57`.

Mountain Giant's special spawn state is also directly visible in runtime Lua. `onUnitEnteredMap` dispatches `e00F` to `castNatureAttackTree`, which creates a temporary `VTlt` tree 32 world units in front of the unit, issues target order ID `852511`, waits 1.6 seconds before restoring attack behavior, and removes the temporary destructable two seconds later. Ability `A0BC` inherits Warcraft's `Agra` War Club behavior and decodes to attack-index switch `0 → 1` with **Maximum Attacks = 10**; the conditional second weapon is siege, can target air/ground/structures, and resolves to `174 + 1d26` at 2.2 s and 150 range. Roterothopter `h06Q` instead has two ordinary enabled weapons: `19 + 1d11` at 1.0 s against air (**25 DPS**) and `19 + 1d1` at 1.0 s against structures (**20 DPS**), both at 500 range.

The absence of a canonical member such as `war3mapUnits.doo`, `war3map.w3q`, or `war3map.imp` means it could not be opened under that standard name in this protected archive. Do not infer from that alone that the original editor project never contained equivalent data; protected-map packaging can remove editor-only sources and hide imported assets.

## Native runtime effect coverage

The extractor inventory and native engine implementation ledger are intentionally separate. `crates/sim/data/castle-fight/native-effect-bindings.json` binds extracted stable source keys to native implementation IDs and inclusive map-version ranges; per-version numeric tuning lives under `crates/sim/data/castle-fight/<version>/`. This means a balance-only change can reuse the same behavior implementation while a semantic rewrite gets a new implementation ID/range.

Report the current 9.27 implementation gaps with:

```sh
python tools/wc3-map/native_effect_coverage.py --map-version 9.27
python tools/wc3-map/native_effect_coverage.py --map-version 9.27 --show-unimplemented
```

The report inventories normalized unit abilities, scripted unit spells, scripted building spells, and production-unit special mechanics directly from the resolved extraction. A row is `implemented` only when a binding covers the requested version; otherwise it remains explicitly `unimplemented`. The first playable-unit slice covers all eight distinct 9.27 unit-ability keys attached to the currently exposed Footman/Ranger/Catapult/Ice Troll Shadow Priest/Gryphon Rider roster, so the repository currently reports `8/398` distinct inventory keys covered for 9.27. The ledger includes explicit no-runtime bindings for the shared Channel marker (`A0CV`) and Ranger's zero-damage Barrage (`A03N`) rather than silently dropping them. Other currently unimplemented entries may likewise prove data-only/marker behavior, but that classification must remain explicit.

Use `--output-tsv <path>` when a reviewable snapshot is useful. For a newer extracted map, point `--resolved-dir` at that version's resolved output and use the matching `--map-version`; this keeps extraction data version-conscious without baking 9.27 paths into the coverage algorithm.

## Tests

```sh
python -m unittest tools/wc3-map/test_decode.py tools/wc3-map/test_resolve_base.py tools/wc3-map/test_native_effect_coverage.py
python -m py_compile tools/wc3-map/decode_map.py tools/wc3-map/lua_index.py tools/wc3-map/resolve-base-data.py tools/wc3-map/native_effect_coverage.py
```

A full end-to-end validation is `tools/wc3-map/extract-base-data.sh` followed by `tools/wc3-map/extract.sh` and inspection of both `summary.json` and `resolved/summary.json`. The current resolved summary requires zero unresolved inheritance anchors, missing referenced pathing textures, or unknown placed doodad rawcodes.
