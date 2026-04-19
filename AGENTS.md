# Life-Monitor - Agent Guide

## Purpose

This file exists to help coding agents become productive in `life-monitor` quickly and avoid breaking the parts of the project that are easy to misunderstand.

Use it as:

- a map of the current architecture
- a list of invariants and expectations
- a guide to where changes should go
- a warning list for the fragile or high-impact parts of the codebase

This guide should optimize for correctness and velocity, not for completeness.

---

## Project Summary

`life-monitor` is a cross-platform Rust activity tracker for Linux and Windows.

It collects:

- keyboard activity
- mouse movement, clicks, and scroll activity
- active/focused window information over time

It stores data in a local SQLite database using a bucket-based schema.

The project is intentionally local-first:

- SQLite is the only storage backend
- cross-machine movement is handled through snapshot export/import
- custom database paths can point to local disks or already-mounted network shares

Current release line:

- crate version: `0.1.6`
- release tags: `vX.Y.Z`

---

## What Changed Recently

These recent commits define the current architecture and are worth reading before large changes:

- `6036373` `feat(storage): move to local-first sqlite snapshots`
- `95b097e` `refactor(input): improve raw motion tracking across platforms`
- `77fd3d2` `ci(windows): run tests and release builds on windows`
- `daf8f5a` `ci(release): publish tagged crates after full validation`
- `971fdbe` `release: prepare v0.1.6 publication flow`
- `3bbd602` `build: update lockfile for v0.1.6`

Those commits are the reason:

- the remote backend is gone
- the storage model is bucket-based
- import/export is now the supported sync story
- DB paths can be remembered and point at mounted shares
- DPI is persistent configuration
- Windows has its own CI job
- release tags now publish to crates.io and attach release binaries

If a change appears to conflict with those goals, stop and verify whether the change is actually intended.

---

## Core Product Model

### Storage strategy

Do not reintroduce the old remote/API sync idea casually.

The intended product model is:

- collect locally
- export a consistent SQLite snapshot
- import and merge that snapshot somewhere else

If a user asks for “sync,” assume the preferred path is still local-first plus import/export unless they explicitly want a new design.

### Data model

The system no longer stores only one running total for inputs or per-window totals.

The primary truth is now time buckets:

- `InputBucketRecord`
- `FocusBucketRecord`

That enables:

- historical activity analysis
- merging across machines
- duplicate import detection
- future session-based reporting

Any change that collapses data back into simple running totals is probably moving in the wrong direction.

---

## Architecture Map

```text
src/
├── main.rs
├── common.rs
├── input_bindings.rs
├── platform/
│   ├── common.rs
│   ├── linux/
│   │   ├── common.rs
│   │   ├── inputs.rs
│   │   ├── process.rs
│   │   ├── wayland.rs
│   │   └── x11.rs
│   └── windows/
│       ├── common.rs
│       ├── inputs.rs
│       ├── process.rs
│       └── systray.rs
├── storage/
│   ├── backend.rs
│   └── localdb.rs
└── utils/
    ├── args.rs
    ├── dpi.rs
    ├── lock.rs
    └── logger.rs

.github/workflows/
├── nix.yml
├── no-nix.yml
└── release.yml
```

### Where to look first

If the task is about:

- CLI behavior:
  - `src/main.rs`
  - `src/utils/args.rs`

- database schema / import / export / path behavior:
  - `src/storage/localdb.rs`
  - `src/storage/backend.rs`

- shared types, bucket logic, paths, or math:
  - `src/common.rs`

- DPI persistence:
  - `src/utils/dpi.rs`

- Linux raw input:
  - `src/platform/linux/inputs.rs`

- Windows raw input or idle behavior:
  - `src/platform/windows/inputs.rs`
  - `src/platform/windows/common.rs`

- focus tracking:
  - `src/platform/linux/process.rs`
  - `src/platform/windows/process.rs`

- locking and multi-process coordination:
  - `src/utils/lock.rs`

- CI / release behavior:
  - `.github/workflows/no-nix.yml`
  - `.github/workflows/release.yml`
  - `cliff.toml`

---

## Main Runtime Flow

`src/main.rs` does two fundamentally different things:

1. short-circuit commands
   - startup enable/disable
   - export
   - import
   - import dry-run

2. long-running collection
   - resolve DB path
   - resolve DPI
   - initialize local DB backend
   - spawn input task
   - spawn focus/process task
   - spawn systray on Windows

When changing CLI behavior, keep that split clear. Import/export should remain “do the work and exit,” not “start the runtime and then special-case later.”

---

## Important Invariants

These are the highest-value facts to preserve.

### 1. SQLite is the only backend

`StorageBackend` only has `Local(LocalDb)`.

If a task suggests adding another backend, treat it as a substantial architectural change, not a small extension.

### 2. Imports must remain idempotency-aware

The import flow records metadata so the same snapshot is not silently imported twice.

Do not remove or weaken:

- `exports`
- `imports`
- file hash checks
- export UUID checks

Those exist to prevent doubled totals.

### 3. Bucket records are the primary stored activity model

Do not bypass bucket writes with ad-hoc cumulative updates unless there is a strong reason and the schema/model is being deliberately redesigned.

### 4. DB path resolution is user-facing behavior

`--db-path` now accepts:

- a file
- a directory
- a directory-like missing path

It also persists remembered paths.

Do not change this casually; users may depend on it for mounted shares.

### 5. Raw input counts are not physical distance by themselves

The code intentionally distinguishes:

- raw input collection
- shared motion math
- DPI/CPI-based distance estimation

Do not claim “real distance” without accounting for DPI/CPI.

### 6. Linux and Windows should share logic where behavior is actually shared

But do not unify platform code if that reduces measurement accuracy or hides real platform differences.

Good rule:

- share math and buffering
- keep platform event decoding separate

### 7. Release tags must match `Cargo.toml`

The release workflow enforces this.

Do not change release logic in a way that allows publishing mismatched versions.

---

## Key Files and Responsibilities

### `src/common.rs`

Shared home for:

- bucket record structs
- focus/input buffers
- process tracker
- program data directory logic
- shared motion math helpers

This file is central and high-impact.

If editing it:

- check whether the logic truly belongs here
- avoid turning it into a generic dumping ground
- keep tests close to shared logic when practical

### `src/storage/localdb.rs`

This is the heaviest file in the repo.

It currently owns:

- schema creation
- metadata tables
- source row management
- import/export logic
- duplicate detection
- DB path resolution
- remembered DB path behavior
- SQLite helpers
- a large amount of tests

Before adding more behavior here, consider whether it belongs in a smaller helper module instead.

### `src/platform/linux/inputs.rs`

Linux input is raw evdev-based.

Important detail:

- relative movement should be aggregated per report before conversion

That was a deliberate fix to avoid overstating diagonal movement.

Do not regress that by summing `REL_X` and `REL_Y` independently as separate physical distances.

### `src/platform/windows/inputs.rs`

Windows input uses Raw Input.

Important distinctions:

- relative motion and absolute motion are handled differently
- some shared math is reused from `common.rs`
- platform event interpretation stays local here

### `src/utils/dpi.rs`

Current behavior:

- `--dpi` overrides and persists
- remembered DPI is reused
- interactive prompt happens when no DPI is known

This is a product decision as much as a technical one.

Do not replace it with fake “automatic DPI detection” unless the detection is truly reliable.

### `src/utils/lock.rs`

There are two lock concepts:

- single-instance lock
- per-database operation lock

The per-database lock is important for:

- regular writes
- export
- import
- mounted-share usage

This area is sensitive because filesystem locking semantics vary across environments.

---

## Current SQLite Tables

Main tables:

- `schema_meta`
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

What they are for:

- `sources`: identify a local source/machine profile
- `input_buckets`: bucketed keyboard/mouse/scroll metrics
- `focus_buckets`: bucketed focus data
- `exports`: identify created snapshots
- `imports`: prevent duplicate imports and keep merge history
- `sessions`: foundation for future session-level analytics

If a migration or schema change is proposed:

- update the schema version
- think through import/export consequences
- think through dry-run reporting
- think through duplicate detection behavior

---

## Testing Expectations

The repo already has a good amount of targeted tests.

Preferred tests:

- singular tests for real behavioral guarantees
- import/export edge cases
- path resolution
- buffer aggregation
- lock behavior where it matters
- CLI short-circuit behavior
- observable behavior, state transitions, side effects, and contracts
- the lowest stable test level that covers the risk well

Avoid low-value tests such as:

- asserting exact wording of ordinary static strings
- tests that fail without breaking behavior
- broad smoke tests that are hard to diagnose unless they simulate something genuinely important
- testing OS behavior, shell behavior, environment-variable mechanics, clocks, randomness, or external tool correctness
- tests for trivial path-joining or wrapper logic when the real program risk is elsewhere
- brittle timing-based tests, sleeps, real subprocess tests, or tests that depend on global machine state unless that integration boundary is the behavior being verified

When adding tests:

- prefer testing helpers or isolated behavior directly
- choose the lowest level that gives strong confidence without coupling to internals
- verify what the program is responsible for, not what the OS or dependencies are responsible for
- inject, fake, or stub OS/process/time/randomness dependencies when possible
- only use command-spawning or timing-heavy tests when the integration behavior itself is the thing being verified
- explicitly note which candidate behaviors should not be tested when they add noise instead of confidence

Useful classification when deciding what to test:

- must test:
  - bucket aggregation behavior
  - import/export duplicate protection
  - focus/input state transitions
  - CLI short-circuit behavior
  - recovery/error handling that changes persisted state or user-visible behavior
- nice to test:
  - narrow helpers that encode meaningful domain behavior and are hard to reason about at a glance
  - stable contract/API behavior that would be expensive to debug if broken
- do not test:
  - raw OS correctness
  - third-party library correctness
  - simple environment/path plumbing unless the program adds non-trivial policy around it
  - giant smoke paths that can fail for many unrelated reasons

Current CI coverage:

- Linux checks/tests/build
- native Windows tests/build on GitHub Actions
- release workflow on tags

Windows test note:

- `cargo test --target x86_64-pc-windows-gnu` can be attempted from the Nix dev shell through Wine
- this is useful for compile coverage and for a subset of runtime tests
- it is not full Windows validation; some Windows-target tests still fail under Wine because of missing or incomplete Windows APIs
- treat real Windows CI or a real Windows machine as the final authority for Windows runtime behavior

Current release workflow:

- validates tag vs `Cargo.toml`
- reruns Linux and Windows checks
- runs `cargo package`
- publishes to crates.io
- creates a GitHub release
- attaches Linux and Windows archives

Changelog support:

- `CHANGELOG.md` is maintained manually
- `cliff.toml` exists to support `git-cliff`

---

## Build and Release Commands

Builds and checks should be run from the environments defined in `flake.nix`.

Use the Nix shell for routine validation instead of assuming host tools are configured correctly.

Common commands:

```bash
nix develop --command cargo check
nix develop --command cargo build
nix develop --command cargo build --release
nix develop --command cargo test
nix develop --command cargo fmt -- --check
nix develop --command cargo clippy -- -D warnings
```

Linux + X11 build/testing:

```bash
nix develop --command cargo test --features x11
```

Nix-based flows:

```bash
nix develop --command ci-checks
nix develop --command ci-test-build
nix develop --command cargo test --target x86_64-pc-windows-gnu
nix build .#linux
nix build .#windows
```

Windows target note:

- `.cargo/config.toml` points the Windows GNU runner at `wine`
- that makes Windows-target tests runnable from Linux inside the Nix shell
- do not treat Wine-backed results as equivalent to native Windows results

Changelog generation:

```bash
git cliff --unreleased
git cliff --tag v0.1.6
```

---

## Preferred Change Strategy For Agents

When working in this repo:

1. Identify whether the task is:
   - runtime collection
   - storage/import/export
   - CLI/config
   - platform-specific behavior
   - CI/release

2. Read the narrowest responsible files first.

3. Preserve the current product direction:
   - local-first
   - bucket-based
   - explicit import/export
   - user-visible recovery messages

4. Add or update targeted tests when behavior changes.

5. Prefer small coherent commits:
   - storage/schema/import-export
   - input/runtime behavior
   - CI/release
   - docs

6. Split commits by feature or coherent code-change area, not as one mixed batch.

For commits:

- use a helpful title that explains the change area
- add a real commit body
- explain what changed
- explain why it changed
- explain how it was implemented when that context will help future readers
- avoid “blog post” commit bodies, but do leave enough detail for release notes and future debugging

This commit structure already fits the recent history and makes release-note generation easier.

---

## High-Risk Areas

Be extra careful in these areas:

- `src/storage/localdb.rs`
  - wide blast radius
  - easy to break schema, merge behavior, or tests

- `src/common.rs`
  - shared logic used by both platforms
  - regressions spread quickly

- `src/platform/linux/inputs.rs`
  - measurement accuracy can regress silently

- `src/utils/lock.rs`
  - timing and filesystem behavior can fail differently across systems

- `.github/workflows/release.yml`
  - mistakes here can break publishing or attach the wrong assets

---

## Known Weak Spots

These are good candidates for future cleanup.

### Structural

- `src/storage/localdb.rs` still owns too many responsibilities
- `src/common.rs` is becoming too broad
- platform focus runtimes share the storage model but not enough coordination structure

### Missing or incomplete features

- Windows startup support is still `unimplemented!()`
- no built-in dashboard or TUI
- changelog generation is not automated in CI
- mounted-share locking is still best-effort because filesystems differ
- session-level analytics are not surfaced yet even though the schema has a `sessions` table

### Documentation/process gaps

- release process depends on repo secrets being configured correctly
- binary release assets exist now, but there is not yet a documented checksum/signing flow

---

## Anti-Patterns To Avoid

- reintroducing a remote backend as a “quick fix”
- bypassing bucket storage with ad-hoc totals
- changing import semantics without updating duplicate protection
- writing tests that only assert static message wording
- writing tests for OS behavior instead of program behavior
- trusting Wine-based Windows results as if they were native Windows validation
- mixing release, runtime, and schema changes into one commit when they can be separate
- assuming Windows behavior from Linux-only testing

---

## One-Sentence Mental Model

`life-monitor` is a local-first, bucket-based activity recorder whose most important guarantees are accurate raw-input collection, safe SQLite persistence, and explicit snapshot-based movement of history across machines.
