use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "http://devbox:8090";
pub const DEFAULT_HOTKEY: &str = "Cmd+Shift+Space";

/// Non-secret app config persisted as JSON in the app config dir.
/// The API token deliberately does NOT live here — see keychain.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub base_url: String,
    pub hotkey: String,
    /// Send `cleanup=true` so the server returns de-filler text for injection.
    pub cleanup: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            hotkey: DEFAULT_HOTKEY.to_string(),
            cleanup: true,
        }
    }
}

impl AppConfig {
    pub fn load(path: &PathBuf) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &PathBuf) -> Result<(), crate::error::ClientError> {
        use crate::error::ClientError;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| ClientError::audio(format!("config dir: {e}")))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| ClientError::audio(e.to_string()))?;
        fs::write(path, json).map_err(|e| ClientError::audio(format!("config write: {e}")))
    }
}

/// Partial update payload from the settings panel.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatch {
    pub base_url: Option<String>,
    pub hotkey: Option<String>,
    pub cleanup: Option<bool>,
}
