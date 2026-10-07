# Single-player control and vision

Date: 2026-10-07.

The main menu again offers Single Player, which opens the offline setup lobby and keeps the
simulation paused until Start. The selected position supplies the initial camera focus and
resource display; all configured players' builders and buildings are controlled under their
owners' command authority. This session control remains enabled independently of debug-menu
availability, and installing a network session clears it.

Observer presentation combines the existing authoritative team visibility and exploration
when all-player control is active. Controlled units (including invisible units), builders,
and buildings remain observable; terrain outside the combined sight retains fog/shroud.
The authenticated host's control override uses the same vision policy. Control changes rebuild
both presentation endpoints from unfiltered authoritative state, discard network samples
filtered under the old permissions, and reset the cosmetic fog observer. Quickload retains
the current session's control policy. Simulation sight, targeting, content identity, checksum,
and snapshot schemas are unchanged.

Validation:

- `tools/cargo-interactive test -p castle-fight-client`: 257 passed, four existing
  asset/GPU-dependent tests ignored.
- `tools/cargo-interactive check -p castle-fight-client --all-targets`.
- `tools/cargo-interactive clippy -p castle-fight-client --all-targets -- -D warnings`.
- `cargo fmt --all -- --check` and `git diff --check`.
- Regressions cover menu-to-lobby entry, all builders in the configured roster, command-owner
  resolution for builders/buildings, both observer teams, invisible controlled units, live
  structures and attack events, retained fog/shroud, paused permission changes, queued network
  ticks/boundaries, reset/load continuity, and unchanged authoritative checksums.

No manual rendered GUI session was used for this verification.
