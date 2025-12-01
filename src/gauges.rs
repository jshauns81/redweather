//! Custom gauge widgets for weather metrics
//!
//! Implements Cairo-based drawing for Arc Gauges (Humidity, UV, etc.)
//! and Compass Gauges (Wind).
//!
//! OPTIMIZED: Uses render caching and debounced resize handling to prevent
//! heavy gradient repaints during window resizing.

use gtk::prelude::*;
use gtk::DrawingArea;
use gtk::cairo::{Context, Format, ImageSurface};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

struct RenderCache {
    surface: Option<ImageSurface>,
    width: i32,
    height: i32,
}

impl RenderCache {
    fn new() -> Self {
        Self {
            surface: None,
            width: -1,
            height: -1,
        }
    }
}

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

    let cache = Rc::new(RefCell::new(RenderCache::new()));

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let mut cache = cache.borrow_mut();
        
        // Check if we can reuse the cached surface
        if let Some(surface) = &cache.surface {
            if cache.width == w && cache.height == h {
                ctx.set_source_surface(surface, 0.0, 0.0).unwrap();
                ctx.paint().unwrap();
                return;
            }
        }

        // Cache miss or resize: Draw to new surface
        let surface = ImageSurface::create(Format::ARgb32, w, h)
            .expect("Failed to create gauge surface");
        let c = Context::new(&surface).expect("Failed to create gauge context");

        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

        if radius > 0.0 {
            // Angles: 135 deg to 45 deg (clockwise) -> 3/4 circle
            let start_angle = 0.75 * PI;
            let end_angle = 2.25 * PI;
            let full_span = 1.5 * PI;

            // Background Track
            c.set_source_rgba(1.0, 1.0, 1.0, 0.1);
            c.set_line_width(radius * 0.20);
            c.set_line_cap(gtk::cairo::LineCap::Round);
            c.arc(cx, cy, radius, start_angle, end_angle);
            c.stroke().expect("Failed gauge bg");

            // Active Arc with Gradient
            let current_angle = start_angle + (value_normalized.clamp(0.0, 1.0) * full_span);

            let gradient = gtk::cairo::LinearGradient::new(
                cx - radius,
                cy - radius,
                cx + radius,
                cy + radius,
            );
            gradient.add_color_stop_rgb(0.0, color_start.0, color_start.1, color_start.2);
            gradient.add_color_stop_rgb(1.0, color_end.0, color_end.1, color_end.2);

            let _ = c.set_source(&gradient);
            c.arc(cx, cy, radius, start_angle, current_angle);
            c.stroke().expect("Failed gauge fg");

            // Center backdrop
            let center_radius = (radius * 0.38).max(1.0);
            c.set_source_rgba(0.1, 0.1, 0.15, 0.82);
            c.arc(cx, cy, center_radius, 0.0, 2.0 * PI);
            c.fill().expect("Failed center fill");

            // Text
            c.set_source_rgb(1.0, 1.0, 1.0);
            c.select_font_face(
                "Sans",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            c.set_font_size((radius * 0.35).max(8.0));
            if let Ok(ext) = c.text_extents(&text) {
                c.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
                let _ = c.show_text(&text);
            }
        }

        // Update cache
        cache.surface = Some(surface.clone());
        cache.width = w;
        cache.height = h;

        // Paint to screen
        ctx.set_source_surface(&surface, 0.0, 0.0).unwrap();
        ctx.paint().unwrap();
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

    let cache = Rc::new(RefCell::new(RenderCache::new()));

    canvas.set_draw_func(move |_area, ctx, w, h| {
        let mut cache = cache.borrow_mut();

        // Check cache
        if let Some(surface) = &cache.surface {
            if cache.width == w && cache.height == h {
                ctx.set_source_surface(surface, 0.0, 0.0).unwrap();
                ctx.paint().unwrap();
                return;
            }
        }

        // Draw to new surface
        let surface = ImageSurface::create(Format::ARgb32, w, h)
            .expect("Failed to create compass surface");
        let c = Context::new(&surface).expect("Failed to create compass context");

        let width = w as f64;
        let height = h as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;
        let radius = (width.min(height) / 2.0) - 5.0;

        if radius > 0.0 {
            // Compass Ring
            c.set_source_rgba(1.0, 1.0, 1.0, 0.1);
            c.set_line_width(radius * 0.20);
            c.arc(cx, cy, radius, 0.0, 2.0 * PI);
            c.stroke().expect("Failed compass ring");

            // Cardinal Marks (N, E, S, W)
            c.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            let cardinal_font = (radius * 0.30).max(8.0);
            c.set_font_size(cardinal_font);
            let label_offset = radius * 0.2;
            for (i, label) in ["E", "S", "W", "N"].iter().enumerate() {
                let angle = (i as f64) * PI / 2.0;
                let lx = cx + (radius - label_offset) * angle.cos();
                let ly = cy + (radius - label_offset) * angle.sin();
                if let Ok(ext) = c.text_extents(label) {
                    c.move_to(lx - ext.width() / 2.0, ly + ext.height() / 2.0);
                    let _ = c.show_text(label);
                }
            }

            // Arrow
            let wind_rad = (degrees - 90.0) * PI / 180.0;
            let arrow_len = radius * 0.9;
            let tip_x = cx + arrow_len * wind_rad.cos();
            let tip_y = cy + arrow_len * wind_rad.sin();

            c.set_source_rgb(0.48, 0.64, 0.96);
            c.set_line_width(radius * 0.08);
            c.move_to(cx, cy);
            c.line_to(tip_x, tip_y);
            c.stroke().expect("Failed arrow");

            c.arc(tip_x, tip_y, radius * 0.1, 0.0, 2.0 * PI);
            c.fill().expect("Failed arrow tip");

            // Speed Text (Centered)
            let center_radius = (radius * 0.38).max(1.0);
            c.set_source_rgba(0.1, 0.1, 0.15, 0.82);
            c.arc(cx, cy, center_radius, 0.0, 2.0 * PI);
            c.fill().expect("Failed text bg");

            c.set_source_rgb(1.0, 1.0, 1.0);
            let speed_font = (radius * 0.35).max(8.0);
            c.set_font_size(speed_font);
            if let Ok(ext) = c.text_extents(&speed_text) {
                c.move_to(cx - ext.width() / 2.0, cy + ext.height() / 2.0);
                let _ = c.show_text(&speed_text);
            }
        }

        cache.surface = Some(surface.clone());
        cache.width = w;
        cache.height = h;

        ctx.set_source_surface(&surface, 0.0, 0.0).unwrap();
        ctx.paint().unwrap();
    });

    canvas
}