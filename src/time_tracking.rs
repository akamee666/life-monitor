pub async fn start_tracking() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::spawn(crate::linux::process::ProcessTracker::track_processes()).await;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = tokio::spawn(crate::win::process::ProcessTracker::track_processes()).await;
    }
}
