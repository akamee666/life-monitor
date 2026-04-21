use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use tracing::*;

use crate::common::{program_data_dir, DEFAULT_MOUSE_DPI};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseDpiSource {
    Cli,
    Remembered,
    AutoDetected,
    Prompted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseDpiConfig {
    pub dpi: u32,
    pub source: MouseDpiSource,
}

pub fn resolve_mouse_dpi(cli_dpi: Option<u32>) -> Result<MouseDpiConfig> {
    if let Some(dpi) = cli_dpi {
        store_mouse_dpi(dpi)?;
        return Ok(MouseDpiConfig {
            dpi,
            source: MouseDpiSource::Cli,
        });
    }

    if let Some(dpi) = load_mouse_dpi()? {
        return Ok(MouseDpiConfig {
            dpi,
            source: MouseDpiSource::Remembered,
        });
    }

    if let Some(dpi) = try_detect_mouse_dpi() {
        return Ok(MouseDpiConfig {
            dpi,
            source: MouseDpiSource::AutoDetected,
        });
    }

    prompt_and_store_mouse_dpi()
}

fn prompt_and_store_mouse_dpi() -> Result<MouseDpiConfig> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "Vigil could not determine a mouse DPI value automatically and no saved DPI is available.\n\
             Mouse DPI is needed to estimate real mouse travel in centimeters from raw input counts.\n\
             What you can do:\n\
             - run Vigil once with --dpi <VALUE>\n\
             - or run it in a terminal once and enter the DPI when prompted\n\
             - after that, Vigil will remember the value for future runs\n\
             - if you do not know your mouse DPI, start with {} and calibrate later",
            DEFAULT_MOUSE_DPI
        );
    }

    eprintln!(
        "Vigil could not determine your mouse DPI from the operating system.\n\
         Enter the DPI/CPI used by your mouse. This value will be remembered for later runs.\n\
         Example: {}",
        DEFAULT_MOUSE_DPI
    );

    loop {
        eprint!("Mouse DPI/CPI: ");
        io::stderr().flush()?;

        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .with_context(|| "Failed to read mouse DPI from stdin")?;

        match parse_mouse_dpi(line.trim()) {
            Some(dpi) => {
                store_mouse_dpi(dpi)?;
                return Ok(MouseDpiConfig {
                    dpi,
                    source: MouseDpiSource::Prompted,
                });
            }
            None => {
                eprintln!("Please enter a positive integer such as 800, 1200, or 1600.");
            }
        }
    }
}

fn mouse_dpi_file() -> Result<PathBuf> {
    Ok(program_data_dir()
        .with_context(|| "Could not determine the application data directory for DPI memory")?
        .join("mouse-dpi.txt"))
}

fn load_mouse_dpi() -> Result<Option<u32>> {
    let path = mouse_dpi_file()?;
    match fs::read_to_string(&path) {
        Ok(contents) => Ok(parse_mouse_dpi(contents.trim())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to read remembered mouse DPI from '{}'",
                path.display()
            )
        }),
    }
}

fn store_mouse_dpi(dpi: u32) -> Result<()> {
    let path = mouse_dpi_file()?;
    fs::write(&path, dpi.to_string()).with_context(|| {
        format!(
            "Failed to store remembered mouse DPI in '{}'",
            path.display()
        )
    })
}

fn parse_mouse_dpi(value: &str) -> Option<u32> {
    let dpi = value.parse::<u32>().ok()?;
    if dpi == 0 {
        None
    } else {
        Some(dpi)
    }
}

#[cfg(target_os = "linux")]
fn try_detect_mouse_dpi() -> Option<u32> {
    debug!(
        "Mouse DPI auto-detection is unavailable on Linux for generic evdev/libinput setups. \
         The OS exposes raw motion counts, but not a reliable cross-desktop mouse CPI value."
    );
    None
}

#[cfg(target_os = "windows")]
fn try_detect_mouse_dpi() -> Option<u32> {
    debug!(
        "Mouse DPI auto-detection is unavailable on Windows for generic Raw Input devices. \
         Windows exposes raw motion counts, but not a reliable per-device CPI value."
    );
    None
}

pub fn log_mouse_dpi_resolution(config: MouseDpiConfig) {
    match config.source {
        MouseDpiSource::Cli => {
            info!(
                "Mouse DPI set to {} from --dpi and remembered for future runs.",
                config.dpi
            );
        }
        MouseDpiSource::Remembered => {
            info!(
                "Using remembered mouse DPI {} from the previous run.",
                config.dpi
            );
        }
        MouseDpiSource::AutoDetected => {
            info!("Using automatically detected mouse DPI {}.", config.dpi);
        }
        MouseDpiSource::Prompted => {
            warn!(
                "Mouse DPI could not be detected automatically. Using prompted value {} and remembering it for future runs.",
                config.dpi
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that DPI parsing accepts only positive integers by exercising invalid text,
    /// zero, and a valid value without touching any filesystem or OS-specific resolution path.
    #[test]
    fn parse_mouse_dpi_rejects_zero_and_invalid_values() {
        assert_eq!(parse_mouse_dpi("0"), None);
        assert_eq!(parse_mouse_dpi("abc"), None);
        assert_eq!(parse_mouse_dpi("800"), Some(800));
    }
}
