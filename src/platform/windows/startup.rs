use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;
use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

use crate::Cli;

const SHORTCUT_NAME: &str = "life-monitor.lnk";

#[allow(dead_code)]
pub fn check_startup_status() -> Result<bool> {
    let ctx = startup_context()?;
    let manager = WindowsShortcutManager;
    let enabled = manager.shortcut_exists(&ctx.shortcut_path());

    info!(
        "Startup status on Windows is {}.",
        if enabled { "Enabled" } else { "Disabled" }
    );

    Ok(enabled)
}

pub fn configure_startup(args: &Cli) -> Result<()> {
    let ctx = startup_context()?;
    let manager = WindowsShortcutManager;
    configure_startup_with(args, &ctx, &manager)
}

fn configure_startup_with(
    args: &Cli,
    ctx: &StartupContext,
    manager: &dyn ShortcutManager,
) -> Result<()> {
    let shortcut_path = ctx.shortcut_path();

    if args.enable_startup {
        manager.create_shortcut(&shortcut_path, &ctx.current_exe)?;
        info!("Created startup shortcut at '{}'.", shortcut_path.display());
    } else if args.disable_startup && manager.shortcut_exists(&shortcut_path) {
        manager.remove_shortcut(&shortcut_path)?;
        info!("Removed startup shortcut at '{}'.", shortcut_path.display());
    }

    Ok(())
}

fn startup_context() -> Result<StartupContext> {
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| anyhow!("Could not find APPDATA environment variable"))?;
    let startup_dir = startup_dir_from_appdata(Path::new(&appdata));
    let current_exe = std::env::current_exe()
        .with_context(|| "Could not determine the filesystem path of the application")?;

    Ok(StartupContext {
        startup_dir,
        current_exe,
    })
}

fn startup_dir_from_appdata(appdata: &Path) -> PathBuf {
    appdata
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupContext {
    startup_dir: PathBuf,
    current_exe: PathBuf,
}

impl StartupContext {
    fn shortcut_path(&self) -> PathBuf {
        self.startup_dir.join(SHORTCUT_NAME)
    }
}

trait ShortcutManager {
    fn shortcut_exists(&self, shortcut_path: &Path) -> bool;
    fn create_shortcut(&self, shortcut_path: &Path, target: &Path) -> Result<()>;
    fn remove_shortcut(&self, shortcut_path: &Path) -> Result<()>;
}

struct WindowsShortcutManager;

impl ShortcutManager for WindowsShortcutManager {
    fn shortcut_exists(&self, shortcut_path: &Path) -> bool {
        shortcut_path.exists()
    }

    fn create_shortcut(&self, shortcut_path: &Path, target: &Path) -> Result<()> {
        if let Some(parent) = shortcut_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create the Windows Startup folder at '{}'",
                    parent.display()
                )
            })?;
        }

        let _apartment = ComApartment::new()?;
        unsafe {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            shell_link.SetPath(&HSTRING::from(target.to_string_lossy().as_ref()))?;
            shell_link.SetWorkingDirectory(&HSTRING::from(
                target
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .as_ref(),
            ))?;

            let persist_file: IPersistFile = shell_link.cast()?;
            persist_file.Save(
                &HSTRING::from(shortcut_path.to_string_lossy().as_ref()),
                true,
            )?;
        }

        Ok(())
    }

    fn remove_shortcut(&self, shortcut_path: &Path) -> Result<()> {
        fs::remove_file(shortcut_path).with_context(|| {
            format!(
                "Failed to remove the Windows Startup shortcut '{}'",
                shortcut_path.display()
            )
        })
    }
}

struct ComApartment;

impl ComApartment {
    fn new() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .with_context(|| "Failed to initialize COM for Startup shortcut handling")?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn base_cli() -> Cli {
        Cli {
            #[cfg(feature = "multi-sync")]
            command: None,
            interval: None,
            #[cfg(target_os = "windows")]
            no_systray: true,
            debug: false,
            db_path: None,
            export_db: None,
            import_db: None,
            dry_run: false,
            import_notes: None,
            report: None,
            report_days: 7,
            tui: false,
            tui_ascii: false,
            dpi: None,
            clear: false,
            enable_startup: false,
            disable_startup: false,
            #[cfg(feature = "multi-sync")]
            sync_enable: false,
            #[cfg(feature = "multi-sync")]
            sync_remote_url: None,
            #[cfg(feature = "multi-sync")]
            sync_auth_token: None,
            #[cfg(feature = "multi-sync")]
            sync_interval: 300,
        }
    }

    #[derive(Default)]
    struct FakeShortcutManager {
        created: RefCell<Vec<(PathBuf, PathBuf)>>,
        removed: RefCell<Vec<PathBuf>>,
        existing: RefCell<Vec<PathBuf>>,
    }

    impl ShortcutManager for FakeShortcutManager {
        fn shortcut_exists(&self, shortcut_path: &Path) -> bool {
            self.existing
                .borrow()
                .iter()
                .any(|path| path == shortcut_path)
        }

        fn create_shortcut(&self, shortcut_path: &Path, target: &Path) -> Result<()> {
            self.created
                .borrow_mut()
                .push((shortcut_path.to_path_buf(), target.to_path_buf()));
            self.existing.borrow_mut().push(shortcut_path.to_path_buf());
            Ok(())
        }

        fn remove_shortcut(&self, shortcut_path: &Path) -> Result<()> {
            self.removed.borrow_mut().push(shortcut_path.to_path_buf());
            self.existing
                .borrow_mut()
                .retain(|path| path != shortcut_path);
            Ok(())
        }
    }

    /// Verifies that enabling startup targets the current user's Startup folder by driving the
    /// pure helper with a fake shortcut manager instead of calling real Windows COM APIs.
    #[test]
    fn configure_startup_creates_shortcut_in_startup_folder() {
        let mut cli = base_cli();
        cli.enable_startup = true;
        let ctx = StartupContext {
            startup_dir: PathBuf::from(
                r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
            ),
            current_exe: PathBuf::from(r"C:\tools\life-monitor.exe"),
        };
        let manager = FakeShortcutManager::default();

        configure_startup_with(&cli, &ctx, &manager).unwrap();

        let created = manager.created.borrow();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].0, ctx.shortcut_path());
        assert_eq!(created[0].1, ctx.current_exe);
    }

    /// Verifies that disabling startup removes an existing shortcut only when one is present
    /// by preloading fake manager state and checking the requested removal path.
    #[test]
    fn configure_startup_removes_existing_shortcut_when_disabled() {
        let mut cli = base_cli();
        cli.disable_startup = true;
        let ctx = StartupContext {
            startup_dir: PathBuf::from(
                r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
            ),
            current_exe: PathBuf::from(r"C:\tools\life-monitor.exe"),
        };
        let manager = FakeShortcutManager::default();
        manager.existing.borrow_mut().push(ctx.shortcut_path());

        configure_startup_with(&cli, &ctx, &manager).unwrap();

        assert_eq!(manager.removed.borrow().as_slice(), &[ctx.shortcut_path()]);
    }

    /// Verifies that disabling startup is a no-op when no shortcut exists by using the same
    /// helper path with an empty fake manager and asserting nothing was removed.
    #[test]
    fn configure_startup_does_not_remove_missing_shortcut() {
        let mut cli = base_cli();
        cli.disable_startup = true;
        let ctx = StartupContext {
            startup_dir: PathBuf::from(
                r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
            ),
            current_exe: PathBuf::from(r"C:\tools\life-monitor.exe"),
        };
        let manager = FakeShortcutManager::default();

        configure_startup_with(&cli, &ctx, &manager).unwrap();

        assert!(manager.removed.borrow().is_empty());
    }
}
