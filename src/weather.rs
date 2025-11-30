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
    // Check for test override first, then HOME, then current directory
    let home = env::var("REDWEATHER_CACHE_DIR")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    
    let safe_key = cache_key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    PathBuf::from(home).join(format!("{}/cache_{}.json", CACHE_FILE, safe_key))
}

/// Loads cached weather data if it exists and is fresh
pub fn load_cache(cache_key: &str) -> Option<ApiResponse> {
    let path = cache_path(cache_key);
    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading cache file {}: {}", path.display(), e);
            return None;
        }
    };
    let cached: CacheFile = match serde_json::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error parsing cache file {}: {}", path.display(), e);
            return None;
        }
    };
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
    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading stale cache file {}: {}", path.display(), e);
            return None;
        }
    };
    let cached: CacheFile = match serde_json::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error parsing stale cache file {}: {}", path.display(), e);
            return None;
        }
    };
    Some(cached.data)
}

/// Saves weather data to cache
pub fn save_cache(cache_key: &str, data: &ApiResponse) {
    let path = cache_path(cache_key);
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Error creating cache directory {}: {}", parent.display(), e);
            return;
        }
    }
    let cache = CacheFile {
        fetched_at: Utc::now().timestamp(),
        data: data.clone(),
    };
    match serde_json::to_string(&cache) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("Error writing cache file {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            eprintln!("Error serializing cache data for {}: {}", path.display(), e);
        }
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
pub async fn resolve_location(
    key: &str,
    zip: Option<&str>,
    cfg: &Config,
) -> Result<Option<Location>> {
    // Priority 1: Command line ZIP argument (one-time override)
    if let Some(z) = zip {
        // Try ZIP geocoding first
        match geocode_zip_with_retry(key, z).await {
            Ok(Some(loc)) => return Ok(Some(loc)),
            Err(e) => {
                // If it wasn't a 404/not found logic error but a network/API error, maybe log it?
                // For now, we fall through to try direct geocoding, but we might want to surface this error if direct also fails.
                eprintln!("ZIP geocoding warning: {}", e);
            }
            Ok(None) => {} // Just not found as ZIP, try direct
        }

        // Try direct name geocoding
        match geocode_direct_with_retry(key, z).await {
            Ok(Some(loc)) => return Ok(Some(loc)),
            Err(e) => return Err(e), // Return the error if direct geocoding failed technically
            Ok(None) => return Ok(None), // Both methods returned None (Not Found)
        }
    }

    // Priority 2: Location presets from config
    if let Some(presets) = cfg.location_presets.as_ref() {
        if let Some(active) = cfg.active_preset.as_ref() {
            if let Some(preset) = presets.iter().find(|p| &p.name == active) {
                return Ok(Some(Location {
                    lat: preset.lat,
                    lon: preset.lon,
                    label: preset.label.clone(),
                }));
            }
        }
        // Use first preset if active not specified
        if let Some(first) = presets.first() {
            return Ok(Some(Location {
                lat: first.lat,
                lon: first.lon,
                label: first.label.clone(),
            }));
        }
    }

    // No location configured
    Ok(None)
}

/// Geocodes a ZIP code with retry logic
async fn geocode_zip_with_retry(key: &str, zip: &str) -> Result<Option<Location>> {
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match geocode_zip(key, zip).await {
            Ok(opt) => return Ok(opt),
            Err(e) => last_error = Some(e),
        }
        if attempt < MAX_RETRIES - 1 {
            let delay = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt);
            tokio::time::sleep(StdDuration::from_millis(delay)).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Geocode ZIP failed after retries")))
}

/// Geocodes a direct query with retry logic
async fn geocode_direct_with_retry(key: &str, query: &str) -> Result<Option<Location>> {
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match geocode_direct(key, query).await {
            Ok(opt) => return Ok(opt),
            Err(e) => last_error = Some(e),
        }
        if attempt < MAX_RETRIES - 1 {
            let delay = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt);
            tokio::time::sleep(StdDuration::from_millis(delay)).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Geocode direct failed after retries")))
}

/// Geocodes a ZIP code to geographic coordinates
pub async fn geocode_zip(key: &str, zip: &str) -> Result<Option<Location>> {
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/zip")?;
    let zip_param = if zip.contains(',') {
        zip.to_string()
    } else {
        format!("{},US", zip)
    };
    url.query_pairs_mut()
        .append_pair("zip", &zip_param)
        .append_pair("appid", key);

    let resp = HTTP_CLIENT.get(url).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(anyhow!("API error: {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct ZipResp {
        lat: f64,
        lon: f64,
        name: Option<String>,
        country: Option<String>,
    }
    let zr: ZipResp = resp.json().await?;
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
    Ok(Some(Location {
        lat: zr.lat,
        lon: zr.lon,
        label,
    }))
}

/// Geocodes a city/location query to geographic coordinates
pub async fn geocode_direct(key: &str, query: &str) -> Result<Option<Location>> {
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/direct")?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", "1")
        .append_pair("appid", key);

    let resp = HTTP_CLIENT.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("API error: {}", resp.status()));
    }

    #[derive(Deserialize)]
    struct DirResp {
        lat: f64,
        lon: f64,
        name: Option<String>,
        country: Option<String>,
        state: Option<String>,
    }
    let list: Vec<DirResp> = resp.json().await?;
    if list.is_empty() {
        return Ok(None);
    }
    let first = list.into_iter().next().unwrap(); // Safe because !empty
    let mut label = first.name.unwrap_or_else(|| query.to_string());
    if let Some(c) = first.country {
        label = format!("{}, {}", label, c);
    }
    if let Some(s) = first.state {
        label = format!("{} ({})", label, s);
    }
    Ok(Some(Location {
        lat: first.lat,
        lon: first.lon,
        label,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::env;

    #[test]
    fn test_cache_path_sanitization() {
        let key = "37.545_-97.268";
        let path = cache_path(key);
        assert!(path.to_string_lossy().contains("cache_37_545__97_268"));
    }

    #[test]
    fn test_api_response_parsing() {
        let json = r#"{
            "timezone_offset": -18000,
            "current": {
                "dt": 1684929490,
                "temp": 72.5,
                "feels_like": 71.0,
                "pressure": 1015,
                "humidity": 53,
                "uvi": 5.2,
                "visibility": 10000,
                "wind_speed": 8.5,
                "wind_deg": 220,
                "weather": [
                    {
                        "main": "Clouds",
                        "description": "scattered clouds"
                    }
                ]
            },
            "hourly": [
                {
                    "dt": 1684933200,
                    "temp": 71.2,
                    "weather": [{"main": "Rain", "description": "light rain"}]
                }
            ],
            "daily": [
                {
                    "dt": 1684951200,
                    "temp": {
                        "day": 70.0,
                        "min": 65.0,
                        "max": 75.0
                    },
                    "weather": [{"main": "Clear", "description": "clear sky"}]
                }
            ]
        }"#;

        let resp: ApiResponse = serde_json::from_str(json).expect("Failed to parse valid JSON");
        
        assert_eq!(resp.timezone_offset, -18000);
        assert_eq!(resp.current.temp, 72.5);
        assert_eq!(resp.current.weather[0].main.as_deref(), Some("Clouds"));
        assert_eq!(resp.hourly.len(), 1);
        assert_eq!(resp.hourly[0].temp, 71.2);
        assert_eq!(resp.daily.len(), 1);
        assert_eq!(resp.daily[0].temp.max, Some(75.0));
    }

    #[test]
    fn test_cache_lifecycle() {
        // Create a temporary directory for cache testing
        let temp_dir = env::temp_dir().join("redweather_test_cache");
        fs::create_dir_all(&temp_dir).unwrap();
        
        // Override cache directory using our new env var
        env::set_var("REDWEATHER_CACHE_DIR", &temp_dir);
        
        let cache_key = "test_loc_123";
        
        // Create dummy data
        let dummy_data = ApiResponse {
            timezone_offset: 0,
            current: Current {
                dt: Utc::now().timestamp(),
                temp: 20.0,
                feels_like: None,
                pressure: None,
                humidity: None,
                uvi: None,
                visibility: None,
                wind_speed: None,
                wind_deg: None,
                sunrise: None,
                sunset: None,
                weather: vec![],
                rain: None,
                snow: None,
            },
            hourly: vec![],
            daily: vec![],
        };

        // Test Save
        save_cache(cache_key, &dummy_data);
        
        // Verify file exists
        let expected_path = temp_dir.join(format!("{}/cache_{}.json", CACHE_FILE, cache_key));
        assert!(expected_path.exists(), "Cache file was not created at {:?}", expected_path);

        // Test Load
        let loaded = load_cache(cache_key);
        assert!(loaded.is_some(), "Failed to load cached data");
        assert_eq!(loaded.unwrap().current.temp, 20.0);

        // Cleanup
        env::remove_var("REDWEATHER_CACHE_DIR");
        let _ = fs::remove_dir_all(temp_dir);
    }
}
