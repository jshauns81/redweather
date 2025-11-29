//! Custom gauge widgets for weather metrics
//!
//! Implements Cairo-based drawing for Arc Gauges (Humidity, UV, etc.)
//! and Compass Gauges (Wind).

use gtk::prelude::*;
use gtk::DrawingArea;
use std::f64::consts::PI;

/// Creates a generic arc gauge (speedometer style)
/// value_normalized: 0.0 to 1.0
/// text: Center display text
/// color: RGB tuple (r, g, b)
pub fn create_arc_gauge(
    value_normalized: f64,
    text: String,
    color: (f64, f64, f64),
) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.set_content_width(88);
    canvas.set_content_height(88);

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

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
        ctx.set_line_width(6.0);
        ctx.set_line_cap(gtk::cairo::LineCap::Round);
        ctx.arc(cx, cy, radius, start_angle, end_angle);
        ctx.stroke().expect("Failed gauge bg");

        // Active Arc
        let current_angle = start_angle + (value_normalized.clamp(0.0, 1.0) * full_span);
        ctx.set_source_rgb(color.0, color.1, color.2);
        ctx.arc(cx, cy, radius, start_angle, current_angle);
        ctx.stroke().expect("Failed gauge fg");

        // Center backdrop for consistency with compass
        let center_radius = (radius * 0.38).clamp(12.0, 16.0);
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
        ctx.set_font_size(12.0);
        let ext = ctx.text_extents(&text).unwrap();
        ctx.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
        let _ = ctx.show_text(&text);
    });

    canvas
}

/// Creates a wind compass gauge
/// degrees: Wind direction (0-360)
/// speed_text: Center text (e.g. "12 mph")
pub fn create_compass_gauge(degrees: f64, speed_text: String) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.set_content_width(88);
    canvas.set_content_height(88);

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

        // Compass Ring
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.1);
        ctx.set_line_width(2.0);
        ctx.arc(cx, cy, radius, 0.0, 2.0 * PI);
        ctx.stroke().expect("Failed compass ring");

        // Cardinal Marks (N, E, S, W)
        ctx.set_source_rgba(1.0, 1.0, 1.0, 0.35);
        let cardinal_font = (radius * 0.17).clamp(9.0, 11.0);
        ctx.set_font_size(cardinal_font);
        for (i, label) in ["E", "S", "W", "N"].iter().enumerate() {
            let angle = (i as f64) * PI / 2.0;
            // N is -PI/2 (Up), but loop starts E(0).
            // i=0 -> 0 (E). i=1 -> PI/2 (S). i=2 -> PI (W). i=3 -> 3PI/2 (N).
            // Positions correct.
            let lx = cx + (radius - 10.0) * angle.cos();
            let ly = cy + (radius - 10.0) * angle.sin();
            let ext = ctx.text_extents(label).unwrap();
            ctx.move_to(lx - ext.width() / 2.0, ly + ext.height() / 2.0);
            let _ = ctx.show_text(label);
        }

        // Arrow
        // Rotate context to wind direction
        // Wind direction 0 is North (usually). Cairo 0 is East.
        // So Cairo Angle = (Degrees - 90) * PI / 180.
        let wind_rad = (degrees - 90.0) * PI / 180.0;

        let arrow_len = radius - 2.0;
        let tip_x = cx + arrow_len * wind_rad.cos();
        let tip_y = cy + arrow_len * wind_rad.sin();

        // Simple Arrow Line
        ctx.set_source_rgb(0.48, 0.64, 0.96); // Blue
        ctx.set_line_width(3.0);
        ctx.move_to(cx, cy);
        ctx.line_to(tip_x, tip_y);
        ctx.stroke().expect("Failed arrow");

        // Arrow Head
        // Draw a small triangle at tip? Or just a dot.
        ctx.arc(tip_x, tip_y, 4.0, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed arrow tip");

        // Speed Text (Centered)
        // Clear a small circle in center for text visibility?
        let center_radius = (radius * 0.38).clamp(12.0, 16.0);
        ctx.set_source_rgba(0.1, 0.1, 0.15, 0.82); // Dark background
        ctx.arc(cx, cy, center_radius, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed text bg");

        ctx.set_source_rgb(1.0, 1.0, 1.0);
        ctx.set_font_size(12.0);
        let ext = ctx.text_extents(&speed_text).unwrap();
        ctx.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
        let _ = ctx.show_text(&speed_text);
    });

    canvas
}
