use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind, Result, Write};
use tracing::*;

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows::Win32::Storage::FileSystem::LockFile;

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

pub fn ensure_single_instance() -> Result<()> {
    let mut path = std::env::temp_dir();
    path.push("akame.lock");

    // if file exists and we can write to it: empty and write the pid
    // if file doesn't exist we create
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
    {
        Ok(lock_file) => {
            // Attempt to acquire an exclusive lock
            if acquire_lock(&lock_file).is_ok() {
                if let Err(e) = write!(&lock_file, "{}", std::process::id()) {
                    error!(
                        "Lock file acquired but failed to write PID to the file: {}",
                        e
                    );
                }
                // There is no point in returning the file here
                // Ok(Some(lock_file))
                Ok(())
            } else {
                Err(Error::new(
                    ErrorKind::AlreadyExists,
                    "Another instance of the application is already running",
                ))
            }
        }
        Err(e) => {
            error!("Failed to open lock file: {}", e);
            Err(e)
        }
    }
}

#[cfg(windows)]
fn aquire_lock(file: &File) -> Result<()> {
    pub fn ensure_single_instance() -> Result<Option<File>> {
        let mut path = std::env::temp_dir();
        path.push("akame.lock");

        // if file exists and we can write to it: empty and write the pid
        // if file doesn't exist we create
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
        {
            Ok(lock_file) => {
                // Attempt to acquire an exclusive lock
                if acquire_lock(&lock_file).is_ok() {
                    if let Err(e) = writeln!(&lock_file, "{}", std::process::id()) {
                        error!("Failed to write PID to the file: {}", e);
                    }

                    Ok(Some(lock_file))
                } else {
                    Err(Error::new(
                        ErrorKind::AlreadyExists,
                        "Another instance of the application is already running",
                    ))
                }
            }
            Err(e) => {
                error!("Failed to open lock file: {}", e);
                Err(e)
            }
        }
    }
}

#[cfg(windows)]
fn acquire_lock(file: &File) -> Result<()> {
    unsafe {
        // Lock the entire file
        let result = LockFile(
            HANDLE(file.as_raw_handle() as isize), // File handle
            0,                                     // dwFileOffsetLow (start of the lock)
            0,                                     // dwFileOffsetHigh
            u32::MAX,                              // nNumberOfBytesToLockLow (entire range)
            u32::MAX,                              // nNumberOfBytesToLockHigh
        );

        if result.is_err() {
            return Err(Error::last_os_error());
        }
    }
    // If we didn't fail before we are ok :D
    Ok(())
}

#[cfg(unix)]
fn acquire_lock(file: &File) -> Result<()> {
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };

    if ret < 0 {
        if std::io::Error::last_os_error().kind() == ErrorKind::WouldBlock {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                "Another instance is already running",
            ));
        }
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}
