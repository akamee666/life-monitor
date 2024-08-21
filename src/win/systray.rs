use log::debug;
use std::sync::mpsc;
use tray_item::{IconSource, TrayItem};
enum Message {
    Quit,
    GoTo,
}

pub async fn init() {
    debug!("Spawned systray thread");
    let mut tray = TrayItem::new("akame.spy", IconSource::Resource("makima_icon")).unwrap();

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
                // TODO: Check error and log if it failed for some reason.
                let _ = webbrowser::open("https://github.com/akame0x01/life-monitor");
            }
            _ => {}
        }
    }
}
