use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub host: String,
    pub access_code: String,
    pub serial: String,
    /// Desktop notifications for progress milestones and finished/failed prints.
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { host: String::new(), access_code: String::new(), serial: String::new(), notify: true }
    }
}

impl Settings {
    /// True when there is enough to try connecting. The host is optional:
    /// the printer is found on the network by its serial number.
    pub fn is_complete(&self) -> bool {
        !self.access_code.is_empty() && !self.serial.is_empty()
    }

    pub fn trimmed(self) -> Settings {
        Settings {
            host: self.host.trim().to_string(),
            access_code: self.access_code.trim().to_string(),
            serial: self.serial.trim().to_string(),
            notify: self.notify,
        }
    }
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Can't create the settings folder: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| format!("Can't save settings: {e}"))
}
