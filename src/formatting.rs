//! Weather data formatting for display
//!
//! This module handles formatting weather data into text and tooltip displays
//! for Waybar integration.

use crate::config::{ColorsResolved, TempBand, UiConfigResolved, Units};
use crate::utils::{
    deg_to_dir, escape_pango, fmt_time, is_night, moon_phase_icon, pick_icon, short_desc,
    sparkline, temp_color, uvi_color,
};
use crate::weather::{ApiResponse, Daily, Hourly, WeatherDesc};

/// Formats the current weather section
pub fn format_current_weather(
    data: &ApiResponse,
    location_label: &str,
    temp_bands: &[TempBand],
    units: Units,
) -> (String, Vec<String>) {
    let (temp_unit, speed_unit, dist_unit) = match units {
        Units::Imperial => ("°F", "mph", "mi"),
        Units::Metric => ("°C", "m/s", "km"),
    };
    let current_desc = data.current.weather.get(0).cloned().unwrap_or(WeatherDesc {
        main: Some("Clear".into()),
        description: Some("Clear".into()),
    });
    let night = is_night(data.current.dt, data.current.sunrise, data.current.sunset);
    let moon_icon = Some(moon_phase_icon(data.current.dt, data.timezone_offset));
    let icon = pick_icon(&current_desc, night, moon_icon);
    let temp = data.current.temp.round();
    let feels = data.current.feels_like.map(|t| t.round());
    let humidity = data.current.humidity.unwrap_or(0);
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_dir = deg_to_dir(data.current.wind_deg);
    let uvi = data.current.uvi.unwrap_or(0.0);
    let pressure = data.current.pressure.unwrap_or(0);
    let vis_meters = data.current.visibility.unwrap_or(10000);
    let visibility = match units {
        Units::Imperial => (vis_meters as f64 / 1609.34).round(),
        Units::Metric => (vis_meters as f64 / 1000.0).round(),
    };
    let pop = data
        .hourly
        .get(0)
        .and_then(|h| h.pop)
        .map(|p| (p * 100.0).round() as i64)
        .unwrap_or(0);

    let sunrise = data
        .current
        .sunrise
        .map(|t| fmt_time(t, data.timezone_offset, "%H:%M"))
        .unwrap_or_else(|| "-:-".into());
    let sunset = data
        .current
        .sunset
        .map(|t| fmt_time(t, data.timezone_offset, "%H:%M"))
        .unwrap_or_else(|| "-:-".into());

    let temp_col = temp_color(temp, temp_bands);
    let text = format!(
        "| {} <span foreground='{}'>{:.0}{}</span>",
        icon, temp_col, temp, temp_unit
    );

    let safe_loc = escape_pango(location_label);
    let safe_desc = escape_pango(current_desc.description.as_deref().unwrap_or("N/A"));

    let mut current_lines = vec![
        format!("🌍 <b>{}</b>", safe_loc),
        format!(
            "{} <span foreground='{tc}' size='large'>{:.0}{}</span>  {}",
            icon,
            temp,
            temp_unit,
            safe_desc,
            tc = temp_col
        ),
    ];

    let feels_str = feels
        .map(|f| format!("{:.0}°", f))
        .unwrap_or_else(|| "—".into());

    // Row 1: Feels like, Humidity, UV
    current_lines.push(format!(
        "Feels like {} • Hum {}% • UV <span foreground='{}'>{}</span>",
        feels_str,
        humidity,
        uvi_color(uvi),
        uvi.round()
    ));

    // Row 2: Wind, Precip
    current_lines.push(format!(
        "Wind {:.0} {} {} • Rain {}%",
        wind_speed, speed_unit, wind_dir, pop
    ));

    // Row 3: Astro & Atmos
    current_lines.push(format!(
        "🌅 {}  🌇 {} • 👁️ {:.0}{} • 🌪️ {}hPa",
        sunrise, sunset, visibility, dist_unit, pressure
    ));

    (text, current_lines)
}

/// Formats hourly forecast lines
pub fn format_hourly_forecast(
    hourly: &[Hourly],
    timezone_offset: i64,
    ui: &UiConfigResolved,
    temp_bands: &[TempBand],
) -> Vec<String> {
    hourly
        .iter()
        .take(12)
        .map(|h| {
            let label = fmt_time(h.dt, timezone_offset, "%-I%p");
            let h_temp = h.temp.round();
            let local_hour = ((h.dt + timezone_offset) / 3600) % 24;
            let night = local_hour < 6 || local_hour >= 18;
            let icon_h = h
                .weather
                .get(0)
                .map(|d| pick_icon(d, night, Some(moon_phase_icon(h.dt, timezone_offset))))
                .unwrap_or_else(|| "❓".into());
            let pop = h.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
            let precip_str = if pop > 0 {
                format!("💧{}%", pop)
            } else {
                "".to_string()
            };

            let raw_desc = h
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            let desc = escape_pango(&raw_desc);

            format!(
                "{: <4} <span foreground='{temp_col}' font='{fs}'>{:.0}°</span> {icon} {: <12} {}",
                label,
                h_temp,
                desc,
                precip_str,
                temp_col = temp_color(h_temp, temp_bands),
                fs = ui.font_size,
                icon = icon_h,
            )
        })
        .collect()
}

/// Formats daily forecast lines
pub fn format_daily_forecast(
    daily: &[Daily],
    timezone_offset: i64,
    ui: &UiConfigResolved,
    temp_bands: &[TempBand],
) -> Vec<String> {
    daily
        .iter()
        .take(5)
        .map(|d| {
            let day = fmt_time(d.dt, timezone_offset, "%a");
            let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
            let lo = d.temp.min.unwrap_or(0.0).round();
            let icon_d = d
                .weather
                .get(0)
                .map(|desc| pick_icon(desc, false, None))
                .unwrap_or_else(|| "❓".into());
            let pop = d.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
            let precip_str = if pop > 0 {
                format!("💧{}%", pop)
            } else {
                "".to_string()
            };

            let raw_desc = d
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            let desc = escape_pango(&raw_desc);

            format!(
                "{: <3} <span foreground='{hi_col}' font='{fs}'>{:.0}°</span>/<span foreground='{lo_col}' font='{fs}'>{:.0}°</span> {icon} {: <12} {}",
                day,
                hi,
                lo,
                desc,
                precip_str,
                hi_col = temp_color(hi, temp_bands),
                lo_col = temp_color(lo, temp_bands),
                fs = ui.font_size,
                icon = icon_d,
            )
        })
        .collect()
}

/// Wraps lines with color and font styling
fn wrap_with_style(lines: Vec<String>, color: &str, font_size: u8) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| format!("<span foreground='{color}' font='{font_size}'>{l}</span>"))
        .collect()
}

/// Formats complete popup text and tooltip for Waybar
pub fn format_popup_text(
    data: &ApiResponse,
    location_label: &str,
    ui: &UiConfigResolved,
    colors: &ColorsResolved,
    temp_bands: &[TempBand],
    units: Units,
) -> (String, String) {
    let (text, current_lines) = format_current_weather(data, location_label, temp_bands, units);
    let hourly_lines = format_hourly_forecast(&data.hourly, data.timezone_offset, ui, temp_bands);
    let daily_lines = format_daily_forecast(&data.daily, data.timezone_offset, ui, temp_bands);

    // Generate Sparkline
    let hourly_temps: Vec<f64> = data.hourly.iter().take(12).map(|h| h.temp).collect();
    let spark = sparkline(&hourly_temps);

    // Build compact tooltip: stacked sections
    let mut tooltip_lines = Vec::new();

    // Header is built into current_lines now, but let's keep section structure
    tooltip_lines.extend(wrap_with_style(current_lines, &colors.text, ui.font_size));
    tooltip_lines.push(String::new());

    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>HOURS {spark}</span>",
        hdr = colors.header,
        fs = ui.font_size,
        spark = spark
    ));
    tooltip_lines.extend(wrap_with_style(hourly_lines, &colors.text, ui.font_size));
    tooltip_lines.push(String::new());

    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>DAYS</span>",
        hdr = colors.header,
        fs = ui.font_size
    ));
    tooltip_lines.extend(wrap_with_style(daily_lines, &colors.text, ui.font_size));

    (text, tooltip_lines.join("\n"))
}
