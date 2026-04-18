/// This file is used to store code that will be used for both wayland or x11
use std::ffi::OsStr;
use std::fs::{self, create_dir_all, write};
use std::path::PathBuf;
use std::process::Command;

use crate::utils::args::Cli;

use anyhow::*;
use tracing::*;

const SERVICE_NAME: &str = "life-monitor.service";
const SESSION_ENV_VARS: [&str; 6] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XAUTHORITY",
    "HYPRLAND_INSTANCE_SIGNATURE",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn detect_display_server_from_values(
    wayland_display: Option<&str>,
    wayland_socket: Option<&str>,
    xdg_session_type: Option<&str>,
    hyprland_signature: Option<&str>,
    display: Option<&str>,
) -> DisplayServer {
    let has_wayland_display = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let has_wayland_socket = wayland_socket.is_some_and(|value| !value.trim().is_empty());
    let has_hyprland_signature = hyprland_signature.is_some_and(|value| !value.trim().is_empty());
    let session_type = xdg_session_type.map(|value| value.trim().to_ascii_lowercase());
    let has_x11_display = display.is_some_and(|value| !value.trim().is_empty());

    if has_hyprland_signature
        || has_wayland_socket
        || has_wayland_display
        || session_type.as_deref() == Some("wayland")
    {
        DisplayServer::Wayland
    } else if has_x11_display || session_type.as_deref() == Some("x11") {
        DisplayServer::X11
    } else {
        DisplayServer::Unknown
    }
}

pub fn detect_display_server() -> DisplayServer {
    detect_display_server_from_values(
        non_empty_env("WAYLAND_DISPLAY").as_deref(),
        non_empty_env("WAYLAND_SOCKET").as_deref(),
        non_empty_env("XDG_SESSION_TYPE").as_deref(),
        non_empty_env("HYPRLAND_INSTANCE_SIGNATURE").as_deref(),
        non_empty_env("DISPLAY").as_deref(),
    )
}

fn user_unit_dir() -> Result<PathBuf> {
    let path = expand_home("~/.config/systemd/user");
    if !path.exists() {
        create_dir_all(&path).with_context(|| {
            format!(
                "Failed to create user systemd unit directory: {}",
                path.display()
            )
        })?;
    }
    Ok(path)
}

fn service_unit_path() -> Result<PathBuf> {
    Ok(user_unit_dir()?.join(SERVICE_NAME))
}

fn unit_escape(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn collect_session_environment() -> Vec<(String, String)> {
    SESSION_ENV_VARS
        .iter()
        .filter_map(|key| non_empty_env(key).map(|value| ((*key).to_string(), value)))
        .collect()
}

fn render_service_unit(
    executable: &std::path::Path,
    working_dir: &std::path::Path,
    session_env: &[(String, String)],
) -> String {
    let environment_lines = session_env
        .iter()
        .map(|(key, value)| format!("Environment={}={}", key, unit_escape(value)))
        .collect::<Vec<_>>()
        .join("\n");

    let mut unit = format!(
        "[Unit]\n\
Description=Life Monitor activity tracker\n\
After=graphical-session.target\n\
PartOf=graphical-session.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={}\n\
ExecStart={}\n\
Restart=on-failure\n\
RestartSec=3\n\
",
        unit_escape(&working_dir.display().to_string()),
        unit_escape(&executable.display().to_string()),
    );

    if !environment_lines.is_empty() {
        unit.push_str(&environment_lines);
        unit.push('\n');
    }

    unit.push_str(
        "\n[Install]\n\
WantedBy=default.target\n",
    );

    unit
}

fn run_systemctl<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args_vec = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let output = Command::new("systemctl")
        .arg("--user")
        .args(&args_vec)
        .output()
        .with_context(|| "Failed to invoke systemctl --user")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow!(
        "systemctl --user {:?} failed with status {}. stdout: {} stderr: {}",
        args_vec,
        output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |code| code.to_string()),
        stdout.trim(),
        stderr.trim()
    ))
}

fn import_session_environment(session_env: &[(String, String)]) -> Result<()> {
    if session_env.is_empty() {
        warn!("No graphical session environment variables were available to import into systemd --user");
        return Ok(());
    }

    let env_names = session_env
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    run_systemctl(std::iter::once("import-environment".to_string()).chain(env_names.clone()))
        .with_context(|| "Failed to import session environment into systemd --user")?;

    let key_values = session_env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    run_systemctl(std::iter::once("set-environment".to_string()).chain(key_values))
        .with_context(|| "Failed to set session environment in systemd --user")?;

    Ok(())
}

#[allow(dead_code)]
pub fn check_startup_status() -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "is-enabled", SERVICE_NAME])
        .output()
        .with_context(|| "Failed to invoke systemctl --user is-enabled")?;
    let is_enabled = output.status.success();

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
    let current_exe = std::env::current_exe()
        .with_context(|| "Could not determine the filesystem path of the application")?;
    let working_dir = current_exe
        .parent()
        .with_context(|| "Current executable path did not have a parent directory")?;

    if args.enable_startup {
        let session_env = collect_session_environment();
        let unit_path = service_unit_path()?;
        let service_unit = render_service_unit(&current_exe, working_dir, &session_env);

        write(&unit_path, &service_unit).with_context(|| {
            format!(
                "Failed to write the contents of the unit service into: {}",
                unit_path.display()
            )
        })?;

        info!("Unit file successfully created at: {}", unit_path.display());
        import_session_environment(&session_env)?;

        run_systemctl(["daemon-reload"]).with_context(|| {
            format!(
                "Failed to reload systemd --user after creating service unit: {}",
                unit_path.display()
            )
        })?;
        info!("Reloaded systemctl daemon");

        run_systemctl(["enable", "--now", SERVICE_NAME]).with_context(|| {
            format!(
                "Failed to enable and start service located at: {}",
                unit_path.display()
            )
        })?;
        info!(
            "Enabled and started systemctl service: {}, unit file can be found at: {}",
            SERVICE_NAME,
            unit_path.display()
        );
        if session_env.is_empty() {
            warn!("Startup was enabled without capturing any graphical session variables. If the service cannot connect to Wayland or X11, re-run '--enable-startup' from inside your graphical session.");
        }
        warn!("Startup is now enabled. If the executable path changes, re-run '--enable-startup' so the unit file points to the new binary.");
    }

    if args.disable_startup {
        if let Err(err) = run_systemctl(["stop", SERVICE_NAME]) {
            warn!("Failed to stop service {SERVICE_NAME}: {err:#}");
        }

        run_systemctl(["disable", SERVICE_NAME]).with_context(|| {
            format!("Successfully stopped service {SERVICE_NAME} but failed to disable it")
        })?;

        info!("Systemctl services were stopped or were not running already");
        let unit_f = service_unit_path()?;
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

        run_systemctl(["daemon-reload"])
            .with_context(|| "Failed to reload systemd --user after removing service unit")?;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn uptime() -> Result<u64> {
    let content =
        fs::read_to_string("/proc/uptime").with_context(|| "Failed to read /proc/uptime")?;

    content
        .split_whitespace()
        .next()
        .with_context(|| "Unexpected /proc/uptime format")?
        .split('.')
        .next()
        .ok_or(anyhow!("Failed to parse uptime string"))?
        .parse()
        .with_context(|| "Failed to parse uptime string")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Verifies that absolute or already-expanded paths pass through unchanged, because
    /// systemd unit locations may already be resolved before reaching this helper.
    #[test]
    fn expand_home_leaves_plain_paths_unchanged() {
        let path = "/tmp/life-monitor.service";
        assert_eq!(expand_home(path), PathBuf::from(path));
    }

    /// Verifies that `~/` prefixes expand against the current HOME directory, which is
    /// required for writing user service files into the expected systemd locations.
    #[test]
    fn expand_home_expands_tilde_prefix() {
        let home = std::env::var("HOME").expect("HOME should exist in the test environment");
        let expanded = expand_home("~/.config/systemd/user");

        assert_eq!(expanded, PathBuf::from(home).join(".config/systemd/user"));
    }

    /// Verifies that Wayland is preferred when both Wayland and X11-related variables
    /// are present, which is common in Wayland sessions with Xwayland compatibility.
    #[test]
    fn detect_display_server_prefers_wayland_when_both_are_present() {
        assert_eq!(
            detect_display_server_from_values(
                Some("wayland-1"),
                None,
                Some("wayland"),
                None,
                Some(":0"),
            ),
            DisplayServer::Wayland
        );
    }

    /// Verifies that Hyprland-specific environment is enough to classify the session as
    /// Wayland even if generic session variables are missing.
    #[test]
    fn detect_display_server_recognizes_hyprland_sessions() {
        assert_eq!(
            detect_display_server_from_values(None, None, None, Some("hypr-test"), Some(":0")),
            DisplayServer::Wayland
        );
    }

    /// Verifies that plain X11 sessions still classify as X11 when no Wayland indicators
    /// are available.
    #[test]
    fn detect_display_server_recognizes_x11_sessions() {
        assert_eq!(
            detect_display_server_from_values(None, None, Some("x11"), None, Some(":0")),
            DisplayServer::X11
        );
    }

    /// Verifies that the environment-backed detector uses the same precedence rules as the
    /// pure helper when real process variables are set.
    #[test]
    fn detect_display_server_reads_process_environment() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        std::env::remove_var("WAYLAND_SOCKET");
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", "hypr-test");
        std::env::set_var("DISPLAY", ":0");

        assert_eq!(detect_display_server(), DisplayServer::Wayland);
    }

    /// Verifies that user-managed units are written to the per-user config directory
    /// rather than package-managed system locations.
    #[test]
    fn user_unit_dir_prefers_user_config_directory() -> Result<()> {
        let expected = expand_home("~/.config/systemd/user");
        assert_eq!(user_unit_dir()?, expected);
        Ok(())
    }

    /// Verifies that service units capture restart behavior and the expected session
    /// environment variables so systemd --user launches remain session-aware.
    #[test]
    fn render_service_unit_includes_restart_policy_and_session_environment() {
        let unit = render_service_unit(
            std::path::Path::new("/tmp/life-monitor"),
            std::path::Path::new("/tmp"),
            &[
                ("WAYLAND_DISPLAY".to_string(), "wayland-1".to_string()),
                ("DISPLAY".to_string(), ":0".to_string()),
            ],
        );

        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Environment=WAYLAND_DISPLAY=\"wayland-1\""));
        assert!(unit.contains("Environment=DISPLAY=\":0\""));
        assert!(unit.contains("ExecStart=\"/tmp/life-monitor\""));
    }

    /// Verifies that unit escaping quotes values safely enough for systemd unit files
    /// when paths or environment values contain whitespace or quotes.
    #[test]
    fn unit_escape_quotes_and_escapes_special_characters() {
        assert_eq!(
            unit_escape(r#"/tmp/with "quotes""#),
            r#""/tmp/with \"quotes\"""#
        );
        assert_eq!(
            unit_escape(r#"value with spaces"#),
            r#""value with spaces""#
        );
    }
}
