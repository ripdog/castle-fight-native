# AGENTS.md

## Working style

- Work directly and autonomously; avoid unnecessary clarification when the intent is clear.
- Always commit completed work automatically in logical, reviewable chunks with clear commit messages.
- Keep commits narrowly scoped; do not mix unrelated refactors, formatting, and behavior changes unless they are inseparable.
- Preserve and update the specifications under `docs/spec` when implementation decisions change normative behavior.
- Devspace worktrees are large: after merging a worktree's completed changes, remove that worktree instead of leaving it on disk.

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

- Add or update focused tests for behavior changes, especially determinism-sensitive code.
- Run relevant formatting, linting, tests, and build checks before committing.
- Run compile-heavy Cargo commands through `tools/cargo-interactive` (for example `tools/cargo-interactive check --workspace` or `tools/cargo-interactive test -p castle-fight-sim`) and launch them through Devspace as asynchronous tasks (`async=true`), then inspect their output with the task APIs. The wrapper reserves half of the machine's physical CPU cores for interactive use, constrains the long-lived sccache compiler server, gives sccache a larger shared cache budget, and serializes compile-heavy CF Native builds across worktrees so parallel agents do not oversubscribe the same CPUs. Do not bypass it with raw `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, benchmarks, or similar compile-heavy commands.
- Respect the repository's six-job Cargo build cap; do not override it with `-j`/`--jobs` or `CARGO_BUILD_JOBS` unless the user explicitly asks.
- For performance-sensitive changes, benchmark/profile where practical rather than assuming an optimization is beneficial.
