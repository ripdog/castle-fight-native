# Attack windup verification

Date: 2026-10-06. Castle Fight 9.27/r1, `cf-native-dev-slice-r15`.
Content bundle schema 8; checksum schema 24; snapshot schema 19.

## Behavior and source ownership

Ordinary units now begin their authoritative attack animation before resolving damage or launching
missiles. The generated catalog supplement projects independent `udp1`/`udp2` damage points from
the registered extraction tree's `resolved/object-fields.tsv`; the existing point-plus-backswing
totals remain separately projected. Each decimal source duration is rounded once to simulation
ticks. The projection retains map/release/extraction identity, validates simulation Hz, and is
covered by the source manifest and content gameplay hash. No per-entity timing tables were added.

Windup occupies the existing attack cycle. Cooldown starts with the animation, so uninterrupted
release-to-release cadence stays the same. The first strike occurs after the authored windup.
Both attack slots and attack-speed modifiers retain their independent timing. The release does
not restart the clip or recovery. A point longer than an available cycle is capped so it cannot
extend that cycle; zero-point synthetic fixtures retain immediate resolution. Reactive slows
retain their cooldown effect while elapsed windup is deducted from the remaining cycle.

The committed target and absolute release tick are authoritative `StatusState`, included in
checksums and snapshots. Ground/air actors stay anchored while acting. Stun, disabling orders,
target loss/retarget, invalid release range, source death, and ordered-cast preemption cannot
redirect a committed release to a different target. Damage, misses, procs, retaliation, missile
creation and missile travel still belong to simulation combat, independently of animation
callbacks. Autonomous native spell movers and map-script retreat/sleep control flow are unchanged.

Imported animation playback already follows the authoritative interval and now sees that interval
before the attack event. Fallback weapon animation also starts on the action interval instead of
restarting at release. This is a reusable ordinary-attack mechanic change; the per-entity fidelity
checklist was used to retain slot semantics, hidden/native/script independence, delayed authority,
and negative-case coverage rather than adding tooltip-driven unit implementations.

## Validation

- Catalog-wide source projection/property/hash checks, nine runtime generator tests, and exact
  supplement/source-manifest regeneration checks.
- Synthetic mechanic regressions cover ground/air, melee/ranged, unchanged repeated cadence,
  no early damage/projectiles, independent secondary points, haste/slow, target loss/range,
  stun/cast interruption, and one/four-worker equivalence after wire restore mid-windup.
- Existing imported damage/projectile assertions now wait for the authored release/impact rather
  than assuming an immediate attack. Existing Frost Armor reactive cooldown and spell/AI tests
  continue to apply.
- Final workspace tests: 741 passed; four existing asset/install-dependent tests ignored.
- All-target Clippy with warnings denied, formatting, diff checks, and
  client/server debug builds are run through the required Cargo wrapper.

A short existing mixed synthetic benchmark used 700 units, 10 warmup ticks and 60 measured ticks:

| Workers | ms/tick | Combat ms/tick | Checksum |
| --- | ---: | ---: | --- |
| 1 | 7.775 | 0.115 | `4c85c6e442ff4d6c` |
| 4 | 5.624 | 0.094 | `4c85c6e442ff4d6c` |

This is a cost/determinism smoke check, not a before/after performance claim. The existing mixed
benchmark primarily uses zero-point synthetic units; active windup correctness is covered by the
mechanic regressions. Imported animation-player tests validate timeline seeking, but no manual
archer scene was visually replayed for this change.
