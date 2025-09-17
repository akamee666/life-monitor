//! This file is responsible to store functions, enums or
//! structs that can be used for all platforms supported.
use anyhow::*;
use std::fs;

#[cfg(target_os = "windows")]
use crate::platform::windows::common::*;

#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::GetTickCount64;

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "windows")]
fn uptime() -> u64 {
    unsafe { GetTickCount64() / 1_000 }
}
