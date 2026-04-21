use std::env;
use std::io;
use std::path::PathBuf;

/// Returns a platform-specific path for storing program-related files and ensures the directory exists.
pub fn program_data_dir() -> io::Result<PathBuf> {
    if let Ok(path) = env::var("VIGIL_DATA_DIR") {
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        return Ok(path);
    }

    #[cfg(test)]
    {
        if env::var_os("VIGIL_DATA_DIR").is_none() {
            let path = std::env::temp_dir().join("vigil_test_data");
            std::fs::create_dir_all(&path)?;
            return Ok(path);
        }
    }

    let base_dir = if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA").map(PathBuf::from).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?
    } else if cfg!(target_os = "linux") {
        let home = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home);
        path.push(".local");
        path.push("share");
        path
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    let path = base_dir.join("vigil");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
