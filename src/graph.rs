//! GTK4 DrawingArea for hourly temperature graph
//!
//! This module implements a custom widget that draws a temperature curve using Cairo.

use gtk::prelude::*;
use gtk::{DrawingArea, EventControllerMotion};
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo::{Context, Format, ImageSurface};

use crate::utils::{fmt_time, pick_small_icon_key};
use crate::weather::{Current, Hourly};

fn build_icon_cache() -> HashMap<&'static str, ImageSurface> {
    let mut map = HashMap::new();
    let keys = [
        "clear_day",
        "clear_night",
        "partly_cloudy_day",
        "partly_cloudy_night",
        "cloudy",
        "rain_light",
        "rain",
        "rain_heavy",
        "thunderstorm",
        "snow",
        "sleet",
        "fog",
        "wind",
    ];
    for key in keys {
        map.insert(key, load_icon_surface(key));
    }
    map
}

fn load_icon_surface(key: &str) -> ImageSurface {
    // Placeholder: solid color square with a center dot. Swap with PNG loading when assets exist.
    let surface =
        ImageSurface::create(Format::ARgb32, 36, 36).expect("Failed to create image surface");
    let ctx = Context::new(&surface).expect("Failed to create cairo context");

    let (r, g, b) = match key {
        "clear_day" => (1.0, 0.85, 0.2),
        "clear_night" => (0.7, 0.8, 1.0),
        "partly_cloudy_day" => (0.7, 0.8, 0.95),
        "partly_cloudy_night" => (0.6, 0.7, 0.9),
        "cloudy" => (0.6, 0.65, 0.75),
        "rain_light" => (0.4, 0.75, 0.95),
        "rain" => (0.3, 0.6, 0.9),
        "rain_heavy" => (0.2, 0.45, 0.8),
        "thunderstorm" => (0.85, 0.65, 0.2),
        "snow" => (0.9, 0.95, 1.0),
        "sleet" => (0.7, 0.85, 0.95),
        "fog" => (0.7, 0.7, 0.7),
        "wind" => (0.8, 0.9, 0.9),
        _ => (0.8, 0.8, 0.8),
    };

    ctx.set_source_rgba(r, g, b, 0.9);
    ctx.paint().ok();
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    ctx.rectangle(1.0, 1.0, 34.0, 34.0);
    ctx.stroke().ok();
    ctx.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    ctx.arc(18.0, 18.0, 4.0, 0.0, 2.0 * PI);
    ctx.fill().ok();

    surface
}

pub fn create_hourly_graph(
    hourly: &[Hourly],
    forecast_hours: usize,
    timezone_offset: i64,
    current: &Current,
) -> DrawingArea {
    let icon_cache = Rc::new(build_icon_cache());
    let canvas = DrawingArea::new();
    canvas.set_content_width(forecast_hours as i32 * 40);
    canvas.set_content_height(180);

    let hourly_data: Vec<(i64, f64, f64, String, bool)> = hourly
        .iter()
        .take(forecast_hours)
        .map(|h| {
            let local = h.dt + timezone_offset as i64;
            let sunrise = current.sunrise.unwrap_or(0) + timezone_offset as i64;
            let sunset = current.sunset.unwrap_or(0) + timezone_offset as i64;
            let is_night = local < sunrise || local >= sunset;

            // Map to lightweight icon strings (small to avoid tofu boxes)
            let icon = h
                .weather
                .get(0)
                .map(|desc| pick_small_icon_key(desc, is_night))
                .unwrap_or("wind")
                .to_string();
            (
                h.dt,
                h.temp,
                h.pop.unwrap_or(0.0).clamp(0.0, 1.0),
                icon,
                is_night,
            )
        })
        .collect();

    // Pre-calculate range with a small padding to avoid flat line
    let mut min_temp = hourly_data
        .iter()
        .map(|(_, t, _, _, _)| *t)
        .fold(f64::INFINITY, f64::min);
    let mut max_temp = hourly_data
        .iter()
        .map(|(_, t, _, _, _)| *t)
        .fold(f64::NEG_INFINITY, f64::max);
    if !min_temp.is_finite() || !max_temp.is_finite() {
        min_temp = 0.0;
        max_temp = 1.0;
    }
    // Add +/-2°F padding
    min_temp -= 2.0;
    max_temp += 2.0;
    let temp_range = (max_temp - min_temp).max(1.0);

    let hover_state = Rc::new(RefCell::new(None::<usize>));

    // Motion Controller
    let motion = EventControllerMotion::new();
    let h_state = hover_state.clone();
    let count = hourly_data.len();
    let canvas_weak = canvas.downgrade();

    motion.connect_motion(move |_ctl, x, _y| {
        if let Some(canvas) = canvas_weak.upgrade() {
            let width = canvas.width() as f64;
            // Basic geometry (must match draw_graph)
            let pad_left = 10.0;
            let pad_right = 10.0;
            let graph_w = width - pad_left - pad_right;
            let step_x = graph_w / (count.max(2) - 1) as f64;

            let idx = ((x - pad_left) / step_x).round() as isize;
            let idx = idx.clamp(0, (count - 1) as isize) as usize;

            if *h_state.borrow() != Some(idx) {
                *h_state.borrow_mut() = Some(idx);
                canvas.queue_draw();
            }
        }
    });

    let h_state_leave = hover_state.clone();
    let canvas_weak_leave = canvas.downgrade();
    motion.connect_leave(move |_| {
        if let Some(canvas) = canvas_weak_leave.upgrade() {
            *h_state_leave.borrow_mut() = None;
            canvas.queue_draw();
        }
    });

    canvas.add_controller(motion);

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let hover = *hover_state.borrow();
        draw_graph(
            ctx,
            w as f64,
            h as f64,
            &hourly_data,
            min_temp,
            temp_range,
            hover,
            timezone_offset,
            &icon_cache,
        );
    });

    canvas
}

fn draw_graph(
    ctx: &gtk::cairo::Context,
    width: f64,
    height: f64,
    data: &[(i64, f64, f64, String, bool)],
    min_temp: f64,
    temp_range: f64,
    hover_idx: Option<usize>,
    tz_offset: i64,
    icon_cache: &HashMap<&'static str, ImageSurface>,
) {
    if data.is_empty() {
        return;
    }

    // Layout geometry
    let top_margin = 24.0;
    let bottom_margin = 42.0;
    let left_margin = 32.0;
    let right_margin = 16.0;

    let plot_height = (height - top_margin - bottom_margin).max(1.0);
    let plot_width = (width - left_margin - right_margin).max(1.0);

    // Bands (slight overlap for icon middle zone)
    let temp_top = top_margin;
    let temp_bot = top_margin + plot_height * 0.7;
    let precip_top = top_margin + plot_height * 0.55;
    let precip_bottom = top_margin + plot_height * 0.95;
    let time_axis_y = height - 6.0;

    // Helper to map (index, temp) -> (x, y)
    let count = data.len();
    let step_x = plot_width / (count.max(2) - 1) as f64;

    // Determine Range
    let max_temp = min_temp + temp_range; // Recalculate max from range

    let temp_to_y = |temp: f64| -> f64 {
        let _normalized_t = (temp - min_temp) / temp_range;
        temp_top + (max_temp - temp) / temp_range * (temp_bot - temp_top)
    };

    let get_pt = |i: usize, temp: f64| -> (f64, f64) {
        let x = left_margin + (i as f64 * step_x);
        let y = temp_to_y(temp);
        (x, y)
    };

    // --- Draw Grid (keep minimal for clean look) ---
    ctx.set_source_rgba(1.0, 1.0, 1.0, 0.06); // subtle verticals
    ctx.set_line_width(1.0);

    // Vertical (Time) Lines
    for i in 0..count {
        let x = left_margin + (i as f64 * step_x);
        ctx.move_to(x, temp_top);
        ctx.line_to(x, precip_bottom);
    }
    ctx.stroke().expect("Failed grid vert");

    // Day change markers (thicker line at local midnight)
    let mut last_day = (data[0].0 + tz_offset) / 86_400;
    for i in 1..count {
        let day = (data[i].0 + tz_offset) / 86_400;
        if day != last_day {
            let x = left_margin + (i as f64 * step_x);
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            ctx.set_line_width(2.0);
            ctx.move_to(x, temp_top);
            ctx.line_to(x, precip_bottom);
            ctx.stroke().expect("Failed day marker");
            last_day = day;
        }
    }

    // Time labels along bottom (24h)
    let label_step = if count > 24 {
        4
    } else if count > 12 {
        3
    } else {
        2
    };
    ctx.set_source_rgba(1.0, 1.0, 1.0, 0.35);
    ctx.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    ctx.set_font_size(9.0);
    for i in 0..count {
        if i % label_step != 0 && i != count - 1 {
            continue;
        }
        let x = left_margin + (i as f64 * step_x);
        let time_s = fmt_time(data[i].0, tz_offset, "%H:%M");
        let ext = ctx.text_extents(&time_s).unwrap();
        ctx.move_to(x - ext.width() / 2.0, time_axis_y);
        let _ = ctx.show_text(&time_s);
    }

    // Precip bars in their own band (secondary, translucent)
    let bar_band_top = precip_top;
    let bar_band_bottom = precip_bottom;
    let bar_height = (bar_band_bottom - bar_band_top - 4.0).max(1.0);
    let bar_base = bar_band_bottom;
    for i in 0..count {
        let pop = data[i].2;
        if pop <= 0.01 {
            continue;
        }
        let x = left_margin + (i as f64 * step_x);
        let h = bar_height * pop;
        ctx.set_source_rgba(0.40, 0.85, 0.98, 0.55);
        ctx.rectangle(x - 5.0, bar_base - h, 10.0, h);
        ctx.fill().expect("Failed precip bar");

        // Always show small POP label under the bar
        let label = format!("{:.0}%", pop * 100.0);
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.75);
        ctx.set_font_size(9.0);
        if let Ok(ext) = ctx.text_extents(&label) {
            ctx.move_to(x - ext.width() / 2.0, bar_base + 12.0);
            let _ = ctx.show_text(&label);
        }
    }

    // Weather icons between temp line and precip band
    let icon_w = 18.0;
    let icon_h = 18.0;
    for i in 0..count {
        let key = data[i].3.as_str();
        let (x, y) = get_pt(i, data[i].1);
        let icon_y = y + (precip_top - y) * 0.5;

        if let Some(surface) = icon_cache.get(key) {
            let sx = icon_w / surface.width() as f64;
            let sy = icon_h / surface.height() as f64;

            ctx.save().ok();
            ctx.translate(x - icon_w / 2.0, icon_y - icon_h / 2.0);
            ctx.scale(sx, sy);
            let _ = ctx.set_source_surface(surface, 0.0, 0.0);
            let _ = ctx.paint();
            ctx.restore().ok();
        }
    }

    // Horizontal (Temp) Lines
    let temp_step = if temp_range > 20.0 { 10.0 } else { 5.0 };
    let first_grid_temp = (min_temp / temp_step).ceil() * temp_step;

    let mut t = first_grid_temp;
    ctx.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    ctx.set_font_size(10.0);

    while t <= max_temp {
        let y = temp_to_y(t);

        // Draw line
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        ctx.move_to(left_margin, y);
        ctx.line_to(width - right_margin, y);
        ctx.stroke().expect("Failed grid horz");

        // Draw Label
        let label = format!("{:.0}°", t);
        // Draw label slightly inset from the left margin
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.5);
        ctx.move_to(left_margin + 2.0, y - 2.0);
        let _ = ctx.show_text(&label);

        t += temp_step;
    }

    // Draw Gradient Fill (Area under curve) limited to temp band
    let gradient = gtk::cairo::LinearGradient::new(0.0, temp_top, 0.0, temp_bot);
    gradient.add_color_stop_rgba(0.0, 0.48, 0.64, 0.96, 0.4);
    gradient.add_color_stop_rgba(1.0, 0.48, 0.64, 0.96, 0.0);
    let _ = ctx.set_source(&gradient);

    let (start_x, start_y) = get_pt(0, data[0].1);
    ctx.move_to(start_x, temp_bot);
    ctx.line_to(start_x, start_y);

    for i in 0..count - 1 {
        let (x0, y0) = get_pt(i, data[i].1);
        let (x1, y1) = get_pt(i + 1, data[i + 1].1);
        let mid_x = (x0 + x1) / 2.0;
        ctx.curve_to(mid_x, y0, mid_x, y1, x1, y1);
    }

    let (end_x, _) = get_pt(count - 1, data[count - 1].1);
    ctx.line_to(end_x, temp_bot);
    ctx.close_path();
    ctx.fill().expect("Failed to fill graph");

    // Stroke Line
    ctx.set_source_rgb(0.48, 0.64, 0.96);
    ctx.set_line_width(3.0);
    ctx.move_to(start_x, start_y);
    for i in 0..count - 1 {
        let (x0, y0) = get_pt(i, data[i].1);
        let (x1, y1) = get_pt(i + 1, data[i + 1].1);
        let mid_x = (x0 + x1) / 2.0;
        ctx.curve_to(mid_x, y0, mid_x, y1, x1, y1);
    }
    ctx.stroke().expect("Failed to stroke graph");

    // Dots
    ctx.set_source_rgb(1.0, 1.0, 1.0);
    for i in 0..count {
        let (x, y) = get_pt(i, data[i].1);
        // Highlight hovered dot
        let r = if Some(i) == hover_idx { 6.0 } else { 3.0 };
        ctx.arc(x, y, r, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed to draw dot");
    }

    // Hover Overlay
    if let Some(idx) = hover_idx {
        if idx < count {
            let (dt, temp, pop) = (data[idx].0, data[idx].1, data[idx].2);
            let (x, y) = get_pt(idx, temp);

            // Vertical Line
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.3);
            ctx.set_line_width(1.0);
            ctx.move_to(x, temp_top);
            ctx.line_to(x, precip_bottom);
            ctx.stroke().expect("Failed overlay line");

            // Tooltip Box
            let time_s = fmt_time(dt, tz_offset, "%H:%M");
            let temp_s = format!("{:.0}°", temp.round());

            ctx.select_font_face(
                "Sans",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            ctx.set_font_size(12.0);
            let extents = ctx.text_extents(&time_s).unwrap(); // rough calc

            // Draw text above point
            ctx.set_source_rgb(1.0, 1.0, 1.0);
            ctx.move_to(x - extents.width() / 2.0, y - 15.0);
            let _ = ctx.show_text(&temp_s);

            // Draw time at bottom
            let bottom_text = if pop > 0.0 {
                format!("{}  •  POP {:.0}%", time_s.trim(), pop * 100.0)
            } else {
                time_s
            };
            let ext_b = ctx.text_extents(&bottom_text).unwrap();
            ctx.move_to(x - ext_b.width() / 2.0, time_axis_y);
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.7);
            let _ = ctx.show_text(&bottom_text);
        }
    }
}
