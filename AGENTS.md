# Life-Monitor - Agent Guide

## Project Overview

Cross-platform (Linux/Windows) Rust activity tracker that collects keyboard/mouse input metrics and active window data, persisting to SQLite. Built for personal productivity analytics and blog content.

**Current version:** 0.1.5 | **License:** MIT | **Repo:** github.com/akamee666/life-monitor

---

## Technology Stack

| Component     | Crate/Tool                                                                               |
| ------------- | ---------------------------------------------------------------------------------------- |
| Async runtime | `tokio` (multi-thread, features: rt-multi-thread, time, macros, sync, net)               |
| Database      | `rusqlite` (bundled on Windows, system lib on Linux)                                     |
| Linux input   | `nix`, `wayland-client`, `wayland-protocols-wlr`, `x11rb`                                |
| Windows input | `windows` crate (Win32 Raw Input API)                                                    |
| CLI           | `clap` (derive)                                                                          |
| HTTP (remote) | `reqwest` (optional feature)                                                             |
| Serialization | `serde` + `serde_json` (optional, tied to `remote` feature)                              |
| Logging       | `tracing` + `tracing-subscriber`                                                         |
| Build         | `bindgen` (Linux only, generates `/dev/input` bindings), `embed-resource` (Windows icon) |

---

## Directory Structure

```
src/
├── main.rs                    # Entry point, task orchestration via JoinSet
├── common.rs                  # Shared structs: InputLogger, ProcessInfo, ProcessTracker, Signals, spawn_ticker(), program_data_dir()
├── input_bindings.rs          # Auto-generated Linux input event bindings (OUT_DIR/input_bindings.rs)
├── platform/
│   ├── mod.rs
│   ├── common.rs              # Window struct, record_window_time()
│   ├── linux/
│   │   ├── mod.rs
│   │   ├── common.rs          # systemd startup config, uptime() via /proc/uptime
│   │   ├── inputs.rs          # /dev/input reader, ioctl device detection, async event loop
│   │   ├── process.rs         # Window tracker: X11 polling loop + Wayland event-driven loop (TrackingState FSM)
│   │   ├── wayland.rs         # zwlr_foreign_toplevel_manager_v1 Wayland protocol impl
│   │   └── x11.rs             # X11 active window via _NET_ACTIVE_WINDOW
│   └── windows/
│       ├── mod.rs
│       ├── common.rs          # GetForegroundWindow, GetLastInputInfo, MouseSettings, uptime()
│       ├── inputs.rs          # Win32 Raw Input API (WM_INPUT), message-only window
│       ├── process.rs         # Window polling loop (1s tick)
│       └── systray.rs         # NOTIFYICONDATAW tray icon, context menu
├── storage/
│   ├── mod.rs
│   ├── backend.rs             # StorageBackend enum, LocalDb, DataStore trait
│   ├── localdb.rs             # SQLite schema, granularity logic, bucket-based time updates
│   └── remote.rs              # HTTP API backend (feature = "remote")
└── utils/
    ├── mod.rs
    ├── args.rs                # Clap CLI struct
    ├── lock.rs                # Single-instance enforcement via flock (Linux) / LockFile (Windows)
    └── logger.rs              # tracing_subscriber setup, dual output (file + stdout), spy.log
.github/
└── workflows/
    ├── nix.yml                # GitHub Actions CI using the repo's nix develop shell
    └── no-nix.yml             # GitHub Actions CI using standard Ubuntu + Rust setup
```

---

## Cargo Features

| Feature   | Default | What it enables                                                            |
| --------- | ------- | -------------------------------------------------------------------------- |
| `wayland` | ✅      | `wayland-client`, `wayland-protocols-wlr`                                  |
| `x11`     | ❌      | `x11rb` window tracking                                                    |
| `remote`  | ✅      | `reqwest`, `serde`, `serde_json`; activates `RemoteDb` and `--remote` flag |

Build variants:

```bash
cargo build                          # Linux, Wayland (default)
cargo build --features x11           # Linux, X11
cargo build --features remote        # Adds remote API support
cargo build --target x86_64-pc-windows-gnu  # Windows cross-compile
```

---

## Core Data Structures

### `InputLogger` (`src/common.rs`)

Tracks cumulative input metrics per session. Loaded from DB on startup, updated in memory, flushed on interval.

- `left_clicks`, `right_clicks`, `middle_clicks`, `key_presses`
- `pixels_traveled`, `cm_traveled` (computed from DPI), `mouse_dpi`
- `vertical_scroll_clicks/cm`, `horizontal_scroll_clicks/cm`
- `w: WindowsSpecific` (Windows-only: pressed key state, screen dimensions, last abs position)

### `ProcessInfo` (`src/common.rs`)

- `w_name: String` — window title
- `w_class: String` — process/app name
- `w_time: u64` — seconds focused

### `ProcessTracker` (`src/common.rs`)

Wraps `Vec<ProcessInfo>` with last-seen window state and uptime timestamp.

### `StorageBackend` (`src/storage/backend.rs`)

Enum: `Local(LocalDb)` | `Api(RemoteDb)`. Both implement `DataStore` trait:

- `store_keys_data`, `get_keys_data`
- `store_proc_data`, `get_proc_data`

---

## SQLite Schema

**`procs` table** — always 1 row per unique `window_name`:

```sql
id INTEGER PK, window_name TEXT, time_focused INTEGER, window_class TEXT
```

**`keys` table** — rows depend on granularity level:

| Level       | Rows/day | Interval |
| ----------- | -------- | -------- |
| 0 (default) | 1        | 24h      |
| 1           | 6        | 4h       |
| 2           | 12       | 2h       |
| 3           | 24       | 1h       |
| 4           | 48       | 30min    |
| 5           | 96       | 15min    |

```sql
id INTEGER PK, left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled INTEGER, timestamp TEXT (HH:MM)
```

Updates use `find_bucket()` to floor current time to the nearest interval and `UPDATE WHERE timestamp = ?`.

**Data paths:**

- Linux: `~/.local/share/life_monitor/data.db`
- Windows: `%LOCALAPPDATA%\life_monitor\data.db`
- Log file: same dir, `spy.log`

---

## Task Architecture

`main()` spawns tasks into a `JoinSet`, any task returning = fatal error:

1. **Input task** — `platform::{linux,windows}::inputs::run()` — reads raw input events, accumulates to `InputLogger`, flushes on `DbUpdate` signal
2. **Process task** — `platform::{linux,windows}::process::run()` — tracks active window time, flushes on interval
3. **Systray task** (Windows only) — `platform::windows::systray::init_tray()`

Ticker pattern: `spawn_ticker(tx, Duration, Signals::DbUpdate)` sends signals to tasks.

### Linux Input Architecture

- `discover_devices()` scans `/dev/input`, classifies via ioctl bitmask checks (`is_keyboard`, `is_mouse`)
- Each device gets its own `AsyncFd<File>` task
- Events sent over `mpsc::channel::<InputEvent>` to main loop
- Idle detection: `static mut IDLE_TIME: u64` updated by 20s ticker comparing `event.time` to `SystemTime::now()`

### Linux Window Tracking (Wayland)

`TrackingState` FSM: `NoFocus` → `Active(Window, Instant)` ↔ `Idle(Window)`

- `FocusEvent::FocusGained/FocusLost` from `zwlr_foreign_toplevel_handle_v1`
- Idle check pauses timer, resumes on activity

### Windows Input Architecture

- Message-only `HWND` window with `RIDEV_INPUTSINK` for keyboard + mouse
- `WM_INPUT` → `handle_raw_input()` → `mpsc::Sender<RawInputEvent>`
- Runs in `spawn_blocking` thread (Win32 message loop is blocking)

---

## Known Issues / TODOs in Code

- `is_idle()` on Windows still returns `true` based on uptime instead of real idle duration
- `configure_startup()` on Windows is still `unimplemented!()`
- Scroll tracking on Windows (`RI_MOUSE_WHEEL`) reads `usButtonData` but still does not use it
- The remote backend still needs docs and more error-handling cleanup

---

## Unresolved Product Gaps

- There is still no built-in visual representation of the collected data. The project stores metrics in SQLite, but users must inspect the database manually or use external tools. A TUI is planned but does not exist yet.
- Data is still installation-local by default. If one person uses `life-monitor` across multiple operating systems or multiple computers, each installation keeps its own database and the activity data remains split.
- The current remote backend is optional and does not yet provide a complete, reliable sync story for merging one user's history across different machines or dual-boot setups.
- There is no built-in import/export or merge workflow for combining data gathered from separate installations into one canonical dataset.

---

## Build System

**`build.rs`:**

- Linux: runs `bindgen` on `linux/input.h` + `linux/input-event-codes.h` → `$OUT_DIR/input_bindings.rs`
- All platforms: `embed-resource::compile("icon-resource.rc")` for Windows icon embedding

**`flake.nix`** (NixOS):

```bash
nix build .#linux     # Linux build
nix build .#windows   # Cross-compile to x86_64-pc-windows-gnu via mingwW64
nix develop           # Dev shell with full toolchain + wine64 for testing Windows binary
```

Dev shell sets `LIBCLANG_PATH`, `BINDGEN_EXTRA_CLANG_ARGS`, `WINEPREFIX` automatically.

**GitHub Actions CI** (`.github/workflows/nix.yml`, `.github/workflows/no-nix.yml`):

- Both trigger on `push` and `pull_request`
- `nix.yml` runs `nix develop --command ci-checks` and `nix develop --command ci-test-build`
- `no-nix.yml` runs `cargo fmt -- --check`, `cargo-deny`, `cargo test`, and `cargo build --release`
- Local Nix helper commands:
  - `nix develop --command ci-checks`
  - `nix develop --command ci-test-build`
  - `nix develop --command ci-local`

---

## CLI Reference

```
-i/--interval <SECS>    DB update interval [default: 300]; debug mode uses 5
-g/--gran <0-5>         Granularity level for keys table [default: 0]
-d/--debug              Verbose logging + 5s interval
-p/--dpi <DPI>          Mouse DPI for cm calculation [default: 800]
-c/--clear              Delete data.db and start fresh
-r/--remote <FILE>      JSON config for remote API (requires `remote` feature)
--enable-startup        Install systemd user service (Linux) / Startup shortcut (Windows, unimplemented)
--disable-startup       Remove startup config
-s/--no-systray         Disable tray icon (Windows only)
```

---

## Environment Variables

| Variable                | Purpose                                          |
| ----------------------- | ------------------------------------------------ |
| `WAYLAND_DISPLAY`             | Wayland session detection                        |
| `WAYLAND_SOCKET`              | Additional Wayland session detection             |
| `XDG_SESSION_TYPE`            | Session type fallback for Wayland/X11 detection  |
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland-specific Wayland session hint           |
| `DISPLAY`                     | X11 session detection / Xwayland compatibility   |
| `RUST_LOG`                    | Standard tracing filter override                 |
| `API_KEY`                     | Remote API auth (fallback if not in config JSON) |
| `HOME` / `LOCALAPPDATA`       | Data directory resolution                        |

---

## Common Commands

```bash
cargo check                          # Fast type-check
cargo build                          # Debug build
cargo build --release                # Release build
cargo test                           # Run tests (storage/localdb has unit tests)
cargo clippy -- -D warnings          # Lint
cargo fmt -- --check                 # Format check
./lint.sh                            # All three above
nix develop --command ci-checks
nix develop --command ci-test-build
nix develop --command ci-local
nix develop -c cargo test --features x11
nix build .#linux
nix build .#windows

# Run with debug output
cargo run -- --debug --interval 5

# Cross-compile for Windows (inside nix devShell or with mingw toolchain)
cargo build --target x86_64-pc-windows-gnu
```
