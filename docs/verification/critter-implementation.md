# Critter implementation

Retained Castle Fight 9.27/r1 extraction tree: `8ea806dca331ff254995e94e6f0baf225a14bf10`. Builder `X0Z0`, registration `pK` and the command-card positions define the order. Complete each root and its upgrade before moving right. This source race is campaign-only: that catalog metadata and its exclusion from the source STANDARD list remain intact. Native content selection supports the newly implemented race after its first root is verified.

## Mudcrab Burrow / Mudcrab

- Identity/source: runtime r111; `h0Z1` (`hbla`) produces `n0Z1` (`nscb`). Protected body/building life, armor, attack damage and movement override static sentinels. Both ordinary weapon slots, Normal melee attack and independent ground/structure mask, acquisition/collision, regeneration, construction/repair/economy/production and the source upgrade edge consume retained versioned catalogs. Command slot is the source top-left root.
- Complete inventory/control flow: both authored inventories are empty. Literal script references close to `pK` registration, protected statistics, object/upgrade metadata and initialization. No hidden/proxy ability, delayed cast, train-finish bonus or decoration factory is installed. Native sapper/ward classifications remain separate from ground target eligibility. Organic corpse creation, decay and raise eligibility consume the original death fields. Common trained/entered combat orders use the existing authoritative production and lane-route lifecycle.
- Presentation/verification: exact source crab and burrow models, skins/scales, ordinary weapon/lifecycle clips and UI are delivered by the local asset pack. Catalog-wide checks cover source stats, both weapons, complete inventory, corpse, economy, versioned command slot and upgrade links without copying tuning values into tests. No new timed state or mechanic is invented. Critter becomes selectable with this completed first root, retaining its source campaign flag.

Campaign import correction: the extracted default-combat attack report omits campaign bodies. Missing enabled primary weapons now derive from the already retained `units.tsv` and protected overlays, preserving existing report rows and never arming an object with its weapon disabled. A synthetic importer fixture proves protected precedence, static fallback and disabled-weapon rejection. Corpse/economy/production reports already include the campaign race.

Validation: 34 catalog/importer checks, ten projection checks, strict workspace/all-target Clippy, source-manifest reproduction, formatting and diff checks passed. Critter presentation closure has zero findings (13 source entities, 23 model/dependency bindings). Logs: `.local-tools/orc-validation/mudcrab-*`.


## Rockcrab Burrow / Rock Crab — Mudcrab upgrade

- Identity/source: runtime r112; `h0Z2` (`hbla`) upgrades the source top-left root and produces `n0Z2` (`nsc2`). Protected statistics, both weapon slots, Normal ground/structure melee attack, acquisition/collision/movement, regeneration, organic raisable/decaying corpse and independent production/construction/repair/economy consume retained catalogs. The authored precursor/upgrade edge controls menu access and upgrade transport.
- Complete inventory/control flow: neither object has abilities. Script closure remains registration, metadata/upgrade registration and protected statistics; no proxy, bonus, factory or extra clock is added. Native sapper/ward and ground target classifications remain distinct. The existing upgrade lifecycle retains owner/team/production state and resolves the upgraded body independently.
- Presentation/verification: exact source crab variant/skin/scale and Temple of Tides building art, ordinary/lifecycle clips and UI are delivered. Catalog-wide checks and the shared production/upgrade machinery cover these definitions without copied tuning values.

Validation: 34 catalog checks, strict simulation/all-target Clippy, formatting, diff checks and source-manifest reproduction passed. Logs: `.local-tools/orc-validation/rockcrab-*`.


## Spinecrab Tidepool / Spiny Crab

- Identity/source: runtime r113; `h0Z3` (`hbla`) produces `n0Z3` (`ncrb`), the next root to the right. Protected statistics, Pierce missile delivery/speed, independent primary ground/air/structure mask, disabled secondary weapon, acquisition/collision/movement, regeneration, organic raisable/decaying corpse and independent production/construction/repair/economy consume retained catalogs. The source marks this line anti-air; it has no upgrades.
- Complete inventory/control flow: both inventories are empty and all literal references close to registration/protected metadata. The native Sludge Missile art does not grant a poison or slow effect. No proxy, trained-unit bonus, factory, spell or mana loop is added. Sapper/ward classification remains separate from ordinary ground targeting and corpse flags.
- Presentation/verification: exact source Spider Crab model/skin/scale and Defiled Fountain building art, Sludge Missile, ordinary/lifecycle clips and UI are delivered. Existing authoritative homing flight and catalog-wide checks cover complete ordinary definitions and worker-independent missile continuation.

Training-card correction: Spiny Crab has no authored hotkey. Production definitions now preserve optional source hotkeys; keyboard handling and tooltip shortcuts skip absent bindings. A catalog-wide check validates both present and absent source bindings. Content bundle schema advances to 66 for the typed definition change.

Validation: 35 catalog/importer checks, strict workspace/all-target Clippy, formatting, diff checks and source-manifest reproduction passed. Presentation closure remains zero findings. Logs: `.local-tools/orc-validation/spiny-crab-*`.


## Hermit Mound / Hermit Crab

- Identity/source: runtime r114; `h0Z4` (`hbla`) produces `n0Z4` (`nhmc`), the third first-row root. Protected life, armor, movement and ordinary Normal melee damage/cadence, independent ground/structure mask, disabled secondary weapon, acquisition/collision, regeneration, raisable/decaying organic corpse and production/construction/repair/economy consume retained catalogs. It has no upgrades.
- Complete inventory/control flow: both inventories are empty; all literal references close to source registration, metadata and protected initialization. Native armor does not imply Hardened Skin or damage block. No hidden/proxy spell, trained-unit bonus, factory, resource clock or extra authoritative state is added. Sapper/ward remains independent of ground target eligibility and organic corpse behavior.
- Presentation/verification: source Hermit Crab and Altar of Depths models/skins/scales, ordinary/lifecycle clips and UI are delivered. Catalog-wide checks validate resolved ordinary data, classification, inventory and versioned panel/economy/production relationships.

Hermit range discrepancy: the inherited `nhmc` UnitWeapons `rangeN1` is the literal SLK dash; the map enables the primary weapon but neither the object delta nor `jP`/`xD` supplies a range. Preserve that native integer zero rather than inventing ordinary melee tuning. Numeric dash decoding is corroborated by [Warsmash's integer field reader](https://github.com/Retera/WarsmashModEngine/blob/main/core/src/com/etheller/warsmash/units/HashedGameObject.java); physical-contact geometry by its [unit distance implementation](https://github.com/Retera/WarsmashModEngine/blob/main/core/src/com/etheller/warsmash/viewer5/handlers/w3x/simulation/CUnit.java). These are compatibility corroboration, not independent tuning sources. A shared zero-range melee contact envelope uses retained collision radii, with canonical coordinate rounding tolerance; the original definition stays zero. Synthetic fixtures cover pursuit/release, unit and footprint-edge boundaries, ranged/positive-range isolation, and wire continuation with different workers. The importer distinguishes a literal dash from missing cells/protection sentinels and still rejects disabled weapons.

Validation: all 700 simulation tests, all 206 map-tooling tests, strict workspace/all-target Clippy, source-manifest reproduction, formatting and diff checks passed. The complete imported battle also retains worker parity. Logs: `.local-tools/orc-validation/hermit-*` and `critter-map-tests.log`.


## Tidal Cairn / Carapace Aura

- Identity/source: runtime r115; utility building `h0Z5` (`hbla`) occupies the next source root at (0,1), with no upgrade, weapon or production body. Protected life, native Fortified armor/regeneration, build/repair/economy and command slot/hotkey consume versioned extraction.
- Complete inventory/control flow: original `A0Z1` derives from `AHad`. The reproducible native-aura projection retains level-one flat armor, radius, both weapon classes and the complete friendly air/ground/self/vulnerable/invulnerable mask, plus source zero linger and `BHad` art. It does not affect buildings. No cast, mana resource, hidden proxy, level controller, repair bonus or upgrade tree is installed. `pK` extra tags are metadata; literal ability references only bootstrap. The common finish callback has no Critter-specific hidden grant.
- Shared engine completion: ordinary passive auras now accept completed live building sources at footprint centers, without requiring StatusState on an unarmed building. Duplicate source abilities refresh one modifier. Timer-phase mana sampling supports the same sources with existing canonical fixed-point remainders. Construction activation/removal, death/allegiance changes, allied air/mechanical/invulnerable bodies, enemy/outside/structure rejection, fractional mana and wire continuation are covered by synthetic fixtures. No authoritative shape changes or new clocks are introduced.
- Presentation: source Tomb of Relics model/skin/scale, lifecycle clips, native Carapace/Devotion icon and actual recipient aura art remain versioned asset bindings. The inherited Blacksmith upgrade tooltip does not create upgrades absent from inventory/control flow.

Validation: strict workspace/all-target Clippy, formatting and diff checks passed; 701 simulation checks passed (the broad imported battle already passed in the Hermit chunk), including 35 catalog/import checks and both new shared aura fixtures. Native tuning/carrier/building-mechanic/buff-visual/source-manifest projections reproduce. Critter presentation remains zero findings. Logs: `.local-tools/orc-validation/tidal-cairn-*`.
