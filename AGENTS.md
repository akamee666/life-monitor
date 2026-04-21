# Vigil — Agent Guide

## Purpose

Use this file to get oriented quickly in `vigil` and avoid breaking the parts of the product that are easy to regress:

- local-first collection
- bucket-based storage
- import/export merge safety
- feature-gated multi-device sync
- platform-specific collectors
- the read-only ratatui dashboard

This guide is intentionally practical. It is optimized for helping the next model find the right files, preserve product invariants, and choose the right verification steps.

---

## Project Summary

`vigil` is a cross-platform Rust activity tracker for Linux and Windows.

It records:

- keyboard activity
- mouse movement, clicks, and scroll
- focused-window / active-app activity over time

It stores data in a local SQLite database using bucketed rows:

- `InputBucketRecord`
- `FocusBucketRecord`

It also supports:

- snapshot export/import
- a read-only ratatui dashboard (`vigil dashboard`)
- optional feature-gated multi-device sync (`--features multi-sync`)

Current release line:

- crate version: `1.0.0`
- release tags: `vX.Y.Z`

---

## Mental Model

The product has three distinct surfaces:

1. **collector** (`vigil collector`)
   - long-running process that writes activity into local SQLite

2. **dashboard** (`vigil dashboard`)
   - read-only inspection and analytics
   - does not start collection, does not acquire the collector lock

3. **history movement / convergence**
   - export/import snapshots (under `collector`)
   - optional sync with a remote canonical store (`vigil sync`, `multi-sync` feature)

If you keep those surfaces separate, most changes fall into place.

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
│   │   ├── mod.rs
│   │   ├── process.rs
│   │   ├── wayland.rs
│   │   └── x11.rs
│   └── windows/
│       ├── common.rs
│       ├── inputs.rs
│       ├── mod.rs
│       ├── process.rs
│       ├── startup.rs
│       └── systray.rs
├── storage/
│   ├── backend.rs
│   ├── mod.rs
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
│   ├── app.rs
│   ├── data.rs
│   ├── mod.rs
│   └── ui.rs
└── utils/
    ├── args.rs
    ├── dpi.rs
    ├── lock.rs
    ├── logger.rs
    └── mod.rs
```

---

## Where To Look First

| Task | Files |
| --- | --- |
| CLI flags, command routing, startup flow | `src/main.rs`, `src/utils/args.rs` |
| Shared bucket logic, focus buffering, motion math | `src/common/*` |
| SQLite schema, import/export, analytics | `src/storage/localdb/*`, `src/storage/backend.rs` |
| Read-only terminal dashboard | `src/tui/mod.rs`, `src/tui/app.rs`, `src/tui/data.rs`, `src/tui/ui.rs` |
| Linux raw input collection | `src/platform/linux/inputs.rs` |
| Linux startup generation + picker UI | `src/platform/linux/common.rs` |
| Windows raw input / focus / startup / tray | `src/platform/windows/*` |
| Sync implementation | `src/sync/*`, `src/main.rs`, `src/utils/args.rs` |
| Locking and multi-process coordination | `src/utils/lock.rs` |
| DPI persistence and resolution | `src/utils/dpi.rs` |
| CI / packaging / release | `.github/workflows/*`, `flake.nix` |

---

## Current Runtime Flow

`src/main.rs` has three execution paths.

### 1. `vigil collector` — short-circuit maintenance commands

These do work and exit:

- startup enable/disable
- `--export-db`
- `--import-db` / `--import-db --dry-run`
- `sync push/pull/status` when `multi-sync` is enabled

### 2. `vigil dashboard` — read-only dashboard

- opens the ratatui dashboard
- reads from the local SQLite DB
- refreshes periodically
- does not start the collector
- does not acquire the single-instance lock

This separation matters. The dashboard is an inspection tool, not a collection mode.

### 3. `vigil collector` — long-running collector

Normal run without short-circuit flags:

1. resolve DB path
2. acquire single-instance lock
3. resolve mouse DPI
4. initialize the local SQLite backend
5. start platform input collection
6. start focus/process collection
7. start Windows systray if enabled
8. start opportunistic sync loop only when compiled and configured

Do not blur these paths together.

---

## TUI Structure

### `src/tui/mod.rs`

- terminal setup / teardown
- alternate screen and raw mode handling
- event loop
- periodic refresh

### `src/tui/app.rs`

- dashboard state machine
- focus sections
- keyboard handling
- time window state (`All`, `1h`, `6h`, `24h`, `7d`, `30d`)
- chart mode state (single / scope overlay)
- list / heatmap selection and scrolling

### `src/tui/data.rs`

- loads dashboard data from SQLite
- aggregates app usage, chart series, summary totals, heatmap rows
- contains dashboard-facing presentation models

### `src/tui/ui.rs`

- ratatui layout
- chart rendering
- app list rendering with per-app histograms
- week activity grid rendering
- header/footer/help modal

### Current dashboard behavior

The dashboard is read-only and shows:

- summary cards (totals for the selected time window)
- app activity list with per-app histograms, selection and scrolling
- activity chart with multiple time windows and modes
- week activity grid with row selection and current-day highlighting
- footer hints and collector/sync status

If you change the TUI, verify both behavior and layout. Most TUI regressions are not compiler errors; they show up as clipped text, dead space, bad resizing, or misleading status.

---

## Product Invariants

These are the constraints most changes must preserve.

### 1. Local SQLite is the collector database

Even with `multi-sync`, collection writes go to local SQLite first.

### 2. Bucket rows are the source of truth

Totals, analytics, TUI aggregates, import/export behavior, and sync convergence all derive from bucket rows.

### 3. Import/export must stay idempotent

Do not weaken:

- export UUID checks
- import history tracking
- snapshot hash checks
- duplicate detection

### 4. `--db-path` is a user-facing contract

It accepts a file, a directory, or a missing directory-like path. It is remembered across runs.

### 5. `multi-sync` must remain fully feature-gated

When the feature is off: sync code must not compile, no remote dependency should be required, default local-only behavior must stay intact.

### 6. Sync source ownership is strict

Each device owns exactly one `source_uuid`. Pulled foreign rows may exist locally but must never be re-enqueued as local outbox rows.

### 7. Sync must be retry-safe

No duplicate canonical rows on retry. No marking outbox rows as sent before acknowledgement. No cursor advance before a full successful pull apply.

### 8. Sync failure must not stop collection

If the remote is unavailable, local collection and local writes continue.

### 9. The dashboard is inspection-only

The dashboard must not acquire the collector instance lock, mutate activity data, or start background collection implicitly.

### 10. Platform behavior should only be shared when it is actually shared

Motion math, bucket segmentation, and focus buffering are safe to share. Keep OS event decoding and OS integration inside `platform/linux` or `platform/windows`.

### 11. Linux startup remains XDG-first

- `xdg` autostart as the default
- `systemd --user` as an explicit advanced fallback
- both artifacts launch `vigil collector`

### 12. Release tags must match `Cargo.toml`

The workflow enforces this.

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `VIGIL_DATA_DIR` | Override the default data directory |
| `VIGIL_SKIP_INSTANCE_LOCK` | Set to `1` to skip the single-instance lock (testing only) |
| `VIGIL_SYNC_REMOTE_URL` | Remote sqld/libSQL endpoint (`multi-sync` only) |
| `VIGIL_SYNC_AUTH_TOKEN` | Auth token for the sync remote (`multi-sync` only) |

---

## Storage and Sync Tables

### Local tables

- `schema_meta`
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

### Local sync tables (`multi-sync` only)

- `sync_state`
- `sync_outbox_sources`
- `sync_outbox_input_buckets`
- `sync_outbox_focus_buckets`

### Remote canonical sync tables

- `sources`
- `input_buckets`
- `focus_buckets`
- `sync_applied_batches`
- `sync_revisions`
- `sync_source_changes`
- `sync_input_changes`
- `sync_focus_changes`

---

## Build and Test

Use the flake environment and pass `--target` explicitly.

```bash
nix develop --command cargo fmt --all
nix develop --command cargo build --target x86_64-unknown-linux-gnu
nix develop --command cargo test --target x86_64-unknown-linux-gnu
nix develop --command cargo build --target x86_64-pc-windows-gnu
nix develop --command cargo check --target x86_64-pc-windows-gnu
nix build .#linux
nix build .#windows
```

Notes:

- `nix build .#windows` is a Linux-hosted cross-build, not native Windows execution.
- the dev shell intentionally separates host Linux and cross Windows toolchains

### SQLite expectation

`rusqlite` is bundled on both platforms. Linux runtime should not depend on a system `libsqlite3.so`. Windows runtime should not require an external SQLite install.

### Windows validation

Wine can help locally, but native Windows CI is the real runtime gate.

---

## Change Strategy

### Start narrow

Before editing:

1. identify the task area
2. open the smallest responsible files first
3. confirm which invariant matters most for that change

### Prefer additive changes in the TUI/data layers

For dashboard work:

- prefer deriving new views from existing bucket data
- avoid changing storage semantics just to support a UI feature
- keep rendering concerns in `ui.rs`
- keep state transitions in `app.rs`
- keep SQL/aggregation in `data.rs`

### Keep commits coherent

Prefer separate commits for product behavior changes, schema changes, sync changes, TUI/dashboard changes, and docs-only updates.

Commit messages should include: what changed, how it changed, why it changed.

---

## High-Risk Areas

Be extra careful in:

- `src/storage/localdb/*` — schema, import/export, merge logic, analytics
- `src/sync/*` — ownership, outbox safety, cursor handling, retry semantics
- `src/common/*` — shared logic affects both platforms
- `src/platform/linux/inputs.rs` — input measurement can drift silently
- `src/platform/windows/inputs.rs` — raw input and message loop code are easy to break subtly
- `src/utils/lock.rs` — filesystem and lock semantics vary by environment
- `src/tui/ui.rs` — many regressions are visual/layout regressions, not compile failures

---

## TUI-Specific Gotchas

When editing the dashboard:

- check wide and narrow layouts
- check fullscreen and medium terminal sizes
- avoid leaving dead interior space inside bordered panels
- protect duration strings and labels from clipping
- treat charts and scrollable lists as the primary sinks for extra space
- keep footer/header status high-signal and compact
- remember the dashboard may run while the collector is active elsewhere

If a TUI change needs new data, prefer adding an additive aggregation in `src/tui/data.rs` instead of altering storage design.

---

## Test Philosophy

Prefer tests that prove:

- observable behavior
- merge/import/export outcomes
- sync ownership and retry behavior
- dashboard state transitions and input handling
- analytics derived from stored rows

Avoid tests that mostly prove:

- the OS itself
- exact wording of UI copy unless it is product-critical
- brittle timing behavior
- giant smoke paths that obscure the real failing invariant

Use the lowest stable test level that proves the behavior.

---

## Common Mistakes To Avoid

- making the collector remote-first
- bypassing bucket rows with ad hoc totals
- starting collection from the dashboard path
- re-enqueueing pulled foreign rows
- advancing sync state before a full successful apply
- mixing unrelated feature, docs, CI, and schema changes into one commit
- assuming Linux-only validation is enough for Windows-sensitive code

---

## One-Sentence Model

`vigil` is a local-first, bucket-based activity recorder with a read-only analytics dashboard and optional feature-gated sync, where correctness depends on preserving local collection, safe SQLite semantics, and clear separation between collection, inspection, and history movement.
