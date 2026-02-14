//! RedWeather - A Waybar weather widget using OpenWeatherMap API
//!
//! This application fetches weather data and displays it in a format suitable for Waybar.
//! It supports geocoding, caching, and a GTK prompt for location configuration.

mod astro;
mod config;
mod dashboard;
mod formatting;
mod gauges;
mod graph;
mod popup;
mod ui;
mod utils;
mod weather;

use serde_json::json;
use std::env;
use tracing::{error, info, warn};

use config::{load_config, load_key, ColorsResolved, TempBand, UiConfigResolved, Units};
use formatting::format_popup_text;
use ui::run_prompt;
use weather::{
    fetch_weather_for_loc, load_cache, load_stale_cache, resolve_location, save_cache, ApiResponse,
};

/// Fetches weather data with cache support and stale-cache fallback.
///
/// When `skip_cache` is true, always fetches fresh data from the API.
/// On fetch failure, falls back to stale cached data if available.
async fn fetch_or_fallback(
    key: &str,
    loc: &weather::Location,
    units: Units,
    cache_key: &str,
    skip_cache: bool,
) -> Option<ApiResponse> {
    // Try fresh cache first (unless skipping)
    if !skip_cache {
        if let Some(cached) = load_cache(cache_key) {
            return Some(cached);
        }
    }

    // Fetch from API
    match fetch_weather_for_loc(key, loc, units).await {
        Ok(data) => {
            save_cache(cache_key, &data);
            Some(data)
        }
        Err(e) => {
            // Fall back to stale cache
            if let Some(stale) = load_stale_cache(cache_key) {
                warn!(error = %e, "Using stale cache due to fetch error");
                Some(stale)
            } else {
                error!(error = %e, "Weather fetch failed with no cache fallback");
                None
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Initialize structured logging (controlled via RUST_LOG env, e.g. RUST_LOG=debug)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = env::args().collect::<Vec<String>>();
    let prompt_mode = args.iter().any(|a| a == "--prompt");
    let dashboard_mode = args.iter().any(|a| a == "--dashboard");
    let popup_mode = args.iter().any(|a| a == "--popup");
    let reload_mode = args.iter().any(|a| a == "--reload");
    let open_web_mode = args.iter().any(|a| a == "--open-web");

    let key = match load_key() {
        Some(k) => k,
        None => {
            error!("Missing OWM_API_KEY (env or ~/.config/redweather/apikey)");
            if prompt_mode || dashboard_mode || popup_mode {
                return;
            }
            let fallback = json!({
                "text": "| ❓ N/A",
                "tooltip": "<span foreground='#f4b8e4'>Missing OWM_API_KEY (env or ~/.config/redweather/apikey)</span>",
                "markup": "pango"
            });
            println!("{}", fallback);
            return;
        }
    };

    // Handle prompt mode or first-run setup
    if prompt_mode {
        let cfg = load_config();
        let _ = run_prompt(&key, &cfg);
        return;
    }

    // Handle open web mode
    if open_web_mode {
        let zip_arg = args.iter().skip(1).find(|s| !s.starts_with("--")).cloned();
        let cfg = load_config();

        if let Ok(Some(loc)) = resolve_location(&key, zip_arg.as_deref(), &cfg).await {
            let url = format!("https://openweathermap.org/city/{}/{}", loc.lat, loc.lon);
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
        return;
    }

    // Check if this is first run (no location configured)
    // Filter out flags from arguments to find potential ZIP/location arg
    let zip_arg = args.iter().skip(1).find(|s| !s.starts_with("--")).cloned();
    let cfg = load_config();

    let loc = resolve_location(&key, zip_arg.as_deref(), &cfg).await;

    // If no location is configured, show setup message
    let loc = match loc {
        Ok(Some(l)) => l,
        Ok(None) => {
            warn!("No location configured. Please run with --prompt to set your location.");
            if dashboard_mode {
                error!("Cannot launch dashboard without a configured location.");
                return;
            }
            let fallback = json!({
                "text": "| ❓ Setup",
                "tooltip": "<span foreground='#f4b8e4'>Right-click to set your location</span>",
                "markup": "pango"
            });
            println!("{}", fallback);
            return;
        }
        Err(e) => {
            error!(error = %e, "Location resolution failed");
            if dashboard_mode {
                return;
            }
            let fallback = json!({
                "text": "| ❓ Error",
                "tooltip": format!("<span foreground='#f4b8e4'>Location error: {}</span>", e),
                "markup": "pango"
            });
            println!("{}", fallback);
            return;
        }
    };

    let cache_key = format!("{:.3}_{:.3}", loc.lat, loc.lon);

    // Popup Mode: Layer Shell overlay panel triggered by Waybar click
    if popup_mode {
        let data = match fetch_or_fallback(&key, &loc, cfg.units, &cache_key, reload_mode).await {
            Some(d) => d,
            None => {
                error!("Cannot launch popup: weather fetch failed with no cache");
                return;
            }
        };
        popup::run_popup(data, &loc.label, &cfg);
        return;
    }

    // Dashboard Mode: Launch immediately with cached data (if any)
    // The dashboard will handle background fetching/refreshing
    if dashboard_mode {
        let data = if reload_mode {
            None
        } else {
            load_stale_cache(&cache_key)
        };
        dashboard::run_dashboard(data, loc, cfg.units, key, cfg);
        return;
    }

    // Waybar Mode: fetch with cache support and stale fallback
    let data = match fetch_or_fallback(&key, &loc, cfg.units, &cache_key, reload_mode).await {
        Some(d) => d,
        None => {
            let fallback = json!({
                "text": "| ❓ N/A",
                "tooltip": "<span foreground='#f4b8e4'>Weather fetch failed</span>",
                "markup": "pango"
            });
            println!("{}", fallback);
            return;
        }
    };

    info!(location = %loc.label, "Rendering weather output");

    let ui_resolved = UiConfigResolved::from_config(&cfg.ui);
    let colors_resolved = ColorsResolved::from_config(&cfg.colors);
    let bands = TempBand::from_config(&cfg.temp_bands);

    let (text, _tooltip) = format_popup_text(
        &data,
        &loc.label,
        &ui_resolved,
        &colors_resolved,
        &bands,
        cfg.units,
    );
    let out = json!({
        "text": text,
        "tooltip": "Left-click: Dashboard  ·  Right-click: Popup",
    });
    println!("{}", out);
}
