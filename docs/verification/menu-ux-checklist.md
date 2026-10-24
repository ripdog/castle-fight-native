# Menu, lobby and HUD UX checklist

Status: **audit of 2026-10-24; nothing below is implemented unless ticked.** Ticked items have not yet been checked by hand in the running client.

This is a progress tracker, not a specification. It was produced by reading the client code (`crates/client/src/{main_menu,lobby,match_ui,resource_ui,scoreboard_ui,inspection,build_ui,main,presentation}.rs`) and was not validated by playing the game. Re-check each item against the code before starting it; line references drift.

Normative UI behavior lives in `docs/spec/31-input-building-ui.md` and `docs/spec/30-client-presentation.md`. Change those only for design changes, not to record that an item was done.

Items are grouped by priority. Within a group, order is roughly the suggested implementation order.

## P1 — Core flow gaps

### Esc menu, leaving, and end of match

- [x] In-game menu opens on `F10` (as in Warcraft III), **not** `Esc`: `Esc` is spammed to clear selection and cancel targeting, so it must never open a menu. `Esc` only closes the menu or backs out of a confirmation once it is open. Implemented in `game_menu.rs`; spec §15.1 of `31-input-building-ui.md`.
- [x] In-game menu offers Resume, Main Menu and Exit. (Settings is added to it by the Settings screen items below.)
- [x] Leaving a networked match disconnects cleanly and returns to the main menu with state reset (authoritative sim, presentation samples, selection, action panel, lobby). The client socket is now actually closed on drop, and a hosted server is stopped and its port released. Follow-up: the server treats this as an ordinary disconnect (teammates see the disconnect pause) because there is no explicit "leave" message in the protocol.
- [x] Leaving an offline match returns to the main menu, with a confirmation.
- [x] End-of-match screen: the final scoreboard opens automatically with a Victory / Defeat / Draw headline (relative to the local player's team; just the winning side's name when one player controls both teams) and View battlefield, Main Menu and Exit buttons. `F9` shows or hides the result afterwards. Also stops a finished networked client from retrying to reconnect to the server that ended the match.
- [x] Destructive actions (Leave, Exit) ask for confirmation, skipped once the match has finished.
- [ ] Decide whether Surrender exists in Castle Fight DE; if so it needs a protocol command and a vote/host rule. (Open question — do not add without map evidence.)

### Lobby

- [x] Leave button in both the single-player and networked lobby; it returns to the main menu, closes the connection and stops a hosted server. (A joiner whose host left can now get out of the frozen lobby too.)
- [x] Host can choose players per team (1v1 / 2v2 / 3v3) in a hosted lobby. New `SelectTeamSize` request (protocol 16); players whose slot disappears are moved, and the shrink is refused if they would not fit. Joiners rebuild their local world from the server's roster.
- [x] Non-host controls that are disabled say why ("Only the host can change these settings", "Players per team (set by host)").
- [x] Joiners can move to any open slot, including on the other team (`SelectSlot`); open slots show a "Move here" button and no race selector. Leaving the lobby (`LeaveLobby`) now frees the slot; before, a departed player kept it reserved and the match looked full.
- [x] Player names are sent on join, shown in the lobby, the scoreboard and the top bar instead of "Player N". The name defaults to the operating system account name; a field to edit it belongs with the Settings screen (still open under P2).
- [x] Waiting-for-players state is a prominent status panel ("WAITING FOR PLAYERS 2 / 4" with the number of open slots, then "ALL PLAYERS CONNECTED"), not just the footer line.

### Join / host flow (main menu)

- [ ] Paste support in the server-address field. `Ctrl+V` currently arrives as `Key::Character("v")` and inserts a literal `v`.
- [ ] Ignore text input while a control modifier is held, and add basic editing (caret movement, clear, select-all).
- [x] `Esc` returns from the Join screen to the main menu (and cancels a connection attempt in progress); `Enter` already connects.
- [x] Host and Connect run off the UI thread with a visible "Connecting..." state and a Cancel button (`Esc` also cancels). The TCP connect is bounded to 5 seconds instead of the operating system's timeout. Failures return to the Join screen with the error.
- [ ] Remember the last-used server address (and optionally a short recent-servers list) instead of defaulting to `127.0.0.1`.
- [ ] Host sees their reachable address/port and can copy it (LAN IP, port 6112).
- [ ] Host port is configurable (it is fixed at `DEFAULT_GAME_PORT` and only displayed).
- [ ] Version/compatibility mismatch on join is explained in plain language rather than surfaced as a raw error string.

### Connection status and messages

- [ ] Disconnect / reconnecting / reconnect-failed state shown as a persistent banner or modal with the reason, not only as a replaceable line. (`pending_status` is copied into the single `action_panel.status` text, so a later message overwrites an earlier one.)
- [ ] A transient message/log area for command rejections ("Not enough gold", blocked placement, out of range) so they are noticeable and not overwritten immediately. The spec (§10 of `31-input-building-ui.md`) already asks for structured rejection reasons to reach the user.
- [ ] Optional audio cue for rejected commands and alerts.

## P2 — Settings and discoverability

### Settings screen

- [ ] Settings reachable from the main menu and the in-game menu.
- [ ] Audio: master, effects and (if/when added) music volume. Currently audio exists only as spatial combat effects (`wc3_effects.rs`) with no volume control.
- [ ] Display: fullscreen/windowed toggle in UI (state is persisted in `view_state.rs` but there is no menu control), resolution/vsync as applicable.
- [ ] UI scale / text scale (menus use fixed pixel sizes).
- [ ] Input: edge-scroll on/off, camera pan speed, scroll-wheel zoom sensitivity.
- [ ] Persist settings alongside the existing view state.

### Help and discoverability

- [ ] Controls / hotkey reference screen (camera, selection groups, `Tab` type cycling, grave idle-builder cycle, `Shift` queueing, `F1`/`F8`/`F9`, `H`, `Ctrl+S`/`Ctrl+O`).
- [ ] Scoreboard can be held with `Tab` (and/or opened by a clickable HUD button) in addition to `F9`.
- [ ] Round/time/Rescue Strike line in the top HUD split into readable elements instead of one text string with an embedded "F9: Scores" hint.
- [ ] Debug-only HUD text (FPS / tick) and the `F1`/`F8` developer overlays are hidden from ordinary play, or gated behind a flag. (Not confirmed how `debug_available()` gates them in a normal network match.)
- [ ] Quicksave / quickload (`Ctrl+S` / `Ctrl+O`) exposed in the Esc menu for offline matches.

### Camera

- [ ] "Jump to castle" / "jump to last alert" / "centre on builder" shortcuts.
- [ ] Option to disable edge scrolling (useful in windowed mode).

## P3 — Lobby and polish

- [ ] Random race option, and some preview of a race (builder description, building/unit overview) in the race picker. It is currently a text-only grid.
- [ ] Lobby chat.
- [ ] Ready state and host kick, if desired once lobby chat/names exist.
- [ ] Move the debug "Starting buildings" setting out of the ordinary match settings (hide it unless debug is enabled) and make it behave like the other options rather than "click again to turn off".
- [ ] Simplify the time-limit control (stepper buttons plus click-to-type) and give it keyboard focus handling; avoid depending on a full lobby despawn/respawn for every change (hover/focus state is lost on each redraw).
- [ ] Keyboard navigation and focus rings in menus (Tab / Enter / arrows).
- [ ] Hover/pressed feedback on lobby/menu buttons is colour-only.
- [ ] First-run / missing-asset notice that points to the `cf-wc3-assets` converter instead of silently showing placeholders.
- [ ] About / version / licence entry in the menu (bundled Blizzard-derived content notice).
- [ ] Colour-blind friendly team indication (team colour is currently the main cue).
- [ ] In-match text chat and quick pings for team games.
- [ ] Under-attack / castle-damaged alert (sound, HUD flash, and camera-jump target) — needs an authoritative or presentation-side event source.
- [ ] Replay or spectate entry point in the menu, if the replay machinery in the sim is wired to the client.
- [ ] Gamepad support (low priority; not required for a Warcraft-style RTS).

## P4 — Large, deferred

- [ ] **Minimap.** `inspection.rs` draws a "MINIMAP" label in an empty box. A real implementation needs terrain, building and unit markers, the camera rectangle, click/drag to move the camera, and a fog-of-war aware view, and it is the natural surface for alerts and pings. This is a substantial, self-contained piece of work; treat it as its own plan rather than as part of the menu pass.

## Not applicable / spec corrections

- **Selling buildings does not exist in Castle Fight.** Do not add a Sell command or button. The `docs/spec` mentions of selling/sale were removed or reworded (cancelling unfinished construction, which refunds, remains).
- **Builder inventory slots.** The six empty slots in the selection console (`InventoryPlaceholder`) are a layout placeholder until builder items and `UseItem` exist. Either hide them until then or leave them, but don't treat them as a UI bug on their own. Tracked by the builder-items work in `docs/spec/42-builder-items.md`.
