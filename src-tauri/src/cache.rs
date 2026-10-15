//! Small JSON files remembering what was slow to find out (PowerShell queries, icons, sizes), so a
//! page can show the last answer at once and refresh it quietly.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the app's data folder.
pub fn init(data_dir: PathBuf) {
    let _ = DIR.set(data_dir.join("cache"));
}

fn file(name: &str) -> Option<PathBuf> {
    DIR.get().map(|d| d.join(name))
}

/// The cached value, or None when there is none yet (or it no longer parses).
pub fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
    let bytes = std::fs::read(file(name)?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save<T: Serialize>(name: &str, value: &T) {
    let Some(path) = file(name) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written aside and renamed, so a crash mid-write never leaves half a file to be read.
    let tmp = path.with_extension("tmp");
    if let Ok(bytes) = serde_json::to_vec(value) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn now() -> u64 {
    crate::util::unix_secs(std::time::SystemTime::now())
}
