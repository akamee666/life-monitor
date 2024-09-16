# Life Monitor in Rust

The main goal of this project is to create an mini spyware on my own to monitor what i'm doing daily and create some graphs about it to use in an personal blog. This idea comes from this post that i found interesting in twitter [vin_acct twitter post](https://x.com/vin_acct/status/1807973375014506597)

looks like i've finished it, didn't test everything though. Go ahead if you wanna try it, the worse that can happen is incorrect data being sent to database or the program crashes. Also, if you think that you've found a bug, i would be happy if you report it to me so i can fix it soon as possible.

### Building

```bash
git clone https://github.com/akame0x01/life-monitor.git && cd life-monitor
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
./life-agent-for-windows.exe
```
