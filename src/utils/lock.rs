use std::fs::OpenOptions;
use std::io::{ErrorKind, Result, Write};
use std::process;

use tracing::*;

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows::Win32::Storage::FileSystem::LockFile;

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

use crate::common::program_data_dir;

pub fn ensure_single_instance() -> Result<()> {
    let mut path = program_data_dir()?;
    path.push("life.lock");

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;

    acquire_lock(&file)?;

    file.set_len(0)?; // truncate safely
    write!(file, "{}", process::id())?;

    std::mem::forget(file);

    Ok(())
}

#[cfg(target_os = "linux")]
fn acquire_lock(file: &std::fs::File) -> Result<()> {
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == ErrorKind::WouldBlock {
            return Err(std::io::Error::new(
                ErrorKind::AlreadyExists,
                "Another instance is running",
            ));
        }
        Err(err)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn acquire_lock(file: &std::fs::File) -> Result<()> {
    let handle = file.as_raw_handle() as HANDLE;

    // Lock the entire file
    let result = unsafe { LockFile(handle, 0, 0, u32::MAX, u32::MAX) };

    if result == 0 {
        return Err(Error::last_os_error());
    }

    Ok(())
}
