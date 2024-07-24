use std::time::Duration;
use tokio::time;

use super::util::print_active_window;

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 10;

pub async fn track_processes() {
    let mut interval = time::interval(Duration::from_secs(5));

    let mut i = 0;
    let mut idle = false;

    loop {
        i = i + 1;

        interval.tick().await;

        if !idle {
            print_active_window().await;
        }
    }
}
