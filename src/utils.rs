//! Utility functions for weather display
//!
//! This module contains helper functions for formatting and displaying weather data.

use chrono::{DateTime, Duration, Utc};

use crate::config::TempBand;
use crate::weather::WeatherDesc;

const SYNODIC_MONTH: f64 = 29.530_588_67;
const NEW_MOON_EPOCH_UTC: i64 = 947_182_440; // 2000-01-06 18:14:00 UTC

/// Converts wind degree to cardinal direction (N, NE, E, etc.)
pub fn deg_to_dir(deg: Option<i64>) -> String {
    let d = deg.unwrap_or(0) as f64;
    let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((d + 22.5) / 45.0).floor() as usize) % 8;
    dirs[idx].to_string()
}

/// Returns a moon phase emoji for the given timestamp (with tz offset)
pub fn moon_phase_icon(dt: i64, tz_offset: i64) -> &'static str {
    let local_ts = dt + tz_offset;
    let days_since_epoch = (local_ts - NEW_MOON_EPOCH_UTC) as f64 / 86_400.0;
    let phase = days_since_epoch.rem_euclid(SYNODIC_MONTH);
    let idx = ((phase / SYNODIC_MONTH) * 8.0).round() as usize % 8;
    match idx {
        0 => "🌑",
        1 => "🌒",
        2 => "🌓",
        3 => "🌔",
        4 => "🌕",
        5 => "🌖",
        6 => "🌗",
        _ => "🌘",
    }
}

/// Determines whether a timestamp is considered night given sunrise/sunset
pub fn is_night(dt: i64, sunrise: Option<i64>, sunset: Option<i64>) -> bool {
    match (sunrise, sunset) {
        (Some(sr), Some(ss)) if ss > sr => dt < sr || dt >= ss,
        _ => false,
    }
}

/// Selects an appropriate emoji icon based on weather description and time of day
pub fn pick_icon(desc: &WeatherDesc, is_night: bool, moon_icon: Option<&str>) -> String {
    let main = desc.main.as_deref().unwrap_or("").to_lowercase();
    let full = desc.description.as_deref().unwrap_or("").to_lowercase();
    if main.contains("thunder") || full.contains("thunder") {
        "⛈".into()
    } else if main.contains("snow") || full.contains("snow") || main.contains("sleet") {
        "❄".into()
    } else if main.contains("rain") || full.contains("rain") || full.contains("drizzle") {
        "🌧".into()
    } else if full.contains("fog") || full.contains("mist") || full.contains("haze") {
        "🌫".into()
    } else if main.contains("cloud") || full.contains("cloud") || full.contains("overcast") {
        if is_night {
            "☁".into()
        } else {
            "☁".into()
        }
    } else if main.contains("clear") || full.contains("clear") || full.contains("sun") {
        if is_night {
            moon_icon.unwrap_or("🌙").to_string()
        } else {
            "☀".into()
        }
    } else {
        "❓".into()
    }
}

/// Maps weather description to a small icon key for chart usage.
/// Keys are asset identifiers (e.g., "clear_day") rather than emoji.
#[allow(dead_code)]
pub fn pick_small_icon_key(desc: &WeatherDesc, is_night: bool) -> &'static str {
    let main = desc.main.as_deref().unwrap_or("").to_lowercase();
    let full = desc.description.as_deref().unwrap_or("").to_lowercase();

    if main.contains("thunder") || full.contains("thunder") {
        "thunderstorm"
    } else if main.contains("snow") || full.contains("snow") || main.contains("sleet") {
        if full.contains("rain") || full.contains("mix") {
            "sleet"
        } else {
            "snow"
        }
    } else if main.contains("rain") || full.contains("rain") || full.contains("drizzle") {
        if full.contains("heavy") {
            "rain_heavy"
        } else if full.contains("light") || full.contains("drizzle") {
            "rain_light"
        } else {
            "rain"
        }
    } else if full.contains("fog") || full.contains("mist") || full.contains("haze") {
        "fog"
    } else if main.contains("cloud") || full.contains("cloud") || full.contains("overcast") {
        if full.contains("few") || full.contains("scattered") || full.contains("partly") {
            if is_night {
                "partly_cloudy_night"
            } else {
                "partly_cloudy_day"
            }
        } else {
            "cloudy"
        }
    } else if main.contains("clear") || full.contains("clear") || full.contains("sun") {
        if is_night {
            "clear_night"
        } else {
            "clear_day"
        }
    } else {
        "wind"
    }
}

/// Returns the color for a given temperature based on configured bands
pub fn temp_color(temp: f64, bands: &[TempBand]) -> String {
    for b in bands {
        if temp <= b.max {
            return b.color.clone();
        }
    }
    bands
        .last()
        .map(|b| b.color.clone())
        .unwrap_or_else(|| "#ffffff".into())
}

/// Formats a Unix timestamp with timezone offset using the given format string
pub fn fmt_time(dt: i64, tz_offset: i64, fmt: &str) -> String {
    let utc = DateTime::<Utc>::from_timestamp(dt, 0).unwrap_or_else(|| {
        DateTime::<Utc>::from_timestamp(0, 0).expect("epoch timestamp should be valid")
    });
    let shifted = utc + Duration::seconds(tz_offset);
    shifted.format(fmt).to_string()
}

/// Truncates a description string to the specified maximum character count.
/// Uses char boundaries to avoid panics on multibyte characters.
pub fn short_desc(desc: &str, max_len: usize) -> String {
    let trimmed = desc.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_len).collect()
}

/// Generates a sparkline string from a list of values using Unicode block characters
pub fn sparkline(values: &[f64]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let min = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let range = max - min;
    let blocks = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    values
        .iter()
        .map(|&v| {
            if range.abs() < f64::EPSILON {
                blocks[3] // Middle block for flat line
            } else {
                let idx = ((v - min) / range * (blocks.len() - 1) as f64).round() as usize;
                blocks[idx.min(blocks.len() - 1)]
            }
        })
        .collect()
}

/// Returns a color hex code for a given humidity percentage
pub fn humidity_color(humidity: u8) -> &'static str {
    if humidity < 30 {
        "#85c1dc" // Dry (light blue)
    } else if humidity < 55 {
        "#a3be8c" // Comfortable (green)
    } else if humidity < 70 {
        "#ebcb8b" // Slightly muggy (yellow)
    } else if humidity < 85 {
        "#d08770" // Muggy (orange)
    } else {
        "#bf616a" // Oppressive (red)
    }
}

/// Returns a color hex code for a given UV index
pub fn uvi_color(uvi: f64) -> &'static str {
    if uvi < 3.0 {
        "#a3be8c" // Low (Green)
    } else if uvi < 6.0 {
        "#ebcb8b" // Moderate (Yellow)
    } else if uvi < 8.0 {
        "#d08770" // High (Orange)
    } else if uvi < 11.0 {
        "#bf616a" // Very High (Red)
    } else {
        "#b48ead" // Extreme (Purple)
    }
}

/// Escapes special characters for Pango markup
pub fn escape_pango(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deg_to_dir() {
        assert_eq!(deg_to_dir(Some(0)), "N");
        assert_eq!(deg_to_dir(Some(45)), "NE");
        assert_eq!(deg_to_dir(Some(90)), "E");
        assert_eq!(deg_to_dir(Some(135)), "SE");
        assert_eq!(deg_to_dir(Some(180)), "S");
        assert_eq!(deg_to_dir(Some(225)), "SW");
        assert_eq!(deg_to_dir(Some(270)), "W");
        assert_eq!(deg_to_dir(Some(315)), "NW");
        assert_eq!(deg_to_dir(Some(360)), "N");
        assert_eq!(deg_to_dir(None), "N");
    }

    #[test]
    fn test_pick_icon() {
        let thunder = WeatherDesc {
            main: Some("Thunderstorm".into()),
            description: Some("thunderstorm with rain".into()),
        };
        assert_eq!(pick_icon(&thunder, false, None), "⛈");

        let snow = WeatherDesc {
            main: Some("Snow".into()),
            description: Some("light snow".into()),
        };
        assert_eq!(pick_icon(&snow, false, None), "❄");

        let rain = WeatherDesc {
            main: Some("Rain".into()),
            description: Some("moderate rain".into()),
        };
        assert_eq!(pick_icon(&rain, false, None), "🌧");

        let clear = WeatherDesc {
            main: Some("Clear".into()),
            description: Some("clear sky".into()),
        };
        assert_eq!(pick_icon(&clear, false, Some("🌙")), "☀");
        assert_eq!(pick_icon(&clear, true, Some("🌙")), "🌙");

        let clouds = WeatherDesc {
            main: Some("Clouds".into()),
            description: Some("broken clouds".into()),
        };
        assert_eq!(pick_icon(&clouds, false, None), "☁");
    }

    #[test]
    fn test_temp_color() {
        let bands = vec![
            TempBand {
                max: 50.0,
                color: "#blue".into(),
            },
            TempBand {
                max: 70.0,
                color: "#green".into(),
            },
            TempBand {
                max: 90.0,
                color: "#orange".into(),
            },
            TempBand {
                max: 500.0,
                color: "#red".into(),
            },
        ];

        assert_eq!(temp_color(40.0, &bands), "#blue");
        assert_eq!(temp_color(60.0, &bands), "#green");
        assert_eq!(temp_color(80.0, &bands), "#orange");
        assert_eq!(temp_color(100.0, &bands), "#red");
    }

    #[test]
    fn test_short_desc() {
        assert_eq!(short_desc("partly cloudy", 10), "partly clo");
        assert_eq!(short_desc("clear", 10), "clear");
        assert_eq!(short_desc("  cloudy  ", 10), "cloudy");
    }

    #[test]
    fn test_sparkline() {
        let data = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0];
        let sl = sparkline(&data);
        assert_eq!(sl.chars().count(), 8);
        // Should start low and end high
        assert!(sl.starts_with(' '));
        assert!(sl.ends_with('█'));
    }

    #[test]
    fn test_uvi_color() {
        assert_eq!(uvi_color(1.0), "#a3be8c");
        assert_eq!(uvi_color(12.0), "#b48ead");
    }

    #[test]
    fn test_humidity_color() {
        assert_eq!(humidity_color(20), "#85c1dc");  // Dry
        assert_eq!(humidity_color(45), "#a3be8c");  // Comfortable
        assert_eq!(humidity_color(65), "#ebcb8b");  // Slightly muggy
        assert_eq!(humidity_color(75), "#d08770");  // Muggy
        assert_eq!(humidity_color(90), "#bf616a");  // Oppressive
    }

    #[test]
    fn test_escape_pango() {
        assert_eq!(escape_pango("Safe"), "Safe");
        assert_eq!(escape_pango("R&B"), "R&amp;B");
        assert_eq!(escape_pango("<tag>"), "&lt;tag&gt;");
        assert_eq!(escape_pango("'Quote'"), "&apos;Quote&apos;");
    }
}
