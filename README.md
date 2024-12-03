# Life Monitor in Rust

The main goal of this project is to create a mini spyware on my own to monitor what I'm doing daily and create some graphs about it to use in a personal blog. This idea comes from this post that I found interesting on Twitter: [Vin Twitter post](https://x.com/vin_acct/status/1807973375014506597)

### Installing

You can install life-monitor easily by using cargo install or using github releases,although i am not sure how up to date it is compared to current commits so i recommend you to build from source :3

1. Installing and running.

```bash
cargo install life-monitor
life-monitor
```

Binary will be available at `~/.cargo/bin/life-monitor` 

If you tried to run and received a error about unknown command, mostly likely you don't have Cargo variable in your path, you can add running the following command.

1. Fish Shell

```bash
fish_add_path /home/your-username/.cargo/bin/
```

### Building from source

- Install [rustup](https://rustup.rs/) and [cargo](https://github.com/rust-lang/cargo/)
- Install and configure the default toolchain with `rustup install stable` and `rustup default stable`
- Install the equivalent of the `libssl-dev` package using your package manager
- Clone this repository and enter it
- Run `cargo build --release` or `cargo run`

The compiled binary will be available at `./target/release/life-monitor`

<a id="compiling-windows"></a>
Note: To cross compile, you may need to install additional packages. cross-compile with `cargo build --target x86_64-pc-windows-gnu` (assuming you've already added the `windows` toolchain via `rustup target add x86_64-pc-windows-gnu`).

### Usage
Usage: life-monitor [OPTIONS]

| Flag | Long Form | Description |
| --- | --- | --- |
| `-t` | `--interval` < secs > | Set interval for data sending (secs) |
| `-k` | `--no-keys `| Disable key/mouse tracking |
| `-w` | `--no-window`  | Disable window-based tracking |
| `-d` | `--debug` | Enable debug mode |
| `-p` | `--dpi` < dpi > | Specify mouse DPI for tracking |
| `-c` | `--clear`  | Clear existing data, start new |
| `-g` | `--gran` <0-6>| Divide the entries for keys based on hour. |
| `-r` | `--remote` < file > | Send collected data through remote defined by json file. |
| `-h` | `--help` | Show help information |

More detailed descriptions can be found running with --help flag.

For the API and the `-g` flag check the section below.

`--gran`:

This flag helps decide how detailed the data tracking will be for your activity, like keyboard and mouse use, across different times of the day. You can think of it as setting how “zoomed in” you want the time tracking to be:

    Level 5: Breaks down your activity into 15-minute intervals.
    Level 4: Shows activity in 30-minute intervals.
    Level 3: Tracks in 1-hour intervals.
    Level 2: Groups activity into 2-hour intervals.
    Level 1: Summarizes in 4-hour intervals.
    Level 0: A single summary of your activity, with no breakdown by time.

This way, you can choose how detailed or summarized you want the information to be!

### Contribute

The main functionalities of this program is finish, not entirely because i keep finding new things to add to it though. It probably has bugs :), but it's usable, I guess. I'll continue working on it and adding features if requested, of course. Go ahead if you want to try it, the worst that can happen is incorrect data being sent to the database or the program crashing. Also, if you think you've found a bug, I would be happy if you report it to me so I can fix it as soon as possible. If you want some kind of feature, you can fork and open a PR, and I will accept it as soon as possible, or just clone and do whatever you want. One people ask me if it was okay to clone the repo to learning purpose, so I did add some comments to help. If you want some kind of feature but don't want to code it, contact me or open an issue, and I'll try to add it as soon as possible.

### What life-monitor does

Life-monitor will start listening for inputs and active windows, it will save how many keys and buttons you press in your keyboard and mouse, it also tries to measure how much you are moving your mouse on your table and convert it to centimeters.For the active window, it saves its `name` and `class` using `X11` or `WINAPI` and keep increasing a timer while the window is active. If you did not use the `--api` flag, life-monitor will save all data it has collected to different paths depending if you are on Windows or Linux using `SQLITE`. A log file will also be found at these paths with the name `spy.log`.

For Windows it will be saved at: `%LOCALAPPDATA%\akame_monitor\data.db`

For Linux it will be saved at: `/home/user/.local/share/akame_monitor/`

You can visualize the collected data using [SQLite Browser](https://sqlitebrowser.org/).

After that, it's all up to you to use the data collected however you want. You can stop its process by using the system tray if you are on Windows or using kill command if you are on Linux. If you have the feeling that the data isn't accurate, which it probably will be for mouse distance, please open an issue or contact me somewhere, and I'll try to fix it as soon as possible. Your AV will most likely flag it as a spyware(which is reasonable) due to its functionalities, but will NOT steal or send your data to somewhere else. You can read the code and confirm it yourself or debug it, but I do not recommend trying to debug it though, you can check this [issue](https://github.com/Narsil/rdev/issues/128) from the library that it's used to listen to inputs, most likely the "problem" is with the `SetWindowsHookEx` as you can see [here](https://developercommunity.visualstudio.com/t/debugging-with-keyboard-very-slow/42018).

If you are struggling to understand the code, contact me somewhere, and I will do my best to explain it to you.

### To do

- [x]  Auto start argument.
- [x]  Check and print to Stdout if startup is enabled.
- [x]  Create option to save input based on time.
  - [x]  Create flag and descriptions.
  - [x]  Change how database is structured based on the level.
- [x] Space one or two seconds the interval for updates in one of the two tasks.
- [x] I have publish a broken version to both cargo install and github, need to fix that. Omg i am just so dumb, keys table does not have the row when created using default i guess.
- [x] Change API flag to remote instead, bc make more sense.
- [x]  Error handling when using remote instead of just panic.
- [x] I am dumb, startup on Linux is failing and i need to fix.
- [x] Organize project tree.
- [ ] Check weird shit on startup and i totally forgot about windows lol.
- [ ] Doc to remote flag.
- [ ]  Check CPU load with the new features. Now that i have more data in both table to go through it may impact the performance a little bit. 

- [ ] Wayland support? I saw somewhere that Wayland has some kinda of support on X11 APIs so maybe it is not necessary.

- [ ] Maybe create a cool tui to display the collected data in a cool way to the user i guess.
  - [ ] Percentages of the most used apps would be cool.
