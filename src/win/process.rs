use crate::win::util::*;
use std::time::Duration;
use sysinfo::*;
use tokio::time;

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 10;

pub async fn track_processes() {
    println!("tracking");
    let mut interval = time::interval(Duration::from_secs(5));

    let mut i = 0;
    let mut idle = false;

    loop {
        i = i + 1;
        // if i understand correctly im waiting 1 sec before each time i go trought it.
        interval.tick().await;

        if i == IDLE_CHECK_SECS {
            // we check that the last time the user made any input
            // was shorter ago than our idle(5 seconds) period.
            // if it wasn't, we pause tracking
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

    if let Some(_process) = process {
        println!(
            "Active window[{}] title: {}",
            window_pid,
            window_title.as_str()
        );
    }
}
