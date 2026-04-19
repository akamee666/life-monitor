# Life-Monitor - Agent Guide

## Project Overview

Cross-platform (Linux/Windows) Rust activity tracker that collects keyboard, mouse, scroll, and focused-window activity and stores it in a local SQLite database.

The project is now intentionally local-first:
- local SQLite is the only storage backend
- consistent snapshot export/import is the supported cross-machine workflow
- custom database paths can point to local disks or already-mounted network shares

**Current version:** 0.1.6  
**License:** MIT  
**Repo:** `github.com/akamee666/life-monitor`

---

## Technology Stack

| Component     | Crate/Tool |
| ------------- | ---------- |
| Async runtime | `tokio` |
| Database      | `rusqlite` |
| Linux input   | `nix`, `wayland-client`, `wayland-protocols-wlr`, `x11rb` |
| Windows input | `windows` crate (Raw Input / Win32 APIs) |
| CLI           | `clap` |
| Logging       | `tracing` + `tracing-subscriber` |
| Build         | `bindgen` (Linux input bindings), `embed-resource` (Windows icon) |
| Metadata      | `uuid`, `sha2`, `chrono` |

Notable removals:
- the remote HTTP backend is gone
- `reqwest`, `serde`, and `serde_json` are no longer part of the main storage story

---

## Directory Structure

```text
src/
├── main.rs                    # CLI entry point and import/export short-circuit logic
├── common.rs                  # Shared activity records, bucket buffers, path helpers, shared math
├── input_bindings.rs          # Generated Linux input bindings
├── platform/
│   ├── mod.rs
│   ├── common.rs              # Shared platform helpers
│   ├── linux/
│   │   ├── mod.rs
│   │   ├── common.rs          # Linux startup/systemd helpers
│   │   ├── inputs.rs          # /dev/input reader and raw event aggregation
│   │   ├── process.rs         # Wayland/X11 focus tracking runtime
│   │   ├── wayland.rs         # zwlr_foreign_toplevel_manager_v1 integration
│   │   └── x11.rs             # X11 active window polling
│   └── windows/
│       ├── mod.rs
│       ├── common.rs          # Win32 window/idle helpers
│       ├── inputs.rs          # Raw Input collection and bucket writes
│       ├── process.rs         # Focus tracking runtime
│       └── systray.rs         # Windows tray integration
├── storage/
│   ├── mod.rs
│   ├── backend.rs             # StorageBackend enum and LocalDb runtime wrapper
│   └── localdb.rs             # SQLite schema, import/export, merge, path resolution
└── utils/
    ├── mod.rs
    ├── args.rs                # Clap CLI definition and help text
    ├── dpi.rs                 # DPI persistence and fallback prompting
    ├── lock.rs                # Instance lock and per-database operation lock
    └── logger.rs              # tracing setup and log file creation

.github/
└── workflows/
    ├── nix.yml                # Nix-based CI
    ├── no-nix.yml             # Linux + Windows CI for pushes and PRs
    └── release.yml            # Tag-driven release/publish workflow
```

---

## Cargo Features

| Feature   | Default | What it enables |
| --------- | ------- | ----------------|
| `wayland` | ✅      | Wayland focused-window tracking |
| `x11`     | ❌      | X11 focused-window tracking |

Build variants:

```bash
cargo build
cargo build --features x11
cargo build --target x86_64-pc-windows-gnu
```

---

## Core Runtime Model

### Input data model

Keyboard and mouse data are accumulated into time buckets before being flushed to SQLite.

Important shared types in `src/common.rs`:
- `InputBucketRecord`
- `InputBucketBuffer`
- `BucketMetadata`

Input buckets record:
- `source_id`
- `bucket_start_utc`
- `bucket_end_utc`
- `local_date`
- `local_hour`
- `timezone_offset_minutes`
- `granularity_minutes`
- `left_clicks`
- `right_clicks`
- `middle_clicks`
- `key_presses`
- `mouse_distance_cm`
- `scroll_vertical_cm`
- `scroll_horizontal_cm`

### Focus data model

Focused-window time is also bucketed instead of stored as a single cumulative total.

Important shared types:
- `FocusBucketRecord`
- `FocusBucketBuffer`
- `ProcessTracker`
- `Window`

Focus buckets record:
- source and time bucket boundaries
- normalized app identifier
- window title
- window class
- focus seconds

### Storage backend

`StorageBackend` only has one implementation now:
- `StorageBackend::Local(LocalDb)`

The old API/remote backend no longer exists.

---

## SQLite Schema

Main tables created in `src/storage/localdb.rs`:

- `schema_meta`
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

Purpose of the metadata tables:
- `exports` records snapshot exports so imports can identify snapshot origin
- `imports` prevents duplicate imports of the same snapshot
- `sessions` keeps session-oriented metadata for later reporting and overlap analysis

### Default data locations

- Linux DB: `~/.local/share/life_monitor/data.db`
- Linux log: `~/.local/share/life_monitor/spy.log`
- Windows DB: `%LOCALAPPDATA%\life_monitor\data.db`
- Windows log: `%LOCALAPPDATA%\life_monitor\spy.log`

### Custom database paths

The program accepts `--db-path` pointing to:
- a SQLite file
- a directory, in which case `data.db` is created or reused inside it
- a mounted network share path such as Samba or NFS

When `--db-path` is provided:
- the path is remembered for later runs
- the remembered path is reused until another `--db-path` is provided
- if the remembered path becomes unavailable, the program returns a user-facing recovery message

### Snapshot workflow

Supported movement between machines is file-based:
- `--export-db` creates a consistent SQLite snapshot
- `--import-db` merges a prior snapshot
- `--dry-run` previews the import plan without modifying the destination

Import flow:
1. open destination DB
2. integrity-check source and destination
3. create automatic pre-import backup
4. attach source DB read-only
5. validate schema version
6. merge inside a transaction
7. record import metadata to prevent duplicate imports

---

## Task Architecture

`main()` either:
- handles import/export commands and exits
- or initializes the local DB backend and starts runtime tasks

Runtime tasks:
1. input task
2. process/focus task
3. systray task on Windows only

Ticker pattern:
- `spawn_ticker()` sends `Signals::DbUpdate`
- input and focus buffers are flushed periodically

---

## Platform Notes

### Linux input path

- reads raw events from `/dev/input/event*`
- classifies devices with ioctl capability checks
- aggregates relative mouse motion per report before converting to centimeters
- uses raw evdev events, so desktop pointer acceleration is not part of the measurement path

### Windows input path

- uses Raw Input through a message-only window
- keeps absolute and relative motion handling separate
- shares the same core motion math where possible
- uses real `GetLastInputInfo()`-based idle timing now

### DPI handling

The project now treats DPI/CPI as persistent configuration:
- `--dpi` overrides and remembers the value
- remembered DPI is reused on later runs
- if no DPI is known, interactive runs prompt once and then persist the value
- generic OS-wide automatic CPI detection is not relied on because it is not portable or trustworthy enough

---

## Tests and CI

Current CI:
- `.github/workflows/no-nix.yml`
  - Ubuntu format/check/test/build
  - Windows test/build
- `.github/workflows/nix.yml`
  - Nix-based validation
- `.github/workflows/release.yml`
  - tag-driven release validation and crates.io publish

Current release workflow:
- triggers on tags like `v0.1.6`
- verifies tag version matches `Cargo.toml`
- reruns Linux and Windows validation
- runs `cargo package`
- publishes to crates.io
- creates a GitHub release entry
- attaches Linux and Windows release archives to the GitHub release

Current limitation:
- changelog generation is still not wired into CI automatically; `git-cliff` config exists but the release notes/changelog update step is still manual

---

## Today’s Changes

This session introduced or finalized:
- removal of the remote backend
- local-first import/export workflow
- remembered DB paths
- mounted-share-friendly DB path handling
- operation locking around DB writes/import/export
- new bucket-oriented schema and merge logic
- improved Clap help output
- DPI persistence and interactive fallback
- more explicit user-facing logging around storage and share failures
- Linux raw-motion accuracy fix by aggregating `REL_X` and `REL_Y` per report
- reduced Windows input boilerplate and added Windows CI coverage
- tag-driven release workflow for crates.io publishing

---

## Places To Improve

### Code that can be refactored

- `src/storage/localdb.rs`
  - still holds too many responsibilities: schema creation, path resolution, import planning, merge execution, hashing, backups, and tests
  - a future split could separate:
    - schema/setup
    - import/export
    - DB path/config
    - query helpers

- `src/common.rs`
  - central and useful, but now broad
  - bucket records, path helpers, and math helpers may eventually deserve smaller modules

- platform focus runtimes
  - Linux and Windows now share the storage model, but focus-loop control flow is still structured differently
  - there is room to unify higher-level flush behavior while keeping platform-specific discovery logic separate

### Code that still needs work

- Windows startup support is still `unimplemented!()`
- there is no built-in UI/TUI/dashboard yet
- release workflow does not yet upload platform binaries as downloadable assets
- changelog generation is still manual unless an external tool is added
- DB locking across remote shares is still best-effort and depends on share/filesystem semantics

---

## Missing Product Features

- import conflict visualization beyond the current dry-run summary
- richer analytics and built-in summaries
- session-level reports built on the `sessions` table
- better calibration workflow for mouse DPI/CPI
- optional release artifacts for users who do not install via Cargo

---

## Common Commands

```bash
cargo check
cargo build
cargo build --release
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check

nix develop --command ci-checks
nix develop --command ci-test-build
nix build .#linux
nix build .#windows

cargo run -- --debug
cargo run -- --db-path /mnt/shared/life-monitor
cargo run -- --export-db ./snapshot.sqlite
cargo run -- --import-db ./snapshot.sqlite --dry-run
```
