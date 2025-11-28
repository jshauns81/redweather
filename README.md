# RedWeather 🌦️

A beautiful, customizable weather widget for Waybar using the OpenWeatherMap API.

## Features

✨ **Smart First-Run Setup** - Automatically prompts for location on first use (no hardcoded defaults!)
🌡️ **Unit Switching** - Toggle between Imperial (°F, mph) and Metric (°C, m/s)
📍 **Multiple Locations** - Save presets for home, work, vacation, etc.
🔄 **Retry Logic** - Exponential backoff with 3 retry attempts for reliability
💾 **Graceful Degradation** - Falls back to stale cache during network issues
🎨 **Customizable Colors** - Temperature bands and UI colors
⚡ **Performance** - Shared HTTP client and efficient caching
🔒 **Type-Safe** - Well-structured modules with comprehensive documentation

## Installation

### Prerequisites

- Rust 1.70+
- GTK4
- An OpenWeatherMap API key (get one free at [openweathermap.org](https://openweathermap.org/api))

### Build from Source

```bash
git clone https://github.com/yourusername/redweather
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

On first run, redweather will automatically prompt you to set your location:

```bash
redweather --prompt
```

Enter your ZIP code or city name (e.g., "10001" or "New York, NY"), click Check, then Save.

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
```

## Waybar Integration

Add to your Waybar config (`~/.config/waybar/config`):

```json
"custom/weather": {
    "exec": "~/.local/bin/redweather",
    "return-type": "json",
    "interval": 600,
    "on-click": "redweather --prompt",
    "tooltip": true
}
```

## Usage

### Display Weather
```bash
redweather
```

### Set/Change Location
```bash
redweather --prompt
```

### Use Specific ZIP Code (One-Time)
```bash
redweather 10001
```

## Location Priority

Redweather resolves your location in this order:

1. Command-line ZIP argument (`redweather 10001`)
2. Active location preset from config
3. Legacy single location from config
4. Saved home location (~/.config/redweather/home_location.json)
5. First-run prompt if none of the above

## Features in Detail

### 🔄 Error Handling & Retries

- 3 automatic retry attempts with exponential backoff
- Falls back to stale cache if API is unreachable
- Clear error messages in tooltip

### ⚡ Performance

- Shared HTTP client (reused across requests)
- 10-minute cache to reduce API calls
- Async/await for non-blocking requests

### 🎨 Temperature Color Bands

Customize temperature colors in your config:

```toml
[[temp_bands]]
max = 75.0
color = "#81c8be"  # Temps ≤ 75°F use this color
```

## Module Structure

```
src/
├── main.rs         - Entry point & orchestration
├── config.rs       - Configuration & settings
├── weather.rs      - API calls & caching
├── formatting.rs   - Display formatting
├── ui.rs           - GTK prompt window
└── utils.rs        - Helper functions
```

## Troubleshooting

### "Missing OWM_API_KEY"
Add your API key to `~/.config/redweather/apikey` or set the `OWM_API_KEY` environment variable.

### "No location configured"
Run `redweather --prompt` to set your home location.

### Stale Data Warning
If you see "Using stale cache" in logs, check your internet connection. The widget will continue showing cached data until connectivity returns.

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.

## Credits

Built with:
- [reqwest](https://github.com/seanmonstar/reqwest) - HTTP client
- [GTK4](https://gtk.org/) - UI framework
- [tokio](https://tokio.rs/) - Async runtime
- [serde](https://serde.rs/) - Serialization
