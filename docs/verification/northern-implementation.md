# Northern implementation

Northern is selectable for retained Castle Fight 9.27/r1 once Snowy Rocks is promoted. Only audited, promoted buildings enter its source-owned menu. Work proceeds by command-card row, left to right, completing each upgrade chain before the next root. The retained extraction tree is `8ea806dca331ff254995e94e6f0baf225a14bf10`; tuning remains in generated version-scoped projections.

## Snowy Rocks / Frost Wolf

- Entity/source: `h049` (base `hbla`) produces `n017` (base `nwwf`), first authored Northern tier. Upgrade links remain source-owned and are exposed only after the destination is promoted.
- Ordinary attack, protected life/armor/damage/cadence, movement, acquisition, collision, production/build/repair timing and sight use the shared resolved catalog and protected overlays. Static placeholder unit values are not runtime tuning.
- Complete inventory: native Critical Strike `A062`, with no active spell, proxy, hidden marker, script delay or AI override. Its inherited target mask applies to units; ordinary weapon attacks can separately hit structures. Values are projected from retained object fields.
- Corpse/classification: organic combat sapper/ward; retained can-raise/decay behavior, decay deadlines and source identity use existing authoritative corpse transport.
- Presentation: ordinary source identities flow through existing asset lookup; critical strike emits the existing attack event. No new persistent state or snapshot schema is introduced.
- Verification: catalog-wide definition/production and ability-closure checks; shared directed-critical mechanic tests; public mixed-race menu/admission and wire restoration fixture includes Northern across worker counts.
- Unsupported interactions: hostile systems outside the current development content subset remain unavailable. No Frost Wolf-specific spell behavior is left dormant.

## Remaining command-card order

Finish Glacier / Magnataur; then Igloo and Modern Igloo; Ice Troll Hut and Voodoo Lounge; Ice Claws and Frost Claws; Azure Nest; Crystal Palace; alternate Snowveil Fountain / Frost Launcher and its upgrade; Chilling Mushroom; Icy Tower; World Freezer. This is an implementation checklist, not a claim that those entities are playable.

## Icy Rocks / Polar Bear

- Entity/source: `h04F` (base `hbla`) upgrades Snowy Rocks and produces `n018` (base `nplb`). All ordinary stats, timing, attacks, classification and corpse data consume the retained catalog.
- Complete inventory: native Critical Strike `A00D` and Cleave `A063`; neither has a script cast, proxy, delayed callback, extra resource cost or custom AI. Critical chance/multiplier and cleave radius/fraction/mask are reproducibly projected from object fields.
- Cleave affects enemy ground units; the ordinary attack can still hit structures. The engine retains the separate mask rather than inferring effect eligibility from weapon eligibility.
- Shared synthetic tests distinguish primary-centered cleave from caster-centered effects; prove critical damage composition, melee-only behavior, misses, air/ally/outside/structure exclusions, and checksum/wire continuation across worker counts. Projection tests reject unsupported class qualifiers.
- Both models are delivered by the existing local pack. The shared upgrade/production lifecycle transports the resolved definition. Cleave masks change authoritative shape: bundle schema 10, checksum 27, snapshot 22.
