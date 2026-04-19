use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::*;

use crate::common::program_data_dir;

pub fn ensure_single_instance() -> Result<()> {
    if should_skip_instance_lock() {
        return Ok(());
    }

    let path = program_data_dir()
        .with_context(|| "Could not determine directory to store the lock file")?
        .join("life.lock");

    let mut lock_file = acquire_instance_lock(&path)
        .with_context(|| format!("Could not acquire lock in file: {}", path.display()))?;

    lock_file.set_len(0)?;
    write!(lock_file, "{}", process::id())
        .with_context(|| format!("Could not write pid to locked file: {}", path.display()))?;

    std::mem::forget(lock_file);

    Ok(())
}

fn should_skip_instance_lock() -> bool {
    std::env::var("LIFE_MONITOR_SKIP_INSTANCE_LOCK")
        .map(|value| value == "1")
        .unwrap_or(false)
}

pub fn db_operation_lock_path(db_path: &Path) -> PathBuf {
    let filename = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.oplock"))
        .unwrap_or_else(|| "life-monitor.db.oplock".to_string());

    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(filename)
}

pub struct OperationLockGuard {
    #[allow(dead_code)]
    file: File,
}

pub fn acquire_db_operation_lock(db_path: &Path) -> Result<OperationLockGuard> {
    let lock_path = db_operation_lock_path(db_path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for DB operation lock: {}",
                parent.display()
            )
        })?;
    }

    let file = acquire_blocking_lock(&lock_path).with_context(|| {
        format!(
            "Failed to acquire DB operation lock: {}",
            lock_path.display()
        )
    })?;

    Ok(OperationLockGuard { file })
}

#[cfg(target_os = "linux")]
fn acquire_instance_lock(lock_f_path: &PathBuf) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_f_path)?;

    let ret =
        unsafe { nix::libc::flock(file.as_raw_fd(), nix::libc::LOCK_EX | nix::libc::LOCK_NB) };
    if ret == -1 {
        let err = nix::errno::Errno::last();
        if err == nix::errno::Errno::EWOULDBLOCK {
            use std::io::Read;
            let mut buf = String::new();
            let _ = file.read_to_string(&mut buf);
            let pid = buf.trim();
            let pid = if pid.is_empty() { "N/A" } else { pid };
            anyhow::bail!("Another instance is already running with pid: {pid}, you shouldn't start more than one instance of life-monitor");
        }
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
fn acquire_blocking_lock(lock_f_path: &Path) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_f_path)?;

    let ret = unsafe { nix::libc::flock(file.as_raw_fd(), nix::libc::LOCK_EX) };
    if ret == -1 {
        let err = nix::errno::Errno::last();
        anyhow::bail!(
            "Blocking flock failed for '{}': {}",
            lock_f_path.display(),
            err
        );
    }
    Ok(file)
}

#[cfg(target_os = "windows")]
fn acquire_instance_lock(lock_f_path: &PathBuf) -> Result<File> {
    acquire_windows_lock(lock_f_path, false)
}

#[cfg(target_os = "windows")]
fn acquire_blocking_lock(lock_f_path: &Path) -> Result<File> {
    acquire_windows_lock(lock_f_path, true)
}

#[cfg(target_os = "windows")]
fn acquire_windows_lock(lock_f_path: &Path, blocking: bool) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
    use windows::Win32::System::IO::OVERLAPPED;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_f_path)?;

    let handle = windows::Win32::Foundation::HANDLE(file.as_raw_handle());

    loop {
        let mut overlapped = OVERLAPPED::default();
        let result = unsafe {
            LockFileEx(
                handle,
                LOCKFILE_EXCLUSIVE_LOCK,
                Some(0),
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };

        match result {
            Ok(()) => return Ok(file),
            Err(err) if blocking => {
                let _ = err;
                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => {
                return Err(anyhow::Error::new(err)).with_context(|| {
                    format!("Failed to acquire lock for '{}'", lock_f_path.display())
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_operation_lock_path_uses_database_directory() {
        let path = Path::new("/tmp/life-monitor/shared.db");
        let lock_path = db_operation_lock_path(path);

        assert_eq!(
            lock_path,
            PathBuf::from("/tmp/life-monitor/shared.db.oplock")
        );
    }
}
