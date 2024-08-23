# Life Monitor in Rust

The main goal of this project is to create an mini spyware on my own to monitor what i'm doing daily and create some graphs about it to use in an personal blog. This idea comes from this post that i found interesting in twitter [vin_acct twitter post](https://x.com/vin_acct/status/1807973375014506597)

I did not finish this project yet but i'm close to, i still need to handle some logic that is missing when i send data to db, get some better logging maybe, check for bugs and performance issues cause i do not want that my cpu goes crazy when i'm playing something cause of my bad code.

### Building

# Windows

```bash
git clone https://github.com/akame0x01/life-monitor.git && cd life-monitor
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
./life-agent-for-windows.exe
```
