//! RedWeather - A Waybar weather widget using OpenWeatherMap API
//!
//! This application fetches weather data and displays it in a format suitable for Waybar.
//! It supports geocoding, caching, and a GTK prompt for location configuration.

mod config;
mod formatting;
mod ui;
mod utils;
mod weather;

use serde_json::json;
use std::env;

use config::{load_config, load_key, ColorsResolved, TempBand, UiConfigResolved};
use formatting::format_popup_text;
use ui::run_prompt;
use weather::{
    fetch_weather_for_loc, load_cache, load_home_location, load_stale_cache, load_zip_override,
    resolve_location, save_cache,
};

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<String>>();
    let prompt_mode = args.iter().any(|a| a == "--prompt");

    let key = match load_key() {
        Some(k) => k,
        None => {
            eprintln!("Missing OWM_API_KEY (env or ~/.config/redweather/apikey)");
            if prompt_mode {
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
        let _ = run_prompt(&key);
        return;
    }

    // Check if this is first run (no location configured)
    let zip_arg = args.get(1).cloned().filter(|s| !s.trim().is_empty());
    let zip_override = load_zip_override();
    let zip_choice = zip_arg.as_deref().or(zip_override.as_deref());
    let cfg = load_config();

    let loc = resolve_location(&key, zip_choice, &cfg).await;

    // If no location is configured, prompt the user
    if loc.is_none() && load_home_location().is_none() {
        eprintln!("No location configured. Please run with --prompt to set your location.");
        let fallback = json!({
            "text": "| ❓ Setup",
            "tooltip": "<span foreground='#f4b8e4'>Click to set your location\nRun: redweather --prompt</span>",
            "markup": "pango"
        });
        println!("{}", fallback);
        // Automatically launch prompt for first-run
        let _ = run_prompt(&key);
        return;
    }

    let loc = loc.unwrap(); // Safe because we checked above
    let cache_key = format!("{:.3}_{:.3}", loc.lat, loc.lon);

    let data = match load_cache(&cache_key) {
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
    };

    let ui_resolved = UiConfigResolved::from_config(&cfg.ui);
    let colors_resolved = ColorsResolved::from_config(&cfg.colors);
    let bands = TempBand::from_config(&cfg.temp_bands);

    let (text, tooltip) =
        format_popup_text(&data, &loc.label, &ui_resolved, &colors_resolved, &bands, cfg.units);
    let out = json!({
        "text": text,
        "tooltip": tooltip,
        "markup": "pango"
    });
    println!("{}", out);
}
