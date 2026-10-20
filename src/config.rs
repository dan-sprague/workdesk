//! Settings file and data location.
//! On a Mac everything lives in ~/Library/Application Support/WorkDesk.

use crate::provider::{self, Provider};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where new project folders go unless `projects_root` says otherwise.
pub const DEFAULT_PROJECTS_ROOT: &str = "~/Documents/Projects/Repos";

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    /// Key for the chat model that files notes and writes the brief (any supported service).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Which service `api_key` belongs to (see provider.rs). Worked out from the key by `desk key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Any other OpenAI-compatible service: set this and `model` by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// TypeSafe key for Jev, which checks whether a note really fits one of the projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jev_key: Option<String>,
    /// Where workdesk.json lives, e.g. inside Google Drive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_file: Option<String>,
    /// Where new project folders are created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_root: Option<String>,
    /// Settings from 0.2 and earlier; moved into `api_key` on load.
    #[serde(default, skip_serializing)]
    deepseek_key: Option<String>,
}

/// The chat model Work Desk will talk to.
pub struct Chat {
    pub key: String,
    pub provider: &'static Provider,
    pub base: String,
    pub model: String,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn clean(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
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

/// Write a path the short way (`~/Documents/...`) so links work on any Mac with the same folders.
pub fn tilde(p: &std::path::Path) -> String {
    if let Some(h) = dirs::home_dir() {
        if let Ok(rest) = p.strip_prefix(&h) {
            return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
        }
    }
    p.display().to_string()
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Config {
        let mut cfg: Config = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if let (None, Some(k)) = (&cfg.api_key, cfg.deepseek_key.take()) {
            cfg.api_key = Some(k);
            cfg.provider = Some("deepseek".into());
        }
        cfg
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

    /// The chat model, if there is a key for one. `WORKDESK_API_KEY` wins over the saved key.
    pub fn chat(&self) -> Option<Chat> {
        let saved = clean(&self.provider).and_then(|p| provider::by_id(&p));
        let (key, provider) = match (env("WORKDESK_API_KEY"), env("DEEPSEEK_API_KEY")) {
            (Some(k), _) => {
                let p = provider::by_prefix(&k).or(saved).unwrap_or(&provider::PROVIDERS[0]);
                (k, p)
            }
            (None, Some(k)) => (k, provider::by_id("deepseek").unwrap()),
            (None, None) => (clean(&self.api_key)?, saved.unwrap_or(&provider::PROVIDERS[0])),
        };
        // A hand-set model or address only applies to the service it was set for.
        let own = saved.map_or(true, |s| s.id == provider.id);
        let model = clean(&self.model).filter(|_| own).unwrap_or_else(|| provider.model.to_string());
        let base = clean(&self.base_url).filter(|_| own).unwrap_or_else(|| provider.base.to_string());
        Some(Chat { key, provider, base: base.trim_end_matches('/').to_string(), model })
    }

    /// The Jev key, if any. `TYPESAFE_API_KEY` wins over the saved key.
    pub fn jev_key(&self) -> Option<String> {
        env("TYPESAFE_API_KEY").or_else(|| clean(&self.jev_key))
    }

    pub fn projects_root(&self) -> PathBuf {
        expand_home(clean(&self.projects_root).as_deref().unwrap_or(DEFAULT_PROJECTS_ROOT))
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
}
