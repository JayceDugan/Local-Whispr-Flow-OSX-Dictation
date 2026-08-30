use std::path::PathBuf;

use parking_lot::Mutex;

use crate::config::AppConfig;

/// Managed state: current config + where it persists.
/// Mutex because `tauri::State` hands out shared references; commands swap the
/// config when settings change and the hotkey/health tasks read it per-event.
pub struct ConfigHolder {
    pub path: PathBuf,
    config: Mutex<AppConfig>,
}

impl ConfigHolder {
    pub fn new(path: PathBuf) -> Self {
        let config = AppConfig::load(&path);
        Self {
            path,
            config: Mutex::new(config),
        }
    }

    pub fn get(&self) -> AppConfig {
        self.config.lock().clone()
    }

    pub fn set(&self, next: AppConfig) {
        *self.config.lock() = next;
    }
}
