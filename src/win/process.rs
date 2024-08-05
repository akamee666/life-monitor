use crate::win::util::*;
use std::time::Duration;
use sysinfo::*;
use tokio::time;

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 20;

pub async fn track_processes() {
    let mut interval = time::interval(Duration::from_secs(1));

    let mut i = 0;
    let mut idle = false;

    loop {
        i = i + 1;

        // wait one second each time through
        interval.tick().await;

        // every five seconds we check if the last input time is greater than 30 seconds, if it's
        // we pause tracking cause user is probably idle.
        if i == IDLE_CHECK_SECS {
            let duration = get_last_input_time().as_secs();
            if IDLE_PERIOD > 0 && duration > IDLE_PERIOD {
                idle = true;
            } else {
                idle = false;
            }
            i = 0;
        }

        if !idle {
            get_process().await;
        }
    }
}

async fn get_process() {
    let sys = sysinfo::System::new_all();

    let (window_pid, window_title) = get_active_window();

    if window_pid == 0 {
        return;
    }

    let process = sys.processes().get(&Pid::from_u32(window_pid));

    if let Some(process) = process {
        // here comes store timestamp
        println!(
            "Active window[{}] title: {}",
            window_pid,
            window_title.as_str(),
        );
    }
}
