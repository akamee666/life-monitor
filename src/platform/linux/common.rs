/// This file is used to store code that will be used for both wayland or x11
use std::fs::{self, create_dir_all, write};
use std::path::PathBuf;
use std::process::Command;

use crate::utils::args::Cli;

use anyhow::*;
use tracing::*;

const SERVICE_NAME: &str = "life-monitor.service";

#[allow(dead_code)]
pub fn check_startup_status() -> Result<bool> {
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
// `/usr/lib/systemd/user/` where units provided by installed packages belong.
// `~/.local/share/systemd/user/` where units of packages that have been installed in the home directory belong.
// `/etc/systemd/user/` where system-wide user units are placed by the system administrator. !!! I don't think this shouldn't be used.
// `~/.config/systemd/user/` where the user puts their own units.

/// This function is used to enable or disabling the startup of the program using `systemctl`
pub fn configure_startup(args: &Cli) -> Result<()> {
    // Paths
    let unit_dirs = [
        "/usr/lib/systemd/user/",
        "~/.local/share/systemd/user/",
        "~/.config/systemd/user/",
    ];

    let current_exe = std::env::current_exe()
        .with_context(|| "Could not determine the filesystem path of the application")?;
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
                create_dir_all(&p).with_context(|| {
                    format!(
                        "Failed to create directory {} to place our service unit",
                        p.display()
                    )
                })?;
                target_dir = Some(p);
            }
        }
        let target_dir = target_dir.clone().unwrap(); // Safe, we find the directory or create one.
        let unit_path = target_dir.join(SERVICE_NAME);

        write(&unit_path, &service_unit).with_context(|| {
            format!(
                "Failed to write the contents of the unit service into: {}",
                unit_path.display()
            )
        })?;

        info!("Unit file successfully created at: {}", unit_path.display());
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .with_context(|| {
                format!(
                    "Failed to reload systemctl deamon after creating service unit: {}",
                    unit_path.display()
                )
            })?;
        info!("Reloaded systemctl daemon");

        Command::new("systemctl")
            .args(["--user", "enable", SERVICE_NAME])
            .status()
            .with_context(|| {
                format!(
                    "Failed to enable our service located at: {}",
                    unit_path.display()
                )
            })?;
        info!(
            "Enabled systemctl service: {}, unit file can be found at: {}",
            SERVICE_NAME,
            unit_path.display()
        );
        warn!("Startup is now enabled, if you happen to change the directory where the program is you will need to re-enable it by running the program with the flag '--enable-startup' again, otherwise it will fail to auto start");
    }

    if args.disable_startup {
        Command::new("systemctl")
            .args(["--user", "stop", SERVICE_NAME])
            .status()
            .with_context(|| format!("Failed to stop service {SERVICE_NAME} to disable startup"))?;

        Command::new("systemctl")
            .args(["--user", "disable", SERVICE_NAME])
            .status()
            .with_context(|| {
                format!("Succesfully stopped service {SERVICE_NAME} but failed to disable it")
            })?;

        info!("Systemctl services were stopped or were not running already");
        for dir in unit_dirs {
            let unit_f = expand_home(dir).join(SERVICE_NAME);
            if unit_f.exists() {
                fs::remove_file(unit_f.clone()).with_context(|| {
                    format!(
                        "Stopped and disabled service {SERVICE_NAME} but failed to remove unit file {}. Please remove it manually",
                        unit_f.display()
                    )
                })?;
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
