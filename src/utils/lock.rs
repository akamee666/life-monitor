use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use anyhow::*;

use crate::common::program_data_dir;

pub fn ensure_single_instance() -> Result<()> {
    let path = program_data_dir()
        .with_context(|| "Could not determine directory to store the lock file")?
        .join("life.lock");

    let mut lock_file = acquire_lock(&path)
        .with_context(|| format!("Could not acquire lock in file: {}", path.display()))?;

    lock_file.set_len(0)?;
    write!(lock_file, "{}", process::id())
        .with_context(|| format!("Could not write pid to locked file: {}", path.display()))?;

    std::mem::forget(lock_file);

    Ok(())
}

#[cfg(target_os = "linux")]
fn acquire_lock(lock_f_path: &PathBuf) -> Result<File> {
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
        // I think this should be "handled" by the caller but it's ok in this case since we can
        // provide useful context? don't know aaaaaaa rust error handling is hard sometimes
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

#[cfg(target_os = "windows")]
fn acquire_lock(lock_f_path: &PathBuf) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Storage::FileSystem::LockFile;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_f_path)?;

    let handle = windows::Win32::Foundation::HANDLE(file.as_raw_handle());

    // Lock the entire file
    unsafe {
        LockFile(handle, 0, 0, u32::MAX, u32::MAX)
            .with_context(|| format!("Call to LockFile API failed for file: {file:?}"))?
    };

    Ok(file)
}
