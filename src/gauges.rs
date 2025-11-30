//! Custom gauge widgets for weather metrics
//!
//! Implements Cairo-based drawing for Arc Gauges (Humidity, UV, etc.)
//! and Compass Gauges (Wind).

use gtk::prelude::*;
use gtk::DrawingArea;
use std::f64::consts::PI;

/// Creates a generic arc gauge (speedometer style) with gradient fill
/// value_normalized: 0.0 to 1.0
/// text: Center display text
/// color_start: RGB tuple (r, g, b)
/// color_end: RGB tuple (r, g, b)
pub fn create_arc_gauge(
    value_normalized: f64,
    text: String,
    color_start: (f64, f64, f64),
    color_end: (f64, f64, f64),
) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.add_css_class("gauge-canvas");
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

        if radius <= 0.0 {
            return;
        } // Safety check

        // Angles: 135 deg to 45 deg (clockwise) -> 3/4 circle
        // Cairo 0 is East.
        // Start: 135 deg = 3*PI/4
        // End: 45 deg = PI/4 (wrapped? No, 2*PI + PI/4 = 2.25 PI)
        // Let's do 135 deg (Left-Down) to 405 deg (Right-Down)
        let start_angle = 0.75 * PI;
        let end_angle = 2.25 * PI;
        let full_span = 1.5 * PI;

        // Background Track
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.1);
        ctx.set_line_width(radius * 0.20); // Increased line width
        ctx.set_line_cap(gtk::cairo::LineCap::Round);
        ctx.arc(cx, cy, radius, start_angle, end_angle);
        ctx.stroke().expect("Failed gauge bg");

        // Active Arc with Gradient
        let current_angle = start_angle + (value_normalized.clamp(0.0, 1.0) * full_span);

        // Create a linear gradient covering the gauge area
        let gradient =
            gtk::cairo::LinearGradient::new(cx - radius, cy - radius, cx + radius, cy + radius);
        gradient.add_color_stop_rgb(0.0, color_start.0, color_start.1, color_start.2);
        gradient.add_color_stop_rgb(1.0, color_end.0, color_end.1, color_end.2);

        let _ = ctx.set_source(&gradient);
        ctx.arc(cx, cy, radius, start_angle, current_angle);
        ctx.stroke().expect("Failed gauge fg");

        // Center backdrop for consistency with compass
        let center_radius = (radius * 0.38).max(1.0);
        ctx.set_source_rgba(0.1, 0.1, 0.15, 0.82);
        ctx.arc(cx, cy, center_radius, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed center fill");

        // Text
        ctx.set_source_rgb(1.0, 1.0, 1.0);
        ctx.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Bold,
        );
        // Scale font with radius, removing restrictive upper clamp
        // 0.35 * radius is a more prominent proportion for the central text
        ctx.set_font_size((radius * 0.35).max(8.0));
        if let Ok(ext) = ctx.text_extents(&text) {
            ctx.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
            let _ = ctx.show_text(&text);
        }
    });

    canvas
}

/// Creates a wind compass gauge
/// degrees: Wind direction (0-360)
/// speed_text: Center text (e.g. "12 mph")
pub fn create_compass_gauge(degrees: f64, speed_text: String) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.add_css_class("gauge-canvas");
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

        if radius <= 0.0 {
            return;
        }

        // Compass Ring
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.1);
        ctx.set_line_width(radius * 0.20); // Increased line width
        ctx.arc(cx, cy, radius, 0.0, 2.0 * PI);
        ctx.stroke().expect("Failed compass ring");

        // Cardinal Marks (N, E, S, W)
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.35);
        // Cardinal font slightly smaller than central text
        let cardinal_font = (radius * 0.30).max(8.0);
        ctx.set_font_size(cardinal_font);
        let label_offset = radius * 0.2; // Pull labels inside ring
        for (i, label) in ["E", "S", "W", "N"].iter().enumerate() {
            let angle = (i as f64) * PI / 2.0;
            let lx = cx + (radius - label_offset) * angle.cos();
            let ly = cy + (radius - label_offset) * angle.sin();
            if let Ok(ext) = ctx.text_extents(label) {
                ctx.move_to(lx - ext.width() / 2.0, ly + ext.height() / 2.0);
                let _ = ctx.show_text(label);
            }
        }

        // Arrow
        let wind_rad = (degrees - 90.0) * PI / 180.0;
        let arrow_len = radius * 0.9;
        let tip_x = cx + arrow_len * wind_rad.cos();
        let tip_y = cy + arrow_len * wind_rad.sin();

        ctx.set_source_rgb(0.48, 0.64, 0.96); // Blue
        ctx.set_line_width(radius * 0.08); // Keep arrow thinner
        ctx.move_to(cx, cy);
        ctx.line_to(tip_x, tip_y);
        ctx.stroke().expect("Failed arrow");

        ctx.arc(tip_x, tip_y, radius * 0.1, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed arrow tip");

        // Speed Text (Centered)
        let center_radius = (radius * 0.38).max(1.0);
        ctx.set_source_rgba(0.1, 0.1, 0.15, 0.82);
        ctx.arc(cx, cy, center_radius, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed text bg");

        ctx.set_source_rgb(1.0, 1.0, 1.0);
        // Increased speed font size
        let speed_font = (radius * 0.35).max(8.0);
        ctx.set_font_size(speed_font);
        if let Ok(ext) = ctx.text_extents(&speed_text) {
            ctx.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
            let _ = ctx.show_text(&speed_text);
        }
    });

    canvas
}
