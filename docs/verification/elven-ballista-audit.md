# Ballista / Highelf Siege Factory fidelity audit

## Identity and evidence

- Entity: Ballista `e005`, produced by Highelf Siege Factory `h070`.
- Map/release: Castle Fight **9.27 r1**, retained extraction identified by
  `docs/original_map/releases.json`; native content revision remains unchanged for
  coordinator integration.
- Base objects: `e005` inherits `ebal`; `h070` inherits `hbla`.
- Stable IDs: unit `0x10000014`, production building `0x20000014`.
- Elven remains unselectable. No lobby, race setup, release registry, content
  revision, or schema-number change is part of this branch. Coordinator must
  perform the global compatibility/schema bump before releasing the combined work.

Authoritative evidence under `docs/original_map/extracted/`:

| Evidence | Use |
| --- | --- |
| `resolved/units.tsv`, `resolved/buildings.tsv` | Identity, classifications, movement, acquisition, regeneration, armor classes, costs, construction and attack availability |
| `resolved/protected-unit-stats.tsv`, `resolved/effective-unit-stats.tsv` | Script-restored life, armor, ordinary damage/cooldown/range; static protected placeholders are not runtime tuning |
| `resolved/production-buildings.tsv` | Producer/child link, spawn interval, ownership-independent definition, footprint, tier and siege metadata |
| `resolved/production-unit-attacks.tsv` | Enabled primary siege `mline`; inherited second `aline` attack is **unavailable**, not a second executable attack |
| `resolved/object-fields.tsv` | Resolved inherited minimum range, spill distance/radius, damage loss, splash mask, repair timing, models, missile arc, icons and command slots |
| `resolved/production-unit-corpses.tsv`, `resolved/death-decay-constants.tsv` | Mechanical, cannot-raise but decaying remains and their delayed decay/expiry |
| `script/function-rawcodes.tsv`, `script/rawcode-reference-sites.tsv` | Complete rawcode-reference trace |
| `script/race-building-semantics.tsv`, `script/unit-object-metadata.tsv` | Production registration, siege classification, income, tier, command/presentation identity |

`tools/wc3-map/build_runtime_catalog.py` now reproducibly projects resolved
`usd1`, `usr1`, `udl1`, `uamn`, and `ua1p` for retained line weapons into the
version-scoped supplement. It reads the registered retained Git tree, preserves
its identity and source-object hash, rejects lossy distances, and converts damage
loss with exact decimal arithmetic. The generated source manifest changes only
its evidence hash, not revision/schema numbers. No hand-authored Ballista tuning
was added. Catalog-wide tests compare imports to these projections.

## Script trace and ability inventory

The retained Lua and `/tmp/elven-next-source.lua` were inspected using the numeric
FourCC identities as well as the extraction's function/reference index:

- `H`: protected object bootstrap rows; does not implement a combat spell.
- `AK`, retained byte offset `3571057`: registers `h070` producing `e005`, wrapped
  by income-factor, siege and tier metadata. No precursor/upgrade edge or special
  production callback is registered for this factory.
- `xO`, retained byte offset `3918675`: registers the effective unit statistics.
- `ensureUnitObjectMetadataRegistered`, retained byte offset `4136405`: retains
  producer/child names, icons, tooltip, costs, production time and mechanical /
  non-caster / non-air classification.
- `jP`, retained byte offsets `4266951` and `4270065`: protected runtime stat
  restoration for the child and factory, reconciled against
  `resolved/protected-unit-stats.tsv` rather than decoding tooltip numbers.
- No entity-specific ability-add/remove or delayed spell control flow appears in
  the indexed rawcode mutator traces. Both resolved `uabi` lists are empty;
  Ballista explicitly removes inherited `Aimp,Ault`, and the factory removes
  inherited `Abds`. There is no proxy, hidden protection ability, mana pool,
  autocast, proc, aura, active spell, retreat/sleep callback or secondary cooldown
  to synthesize. “Piercing Bolts” is the ordinary **weapon**, not an ability.

The child uses generic autonomous siege acquisition/pursuit and production keeps
its owner's identity. The factory has no ordinary attack. Existing versioned
construction/repair/economy APIs consume its retained metadata.

## Ordinary weapon contract

- Delivery: typed `AttackDelivery::Line`, stable tag **5**, not ballistic splash.
- Trigger eligibility: ordinary primary attack mask and inclusive minimum / maximum
  range; acquisition/retention and final intents both exclude the inner region.
- Approach: ordinary target attack envelope, with no artificial retreat to maximum
  range when already in the legal band.
- Effect origin: primary missile follows its selected target; arrival captures
  that target's post-movement position. The fixed spill ray runs from the launch
  point through that arrival point, continuing **behind** the main target.
- Effect eligibility: spill mask is independent of primary mask. Enemy ground
  units and permitted building footprints intersect the travelling strip. Air,
  allies, off-strip points, already-passed points, beyond-end points and previous
  victims are excluded. Structures are retained from the splash mask even though
  the tooltip only mentions ground units.
- Resources/timing: ordinary attack cooldown commits on launch. Primary and spill
  have distinct authoritative clocks; spill sweeps at projectile speed rather than
  applying a circle or instantaneous damage to the whole line. Missing/dead primary
  cancels spill; source death does not cancel launched projectiles.
- Ordering: projectile `SimId`, then longitudinal collision position and victim
  `SimId`; damage retention advances after each collision. Each victim is hit once.
- Corpse/classification: mechanical repair metadata is imported; mechanical remains
  are created/decay independently of resurrection eligibility and cannot be raised.
  The corpse fix applies the same extracted death-type rule to all promoted units.

## Persistence and presentation

Line source/team/art identity, target, full delivery metadata, clocks, retained
damage, launch point, fixed spill origin/destination and ordered hit history are
canonical checksum and wire snapshot state. Restore works during primary flight
and during spill, across worker counts. Projectile art survives source removal.

The existing all-production-unit/building asset generator consumes resolved
`umdl`, `usca`, icons, `ua1m` and `uma1`; no separate Ballista asset table is needed.
It resolves the imported custom `Ballista.mdl`, factory HumanShipyard model and
`Abilities\\Weapons\\BallistaMissile\\BallistaMissile.mdl` from retained evidence.
Client line presentation homes on the primary until impact, switches to the fixed
spill segment and its own clock, retains the authored primary arc, and makes spill
straight. Ranged fallback presentation never inherits the ballistic fallback arc.
Imported projectile orientation, team-color/model-local particles and animation
handling remain on the existing generic asset path. Gameplay never reads visuals.

## Tests and validation

Focused synthetic tests in `simulation/line_projectiles/tests.rs` prove:

- Separate primary/spill masks, timed directed positive hits and negative geometry,
  ally/air/building exclusions, exact minimum-range boundary and forced-intent
  revalidation.
- Longitudinal / stable-ID collision ordering and independent projectile ordering
  even after reversing ECS insertion order.
- Moving primary versus fixed spill, moving victims not re-entering passed sweeps,
  no duplicate hits, diagonal strip and footprint intersection (not circular AoE).
- Source death, primary invalidation, retained art identity and checksum sensitivity,
  wire continuation during both phases and worker-count independence.

Reusable mechanical-corpse tests prove decay/expiry and restore without
resurrection. The synthetic fixture supplies positive synthetic build-time repair
metadata rather than copying map values. Client presentation tests prove segment
clocks/arc, straight fallback and retained art after source disappearance.

Validation at handoff:

- `python3 tools/wc3-map/test_build_runtime_catalog.py`: **7 passed**, including
  exact regeneration of committed supplement and source manifest, and working
  extraction alias/evidence-hash agreement.
- `tools/cargo-interactive test -p castle-fight-sim`: **305 passed** (including
  **11 line-projectile tests** and the corrected mechanical-corpse fixture).
- Saved workspace run: client **210**, protocol **9**, server library **19** and
  server binary **1** passed; its only simulation failure was the now-corrected
  synthetic repair metadata. No redundant full-workspace rebuild was needed.
- Saved workspace check and Clippy passed; final simulation all-target Clippy with
  `-D warnings`, formatting and diff whitespace checks passed after the fixture fix.

## Explicit shared-engine fidelity limitations / integration notes

These are existing generic native-path precision/compatibility issues, not special
Ballista overrides, and are intentionally not expanded into unrelated refactors:

- Ordinary imported damage uses rounded mean damage rather than Warcraft's per-hit
  damage-dice distribution; fractional armor is rounded on the existing import
  path. Line damage uses that same integer ordinary-attack contract.
- Movement/projectile speed uses integer subunits per simulation tick; movement can
  truncate rates that do not divide evenly. Retained Ballista missile speed divides
  exactly. General decimal/timing import still rounds through existing `f64`
  helpers; the new line projection itself refuses lossy conversion.
- Native ordinary attacks do not model authored animation damage-point/backswing
  delays; primary launch timing and homing arrival obey the existing provisional
  directed-projectile contract, not a new Warcraft animation scheduler.
- The authoritative victim domain has combat units/buildings, not independent
  items/trees/destructibles; retained mask tokens outside that domain remain dormant.
  Mechanical classification is executable; sapper/ward tags do not introduce a
  separate neutral-object simulation or previously unsupported hostile mechanics.
- Line on-hit passives are explicitly rejected rather than silently dropped.
  Ballista has none, so this does not restrict this entity.
- Factory tech prerequisites / completed Elven race availability, coordinated
  content/schema bumps and combined-branch gates belong to coordinator integration.
