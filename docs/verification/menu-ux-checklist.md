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
- [x] Decided: Castle Fight DE has a **team forfeit vote** (`docs/original_map/extracted/resolved/runtime-session-mechanics.tsv`, row `team-forfeit-vote`): any eligible player (in game, not away/AFK) can start a vote; the initiator's vote counts immediately; it passes when the eligible votes reach the number of non-away human players on the team at vote start; it times out after 60 seconds; success kills the forfeiting team's own castle.
- [ ] Implement the team forfeit vote. It needs a protocol message and server-side vote tracking, and a canonical control event that kills the team's castle so replays and reconnects agree (a simulation change, so it should follow the engine's control-event process rather than be done from the UI). The F10 menu is the natural place for the command.

### Lobby

- [x] Leave button in both the single-player and networked lobby; it returns to the main menu, closes the connection and stops a hosted server. (A joiner whose host left can now get out of the frozen lobby too.)
- [x] Host can choose players per team (1v1 / 2v2 / 3v3) in a hosted lobby. New `SelectTeamSize` request (protocol 16); players whose slot disappears are moved, and the shrink is refused if they would not fit. Joiners rebuild their local world from the server's roster.
- [x] Non-host controls that are disabled say why ("Only the host can change these settings", "Players per team (set by host)").
- [x] Joiners can move to any open slot, including on the other team (`SelectSlot`); open slots show a "Move here" button and no race selector. Leaving the lobby (`LeaveLobby`) now frees the slot; before, a departed player kept it reserved and the match looked full.
- [x] Player names are sent on join, shown in the lobby, the scoreboard and the top bar instead of "Player N". The name defaults to the operating system account name and can be changed in Settings.
- [x] Waiting-for-players state is a prominent status panel ("WAITING FOR PLAYERS 2 / 4" with the number of open slots, then "ALL PLAYERS CONNECTED"), not just the footer line.

### Join / host flow (main menu)

- [x] Paste (`Ctrl+V`) in the server-address and port fields via the system clipboard (bevy `system_clipboard`); not verified on every desktop environment.
- [x] Shortcuts no longer type their letter into the field, and `Ctrl+Backspace` clears it. Caret movement and select-all are not implemented (the box is append-only).
- [x] `Esc` returns from the Join screen to the main menu (and cancels a connection attempt in progress); `Enter` already connects.
- [x] Host and Connect run off the UI thread with a visible "Connecting..." state and a Cancel button (`Esc` also cancels). The TCP connect is bounded to 5 seconds instead of the operating system's timeout. Failures return to the Join screen with the error.
- [x] The last server address that connected successfully is saved in `settings.json` (next to the view state) and offered again. A recent-servers list is not implemented.
- [x] The hosted lobby shows `LAN-IP:port` with a Copy button. Only the LAN address can be determined; for internet play the host still has to know their public IP and forward the port.
- [x] Host Game opens a port screen (default 6112, remembered; 1024-65535).
- [x] Join rejections are explained in plain language (different network version, different map, different game data, different match setup, game full).

### Connection status and messages

- [x] A persistent "CONNECTION LOST" banner shows the reason and the latest reconnect failure while the connection is down, and points at F10 to leave. It is hidden once the match has finished.
- [x] Messages stack as toasts under the top bar for five seconds (newest repeat refreshes its timer), and command rejections are worded for players ("Not enough gold (50/200).", "Another builder is already building there.") instead of `{reason:?}`.
- [ ] Optional audio cue for rejected commands and alerts. Blocked on assets: the Warcraft III asset converter only extracts unit and effect sounds, so there is no interface/error sound to play yet. Extend `cf-wc3-assets` to extract the stock interface sounds first.

## P2 — Settings and discoverability

### Settings screen

- [x] Settings and Controls are reachable from the main menu and the F10 menu (`settings_menu.rs`).
- [x] Audio: master volume (applied through Bevy's global volume). All current sound is effects, so a separate effects slider waits for music.
- [x] Display: fullscreen toggle in Settings (shares the window-mode logic with Alt+Enter). Resolution and vsync controls are not implemented.
- [ ] UI scale / text scale (menus use fixed pixel sizes). Deferred: Bevy's global `UiScale` would also scale the in-game HUD, whose hit-testing helpers (`cursor_over_*`) assume unscaled pixel layouts, so world clicks could land through the panels. Needs those helpers made scale-aware first.
- [x] Input: edge scrolling on/off, camera speed and zoom speed multipliers.
- [x] Settings persist in `settings.json` beside `view-state.json`.

### Help and discoverability

- [x] Controls reference overlay (camera, selection, orders, game keys), reachable from the main menu and F10 menu.
- [x] A clickable SCORES button beside the GRID/SNAP buttons opens and closes the scoreboard. `Tab` is not used: it already cycles the selected unit type.
- [x] The top match line is split into Round, Score, Timer and Rescue segments plus a status segment (round complete, disconnect pause, match result); the embedded "F9: Scores" hint was dropped now that a SCORES button exists.
- [x] The FPS / TICK readout in the top bar is shown only while the F1 debug overlays are on. The F8 debug menu already refuses to act in a networked match unless you are the connected host (`debug_available()`); offline it acts as the single-player cheat menu.
- [x] Quicksave and Quickload are buttons in the F10 menu for offline matches (the Ctrl+S / Ctrl+O hotkeys remain).

### Camera

- [x] `Backspace` jumps the camera to your castle. Centring on a builder already exists (grave key, builder shortcut buttons). "Jump to last alert" waits for the alert system (P3).
- [x] Edge scrolling can be turned off in Settings.

## P3 — Lobby and polish

- [x] The race picker has a Random tile (resolved when pressed) and shows each race's number of buildable structures. A fuller preview (building/unit list) would need per-race descriptions that are not extracted.
- [x] Lobby chat: press Enter, type, Enter to send (protocol 17).
- [x] Host kick: the host gets a Kick button on other players' lobby rows (protocol 18); the removed player returns to the main menu with a notice. A ready state was not added: the host already waits for every slot to be filled and decides when to start.
- [x] The debug "Starting buildings" setting is listed in the lobby only when the `CASTLE_FIGHT_DEBUG` environment variable is set.
- [ ] Simplify the time-limit control (stepper buttons plus click-to-type) and give it keyboard focus handling; avoid depending on a full lobby despawn/respawn for every change (hover/focus state is lost on each redraw).
- [x] `Tab`/`Shift+Tab`/arrow keys move a focus ring through the topmost menu overlay and `Enter`/`Space` press the focused button (`menu_focus.rs`).
- [x] Menu buttons get an accent border on hover/focus and a distinct pressed colour; disabled buttons are excluded.
- [x] The main menu warns, with a pointer to `cf-wc3-assets` and the README, when `assets/wc3/manifest.json` is missing.
- [x] The main menu footer shows the version and the unofficial-fan-project / Blizzard-assets notice. A dedicated About screen was judged unnecessary.
- [x] Settings has a colour-blind friendly option that switches the minimap enemy colour from red to orange. In-world team colours and the scoreboard are unchanged, so this is partial; the top bar already spells out YOU / ALLY / ENEMY.
- [x] In-match text chat: Enter talks to allies, Shift+Enter to everyone, lines fade after about 14 seconds. Quick pings are not implemented (they need the minimap).
- [x] A "Your castle/base is under attack!" message appears (at most every 10 seconds) when a building of yours loses health, and `.` jumps the camera to the last attacked building. No sound or minimap flash yet (no interface sounds are extracted; no minimap).
- [ ] Replay or spectate entry point in the menu, if the replay machinery in the sim is wired to the client. Not done: the client has no replay loader or spectator mode to attach a menu entry to yet.
- [ ] Gamepad support (low priority; not required for a Warcraft-style RTS). Not attempted.

## P4 — Large, deferred

- [x] **Minimap.** Implemented in `minimap.rs`: shaded terrain, buildings, units, builders, the camera footprint and an attack ping, with click/drag to move the camera (spec §15.3). Not done: right-click orders and pings on the minimap, and unit/building icons.

## Not applicable / spec corrections

- **Selling buildings does not exist in Castle Fight.** Do not add a Sell command or button. The `docs/spec` mentions of selling/sale were removed or reworded (cancelling unfinished construction, which refunds, remains).
- **Builder inventory slots.** The six empty slots in the selection console (`InventoryPlaceholder`) are a layout placeholder until builder items and `UseItem` exist. Either hide them until then or leave them, but don't treat them as a UI bug on their own. Tracked by the builder-items work in `docs/spec/42-builder-items.md`.
