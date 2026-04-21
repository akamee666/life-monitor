/// Shared Linux helpers for session detection and startup configuration.
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, create_dir_all, write};
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::utils::args::{CollectorCli, LinuxStartupMode};

use anyhow::*;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use tracing::*;

const SERVICE_NAME: &str = "vigil.service";
const DESKTOP_ENTRY_NAME: &str = "vigil.desktop";
const COLLECTOR_SUBCOMMAND: &str = "collector";
const SESSION_ENV_VARS: [&str; 5] = [
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XAUTHORITY",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XdgAutostartProbe {
    current_desktop: Option<String>,
    desktop_session: Option<String>,
    session_type: Option<String>,
    display_server: DisplayServer,
}

impl XdgAutostartProbe {
    fn gather() -> Self {
        Self {
            current_desktop: non_empty_env("XDG_CURRENT_DESKTOP"),
            desktop_session: non_empty_env("DESKTOP_SESSION"),
            session_type: non_empty_env("XDG_SESSION_TYPE"),
            display_server: detect_display_server(),
        }
    }

    fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.current_desktop.is_none() && self.desktop_session.is_none() {
            warnings.push(
                "No XDG_CURRENT_DESKTOP or DESKTOP_SESSION value was detected. XDG autostart is the recommended default, but minimalist window-manager or compositor setups may require extra session setup.".to_string(),
            );
        }

        if self.display_server == DisplayServer::Unknown {
            warnings.push(
                "The current process does not look like it is running inside a recognizable graphical Wayland or X11 session. Enable startup from the desktop session where you normally run Vigil.".to_string(),
            );
        }

        if self.session_type.is_none() {
            warnings.push(
                "XDG_SESSION_TYPE is not set in the current environment. This does not block XDG autostart, but it is a sign that the session integration may be unusual.".to_string(),
            );
        }

        warnings
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemdEnvironmentProbe {
    missing_keys: Vec<String>,
}

impl SystemdEnvironmentProbe {
    fn gather() -> Result<Self> {
        let output = Command::new("systemctl")
            .args(["--user", "show-environment"])
            .output()
            .with_context(|| "Failed to invoke systemctl --user show-environment")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "systemctl --user show-environment failed: {}",
                stderr.trim()
            ));
        }

        let manager_keys = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_string()))
            .collect::<HashSet<_>>();

        let missing_keys = SESSION_ENV_VARS
            .iter()
            .filter(|key| non_empty_env(key).is_some() && !manager_keys.contains(**key))
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>();

        Ok(Self { missing_keys })
    }

    fn warning(&self) -> Option<String> {
        if self.missing_keys.is_empty() {
            return None;
        }

        Some(format!(
            "systemd --user does not currently expose these graphical-session variables from your login environment: {}. The systemd startup mode expects the desktop session to import them at login, so prefer the XDG mode unless you have already verified your systemd user manager inherits the graphical environment.",
            self.missing_keys.join(", ")
        ))
    }
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
    display: Option<&str>,
) -> DisplayServer {
    let has_wayland_display = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let has_wayland_socket = wayland_socket.is_some_and(|value| !value.trim().is_empty());
    let session_type = xdg_session_type.map(|value| value.trim().to_ascii_lowercase());
    let has_x11_display = display.is_some_and(|value| !value.trim().is_empty());

    if has_wayland_socket || has_wayland_display || session_type.as_deref() == Some("wayland") {
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
        non_empty_env("DISPLAY").as_deref(),
    )
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

fn user_config_dir() -> PathBuf {
    non_empty_env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| expand_home("~/.config"))
}

fn autostart_dir_path() -> PathBuf {
    user_config_dir().join("autostart")
}

fn autostart_dir() -> Result<PathBuf> {
    let path = autostart_dir_path();
    if !path.exists() {
        create_dir_all(&path).with_context(|| {
            format!(
                "Failed to create XDG autostart directory: {}",
                path.display()
            )
        })?;
    }
    Ok(path)
}

fn desktop_entry_path() -> PathBuf {
    autostart_dir_path().join(DESKTOP_ENTRY_NAME)
}

fn user_unit_dir_path() -> PathBuf {
    expand_home("~/.config/systemd/user")
}

fn user_unit_dir() -> Result<PathBuf> {
    let path = user_unit_dir_path();
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

fn service_unit_path() -> PathBuf {
    user_unit_dir_path().join(SERVICE_NAME)
}

fn systemd_path_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(' ', "\\ ")
        .replace('\t', "\\\t")
}

fn desktop_entry_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
}

fn desktop_exec_escape(arg: &str) -> String {
    let reserved = [
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");

    if arg.chars().any(|ch| reserved.contains(&ch)) {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

fn render_desktop_entry(executable: &Path, working_dir: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=Vigil\n\
Comment=Track keyboard, mouse, and focused-window activity\n\
Exec={}\n\
TryExec={}\n\
Path={}\n\
Terminal=false\n\
StartupNotify=false\n\
X-GNOME-Autostart-enabled=true\n",
        format!(
            "{} {}",
            desktop_exec_escape(&executable.display().to_string()),
            desktop_exec_escape(COLLECTOR_SUBCOMMAND)
        ),
        desktop_entry_escape(&executable.display().to_string()),
        desktop_entry_escape(&working_dir.display().to_string()),
    )
}

fn render_service_unit(executable: &Path, working_dir: &Path) -> String {
    format!(
        "[Unit]\n\
Description=Vigil activity tracker\n\
After=graphical-session.target\n\
PartOf=graphical-session.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={}\n\
ExecStart={}\n\
Restart=on-failure\n\
RestartSec=3\n\
Slice=app.slice\n\
\n\
[Install]\n\
WantedBy=graphical-session.target\n",
        systemd_path_escape(&working_dir.display().to_string()),
        format!(
            "{} {}",
            systemd_path_escape(&executable.display().to_string()),
            systemd_path_escape(COLLECTOR_SUBCOMMAND)
        ),
    )
}

fn prompt_startup_mode() -> Result<LinuxStartupMode> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "Enabling Linux startup now requires an interactive choice between XDG and systemd --user modes. Run `vigil collector --enable-startup` from a terminal and choose a mode there."
        ));
    }
    run_startup_mode_picker()
}

fn run_startup_mode_picker() -> Result<LinuxStartupMode> {
    let mut terminal = init_startup_picker_terminal()?;
    let result = run_startup_mode_picker_loop(&mut terminal);
    restore_startup_picker_terminal(&mut terminal)?;
    result
}

fn init_startup_picker_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_startup_picker_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_startup_mode_picker_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<LinuxStartupMode> {
    let choices = startup_mode_choices();
    let mut state = ListState::default();
    state.select(Some(0));

    loop {
        terminal.draw(|frame| {
            let area = centered_popup(frame.area());
            frame.render_widget(Clear, area);
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(6),
                    Constraint::Min(8),
                ])
                .split(area);

            let header = Paragraph::new(vec![
                Line::from(Span::styled(
                    "Choose Startup Mode",
                    Style::default()
                        .fg(Color::Rgb(120, 255, 140))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "Use ↑/↓ to choose and Enter to confirm",
                    Style::default().fg(Color::Rgb(120, 180, 130)),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                    .style(Style::default().bg(Color::Black))
                    .border_style(Style::default().fg(Color::Rgb(35, 80, 40))),
            )
            .alignment(Alignment::Center);

            let items = choices
                .iter()
                .enumerate()
                .map(|choice| {
                    let index = choice.0;
                    let choice = choice.1;
                    let selected = state.selected() == Some(index);
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if selected { "[x] " } else { "[ ] " },
                            Style::default().fg(Color::Rgb(100, 255, 120)),
                        ),
                        Span::styled(choice.title, Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                })
                .collect::<Vec<_>>();

            let chooser = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::LEFT | Borders::RIGHT)
                        .style(Style::default().bg(Color::Black))
                        .border_style(Style::default().fg(Color::Rgb(35, 80, 40))),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Rgb(150, 255, 170))
                        .bg(Color::Rgb(25, 45, 25))
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");

            let selected = state
                .selected()
                .unwrap_or(0)
                .min(choices.len().saturating_sub(1));
            let details = Paragraph::new(choices[selected].description)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .title("When To Choose This")
                        .borders(Borders::ALL)
                        .style(Style::default().bg(Color::Black))
                        .border_style(Style::default().fg(Color::Rgb(35, 80, 40))),
                );

            frame.render_widget(header, sections[0]);
            frame.render_stateful_widget(chooser, sections[1], &mut state);
            frame.render_widget(details, sections[2]);
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let current = state.selected().unwrap_or(0);
                        state.select(Some(current.saturating_sub(1)));
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let current = state.selected().unwrap_or(0);
                        state.select(Some((current + 1).min(choices.len().saturating_sub(1))));
                    }
                    KeyCode::Enter => {
                        let selected = state.selected().unwrap_or(0);
                        return Ok(choices[selected].mode);
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        return Err(anyhow!("Startup selection cancelled by user"));
                    }
                    _ => {}
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct StartupModeChoice {
    mode: LinuxStartupMode,
    title: &'static str,
    description: &'static str,
}

fn startup_mode_choices() -> [StartupModeChoice; 2] {
    [
        StartupModeChoice {
            mode: LinuxStartupMode::Xdg,
            title: "XDG autostart (recommended)",
            description: "Choose this if you use a normal desktop environment where login-session autostart already works for other applications. This is the standard path for GNOME, KDE Plasma, Xfce, Cinnamon, LXQt, MATE, Budgie and most mainstream desktop sessions. Vigil writes a .desktop entry into your XDG autostart directory and your desktop launches it after graphical login.",
        },
        StartupModeChoice {
            mode: LinuxStartupMode::Systemd,
            title: "systemd user service",
            description: "Choose this only if you deliberately run a minimal or custom session where you manage startup yourself and know your graphical environment is integrated with systemd --user. This is more likely in setups built around i3, sway, Hyprland, bspwm, river, awesome, dwm or other hand-configured window-manager/compositor sessions. If you are unsure, do not choose this one first.",
        },
    ]
}

fn centered_popup(area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(area.height.min(20)),
            Constraint::Fill(1),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(area.width.min(90)),
            Constraint::Fill(1),
        ])
        .split(vertical[1]);
    horizontal[1]
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

#[allow(dead_code)]
pub fn check_startup_status() -> Result<bool> {
    let xdg_enabled = desktop_entry_path().exists();
    let systemd_enabled = Command::new("systemctl")
        .args(["--user", "is-enabled", SERVICE_NAME])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    let is_enabled = xdg_enabled || systemd_enabled;

    info!(
        "Startup status on Linux is {}.",
        if is_enabled { "Enabled" } else { "Disabled" }
    );

    Ok(is_enabled)
}

fn looks_like_repo_build_output(executable: &Path) -> bool {
    let components = executable
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    components.windows(2).any(|window| {
        matches!(
            window,
            [target, profile]
                if target == "target" && (profile == "debug" || profile == "release")
        )
    })
}

fn write_xdg_autostart(executable: &Path, working_dir: &Path) -> Result<PathBuf> {
    let entry_path = desktop_entry_path();
    let autostart_dir = autostart_dir()?;
    let desktop_entry = render_desktop_entry(executable, working_dir);

    write(&entry_path, desktop_entry).with_context(|| {
        format!(
            "Failed to write the XDG autostart desktop entry to {}",
            entry_path.display()
        )
    })?;

    debug!(
        "Ensured XDG autostart directory exists at '{}'.",
        autostart_dir.display()
    );

    Ok(entry_path)
}

fn write_systemd_startup(executable: &Path, working_dir: &Path) -> Result<PathBuf> {
    let unit_dir = user_unit_dir()?;
    let unit_path = service_unit_path();
    let service_unit = render_service_unit(executable, working_dir);

    write(&unit_path, &service_unit).with_context(|| {
        format!(
            "Failed to write the contents of the unit service into: {}",
            unit_path.display()
        )
    })?;

    debug!(
        "Ensured systemd user unit directory exists at '{}'.",
        unit_dir.display()
    );

    run_systemctl(["daemon-reload"]).with_context(|| {
        format!(
            "Failed to reload systemd --user after creating service unit: {}",
            unit_path.display()
        )
    })?;

    run_systemctl(["enable", SERVICE_NAME]).with_context(|| {
        format!(
            "Failed to enable service located at: {}",
            unit_path.display()
        )
    })?;

    Ok(unit_path)
}

fn remove_xdg_autostart() -> Result<()> {
    let entry_path = desktop_entry_path();
    if entry_path.exists() {
        fs::remove_file(&entry_path).with_context(|| {
            format!(
                "Failed to remove XDG autostart desktop entry {}",
                entry_path.display()
            )
        })?;
        info!("Removed XDG autostart entry at '{}'.", entry_path.display());
    } else {
        debug!(
            "No XDG autostart entry exists at '{}'; nothing to remove.",
            entry_path.display()
        );
    }
    Ok(())
}

fn remove_systemd_startup() -> Result<()> {
    let unit_path = service_unit_path();
    if !unit_path.exists() {
        debug!(
            "No Vigil systemd user unit exists at '{}'; nothing to remove.",
            unit_path.display()
        );
        return Ok(());
    }

    info!(
        "Removing Vigil systemd --user startup unit at '{}'.",
        unit_path.display()
    );

    if let Err(err) = run_systemctl(["stop", SERVICE_NAME]) {
        warn!("Failed to stop service {SERVICE_NAME}: {err:#}");
    } else {
        info!("Stopped systemd --user service '{}'.", SERVICE_NAME);
    }

    if let Err(err) = run_systemctl(["disable", SERVICE_NAME]) {
        warn!("Failed to disable service {SERVICE_NAME}: {err:#}");
    } else {
        info!("Disabled systemd --user service '{}'.", SERVICE_NAME);
    }

    fs::remove_file(&unit_path).with_context(|| {
        format!(
            "Failed to remove systemd user unit file {}",
            unit_path.display()
        )
    })?;
    info!("Removed systemd user unit at '{}'.", unit_path.display());

    run_systemctl(["daemon-reload"])
        .with_context(|| "Failed to reload systemd --user after removing service unit")?;
    info!("Reloaded systemd --user after removing '{}'.", SERVICE_NAME);

    Ok(())
}

/// Configures or removes Linux user-session startup.
pub fn configure_startup(args: &CollectorCli) -> Result<()> {
    let current_exe = std::env::current_exe()
        .with_context(|| "Could not determine the filesystem path of the application")?;
    let working_dir = current_exe
        .parent()
        .with_context(|| "Current executable path did not have a parent directory")?;

    if args.enable_startup {
        let startup_mode = prompt_startup_mode()?;
        info!(
            "Enabling Linux startup in {:?} mode for executable '{}'.",
            startup_mode,
            current_exe.display()
        );
        debug!(
            "Removing any existing Vigil startup artifacts before creating the new startup entry."
        );
        remove_xdg_autostart()?;
        remove_systemd_startup()?;

        match startup_mode {
            LinuxStartupMode::Xdg => {
                let entry_path = write_xdg_autostart(&current_exe, working_dir)?;
                info!(
                    "Created XDG autostart desktop entry at '{}'.",
                    entry_path.display()
                );
                info!(
                    "Startup was configured successfully. Vigil will auto-start on the next graphical login."
                );
                info!(
                    "This command does not launch a second Vigil instance immediately. If you want to keep collecting now, run Vigil again without the startup flags."
                );

                for warning in XdgAutostartProbe::gather().warnings() {
                    warn!("{warning}");
                }
            }
            LinuxStartupMode::Systemd => {
                let unit_path = write_systemd_startup(&current_exe, working_dir)?;
                info!(
                    "Enabled systemd --user service '{}', unit file can be found at '{}'.",
                    SERVICE_NAME,
                    unit_path.display()
                );
                info!(
                    "Startup was configured successfully. Vigil will auto-start on future graphical logins when your systemd user session reaches the graphical session target."
                );
                info!(
                    "The startup unit was not started immediately. This avoids racing the current Vigil instance against its single-instance lock."
                );
                info!(
                    "If you want to keep collecting now, run Vigil again without the startup flags. If you want to test the systemd startup unit itself, stop the current Vigil process first and then run: systemctl --user start {}",
                    SERVICE_NAME
                );

                match SystemdEnvironmentProbe::gather() {
                    std::result::Result::Ok(probe) => {
                        if let Some(warning) = probe.warning() {
                            warn!("{warning}");
                        }
                    }
                    std::result::Result::Err(err) => warn!(
                        "Could not inspect the systemd --user manager environment after enabling startup: {err:#}"
                    ),
                }
            }
        }

        if looks_like_repo_build_output(&current_exe) {
            warn!(
                "Startup was enabled from a repository build output at '{}'. This works, but it is fragile if you clean, rebuild, or move the repository. Prefer enabling startup from a stable installed binary such as '~/.cargo/bin/vigil'.",
                current_exe.display()
            );
        }

        warn!(
            "Startup is now enabled. If the executable path changes, re-run '--enable-startup' so the startup entry points to the new binary."
        );
    }

    if args.disable_startup {
        info!("Disabling all Vigil Linux startup artifacts for the current user.");
        remove_xdg_autostart()?;
        remove_systemd_startup()?;
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

    /// Verifies that Wayland is preferred when both Wayland and X11-related variables
    /// are present, which is common in Wayland sessions with Xwayland compatibility.
    #[test]
    fn detect_display_server_prefers_wayland_when_both_are_present() {
        assert_eq!(
            detect_display_server_from_values(Some("wayland-1"), None, Some("wayland"), Some(":0"),),
            DisplayServer::Wayland
        );
    }

    /// Verifies that plain X11 sessions still classify as X11 when no Wayland indicators
    /// are available.
    #[test]
    fn detect_display_server_recognizes_x11_sessions() {
        assert_eq!(
            detect_display_server_from_values(None, None, Some("x11"), Some(":0")),
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
        std::env::set_var("DISPLAY", ":0");

        assert_eq!(detect_display_server(), DisplayServer::Wayland);
    }

    /// Verifies that the systemd fallback unit is tied to the graphical session and
    /// keeps service startup free of hardcoded graphical-session environment values.
    #[test]
    fn render_service_unit_includes_graphical_session_binding_without_env_snapshot() {
        let unit = render_service_unit(Path::new("/tmp/vigil"), Path::new("/tmp"));

        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("After=graphical-session.target"));
        assert!(unit.contains("PartOf=graphical-session.target"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
        assert!(!unit.contains("Environment="));
        assert!(unit.contains("WorkingDirectory=/tmp"));
        assert!(unit.contains("ExecStart=/tmp/vigil collector"));
    }

    /// Verifies that the generated desktop entry uses XDG autostart conventions and
    /// points to the current executable and working directory explicitly.
    #[test]
    fn render_desktop_entry_uses_absolute_exec_and_working_directory() {
        let entry = render_desktop_entry(
            Path::new("/tmp/with spaces/vigil"),
            Path::new("/tmp/with spaces"),
        );

        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("Name=Vigil"));
        assert!(entry.contains("Exec=\"/tmp/with spaces/vigil\" collector"));
        assert!(entry.contains("TryExec=/tmp/with spaces/vigil"));
        assert!(entry.contains("Path=/tmp/with spaces"));
        assert!(entry.contains("Terminal=false"));
    }

    /// Verifies that path directives use systemd-style escaping instead of shell quotes,
    /// because directives like `WorkingDirectory=` reject quoted absolute paths.
    #[test]
    fn systemd_path_escape_escapes_whitespace_without_adding_quotes() {
        assert_eq!(
            systemd_path_escape("/tmp/with spaces/vigil"),
            "/tmp/with\\ spaces/vigil"
        );
        assert_eq!(systemd_path_escape("/tmp/plain"), "/tmp/plain");
    }

    /// Verifies that desktop-entry Exec escaping quotes reserved characters without
    /// turning simple paths into noisier quoted strings than necessary.
    #[test]
    fn desktop_exec_escape_quotes_only_when_required() {
        assert_eq!(desktop_exec_escape("/tmp/plain"), "/tmp/plain");
        assert_eq!(
            desktop_exec_escape("/tmp/with spaces/vigil"),
            "\"/tmp/with spaces/vigil\""
        );
        assert_eq!(
            desktop_exec_escape("/tmp/with$cash"),
            "\"/tmp/with\\$cash\""
        );
    }

    /// Verifies that the XDG startup probe warns only for ambiguous or non-graphical
    /// session signals so startup can stay quiet on ordinary desktop sessions.
    #[test]
    fn xdg_autostart_probe_warns_when_session_shape_is_ambiguous() {
        let healthy = XdgAutostartProbe {
            current_desktop: Some("KDE".to_string()),
            desktop_session: Some("plasma".to_string()),
            session_type: Some("wayland".to_string()),
            display_server: DisplayServer::Wayland,
        };
        assert!(healthy.warnings().is_empty());

        let ambiguous = XdgAutostartProbe {
            current_desktop: None,
            desktop_session: None,
            session_type: None,
            display_server: DisplayServer::Unknown,
        };
        assert_eq!(ambiguous.warnings().len(), 3);
    }

    /// Verifies that the systemd fallback warning is based on missing manager-side
    /// graphical environment keys rather than on OS-specific assumptions.
    #[test]
    fn systemd_environment_probe_warns_when_manager_is_missing_graphical_keys() {
        let probe = SystemdEnvironmentProbe {
            missing_keys: vec!["WAYLAND_DISPLAY".to_string(), "XDG_RUNTIME_DIR".to_string()],
        };
        let warning = probe.warning().expect("warning should be present");

        assert!(warning.contains("WAYLAND_DISPLAY"));
        assert!(warning.contains("XDG_RUNTIME_DIR"));
    }

    /// Verifies that repo-local `target/debug` and `target/release` binaries are treated as
    /// fragile startup targets so the caller can warn the user about unstable executable paths.
    #[test]
    fn looks_like_repo_build_output_matches_target_profiles() {
        assert!(looks_like_repo_build_output(Path::new(
            "/home/me/project/target/debug/vigil"
        )));
        assert!(looks_like_repo_build_output(Path::new(
            "/home/me/project/target/release/vigil"
        )));
        assert!(!looks_like_repo_build_output(Path::new(
            "/home/me/.cargo/bin/vigil"
        )));
    }
}
