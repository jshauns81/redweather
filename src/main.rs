use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Button, Entry, Label, Orientation};
use reqwest::blocking::Client;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use toml;

const DEFAULT_LAT: f64 = 37.545;
const DEFAULT_LON: f64 = -97.268;
const DEFAULT_LOCATION_NAME: &str = "Derby, KS";
const CACHE_MAX_AGE_SECS: i64 = 600; // 10 minutes
const CACHE_FILE: &str = ".cache/redweather";
const ZIP_OVERRIDE_FILE: &str = ".cache/redweather/zip_override";
const KEY_FILE: &str = ".config/redweather/apikey";
const CONFIG_FILE: &str = ".config/redweather/config.toml";

#[derive(Debug, Clone)]
struct Location {
    lat: f64,
    lon: f64,
    label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct WeatherDesc {
    main: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Current {
    dt: i64,
    temp: f64,
    feels_like: Option<f64>,
    humidity: Option<u8>,
    wind_speed: Option<f64>,
    wind_deg: Option<i64>,
    weather: Vec<WeatherDesc>,
    #[serde(default)]
    rain: Option<HashMap<String, f64>>,
    #[serde(default)]
    snow: Option<HashMap<String, f64>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Hourly {
    dt: i64,
    temp: f64,
    pop: Option<f64>,
    wind_speed: Option<f64>,
    wind_deg: Option<i64>,
    weather: Vec<WeatherDesc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TempRange {
    day: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Daily {
    dt: i64,
    temp: TempRange,
    pop: Option<f64>,
    weather: Vec<WeatherDesc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ApiResponse {
    timezone_offset: i64,
    current: Current,
    hourly: Vec<Hourly>,
    daily: Vec<Daily>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CacheFile {
    fetched_at: i64,
    data: ApiResponse,
}

#[derive(Debug, Clone, Deserialize)]
struct TempBand {
    max: f64,
    color: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UiConfig {
    font_size: Option<u8>,
    max_desc_len: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct ColorsConfig {
    header: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LocationConfig {
    lat: Option<f64>,
    lon: Option<f64>,
    label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    location: Option<LocationConfig>,
    ui: Option<UiConfig>,
    colors: Option<ColorsConfig>,
    temp_bands: Option<Vec<TempBand>>,
}

#[derive(Debug, Clone)]
struct UiConfigResolved {
    font_size: u8,
    max_desc_len: usize,
}

impl UiConfigResolved {
    fn from_config(ui: &Option<UiConfig>) -> Self {
        UiConfigResolved {
            font_size: ui.as_ref().and_then(|u| u.font_size).unwrap_or(9),
            max_desc_len: ui.as_ref().and_then(|u| u.max_desc_len).unwrap_or(10),
        }
    }
}

#[derive(Debug, Clone)]
struct ColorsResolved {
    header: String,
    text: String,
}

impl ColorsResolved {
    fn from_config(c: &Option<ColorsConfig>) -> Self {
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
    fn from_config(b: &Option<Vec<TempBand>>) -> Vec<TempBand> {
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

fn load_key() -> Option<String> {
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

fn load_config() -> Config {
    let home = match env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Config { location: None, ui: None, colors: None, temp_bands: None },
    };
    let path = PathBuf::from(home).join(CONFIG_FILE);
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Config { location: None, ui: None, colors: None, temp_bands: None },
    };
    toml::from_str(&contents).unwrap_or(Config { location: None, ui: None, colors: None, temp_bands: None })
}

fn cache_path(cache_key: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let safe_key = cache_key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    PathBuf::from(home).join(format!("{}/cache_{}.json", CACHE_FILE, safe_key))
}

fn load_cache(cache_key: &str) -> Option<ApiResponse> {
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

fn save_cache(cache_key: &str, data: &ApiResponse) {
    let path = cache_path(cache_key);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let cache = CacheFile {
        fetched_at: Utc::now().timestamp(),
        data: data.clone(),
    };
    let _ = fs::write(path, serde_json::to_string(&cache).unwrap_or_default());
}

fn load_zip_override() -> Option<String> {
    let home = env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(ZIP_OVERRIDE_FILE);
    let contents = fs::read_to_string(path).ok()?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn save_zip_override(raw: &str) {
    if raw.trim().is_empty() {
        return;
    }
    if let Ok(home) = env::var("HOME") {
        let path = PathBuf::from(home).join(ZIP_OVERRIDE_FILE);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, raw.trim());
    }
}

fn reload_waybar() {
    let _ = Command::new("pkill").arg("-SIGUSR2").arg("waybar").status();
}

fn deg_to_dir(deg: Option<i64>) -> String {
    let d = deg.unwrap_or(0) as f64;
    let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((d + 22.5) / 45.0).floor() as usize) % 8;
    dirs[idx].to_string()
}

fn pick_icon(desc: &WeatherDesc) -> &'static str {
    let main = desc.main.as_deref().unwrap_or("").to_lowercase();
    let full = desc.description.as_deref().unwrap_or("").to_lowercase();
    if main.contains("thunder") || full.contains("thunder") {
        "⛈️"
    } else if main.contains("snow") || full.contains("snow") || main.contains("sleet") {
        "❄️"
    } else if main.contains("rain") || full.contains("rain") || full.contains("drizzle") {
        "🌧️"
    } else if full.contains("fog") || full.contains("mist") || full.contains("haze") {
        "🌫️"
    } else if main.contains("cloud") || full.contains("cloud") || full.contains("overcast") {
        "☁️"
    } else if main.contains("clear") || full.contains("clear") || full.contains("sun") {
        "☀️"
    } else {
        "❓"
    }
}

fn temp_color(temp: f64, bands: &[TempBand]) -> String {
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

fn fmt_time(dt: i64, tz_offset: i64, fmt: &str) -> String {
    let naive = NaiveDateTime::from_timestamp_opt(dt + tz_offset, 0)
        .unwrap_or_else(|| NaiveDateTime::from_timestamp_opt(0, 0).unwrap());
    let dt = DateTime::<Utc>::from_utc(naive, Utc);
    dt.format(fmt).to_string()
}

fn short_desc(desc: &str, max_len: usize) -> String {
    let mut d = desc.trim().to_string();
    if d.len() > max_len {
        d.truncate(max_len);
    }
    d
}

fn visible_len(s: &str) -> usize {
    let mut in_tag = false;
    let mut count = 0;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => count += 1,
            _ => {}
        }
    }
    count
}

fn format_popup_text(
    data: &ApiResponse,
    location_label: &str,
    ui: &UiConfigResolved,
    colors: &ColorsResolved,
    temp_bands: &[TempBand],
) -> (String, String) {
    let current_desc = data
        .current
        .weather
        .get(0)
        .cloned()
        .unwrap_or(WeatherDesc {
            main: Some("Clear".into()),
            description: Some("Clear".into()),
        });
    let icon = pick_icon(&current_desc);
    let temp = data.current.temp.round();
    let feels = data.current.feels_like.map(|t| t.round());
    let humidity = data.current.humidity.unwrap_or(0);
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_dir = deg_to_dir(data.current.wind_deg);
    let pop = data
        .hourly
        .get(0)
        .and_then(|h| h.pop)
        .map(|p| (p * 100.0).round() as i64)
        .unwrap_or(0);

    let temp_col = temp_color(temp, temp_bands);
    let text = format!("| {} <span foreground='{}'>{:.0}°F</span>", icon, temp_col, temp);

    let mut current_lines = vec![
        format!("🌍 {}", location_label),
        format!(
            "{} <span foreground='{tc}'>{:.0}°F</span> — {}",
            icon,
            temp,
            current_desc.description.unwrap_or_else(|| "N/A".into()),
            tc = temp_col
        ),
    ];
    let feels_str = feels.map(|f| format!("Feels {:.0}°", f)).unwrap_or_else(|| "Feels —".into());
    let meta_top = format!("{} • Hum {}%", feels_str, humidity);
    let meta_bottom = format!("Wind {:.0} mph {} • Precip {}%", wind_speed, wind_dir, pop);
    current_lines.push(meta_top);
    current_lines.push(meta_bottom);

    // Hourly lines (next 5, stacked)
    let mut hourly_lines = Vec::new();
    for h in data.hourly.iter().take(12) {
        let label = fmt_time(h.dt, data.timezone_offset, "%-I%p");
        let h_temp = h.temp.round();
        let icon_h = h.weather.get(0).map(pick_icon).unwrap_or("❓");
        let desc = h
            .weather
            .get(0)
            .and_then(|w| w.description.as_ref())
            .map(|s| short_desc(s, ui.max_desc_len))
            .unwrap_or_else(|| "—".into());
        hourly_lines.push(format!(
            "{} <span foreground='{temp_col}' font='{fs}'>{:.0}°</span> {icon} {desc}",
            label,
            h_temp,
            temp_col = temp_color(h_temp, temp_bands),
            fs = ui.font_size,
            icon = icon_h,
            desc = desc
        ));
    }

    // Daily lines (next 5 days, stacked)
    let mut daily_lines = Vec::new();
    for d in data.daily.iter().take(5) {
        let day = fmt_time(d.dt, data.timezone_offset, "%a");
        let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
        let lo = d.temp.min.unwrap_or(0.0).round();
        let icon_d = d.weather.get(0).map(pick_icon).unwrap_or("❓");
        let desc = d
            .weather
            .get(0)
            .and_then(|w| w.description.as_ref())
            .map(|s| short_desc(s, ui.max_desc_len))
            .unwrap_or_else(|| "—".into());
        daily_lines.push(format!(
            "{} <span foreground='{hi_col}' font='{fs}'>{:.0}°</span>/<span foreground='{lo_col}' font='{fs}'>{:.0}°</span> {icon} {desc}",
            day,
            hi,
            lo,
            hi_col = temp_color(hi, temp_bands),
            lo_col = temp_color(lo, temp_bands),
            fs = ui.font_size,
            icon = icon_d,
            desc = desc
        ));
    }

    // Build compact tooltip: stacked sections
    let mut tooltip_lines = Vec::new();
    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>NOW</span>",
        hdr = colors.header,
        fs = ui.font_size
    ));
    tooltip_lines.extend(current_lines.into_iter().map(|l| format!(
        "<span foreground='{txt}' font='{fs}'>{}</span>",
        l,
        txt = colors.text,
        fs = ui.font_size
    )));
    tooltip_lines.push(String::new());

    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>HOURS</span>",
        hdr = colors.header,
        fs = ui.font_size
    ));
    tooltip_lines.extend(hourly_lines.into_iter().map(|l| format!(
        "<span foreground='{txt}' font='{fs}'>{}</span>",
        l,
        txt = colors.text,
        fs = ui.font_size
    )));
    tooltip_lines.push(String::new());

    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>DAYS</span>",
        hdr = colors.header,
        fs = ui.font_size
    ));
    tooltip_lines.extend(daily_lines.into_iter().map(|l| format!(
        "<span foreground='{txt}' font='{fs}'>{}</span>",
        l,
        txt = colors.text,
        fs = ui.font_size
    )));

    (text, tooltip_lines.join("\n"))
}

fn fetch_weather_for_loc(key: &str, loc: &Location) -> Result<ApiResponse> {
    let client = Client::new();
    let mut url = Url::parse("https://api.openweathermap.org/data/3.0/onecall")?;
    url.query_pairs_mut()
        .append_pair("lat", &loc.lat.to_string())
        .append_pair("lon", &loc.lon.to_string())
        .append_pair("appid", key)
        .append_pair("units", "imperial")
        .append_pair("exclude", "minutely,alerts");
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .context("request failed")?
        .error_for_status()
        .context("bad status")?;
    let parsed: ApiResponse = resp.json().context("invalid json")?;
    Ok(parsed)
}

fn resolve_location(key: &str, zip: Option<&str>, cfg: &Config) -> Location {
    if let Some(z) = zip {
        if let Some(loc) = geocode_zip(key, z).or_else(|| geocode_direct(key, z)) {
            return loc;
        }
    }
    if let Some(loc_cfg) = cfg.location.as_ref() {
        if let (Some(lat), Some(lon)) = (loc_cfg.lat, loc_cfg.lon) {
            return Location {
                lat,
                lon,
                label: loc_cfg
                    .label
                    .clone()
                    .unwrap_or_else(|| DEFAULT_LOCATION_NAME.to_string()),
            };
        }
    }
    Location {
        lat: DEFAULT_LAT,
        lon: DEFAULT_LON,
        label: DEFAULT_LOCATION_NAME.to_string(),
    }
}

fn geocode_zip(key: &str, zip: &str) -> Option<Location> {
    let client = Client::new();
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/zip").ok()?;
    let zip_param = if zip.contains(',') {
        zip.to_string()
    } else {
        format!("{},US", zip)
    };
    url.query_pairs_mut()
        .append_pair("zip", &zip_param)
        .append_pair("appid", key);
    let resp = client.get(url).timeout(std::time::Duration::from_secs(10)).send().ok()?;
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
    let zr: ZipResp = resp.json().ok()?;
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

fn geocode_direct(key: &str, query: &str) -> Option<Location> {
    let client = Client::new();
    let mut url = Url::parse("https://api.openweathermap.org/geo/1.0/direct").ok()?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", "1")
        .append_pair("appid", key);
    let resp = client.get(url).timeout(std::time::Duration::from_secs(10)).send().ok()?;
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
    let list: Vec<DirResp> = resp.json().ok()?;
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

fn run_prompt(key: &str) -> Result<()> {
    let key = key.to_string();
    let app = Application::builder()
        .application_id("com.shaun.redweather.prompt")
        .build();

    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Set Weather Location")
            .default_width(360)
            .default_height(180)
            .resizable(false)
            .build();

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(10);
        vbox.set_margin_bottom(10);
        vbox.set_margin_start(10);
        vbox.set_margin_end(10);

        let row = GtkBox::new(Orientation::Horizontal, 6);
        let label = Label::new(Some("ZIP or city,country:"));
        let entry = Entry::new();
        entry.set_hexpand(true);
        row.append(&label);
        row.append(&entry);

        let status = Label::new(Some("Enter location and press Check"));
        status.set_halign(gtk::Align::Start);
        let result = Label::new(None);
        result.set_halign(gtk::Align::Start);
        result.add_css_class("title-4");

        let buttons = GtkBox::new(Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        let check_btn = Button::with_label("Check");
        let save_btn = Button::with_label("Save");
        save_btn.set_sensitive(false);
        let cancel_btn = Button::with_label("Cancel");
        buttons.append(&check_btn);
        buttons.append(&save_btn);
        buttons.append(&cancel_btn);

        vbox.append(&row);
        vbox.append(&status);
        vbox.append(&result);
        vbox.append(&buttons);
        window.set_child(Some(&vbox));

        let current_raw: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let entry_check = entry.clone();
        let status_check = status.clone();
        let result_check = result.clone();
        let save_check = save_btn.clone();
        let key_check = key.clone();
        let current_for_check = current_raw.clone();
        check_btn.connect_clicked(move |_| {
            let q = entry_check.text().trim().to_string();
            result_check.set_text("");
            save_check.set_sensitive(false);
            if q.is_empty() {
                status_check.set_text("Enter a ZIP or city");
                return;
            }
            let info = geocode_zip(&key_check, &q).or_else(|| geocode_direct(&key_check, &q));
            match info {
                Some(loc) => {
                    result_check.set_text(&format!("→ {}", loc.label));
                    status_check.set_text("OK");
                    *current_for_check.borrow_mut() = Some(q.clone());
                    save_check.set_sensitive(true);
                }
                None => {
                    status_check.set_text("No result");
                }
            }
        });

        let current_save = current_raw.clone();
        let window_for_save = window.clone();
        save_btn.connect_clicked(move |_| {
            if let Some(raw) = current_save.borrow().as_ref() {
                save_zip_override(raw);
                reload_waybar();
            }
            window_for_save.close();
        });

        let entry_return = entry.clone();
        let status_return = status.clone();
        let result_return = result.clone();
        let save_return = save_btn.clone();
        let key_return = key.clone();
        let current_for_return = current_raw.clone();
        entry.connect_activate(move |_| {
            let q = entry_return.text().trim().to_string();
            result_return.set_text("");
            save_return.set_sensitive(false);
            if q.is_empty() {
                status_return.set_text("Enter a ZIP or city");
                return;
            }
            let info = geocode_zip(&key_return, &q).or_else(|| geocode_direct(&key_return, &q));
            match info {
                Some(loc) => {
                    result_return.set_text(&format!("→ {}", loc.label));
                    status_return.set_text("OK");
                    *current_for_return.borrow_mut() = Some(q.clone());
                    save_return.set_sensitive(true);
                }
                None => {
                    status_return.set_text("No result");
                }
            }
        });

        let window_for_cancel = window.clone();
        cancel_btn.connect_clicked(move |_| {
            window_for_cancel.close();
        });

        window.show();
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}

fn main() {
    let args = env::args().collect::<Vec<String>>();
    let prompt_mode = args.iter().any(|a| a == "--prompt");
    if prompt_mode {
        if let Some(k) = load_key() {
            let _ = run_prompt(&k);
        } else {
            eprintln!("Missing OWM_API_KEY (env or ~/.config/redweather/apikey)");
        }
        return;
    }

    let zip_arg = args.get(1).cloned().filter(|s| !s.trim().is_empty());
    let zip_override = load_zip_override();
    let zip_choice = zip_arg.as_deref().or(zip_override.as_deref());
    let cfg = load_config();
    let key = match load_key() {
        Some(k) => k,
        None => {
            let fallback = json!({
                "text": "| ❓ N/A",
                "tooltip": "<span foreground='#f4b8e4'>Missing OWM_API_KEY (env or ~/.config/redweather/apikey)</span>",
                "markup": "pango"
            });
            println!("{}", fallback);
            return;
        }
    };

    let loc = resolve_location(&key, zip_choice, &cfg);
    let cache_key = format!("{:.3}_{:.3}", loc.lat, loc.lon);

    let data = match load_cache(&cache_key) {
        Some(cached) => cached,
        None => match fetch_weather_for_loc(&key, &loc) {
            Ok(d) => {
                save_cache(&cache_key, &d);
                d
            }
            Err(e) => {
                if let Some(cached) = load_cache(&cache_key) {
                    cached
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

    let (text, tooltip) = format_popup_text(&data, &loc.label, &ui_resolved, &colors_resolved, &bands);
    let out = json!({
        "text": text,
        "tooltip": tooltip,
        "markup": "pango"
    });
    println!("{}", out);
}
