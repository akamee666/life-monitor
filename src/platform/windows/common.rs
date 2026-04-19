#![allow(dead_code)]

use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tracing::*;
use windows::core::Error;

use anyhow::{anyhow, Result};

use crate::common::{ProcessTracker, Window};
#[cfg(test)]
use crate::common::{DEFAULT_BUCKET_MINUTES, DEFAULT_SOURCE_ID};
use crate::Cli;
use windows::Win32::{
    Foundation::*,
    System::SystemInformation::*,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    },
};

use std::env;

use std::time::Duration;

use windows::Win32::System::SystemInformation::GetTickCount;

pub fn check_startup_status() -> Result<bool> {
    let startup_folder = env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|path| {
            path.join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup")
                .join("life-monitor.lnk")
        })
        .unwrap_or_default();

    let startup_exists = startup_folder.exists();

    info!("Startup status on Windows:");
    info!(
        "  Startup Folder: {}",
        if startup_exists {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    Ok(startup_exists)
}

pub fn configure_startup(_args: &Cli) -> Result<()> {
    unimplemented!();
    // let startup_folder = if let Some(appdata) = env::var_os("APPDATA") {
    //     PathBuf::from(appdata)
    //         .join("Microsoft")
    //         .join("Windows")
    //         .join("Start Menu")
    //         .join("Programs")
    //         .join("Startup")
    // } else {
    //     return Err(anyhow!("Could not find APPDATA environment variable"));
    // };
    // let shortcut_path = startup_folder.join("life_monitor.lnk");
    // let current_exe = env::current_exe()?;
    //
    // if args.enable_startup {
    //     // Using PowerShell to create shortcut since it's more reliable than direct COM automation
    //     let ps_script = format!(
    //         "$WScriptShell = New-Object -ComObject WScript.Shell; \
    //          $Shortcut = $WScriptShell.CreateShortcut('{}'); \
    //          $Shortcut.TargetPath = '{}'; \
    //          $Shortcut.Save()",
    //         shortcut_path.to_str().unwrap(),
    //         current_exe.to_str().unwrap()
    //     );
    //
    //     Command::new("powershell")
    //         .arg("-Command")
    //         .arg(&ps_script)
    //         .output()?;
    //
    //     info!("Created startup shortcut at: {:?}", shortcut_path);
    // } else if shortcut_path.exists() {
    //     fs::remove_file(&shortcut_path)?;
    //     info!("Removed startup shortcut");
    // }
    //
    // Ok(())
}

// Returns window title and class in that order.
pub fn get_focused_window() -> Result<(String, String)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!(Error::from_win32()));
        }

        let mut process_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_pid));
        Ok((read_window_title(hwnd), resolve_process_name(process_pid)?))
    }
}

fn read_window_title(hwnd: HWND) -> String {
    unsafe {
        let mut title: [u16; 256] = [0; 256];
        let title_len = GetWindowTextW(hwnd, &mut title);
        OsString::from_wide(&title[..title_len as usize])
            .to_string_lossy()
            .into_owned()
    }
}

fn resolve_process_name(process_pid: u32) -> Result<String> {
    if process_pid == 0 {
        return Err(anyhow!("foreground window process id was missing"));
    }

    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );

    let process = sys
        .processes()
        .get(&Pid::from_u32(process_pid))
        .ok_or_else(|| anyhow!("foreground process {process_pid} was not found"))?;

    Ok(process.name().to_string_lossy().to_string())
}

pub fn get_idle_time() -> Result<Duration> {
    // Retrieves the number of milliseconds that have elapsed since the system was started, up to 49.7 days.
    // we will be using it to get how much time was went since the last user input
    let tick_count = unsafe { GetTickCount() };

    // struct defined by windows.
    let mut last_input_info = LASTINPUTINFO {
        cbSize: 8,
        dwTime: 0,
    };

    let p_last_input_info = &mut last_input_info as *mut LASTINPUTINFO;

    let success = unsafe { GetLastInputInfo(p_last_input_info) };

    if !success.as_bool() {
        return Err(anyhow!(Error::from_win32()));
    }

    let diff = tick_count - last_input_info.dwTime;
    Ok(Duration::from_millis(diff.into()))
}

pub fn should_be_idle(idle_time: Duration) -> bool {
    idle_time >= Duration::from_secs(20)
}

pub fn sync_focus_tracker(
    tracker: &mut ProcessTracker,
    focused_window: Option<Window>,
    now: chrono::DateTime<chrono::Utc>,
    idle: bool,
) {
    if idle {
        tracker.pause(now);
        return;
    }

    let Some(window) = focused_window else {
        tracker.clear_focus(now);
        return;
    };

    if active_window_changed(tracker, &window) {
        tracker.switch_window(window, now);
    } else {
        tracker.resume(now);
    }
}

fn active_window_changed(tracker: &ProcessTracker, window: &Window) -> bool {
    tracker.current_window_name() != Some(window.name.as_str())
        || tracker.current_window_class() != Some(window.class.as_str())
}

#[cfg(target_os = "windows")]
pub fn uptime() -> u64 {
    unsafe { GetTickCount64() / 1_000 }
}

pub fn is_idle() -> bool {
    get_idle_time().map(should_be_idle).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn should_be_idle_uses_real_idle_duration_threshold() {
        assert!(!should_be_idle(Duration::from_secs(19)));
        assert!(should_be_idle(Duration::from_secs(20)));
        assert!(should_be_idle(Duration::from_secs(45)));
    }

    #[test]
    fn sync_focus_tracker_switches_when_window_class_changes() {
        let mut tracker = ProcessTracker::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let title = "Project Plan".to_string();
        let editor = Window {
            name: title.clone(),
            class: "notepad.exe".to_string(),
        };
        let browser = Window {
            name: title,
            class: "firefox.exe".to_string(),
        };

        sync_focus_tracker(
            &mut tracker,
            Some(editor),
            Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            false,
        );
        sync_focus_tracker(
            &mut tracker,
            Some(browser),
            Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 30).unwrap(),
            false,
        );
        tracker.record_active_until(Utc.with_ymd_and_hms(2026, 4, 18, 12, 1, 0).unwrap());

        let rows = tracker.drain_pending();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].window_class, "notepad.exe");
        assert_eq!(rows[0].focus_seconds, 30);
        assert_eq!(rows[1].window_class, "firefox.exe");
        assert_eq!(rows[1].focus_seconds, 30);
    }

    #[test]
    fn sync_focus_tracker_pauses_and_resumes_current_window() {
        let mut tracker = ProcessTracker::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();
        let resume = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 10).unwrap();
        let flush = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 20).unwrap();
        let window = Window {
            name: "Editor".to_string(),
            class: "nvim".to_string(),
        };

        sync_focus_tracker(&mut tracker, Some(window.clone()), start, false);
        sync_focus_tracker(&mut tracker, Some(window.clone()), resume, true);
        sync_focus_tracker(&mut tracker, Some(window), flush, false);
        tracker.record_active_until(Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 30).unwrap());

        let rows = tracker.drain_pending();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].focus_seconds, 20);
    }

    #[test]
    fn sync_focus_tracker_switches_windows_and_clears_when_missing() {
        let mut tracker = ProcessTracker::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let editor = Window {
            name: "Editor".to_string(),
            class: "nvim".to_string(),
        };
        let browser = Window {
            name: "Browser".to_string(),
            class: "firefox".to_string(),
        };

        sync_focus_tracker(
            &mut tracker,
            Some(editor),
            Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            false,
        );
        sync_focus_tracker(
            &mut tracker,
            Some(browser),
            Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 30).unwrap(),
            false,
        );
        sync_focus_tracker(
            &mut tracker,
            None,
            Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 45).unwrap(),
            false,
        );

        let rows = tracker.drain_pending();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].focus_seconds, 30);
        assert_eq!(rows[1].focus_seconds, 15);
    }
}
