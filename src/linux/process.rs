use super::util::get_active_window;
use std::time::Duration;
use tokio::time;

const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 10;

pub async fn track_processes() {
    let mut interval = time::interval(Duration::from_secs(5));

    let mut i = 0;
    let mut idle = false;

    loop {
        i = i + 1;

        interval.tick().await;

        if i == IDLE_CHECK_SECS {
            let duration = user_idle::UserIdle::get_time().unwrap().as_seconds();
            println!("idle_time: {}", duration);
            if IDLE_PERIOD > 0 && duration > IDLE_PERIOD {
                idle = true;
            } else {
                idle = false;
            }
        }
        if !idle {
            get_process().await;
        }
    }
}

async fn get_process() {
    let sys = sysinfo::System::new_all();
    let window_class = get_active_window().unwrap_or("".to_string());
    println!("window_class: {}", window_class); // this is the name.

    if !window_class.is_empty() {
        let window_class = window_class.to_lowercase();

        for process in sys.processes_by_name(&window_class) {
            println!(
                "pid: [{}] Active Window: [{}]",
                process.pid(),
                process.name()
            );
        }
    }
}
