# Host debug controls verification

Date: 2026-10-06. Protocol schema 8; checksum schema 23; snapshot schema 18.

The interactive network host can grant resources, damage all units, populate the selected
version's complete building catalog, toggle building immunity, control other players' actors,
pause/resume, change speed, and advance a single paused tick. The original authenticated host
session owns this authority, including across reconnects. Guests and pre-start requests are
rejected by the server independently of menu visibility. The client closes the menu and clears
the control override when host access is unavailable.

Gameplay mutations use canonical boundary records, shared with offline debugging, replicas,
and replay. Other-player orders keep normal owner admission and execution validation without
consuming another player's client-command sequence. Building population/layout moved unchanged
to the simulation so replicas use one deterministic implementation and selected-version content.
Building immunity now participates in checksums and snapshot restore; quickload uses the saved
flag rather than overwriting it from the menu. Pause/speed/step pace server ticks; they do not
alter the simulation's deterministic tick calculation.

Validation:

- Workspace tests: 738 passed; four existing asset/install-dependent tests ignored.
- Workspace check, all-target Clippy with warnings denied, formatting, and diff checks.
- TCP regression exercises every mutation from host/guest, compares a four-worker wire replica,
  restores immunity from wire snapshot, replays the canonical history, rejects guest playback
  requests, and advances exactly one paused tick.
- Client regressions cover menu visibility, loss of authority, host/guest/lobby access, and
  direct guest-request rejection before any local mutation.
- Existing population-layout and catalog-completeness tests remain in place.

No manual network GUI session was used for this verification.
