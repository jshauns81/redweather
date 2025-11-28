//! Configuration management for redweather
//!
//! This module handles loading and parsing configuration from files and environment variables.

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

/// API key file path relative to HOME
pub const KEY_FILE: &str = ".config/redweather/apikey";
/// Configuration file path relative to HOME
pub const CONFIG_FILE: &str = ".config/redweather/config.toml";

/// Unit system for temperature and speed
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    Imperial,
    Metric,
}

impl Default for Units {
    fn default() -> Self {
        Units::Imperial
    }
}

/// Temperature band configuration for color coding
#[derive(Debug, Clone, Deserialize)]
pub struct TempBand {
    pub max: f64,
    pub color: String,
}

/// UI configuration options from config file
#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    pub font_size: Option<u8>,
    pub max_desc_len: Option<usize>,
}

/// Color configuration for UI elements
#[derive(Debug, Clone, Deserialize)]
pub struct ColorsConfig {
    pub header: Option<String>,
    pub text: Option<String>,
}

/// Location configuration from config file (legacy single location)
#[derive(Debug, Clone, Deserialize)]
pub struct LocationConfig {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub label: Option<String>,
}

/// Named location preset
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocationPreset {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}

/// Main configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub units: Units,
    pub location: Option<LocationConfig>,
    pub location_presets: Option<Vec<LocationPreset>>,
    pub active_preset: Option<String>,
    pub ui: Option<UiConfig>,
    pub colors: Option<ColorsConfig>,
    pub temp_bands: Option<Vec<TempBand>>,
}

/// Resolved UI configuration with defaults applied
#[derive(Debug, Clone)]
pub struct UiConfigResolved {
    pub font_size: u8,
    pub max_desc_len: usize,
}

impl UiConfigResolved {
    pub fn from_config(ui: &Option<UiConfig>) -> Self {
        UiConfigResolved {
            font_size: ui.as_ref().and_then(|u| u.font_size).unwrap_or(9),
            max_desc_len: ui.as_ref().and_then(|u| u.max_desc_len).unwrap_or(10),
        }
    }
}

/// Resolved color configuration with defaults applied
#[derive(Debug, Clone)]
pub struct ColorsResolved {
    pub header: String,
    pub text: String,
}

impl ColorsResolved {
    /// Creates resolved colors from optional config, applying defaults
    pub fn from_config(c: &Option<ColorsConfig>) -> Self {
        ColorsResolved {
            header: c
                .as_ref()
                .and_then(|c| c.header.clone())
                .unwrap_or_else(|| "#f4b8e4".into()),
            text: c
                .as_ref()
                .and_then(|c| c.text.clone())
                .unwrap_or_else(|| "#ffffff".into()),
        }
    }
}

impl TempBand {
    /// Returns temperature bands from config or default catppuccin-inspired palette
    pub fn from_config(b: &Option<Vec<TempBand>>) -> Vec<TempBand> {
        if let Some(v) = b {
            if !v.is_empty() {
                return v.clone();
            }
        }
        vec![
            TempBand { max: 59.0, color: "#8caaee".into() },
            TempBand { max: 64.0, color: "#85c1dc".into() },
            TempBand { max: 70.0, color: "#99d1db".into() },
            TempBand { max: 75.0, color: "#81c8be".into() },
            TempBand { max: 81.0, color: "#a6d189".into() },
            TempBand { max: 86.0, color: "#e5c890".into() },
            TempBand { max: 90.0, color: "#ef9f76".into() },
            TempBand { max: 92.0, color: "#ea999c".into() },
            TempBand { max: 500.0, color: "#e78284".into() },
        ]
    }
}

/// Loads the OpenWeatherMap API key from environment variable or file
///
/// Checks OWM_API_KEY environment variable first, then ~/.config/redweather/apikey
pub fn load_key() -> Option<String> {
    if let Ok(k) = env::var("OWM_API_KEY") {
        if !k.trim().is_empty() {
            return Some(k);
        }
    }
    let home = env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(KEY_FILE);
    if let Ok(contents) = fs::read_to_string(path) {
        let t = contents.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    None
}

/// Loads configuration from ~/.config/redweather/config.toml
///
/// Returns empty config if file doesn't exist or parsing fails
pub fn load_config() -> Config {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Config {
            units: Units::default(),
            location: None,
            location_presets: None,
            active_preset: None,
            ui: None,
            colors: None,
            temp_bands: None
        },
    };
    let path = PathBuf::from(home).join(CONFIG_FILE);
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Config {
            units: Units::default(),
            location: None,
            location_presets: None,
            active_preset: None,
            ui: None,
            colors: None,
            temp_bands: None
        },
    };
    toml::from_str(&contents).unwrap_or(Config {
        units: Units::default(),
        location: None,
        location_presets: None,
        active_preset: None,
        ui: None,
        colors: None,
        temp_bands: None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_config_resolved_defaults() {
        let config = UiConfigResolved::from_config(&None);
        assert_eq!(config.font_size, 9);
        assert_eq!(config.max_desc_len, 10);

        let custom = Some(UiConfig {
            font_size: Some(12),
            max_desc_len: Some(20),
        });
        let config = UiConfigResolved::from_config(&custom);
        assert_eq!(config.font_size, 12);
        assert_eq!(config.max_desc_len, 20);
    }

    #[test]
    fn test_colors_resolved_defaults() {
        let colors = ColorsResolved::from_config(&None);
        assert_eq!(colors.header, "#f4b8e4");
        assert_eq!(colors.text, "#ffffff");

        let custom = Some(ColorsConfig {
            header: Some("#123456".into()),
            text: Some("#abcdef".into()),
        });
        let colors = ColorsResolved::from_config(&custom);
        assert_eq!(colors.header, "#123456");
        assert_eq!(colors.text, "#abcdef");
    }

    #[test]
    fn test_temp_bands_defaults() {
        let bands = TempBand::from_config(&None);
        assert_eq!(bands.len(), 9);
        assert_eq!(bands[0].max, 59.0);
        assert_eq!(bands[8].max, 500.0);
    }
}
