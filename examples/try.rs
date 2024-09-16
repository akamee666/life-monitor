use std::time::Duration;
use tokio::{
    select,
    task::spawn,
    time::{interval, sleep},
};

#[tokio::main]
async fn main() {
    let tick_1s = spawn(ticker(1));
    let tick_3s = spawn(ticker(3));

    let timeout = sleep(Duration::from_secs(10));
    select! {
        _ = tick_1s => {}
        _ = tick_3s => {}
        _ = timeout => {}
    };
}

async fn ticker(secs: u64) {
    let mut interval = interval(Duration::from_secs(secs));
    interval.tick().await; // skip first tick

    loop {
        interval.tick().await;
        println!("Waited {}s!", secs);
    }
}
