/// This file is used to store code that will be used for both wayland or x11
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use tracing::*;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct MouseSettings {
    pub threshold: u16,
    pub acceleration_denominator: u16,
    pub acceleration_numerator: u16,
    pub dpi: u32,
}

// Default values from ArchLinux, didn't check for other OS's.
impl Default for MouseSettings {
    fn default() -> Self {
        MouseSettings {
            threshold: 4,
            acceleration_numerator: 2,
            acceleration_denominator: 1,
            dpi: 800,
        }
    }
}

impl MouseSettings {
    #[allow(dead_code)]
    pub fn noacc_default() -> Self {
        MouseSettings {
            acceleration_numerator: 1,
            acceleration_denominator: 1,
            threshold: 0,
            dpi: 800,
        }
    }
}

// According to system/user arch wiki, user units are located at:
//
// /usr/lib/systemd/user/ where units provided by installed packages belong.
// ~/.local/share/systemd/user/ where units of packages that have been installed in the home directory belong.
// /etc/systemd/user/ where system-wide user units are placed by the system administrator.
// ~/.config/systemd/user/ where the user puts their own units.
//
// If you want to start units on first login, execute systemctl --user enable unit for any unit you want to be autostarted.
//
// TODO: This need fix bc it can fail in a numerous of ways
pub fn configure_startup(should_enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    let is_enable = check_startup_status()?;

    // let unit_dirs = Vec<Path> = ["/usr/lib/systemd/user/", "~/.local/share/systemd/user/", "~/.config/systemd/user/"];
    // For our binary:
    // We may launch directly: life-monitor
    // We may search in: $CARGO_HOME
    // We may get the path in the first run with: std::env::current_exe()
    // Making a service is just a question of writing a short configuration file that describes how to call your executable.

    // TODO: is there a better way to do it?
    let service_name = "life-monitor.service";
    let user_service_dir = Path::new(&env::var("HOME")?).join(".config/systemd/user");
    fs::create_dir_all(&user_service_dir)?;
    let service_path = user_service_dir.join(service_name);
    let current_exe = env::current_exe()?;

    if should_enable {
        if is_enable {
            info!("Startup is already enabled!");
            return Ok(());
        }
        info!("Creating service for life-monitor");

        let service_content = format!(
            "[Unit]\n\
            Description=Life Monitor Service\n\
            After=display-manager.service\n\
            Wants=graphical-session.target multi-user.target\n\
            \n\
            [Service]\n\
            Type=simple\n\
            Environment=DISPLAY=:0\n\
            Environment=XAUTHORITY=/home/{}/.Xauthority\n\
            ExecStart={}\n\
            Restart=always\n\
            ExecStartPre=/bin/sh -c 'until [ -n \"$DISPLAY\" ] && xset q; do sleep 1; done'
            \n\
            [Install]\n\
            WantedBy=graphical-session.target multi-user.target\n",
            env::var("USER")?,
            current_exe.to_str().unwrap()
        );

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&service_path)?;
        file.write_all(service_content.as_bytes())?;

        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        Command::new("systemctl")
            .args(["--user", "enable", service_name])
            .status()?;
        Command::new("systemctl")
            .args(["--user", "start", service_name])
            .status()?;
        info!("Created and enabled user systemd service: {}", service_name);
    } else {
        Command::new("systemctl")
            .args(["--user", "stop", service_name])
            .status()?;
        Command::new("systemctl")
            .args(["--user", "disable", service_name])
            .status()?;

        if service_path.exists() {
            fs::remove_file(&service_path)?;
        }
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        info!("Removed user systemd service: {}", service_name);
    }
    Ok(())
}

pub fn check_startup_status() -> Result<bool, Box<dyn std::error::Error>> {
    let service_name = "life-monitor.service";

    // Check if service is enabled
    let status = Command::new("systemctl")
        .args(["--user", "is-enabled", service_name])
        .output()?;

    // Also check if the service file exists
    let user_service_dir = Path::new(&env::var("HOME")?).join(".config/systemd/user");
    let service_path = user_service_dir.join(service_name);

    let is_enabled = status.status.success() && service_path.exists();

    info!(
        "Startup status on Linux is {}.",
        if is_enabled { "Enabled" } else { "Disabled" }
    );

    Ok(is_enabled)
}
