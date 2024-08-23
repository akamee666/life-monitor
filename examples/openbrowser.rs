use std::process::Command;
use std::thread;
use std::time;

fn main() {
    println!("Start");
    let _child = Command::new("cmd.exe")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg("http://www.rust-lang.org")
        .spawn()
        .expect("failed to launch browser");
    thread::sleep(time::Duration::new(10, 0)); // Windows needs time!
    println!("End");
}
