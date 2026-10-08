# Orc implementation

Work proceeds across the retained 9.27/r1 command card, starting at the top left and completing each root's upgrade chain before moving right. The retained extraction tree is `8ea806dca331ff254995e94e6f0baf225a14bf10`. Tuning and relationships remain source-owned, version-scoped catalog projections. Only completed identities are promoted.

## Fighters' Hall / Grunt

- Entity/source: `h029` (base `hbla`) produces `o002` (base `ogru`), command-card root Q. Ordinary life, regeneration, armor, movement, collision, acquisition, weapon and production/build/repair/economy data consume `resolved/units.tsv`, `protected-unit-stats.tsv`, `production-unit-attacks.tsv`, `production-buildings.tsv` and the retained catalog supplement. Placeholder object combat values are replaced by their protected runtime overlays.
- Complete inventory: neither unit nor building has an ability, hidden marker, proxy, scripted cast, delay or AI override. The ordinary ground melee weapon separately permits structures.
- Corpse/classification: organic combat sapper/ward, with source-owned raisable/decaying corpse and exact decay deadlines. Production, upgrades, source model/icon identity and authoritative state transport use existing shared primitives.
- Verification: catalog-wide definition/production/ability closure; mixed-race menu/admission and wire restoration across worker counts now includes Orc. No new persistent shape is introduced.
- Orc becomes selectable after this first completed unit. Further buildings/upgrades appear only after their full behavior is promoted. No Grunt-specific interaction is left dormant.

Validation: all 472 simulation tests, nine runtime-catalog projection tests, formatting and strict workspace/all-target Clippy passed (`.local-tools/orc-validation/grunt-*.log`).

## Advanced Fighters' Hall / Veteran Grunt

- Entity/source: `h02U` (base `hbla`) upgrades Fighters' Hall and produces `o004` (base `ogru`). All ordinary stats, protected combat overlays, source weapon masks, production, construction, repair, economy and corpse data use the retained catalog.
- Complete inventory: native Critical Strike `A02V`, projected reproducibly from retained object fields. It applies to enemy air/ground units independently of the ordinary melee weapon's structure permission. No active spell, proxy, hidden ability, resource commitment, delay or scripted AI override.
- Organic combat sapper/ward; raisable/decaying corpse. Existing source presentation, upgrade/production lifecycle and authoritative passive/RNG transport apply. No persistent shape change.
- Verification: shared directed-critical tests cover deterministic rolls, misses and target masks; catalog-wide ability closure and production definitions cover the promotion.

## Superior Fighters' Hall / Axemaster

- Entity/source: `h031` (base `hbla`) upgrades Advanced Fighters' Hall and produces `o005` (base `ogru`). Retained ordinary/protected combat data, construction, training, repair, economy, organic sapper/ward classification and raisable/decaying corpse all consume the catalog.
- Complete inventory: native critical `A02W` and Pulverize `A02P`; no active cast, proxy, hidden ability, script delay, resource cost or special AI. All parameters are generated from retained fields.
- Critical modifies landed directed attacks on eligible units; Pulverize rolls independently at released attacks, uses the caster center and its separate ground-unit mask, and permits spell immunity. Ordinary structure targeting expands neither passive's unit-only targets. Its identical full/half radii leave no half-damage annulus; the shared primitive preserves that source geometry.
- Verification: shared critical and Pulverize synthetic fixtures prove center, falloff, miss independence, masks, immunity, deterministic RNG and wire continuation; catalog-wide closure covers both abilities and the full upgrade chain. No new persistent shape.

## Shamanic Tent / Shaman

- Entity/source: `h02B` (base `hbla`) produces `o003` (base `oshm`), next command-card root W. Ordinary/protected ranged magic weapon, stats, movement, acquisition, collision, construction/training/repair/economy, organic sapper/ward classification and raisable/decaying corpse come from the retained catalog.
- Complete inventory: assassination-target marker `A0CV` and native Inner Fire `A02M`. `udaa` explicitly makes this ability active on creation. Its native `Ainf` behavior grants damage, armor and health regeneration as one timed bundle; `B009` is the buff family. Source normal/hero durations, native autocast range, cast range, resources, protected cooldown and rational mana regeneration are projected from retained fields. No script cast, dummy, delayed callback, retreat or special AI is attached.
- Native autocast selects an unbuffed friendly combat unit, including air, mechanical and magic-immune units; enemy, invulnerable, idle or out-of-acquisition-range candidates cannot spend its resources. Cast windup and same-phase revalidation use the shared ordered-ability scheduler. Existing buff family excludes duplicate autocasts and complete replacement prevents stacking across different granting abilities.
- Native buff identity/polarity/transfer eligibility is authoritative alongside armor, outgoing damage and regeneration. Intrinsic auras and permanent script bonuses remain separate. Existing modifier source identity drives inspection and retained native buff art. No gameplay relies on presentation timers.
- Verification: synthetic stat-buff fixtures cover separate acquisition/cast limits, eligibility, resource commitment, simultaneous casters, full bundle replacement, independent hero expiry, outgoing weapon damage, regeneration and wire continuation across workers. Generated projection and catalog-wide ability/production closure cover the source. Compatibility advances to bundle/checksum/snapshot 25/42/37.
- Native spell behavior is identified by the [Blizzard Priest reference](https://classic.battle.net/war3/human/units/priest.shtml); all Castle Fight tuning comes from retained 9.27/r1 evidence.

Validation: all 477 simulation tests, 16 native projection tests, formatting and strict workspace/all-target Clippy passed (`.local-tools/orc-validation/shaman-*.log`). The synthetic cast-windup fixture also proves that combat ending during a pending cast does not cancel an otherwise valid recipient, while a competing buff does prevent duplicate resource commitment at release.
