# Vigil — Agent Guide

## Purpose

Use this file to orient quickly in `vigil` and avoid breaking the parts of the product that are easiest to regress:

- local-first collection
- bucket-based storage
- import/export merge safety
- feature-gated sync
- Linux and Windows collectors
- the read-only ratatui dashboard

This guide is practical rather than exhaustive. It is written for the next model that needs to change code safely.

---

## Project Model

`vigil` is a cross-platform Rust activity tracker for Linux and Windows.

It records:

- keyboard activity
- mouse movement, clicks, and scroll
- focused-window / active-app activity over time

It stores that history in a local SQLite database using bucket rows:

- `InputBucketRecord`
- `FocusBucketRecord`

There are three main product surfaces:

1. `collector`
   - long-running writer
   - owns collection, startup setup, import/export, and optional background sync

2. `dashboard`
   - read-only ratatui inspection UI
   - must not start collection or acquire the collector lock

3. `sync` / history movement
   - import/export snapshots
   - optional remote convergence when built with `multi-sync`

Keep those surfaces separate.

---

## Current CLI Shape

Top-level commands:

- `vigil collector`
- `vigil dashboard`
- `vigil sync` when `multi-sync` is enabled

Collector-only behavior lives under `collector`:

- `--db-path`
- `--dpi`
- `--clear`
- `--enable-startup`
- `--disable-startup`
- `--export-db`
- `--import-db`
- collector debug / interval settings
- collector-side sync runtime flags

The old report-oriented CLI flow is gone. Analytics now live in `dashboard`.

---

## Architecture Map

```text
src/
├── main.rs
├── input_bindings.rs
├── common.rs
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
│   ├── mod.rs
│   ├── common.rs
│   ├── linux/
│   │   ├── mod.rs
│   │   ├── common.rs
│   │   ├── inputs.rs
│   │   ├── process.rs
│   │   ├── wayland.rs
│   │   └── x11.rs
│   └── windows/
│       ├── mod.rs
│       ├── common.rs
│       ├── inputs.rs
│       ├── process.rs
│       ├── startup.rs
│       └── systray.rs
├── storage/
│   ├── mod.rs
│   ├── backend.rs
│   ├── localdb.rs
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
├── tui/
│   ├── mod.rs
│   ├── app.rs
│   ├── data.rs
│   └── ui.rs
└── utils/
    ├── mod.rs
    ├── args.rs
    ├── dpi.rs
    ├── lock.rs
    └── logger.rs
```

Other important repo paths:

- `.cargo/config.toml`
- `build.rs`
- `icon-resource.rc`
- `screenshots/`
- `.github/workflows/`

---

## Where To Look First

| Task | Files |
| --- | --- |
| CLI parsing, subcommands, help text | `src/utils/args.rs` |
| Command routing and runtime flow | `src/main.rs` |
| Shared bucket / tracker logic | `src/common/*` |
| DB path resolution and SQLite setup | `src/storage/localdb.rs`, `src/storage/localdb/config.rs`, `src/storage/localdb/schema.rs` |
| Import/export/merge behavior | `src/storage/localdb/export.rs`, `src/storage/localdb/import.rs`, `src/storage/localdb/integrity.rs` |
| Storage write path | `src/storage/localdb/rows.rs`, `src/storage/backend.rs` |
| Dashboard state/data/rendering | `src/tui/app.rs`, `src/tui/data.rs`, `src/tui/ui.rs`, `src/tui/mod.rs` |
| Linux startup and startup picker | `src/platform/linux/common.rs` |
| Linux collection | `src/platform/linux/inputs.rs`, `src/platform/linux/process.rs`, `src/platform/linux/wayland.rs`, `src/platform/linux/x11.rs` |
| Windows collection | `src/platform/windows/inputs.rs`, `src/platform/windows/process.rs`, `src/platform/windows/common.rs` |
| Windows startup and tray | `src/platform/windows/startup.rs`, `src/platform/windows/systray.rs` |
| Locking and multi-process coordination | `src/utils/lock.rs` |
| DPI persistence | `src/utils/dpi.rs` |
| Sync | `src/sync/*` |

---

## Runtime Flow

`src/main.rs` has three practical execution paths.

### 1. `vigil collector` short-circuit operations

These do work and exit:

- startup enable/disable
- export
- import
- import dry-run
- `sync push/pull/status` when `multi-sync` is enabled

These must not accidentally start the long-running collector.

### 2. `vigil dashboard`

- opens the read-only ratatui dashboard
- reads SQLite
- refreshes periodically
- does not acquire the collector lock
- does not mutate tracked activity
- does not start background collection

### 3. `vigil collector` long-running mode

Normal collector run:

1. resolve DB path
2. handle short-circuit flags if present
3. acquire single-instance lock
4. resolve DPI
5. initialize SQLite backend
6. spawn input collection
7. spawn focus/process collection
8. spawn Windows systray when enabled
9. spawn background sync when compiled and configured

Do not blur dashboard and collector behavior.

---

## Core Invariants

### 1. Local SQLite is the collector database

Collection writes go to local SQLite first, even with sync enabled.

### 2. Bucket rows are the source of truth

Dashboard views, analytics, import/export merges, and sync convergence derive from bucket rows.

### 3. Import/export must remain idempotent

Do not weaken:

- export UUID tracking
- file hash checks
- duplicate import detection
- merge-safety checks

### 4. `--db-path` is a user-facing contract

It accepts:

- a direct file path
- a directory
- a missing directory-like path

It is also remembered across runs.

### 5. `multi-sync` must remain fully feature-gated

When the feature is off:

- sync code should not compile
- no remote dependency should be required
- local-only behavior must remain intact

### 6. Sync ownership is strict

Each device owns exactly one `source_uuid`.

- local devices push only their own rows
- foreign rows may exist locally
- foreign rows must never be re-enqueued as local outbox rows

### 7. Sync failures must not stop collection

Sync is opportunistic. Remote failure must not stop local writes.

### 8. Dashboard is inspection-only

The dashboard may read while a collector is running, but it must not become a second collector.

### 9. Platform code should stay platform-specific

Share math and tracker logic when behavior is identical. Keep OS event decoding and integration inside the relevant platform modules.

### 10. Startup should launch `vigil collector`

Both Linux and Windows startup artifacts must target the collector subcommand, not an ambiguous bare binary invocation.

---

## Storage Model

Important local tables:

- `schema_meta`
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

Sync-related local tables when `multi-sync` is enabled:

- `sync_state`
- `sync_outbox_sources`
- `sync_outbox_input_buckets`
- `sync_outbox_focus_buckets`

Remote canonical tables:

- `sources`
- `input_buckets`
- `focus_buckets`
- `sync_applied_batches`
- `sync_revisions`
- `sync_source_changes`
- `sync_input_changes`
- `sync_focus_changes`

When schema changes:

- update setup/bootstrap logic
- update tests
- think through import/export effects
- think through sync effects

---

## Dashboard Structure

### `src/tui/mod.rs`

- terminal setup and teardown
- alternate screen
- raw mode handling
- event loop
- periodic refresh

### `src/tui/app.rs`

- state machine
- focus sections
- chart mode
- app-list mode
- time windows
- scrolling and selection
- key handling

### `src/tui/data.rs`

- loads SQLite-backed dashboard data
- builds summary cards, chart data, app lists, week activity rows
- resolves display names
- attaches per-app histograms

### `src/tui/ui.rs`

- ratatui layout
- charts
- app list panel
- week activity grid
- header/footer/help

### Current dashboard behavior

The dashboard currently includes:

- summary cards
- apps activity panel with histograms
- generic and specific app list modes
- activity chart with multiple time windows and chart modes
- week activity grid
- collector and sync status
- focused-panel hints
- ASCII/Unicode display toggle

Important recent behavior:

- the app list now supports:
  - `generic` mode: app-level aggregation
  - `specific` mode: richer app/context labels
- desktop entry matching on Linux is the primary source for app display names
- reverse-DNS and title-based fallbacks exist when desktop entries do not resolve a clean name

When changing the dashboard:

- keep rendering in `ui.rs`
- keep state transitions in `app.rs`
- keep aggregation/queries in `data.rs`
- do not change storage semantics just to support UI display

---

## Linux-Specific Notes

Important files:

- `src/platform/linux/common.rs`
- `src/platform/linux/inputs.rs`
- `src/platform/linux/process.rs`
- `src/platform/linux/wayland.rs`
- `src/platform/linux/x11.rs`

Important behavior:

- Linux startup uses an interactive mode picker
- XDG autostart is the recommended default
- `systemd --user` is the advanced/manual fallback
- startup artifacts should point to the current executable and launch `vigil collector`

High-risk area:

- input measurement drift in `inputs.rs`

---

## Windows-Specific Notes

Important files:

- `src/platform/windows/common.rs`
- `src/platform/windows/inputs.rs`
- `src/platform/windows/process.rs`
- `src/platform/windows/startup.rs`
- `src/platform/windows/systray.rs`

Important behavior:

- Windows collection uses raw input and foreground-process tracking
- Windows startup creates a Startup-folder shortcut that launches `vigil collector`
- Windows systray is optional and collector-only

Recent Windows-specific cleanup:

- systray loop moved onto a blocking task
- tray menu command mapping was refactored into a testable pure path
- startup behavior was kept aligned with the collector/dashboard split

High-risk areas:

- raw input decoding
- tray message loop behavior
- startup shortcut generation

---

## Locks and Concurrency

`src/utils/lock.rs` matters more than it looks.

There are two lock concepts:

1. collector instance lock
   - ensures only one collector runs

2. DB operation lock
   - serializes DB operations for flows like sync/import/export

Do not casually change lock behavior.

Important env var:

- `VIGIL_SKIP_INSTANCE_LOCK=1`
  - testing only

---

## Tests

Current tests are mostly in-module:

- pure logic/unit tests in `common/*`, `platform/*`, `tui/*`, `utils/*`
- storage/integration-style tests in `src/storage/localdb.rs`
- command-routing/integration-style tests in `src/main.rs`
- sync-specific tests in `src/sync/tests.rs`

What the current suite meaningfully covers:

- bucket segmentation
- focus tracker behavior
- input aggregation
- DB path semantics
- import/export behavior
- dashboard state transitions
- app display-name resolution
- CLI parsing boundaries
- Windows startup / tray pure logic helpers

Testing guidance:

- prefer tests that prove observable behavior
- avoid tests for OS or stdlib behavior outside project responsibility
- prefer pure helpers when possible
- use real SQLite-backed tests where merge/import/export behavior matters

Current practical verification commands:

```bash
cargo fmt --all
cargo test --target x86_64-unknown-linux-gnu
cargo check --target x86_64-pc-windows-gnu
cargo check --target x86_64-pc-windows-gnu --all-features
cargo check --tests --target x86_64-pc-windows-gnu
```

Important note:

- in this environment, Windows test linking may fail due to toolchain linkage issues even when Windows code typechecks correctly

---

## Build / Tooling Notes

- `build.rs` generates Linux bindings and embeds Windows icon resources
- `.cargo/config.toml` contains Windows runner config
- `screenshots/` contains dashboard images and capture assets that are useful for docs and UI review

Do not silently break:

- Windows icon/resource embedding
- Linux bindgen generation
- target-specific build behavior

---

## High-Risk Files

Be especially careful in:

- `src/storage/localdb/import.rs`
- `src/storage/localdb/export.rs`
- `src/storage/localdb/schema.rs`
- `src/storage/localdb/rows.rs`
- `src/sync/*`
- `src/platform/linux/inputs.rs`
- `src/platform/windows/inputs.rs`
- `src/platform/windows/systray.rs`
- `src/utils/lock.rs`
- `src/tui/ui.rs`
- `src/tui/data.rs`

Most regressions in these areas are behavioral, not syntactic.

---

## Common Mistakes To Avoid

- making the collector remote-first
- bypassing bucket rows with ad hoc cumulative state
- starting collection from the dashboard path
- treating dashboard display labels as storage semantics
- re-enqueueing foreign sync rows
- weakening duplicate import detection
- conflating app display naming with source data fidelity
- assuming Linux-only validation is enough for Windows-sensitive code

---

## One-Sentence Model

`vigil` is a local-first, bucket-based activity recorder with a read-only dashboard, platform-specific collectors, and optional feature-gated sync, where correctness depends on preserving clean separation between collection, inspection, and history movement.
