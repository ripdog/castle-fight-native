# AGENTS.md

## Working style

- Work directly and autonomously; avoid unnecessary clarification when the intent is clear.
- Use a single active coding agent on this host; do not launch delegated workers without explicit user approval. Run compile-heavy tasks sequentially rather than leaving concurrent build queues active.
- `/tmp` is RAM-backed on this host. Keep worktrees and build artifacts on disk, for example under `/home/ripdog/RustroverProjects/castle-fight-native-worktrees/`, not in `/tmp`.
- Always commit completed work automatically in logical, reviewable chunks with clear commit messages.
- Keep commits narrowly scoped; do not mix unrelated refactors, formatting, and behavior changes unless they are inseparable.
- Change `docs/spec` only for design changes or changes to normative engine behavior, not to record newly implemented entities. Keep implementation checklists, source audits, and progress tracking separately under `docs/verification`.
- Extracted map data is authoritative. Consumers must reference that evidence or reproducibly generated projections; do not maintain independent copies of map values in specifications, tests, UI labels, or hand-authored runtime tables. Generated projections must retain source identity and be reproducible.
- When implementing or auditing a Warcraft III-derived unit/building, use `docs/verification/unit-implementation-checklist.md` as the per-entity fidelity checklist. Reconcile resolved object data with map-script control flow, including hidden/proxy abilities, trigger vs effect target semantics, timing, AI behavior, authoritative delayed state, and negative-case tests; do not implement from tooltips alone.
- Every Warcraft III-derived unit, building, ability and effect targets full presentation parity with Warcraft III: what WC3 draws, animates and plays for it, the client draws, animates and plays. Never substitute an invented or generic effect (for example a placeholder ring) where WC3 shows something else, and never drop WC3's visual without a documented, measured reason. When the object data has no effect art, look for what WC3 actually does (animation tags, model event objects, particle emitters, ability sounds) and reproduce that; reserve generic placeholders for abilities that genuinely have no WC3 counterpart.
- When using Devspace worktrees, remove a completed worktree after merging its changes instead of leaving it on disk.

## Rust expectations

- Target **Rust 2024 edition** and current stable Rust unless the project explicitly requires otherwise.
- Prefer modern, idiomatic, type-safe Rust over legacy patterns or convenience shortcuts.
- Optimize for correctness first, then measured efficiency; avoid cleverness that weakens determinism, ownership clarity, or maintainability.
- Prefer borrowing/references over copying or cloning where practical. Avoid unnecessary allocation, cloning, intermediate collections, boxing, and dynamic dispatch in hot paths.
- Use ownership, lifetimes, enums, newtypes, iterators, consts, and exhaustive matching to encode invariants in the type system.
- Prefer explicit data-oriented layouts and cache-friendly access patterns for simulation-heavy code.
- Avoid `unsafe` unless there is a demonstrated need, a measurable benefit, and a documented safety argument.
- Treat Clippy warnings, compiler warnings, formatting, and failing tests as issues to resolve rather than suppress by default.

## Architecture

- The deterministic simulation is authoritative. Presentation, rendering, wall-clock time, worker count, hash-map iteration order, and task scheduling must not affect gameplay results.
- Preserve clear crate/module boundaries and narrow ownership. Avoid global mutable state and broad `&mut World` access where more precise interfaces are possible.
- Prefer deterministic, parallel-friendly algorithms and explicit phase boundaries over implicit ordering or scheduler-dependent behavior.
- Do not regress core gameplay invariants documented in `docs/spec`.
- Castle Fight-derived content/tuning values (unit/building stats, costs, ranges, cooldowns, repair/build times, command-card positions, map geometry, ability parameters, etc.) MUST remain version-scoped. It is acceptable to compile extracted constants into Rust, but they must live behind version-aware definitions/lookups such as `definition_for_version(MapVersion)` (or equivalent generated/versioned content structures), with callers consuming those APIs rather than scattering unqualified map-version literals through simulation/client code. Supporting a future map version must not require replacing large numbers of callsites that embedded an older version's values.

## Verification

- Add or update focused mechanic-level tests for behavior changes, especially determinism-sensitive code. Prefer synthetic fixtures that exercise the engine contract, and catalog-wide validation against extracted evidence. Avoid per-entity tests by default; add one only for a demonstrated entity-specific integration bug or interaction that cannot reasonably be covered by a reusable mechanic test. Do not write tests that merely duplicate extracted numeric values.
- Only write a test when it guards a likely point of regression or a bug actually observed in-game or in a real run (for example a leaked socket, stale state surviving a world swap, or an easily broken ordering). Do not add tests that restate the implementation or check trivial wiring: a key toggling a menu, a key that must not do something, input being blocked while a modal is open, a button existing, a flag being set on drop, or a constructor returning what it was given. When unsure, leave the test out; a smaller suite of meaningful tests is better than coverage for its own sake.
- Run relevant formatting, linting, tests, and build checks before committing.
- Run compile-heavy Cargo commands through `tools/cargo-interactive` (for example `tools/cargo-interactive check --workspace` or `tools/cargo-interactive test -p castle-fight-sim`). The wrapper reserves half of the machine's physical CPU cores for interactive use, constrains the long-lived sccache compiler server, gives sccache a larger shared cache budget, and serializes compile-heavy CF Native builds across worktrees so parallel agents do not oversubscribe the same CPUs. Codex should launch these commands with its native command/task tools and inspect their output there. ChatGPT, which does not have those native tools, should launch them through Devspace as asynchronous tasks (`async=true`) and inspect their output with the task APIs. Do not bypass the wrapper with raw `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, benchmarks, or similar compile-heavy commands.
- Respect the repository's six-job Cargo build cap; do not override it with `-j`/`--jobs` or `CARGO_BUILD_JOBS` unless the user explicitly asks.
- For performance-sensitive changes, benchmark/profile where practical rather than assuming an optimization is beneficial.
