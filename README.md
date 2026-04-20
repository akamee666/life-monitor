# Life Monitor

`life-monitor` is a cross-platform Rust activity tracker for Linux and Windows. It collects raw keyboard and mouse activity, tracks which window is focused over time, and stores the data in SQLite for later analysis.

- activity is written to a local SQLite database
- you can move or merge history between machines with snapshot export/import
- you can place the database on another disk or an already-mounted network share

There is no built-in dashboard _yet_. The output is a SQLite database plus a log file, so the current workflow is to inspect the data with SQL, scripts, or external tools.

## Current Status

The main workflow is stable enough for local use:

- local SQLite storage is the supported default.
- import/export snapshots are the simplest built-in way to move history between machines
- remote samba/NFS shares are supported when already mounted by the OS
- optional multi-device sync is available behind the `multi-sync` cargo feature

Still missing:

- built-in charts or TUI
- deeper reporting and visualization on top of the stored data

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

If you are building or testing from source, prefer the flake environment so the expected toolchain and native dependencies are available:

```bash
nix develop --command cargo test --target x86_64-unknown-linux-gnu
nix develop --command cargo check --target x86_64-pc-windows-gnu
```

> [!WARNING]
> On Linux, `life-monitor` reads raw input events from `/dev/input`. Your user usually needs permission to access those devices, add yourself to input group using `sudo usermod -aG input $USER` or run the program as `root`

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

Optional multi-device sync examples, when built with `--features multi-sync`:

```bash
life-monitor --sync-enable --sync-remote-url http://homeserver:8080
life-monitor --sync-remote-url http://homeserver:8080 sync push
life-monitor --sync-remote-url http://homeserver:8080 sync pull
life-monitor --sync-remote-url http://homeserver:8080 sync status
```

## Main CLI Options

| Flag                      | Purpose                                                   |
| ------------------------- | --------------------------------------------------------- |
| `-i`, `--interval <SECS>` | Flush buffered activity to SQLite every N seconds         |
| `-d`, `--debug`           | Enable more verbose logs and a shorter default interval   |
| `-p`, `--dpi <DPI>`       | Set mouse DPI/CPI for distance estimation and remember it |
| `-c`, `--clear`           | Delete the current database and start from an empty one   |
| `--db-path <PATH>`        | Use a custom SQLite path or directory and remember it     |
| `--export-db <FILE>`      | Export the current DB into a consistent SQLite snapshot   |
| `--import-db <FILE>`      | Import a previously exported snapshot                     |
| `--dry-run`               | Preview the import without modifying the destination DB   |
| `--import-notes <TEXT>`   | Record notes alongside import metadata                    |
| `--enable-startup`        | Enable automatic startup for the current user session     |
| `--disable-startup`       | Disable automatic startup for the current user session    |
| `--startup-mode <MODE>`   | Linux only: choose `xdg` or `systemd` startup mode        |
| `-s`, `--no-systray`      | Windows only: disable the tray icon                       |

When built with `--features multi-sync`, additional sync options and subcommands are available:

| Flag / Command | Purpose |
| -------------- | ------- |
| `--sync-enable` | Enable background push/pull sync during normal collection |
| `--sync-remote-url <URL>` | Point the program at a remote `sqld` / libSQL endpoint |
| `--sync-auth-token <TOKEN>` | Provide an auth token for the remote endpoint |
| `--sync-interval <SECS>` | Change how often background sync runs |
| `sync push` | Send pending local device-owned rows to the remote |
| `sync pull` | Pull remote changes into the local SQLite database |
| `sync status` | Show the local sync state, pending work, and last known remote status |

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

SQLite note:

- Life Monitor bundles SQLite on both Linux and Windows
- you do not need a separate system SQLite installation just to run the program

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

## Optional Multi-Device Sync

`life-monitor` now has an optional sync mode for people who want multiple devices to converge into the same dataset without manually exporting and importing snapshots all the time.

This mode is not enabled in the default build. Build from source with:

```bash
cargo build --release --features multi-sync
```

User-facing model:

- each device keeps collecting into its own local SQLite database
- the local database remains usable even if the remote is down
- sync push sends only rows authored by that device
- sync pull brings down rows from other devices
- `sync status` shows pending changes, last successful sync times, and the last known remote state

What you need:

- a reachable `sqld` / libSQL server
- the remote URL
- an auth token if your deployment requires one

Typical setup:

1. build `life-monitor` with `--features multi-sync`
2. run your `sqld` server somewhere you control
3. start `life-monitor` with `--sync-enable --sync-remote-url <URL>`
4. use `sync status` to confirm the local database is catching up

Important behavior:

- if the remote is unavailable, `life-monitor` keeps collecting locally
- pending sync work stays queued and is retried later
- local-only mode remains the default if you do not enable sync

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

## Startup

### Linux

Linux has two startup modes:

- `xdg` is the default and recommended mode
- `systemd` is an explicit advanced fallback

Use it from the graphical session where you normally run the program.

Recommended default:

```bash
life-monitor --enable-startup --startup-mode xdg
```

Fallback mode:

```bash
life-monitor --enable-startup --startup-mode systemd
```

What each mode does:

- `xdg` writes `life-monitor.desktop` into `~/.config/autostart` or `$XDG_CONFIG_HOME/autostart`
- `systemd` writes and enables a per-user `systemd --user` unit tied to `graphical-session.target`
- the `systemd` mode does not freeze volatile values such as `WAYLAND_DISPLAY` into the unit file; it expects the graphical session to import them into `systemd --user` at login

Best practice:

- enable startup from a stable installed binary such as `cargo install life-monitor`
- avoid enabling startup from `target/debug` or `target/release` inside a repo checkout, because moving or cleaning the repository will break the stored executable path
- prefer `xdg` unless you have already verified that your session imports graphical variables into `systemd --user`

How to check whether XDG autostart is likely a good fit before enabling it:

```bash
printf 'XDG_CURRENT_DESKTOP=%s\nDESKTOP_SESSION=%s\nXDG_SESSION_TYPE=%s\n' \
  "$XDG_CURRENT_DESKTOP" "$DESKTOP_SESSION" "$XDG_SESSION_TYPE"
echo "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
```

Good signs:

- you are running inside a normal graphical session
- at least one of `XDG_CURRENT_DESKTOP` or `DESKTOP_SESSION` is set
- `XDG_SESSION_TYPE` is set to `wayland` or `x11`

If those values are missing, XDG autostart may still work, but your setup is probably more manual and you should verify it after logging in again.

How to check whether the `systemd` fallback is likely to inherit the graphical environment correctly:

```bash
env | rg '^(DISPLAY|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|XAUTHORITY|XDG_SESSION_TYPE)='
systemctl --user show-environment | rg '^(DISPLAY|WAYLAND_DISPLAY|XDG_RUNTIME_DIR|XAUTHORITY|XDG_SESSION_TYPE)='
systemctl --user status graphical-session.target
```

Good signs:

- the same graphical-session variables appear in both `env` and `systemctl --user show-environment`
- `graphical-session.target` exists and is part of the user session lifecycle

If the process environment has those variables but `systemctl --user show-environment` does not, prefer XDG startup. That means your session is not currently exporting the graphical environment into `systemd --user`.

After `--enable-startup`, Life Monitor does not try to launch a second copy immediately on Linux:

- XDG mode takes effect on the next graphical login
- systemd mode installs and enables the user unit, but does not start it right away
- this avoids racing the current process against Life Monitor's single-instance lock

If you want to test the systemd fallback immediately, first stop the current Life Monitor process and then run:

```bash
systemctl --user start life-monitor.service
```

What `life-monitor` itself does not do:

- it does not try to detect every compositor or desktop environment individually
- it does not rewrite the wider `systemd --user` manager environment for you
- it does not mount shares or recreate the whole desktop environment

Disable it with:

```bash
life-monitor --disable-startup
```

### Windows

`--enable-startup` creates a shortcut in the current user's Startup folder.

Disable it with:

```bash
life-monitor --disable-startup
```

## Notes and Limitations

- there is still no built-in dashboard or TUI
- mouse distance is still an estimate, not a physical measurement guarantee
- remote-share locking is best-effort and depends on filesystem/share semantics
- local-only mode is the supported default; multi-device sync is optional and requires a remote `sqld` / libSQL server
- Wine can help with some Windows checks on Linux, but native Windows remains the reliable runtime validation environment

## Contributing

Bug reports and PRs are welcome. Feel free to reach out to me if you have any questions :).

If you report a platform-specific issue, include:

- operating system
- desktop session type (`Wayland` or `X11`) on Linux
- how you launched the program
- relevant logs
