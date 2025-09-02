/// This file is used to store code that will be used for both wayland or x11
use std::fs;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::process::Command;

use crate::utils::args::Cli;
use serde::Deserialize;

use tracing::*;

const SERVICE_NAME: &str = "life-monitor.service";

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

#[allow(dead_code)]
pub fn check_startup_status() -> Result<bool, std::io::Error> {
    let status = Command::new("systemctl")
        .args(["--user", "is-enabled", SERVICE_NAME])
        .output()?;
    info!("systemctl output: {status:?}");
    let is_enabled = status.status.success();

    info!(
        "Startup status on Linux is {}.",
        if is_enabled { "Enabled" } else { "Disabled" }
    );

    Ok(is_enabled)
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| {
            let username = std::env::var("USER").expect("Cannot determine home directory");
            format!("/home/{username}")
        });

        PathBuf::from(home).join(stripped)
    } else {
        PathBuf::from(path)
    }
}

// According to system/user arch wiki, user units are located at:
//
// /usr/lib/systemd/user/ where units provided by installed packages belong.
// ~/.local/share/systemd/user/ where units of packages that have been installed in the home directory belong.
// /etc/systemd/user/ where system-wide user units are placed by the system administrator. !!! I don't think this shouldn't be used.
// ~/.config/systemd/user/ where the user puts their own units.
//
pub fn configure_startup(args: &Cli) -> Result<(), std::io::Error> {
    // Paths
    let unit_dirs = [
        "/usr/lib/systemd/user/",
        "~/.local/share/systemd/user/",
        "~/.config/systemd/user/",
    ];

    let current_exe = std::env::current_exe()?;
    let working_dir = current_exe.parent().unwrap(); // most likely won't fail

    if args.enable_startup {
        let mut target_dir = None;
        let service_unit = format!(
            r#"
        [Unit]
        Description=Life monitor service used to enable automatic startup
        After=graphical-session.target

        [Service]
        Type=simple
        ExecStart={}
        WorkingDirectory={}

        [Install]
        WantedBy=graphical-session.target
    "#,
            current_exe.display(),
            working_dir.display()
        );

        for (i, dir) in unit_dirs.iter().enumerate() {
            let p = expand_home(dir);

            if p.exists() {
                info!("Found existing unit directory: {}", p.display());
                target_dir = Some(p);
                break;
            }

            if i == unit_dirs.len() - 1 {
                warn!(
                    "No existing unit directory found, creating: {}",
                    p.display()
                );
                create_dir_all(&p)?;
                target_dir = Some(p);
            }
        }
        let target_dir = target_dir
            .clone()
            .expect("Failed to determine or create a systemd unit directory");
        let unit_path = target_dir.join(SERVICE_NAME);

        std::fs::write(&unit_path, &service_unit)?;
        info!("Unit file successfully created at: {}", unit_path.display());

        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        info!("Reloaded systemctl daemon");

        Command::new("systemctl")
            .args(["--user", "enable", SERVICE_NAME])
            .status()?;
        info!("Enabled user systemd service: {}", SERVICE_NAME);

        info!("Startup is now enabled, if you happen to change the directory where the program is you will need to re-enable it by running the program with the flag '--enable-startup' again, otherwise it will fail to auto start");
    }

    if args.disable_startup {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", SERVICE_NAME])
            .status()
            .map_err(|err| {
                error!("failed to stop life-monitor service due to: {err}");
                panic!();
            });
        let _ = Command::new("systemctl")
            .args(["--user", "disable", SERVICE_NAME])
            .status()
            .map_err(|err| {
                error!("Failed to disable life-monitor service due to: {err}");
                panic!();
            });
        info!("Systemctl services were stopped or were not running already");
        for dir in unit_dirs {
            let unit_f = expand_home(dir).join(SERVICE_NAME);
            if unit_f.exists() {
                let _ = fs::remove_file(unit_f.clone()).map_err(|err| {
                    error!(
                        "Failed to remove unit file '{}' due to: {err}",
                        unit_f.display()
                    );
                    panic!()
                });
                info!(
                    "Disabled service '{}' and removed unit file: '{}'",
                    SERVICE_NAME,
                    unit_f.display()
                );
            }
        }
    }

    Ok(())
}
