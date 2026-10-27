//! The saved download list: `downloads.json` in DownloadCraft's data folder, written to a
//! temporary file and renamed into place, so a crash mid-write never leaves half a list.

use std::path::PathBuf;

use serde_json::Value;

/// The most bytes the saved list is read up to.
const MAX_SAVED: u64 = 256 * 1024 * 1024;

pub struct Store {
    dir: PathBuf,
}

/// The file beside the exe that makes a copy portable (packaging/windows/portable.txt).
pub const PORTABLE_MARKER: &str = "portable.txt";

/// `DownloadCraftData` beside the exe for a portable copy; else `%APPDATA%\DownloadCraft`,
/// `~/Library/Application Support/DownloadCraft`, `~/.local/share/DownloadCraft`.
pub fn default_dir() -> Option<PathBuf> {
    portable_dir().or_else(|| dirs::data_dir().map(|d| d.join("DownloadCraft")))
}

/// The portable data folder, when `portable.txt` sits beside the exe and the folder can be made.
fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if !dir.join(PORTABLE_MARKER).is_file() {
        return None;
    }
    let data = dir.join("DownloadCraftData");
    match std::fs::create_dir_all(&data) {
        Ok(()) => Some(data),
        Err(e) => {
            log::warn!("portable: {} can't be written ({e}); using the per-user folder", data.display());
            None
        }
    }
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store { dir }
    }
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
    fn path(&self) -> PathBuf {
        self.dir.join("downloads.json")
    }
    /// The saved list; `None` on a first run.
    pub fn load(&self) -> Result<Option<Value>, String> {
        let path = self.path();
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(m) if m.len() > MAX_SAVED => {
                self.set_aside();
                return Err("the saved list is too big; it was set aside".into());
            }
            Ok(_) => {}
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                self.set_aside();
                Err(format!("it was damaged ({e}) and was kept as downloads.json.bad"))
            }
        }
    }
    /// Keep a damaged list as `downloads.json.bad` and start afresh.
    pub fn set_aside(&self) {
        let _ = std::fs::rename(self.path(), self.dir.join("downloads.json.bad"));
    }
    pub fn save(&self, v: &Value) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join("downloads.json.tmp");
        let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn saves_loads_and_sets_damage_aside() {
        let dir = std::env::temp_dir().join(format!("downloadcraft-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = Store::new(dir.clone());
        assert_eq!(s.load(), Ok(None));
        s.save(&serde_json::json!({"downloads": []})).unwrap();
        assert_eq!(s.load(), Ok(Some(serde_json::json!({"downloads": []}))));
        std::fs::write(dir.join("downloads.json"), b"{ not json").unwrap();
        assert!(s.load().is_err());
        assert!(dir.join("downloads.json.bad").exists());
        assert_eq!(s.load(), Ok(None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
