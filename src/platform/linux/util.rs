use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use x11rb::connection::Connection;
use x11rb::protocol::screensaver;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

use tracing::*;

// https://www.reddit.com/r/rust/comments/f7yrle/get_information_about_current_w_xorg/
// Returns name, instance and class of the focused window in that order.
pub fn get_focused_window() -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // Set up our state
    let (conn, screen) = x11rb::connect(None).expect("Failed to connect");
    let root = conn.setup().roots[screen].root;
    let net_active_window = get_or_intern_atom(&conn, b"_NET_ACTIVE_WINDOW");
    let net_wm_name = get_or_intern_atom(&conn, b"_NET_WM_NAME");
    let utf8_string = get_or_intern_atom(&conn, b"UTF8_STRING");

    let focus = match find_active_window(&conn, root, net_active_window) {
        None => {
            return Err("No active window".into());
        }
        Some(x) => x,
    };

    // Collect the replies to the atoms
    let (net_wm_name, utf8_string) = (net_wm_name, utf8_string);
    let (wm_class, string): (Atom, Atom) = (AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());

    // Get the property from the window that we need
    let name = conn.get_property(false, focus, net_wm_name, utf8_string, 0, u32::MAX)?;
    let class = conn.get_property(false, focus, wm_class, string, 0, u32::MAX)?;
    let (name, class) = (name.reply()?, class.reply()?);

    let (instance, class) = parse_wm_class(&class);
    let name = parse_string_property(&name).to_string();
    let instance = instance.to_string();
    let class = class.to_string();

    Ok((name, instance, class))
}

pub fn get_screen_dpi() -> Result<f64, Box<dyn std::error::Error>> {
    // Open connection to the X server
    let (conn, screen_num) = RustConnection::connect(None)?;

    // Get the setup and screen information
    let setup = conn.setup();
    let screen = &setup.roots[screen_num];

    // Get screen dimensions in pixels and millimeters
    let width_px = screen.width_in_pixels as f64;
    let height_px = screen.height_in_pixels as f64;

    let width_mm = screen.width_in_millimeters as f64;
    let height_mm = screen.height_in_millimeters as f64;

    // Calculate DPI
    let dpi_x = (width_px / width_mm) * 25.4; // 25.4 mm in an inch
    let dpi_y = (height_px / height_mm) * 25.4;

    // Use the average DPI of the x and y dimensions
    let average_dpi = (dpi_x + dpi_y) / 2.0;

    Ok(average_dpi)
}

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
    pub fn noacc_default() -> Self {
        MouseSettings {
            acceleration_numerator: 1,
            acceleration_denominator: 1,
            threshold: 0,
            dpi: 800,
        }
    }
}

pub fn get_mouse_settings() -> Result<MouseSettings, Box<dyn std::error::Error>> {
    // Open connection to the X server
    let (conn, _) = RustConnection::connect(None)?;

    // Get the mouse acceleration settings
    let pointer_control = conn.get_pointer_control()?.reply()?;

    // The values are:
    // - `acceleration_numerator`: Numerator for pointer acceleration
    // - `acceleration_denominator`: Denominator for pointer acceleration
    // - `threshold`: The threshold before acceleration applies
    // These values are set to 1,1,0 respectively if no mouse acceleration is active, which will
    // not changes the results if used later.
    let acceleration_numerator = pointer_control.acceleration_numerator;
    let acceleration_denominator = pointer_control.acceleration_denominator;
    let threshold = pointer_control.threshold;

    let s: MouseSettings = MouseSettings {
        acceleration_numerator,
        acceleration_denominator,
        threshold,
        ..Default::default()
    };
    Ok(s)
}

// TODO:
// Add flags from somewhere i dont know where yet
pub fn configure_startup(
    should_enable: bool,
    is_enable: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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

        // Write service file
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&service_path)?;
        file.write_all(service_content.as_bytes())?;

        // Enable and start the user service
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
        // Disable and stop the user service
        Command::new("systemctl")
            .args(["--user", "stop", service_name])
            .status()?;
        Command::new("systemctl")
            .args(["--user", "disable", service_name])
            .status()?;

        // Remove service file if it exists
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

pub fn get_idle_time() -> Result<Duration, Box<dyn std::error::Error>> {
    // Connect to the X server
    let (conn, screen_num) = RustConnection::connect(None)?;
    // Get the root window of the default screen
    let screen = &conn.setup().roots[screen_num];

    // Query the XScreenSaver extension for idle time
    let reply = screensaver::query_info(&conn, screen.root)?.reply()?;

    // Extract idle time in milliseconds
    let idle_time_ms = reply.ms_since_user_input;

    // Convert to `Duration`
    Ok(Duration::from_millis(idle_time_ms as u64))
}

fn get_or_intern_atom(conn: &RustConnection, name: &[u8]) -> Atom {
    let result = conn
        .intern_atom(false, name)
        .expect("Failed to intern atom")
        .reply()
        .expect("Failed receive interned atom");

    result.atom
}

fn find_active_window(
    conn: &impl Connection,
    root: Window,
    net_active_window: Atom,
) -> Option<Window> {
    let window: Atom = AtomEnum::WINDOW.into();
    let active_window = conn
        .get_property(false, root, net_active_window, window, 0, 1)
        .expect("Failed to get X11 property")
        .reply()
        .expect("Failed to receive X11 property reply");

    if active_window.format == 32 && active_window.length == 1 {
        active_window
            .value32()
            .expect("Invalid message. Expected value with format = 32")
            .next()
    } else {
        // Query the input focus
        Some(
            conn.get_input_focus()
                .expect("Failed to get input focus")
                .reply()
                .expect("Failed to receive X11 input focus")
                .focus,
        )
    }
}

fn parse_string_property(property: &GetPropertyReply) -> &str {
    std::str::from_utf8(&property.value).unwrap_or("Invalid utf8")
}

fn parse_wm_class(property: &GetPropertyReply) -> (&str, &str) {
    if property.format != 8 {
        return (
            "Malformed property: wrong format",
            "Malformed property: wrong format",
        );
    }
    let value = &property.value;
    // The property should contain two null-terminated strings. Find them.
    if let Some(middle) = value.iter().position(|&b| b == 0) {
        let (instance, class) = value.split_at(middle);
        // Skip the null byte at the beginning
        let mut class = &class[1..];
        // Remove the last null byte from the class, if it is there.
        if class.last() == Some(&0) {
            class = &class[..class.len() - 1];
        }
        let instance = std::str::from_utf8(instance);
        let class = std::str::from_utf8(class);
        (
            instance.unwrap_or("Invalid utf8"),
            class.unwrap_or("Invalid utf8"),
        )
    } else {
        ("Missing null byte", "Missing null byte")
    }
}
