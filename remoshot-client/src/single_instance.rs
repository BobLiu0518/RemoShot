use directories::ProjectDirs;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub struct SingleInstance {
    _file: File,
    lock_path: PathBuf,
}

impl SingleInstance {
    pub fn new(app_name: &str) -> Result<Self, String> {
        let lock_path = Self::get_lock_path(app_name)?;

        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create lock directory: {}", e))?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)
            .map_err(|e| format!("Failed to open lock file: {}", e))?;

        file.try_lock_exclusive().map_err(|e| {
            format!(
                "Failed to acquire lock (another instance is running): {}",
                e
            )
        })?;

        let mut file_clone = file
            .try_clone()
            .map_err(|e| format!("Failed to clone file handle: {}", e))?;
        file_clone
            .set_len(0)
            .map_err(|e| format!("Failed to truncate lock file: {}", e))?;
        writeln!(file_clone, "{}", std::process::id())
            .map_err(|e| format!("Failed to write PID: {}", e))?;

        Ok(Self {
            _file: file,
            lock_path,
        })
    }

    fn get_lock_path(app_name: &str) -> Result<PathBuf, String> {
        let project_dirs = ProjectDirs::from("tech", "bobliu", app_name)
            .ok_or_else(|| "Failed to determine project directories".to_string())?;

        let data_dir = project_dirs.data_local_dir();
        Ok(data_dir.join(".lock"))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}
