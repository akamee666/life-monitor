use serde::Deserialize;
use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};
use tracing::*;
use windows::core::Error;

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

pub fn check_startup_status() -> Result<bool, Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    use winreg::enums::*;
    use winreg::RegKey;

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

    // Also check Registry (as a fallback since some programs use this method)
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let startup_path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let startup = hkcu.open_subkey(startup_path)?;

    let current_exe = env::current_exe()?;
    let exe_path = current_exe.to_string_lossy().to_string();

    let registry_enabled = match startup.get_value::<String, _>("LifeMonitor") {
        Ok(path) => path == exe_path,
        Err(_) => false,
    };

    let is_enabled = startup_exists || registry_enabled;

    info!("Startup status on Windows:");
    info!(
        "  Startup Folder: {}",
        if startup_exists {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    info!(
        "  Registry: {}",
        if registry_enabled {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    info!(
        "  Overall: {}",
        if is_enabled { "Enabled" } else { "Disabled" }
    );

    Ok(is_enabled)
}

pub fn configure_startup(
    should_enable: bool,
    is_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let startup_folder = if let Some(appdata) = env::var_os("APPDATA") {
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
    } else {
        return Err("Could not find APPDATA environment variable".into());
    };

    let shortcut_path = startup_folder.join("life_monitor.lnk");
    let current_exe = env::current_exe()?;

    if should_enable {
        if is_enabled {
            info!("Startup is already enabled!");
            return Ok(());
        }
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
pub fn get_focused_window() -> Result<(String, String), windows::core::Error> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return Err(Error::from_win32());
        }

        let mut title: [u16; 256] = [0; 256];
        let title_len = GetWindowTextW(hwnd, &mut title);
        if title_len == 0 {
            return Err(Error::from_win32());
        }

        // Convert the title from UTF-16 to String
        let w_title = OsString::from_wide(&title[..title_len as usize])
            .to_string_lossy()
            .into_owned();

        let mut process_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_pid));
        let sys = sysinfo::System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new()),
        );
        let proc = sys.processes().get(&Pid::from_u32(process_pid)).unwrap();
        let w_class = proc.name().to_string_lossy().to_string();
        Ok((w_title, w_class))
    }
}

pub fn get_idle_time() -> Result<Duration, windows::core::Error> {
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
        return Err(Error::from_win32());
    }

    let diff = tick_count - last_input_info.dwTime;
    Ok(Duration::from_millis(diff.into()))
}

#[derive(Debug, Clone, Deserialize)]
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

pub fn get_mouse_settings() -> Result<MouseSettings, windows::core::Error> {
    let mut mouse_params = [0i32; 3];
    let mut speed = 0i32;
    let mut enhanced_pointer_precision = 0i32;

    // https://stackoverflow.com/questions/60268940/sendinput-mouse-movement-calculation
    // Threshold values are only set if enhanced_pointer_precision is true.
    unsafe {
        // Get mouse parameters
        SystemParametersInfoA(
            SPI_GETMOUSE,
            0,
            Some(mouse_params.as_mut_ptr() as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;

        // Get mouse speed
        SystemParametersInfoA(
            SPI_GETMOUSESPEED,
            0,
            Some(&mut speed as *mut i32 as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;

        // Get enhanced pointer precision setting
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
