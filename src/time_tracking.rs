// struct TimeWastedPerCategory {
//     time_coding: u64,
//     time_gaming: u64,
//     time_entertainment: u64,
//     others: u64,
// }
//
// struct TimeWasted {
//     process_name: String,
//     time: u64,
// }

pub async fn start_tracking() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::spawn(crate::linux::process::track_processes()).await;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = tokio::spawn(crate::win::process::track_processes()).await;
    }
}
