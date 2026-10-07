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

Alternate Snowveil Fountain / Frost Launcher and its upgrade; Chilling Mushroom; Icy Tower; World Freezer. This is an implementation checklist, not a claim that those entities are playable.

## Icy Rocks / Polar Bear

- Entity/source: `h04F` (base `hbla`) upgrades Snowy Rocks and produces `n018` (base `nplb`). All ordinary stats, timing, attacks, classification and corpse data consume the retained catalog.
- Complete inventory: native Critical Strike `A00D` and Cleave `A063`; neither has a script cast, proxy, delayed callback, extra resource cost or custom AI. Critical chance/multiplier and cleave radius/fraction/mask are reproducibly projected from object fields.
- Cleave affects enemy ground units; the ordinary attack can still hit structures. The engine retains the separate mask rather than inferring effect eligibility from weapon eligibility.
- Shared synthetic tests distinguish primary-centered cleave from caster-centered effects; prove critical damage composition, melee-only behavior, misses, air/ally/outside/structure exclusions, and checksum/wire continuation across worker counts. Projection tests reject unsupported class qualifiers.
- Both models are delivered by the existing local pack. The shared upgrade/production lifecycle transports the resolved definition. Cleave masks change authoritative shape: bundle schema 10, checksum 27, snapshot 22.

## Glacier / Magnataur

- Entity/source: `h03W` (base `hbla`) upgrades Icy Rocks and produces `n016` (base `nmgw`). Ordinary data, protected overlays, ground sapper/ward classification, organic raisable/decaying corpse, production and repair/build timings use retained catalog definitions.
- Complete inventory: native critical `A04E`, cleave `A04D`, scripted Parasite order `A05E` invoking native War Stomp `A05D`. Both physical passives permit structures; this differs from the earlier chain members and is retained explicitly.
- Trigger: `RK` registers the enemy-ground-combat-sapper Parasite order, with primary nonhero/vulnerable/native spell eligibility. Approach/range comes from the parent order. The native child is cast immediately at the caster through `dummyCastImmediateFrom`; effect eligibility is independently enemy ground units, including heroes and non-sappers. Damage, radius and ordinary/hero stun durations come from the child object.
- Resources/timing: protected `xD` supplies the parent's cost/cooldown and makes the child free. Dummy recycling after the helper's lifetime does not delay or repeat the effect. Existing mana, cooldown sequence and absolute stun state are authoritative. No retreat or sleep is scripted in this handler.
- Presentation uses child `A05D` at the caster, while the parent retains order/resources. Parent Parasite does not create a damaging parasite or a summoned minion: runtime initialization and its callback own the actual effect.
- Verification: generic proxy area tests distinguish cast trigger from native effect, effect center from selected-target center, hero duration, air/ally/immunity/range/resource rejection, cooldown and worker/wire continuation. Generated projection includes both parent and child, checked against the retained tree. Compatibility becomes bundle 11, checksum 28, snapshot 23.

## Igloo / Hrimthrusa

- Entity/source: `h03U` (base `hbla`) produces `n011` (base `ntka`), the next authored command-card root. Ordinary missile attack, protected life/damage/armor/range/cadence, movement, collision, organic sapper/ward classification and raisable/decaying corpse use the shared source catalog.
- Complete inventory: native orb chance `A03X` invokes `A03W` Entangling Roots on a landed directed attack. Parent probability and targets are newly projected; child damage, duration, targeting and authoritative root/DOT transport already belong to the shared Ice Troll mechanic. Neither the orb nor its free child spends unit mana or runs a scripted spell/retreat.
- The proc targets units independently of the ordinary weapon's structure permission. Air targets are retained according to the parent/child evidence. Existing deterministic orb roll, live impact immunity, root/DOT timing and wire state apply; inherited static placeholder attack values are not used.
- Verification: catalog-wide ability closure and production definitions; shared triggered proc/roots tests and strict simulation Clippy. Local pack has the source unit/building models. No entity-specific tuning constants or new persistent fields were added.

## Modern Igloo / Angry Hrimthrusa

- Entity/source: `h06L` upgrades Igloo and produces `n02I`; all resolved/protected stats, costs, timing, ordinary missile attack, corpse data and command relationships use the catalog.
- Complete inventory: native Barrage `A0FL`, orb proc `A0FM` invoking shared roots `A03W`, spell resistance `A0AH`, legendary classifier `A07H`, visual legendary marker `A06V`, and anti-Hex/Devour protection `A070`. The existing legendary classification and spell-resistance bindings retain these hidden hooks.
- `xD` restores native Barrage cooldown. The ordinary weapon controls firing; Barrage has independent source-derived range, target mask, count and missile speed. Its damage fields are not interpreted as a separate damage budget. Secondary arrows are ordinary attacks and carry no inherited primary orb proc. Both proc chance and Barrage tuning remain reproducible projections.
- Runtime now discovers Barrage from the source inventory for unit and tower sources and resolves every arrow's speed by its retained version/ability, including restored flight. No unit-specific hand-authored stat table or persistent shape change was introduced.
- Verification: shared Barrage capacity/masks/critical/miss/immunity/homing tests, new unit-source and mixed-worker wire-flight regression using source definitions rather than copied tuning, catalog-wide closure, projection reproduction and workspace Clippy. Unsupported hostile mechanics outside the development slice remain dormant; legendary exclusions remain represented.

The Northern proc audit also found a shared importer bug: `A03W` explicitly has `nonhero` targeting, which the old physical-mask projection dropped. The shared root profile now retains that qualifier and the independent hero duration. A normal attack can hit a hero while its child roots fails. Synthetic tests prove this distinction and restored continuation; it applies to the already promoted Hrimthrusa and Ice Troll procs too. This profile-shape correction advances bundle/checksum/snapshot schemas to 12/29/24.

## Ice Troll Hut / Shadow Priest; Voodoo Lounge / Witch Doctor

- The existing `h03K` / `n015` root is already promoted; its source-owned upgrade now reaches `h03J` / `o00A` (base `odoc`). Both consume catalog/protected ordinary stats, production, repair and corpse definitions.
- Witch Doctor inventory: assassination-target classifier `A0CV`, native Frost Attack `A03C`, independent orb `A048` invoking its nonhero roots child `A047`, and native autocast Frost Armor `A040`. No scripted proxy, delayed callback, retreat or mana cost is attached to ordinary attacks. Frost Armor uses protected resources, ally retaliation selection and the existing reactive melee slow mechanic.
- Frost Attack reads retained map Misc movement/attack decreases and independent normal/hero object durations. Its payload survives directed missile flight, applies at live impact, rejects native immunity and misses, and coexists with the separate root roll. The tooltip's unit-or-building freezing promise does not override the actual unit-only parent/child masks.
- Native spellcasting now retains rational mana regeneration rather than requiring a rate divisible by the simulation frequency. Existing mana remainder transport provides exact accumulation.
- Shared synthetic tests cover simultaneous slow and roots, ordinary/hero duration, live immunity, misses and wire continuation. Catalog-wide closure covers the upgrade and hidden inventory. Profile/payload shape advances bundle/checksum/snapshot schemas to 13/30/25.


## Ice Claws / Wandigoo

- `h03T` produces `n012` (base `nwen`); catalog/protected definitions own ordinary melee attacks, stats, timing, organic sapper/ward classification, corpse lifetime and upgrade relationships.
- Complete inventory is native Pulverize `A059`; no resource use, script delay, dummy, marker or AI override. Its probability, damage, full/half radii and ground-enemy mask are projected from retained object fields.
- The native physical area proc rolls at released attacks separately from directed evasion, is centered on the attacker, and includes its primary target if within the appropriate radius. It bypasses ordinary armor points, retains the spell attack/armor table and permits magic immunity. It emits the native ability identity at the caster. Blizzard's [native ability description](https://classic.battle.net/war3/orc/units/tauren.shtml) identifies the attack-triggered area primitive; Castle Fight tuning comes exclusively from its retained object.
- Shared synthetic fixtures distinguish caster versus primary center, inclusive radii/full/half falloff, zero/full probability, directed evasion, immunity, air/ally/structure exclusions and saved-state RNG continuation. Catalog closure and strict workspace checks cover the promotion. Profile shape becomes bundle/checksum/snapshot 14/31/26.

## Frost Claws / Ancient Wandigoo

- `h043` upgrades Ice Claws and produces `n013` (base `nwns`). Ordinary attacks, stats, organic sapper/ward classification, corpse handling, production/build/repair costs and timings remain catalog-owned.
- Complete inventory: native Pulverize `A05A` and scripted Parasite parent `A05C` invoking Howl of Terror `A05B`. Pulverize consumes its own generated profile through the shared primitive.
- `RK` selects an enemy ground combat sapper for the parent order; native parent restrictions exclude heroes, invulnerability and magic immunity. Parent range/resources use object/protected fields. The immediate helper casts the child at the caster, with independent air/ground enemy effect eligibility including heroes and non-sappers. There is no effect delay, parasite summon, DOT, retreat or child resource commitment.
- Howl retains normal/hero durations, lowers armor and outgoing weapon damage, and replaces its own active modifier rather than stacking on repeated casts. Signed authoritative weapon modifiers coexist with existing positive support buffs; damage clamps at zero. The child identity and caster center reach presentation, while the parent owns mana/cooldown.
- Shared proxy area tests prove trigger/effect differences, center, signed outgoing weapon damage, hero expiry, ally/immunity/outside exclusions, resources and worker/wire continuation. Generated closure covers both parent and child. Bundle/checksum/snapshot become 15/32/27.

## Azure Nest / Azure Drake

- `h03I` produces `n010` (base `nadk`), with ordinary/protected flying splash weapon, armor, movement, sight, acquisition, collision, production/build/repair timings and source corpse classifications supplied by the retained catalog.
- Complete inventory is native Frost Breath `A03Y`. Its unit-only air/ground mask, normal/hero durations and map Misc slow rates consume the shared versioned Frost projection. No spell order, mana charge, scripted proxy, delay or special AI is attached.
- Ballistic splash retains its Frost payload independently of the source lifetime and applies slow to live eligible impact victims. Moving the original selected target does not move the impact point. Air and ground victims are eligible; friendly, outside, dead, immune and invulnerable victims receive no slow. Structure weapon permission does not expand the ability's unit-only mask.
- Shared synthetic flight tests prove splash center, a selected target leaving the impact area, hero expiry, ordinary weapon damage on a magic-immune victim without slow, allied/outside exclusions and in-flight wire restoration across workers. Existing splash geometry/falloff and catalog-wide closure tests also apply. Flight shape becomes bundle/checksum/snapshot 16/33/28.

## Crystal Palace / Ice Queen

- `h03S` produces `n014` (base `nhea`); all ordinary/protected weapon, stats, sapper/ward organic classification, corpse, production/build/repair and command-card data come from the catalog.
- Complete inventory: assassination-target marker `A0CV`, visual Sphere `A04A`, Parasite order `A044` invoking Frost Nova `A043`, orb `A046` invoking roots `A045`, and native Brilliance Aura `A042`. The production building also retains visual Spheres `A04B`/`A04C` and its shared production channel marker; these have no authoritative combat effect.
- Nova trigger is a ground enemy combat sapper. Unlike the earlier Parasite parents, this parent permits heroes. Protected parent resources/range apply; the child is free and immediate at the selected target. Native air/ground effect eligibility independently permits nearby heroes and non-sappers, and rejects immunity/invulnerability. Specific-target damage adds to area damage on the primary; the tooltip's primary-only wording does not replace native field composition. Slow uses native object durations and retained map Misc rates. Dummy recycling has no delayed gameplay effect or AI retreat.
- The orb has separate structure permission and its own child tuning. Roots now retain a distinct attack/movement-disable deadline, allowing ordered spellcasting. Structure roots preserve weapon disable and periodic damage in existing authoritative building status. Neither ordinary weapon permission nor the parent proc bypasses the child's nonhero/immunity rules.
- Brilliance is a friendly/self air/ground flat mana-regeneration aura, including native invulnerable recipients, with no percentage bonus or cast-cooldown suspension. Existing modifier identity prevents stacking from identical aura sources. Fractional regeneration retains its phase and remainder.
- Synthetic fixtures prove target-centered Nova and extra primary damage, hero cast eligibility/duration, effect exclusions, structure roots and continued casting during roots, mana-aura range/relation/cooldown behavior, and wire continuation. Catalog closure includes the visual building identities. Client inspection distinguishes roots and shows mana/weapon modifiers. Bundle/checksum/snapshot become 17/34/29.
