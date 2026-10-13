//! Settings file and data location.
//! On a Mac everything lives in ~/Library/Application Support/WorkDesk.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_MODEL: &str = "deepseek-chat";
pub const DEFAULT_BASE: &str = "https://api.deepseek.com";

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deepseek_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Where workdesk.json lives, e.g. inside Google Drive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_file: Option<String>,
}

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("WORKDESK_HOME") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("WorkDesk")
}

pub fn backup_dir() -> PathBuf {
    data_dir().join("backups")
}

/// Find the local "My Drive" folder that Google Drive for desktop keeps in sync.
pub fn find_google_drive() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else { return out };
    let mut roots = Vec::new();
    if let Ok(rd) = std::fs::read_dir(home.join("Library/CloudStorage")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("GoogleDrive") {
                roots.push(e.path());
            }
        }
    }
    // older Drive for desktop / Backup and Sync locations
    for old in ["Google Drive", "My Drive"] {
        let p = home.join(old);
        if p.is_dir() {
            roots.push(p);
        }
    }
    roots.sort();
    for r in roots {
        let my = r.join("My Drive");
        if my.is_dir() {
            out.push(my);
        } else if r.is_dir() {
            out.push(r);
        }
    }
    out
}

pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(p)
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(data_dir())?;
        let path = config_path();
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// The environment variable wins over the saved key.
    pub fn key(&self) -> Option<String> {
        std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| self.deepseek_key.clone())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    /// The data file: $WORKDESK_FILE, then the saved setting, then the local default.
    pub fn data_file(&self) -> PathBuf {
        if let Ok(p) = std::env::var("WORKDESK_FILE") {
            if !p.trim().is_empty() {
                return expand_home(p.trim());
            }
        }
        match self.data_file.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => expand_home(p),
            None => data_dir().join("workdesk.json"),
        }
    }

    pub fn model(&self) -> String {
        self.model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.into())
    }

    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE.into())
            .trim_end_matches('/')
            .to_string()
    }
}
