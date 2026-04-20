# Life-Monitor - Agent Guide

## Purpose

This file helps coding agents work in `life-monitor` without regressing the storage model, platform collectors, or release flow.

Use it as:

- a map of the current architecture
- a list of product and runtime invariants
- a guide for where changes should go
- a warning list for high-risk parts of the repo

This guide optimizes for correctness and velocity, not completeness.

---

## Project Summary

`life-monitor` is a cross-platform Rust activity tracker for Linux and Windows.

It collects:

- keyboard activity
- mouse movement, clicks, and scroll activity
- active or focused window information over time

It stores data in a local SQLite database using bucketed records.

The project is local-first by default:

- SQLite is the collector database
- snapshot export/import is supported for explicit history movement
- custom database paths can point to local disks or already-mounted shares

There is now an optional, feature-gated multi-device sync mode:

- disabled by default
- compiled only with `--features multi-sync`
- keeps local SQLite as the writable collector DB
- uses a remote `sqld` / libSQL endpoint as the canonical merged store

Current release line:

- crate version: `0.1.6`
- release tags: `vX.Y.Z`

---

## Core Product Model

### Default storage strategy

The default product model is still:

- collect locally
- inspect locally
- export a consistent SQLite snapshot when needed
- import and merge that snapshot somewhere else

### Optional multi-device sync strategy

When built with `--features multi-sync` and explicitly configured by the user:

- each device keeps its own local SQLite database
- each device owns exactly one `source_uuid`
- the remote `sqld` database is the canonical merged store
- devices push only their own source-owned rows
- devices pull foreign rows and keep them locally queryable
- if remote sync fails, local collection must continue
- if remote sync is unavailable at startup, the collector must still start and keep writing locally

### Data model

The primary stored activity model is bucket-based:

- `InputBucketRecord`
- `FocusBucketRecord`

Those bucket rows are the source of truth for:

- totals
- analytics
- import/export merging
- sync convergence

Current built-in analytics are CLI-first:

- `sessions`
- `session-stats`
- `apps`
- `daily`

Do not replace bucket storage with ad hoc cumulative counters unless the product model is being deliberately redesigned.

---

## Architecture Map

```text
src/
├── main.rs
├── input_bindings.rs
├── common/
│   ├── buckets.rs
│   ├── focus.rs
│   ├── input.rs
│   ├── motion.rs
│   ├── paths.rs
│   ├── process.rs
│   ├── ticker.rs
│   └── types.rs
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
│       ├── startup.rs
│       └── systray.rs
├── storage/
│   ├── backend.rs
│   └── localdb/
│       ├── analytics.rs
│       ├── config.rs
│       ├── export.rs
│       ├── import.rs
│       ├── integrity.rs
│       ├── rows.rs
│       └── schema.rs
├── sync/
│   ├── mod.rs
│   ├── outbox.rs
│   ├── pull.rs
│   ├── push.rs
│   ├── remote.rs
│   ├── runtime.rs
│   ├── state.rs
│   ├── status.rs
│   ├── tests.rs
│   └── types.rs
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

---

## Where To Look First

If the task is about:

- CLI behavior or command routing:
  - `src/main.rs`
  - `src/utils/args.rs`

- shared bucket logic, motion math, focus buffering, or path helpers:
  - `src/common/*`

- database schema, import/export, analytics, or DB path behavior:
  - `src/storage/localdb/*`
  - `src/storage/backend.rs`

- feature-gated multi-device sync:
  - `src/sync/*`
  - `src/main.rs`
  - `src/utils/args.rs`

- Linux raw input:
  - `src/platform/linux/inputs.rs`

- Windows raw input, focus, startup, or systray:
  - `src/platform/windows/inputs.rs`
  - `src/platform/windows/common.rs`
  - `src/platform/windows/process.rs`
  - `src/platform/windows/startup.rs`

- Linux startup generation:
  - `src/platform/linux/common.rs`

- DPI persistence:
  - `src/utils/dpi.rs`

- locking and multi-process coordination:
  - `src/utils/lock.rs`

- CI and release flow:
  - `.github/workflows/no-nix.yml`
  - `.github/workflows/nix.yml`
  - `.github/workflows/release.yml`

---

## Main Runtime Flow

`src/main.rs` has two major execution modes:

1. short-circuit commands
   - startup enable/disable
   - export
   - import
   - import dry-run
   - analytics reports
   - sync push/pull/status when `multi-sync` is enabled

2. long-running collection
   - resolve DB path
   - resolve DPI
   - initialize local DB backend
   - spawn input collection
   - spawn focus/process collection
   - spawn Windows systray when enabled
   - spawn background sync loop only when `multi-sync` is compiled and configured

Keep that split clear. Commands that do work and exit should not start the long-running collector and then special-case later.

---

## Important Invariants

### 1. Local SQLite remains the collector database

Even with `multi-sync`, local collection still writes to local SQLite first.

Do not turn the collector into a remote-first writer.

### 2. Bucket rows remain the primary truth

Totals, analytics, imports, and sync convergence should all derive from bucket rows.

Do not introduce mutable shared running totals as the primary persisted state.

### 3. Import/export must remain idempotency-aware

Do not weaken:

- `exports`
- `imports`
- file hash checks
- export UUID checks

Those exist to prevent duplicate imports and doubled totals.

### 4. `--db-path` behavior is user-facing

`--db-path` accepts:

- a file
- a directory
- a missing directory-like path

It is remembered for future runs.

Do not change this casually. Users may rely on it for removable disks or mounted shares.

### 5. Multi-sync must stay fully gated

When `multi-sync` is disabled:

- no sync code should compile
- no libSQL / remote dependency should be required
- the local-only collector flow must keep working unchanged

### 6. Source ownership is strict in sync mode

Each device owns exactly one `source_uuid`.

A device may push only rows owned by that source.

Pulled foreign rows:

- may exist locally
- must remain queryable locally
- must never be re-enqueued as local authored changes

### 7. Sync must remain retry-safe

Push and pull must remain idempotent:

- no duplicate canonical rows on retry
- no cursor advance before successful full apply
- no marking pending rows as sent before remote acknowledgement

### 8. Foreign source metadata must stay real

When pulling foreign-source bucket rows, the local database must receive the real remote
`sources` metadata first.

Do not reintroduce placeholder source rows based on guessed names or the local platform.

### 9. Sync failure must not stop collection

If the remote is unavailable or a sync attempt fails:

- local collection continues
- local writes continue
- pending outbox rows remain queued
- status records the failure

### 10. Linux and Windows should share logic only where behavior is truly shared

Good places to share:

- motion math
- bucket buffering
- tracker state transitions

Keep platform event decoding and OS integration local to each platform.

### 11. Release tags must match `Cargo.toml`

The release workflow enforces this.

Do not change release logic in a way that allows publishing mismatched versions.

### 12. Linux startup should prefer standard XDG autostart and keep systemd fallback narrow

Linux startup now has two modes:

- default: XDG autostart desktop entry
- fallback: `systemd --user` unit tied to the graphical session

Preserve these expectations:

- startup artifacts should point at the executable the user enabled startup from
- prefer XDG autostart for desktop-session startup
- keep the `systemd --user` mode explicit and advanced
- do not bake volatile graphical-session variables such as `WAYLAND_DISPLAY` into the systemd unit
- do not mutate the wider systemd user manager environment just to make the service start
- warn when startup is enabled from a fragile repo build path such as `target/debug` or `target/release`

---

## Storage And Sync Tables

Main local tables:

- `schema_meta`
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

Local sync tables when `multi-sync` is enabled:

- `sync_state`
- `sync_outbox_sources`
- `sync_outbox_input_buckets`
- `sync_outbox_focus_buckets`

Remote canonical sync tables:

- `sources`
- `input_buckets`
- `focus_buckets`
- `sync_applied_batches`
- `sync_revisions`
- `sync_source_changes`
- `sync_input_changes`
- `sync_focus_changes`

If you change schema:

- update schema setup and any migrations or bootstrap logic
- update tests
- think through import/export behavior
- think through sync behavior if `multi-sync` is enabled

Compatibility with older schema versions is not a project priority right now. Keep the implementation simple unless the task explicitly requires compatibility handling.

---

## Build And Test Expectations

### Use flake environments

Builds and tests should be run from environments provided by `flake.nix`.

Preferred commands:

```bash
nix develop --command cargo fmt --all
nix develop --command cargo build --target x86_64-unknown-linux-gnu
nix develop --command cargo test --target x86_64-unknown-linux-gnu
nix develop --command cargo build --target x86_64-pc-windows-gnu
nix develop --command cargo check --target x86_64-pc-windows-gnu
nix build .#linux
nix build .#windows
```

`nix build .#windows` is a cross-compiled Windows GNU package build from the current host system, not a native Windows build job.

Do not assume the repo's current default target. CI explicitly passes `--target`, and local verification should do the same.

The default dev shell intentionally separates host Linux and Windows cross-build C toolchains:

- host Linux builds should use the host compiler toolchain
- Windows cross-builds should use target-specific `x86_64-pc-windows-gnu` toolchain variables
- normal host `cargo build` / `cargo test` should work without forcing a Windows target globally

Do not reintroduce Windows MinGW runtime headers or libraries into the default host build environment in a way that pollutes Linux native C builds.

### SQLite runtime expectation

`rusqlite` is bundled on both Linux and Windows.

Preserve that unless there is a deliberate packaging reason to change it:

- Linux runtime should not depend on a system `libsqlite3.so`
- Windows runtime should not depend on an external SQLite install
- if build or shell changes break bundled SQLite compilation, fix the toolchain environment instead of silently falling back to a system SQLite dependency

### Windows test reality

Wine is useful for some local validation, but it is not authoritative for full Windows runtime behavior.

Assume:

- compile checks for Windows are valuable locally
- some Windows tests may run under Wine
- native Windows CI remains the authoritative runtime gate

Do not contort production code just to make every Windows behavior test pass under Wine.

### Test philosophy

Prefer tests that verify:

- observable behavior
- ownership rules
- state transitions
- merge/import/export outcomes
- retry and idempotency behavior
- analytics outputs derived from real stored bucket/session rows

Avoid tests that mostly verify:

- OS behavior itself
- static message wording
- broad smoke paths with many unrelated failure points
- timing-sensitive behavior when a direct helper-level test would be clearer

If sync behavior changes, add tests for:

- startup/offline behavior
- source ownership
- foreign row pull behavior
- outbox safety
- convergence or retry behavior

Use the lowest stable test level that proves the behavior.

---

## Preferred Change Strategy

When working in this repo:

1. Identify the task area first:
   - runtime collection
   - storage/import/export
   - sync
   - CLI/config
   - platform-specific behavior
   - CI/release

2. Read the narrowest responsible files first.

3. Preserve the current product direction:
   - local-first
   - bucket-based
   - explicit snapshot import/export
   - optional feature-gated sync
   - user-visible recovery messages

4. Add or update targeted tests when behavior changes.

5. Prefer small coherent commits.

Commits should be split by feature or coherent code change, not bundled into one large mixed commit.

Each commit message should have:

- a helpful title
- a body that explains what changed
- how it changed
- why it changed

This is important for release notes and for understanding the repo history later.

---

## High-Risk Areas

Be extra careful in:

- `src/storage/localdb/*`
  - schema, import/export, analytics, and row merge behavior have wide blast radius

- `src/sync/*`
  - ownership, idempotency, and cursor handling can fail subtly

- `src/common/*`
  - shared logic regressions hit both platforms

- `src/platform/linux/inputs.rs`
  - measurement accuracy can regress silently

- `src/platform/windows/inputs.rs`
  - raw input and message-loop behavior are easy to break

- `src/utils/lock.rs`
  - filesystem semantics vary across environments and mounted paths

- `.github/workflows/release.yml`
  - publishing and asset attachment are easy to break

---

## Known Weak Spots

Current likely cleanup targets:

- some sync/outbox seams can still be simplified without undoing the current module boundaries
- storage and sync responsibilities should continue to stay narrow as features grow
- Wine cannot replace native Windows validation
- remote share behavior depends on OS mount semantics and is still best-effort
- there is still no built-in dashboard or TUI

---

## Anti-Patterns To Avoid

- reintroducing a general remote backend into the default product path
- bypassing bucket storage with ad hoc totals
- enqueueing pulled foreign rows into the sync outbox
- marking outbox rows sent before remote acknowledgement
- advancing the pull cursor before a full successful apply
- mixing unrelated feature, schema, CI, and docs changes into one commit when they can be separate
- assuming Linux-only validation is enough for Windows behavior

---

## One-Sentence Mental Model

`life-monitor` is a local-first, bucket-based activity recorder whose core guarantees are accurate input collection, safe SQLite persistence, explicit snapshot movement of history, and optional feature-gated multi-device convergence without interrupting local collection.
