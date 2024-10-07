# Life Monitor in Rust

The main goal of this project is to create a mini spyware on my own to monitor what i'm doing daily and create some graphs about it to use in a personal blog. This idea came from this post i found interesting in twitter [vin_acct twitter post](https://x.com/vin_acct/status/1807973375014506597)


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

The program is almost finish, it has some bugs and it's not exactly what i want yet but it's usable i guess, i'll continue working on it and adding features of course. Go ahead if you wanna try it, the worse that can happen is incorrect data being send to database or the program crashes. Also, if you think that you've found a bug, i would be happy if you report it to me so i can fix it soon as possible. If you want some kinda of feature, you can fork and open a PR and i will accept it as soon as possible or just clone and do whatever you want, if you want some kinda of feature but don't want to code it, contact me or open a issue and i'll try to add it as soon as possible.

### What life-monitor do

If you followed the [building section](#building) that should start the life-monitor and close the current CMD, life-monitor will starting tracking your activities and send it to a db file at `%LOCALAPPDATA%\akame_monitor\forgotthename.db`, after it's all on you to use the data collected by the life-monitor whatever way you want to. You can stop its process by using the system tray item that should be spawned in taskbar when you start life-monitor. Life-monitor do not start with your system, you need to run it from cmd everytime you boot(i'll add a option to active this soon). If you have the feeling that the data isn't accurate(which i am almost sure it wouldn't be for mouse distance, i'll try to fix that as well), have weird names or whatever kind of weird behavior, please open a issue or contact me somewhere and i'll try to fix it as soon as possible. AV's can flag life-monitor as malware(which is reasonable) due to its functionalities, but life-monitor will NOT steal or send your data to other places, you can read the code and confirm it yourself or debug it(which i do not recommend, see this [issue](https://github.com/Narsil/rdev/issues/128)). if you are struggling to understand, contact me somewhere and will do my best to explain to you.

