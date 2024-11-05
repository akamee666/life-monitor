#![windows_subsystem = "windows"]
// used to close the terminal and create a gui with no window in Windows.

use life_monitor::api::ApiConfig;
use life_monitor::args::Cli;
use life_monitor::data::*;
use life_monitor::localdb::{clean_database, open_con};
use life_monitor::logger;

use clap::Parser;

use reqwest::Client;

use tokio::task::JoinSet;

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use tracing::*;

#[tokio::main]
async fn main() {
    let mut args = Cli::parse();
    logger::init(args.debug);
    args.print_args();
    check_startup().unwrap();

    if args.enable_startup || args.disable_startup {
        match configure_startup(args.enable_startup) {
            Ok(_) => {
                info!(
                    "Startup configuration {} successfully, the program will end now. Start it again without the start up flag to run normally.",
                    if args.enable_startup {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
                return;
            }
            Err(e) => {
                error!("Failed to configure startup: {}", e);
                panic!();
            }
        }
    }

    if args.debug && args.interval.is_none() {
        info!("Debug is true but no interval value provided, using default five seconds!");
        args.interval = 5.into();
    }

    let use_api: Option<ApiConfig> = if let Some(ref config_path) = args.api {
        info!("Config file name: {:?}", config_path);

        let cfg = ApiConfig::from_file(config_path).unwrap_or_else(|err| {
            error!("Could not parse {config_path}. Error: {err}");
            panic!()
        });

        Some(cfg)
    } else {
        if args.clear {
            info!("Clean argument provided, cleaning database!");

            match clean_database() {
                Ok(_) => {}
                Err(e) => {
                    error!("Could not delete database, because of error: {e}. Most likely the database does not exist already, no need to crash.");
                }
            }
        };

        None
    };

    let storage_backend: StorageBackend = if use_api.is_some() {
        let api_config = use_api.unwrap();

        StorageBackend::Api(ApiStore::new(Client::new(), api_config))
    } else {
        let con = open_con().unwrap_or_else(|err| {
            error!("Could not open a connection with local database, quitting!\n Err: {err}",);
            panic!();
        });

        StorageBackend::Local(LocalDbStore::new(con))
    };

    run(args, storage_backend).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::linux::process;

    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
    }

    // Need to wait the tasks finish, which they should not if there is no error.
    // Without wait for them, run function will finish and all values will be droped, finishing the
    // entire program.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => {
                error!("A task has panicked: {}", e);
                panic!()
            }
        }
    }
}

#[cfg(target_os = "windows")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::win::process;
    use life_monitor::win::systray;

    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
    }

    if !args.no_systray {
        set.spawn(systray::init());
    }

    // Need to wait the tasks finish, which they should not if there is no error.
    // Without wait for them, run function will finish and all values will be droped, finishing the
    // entire program.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => {
                error!("A task has panicked: {}", e);
                panic!()
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn configure_startup(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;

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

    if enable {
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

#[cfg(target_os = "linux")]
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
        "Startup status on Linux: {}",
        if is_enabled { "Enabled" } else { "Disabled" }
    );

    Ok(is_enabled)
}

pub fn check_startup() -> Result<bool, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        check_startup_status()
    }

    #[cfg(target_os = "windows")]
    {
        check_startup_status()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        error!("Startup check not implemented for this operating system");
        Ok(false)
    }
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "linux")]
pub fn configure_startup(enable: bool) -> Result<(), Box<dyn std::error::Error>> {
    let service_name = "life-monitor.service";
    // Change to user service directory instead of system
    let user_service_dir = Path::new(&env::var("HOME")?).join(".config/systemd/user");
    fs::create_dir_all(&user_service_dir)?;
    let service_path = user_service_dir.join(service_name);
    let current_exe = env::current_exe()?;

    if enable {
        // Create systemd user service file
        let service_content = format!(
            "[Unit]\n\
            Description=Life Monitor Service\n\
            After=graphical-session.target\n\
            PartOf=graphical-session.target\n\
            \n\
            [Service]\n\
            Type=simple\n\
            Environment=DISPLAY=:0\n\
            Environment=XAUTHORITY=/home/{}/.Xauthority\n\
            ExecStart={}\n\
            Restart=always\n\
            \n\
            [Install]\n\
            WantedBy=graphical-session.target\n",
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
