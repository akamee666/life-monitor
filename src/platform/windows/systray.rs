use std::{process::Command, sync::mpsc, thread};
use tracing::debug;
use tray_item::{IconSource, TrayItem};

enum Message {
    Quit,
    GoTo,
}

pub async fn init() {
    debug!("Spawned systray thread");
    let mut tray = TrayItem::new("akame.666 spy", IconSource::Resource("makima_icon")).unwrap();

    let (tx, rx) = mpsc::sync_channel(1);

    let twitter_tx = tx.clone();
    tray.add_menu_item("Project Source", move || {
        twitter_tx.send(Message::GoTo).unwrap();
    })
    .unwrap();

    tray.inner_mut().add_separator().unwrap();
    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        quit_tx.send(Message::Quit).unwrap();
    })
    .unwrap();

    loop {
        match rx.recv() {
            Ok(Message::Quit) => {
                std::process::exit(0);
            }
            Ok(Message::GoTo) => {
                let _child = Command::new("cmd.exe")
                    .arg("/C")
                    .arg("start")
                    .arg("")
                    .arg("http://www.github.com/akamee666/life-monitor")
                    .spawn()
                    .expect("failed to launch browser");
                thread::sleep(tokio::time::Duration::new(10, 0)); // Windows needs time!
            }
            _ => {}
        }
    }
}
