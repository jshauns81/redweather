//! Weather data formatting for display
//!
//! This module handles formatting weather data into text and tooltip displays
//! for Waybar integration.

use crate::config::{ColorsResolved, TempBand, UiConfigResolved, Units};
use crate::utils::{
    deg_to_dir, escape_pango, fmt_time, humidity_color, is_night, moon_phase_icon, pick_icon,
    short_desc, sparkline, temp_color, uvi_color,
};
use crate::weather::{ApiResponse, Daily, Hourly, WeatherDesc};

/// Total visible line width for the monospaced tooltip.
/// All columnar lines are padded/truncated to this width.
const LINE_WIDTH: usize = 34;

/// Styled section divider line (exactly LINE_WIDTH box-drawing chars)
fn divider(color: &str) -> String {
    let bar: String = std::iter::repeat('─').take(LINE_WIDTH).collect();
    format!("<span foreground='{color}'>{bar}</span>")
}

/// Formats precipitation amount (rain/snow) for display, converting units if needed.
/// OWM always returns mm regardless of unit setting.
fn fmt_precip_amount(mm: f64, units: Units) -> String {
    match units {
        Units::Imperial => format!("{:.2}in", mm * 0.03937),
        Units::Metric => format!("{:.1}mm", mm),
    }
}

/// Color helper — wraps text in a foreground color span
fn clr(text: &str, color: &str) -> String {
    format!("<span foreground='{color}'>{text}</span>")
}

/// Build a fixed-width line: left-aligned `left`, right-aligned `right`,
/// padded with spaces to `LINE_WIDTH`. Both arguments are *visible* text
/// (no markup). Returns plain text (no spans) — caller adds color as needed.
fn columns(left: &str, right: &str) -> String {
    let left_len = left.chars().count();
    let right_len = right.chars().count();
    let total = left_len + right_len;
    if total >= LINE_WIDTH {
        format!("{} {}", left, right)
    } else {
        let gap = LINE_WIDTH - total;
        let spaces: String = std::iter::repeat(' ').take(gap).collect();
        format!("{}{}{}", left, spaces, right)
    }
}

/// Like `columns` but the left side contains Pango markup.
/// `left_visible_len` is the number of visible characters in `left`
/// (excluding tags). `right` is plain text.
fn columns_markup(left: &str, left_visible_len: usize, right: &str) -> String {
    let right_len = right.chars().count();
    let total = left_visible_len + right_len;
    if total >= LINE_WIDTH {
        format!("{} {}", left, right)
    } else {
        let gap = LINE_WIDTH - total;
        let spaces: String = std::iter::repeat(' ').take(gap).collect();
        format!("{}{}{}", left, spaces, right)
    }
}

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
    let dew_point = data.current.dew_point.map(|d| d.round());
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_gust = data.current.wind_gust.map(|g| g.round());
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

    // Today's high/low from daily forecast
    let (today_hi, today_lo) = data
        .daily
        .first()
        .map(|d| {
            (
                d.temp.max.or(d.temp.day).unwrap_or(0.0).round(),
                d.temp.min.unwrap_or(0.0).round(),
            )
        })
        .unwrap_or((0.0, 0.0));

    // Rain/snow accumulation (last 1h)
    let rain_1h = data
        .current
        .rain
        .as_ref()
        .and_then(|r| r.get("1h"))
        .copied();
    let snow_1h = data
        .current
        .snow
        .as_ref()
        .and_then(|s| s.get("1h"))
        .copied();

    let temp_col = temp_color(temp, temp_bands);
    let text = format!(
        "| {} {}",
        icon,
        clr(&format!("{:.0}{}", temp, temp_unit), &temp_col)
    );

    let safe_loc = escape_pango(location_label);
    let safe_desc = escape_pango(current_desc.description.as_deref().unwrap_or("N/A"));

    let hi_col = temp_color(today_hi, temp_bands);
    let lo_col = temp_color(today_lo, temp_bands);

    // Build current-conditions block using column helpers
    let feels_val = feels.unwrap_or(temp);

    // Line 1: location
    let line1 = format!("{} <b>{}</b>", icon, safe_loc);

    // Line 2: temp + feels          description (right)
    let temp_left = format!(
        "{} feels {:.0}°",
        clr(&format!("{:.0}{}", temp, temp_unit), &temp_col),
        feels_val,
    );
    let temp_left_vis = format!("{:.0}{} feels {:.0}°", temp, temp_unit, feels_val)
        .chars()
        .count();
    let line2 = columns_markup(&temp_left, temp_left_vis, &safe_desc);

    // Line 3: hi/lo + humidity          dew + wind (right)
    let hilo = format!(
        "↕{}/{} 💧{}",
        clr(&format!("{:.0}°", today_hi), &hi_col),
        clr(&format!("{:.0}°", today_lo), &lo_col),
        clr(&format!("{}%", humidity), humidity_color(humidity)),
    );
    let hilo_vis = format!("↕{:.0}°/{:.0}° 💧{}%", today_hi, today_lo, humidity)
        .chars()
        .count();
    let dew_wind = {
        let dew_str = dew_point
            .map(|d| format!("dew {:.0}° ", d))
            .unwrap_or_default();
        let gust_str = match wind_gust {
            Some(g) if g > wind_speed + 5.0 => format!("g{:.0} ", g),
            _ => String::new(),
        };
        format!(
            "{}{:.0}{} {}{}",
            dew_str, wind_speed, speed_unit, gust_str, wind_dir
        )
    };
    let line3 = columns_markup(&hilo, hilo_vis, &dew_wind);

    // Line 4: sun times + UV        pressure + vis (right)
    let sun_left = format!(
        "☀ {}–{} UV {}",
        sunrise,
        sunset,
        clr(&format!("{}", uvi.round()), uvi_color(uvi)),
    );
    let sun_left_vis = format!("☀ {}–{} UV {}", sunrise, sunset, uvi.round())
        .chars()
        .count();
    let atmo_right = format!("{}hPa 👁{:.0}{}", pressure, visibility, dist_unit);
    let line4 = columns_markup(&sun_left, sun_left_vis, &atmo_right);

    let mut current_lines = vec![line1, line2, line3, line4];

    // Rain chance — right-aligned percentage
    if pop > 0 {
        current_lines.push(columns("🌧 chance of rain", &format!("{}%", pop)));
    }

    // Conditional: Actual rain/snow accumulation
    let mut precip_parts = Vec::new();
    if let Some(r) = rain_1h {
        if r > 0.0 {
            precip_parts.push(format!("🌧 {}/1h", fmt_precip_amount(r, units)));
        }
    }
    if let Some(s) = snow_1h {
        if s > 0.0 {
            precip_parts.push(format!("❄ {}/1h", fmt_precip_amount(s, units)));
        }
    }
    if !precip_parts.is_empty() {
        current_lines.push(precip_parts.join("  "));
    }

    (text, current_lines)
}

/// Formats hourly forecast lines
///
/// Layout: `TIME ICON TEMP  DESCRIPTION   POP%`
/// Time and temp on the left, description and pop right-aligned.
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
            let raw_label = fmt_time(h.dt, timezone_offset, "%-I%p");
            let label = format!("{:>4}", raw_label);
            let h_temp = h.temp.round();
            let local_hour = ((h.dt + timezone_offset) / 3600) % 24;
            let night = local_hour < 6 || local_hour >= 18;
            let icon_h = h
                .weather
                .get(0)
                .map(|d| pick_icon(d, night, Some(moon_phase_icon(h.dt, timezone_offset))))
                .unwrap_or_else(|| "❓".into());

            let raw_desc = h
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            let desc = escape_pango(&raw_desc);

            let pop = h.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
            let pop_str = if pop > 0 {
                format!("💧{:>3}%", pop)
            } else {
                String::new()
            };

            // Left side: "TIME ICON TEMP" (visible ~12 chars)
            let left = format!(
                "{} {} {}",
                label,
                icon_h,
                clr(&format!("{:>3.0}°", h_temp), &temp_color(h_temp, temp_bands)),
            );
            // visible length: 4(time) + 1(sp) + 2(icon) + 1(sp) + 4(temp) = 12
            let left_vis = 4 + 1 + 2 + 1 + 4;

            // Right side: "description  pop"
            let right = if pop_str.is_empty() {
                desc.clone()
            } else {
                format!("{} {}", desc, pop_str)
            };

            columns_markup(&left, left_vis, &right)
        })
        .collect()
}

/// Formats daily forecast lines
///
/// Layout: `DAY ICON HI°/LO°  DESCRIPTION   POP%`
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
            let day = format!("{:>3}", fmt_time(d.dt, timezone_offset, "%a"));
            let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
            let lo = d.temp.min.unwrap_or(0.0).round();
            let icon_d = d
                .weather
                .get(0)
                .map(|desc| pick_icon(desc, false, None))
                .unwrap_or_else(|| "❓".into());

            let raw_desc = d
                .weather
                .get(0)
                .and_then(|w| w.description.as_ref())
                .map(|s| short_desc(s, ui.max_desc_len))
                .unwrap_or_else(|| "—".into());
            let desc = escape_pango(&raw_desc);

            let pop = d.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
            let pop_str = if pop > 0 {
                format!("💧{:>3}%", pop)
            } else {
                String::new()
            };

            // Left: "DAY ICON HI°/LO°"
            let left = format!(
                "{} {} {}/{}",
                day,
                icon_d,
                clr(&format!("{:.0}°", hi), &temp_color(hi, temp_bands)),
                clr(&format!("{:.0}°", lo), &temp_color(lo, temp_bands)),
            );
            // visible: 3(day) + 1 + 2(icon) + 1 + ~4(hi) + 1(/) + ~4(lo) = ~16
            let hi_str = format!("{:.0}°", hi);
            let lo_str = format!("{:.0}°", lo);
            let left_vis = 3 + 1 + 2 + 1 + hi_str.chars().count() + 1 + lo_str.chars().count();

            let right = if pop_str.is_empty() {
                desc.clone()
            } else {
                format!("{} {}", desc, pop_str)
            };

            columns_markup(&left, left_vis, &right)
        })
        .collect()
}

/// Formats complete popup text and tooltip for Waybar
///
/// Uses a monospace font so space-padding creates real columns.
/// A single outer `<span>` sets the font and base color.
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

    let mut body = Vec::new();

    // Current conditions
    body.extend(current_lines);

    // Divider + Hourly
    body.push(divider(&colors.header));
    body.push(format!(
        "<b>{}</b>",
        clr(&format!("HOURLY {}", spark), &colors.header)
    ));
    body.extend(hourly_lines);

    // Divider + Daily
    body.push(divider(&colors.header));
    body.push(format!(
        "<b>{}</b>",
        clr("FORECAST", &colors.header)
    ));
    body.extend(daily_lines);

    // Single outer span — monospace font for column alignment
    let tooltip = format!(
        "<span font='monospace {}' foreground='{}'>{}</span>",
        ui.font_size,
        colors.text,
        body.join("\n")
    );

    (text, tooltip)
}
