//! Configuration management for redweather
//!
//! This module handles loading and parsing configuration from files and environment variables.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// API key file path relative to HOME
pub const KEY_FILE: &str = ".config/redweather/apikey";
/// Configuration file path relative to HOME
pub const CONFIG_FILE: &str = ".config/redweather/config.toml";
/// Legacy home location file (JSON) relative to HOME
const HOME_LOCATION_FILE: &str = ".config/redweather/home_location.json";

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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TempBand {
    pub max: f64,
    pub color: String,
}

/// UI configuration options from config file
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub font_size: Option<u8>,
    pub max_desc_len: Option<usize>,
}

/// Color configuration for UI elements
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ColorsConfig {
    pub header: Option<String>,
    pub text: Option<String>,
}

/// Named location preset
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocationPreset {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}

/// Dashboard specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DashboardConfig {
    #[serde(default)]
    pub show_hourly_graph: bool,
    #[serde(default)]
    pub forecast_hours: usize,
    #[serde(default)]
    pub forecast_days: usize,
    pub window_width: Option<i32>,
    pub window_height: Option<i32>,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        DashboardConfig {
            show_hourly_graph: true,
            forecast_hours: 24,
            forecast_days: 7,
            window_width: None,
            window_height: None,
        }
    }
}

/// Main configuration structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub units: Units,
    pub location_presets: Option<Vec<LocationPreset>>,
    pub active_preset: Option<String>,
    pub ui: Option<UiConfig>,
    pub colors: Option<ColorsConfig>,
    pub temp_bands: Option<Vec<TempBand>>,
    #[serde(default)]
    pub dashboard: Option<DashboardConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            units: Units::default(),
            location_presets: None,
            active_preset: None,
            ui: None,
            colors: None,
            temp_bands: None,
            dashboard: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyLocation {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigWithLegacy {
    #[serde(default)]
    pub units: Units,
    pub location: Option<LegacyLocation>,
    pub location_presets: Option<Vec<LocationPreset>>,
    pub active_preset: Option<String>,
    pub ui: Option<UiConfig>,
    pub colors: Option<ColorsConfig>,
    pub temp_bands: Option<Vec<TempBand>>,
    #[serde(default)]
    pub dashboard: Option<DashboardConfig>,
}

/// Parses configuration TOML, capturing legacy `[location]` if present
fn parse_config_with_legacy(contents: &str) -> (Config, Option<LegacyLocation>) {
    let parsed: ConfigWithLegacy = match toml::from_str(contents) {
        Ok(cfg) => cfg,
        Err(_) => return (Config::default(), None),
    };

    let cfg = Config {
        units: parsed.units,
        location_presets: parsed.location_presets,
        active_preset: parsed.active_preset,
        ui: parsed.ui,
        colors: parsed.colors,
        temp_bands: parsed.temp_bands,
        dashboard: parsed.dashboard,
    };

    (cfg, parsed.location)
}

/// Resolved dashboard configuration with defaults applied
#[derive(Debug, Clone)]
pub struct DashboardConfigResolved {
    pub show_hourly_graph: bool,
    pub forecast_hours: usize,
    pub forecast_days: usize,
    pub window_width: i32,
    pub window_height: i32,
}

impl DashboardConfigResolved {
    pub fn from_config(c: &Option<DashboardConfig>) -> Self {
        let defaults = DashboardConfig::default();
        match c {
            Some(c) => DashboardConfigResolved {
                show_hourly_graph: c.show_hourly_graph,
                forecast_hours: if c.forecast_hours == 0 { defaults.forecast_hours } else { c.forecast_hours },
                forecast_days: if c.forecast_days == 0 { defaults.forecast_days } else { c.forecast_days },
                window_width: c.window_width.unwrap_or(500),
                window_height: c.window_height.unwrap_or(700),
            },
            None => DashboardConfigResolved {
                show_hourly_graph: defaults.show_hourly_graph,
                forecast_hours: defaults.forecast_hours,
                forecast_days: defaults.forecast_days,
                window_width: 500,
                window_height: 700,
            },
        }
    }
}

/// Writes the config file to disk, ensuring parent directory exists
fn write_config_file(path: &Path, config: &Config) -> anyhow::Result<()> {
    let toml_string = toml::to_string_pretty(config).context("Failed to serialize config")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create config directory")?;
    }

    fs::write(path, toml_string).context("Failed to write config file")?;
    Ok(())
}

/// Inserts a preset if it doesn't already exist and sets active_preset if unset
fn upsert_preset(config: &mut Config, preset: LocationPreset) -> bool {
    let mut changed = false;

    if let Some(presets) = &mut config.location_presets {
        if presets.iter().any(|p| p.name == preset.name) {
            // Leave existing entry intact to avoid overwriting user data
        } else {
            presets.push(preset);
            changed = true;
        }
    } else {
        config.location_presets = Some(vec![preset]);
        changed = true;
    }

    if config.active_preset.is_none() {
        if let Some(presets) = &config.location_presets {
            if let Some(first) = presets.first() {
                config.active_preset = Some(first.name.clone());
                changed = true;
            }
        }
    }

    changed
}

/// Migrates legacy home_location.json into presets
fn migrate_legacy_home_location(config: &mut Config, home_dir: &str) -> bool {
    let path = PathBuf::from(home_dir).join(HOME_LOCATION_FILE);
    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    #[derive(Deserialize)]
    struct LegacyHome {
        lat: f64,
        lon: f64,
        label: Option<String>,
    }

    let parsed: LegacyHome = match serde_json::from_str(&contents) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let preset = LocationPreset {
        name: "home".to_string(),
        lat: parsed.lat,
        lon: parsed.lon,
        label: parsed.label.unwrap_or_else(|| "Home".to_string()),
    };

    let changed = upsert_preset(config, preset);
    if changed {
        let _ = fs::remove_file(path);
    }
    changed
}

/// Migrates legacy `[location]` section into presets (if none exist)
fn migrate_legacy_location_section(
    config: &mut Config,
    legacy_location: Option<LegacyLocation>,
) -> bool {
    let legacy = match legacy_location {
        Some(loc) => loc,
        None => return false,
    };

    let (lat, lon) = match (legacy.lat, legacy.lon) {
        (Some(lat), Some(lon)) => (lat, lon),
        _ => return false,
    };

    // If presets already exist, skip migration to avoid overwriting user data
    if config
        .location_presets
        .as_ref()
        .map_or(false, |p| !p.is_empty())
    {
        return false;
    }

    let preset = LocationPreset {
        name: "imported".to_string(),
        lat,
        lon,
        label: legacy
            .label
            .unwrap_or_else(|| "Imported location".to_string()),
    };

    upsert_preset(config, preset)
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
            TempBand {
                max: 59.0,
                color: "#8caaee".into(),
            },
            TempBand {
                max: 64.0,
                color: "#85c1dc".into(),
            },
            TempBand {
                max: 70.0,
                color: "#99d1db".into(),
            },
            TempBand {
                max: 75.0,
                color: "#81c8be".into(),
            },
            TempBand {
                max: 81.0,
                color: "#a6d189".into(),
            },
            TempBand {
                max: 86.0,
                color: "#e5c890".into(),
            },
            TempBand {
                max: 90.0,
                color: "#ef9f76".into(),
            },
            TempBand {
                max: 92.0,
                color: "#ea999c".into(),
            },
            TempBand {
                max: 500.0,
                color: "#e78284".into(),
            },
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
/// Performs a one-time migration of legacy location storage:
/// - Imports ~/.config/redweather/home_location.json as a "home" preset
/// - Imports legacy `[location]` table as a preset if no presets exist
/// Returns empty config if file doesn't exist or parsing fails.
pub fn load_config() -> Config {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Config::default(),
    };
    let path = PathBuf::from(&home).join(CONFIG_FILE);

    let (mut config, legacy_location) = match fs::read_to_string(&path) {
        Ok(contents) => parse_config_with_legacy(&contents),
        Err(_) => (Config::default(), None),
    };

    let mut changed = false;
    changed |= migrate_legacy_home_location(&mut config, &home);
    changed |= migrate_legacy_location_section(&mut config, legacy_location);

    if changed {
        let _ = write_config_file(&path, &config);
    }

    config
}

/// Updates the active_preset in the config file
pub fn update_active_preset(preset_name: &str) -> anyhow::Result<()> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let path = PathBuf::from(home).join(CONFIG_FILE);

    // Load current config
    let mut config = load_config();

    // Update active_preset
    config.active_preset = Some(preset_name.to_string());

    write_config_file(&path, &config)?;
    Ok(())
}

/// Adds or updates a location preset in the config file
pub fn save_location_preset(
    preset_name: &str,
    lat: f64,
    lon: f64,
    label: &str,
) -> anyhow::Result<()> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let path = PathBuf::from(home).join(CONFIG_FILE);

    // Load current config
    let mut config = load_config();

    // Create new preset
    let new_preset = LocationPreset {
        name: preset_name.to_string(),
        lat,
        lon,
        label: label.to_string(),
    };

    // Add or update preset
    if let Some(presets) = &mut config.location_presets {
        // Find and replace existing preset with same name
        if let Some(existing) = presets.iter_mut().find(|p| p.name == preset_name) {
            *existing = new_preset;
        } else {
            // Add new preset
            presets.push(new_preset);
        }
    } else {
        // Create new presets list
        config.location_presets = Some(vec![new_preset]);
    }

    // Set as active preset
    config.active_preset = Some(preset_name.to_string());

    write_config_file(&path, &config)?;
    Ok(())
}

/// Saves weather data to cache
pub fn save_config(config: &Config) -> anyhow::Result<()> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    let path = PathBuf::from(home).join(CONFIG_FILE);
    write_config_file(&path, config)?;
    Ok(())
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
