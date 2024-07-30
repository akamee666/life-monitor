# Life Monitor in Rust

The main goal of this project is to create an mini spyware on my own to monitor what i'm doing daily and create some graphs about it to use in an personal blog. This idea comes from this post that i found interesting in twitter [vin_acct twitter post](https://x.com/vin_acct/status/1807973375014506597)

As i'm using libraries like sysinfo and rdev, cross compile it using rustup with a target and you should be able to use it in any platform that these libraries cover(all of them probably).

### Building

# Windows

```bash
git clone https://github.com/akame0x01/life-monitor.git && cd life-monitor
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu
```

# Linux 

```bash
git clone https://github.com/akame0x01/life-monitor.git && cd life-monitor
cargo run 
```
