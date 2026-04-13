use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const APP_DIR_NAME: &str = "aynime-issen";
const CONFIG_FILENAME: &str = "config.json";

/// Application configuration, persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Enabled still image formats
    pub still_formats: Vec<String>,
    /// Enabled video/animation formats
    pub video_formats: Vec<String>,
    /// Max file size presets in MB (shown as selectable options)
    pub max_size_presets_mb: Vec<f32>,
    /// Default max file size in MB
    pub default_max_size_mb: f32,
    /// Default FPS for video recording
    pub default_fps: u32,
    /// Default export format
    pub default_format: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            still_formats: vec![
                "PNG".into(),
                "WebP".into(),
                "JPEG".into(),
                "BMP".into(),
            ],
            video_formats: vec![
                "GIF".into(),
                "MP4".into(),
                "WebM".into(),
            ],
            max_size_presets_mb: vec![2.0, 4.0, 8.0, 10.0, 25.0, 50.0],
            default_max_size_mb: 8.0,
            default_fps: 15,
            default_format: "PNG".into(),
        }
    }
}

impl AppConfig {
    /// Load config from disk, or create default if not found.
    pub fn load() -> Self {
        let path = Self::config_path();
        match fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(config) => {
                    log::info!("Config loaded from {}", path.display());
                    config
                }
                Err(e) => {
                    log::warn!("Failed to parse config, using defaults: {}", e);
                    let config = Self::default();
                    let _ = config.save();
                    config
                }
            },
            Err(_) => {
                log::info!("No config found, creating default at {}", path.display());
                let config = Self::default();
                let _ = config.save();
                config
            }
        }
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
        fs::write(&path, json)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        log::info!("Config saved to {}", path.display());
        Ok(())
    }

    fn config_path() -> PathBuf {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(local)
                .join(APP_DIR_NAME)
                .join(CONFIG_FILENAME)
        } else {
            PathBuf::from(CONFIG_FILENAME)
        }
    }

    /// Get all format names (still + video) in order.
    pub fn all_formats(&self) -> Vec<String> {
        let mut all = self.still_formats.clone();
        all.extend(self.video_formats.clone());
        all
    }

    /// Check if a format name is a video format.
    pub fn is_video_format(&self, name: &str) -> bool {
        self.video_formats.iter().any(|f| f.eq_ignore_ascii_case(name))
    }
}
