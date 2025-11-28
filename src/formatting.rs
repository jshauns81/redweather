//! Weather data formatting for display
//!
//! This module handles formatting weather data into text and tooltip displays
//! for Waybar integration.

use crate::config::{ColorsResolved, TempBand, UiConfigResolved, Units};
use crate::utils::{deg_to_dir, fmt_time, pick_icon, short_desc, temp_color};
use crate::weather::{ApiResponse, Daily, Hourly, WeatherDesc};

/// Formats the current weather section
pub fn format_current_weather(
    data: &ApiResponse,
    location_label: &str,
    temp_bands: &[TempBand],
    units: Units,
) -> (String, Vec<String>) {
    let (temp_unit, speed_unit) = match units {
        Units::Imperial => ("°F", "mph"),
        Units::Metric => ("°C", "m/s"),
    };
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
    let text = format!("| {} <span foreground='{}'>{:.0}{}</span>", icon, temp_col, temp, temp_unit);

    let mut current_lines = vec![
        format!("🌍 {}", location_label),
        format!(
            "{} <span foreground='{tc}'>{:.0}{}</span> — {}",
            icon,
            temp,
            temp_unit,
            current_desc.description.unwrap_or_else(|| "N/A".into()),
            tc = temp_col
        ),
    ];
    let feels_str = feels.map(|f| format!("Feels {:.0}°", f)).unwrap_or_else(|| "Feels —".into());
    let meta_top = format!("{} • Hum {}%", feels_str, humidity);
    let meta_bottom = format!("Wind {:.0} {} {} • Precip {}%", wind_speed, speed_unit, wind_dir, pop);
    current_lines.push(meta_top);
    current_lines.push(meta_bottom);

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
            let icon_h = h.weather.get(0).map(pick_icon).unwrap_or("❓");
            let desc = h
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            format!(
                "{} <span foreground='{temp_col}' font='{fs}'>{:.0}°</span> {icon} {desc}",
                label,
                h_temp,
                temp_col = temp_color(h_temp, temp_bands),
                fs = ui.font_size,
                icon = icon_h,
                desc = desc
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
            let icon_d = d.weather.get(0).map(pick_icon).unwrap_or("❓");
            let desc = d
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            format!(
                "{} <span foreground='{hi_col}' font='{fs}'>{:.0}°</span>/<span foreground='{lo_col}' font='{fs}'>{:.0}°</span> {icon} {desc}",
                day,
                hi,
                lo,
                hi_col = temp_color(hi, temp_bands),
                lo_col = temp_color(lo, temp_bands),
                fs = ui.font_size,
                icon = icon_d,
                desc = desc
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

    // Build compact tooltip: stacked sections
    let mut tooltip_lines = Vec::new();
    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>NOW</span>",
        hdr = colors.header,
        fs = ui.font_size
    ));
    tooltip_lines.extend(wrap_with_style(current_lines, &colors.text, ui.font_size));
    tooltip_lines.push(String::new());

    tooltip_lines.push(format!(
        "<span foreground='{hdr}' font='{fs}' weight='bold'>HOURS</span>",
        hdr = colors.header,
        fs = ui.font_size
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
