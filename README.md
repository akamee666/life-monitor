# Life Monitor

`life-monitor` is a cross-platform Rust activity tracker for Linux and Windows. It collects raw keyboard and mouse activity, tracks which window is focused over time, and stores the data in SQLite for later analysis.

The project is now local-first:
- activity is written to a local SQLite database
- you can move or merge history between machines with snapshot export/import
- you can place the database on another disk or an already-mounted network share

There is no built-in dashboard yet. The output is a SQLite database plus a log file, so the current workflow is to inspect the data with SQL, scripts, or external tools.

## What it tracks

- keyboard key presses
- left, right, and middle mouse clicks
- estimated mouse travel distance in centimeters
- vertical and horizontal scroll activity
- focused window title, app identifier, and focus time

## Current Status

The main workflow is stable enough for local use:
- local SQLite storage is the supported default
- import/export snapshots are the intended cross-machine sync mechanism
- custom database paths are supported
- remembered DB paths and remembered DPI reduce repetitive setup

Still missing:
- built-in charts or TUI
- Windows startup implementation
- release binaries attached to GitHub releases

## Install

### From crates.io

```bash
cargo install life-monitor
```

Then run:

```bash
life-monitor
```

### From source

```bash
git clone https://github.com/akamee666/life-monitor.git
cd life-monitor
cargo build --release
```

Binary location:

```bash
./target/release/life-monitor
```

## Build Requirements

### Linux

You need:
- a recent Rust toolchain
- `clang`
- `libclang`
- SQLite development libraries
- OpenSSL development libraries
- `pkg-config`
- Wayland/X11 development libraries

Examples:

#### Arch Linux

```bash
sudo pacman -S --needed base-devel clang sqlite openssl pkgconf wayland libx11 libxi libxtst
```

#### Debian / Ubuntu

```bash
sudo apt update
sudo apt install -y build-essential clang libclang-dev pkg-config libsqlite3-dev libssl-dev libwayland-dev libx11-dev libxi-dev libxtst-dev
```

#### Fedora

```bash
sudo dnf install -y gcc gcc-c++ clang llvm-devel pkgconf-pkg-config sqlite-devel openssl-devel wayland-devel libX11-devel libXi-devel libXtst-devel
```

### Nix

This repository ships a `flake.nix` with a development shell and build targets:

```bash
nix develop
nix build .#linux
nix build .#windows
```

## Linux Permissions

On Linux, `life-monitor` reads raw input events from `/dev/input`. Your user usually needs permission to access those devices.

Typical setup:

```bash
sudo usermod -aG input $USER
```

Then log out and back in.

## Usage

Basic form:

```bash
life-monitor [OPTIONS]
```

Useful examples:

```bash
life-monitor
life-monitor --debug --interval 10
life-monitor --db-path /mnt/shared/life-monitor
life-monitor --export-db ./life-monitor-snapshot.sqlite
life-monitor --import-db ./life-monitor-snapshot.sqlite --dry-run
life-monitor --import-db ./life-monitor-snapshot.sqlite --import-notes "desktop sync"
```

## Main CLI Options

| Flag | Purpose |
| ---- | ------- |
| `-i`, `--interval <SECS>` | Flush buffered activity to SQLite every N seconds |
| `-d`, `--debug` | Enable more verbose logs and a shorter default interval |
| `-p`, `--dpi <DPI>` | Set mouse DPI/CPI for distance estimation and remember it |
| `-c`, `--clear` | Delete the current database and start from an empty one |
| `--db-path <PATH>` | Use a custom SQLite path or directory and remember it |
| `--export-db <FILE>` | Export the current DB into a consistent SQLite snapshot |
| `--import-db <FILE>` | Import a previously exported snapshot |
| `--dry-run` | Preview the import without modifying the destination DB |
| `--import-notes <TEXT>` | Record notes alongside import metadata |
| `--enable-startup` | Enable automatic startup for the current user session |
| `--disable-startup` | Disable automatic startup for the current user session |
| `-s`, `--no-systray` | Windows only: disable the tray icon |

Run `life-monitor --help` for the full generated help text.

## Database Paths and Mounted Shares

By default the database lives in:

### Linux

- database: `~/.local/share/life_monitor/data.db`
- log file: `~/.local/share/life_monitor/spy.log`

### Windows

- database: `%LOCALAPPDATA%\life_monitor\data.db`
- log file: `%LOCALAPPDATA%\life_monitor\spy.log`

You can override that with `--db-path`.

Supported `--db-path` forms:
- a direct SQLite file path
- a directory, in which case `life-monitor` uses `data.db` inside it
- a mounted network share path such as Samba or NFS

Behavior:
- the path is remembered for future runs
- supplying `--db-path` again overwrites the remembered path
- if a remembered share is unavailable later, the program errors with a recovery message telling you to remount the share or provide a new DB path

Important limitation:
- `life-monitor` does not mount remote shares and does not prompt for share credentials
- the OS must already have access to the path you provide

## Snapshot Export and Import

To move or merge activity history between machines, use snapshot export/import:

```bash
life-monitor --export-db ./life-monitor-snapshot.sqlite
life-monitor --import-db ./life-monitor-snapshot.sqlite --dry-run
life-monitor --import-db ./life-monitor-snapshot.sqlite
```

Export behavior:
- creates a consistent SQLite snapshot
- does not copy the raw DB file blindly

Import behavior:
- validates source and destination integrity
- creates an automatic pre-import backup
- merges bucketed activity data
- records metadata so the same snapshot is not imported twice accidentally

## Data Model Summary

The old cumulative tables have been replaced by bucket-based records.

Main tables:
- `sources`
- `input_buckets`
- `focus_buckets`
- `exports`
- `imports`
- `sessions`

This lets the project support:
- historical activity by bucket
- merge/import/export workflows
- per-source tracking
- future session-level reporting

## DPI and Mouse Distance

Mouse distance is estimated from raw input counts plus a configured DPI/CPI value.

Current behavior:
- if you provide `--dpi`, the value is remembered
- if you do not provide `--dpi`, `life-monitor` reuses the remembered value
- if no DPI is known yet, interactive runs ask once and persist it

Why this works this way:
- raw input avoids desktop pointer acceleration in the measurement path
- but raw counts still need DPI/CPI to estimate real-world distance in centimeters
- generic automatic CPI detection is not portable enough across Linux and Windows setups to be trusted as the main path

## Desktop Session Tracking

Linux:
- Wayland is preferred when Wayland session indicators are present
- X11 is used when running in an X11 session

Windows:
- input uses Raw Input
- focus tracking uses Win32 window APIs

## Startup

### Linux

`--enable-startup` creates a `systemd --user` service.

Use it from the graphical session where you normally run the program:

```bash
life-monitor --enable-startup
```

Disable it with:

```bash
life-monitor --disable-startup
```

### Windows

Windows startup wiring is not finished yet.

## CI and Releases

Current CI:
- push/PR validation on Linux
- push/PR validation on Windows
- tag-driven release workflow for crates.io publishing

Current release workflow:
- push a tag like `v0.1.6`
- validate that the tag matches `Cargo.toml`
- rerun checks
- publish to crates.io
- create a GitHub Release entry
- attach Linux and Windows release archives for download

## Changelog Strategy

This repository now includes a `CHANGELOG.md`. The recommended approach is:
- keep an `Unreleased` section during normal development
- move those entries into a versioned section when you tag a release
- group entries under:
  - `Added`
  - `Changed`
  - `Fixed`
  - `Removed`

If you want automatic generation later, `git-cliff` is a good fit for Rust projects because it can generate release notes from conventional-style commit messages and tags.

A basic `git-cliff` config is now included as [cliff.toml](/home/ak4m3/programming/life-monitor/cliff.toml). A typical manual flow looks like:

```bash
git cliff --unreleased --tag v0.1.6 > /tmp/CHANGELOG.new
git cliff --tag v0.1.6 > CHANGELOG.md
```

The exact command can be adjusted depending on whether you want to rewrite the full changelog or only preview the next release notes.

## Notes and Limitations

- there is still no built-in dashboard or TUI
- mouse distance is still an estimate, not a physical measurement guarantee
- remote-share locking is best-effort and depends on filesystem/share semantics
- some Windows-specific polish is still missing

## Contributing

Bug reports and PRs are welcome.

If you report a platform-specific issue, include:
- operating system
- desktop session type (`Wayland` or `X11`) on Linux
- how you launched the program
- relevant logs
