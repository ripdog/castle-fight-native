# Elven tower fixtures — Castle Fight 9.27 r1

## Status

Arcane Tower (`h014`) and Obelisk of Light (`h005`) are simulation/content fixtures. This audit does **not** declare the complete Elven race playable. The integration gates below remain open; retaining bindings, compiling art, or passing synthetic tests does not close them.

## Authoritative evidence

`tools/wc3-map/build_native_carriers.py` reads the release registration in `docs/original_map/releases.json` and its retained extraction Git tree, not the mutable extraction alias. The generated `crates/sim/data/castle-fight/9.27/native-carriers.json` retains map version, release revision, extraction tree identity, and input digests.

The inputs are `resolved/object-fields.tsv`, `resolved/runtime-system-mechanics.tsv`, and `resolved/objects.tsv`. The runtime row `obelisk-of-light-cleansing-light` supplies carrier installation/removal semantics and the exact permanent-ability removal list. Object fields supply native `A015` Barrage, `A000` Phoenix Fire, its buff, regeneration, and the removable Holy Bonus health overlay. The projected values remain authoritative; this audit deliberately does not duplicate costs, damage, ranges, cooldowns, or regeneration values.

The content dependency closure includes the actual ability-effect `A000`, not merely the building's `A07W` script marker. Binding identities `warcraft-barrage-v1` and `warcraft-persistent-carrier-v1` have explicit stable implementation tags and source IDs.

## Implemented mechanics

- Barrage has its own retained mask, radius, count interpretation, and projectile art. Additional missiles use ordinary weapon damage and independent evasion/deflection handling, without duplicating the primary attack's on-hit payloads.
- Positive native `Efk3` count mode has a native additional-target offset. `Efk2` has separate low-count/unlimited semantics; it is not universally a damage budget. The tooltip is not used to override resolved object data. This is a native-engine interpretation, not a map tuning value or a Warcraft-executable measurement.
- The Obelisk owns a separate persistent carrier and regeneration clock. Construction does not activate it prematurely; completion is idempotent, and removal/deactivation stops new launches. Committed missiles retain their damage after carrier removal, but the script's live-source-ability cleanse predicate no longer holds.
- Carrier targeting includes authoritative invisibility/reveal checks. A committed missile does not rerun launch visibility eligibility during flight.
- Both missile families retain live homing position and its authoritative tick, in addition to immutable launch identity. A target moving away postpones impact; the original launch-distance deadline does not cause premature damage. This state participates in canonical hashing and snapshots.
- Cleansing is gated on positive damage and a live carrier ability. Permanent grants are removed only when present in the retained script list; unrelated permanent grants survive. Native buff identity is distinct from permanent ability grants. The post-damage Phoenix Fire buff has its own retained identity/lifetime even when its periodic damage is zero.
- Content commitments serialize parsed ordered projection fields and retained source metadata, rather than hashing raw JSON whitespace/key order.
- Live/cold `ContentIdentity` and resolved unit definitions retain their map version. Construction, future production, upgrade precursors, original revival definitions, and wire decoding preserve that identity before activation. Carrier/Barrage and City Hex/shield/Overheat controls consume retained versions rather than unqualified runtime lookups; Hex profiles and delayed callbacks also retain their source version.

## Verification

The reusable tests under `simulation/native_carriers` exercise target masks, independent ordinary arrows, native count modes, construction/activation/removal, visibility/reveal, zero/immune damage, source removal, permanent-grant distinctions, and snapshot/wire continuation. `tests/homing.rs` exercises both damage families against a moving target and one/four-worker continuation. `native_carriers/projection_tests.rs` checks formatting/key-order independence and source-identity sensitivity with synthetic data.

Sequential validation logs are kept outside the repository under the disk-backed `castle-fight-native-worktrees/elven-full` directory. The Tower merge passed 368 simulation tests, simulation all-target Clippy with warnings denied, and client compilation. The subsequent version-retention change passed all 372 simulation tests and strict simulation Clippy (`versioned-identity-sim4.log`, `versioned-identity-clippy.log`); its all-target client validation is separate. The four reusable tests under `content_lifecycle_tests/versioned_identity.rs` exercise cold-only version hash sensitivity and wire rejection, presentation-name independence, and construction/activation/carrier/regeneration continuation across workers. Projection reproducibility was verified during Tower implementation. These checks are synthetic/projection/build evidence; no Warcraft executable conformance run or in-game visual inspection is claimed.

## Remaining integration gates

1. Tower/City runtime version propagation and cold/live identity retention are implemented and covered by focused and full simulation validation. Audit the remaining race-wide defaults in Shrine runtime and Gjallarhorn/Shrine construction activation before declaring all native consumers version-correct. Compatibility constants are now bundle 5, checksum 16, snapshot 13; final release/source/digest publication is still open.
2. Integrate the caster branch's `combat_sapper`/`invulnerable` classifications into native target eligibility and add reusable negative cases. Do not silently discard unsupported retained target-mask semantics.
3. Integrate cleansing with City Hex, native shield/Overheat state, and caster order recovery. Remove native removable state while preserving independent scripted callbacks and recovery. The retained permanent-removal list includes the active shield `A09L`, but not Overheat `A09C`; clearing all target control state would erase an unrelated permanent grant. Preserve baseline passive inventory rather than writing a Hex-suppressed snapshot projection back to the entity. Phoenix Fire must remain independent of ordinary stun/order recovery while respecting Hex ability disable.
4. Reconcile authoritative entity families, schema publication, source/digest registration, imported projectile/buff/impact assets, and model attachment/loading checks on the integrated branch. A successful client check is not evidence of visual fidelity.
5. Re-run integrated lifecycle, wire continuation, mixed-race, deterministic-worker, projection, lint, and workspace checks before race promotion. Record native-oracle uncertainties honestly rather than claiming a synthetic test proves Warcraft behavior.
