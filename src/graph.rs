//! GTK4 DrawingArea for hourly temperature graph
//!
//! This module implements a custom widget that draws a temperature curve using Cairo.

use gtk::prelude::*;
use gtk::{DrawingArea, EventControllerMotion};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use crate::utils::fmt_time;
use crate::weather::Hourly;

pub fn create_hourly_graph(hourly: &[Hourly], forecast_hours: usize, timezone_offset: i64) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.set_content_width(forecast_hours as i32 * 40);
    canvas.set_content_height(180);
    
    let hourly_data: Vec<(i64, f64)> = hourly
        .iter()
        .take(forecast_hours)
        .map(|h| (h.dt, h.temp))
        .collect();

    // Pre-calculate range
    let min_temp = hourly_data.iter().map(|(_, t)| *t).fold(f64::INFINITY, f64::min);
    let max_temp = hourly_data.iter().map(|(_, t)| *t).fold(f64::NEG_INFINITY, f64::max);
    // Ensure range is valid (avoid 0 or infinite range)
    let min_temp = if min_temp.is_finite() { min_temp } else { 0.0 };
    let max_temp = if max_temp.is_finite() { max_temp } else { 100.0 };
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
        draw_graph(ctx, w as f64, h as f64, &hourly_data, min_temp, temp_range, hover, timezone_offset);
    });

    canvas
}

fn draw_graph(
    ctx: &gtk::cairo::Context,
    width: f64,
    height: f64,
    data: &[(i64, f64)],
    min_temp: f64,
    temp_range: f64,
    hover_idx: Option<usize>,
    tz_offset: i64
) {
    if data.is_empty() { return; }

    // Padding
    let pad_top = 40.0;
    let pad_bottom = 40.0; 
    let pad_left = 20.0;
    let pad_right = 20.0;
    let graph_h = height - pad_top - pad_bottom;
    let graph_w = width - pad_left - pad_right;

    // Helper to map (index, temp) -> (x, y)
    let count = data.len();
    let step_x = graph_w / (count.max(2) - 1) as f64;
    
    let get_pt = |i: usize, temp: f64| -> (f64, f64) {
        let x = pad_left + (i as f64 * step_x);
        let normalized_t = (temp - min_temp) / temp_range;
        let y = pad_top + graph_h - (normalized_t * graph_h);
        (x, y)
    };

    // Gradient Fill
    let gradient = gtk::cairo::LinearGradient::new(0.0, pad_top, 0.0, height);
    gradient.add_color_stop_rgba(0.0, 0.48, 0.64, 0.96, 0.4); 
    gradient.add_color_stop_rgba(1.0, 0.48, 0.64, 0.96, 0.0); 
    let _ = ctx.set_source(&gradient);
    
    let (start_x, start_y) = get_pt(0, data[0].1);
    ctx.move_to(start_x, height);
    ctx.line_to(start_x, start_y);

    for i in 0..count - 1 {
        let (x0, y0) = get_pt(i, data[i].1);
        let (x1, y1) = get_pt(i + 1, data[i + 1].1);
        let mid_x = (x0 + x1) / 2.0;
        ctx.curve_to(mid_x, y0, mid_x, y1, x1, y1);
    }

    let (end_x, _) = get_pt(count - 1, data[count - 1].1);
    ctx.line_to(end_x, height);
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
            let (dt, temp) = data[idx];
            let (x, y) = get_pt(idx, temp);

            // Vertical Line
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.3);
            ctx.set_line_width(1.0);
            ctx.move_to(x, pad_top);
            ctx.line_to(x, height);
            ctx.stroke().expect("Failed overlay line");

            // Tooltip Box
            let time_s = fmt_time(dt, tz_offset, "%l %p");
            let temp_s = format!("{:.0}°", temp.round());
            
            ctx.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
            ctx.set_font_size(12.0);
            let extents = ctx.text_extents(&time_s).unwrap(); // rough calc
            
            // Draw text above point
            ctx.set_source_rgb(1.0, 1.0, 1.0);
            ctx.move_to(x - extents.width() / 2.0, y - 15.0);
            let _ = ctx.show_text(&temp_s);
            
            // Draw time at bottom
            ctx.move_to(x - extents.width() / 2.0, height - 5.0);
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.7);
            let _ = ctx.show_text(&time_s);
        }
    }
}
