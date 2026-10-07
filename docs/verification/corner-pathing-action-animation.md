# Corner pursuit and action animation verification

Date: 2026-10-06. Content: Castle Fight 9.27/r1, `cf-native-dev-slice-r14`.

## Corner pursuit

A synthetic stationary enemy on the opposite side of a rectangular blocker reproduced
the reported corner jitter. A legal partial step toward a waypoint could undo the
previous recovery step before the unit aligned with its navigation cell. Integer
rounding made the remaining lateral correction especially easy to lose at low speeds.

Movement now checks the whole swept collision circle to the waypoint. The exact
integer segment/rectangle check preserves legal tangency and rounded corner clearance.
If that segment clips a blocker, alignment continues toward the current cell center.
The regression covers reflected approaches around all four corners, different attack
envelopes and speeds, collision steering, and identical results with one/four workers.
Existing adjacent-tower, siege, cage, diagonal, and objective-detour tests still apply.

## Attack and cast animation

The generated versioned catalog supplement derives action durations from recovered
`udp1`/`ubs1`, `udp2`/`ubs2`, and `ucpt`/`ucbs` in the registered extraction tree's
`resolved/object-fields.tsv`. It rounds the exact decimal sum once using the engine's
tick rate, retains source identity, and participates in the generated source manifest.
The runtime rejects a projection produced for a different tick rate. Catalog-wide
validation checks direct and produced unit definitions against that projection.

Ordinary attacks start authoritative recovery, including misses and killing attacks.
Attack recovery uses the selected attack slot, is capped to its cooldown, and follows
attack-speed modifiers. Successful ordered casts start cast recovery; failed casts
leave no lock. Recovery anchors ground and air units through intentional movement,
separation, and collision resolution. All moving units see anchored collision
reservations, including actors with lower SimIds. The exclusive expiry tick resumes
movement normally.

Support autocasts may replace attack recovery with cast recovery without opening a
movement tick. Already-authored script retreat pauses cap cast recovery; animation
playback fits the complete clip into that pause instead of changing the script's
retreat/sleep timeline. Autonomous Phoenix Fire remains independent of caster orders
and animation recovery. Existing Frost Armor and Warlock AI integration tests cover
those distinctions without copying map timing values into new tests.

Imported clips seek and pause according to the authoritative interval, including
restore/rejoin without the original event. Position interpolation excludes recovery
intervals when snapshots skip ticks. An animation-player test checks clip progress,
attack-to-cast preemption, and removal of paused action poses before walking; leaving
an action uses an immediate transition so a residual action pose cannot slide.

The action profile and state are included in content/canonical hashes, production,
construction/upgrades, resurrection definitions, and snapshots. The original 2026-10-06
change shipped at content bundle schema 7, checksum schema 22, snapshot schema 17; at that
point spell effects still resolved before the cast animation.

### Cast-point extension — 2026-10-07

Castle Fight 9.27/r1 content revision `cf-native-dev-slice-r28` now retains the extracted
unit cast point (`ucpt`) separately from the point-plus-backswing duration. A positive cast
point commits authoritative pending-cast state and starts the cast action immediately. The
selected ability, sequence, target identity/point, and absolute release tick survive checksums,
snapshots and rejoin. Mana/cooldown/effect commitment occurs at the cast point. Trigger-only
AI predicates are not re-run after wind-up, while removed targets and incapacitated casters can
still cancel release. Movement remains anchored through the backswing unless a scripted
post-effect retreat truncates it. Building casts and zero-cast-point actions remain immediate.

Magnataur `n016` now prefers its imported `Spell Slam` sequence over unrelated `Spell Attack`
variants. Warlock `n005` uses the Medivh model's `Spell Attack` sequence; `Morph` and `Morph
Alternate` are not cast candidates. Generic area-impact presentation no longer invents an
extra burst for native `AreaStun` proxies such as Magnataur `A05D`, whose retained map object
explicitly clears stock War Stomp caster art. The extension advances compatibility to content
bundle schema 19, checksum schema 36, and snapshot schema 31.

## Validation

- Required Cargo wrapper used for all compile-heavy checks.
- `test --workspace --quiet`: 810 passed, five asset/GPU-dependent tests ignored;
  447 simulation tests and 260 client tests passed.
- `clippy --workspace --all-targets -- -D warnings`, formatting and diff checks passed.
- Nine runtime catalog generator tests, including supplement/source-manifest reproducibility and exact rounding.
- Mechanic regressions cover attack/cast expiry, target death, failed casts, secondary
  attack timing, autocast preemption, fixed collision reservations, and wire restore
  across worker counts.
- Radius and traffic profiling uses 700 units, one/four workers, 10 warmup ticks and
  60 measured ticks. Final debug-profile results:

| Scenario | Workers | ms/tick | Movement intent ms/tick | Collision ms/tick | Checksum |
| --- | ---: | ---: | ---: | ---: | --- |
| Radius | 1 | 5.207 | 0.801 | 0.918 | `c14bce71b07f1526` |
| Radius | 4 | 4.071 | 0.408 | 0.747 | `c14bce71b07f1526` |
| Traffic | 1 | 5.066 | 0.353 | 1.276 | `84d993a39b25e9ed` |
| Traffic | 4 | 4.585 | 0.188 | 0.911 | `84d993a39b25e9ed` |

These short local measurements verify execution cost and worker-independent outcomes;
there is no claim of a performance improvement. The screenshot scene was not manually
replayed in the GUI.
