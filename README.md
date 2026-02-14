# RedWeather 🌦️

A beautiful, customizable weather widget for Waybar using the OpenWeatherMap API. Features a rich GTK4 dashboard, a lightweight layer-shell popup, and a compact Waybar module — all powered by a single binary.

## Features

✨ **Smart First-Run Setup** - Automatically prompts for location on first use (no hardcoded defaults!)
🖥️ **Full Dashboard** - GTK4 dashboard with gauges, hourly graphs, daily forecasts, sun arc, and weather alerts
🪟 **Layer-Shell Popup** - Lightweight overlay panel anchored above Waybar for quick glances
🌡️ **Unit Switching** - Toggle between Imperial (°F, mph) and Metric (°C, m/s)
📍 **Multiple Locations** - Save presets for home, work, vacation, etc.
🔄 **Retry Logic** - Exponential backoff with 3 retry attempts for reliability
💾 **Graceful Degradation** - Falls back to stale cache during network issues
🎨 **Customizable Colors** - Temperature bands and UI colors via Catppuccin-inspired palette
⚡ **Performance** - Shared HTTP client, render caching for gauges/graphs, and 10-minute data cache
📊 **Structured Logging** - Uses `tracing` with `RUST_LOG` env-filter for diagnostics

## Installation

### Prerequisites

- Rust 1.70+
- GTK4 (with layer-shell support for the popup)
- An OpenWeatherMap API key (get one free at [openweathermap.org](https://openweathermap.org/api))

### Build from Source

```bash
git clone https://github.com/jshauns81/redweather
cd redweather
cargo build --release
sudo cp target/release/redweather /usr/local/bin/
```

## Setup

### 1. Add your API key

```bash
mkdir -p ~/.config/redweather
echo "YOUR_API_KEY_HERE" > ~/.config/redweather/apikey
```

Or set the environment variable:
```bash
export OWM_API_KEY="YOUR_API_KEY_HERE"
```

### 2. Set your home location

On first run, redweather will prompt you to set your location:

```bash
redweather --prompt
```

Enter your ZIP code or city name (e.g., "10001" or "New York, NY"), click Check, then Save as a preset (e.g., "home").

## Configuration

Create `~/.config/redweather/config.toml` (see `config.toml.example` for full options):

```toml
# Temperature and speed units
units = "imperial"  # or "metric"

# Multiple location presets
[[location_presets]]
name = "home"
lat = 40.7128
lon = -74.0060
label = "New York, NY"

[[location_presets]]
name = "work"
lat = 34.0522
lon = -118.2437
label = "Los Angeles, CA"

# Set active location
active_preset = "home"

# UI customization
[ui]
font_size = 9
max_desc_len = 10

[colors]
header = "#f4b8e4"
text = "#ffffff"

# Dashboard settings
[dashboard]
show_hourly_graph = true
forecast_hours = 24
forecast_days = 5         # min 5, max 12
window_width = 500
window_height = 700
# max_window_height = 900 # optional cap
```

### Switching Between Locations

The settings window (accessible from the dashboard or via `--prompt`) offers two modes:

**Mode 1: Switch to an existing preset**
1. Select from "Saved locations" dropdown
2. Click Save

**Mode 2: Search and save a new preset**
1. Enter ZIP or city name
2. Click Check
3. Check "Save as preset"
4. Enter preset name (e.g., "vacation", "office")
5. Click Save (adds to config.toml presets and sets active)

Preset names can be new or existing (overwrites).

## Waybar Integration

Add to your Waybar config (`~/.config/waybar/config`):

```jsonc
"custom/weather": {
    "return-type": "json",
    "exec": "redweather",
    "interval": 900,
    "tooltip": true,
    "on-click": "redweather --dashboard",
    "on-click-right": "redweather --popup"
}
```

**Interactions:**
- **Left Click**: Open the full GTK4 weather dashboard
- **Right Click**: Open the lightweight layer-shell popup

> **Tip:** If you set the API key via environment variable, pass it through in `exec`:
> ```jsonc
> "exec": "OWM_API_KEY=$OWM_API_KEY redweather"
> ```

## Usage

### Display Weather (Waybar JSON output)
```bash
redweather
```

### Open Dashboard
```bash
redweather --dashboard
```
Full GTK4 window with current conditions, gauges (humidity, UV, wind compass), an animated hourly temperature/precipitation graph, multi-day forecasts, a sun arc widget, and weather alerts.

### Open Popup
```bash
redweather --popup
```
Lightweight layer-shell overlay anchored to the top of the screen. Closes on Escape or focus loss.

### Set/Change Location
```bash
redweather --prompt
```

### Open in Browser
```bash
redweather --open-web
```
Opens the OpenWeatherMap website for your current location.

### Reload Weather (Bypass Cache)
```bash
redweather --reload
```
Forces a fresh API call, ignoring cached data. Useful after changing config.toml settings.

### Use Specific ZIP Code (One-Time)
```bash
redweather 10001
```

## Location Priority

Redweather resolves your location in this order:

1. Command-line ZIP argument (`redweather 10001`)
2. Active location preset from config (`active_preset`)
3. First preset in config if `active_preset` is unset
4. Prompt to configure if none are set

## Features in Detail

### 🖥️ Dashboard

The dashboard (`--dashboard`) is a full GTK4 application window featuring:
- **Current conditions** — temperature, feels-like, description, icon
- **Arc gauges** — humidity and UV index with gradient fills and render caching
- **Wind compass** — cardinal direction gauge
- **Hourly graph** — animated temperature ribbon and precipitation bars with offscreen frame buffering
- **Multi-day forecast** — configurable 5–12 day outlook
- **Sun arc** — semi-circle widget showing the sun's current position between sunrise and sunset
- **Weather alerts** — displayed when present in the API response

### 🪟 Popup

The popup (`--popup`) is a `gtk4-layer-shell` overlay that:
- Anchors to the top edge of the screen (Overlay layer)
- Shows current conditions, hourly, and daily summaries
- Closes on Escape key or focus loss

### 🔄 Error Handling & Retries

- 3 automatic retry attempts with exponential backoff (500 ms base delay)
- Falls back to stale cache if the API is unreachable
- Structured error messages via `tracing`

### ⚡ Performance

- Shared HTTP client (reused across requests)
- 10-minute cache to reduce API calls
- Async/await via Tokio for non-blocking requests
- Render caching for gauge and graph widgets (avoids redundant gradient repaints on resize)

### 🎨 Temperature Color Bands

Customize temperature colors in your config:

```toml
[[temp_bands]]
max = 75.0
color = "#81c8be"  # Temps ≤ 75°F use this color
```

### 📊 Logging

RedWeather uses `tracing` with `tracing-subscriber` for structured logging. Control the log level with the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug redweather          # Verbose output to stderr
RUST_LOG=warn redweather           # Only warnings and errors (default)
```

## Module Structure

```
src/
├── main.rs         — Entry point, CLI dispatch, cache fallback logic
├── config.rs       — Configuration loading, migration, and serialization
├── weather.rs      — OpenWeatherMap API calls, geocoding, and caching
├── formatting.rs   — Pango markup formatting for Waybar tooltip
├── dashboard.rs    — Full GTK4 dashboard window
├── popup.rs        — Layer-shell popup overlay
├── ui.rs           — Settings / location prompt dialog
├── astro.rs        — Sun arc widget (Cairo drawing)
├── gauges.rs       — Arc and compass gauge widgets (Cairo, cached)
├── graph.rs        — Hourly temperature/precipitation graph (Cairo, animated)
└── utils.rs        — Helpers: icons, colors, time formatting, moon phase
```

## Troubleshooting

### "Missing OWM_API_KEY"
Add your API key to `~/.config/redweather/apikey` or set the `OWM_API_KEY` environment variable.

### "No location configured"
Run `redweather --prompt` to set your home location.

### Stale Data Warning
If you see "Using stale cache" in logs, check your internet connection. The widget will continue showing cached data until connectivity returns.

### Dashboard Won't Launch
Ensure GTK4 and `gtk4-layer-shell` are installed. On Arch Linux: `pacman -S gtk4 gtk4-layer-shell`.

### Debugging
Run with `RUST_LOG=debug` to see detailed request/response logging on stderr.

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.

## Credits

Built with:
- [reqwest](https://github.com/seanmonstar/reqwest) — HTTP client
- [GTK4](https://gtk.org/) — UI framework
- [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell) — Wayland layer-shell integration
- [tokio](https://tokio.rs/) — Async runtime
- [serde](https://serde.rs/) — Serialization
- [tracing](https://github.com/tokio-rs/tracing) — Structured diagnostics
- [Cairo](https://www.cairographics.org/) — 2D graphics (gauges, graphs, sun arc)
