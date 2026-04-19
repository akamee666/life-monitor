#![allow(dead_code)]

use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};
use tracing::*;
use windows::core::Error;

use std::result::Result::Ok;

use anyhow::*;

use crate::Cli;
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::SystemInformation::*,
    UI::HiDpi::*,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SystemParametersInfoA,
            SPI_GETMOUSE, SPI_GETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        },
    },
};

use std::env;

use std::time::Duration;

use windows::Win32::System::SystemInformation::GetTickCount;

pub fn check_startup_status() -> Result<bool> {
    use std::path::PathBuf;

    // Check Startup folder
    let startup_folder: PathBuf = if let Ok(appdata) = env::var("APPDATA") {
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("life-monitor.lnk")
    } else {
        PathBuf::new()
    };

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

        let mut title: [u16; 256] = [0; 256];
        let title_len = GetWindowTextW(hwnd, &mut title);
        if title_len == 0 {
            return Err(anyhow!(Error::from_win32()));
        }

        // Convert the title from UTF-16 to String
        let w_title = OsString::from_wide(&title[..title_len as usize])
            .to_string_lossy()
            .into_owned();

        let mut process_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_pid));
        let sys = sysinfo::System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );
        let proc = sys.processes().get(&Pid::from_u32(process_pid)).unwrap();
        let w_class = proc.name().to_string_lossy().to_string();
        Ok((w_title, w_class))
    }
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

    if tracker.current_window_name() != Some(window.name.as_str()) {
        tracker.switch_window(window, now);
    } else {
        tracker.resume(now);
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MouseSettings {
    pub threshold: i32,
    pub threshold2: i32,
    pub acceleration: i32,
    pub speed: i32,
    pub enhanced_pointer_precision: bool,
    pub dpi: u32,
}

// Default values form a fresh install of windows 10.
// Didn't cover win11, maybe it has changed.
impl Default for MouseSettings {
    fn default() -> Self {
        MouseSettings {
            threshold: 6,
            threshold2: 10,
            acceleration: 1,
            speed: 10,
            enhanced_pointer_precision: true,
            dpi: 800,
        }
    }
}

/// MouseSettings { threshold: 0, threshold2: 0, acceleration: 0, speed: 10, enhanced_pointer_precision: false }
impl MouseSettings {
    // WARN: These zero values can possibly fuck the calcs.
    #[allow(dead_code)]
    pub fn noacc_default() -> Self {
        MouseSettings {
            threshold: 0,
            threshold2: 0,
            acceleration: 0,
            speed: 10,
            enhanced_pointer_precision: false,
            dpi: 800,
        }
    }
}

fn get_screen_dpi() -> Result<(u32, u32), windows::core::Error> {
    unsafe {
        let screen_dc = GetDC(None);
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        let monitor = MonitorFromWindow(HWND(std::ptr::null_mut()), MONITOR_DEFAULTTOPRIMARY);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)?;
        let _ = ReleaseDC(None, screen_dc);
        Ok((dpi_x, dpi_y))
    }
}

pub fn get_mouse_settings() -> Result<MouseSettings> {
    let mut mouse_params = [0i32; 3];
    let mut speed = 0i32;
    let mut enhanced_pointer_precision = 0i32;

    // https://stackoverflow.com/questions/60268940/sendinput-mouse-movement-calculation
    // Threshold values are only set if enhanced_pointer_precision is true.
    unsafe {
        SystemParametersInfoA(
            SPI_GETMOUSE,
            0,
            Some(mouse_params.as_mut_ptr() as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        SystemParametersInfoA(
            SPI_GETMOUSESPEED,
            0,
            Some(&mut speed as *mut i32 as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        SystemParametersInfoA(
            SPI_GETMOUSE,
            0,
            Some(&mut enhanced_pointer_precision as *mut i32 as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }

    let mouse_settings: MouseSettings = MouseSettings {
        threshold: mouse_params[0],
        threshold2: mouse_params[1],
        acceleration: mouse_params[2],
        speed,
        enhanced_pointer_precision: enhanced_pointer_precision != 0,
        // TODO:
        dpi: 800,
    };

    debug!("Mouse settings: {:?}", mouse_settings);

    Ok(mouse_settings)
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
