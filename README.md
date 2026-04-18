# Life Monitor

`life-monitor` is a cross-platform Rust program for tracking computer activity on Linux and Windows. It counts keyboard and mouse usage, keeps track of which window is focused over time, and saves everything in a SQLite database. It was originally built for personal productivity analysis and blog posts, but the data can also be used for your own scripts, reports, or dashboards.

At the moment, the Linux desktop experience is the most complete part of the project, including Wayland support and optional X11 window tracking.

## What it tracks

- Keyboard key presses
- Mouse clicks
- Mouse travel distance in pixels and estimated centimeters
- Vertical and horizontal scroll input
- The name of the focused window and the app it belongs to over time

The program does not have a built-in dashboard yet. Instead, it writes the collected information to a SQLite database and a log file. You can inspect the database with tools such as [DB Browser for SQLite](https://sqlitebrowser.org/).

## Current status

The project is usable, but it is still being improved. Saving data locally in SQLite is the most stable and complete workflow right now. Wayland support, remote API storage, and Linux autostart through `systemd --user` are all available, but there are still rough edges, and the remote feature should still be treated as beta.

## Installing

You can install `life-monitor` with Cargo:

```bash
cargo install life-monitor
life-monitor
```

The binary is typically installed to:

```bash
~/.cargo/bin/life-monitor
```

If `life-monitor` is not found after installation, make sure Cargo's bin directory is in your `PATH`.

For Fish:

```bash
fish_add_path ~/.cargo/bin
```

## Building from source

### 1. Install Rust

Install Rust with [rustup](https://rustup.rs/) and make sure you have a recent stable toolchain:

```bash
rustup install stable
rustup default stable
```

### 2. Install system dependencies

To build on Linux, you need SQLite, OpenSSL, `pkg-config`, a C compiler toolchain, and `libclang`. `libclang` is needed because this project generates Rust bindings for Linux input headers during the build.

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

### 3. Clone and build

```bash
git clone https://github.com/akamee666/life-monitor.git
cd life-monitor
cargo build --release
```

The compiled binary will be available at:

```bash
./target/release/life-monitor
```

For a quick local run:

```bash
cargo run -- --debug
```

## Linux permissions

On Linux, `life-monitor` reads keyboard and mouse events directly from `/dev/input`. Those device files are usually protected, so your user account normally needs to be in the `input` group.

Without that permission, the program may start but still fail to read your input devices.

Add your user to the group:

```bash
sudo usermod -aG input $USER
```

Then log out and log back in so the new group membership is applied. After that, verify it with:

```bash
groups
```

You should see `input` in the output.

## Building with Nix

This repository includes a `flake.nix` file that sets up a ready-to-use development environment for Linux and also supports Windows cross-compilation.

Enter the development shell:

```bash
nix develop
```

Build the Linux package:

```bash
nix build .#linux
```

Cross-build the Windows package:

```bash
nix build .#windows
```

The Nix development shell already provides the Rust toolchain, `libclang`, and the extra environment variables needed for bindgen to work correctly.

## Windows cross-compilation without Nix

If you are not using Nix, you will need to add the Windows target and install a MinGW-based toolchain yourself:

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
```

## Usage

Basic usage:

```bash
life-monitor [OPTIONS]
```

### Common examples

Run with debug logging and a short update interval:

```bash
life-monitor --debug --interval 5
```

Use 1600 DPI when estimating mouse distance:

```bash
life-monitor --dpi 1600
```

Store keyboard and mouse totals in one-hour time slots:

```bash
life-monitor --gran 3
```

Start with a new, empty database:

```bash
life-monitor --clear
```

Enable startup for the current user session:

```bash
life-monitor --enable-startup
```

### CLI options

| Flag                      | Description                                                                                            |
| ------------------------- | ------------------------------------------------------------------------------------------------------ |
| `-i`, `--interval <SECS>` | How often the program writes its in-memory data to storage, in seconds. Default is `300`. Debug mode uses `5` if you do not set an interval yourself. |
| `-g`, `--gran <0-5>`      | How finely keyboard and mouse totals are split across the day. Higher values create smaller time blocks. |
| `-d`, `--debug`           | Turn on more detailed logs and use a shorter default write interval. |
| `-p`, `--dpi <DPI>`       | Mouse DPI used when estimating cursor distance in centimeters. Default is `800`. |
| `-c`, `--clear`           | Delete the current database and start again with an empty one. |
| `-r`, `--remote <FILE>`   | Send data to a remote HTTP service using a JSON config file. Requires the `remote` feature. |
| `--enable-startup`        | Set the program to start automatically for the current user session. |
| `--disable-startup`       | Remove the automatic startup configuration for the current user session. |
| `-s`, `--no-systray`      | Windows only: do not show the tray icon. |

### Granularity levels

The `--gran` flag controls how keyboard and mouse totals are split across the day:

- `0`: one row for the entire day
- `1`: 4-hour buckets
- `2`: 2-hour buckets
- `3`: 1-hour buckets
- `4`: 30-minute buckets
- `5`: 15-minute buckets

Higher values give you a more detailed picture of when activity happened, but they also create more rows in the database each day.

## Remote backend

Remote storage is optional. It is enabled by the `remote` Cargo feature.

Build with remote support:

```bash
cargo build --features remote
```

Or, if `remote` is already in the default feature set for your local branch, simply run:

```bash
cargo build
```

Then start the program with a JSON config file:

```bash
life-monitor --remote api-examples/example_config.json
```

Example config:

```json
{
  "base_url": "https://api.example.com",
  "keys_endpoint": "/v1/keys",
  "proc_endpoint": "/v1/proc"
}
```

If `api_key` is not included in the file, the program will try to read it from the `API_KEY` environment variable instead.

What the remote mode expects:

- It uses `GET` and `POST` requests
- Keyboard and mouse data is sent to `keys_endpoint`
- Window/process data is sent to `proc_endpoint`
- Responses are expected to be JSON

This part of the project is still being refined. For now, it is best to treat it as beta until the API shape and documentation settle down.

## Data locations

### Linux

- Database: `~/.local/share/life_monitor/data.db`
- Log file: `~/.local/share/life_monitor/spy.log`

### Windows

- Database: `%LOCALAPPDATA%\life_monitor\data.db`
- Log file: `%LOCALAPPDATA%\life_monitor\spy.log`

## Desktop session support

On Linux, the program chooses how to track the active window based on the graphical session it is running inside:

- Wayland if the process sees Wayland session indicators
- X11 if the process is running in an X11 session

Many Wayland desktops also provide a `DISPLAY` variable through Xwayland, so the detection logic is written to prefer Wayland whenever clear Wayland session variables are present.

## Autostart

On Linux, `--enable-startup` creates a `systemd --user` service for the current user. It is meant for starting with your desktop session, not for running as a system-wide service in the background.

Because active-window tracking depends on the graphical session, the safest way to enable startup is to run:

```bash
life-monitor --enable-startup
```

from the same graphical session where you normally use the program.

To disable it later:

```bash
life-monitor --disable-startup
```

## Notes and limitations

- Mouse distance is still an estimate based on DPI and raw pointer movement.
- Some Windows-specific parts of the project are still less polished than the Linux side.
- Security software may flag the program because it reads input and active-window information. The project does not try to hide this behavior, and the source code is available for inspection.

## Contributing

Issues and pull requests are welcome. If you report a bug, especially one related to platform-specific behavior, it helps a lot if you include:

- your operating system
- desktop session type (`Wayland` or `X11`)
- how you launched the program
- relevant log output

That information is especially useful when the problem is related to Linux permissions, session detection, or startup behavior.
