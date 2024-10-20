# Life Monitor in Rust

The main goal of this project is to create a mini spyware on my own to monitor what I'm doing daily and create some graphs about it to use in a personal blog. This idea comes from this post that I found interesting on Twitter: [vin_acct Twitter post](https://x.com/vin_acct/status/1807973375014506597)

## Building from source

- Install [rustup](https://rustup.rs/) and [cargo](https://github.com/rust-lang/cargo/)
- Install and configure the default toolchain with `rustup install stable` and `rustup default stable`
- Install the equivalent of the `libssl-dev` package using your package manager
- Clone this repository and enter it
- Run `cargo build` or `cargo run`

<a id="compiling-windows"></a>
Note: To cross compile, you may need to install additional packages. cross-compile with `cargo build --target x86_64-pc-windows-gnu` (assuming you've already added the `windows` toolchain via `rustup target add x86_64-pc-windows-gnu`).

## About the project

### Contribute

The program is almost finished, no entirely cause i didn't test it enough. It probably  has bugs, but it's usable, I guess. I'll continue working on it and adding features, of course. Go ahead if you want to try it; the worst that can happen is incorrect data being sent to the database or the program crashes. Also, if you think you've found a bug, I would be happy if you report it to me so I can fix it as soon as possible. If you want some kind of feature, you can fork and open a PR, and I will accept it as soon as possible, or just clone and do whatever you want. One people ask me if it was okay to clone the repo to learning purpose, so I did add some comments to help. If you want some kind of feature but don't want to code it, contact me or open an issue, and I'll try to add it as soon as possible.

### What life-monitor does

If you followed the [building section](#building), that should start the life-monitor and close the current CMD. Life-monitor will start tracking your activities and send them to a db file at %LOCALAPPDATA%\akame_monitor\forgotthename.db. After that, it's all up to you to use the data collected by the life-monitor however you want. You can stop its process by using the system tray item that should be spawned in the taskbar when you start life-monitor. Life-monitor does not start with your system; you need to run it from CMD every time you boot (I'll add an option to activate this soon). If you have the feeling that the data isn't accurate (which I am almost sure it wouldn't be for mouse distance, I'll try to fix that as well), has weird names, or whatever kind of weird behavior, please open an issue or contact me somewhere, and I'll try to fix it as soon as possible. AVs can flag life-monitor as malware (which is reasonable) due to its functionalities, but life-monitor will NOT steal or send your data to other places. You can read the code and confirm it yourself or debug it (which I do not recommend, see this issue). If you are struggling to understand, contact me somewhere, and I will do my best to explain it to you.


## TODO

[ ] - Error handling using the api.
[ ] - Organize project tree.
[ ] - Create Github actions for releases.
