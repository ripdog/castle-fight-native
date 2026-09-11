# AGENTS.md

## Working style

- Work directly and autonomously; avoid unnecessary clarification when the intent is clear.
- Always commit completed work automatically in logical, reviewable chunks with clear commit messages.
- Keep commits narrowly scoped; do not mix unrelated refactors, formatting, and behavior changes unless they are inseparable.
- Preserve and update the specifications under `docs/spec` when implementation decisions change normative behavior.

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

## Verification

- Add or update focused tests for behavior changes, especially determinism-sensitive code.
- Run relevant formatting, linting, tests, and build checks before committing.
- For performance-sensitive changes, benchmark/profile where practical rather than assuming an optimization is beneficial.
