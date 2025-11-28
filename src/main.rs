//! RedWeather - A Waybar weather widget using OpenWeatherMap API
//!
//! This application fetches weather data and displays it in a format suitable for Waybar.
//! It supports geocoding, caching, and a GTK prompt for location configuration.

mod config;
mod dashboard;
mod formatting;
mod graph;
mod ui;
mod utils;
mod weather;

use serde_json::json;
use std::env;

use config::{load_config, load_key, ColorsResolved, TempBand, UiConfigResolved};
use formatting::format_popup_text;
use ui::run_prompt;
use weather::{fetch_weather_for_loc, load_cache, load_stale_cache, resolve_location, save_cache};

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<String>>();
    let prompt_mode = args.iter().any(|a| a == "--prompt");
    let dashboard_mode = args.iter().any(|a| a == "--dashboard");
    let reload_mode = args.iter().any(|a| a == "--reload");
    let open_web_mode = args.iter().any(|a| a == "--open-web");

    let key = match load_key() {
        Some(k) => k,
        None => {
            eprintln!("Missing OWM_API_KEY (env or ~/.config/redweather/apikey)");
            if prompt_mode || dashboard_mode {
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
        let zip_arg = args.iter()
            .skip(1)
            .find(|s| !s.starts_with("--"))
            .cloned();
        let cfg = load_config();

        if let Some(loc) = resolve_location(&key, zip_arg.as_deref(), &cfg).await {
            let url = format!("https://openweathermap.org/city/{}/{}", loc.lat, loc.lon);
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
        return;
    }

    // Check if this is first run (no location configured)
    // Filter out flags from arguments to find potential ZIP/location arg
    let zip_arg = args.iter()
        .skip(1)
        .find(|s| !s.starts_with("--"))
        .cloned();
    let cfg = load_config();

    let loc = resolve_location(&key, zip_arg.as_deref(), &cfg).await;

    // If no location is configured, show setup message
    if loc.is_none() {
        eprintln!("No location configured. Please run with --prompt to set your location.");
        if dashboard_mode {
             eprintln!("Cannot launch dashboard without a configured location.");
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

    let loc = loc.unwrap(); // Safe because we checked above
    let cache_key = format!("{:.3}_{:.3}", loc.lat, loc.lon);

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

    // Waybar Mode: Synchronous fetch/cache for CLI output
    // Skip cache if reload mode is active
    let data = if reload_mode {
        match fetch_weather_for_loc(&key, &loc, cfg.units).await {
            Ok(d) => {
                save_cache(&cache_key, &d);
                d
            }
            Err(e) => {
                // Try to use stale cache as fallback
                if let Some(stale) = load_stale_cache(&cache_key) {
                    eprintln!("Using stale cache due to error: {}", e);
                    stale
                } else {
                    let fallback = json!({
                        "text": "| ❓ N/A",
                        "tooltip": format!("<span foreground='#f4b8e4'>Weather error: {}</span>", e),
                        "markup": "pango"
                    });
                    println!("{}", fallback);
                    return;
                }
            }
        }
    } else {
        match load_cache(&cache_key) {
            Some(cached) => cached,
            None => match fetch_weather_for_loc(&key, &loc, cfg.units).await {
                Ok(d) => {
                    save_cache(&cache_key, &d);
                    d
                }
                Err(e) => {
                    // Try to use stale cache as fallback
                    if let Some(stale) = load_stale_cache(&cache_key) {
                        eprintln!("Using stale cache due to error: {}", e);
                        stale
                    } else {
                        let fallback = json!({
                            "text": "| ❓ N/A",
                            "tooltip": format!("<span foreground='#f4b8e4'>Weather error: {}</span>", e),
                            "markup": "pango"
                        });
                        println!("{}", fallback);
                        return;
                    }
                }
            },
        }
    };

    let ui_resolved = UiConfigResolved::from_config(&cfg.ui);
    let colors_resolved = ColorsResolved::from_config(&cfg.colors);
    let bands = TempBand::from_config(&cfg.temp_bands);

    let (text, tooltip) = format_popup_text(
        &data,
        &loc.label,
        &ui_resolved,
        &colors_resolved,
        &bands,
        cfg.units,
    );
    let out = json!({
        "text": text,
        "tooltip": tooltip,
        "markup": "pango"
    });
    println!("{}", out);
}
