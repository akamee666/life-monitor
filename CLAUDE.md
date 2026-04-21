# Vigil — Claude Code Guide

## Project Summary

`vigil` is a cross-platform Rust activity tracker for Linux and Windows.

Tracks: keyboard activity, mouse movement/clicks/scroll, and active window focus over time.

Storage: local SQLite using bucketed records (`InputBucketRecord`, `FocusBucketRecord`).

Optional feature-gated multi-device sync (`--features multi-sync`) uses a remote `sqld`/libSQL endpoint as the canonical merged store. Disabled and not compiled by default.

Current release: `1.0.0` / tags: `vX.Y.Z`

---

## Architecture

```
src/
├── main.rs                        # entry point, command routing, runtime setup
├── input_bindings.rs
├── common/                        # shared cross-platform logic
│   ├── buckets.rs
│   ├── focus.rs
│   ├── input.rs
│   ├── motion.rs
│   ├── paths.rs
│   ├── process.rs
│   ├── ticker.rs
│   └── types.rs
├── platform/
│   ├── linux/
│   │   ├── common.rs              # startup generation + interactive mode picker
│   │   ├── inputs.rs              # raw input — measurement-sensitive
│   │   ├── process.rs
│   │   ├── wayland.rs
│   │   └── x11.rs
│   └── windows/
│       ├── common.rs
│       ├── inputs.rs              # raw input + message loop — easy to break
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
├── sync/                          # compiled only with `multi-sync` feature
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
├── tui/                           # read-only ratatui dashboard
│   ├── app.rs
│   ├── data.rs
│   ├── mod.rs
│   └── ui.rs
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

| Task                                                     | Files                                             |
| -------------------------------------------------------- | ------------------------------------------------- |
| CLI behavior, command routing                            | `src/main.rs`, `src/utils/args.rs`                |
| Bucket logic, motion math, focus buffering, path helpers | `src/common/*`                                    |
| DB schema, migrations, import/export, analytics          | `src/storage/localdb/*`, `src/storage/backend.rs` |
| Read-only dashboard                                      | `src/tui/*`                                       |
| Multi-device sync (feature-gated)                        | `src/sync/*`, `src/main.rs`, `src/utils/args.rs`  |
| Linux raw input                                          | `src/platform/linux/inputs.rs`                    |
| Linux startup generation + picker UI                     | `src/platform/linux/common.rs`                    |
| Windows raw input, focus, startup, systray               | `src/platform/windows/`                           |
| DPI persistence                                          | `src/utils/dpi.rs`                                |
| Lock and multi-process coordination                      | `src/utils/lock.rs`                               |
| CI and release flow                                      | `.github/workflows/`                              |

---

## Runtime Flow

`main.rs` has three distinct execution paths. Keep them clean — do not blur the boundaries.

### 1. `vigil collector` — long-running writer

1. Handle short-circuit flags first (startup enable/disable, export, import) — these do work and exit
2. Acquire single-instance lock
3. Resolve DB path
4. Resolve DPI
5. Initialize local DB backend
6. Spawn input collection
7. Spawn focus/process collection
8. Spawn Windows systray (when enabled)
9. Spawn background sync loop (only when `multi-sync` compiled and configured)

### 2. `vigil dashboard` — read-only inspection

- Opens the ratatui dashboard
- Reads from local SQLite
- Refreshes periodically
- Does not acquire the collector lock
- Does not start collection

### 3. `vigil sync` — short-circuit sync commands (multi-sync only)

- push / pull / status
- Exits after the operation

Short-circuit commands must not start the collector and exit later. The dashboard must never implicitly start collection.

---

## Core Invariants

These define the product. Do not violate them without explicit intent to change the product model.

**1. Local SQLite is always the collector database.**
Even with `multi-sync`, all collection writes go to local SQLite first. The collector is never remote-first.

**2. Bucket rows are the source of truth.**
All totals, analytics, import/export merges, and sync convergence must derive from `InputBucketRecord` and `FocusBucketRecord` rows. Do not introduce mutable running totals as primary persisted state.

**3. Import/export idempotency is non-negotiable.**
Do not weaken: file hash checks, export UUID checks, duplicate import guards. These prevent doubled totals.

**4. `--db-path` is a user-facing contract.**
Accepts a file, a directory, or a missing directory-like path. Remembered across runs. Users may rely on this for removable disks or mounted shares. Do not change its resolution or persistence behavior casually.

**5. `multi-sync` must stay fully feature-gated.**
When the feature is off: no sync code compiles, no libSQL dependency is required, local collection is fully unchanged. Do not leak sync concepts into the default build.

**6. Sync source ownership is strict.**
Each device owns exactly one `source_uuid` and may push only its own rows. Pulled foreign rows may be stored and queried locally but must never be re-enqueued as local outbox entries.

**7. Sync operations must be retry-safe.**
Push and pull are idempotent. No duplicate canonical rows on retry. Outbox rows are not marked sent before remote acknowledgement. Pull cursor does not advance before a full successful apply.

**8. Foreign source metadata must be real.**
When pulling foreign-source rows, the real remote `sources` metadata is inserted first. Do not create placeholder source rows with guessed names or local platform values.

**9. Sync failure must not stop collection.**
Remote unavailability or sync errors: local collection continues, local writes continue, outbox rows stay queued, status records the failure.

**10. Platform logic is shared only where behavior is truly identical.**
Motion math, bucket buffering, and tracker state transitions are safe to share. Platform event decoding and OS integration stay in their respective `platform/` subtrees.

**11. Release tags must match `Cargo.toml`.**
The release workflow enforces this. Do not alter release logic in a way that allows publishing a mismatched version.

**12. Linux startup follows XDG-first, systemd-fallback.**

- Default: XDG autostart desktop entry
- Fallback: `systemd --user` unit tied to graphical session (explicit, advanced)
- Startup artifacts must point to the executable used at enable time and launch `vigil collector`
- Do not bake volatile variables like `WAYLAND_DISPLAY` into the systemd unit
- Do not mutate the wider systemd user manager environment
- Warn when startup is enabled from a `target/debug` or `target/release` path

**13. The dashboard is inspection-only.**
The dashboard may read and refresh from SQLite while a collector runs. It must not acquire the collector lock, mutate data, or start collection implicitly.

---

## Dashboard Architecture

The TUI lives in `src/tui/`:

- **`mod.rs`** — terminal setup/teardown, alternate screen, event loop, refresh timer
- **`app.rs`** — dashboard state machine, focus sections, keyboard handling, time window and chart mode state, selection/scroll
- **`data.rs`** — SQLite-backed data loading; aggregates app usage, chart series, summary totals, heatmap rows; dashboard-facing presentation models
- **`ui.rs`** — ratatui layout and rendering (charts, app list, heatmap, header/footer/help)

Dashboard behavior:

- Time windows: `All`, `1h`, `6h`, `24h`, `7d`, `30d` — affect summary cards, app list, chart
- Chart modes: single metric / scope overlay
- App activity panel: app names, per-app histograms, scroll, scrollbar
- Week activity grid: daily metric breakdown, row selection, current-day highlighting
- Footer: focused-panel hints, ASCII/Unicode toggle hint
- Header: time range, collector status (collecting / idle / stale), sync status

When editing TUI code:
- Keep rendering in `ui.rs`
- Keep state transitions in `app.rs`
- Keep SQL and aggregation in `data.rs`
- Prefer deriving new views from existing bucket data; avoid changing storage semantics for a UI feature
- Check wide and narrow terminal layouts; most regressions are visual, not compiler errors

---

## Storage Tables

**Local (always present):**

- `schema_meta`, `sources`, `input_buckets`, `focus_buckets`
- `exports`, `imports`, `sessions`

**Local sync tables (`multi-sync` only):**

- `sync_state`, `sync_outbox_sources`, `sync_outbox_input_buckets`, `sync_outbox_focus_buckets`

**Remote canonical tables (`multi-sync` only):**

- `sources`, `input_buckets`, `focus_buckets`
- `sync_applied_batches`, `sync_revisions`
- `sync_source_changes`, `sync_input_changes`, `sync_focus_changes`

When changing schema: update schema setup, migrations, and bootstrap logic. Update tests. Think through import/export behavior and sync behavior if `multi-sync` is enabled. Schema compatibility with older versions is not a current priority — keep it simple unless explicitly required.

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `VIGIL_DATA_DIR` | Override the default data directory |
| `VIGIL_SKIP_INSTANCE_LOCK` | Set to `1` to skip the single-instance lock (testing only) |
| `VIGIL_SYNC_REMOTE_URL` | Remote sqld/libSQL endpoint (`multi-sync` feature) |
| `VIGIL_SYNC_AUTH_TOKEN` | Auth token for the sync remote (`multi-sync` feature) |

Default data directories:
- Linux: `~/.local/share/vigil/`
- Windows: `%LOCALAPPDATA%\vigil\`

---

## Build and Test

### Flake environments

Always build and test from the `flake.nix` environment. Always pass `--target` explicitly.

```bash
# Format
nix develop --command cargo fmt --all

# Linux build and test
nix develop --command cargo build --target x86_64-unknown-linux-gnu
nix develop --command cargo test --target x86_64-unknown-linux-gnu

# Windows cross-compile (from Linux host)
nix develop --command cargo build --target x86_64-pc-windows-gnu
nix develop --command cargo check --target x86_64-pc-windows-gnu

# Nix package builds
nix build .#linux
nix build .#windows   # cross-compiled, not native Windows
```

### SQLite bundling

`rusqlite` is bundled on both platforms. Preserve this:

- Linux runtime must not depend on a system `libsqlite3.so`
- Windows runtime must not depend on an external SQLite install
- If toolchain changes break bundled SQLite compilation, fix the toolchain — do not silently fall back to a system dependency

### Windows testing

Wine is useful for local smoke checks but is not authoritative. Native Windows CI is the authoritative runtime gate. Do not contort production code to satisfy Wine-specific behavior.

### Test philosophy

Write tests that verify: observable behavior, ownership rules, state transitions, import/export/merge outcomes, retry and idempotency behavior, analytics outputs from real stored rows.

Do not write tests that primarily verify: OS behavior, static message strings, broad smoke paths with many unrelated failure points, or timing-sensitive behavior where a direct unit test would be clearer.

Use the lowest stable test level that proves the behavior.

---

## Working in This Repo

### Before making changes

1. Identify the task area: collection, storage/import/export, sync, CLI/config, TUI/dashboard, platform-specific, CI/release.
2. Read the narrowest responsible files first.
3. Confirm the change preserves: local-first operation, bucket-based storage, explicit snapshot import/export, feature-gated sync, dashboard read-only contract.

### Commits

Split commits by feature or coherent change. Do not bundle unrelated changes (feature + schema + CI + docs) into one commit.

Each commit message:

- **Title**: concise description of what changed
- **Body**: what changed, how it changed, why it changed

### Claude Code tool use

- Run `cargo check` and `cargo test` before declaring a change done
- Always pass `--target` explicitly when building
- Do not auto-run anything that writes to the database or modifies startup entries without confirming with the user
- Do not run Windows-target binaries directly; use `cargo check` for cross-compile validation

---

## High-Risk Areas

Extra care required:

| Area                             | Risk                                                     |
| -------------------------------- | -------------------------------------------------------- |
| `src/storage/localdb/*`          | Schema, import/export, row merge — wide blast radius     |
| `src/sync/*`                     | Ownership, idempotency, cursor handling — fails subtly   |
| `src/common/*`                   | Regressions hit both platforms simultaneously            |
| `src/platform/linux/inputs.rs`   | Measurement accuracy can regress silently                |
| `src/platform/windows/inputs.rs` | Raw input and message-loop behavior are fragile          |
| `src/utils/lock.rs`              | Filesystem semantics vary across environments and mounts |
| `src/tui/ui.rs`                  | Visual/layout regressions are not compiler errors        |
| `.github/workflows/release.yml`  | Publishing and asset attachment are easy to break        |

---

## Anti-Patterns

- Reintroducing a remote backend into the default (non-`multi-sync`) product path
- Bypassing bucket storage with ad hoc cumulative counters
- Enqueueing pulled foreign rows into the sync outbox
- Marking outbox rows sent before remote acknowledgement
- Advancing the pull cursor before a full successful apply
- Weakening import/export hash or UUID checks
- Assuming Linux-only validation is sufficient for Windows behavior
- Bundling unrelated feature, schema, CI, and docs changes into one commit
- Starting collection from the dashboard path
- Adding CLI analytics flags — the dashboard is the analytics surface

---

## Known Limitations

- Wine cannot replace native Windows runtime validation
- Remote share behavior depends on OS mount semantics and is best-effort
- Some sync/outbox seams still have room for simplification without changing module boundaries

---

## Mental Model

`vigil` is a local-first, bucket-based activity recorder with three surfaces: a long-running **collector** that writes to local SQLite, a read-only **dashboard** for inspection, and optional feature-gated **sync/import-export** for history movement. Core guarantees: accurate input collection, safe SQLite persistence, explicit snapshot movement of history, and optional multi-device convergence that never interrupts local collection.
