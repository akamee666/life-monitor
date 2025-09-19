use serde::Deserialize;
use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};
use tracing::*;
use windows::core::Error;

use std::result::Result::Ok;

use anyhow::*;

use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::Cli;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use windows::Win32::{
    System::SystemInformation::GetTickCount,
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SystemParametersInfoA,
            SPI_GETMOUSE, SPI_GETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        },
    },
};

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

pub fn configure_startup(args: &Cli) -> Result<()> {
    unimplemented!();
    let startup_folder = if let Some(appdata) = env::var_os("APPDATA") {
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
    } else {
        return Err(anyhow!("Could not find APPDATA environment variable"));
    };
    let shortcut_path = startup_folder.join("life_monitor.lnk");
    let current_exe = env::current_exe()?;

    if args.enable_startup {
        // Using PowerShell to create shortcut since it's more reliable than direct COM automation
        let ps_script = format!(
            "$WScriptShell = New-Object -ComObject WScript.Shell; \
             $Shortcut = $WScriptShell.CreateShortcut('{}'); \
             $Shortcut.TargetPath = '{}'; \
             $Shortcut.Save()",
            shortcut_path.to_str().unwrap(),
            current_exe.to_str().unwrap()
        );

        Command::new("powershell")
            .arg("-Command")
            .arg(&ps_script)
            .output()?;

        info!("Created startup shortcut at: {:?}", shortcut_path);
    } else if shortcut_path.exists() {
        fs::remove_file(&shortcut_path)?;
        info!("Removed startup shortcut");
    }

    Ok(())
}

// Returns window title and class in that order.
pub fn get_focused_window() -> Result<(String, String)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
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

#[derive(Debug, Clone, Deserialize)]
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
        dpi: 800,
    };

    debug!("Mouse settings: {:?}", mouse_settings);

    Ok(mouse_settings)
}

#[cfg(target_os = "windows")]
fn uptime() -> u64 {
    unsafe { GetTickCount64() / 1_000 }
}

pub fn is_idle() -> bool {
    if uptime() > 20 {
        return true;
    }
    false
}
