//! Weather API interaction and data structures
//!
//! This module handles all communication with the OpenWeatherMap API including
//! weather data fetching, geocoding, and caching.

use anyhow::{anyhow, Result};
use chrono::Utc;
use once_cell::sync::Lazy;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration as StdDuration;

use crate::config::{Config, Units};

/// Maximum age of cached weather data in seconds (10 minutes)
pub const CACHE_MAX_AGE_SECS: i64 = 600;
/// Cache directory path relative to HOME
pub const CACHE_FILE: &str = ".cache/redweather";
/// Maximum retry attempts for API requests
const MAX_RETRIES: u32 = 3;
/// Base delay for exponential backoff (milliseconds)
const RETRY_BASE_DELAY_MS: u64 = 500;

/// Shared HTTP client for reuse across requests
static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(StdDuration::from_secs(10))
        .build()
        .expect("Failed to create HTTP client")
});

/// Represents a geographic location with coordinates and display label
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}

/// Weather condition description from API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeatherDesc {
    pub main: Option<String>,
    pub description: Option<String>,
}

/// Current weather conditions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Current {
    pub dt: i64,
    pub temp: f64,
    pub feels_like: Option<f64>,
    pub pressure: Option<i64>,
    pub humidity: Option<u8>,
    pub uvi: Option<f64>,
    pub visibility: Option<u32>,
    pub wind_speed: Option<f64>,
    pub wind_deg: Option<i64>,
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
    pub weather: Vec<WeatherDesc>,
    #[serde(default)]
    pub rain: Option<HashMap<String, f64>>,
    #[serde(default)]
    pub snow: Option<HashMap<String, f64>>,
}

/// Hourly weather forecast data
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Hourly {
    pub dt: i64,
    pub temp: f64,
    pub pressure: Option<i64>,
    pub humidity: Option<u8>,
    pub uvi: Option<f64>,
    pub pop: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_deg: Option<i64>,
    pub weather: Vec<WeatherDesc>,
}

/// Temperature range for daily forecasts
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TempRange {
    pub day: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Daily weather forecast data
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Daily {
    pub dt: i64,
    pub sunrise: Option<i64>,
    pub sunset: Option<i64>,
    pub temp: TempRange,
    pub pressure: Option<i64>,
    pub humidity: Option<u8>,
    pub uvi: Option<f64>,
    pub pop: Option<f64>,
    pub weather: Vec<WeatherDesc>,
}

/// OpenWeatherMap API response structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiResponse {
    pub timezone_offset: i64,
    pub current: Current,
    pub hourly: Vec<Hourly>,
    pub daily: Vec<Daily>,
}

/// Cached weather data with timestamp
#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheFile {
    fetched_at: i64,
    data: ApiResponse,
}

/// Generates cache file path for a given cache key
fn cache_path(cache_key: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let safe_key = cache_key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    PathBuf::from(home).join(format!("{}/cache_{}.json", CACHE_FILE, safe_key))
}

/// Loads cached weather data if it exists and is fresh
pub fn load_cache(cache_key: &str) -> Option<ApiResponse> {
    let path = cache_path(cache_key);
    let contents = fs::read_to_string(path).ok()?;
    let cached: CacheFile = serde_json::from_str(&contents).ok()?;
    let age = Utc::now().timestamp() - cached.fetched_at;
    if age <= CACHE_MAX_AGE_SECS {
        Some(cached.data)
    } else {
        None
    }
}

/// Loads cached weather data regardless of age (for error fallback)
pub fn load_stale_cache(cache_key: &str) -> Option<ApiResponse> {
    let path = cache_path(cache_key);
    let contents = fs::read_to_string(path).ok()?;
    let cached: CacheFile = serde_json::from_str(&contents).ok()?;
    Some(cached.data)
}

/// Saves weather data to cache
pub fn save_cache(cache_key: &str, data: &ApiResponse) {
    let path = cache_path(cache_key);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let cache = CacheFile {
        fetched_at: Utc::now().timestamp(),
        data: data.clone(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = fs::write(path, json);
    }
}

/// Fetches weather data for a specific location with retry logic
pub async fn fetch_weather_for_loc(key: &str, loc: &Location, units: Units) -> Result<ApiResponse> {
    let units_str = match units {
        Units::Imperial => "imperial",
        Units::Metric => "metric",
    };

    let mut url = Url::parse("https://api.openweathermap.org/data/3.0/onecall")?;
    url.query_pairs_mut()
        .append_pair("lat", &loc.lat.to_string())
        .append_pair("lon", &loc.lon.to_string())
        .append_pair("appid", key)
        .append_pair("units", units_str)
        .append_pair("exclude", "minutely,alerts");

    // Retry logic with exponential backoff
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match HTTP_CLIENT.get(url.clone()).send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(r) => match r.json::<ApiResponse>().await {
                    Ok(parsed) => return Ok(parsed),
                    Err(e) => last_error = Some(anyhow!("Failed to parse JSON: {}", e)),
                },
                Err(e) => last_error = Some(anyhow!("API returned error status: {}", e)),
            },
            Err(e) => last_error = Some(anyhow!("Request failed: {}", e)),
        }

        // Exponential backoff before retry
        if attempt < MAX_RETRIES - 1 {
            let delay = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt);
            tokio::time::sleep(StdDuration::from_millis(delay)).await;
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow!("Weather fetch failed after {} attempts", MAX_RETRIES)))
}

/// Resolves a location from command-line overrides or configured presets
pub async fn resolve_location(key: &str, zip: Option<&str>, cfg: &Config) -> Option<Location> {
    // Priority 1: Command line ZIP argument (one-time override)
    if let Some(z) = zip {
        if let Some(loc) = geocode_zip_with_retry(key, z).await {
            return Some(loc);
        }
        if let Some(loc) = geocode_direct_with_retry(key, z).await {
            return Some(loc);
        }
    }

    // Priority 2: Location presets from config
    if let Some(presets) = cfg.location_presets.as_ref() {
        if let Some(active) = cfg.active_preset.as_ref() {
            if let Some(preset) = presets.iter().find(|p| &p.name == active) {
                return Some(Location {
                    lat: preset.lat,
                    lon: preset.lon,
                    label: preset.label.clone(),
                });
            }
        }
        // Use first preset if active not specified
        if let Some(first) = presets.first() {
            return Some(Location {
                lat: first.lat,
                lon: first.lon,
                label: first.label.clone(),
            });
        }
    }

    // No location configured
    None
}

/// Geocodes a ZIP code with retry logic
async fn geocode_zip_with_retry(key: &str, zip: &str) -> Option<Location> {
    for attempt in 0..MAX_RETRIES {
        if let Some(loc) = geocode_zip(key, zip).await {
            return Some(loc);
        }
        if attempt < MAX_RETRIES - 1 {
            let delay = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt);
            tokio::time::sleep(StdDuration::from_millis(delay)).await;
        }
    }
    None
}

/// Geocodes a direct query with retry logic
async fn geocode_direct_with_retry(key: &str, query: &str) -> Option<Location> {
    for attempt in 0..MAX_RETRIES {
        if let Some(loc) = geocode_direct(key, query).await {
            return Some(loc);
        }
        if attempt < MAX_RETRIES - 1 {
            let delay = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt);
            tokio::time::sleep(StdDuration::from_millis(delay)).await;
        }
    }
    None
}

/// Geocodes a ZIP code to geographic coordinates
pub async fn geocode_zip(key: &str, zip: &str) -> Option<Location> {
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/zip").ok()?;
    let zip_param = if zip.contains(',') {
        zip.to_string()
    } else {
        format!("{},US", zip)
    };
    url.query_pairs_mut()
        .append_pair("zip", &zip_param)
        .append_pair("appid", key);

    let resp = HTTP_CLIENT.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    #[derive(Deserialize)]
    struct ZipResp {
        lat: f64,
        lon: f64,
        name: Option<String>,
        country: Option<String>,
    }
    let zr: ZipResp = resp.json().await.ok()?;
    let label = zr
        .name
        .or_else(|| Some(format!("ZIP {}", zip)))
        .map(|n| {
            if let Some(c) = zr.country {
                format!("{}, {}", n, c)
            } else {
                n
            }
        })
        .unwrap_or_else(|| format!("ZIP {}", zip));
    Some(Location {
        lat: zr.lat,
        lon: zr.lon,
        label,
    })
}

/// Geocodes a city/location query to geographic coordinates
pub async fn geocode_direct(key: &str, query: &str) -> Option<Location> {
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/direct").ok()?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", "1")
        .append_pair("appid", key);

    let resp = HTTP_CLIENT.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    #[derive(Deserialize)]
    struct DirResp {
        lat: f64,
        lon: f64,
        name: Option<String>,
        country: Option<String>,
        state: Option<String>,
    }
    let list: Vec<DirResp> = resp.json().await.ok()?;
    let first = list.into_iter().next()?;
    let mut label = first.name.unwrap_or_else(|| query.to_string());
    if let Some(c) = first.country {
        label = format!("{}, {}", label, c);
    }
    if let Some(s) = first.state {
        label = format!("{} ({})", label, s);
    }
    Some(Location {
        lat: first.lat,
        lon: first.lon,
        label,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_path_sanitization() {
        let key = "37.545_-97.268";
        let path = cache_path(key);
        assert!(path.to_string_lossy().contains("cache_37_545__97_268"));
    }
}
