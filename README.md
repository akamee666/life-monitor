# Vigil

A local-first activity tracker for Linux and Windows. Vigil records keyboard activity, mouse movement and clicks, and focused-window history into a local SQLite database. Inspect your data through an interactive terminal dashboard.

## Features

- Tracks key presses, mouse clicks (left/right/middle), mouse movement, and scroll
- Tracks focused window and active application over time
- Stores all data locally in SQLite — no cloud required
- Interactive terminal dashboard with charts, app activity, and weekly heatmaps
- Snapshot export and import for moving data between machines
- Optional feature-gated multi-device sync (`--features multi-sync`)
- Linux: XDG autostart and systemd user service support
- Windows: Startup folder shortcut and system tray icon

---

## Installation

### From crates.io

```sh
cargo install vigil
```

### From source

```sh
git clone https://github.com/akamee666/vigil
cd vigil
cargo build --release
# Binary at: target/release/vigil
```

### Nix

```sh
nix build .#linux    # Linux binary
nix build .#windows  # Windows binary (cross-compiled from Linux)
```

---

## Quick Start

Start collecting activity:

```sh
vigil collector
```

Open the dashboard while collection is running (in a separate terminal):

```sh
vigil dashboard
```

---

## Commands

### `vigil collector`

Runs the background activity collector and handles all collector-related maintenance.

```
vigil collector [OPTIONS]
```

**Collection options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-i, --interval <SECS>` | 300 | How often buffered activity is flushed to SQLite |
| `-d, --debug` | off | Verbose logging; uses 5 s flush interval unless `--interval` is set |
| `-p, --dpi <DPI>` | remembered | Mouse DPI used for estimating physical movement in cm |

**Database options:**

| Flag | Description |
|------|-------------|
| `--db-path <PATH>` | Use a custom database file or directory path (remembered across runs) |
| `-c, --clear` | Delete the current database and start fresh |

**Import / Export:**

| Flag | Description |
|------|-------------|
| `--export-db <FILE>` | Export a consistent SQLite snapshot to `<FILE>` and exit |
| `--import-db <FILE>` | Import a previously exported snapshot and exit |
| `--dry-run` | Preview import changes without writing anything (requires `--import-db`) |
| `--import-notes <TEXT>` | Attach notes to the import record (requires `--import-db`) |

**Startup:**

| Flag | Description |
|------|-------------|
| `--enable-startup` | Configure Vigil to start automatically at login |
| `--disable-startup` | Remove the automatic startup entry |

**Windows only:**

| Flag | Description |
|------|-------------|
| `-s, --no-systray` | Disable the system tray icon |

---

### `vigil dashboard`

Opens the interactive read-only terminal dashboard. Does not start collection and does not acquire the collector lock.

```sh
vigil dashboard
```

The dashboard shows:

- **Summary cards** — totals for the selected time window (key presses, clicks, mouse movement, active time)
- **App activity panel** — top applications by focus time with per-app activity histograms
- **Activity chart** — time series for the selected metric and time window
- **Week activity grid** — daily breakdown of activity by metric across recent days

---

## Dashboard Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit (or close help modal) |
| `?` / `h` | Open / close help |
| `Tab` / `Shift-Tab` | Cycle focus between panels |
| `1` / `2` / `3` / `4` | Jump to: summary, apps, chart, weekly grid |
| `[` / `]` | Previous / next time window |
| `j` / `k` or `↑` / `↓` | Scroll / select rows in focused panel |
| `m` | Next chart metric |
| `v` | Toggle chart mode (single metric / scope overlay) |
| `r` / `F5` | Reload data from SQLite |
| `u` | Toggle Unicode / ASCII rendering |

**Time windows:** `All`, `1h`, `6h`, `24h`, `7d`, `30d`

**Chart metrics:** activity score, key presses, left clicks, right clicks, middle clicks, mouse movement

---

## Mouse DPI

Vigil uses your mouse DPI to convert raw input counts into estimated centimeters of physical movement. On the first run without a remembered value, Vigil will prompt you to enter it.

To set or update it:

```sh
vigil collector --dpi 800
```

Vigil remembers this value across runs. Start with `800` if you do not know your DPI.

---

## Custom Database Path

By default Vigil stores data at:

- **Linux:** `~/.local/share/vigil/data.db`
- **Windows:** `%LOCALAPPDATA%\vigil\data.db`

To use a different location:

```sh
vigil collector --db-path /mnt/nas/vigil/data.db
```

The path is remembered across runs. It can point to a file, a directory, or a mounted network share.

---

## Autostart Setup

### Enable

```sh
vigil collector --enable-startup
```

On **Linux**, an interactive picker lets you choose between:

- **XDG autostart** *(recommended)* — creates a `.desktop` entry in `~/.config/autostart/`. Works with GNOME, KDE Plasma, Xfce, Cinnamon, LXQt, MATE, Budgie, and most mainstream desktop environments.
- **systemd user service** *(advanced)* — installs a `vigil.service` unit under `~/.config/systemd/user/`. Appropriate for minimal or hand-configured sessions like i3, sway, Hyprland, bspwm, river, awesome, or dwm.

On **Windows**, a shortcut is created in `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`.

Both methods launch `vigil collector` at login.

### Disable

```sh
vigil collector --disable-startup
```

Removes the XDG autostart entry and/or systemd unit (Linux), or removes the Startup shortcut (Windows).

---

## Export and Import

### Export a snapshot

```sh
vigil collector --export-db ./snapshot.sqlite
```

Creates a consistent SQLite backup using SQLite backup primitives.

### Import a snapshot

```sh
# Preview changes before writing
vigil collector --import-db ./snapshot.sqlite --dry-run

# Apply the import
vigil collector --import-db ./snapshot.sqlite

# With notes
vigil collector --import-db ./snapshot.sqlite --import-notes "from laptop 2025-04"
```

Import is idempotent: re-importing the same snapshot does not double-count data. Each snapshot is identified by a unique export UUID.

---

## Multi-Device Sync (optional)

Multi-device sync via a remote `sqld`/libSQL endpoint is disabled by default and must be compiled in:

```sh
cargo build --features multi-sync
```

When compiled with `multi-sync`, a `sync` subcommand becomes available:

```sh
vigil sync push
vigil sync pull
vigil sync status
```

Sync also runs in the background during collection:

```sh
vigil collector --sync-enable --sync-remote-url <URL> --sync-auth-token <TOKEN>
```

Or via environment variables:

```sh
VIGIL_SYNC_REMOTE_URL=<URL> VIGIL_SYNC_AUTH_TOKEN=<TOKEN> vigil collector --sync-enable
```

Sync is always layered on top of local-first collection. The local SQLite database is the primary store. Sync failure never stops collection.

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `VIGIL_DATA_DIR` | Override the default data directory |
| `VIGIL_SKIP_INSTANCE_LOCK` | Set to `1` to skip the single-instance lock (testing only) |
| `VIGIL_SYNC_REMOTE_URL` | Remote sqld/libSQL endpoint for sync (`multi-sync` feature) |
| `VIGIL_SYNC_AUTH_TOKEN` | Auth token for the sync remote (`multi-sync` feature) |

---

## Data Model

All activity is stored as bucketed rows in local SQLite:

- `input_buckets` — keyboard, mouse clicks, movement, and scroll per time bucket
- `focus_buckets` — focused window and application per time bucket

Buckets are the source of truth for all dashboard views, analytics, and import/export operations.

---

## Building

### Linux

```sh
nix develop --command cargo build --target x86_64-unknown-linux-gnu
nix develop --command cargo test --target x86_64-unknown-linux-gnu
```

### Windows (cross-compile from Linux)

```sh
nix develop --command cargo build --target x86_64-pc-windows-gnu
nix develop --command cargo check --target x86_64-pc-windows-gnu
```

Vigil bundles SQLite on both platforms. No system `libsqlite3` dependency is required at runtime.

---

## License

MIT
