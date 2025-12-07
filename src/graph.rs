//! Hourly graph renderer: temperature ribbon/line + precip bars + labels.
//! 
//! OPTIMIZED: Uses offscreen frame buffering for static elements.

use gtk::prelude::*;
use gtk::DrawingArea;
use gtk::cairo::{self, Format, ImageSurface};

use crate::utils::fmt_time;
use crate::weather::Hourly;
use gtk::glib::ControlFlow;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

// --- Helper Struct for consistent Y-Axis scaling ---
pub struct YAxisMetrics {
    pub grid_min: f64,
    pub grid_max: f64,
    pub temp_range: f64,
    pub ticks: Vec<f64>,
    pub temp_top: f64,
    pub temp_bot: f64,
    pub plot_height: f64,
}

impl YAxisMetrics {
    pub fn new(hourly_data: &[Hourly], total_height: f64, _forecast_hours: usize) -> Self {
        let top_margin_fixed = 24.0;
        let bottom_margin_fixed = 48.0; // Account for X-axis labels

        let plot_height = (total_height - top_margin_fixed - bottom_margin_fixed).max(1.0);
        let temp_top = top_margin_fixed;
        let temp_bot = top_margin_fixed + plot_height * 0.70; // 70% of plot for temp

        let raw_min_temp = hourly_data.iter().map(|h| h.temp).fold(f64::INFINITY, f64::min);
        let raw_max_temp = hourly_data.iter().map(|h| h.temp).fold(f64::NEG_INFINITY, f64::max);
        
        // Ensure valid range
        let raw_min_temp = if raw_min_temp.is_finite() { raw_min_temp } else { 0.0 };
        let raw_max_temp = if raw_max_temp.is_finite() { raw_max_temp } else { 1.0 };

        let mut grid_min = (raw_min_temp / 5.0).floor() * 5.0;
        let mut grid_max = (raw_max_temp / 5.0).ceil() * 5.0;

        // Ensure at least 10 degrees span for better visuals
        if grid_max - grid_min < 10.0 {
            grid_min -= 5.0;
            grid_max += 5.0;
        }
        
        let temp_range = (grid_max - grid_min).max(1.0);
        
        let mut ticks = Vec::new();
        let mut t = grid_min;
        while t <= grid_max + 0.1 { // +0.1 for float precision
            ticks.push(t);
            t += 5.0;
        }
        ticks.sort_by(|a, b| b.partial_cmp(a).unwrap()); // Descending order for drawing from top

        Self {
            grid_min,
            grid_max,
            temp_range,
            ticks,
            temp_top,
            temp_bot,
            plot_height,
        }
    }

    pub fn temp_to_y(&self, temp: f64) -> f64 {
        self.temp_top + (self.grid_max - temp) / self.temp_range * (self.temp_bot - self.temp_top)
    }
}


struct AnimState {
    prev_y: Vec<f64>,
    curr_y: Vec<f64>,
    prev_pop: Vec<f64>,
    curr_pop: Vec<f64>,
    progress: f64,
    animating: bool,
    last_tick: Instant,
    ribbon_phase: f64,
    // Caching
    static_cache: Option<ImageSurface>,
    cache_size: (i32, i32),
}

use chrono::Utc;

pub fn create_hourly_graph_plot( // Renamed to clearly indicate it's the plot area
    hourly: Rc<Vec<Hourly>>, // Now takes Rc<Vec<Hourly>>
    _forecast_hours: usize, // Unused directly here
    tz_offset: i32,
    y_metrics_rc: Rc<RefCell<YAxisMetrics>>, // Shared metrics
) -> DrawingArea {
    let count = hourly.len();
    let min_width = (count as f64 * 60.0).max(100.0); // Minimum 60px per hour

    let area = DrawingArea::new();
    area.add_css_class("hourly-graph-canvas");
    area.set_hexpand(true);
    area.set_vexpand(false);
    area.set_size_request(min_width as i32, 260); // Fixed height, variable width

    let anim_state = Rc::new(RefCell::new(AnimState {
        prev_y: Vec::new(),
        curr_y: Vec::new(),
        prev_pop: Vec::new(),
        curr_pop: Vec::new(),
        progress: 1.0,
        animating: false,
        last_tick: Instant::now(),
        ribbon_phase: 0.0,
        static_cache: None,
        cache_size: (-1, -1),
    }));

    // Animation tick
    {
        let state_for_tick = anim_state.clone();
        area.add_tick_callback(move |area, _clock| {
            let mut st = state_for_tick.borrow_mut();
            let now = Instant::now();
            let dt = now - st.last_tick;
            
            if !st.animating && dt.as_millis() < 33 {
                 return ControlFlow::Continue;
            }
            st.last_tick = now;

            if st.animating {
                let duration = 0.4_f64;
                let frac = dt.as_secs_f64() / duration;
                st.progress = (st.progress + frac).min(1.0);
                if st.progress >= 1.0 {
                    st.animating = false;
                }
                area.queue_draw();
            } else {
                st.ribbon_phase += dt.as_secs_f64() * 1.0;
                area.queue_draw();
            }
            ControlFlow::Continue
        });
    }

    // Tooltips
    {
        let tooltip_data = hourly.clone(); // Use the Rc<Vec<Hourly>> directly
        // _tooltip_y_metrics is unused, but still needed if the plot_width_calc was adjusted to use it
        area.set_has_tooltip(true);
        area.connect_query_tooltip(move |area, x, _y, _keyboard_mode, tooltip| {
            let width = area.width() as f64;
            // Left margin now internal to plot area
            let plot_left_margin = 0.0; // Y-axis is external
            let plot_right_margin = 16.0; // Keep some right margin

            let plot_width_calc = (width - plot_left_margin - plot_right_margin).max(1.0);
            let count = tooltip_data.len();
            
            if count == 0 { return false; }
            
            let dx = if count == 1 { 0.0 } else { plot_width_calc / (count as f64 - 1.0) };
            let idx = if dx < 0.1 { 0 } else { ((x as f64 - plot_left_margin) / dx).round() as usize };
            
            if idx < count {
                let h = &tooltip_data[idx];
                let time = fmt_time(h.dt, tz_offset as i64, "%I:%M %p");
                let temp = h.temp.round();
                let pop = h.pop.unwrap_or(0.0) * 100.0;
                let desc = h.weather.get(0).and_then(|w| w.description.as_deref()).unwrap_or("");
                
                let txt = format!("<b>{}</b>\n{:.0}°\nPrecip: {:.0}%\n<i>{}</i>", time, temp, pop, desc);
                tooltip.set_markup(Some(&txt));
                return true;
            }
            false
        });
    }

    let anim_state_for_draw = anim_state.clone();
    let data_for_draw = hourly.clone(); // Use the Rc<Vec<Hourly>>
    let draw_y_metrics = y_metrics_rc.clone();
    
    area.set_draw_func(move |_widget, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        let count = data_for_draw.len(); // Use data_for_draw.len() instead of _forecast_hours
        let now_local = Utc::now().timestamp() + tz_offset as i64;
        
        let mut st = anim_state_for_draw.borrow_mut();
        let y_metrics = draw_y_metrics.borrow(); // Access shared Y-axis metrics

        // Colors
        // let bg_top = (0x10 as f64 / 255.0, 0x14 as f64 / 255.0, 0x2a as f64 / 255.0);
        // let bg_bottom = (0x0c as f64 / 255.0, 0x11 as f64 / 255.0, 0x23 as f64 / 255.0);
        let grid_color = (56.0 / 255.0, 64.0 / 255.0, 100.0 / 255.0, 0.16);
        let time_color = (0.58, 0.66, 0.87);
        let temp_line = (0.133, 0.827, 0.933);
        let temp_fill_top = (0.659, 0.333, 0.969, 0.4);
        let temp_fill_bot = (0.659, 0.333, 0.969, 0.0);
        let pop_label = (0.35, 0.84, 1.0, 0.82);
        let marker_color = (0.88, 0.93, 1.0);

        // Layout Geometry
        let temp_top = y_metrics.temp_top;
        let temp_bot = y_metrics.temp_bot;
        let temp_to_y = |t: f64| y_metrics.temp_to_y(t);

        let plot_left_margin = 0.0; // Y-axis is external
        let plot_right_margin = 16.0;

        let plot_width_draw = (width - plot_left_margin - plot_right_margin).max(1.0);
        
        let precip_top = temp_top + y_metrics.plot_height * 0.70;
        let precip_bottom = temp_top + y_metrics.plot_height * 0.96;
        let time_axis_y = height - 8.0;

        // --- PHASE 1: STATIC BACKGROUND (Cached) ---
        let draw_static = |c: &cairo::Context| {
            // Background Gradient (removed for transparency)
            // let bg_gradient = cairo::LinearGradient::new(0.0, 0.0, 0.0, height);
            // bg_gradient.add_color_stop_rgb(0.0, bg_top.0, bg_top.1, bg_top.2);
            // bg_gradient.add_color_stop_rgb(1.0, bg_bottom.0, bg_bottom.1, bg_bottom.2);
            // let _ = c.set_source(&bg_gradient);
            // let _ = c.paint();

            if count == 0 {
                c.set_source_rgb(1.0, 1.0, 1.0);
                c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                c.set_font_size(12.0);
                let text = "No hourly data";
                if let Ok(ext) = c.text_extents(text) {
                    c.move_to(width / 2.0 - ext.width() / 2.0, height / 2.0 + ext.height() / 2.0);
                    let _ = c.show_text(text);
                }
                return;
            }

            // X positions
            let dx = if count == 1 { 0.0 } else { plot_width_draw / (count as f64 - 1.0) };
            let mut xs = Vec::with_capacity(count);
            for i in 0..count {
                let x = if count == 1 { plot_left_margin + plot_width_draw / 2.0 } else { plot_left_margin + dx * i as f64 };
                xs.push(x);
            }

            // Day/Night Shading
            let mut shade_dx = dx;
            if count == 1 { shade_dx = plot_width_draw; }
            for i in 0..count {
                let hour_local = data_for_draw[i].dt + tz_offset as i64;
                let hour_of_day = ((hour_local / 3600) % 24) as i32;
                let is_night = hour_of_day < 6 || hour_of_day >= 18;
                let x0 = xs[i];
                let x1 = x0 + shade_dx;
                if is_night { c.set_source_rgba(0.08, 0.10, 0.18, 0.16); } 
                else { c.set_source_rgba(0.10, 0.12, 0.21, 0.10); }
                c.rectangle(x0, temp_top, (x1 - x0).max(1.0), precip_bottom - temp_top);
                let _ = c.fill();
            }
            
            // 3-hour block shading
            if shade_dx > 20.0 {
                c.set_source_rgba(0.12, 0.14, 0.24, 0.10);
                for i in (0..count).step_by(3) {
                    let x0 = xs[i];
                    let last_x = *xs.last().unwrap_or(&x0);
                    let x1 = xs.get(i + 3).cloned().unwrap_or(last_x);
                    c.rectangle(x0, temp_top, (x1 - x0).max(1.0), precip_bottom - temp_top);
                    let _ = c.fill();
                }
            }

            // Grid lines
            c.set_source_rgba(grid_color.0, grid_color.1, grid_color.2, grid_color.3);
            c.set_line_width(1.0);
            for t in &y_metrics.ticks {
                let y_tick = temp_to_y(*t);
                c.move_to(plot_left_margin, y_tick); // Grid lines start from 0.0 (left edge of plot)
                c.line_to(width - plot_right_margin, y_tick);
            }
            let _ = c.stroke();

            // Minor grid lines
            let minor_grid_step = 2.5;
            c.set_source_rgba(grid_color.0, grid_color.1, grid_color.2, 0.10);
            c.set_line_width(0.75);
            let mut t = y_metrics.grid_min;
            while t <= y_metrics.grid_max + 0.1 {
                if (t % 5.0).abs() > 0.1 {
                    let y_tick = temp_to_y(t);
                    c.move_to(plot_left_margin, y_tick);
                    c.line_to(width - plot_right_margin, y_tick);
                }
                t += minor_grid_step;
            }
            let _ = c.stroke();

            // Day-change markers
            for i in 1..count {
                let day_prev = (data_for_draw[i - 1].dt + tz_offset as i64) / 86400;
                let day_curr = (data_for_draw[i].dt + tz_offset as i64) / 86400;
                if day_curr != day_prev {
                    let x = xs[i];
                    c.set_source_rgba(120.0 / 255.0, 130.0 / 255.0, 170.0 / 255.0, 0.15); // Fainter
                    c.set_line_width(1.0);
                    c.move_to(x, temp_top);
                    c.line_to(x, precip_bottom);
                    let _ = c.stroke();
                }
            }

            // Time labels
            c.set_font_size(10.0);
            for i in 0..count {
                let local_t = data_for_draw[i].dt + tz_offset as i64;
                let time_str = fmt_time(data_for_draw[i].dt, tz_offset as i64, "%H:%M");
                if let Ok(ext) = c.text_extents(&time_str) {
                    let past = local_t < now_local;
                    let alpha = if past { 0.4 } else { 1.0 };
                    c.set_source_rgba(time_color.0, time_color.1, time_color.2, alpha);
                    c.move_to(xs[i] - ext.width() / 2.0, time_axis_y);
                    let _ = c.show_text(&time_str);
                }
            }
        };

        // Execute Static Draw (Cached)
        if st.cache_size == (w, h) && st.static_cache.is_some() {
            if let Some(ref surf) = st.static_cache {
                ctx.set_source_surface(surf, 0.0, 0.0).unwrap();
                ctx.paint().unwrap();
            }
        } else {
            // Cache miss: redraw static
            let surface = ImageSurface::create(Format::ARgb32, w, h).expect("Graph surface failed");
            let c = cairo::Context::new(&surface).expect("Graph context failed");
            draw_static(&c);
            
            st.static_cache = Some(surface.clone());
            st.cache_size = (w, h);
            
            ctx.set_source_surface(&surface, 0.0, 0.0).unwrap();
            ctx.paint().unwrap();
        }

        // --- PHASE 2: DYNAMIC OVERLAY (Every Frame) ---
        if count > 0 {
            let dx = if count == 1 { 0.0 } else { plot_width_draw / (count as f64 - 1.0) };
            let mut xs = Vec::with_capacity(count);
            for i in 0..count {
                let x = if count == 1 { plot_left_margin + plot_width_draw / 2.0 } else { plot_left_margin + dx * i as f64 };
                xs.push(x);
            }

            // Anim Targets
            let mut y_temps_target = Vec::with_capacity(count);
            let mut local_times = Vec::with_capacity(count);
            for h in data_for_draw.iter().take(count) {
                y_temps_target.push(y_metrics.temp_to_y(h.temp));
                local_times.push(h.dt + tz_offset as i64);
            }
            let mut pop_targets = Vec::with_capacity(count);
            for h in data_for_draw.iter().take(count) {
                pop_targets.push(h.pop.unwrap_or(0.0).clamp(0.0, 1.0));
            }

            // State Updates
            if st.curr_y.is_empty() {
                st.curr_y = y_temps_target.clone(); st.prev_y = y_temps_target.clone();
                st.curr_pop = pop_targets.clone(); st.prev_pop = pop_targets.clone();
                st.progress = 1.0; st.animating = false;
            } else if st.curr_y.len() != y_temps_target.len() 
                || y_temps_target.iter().zip(st.curr_y.iter()).any(|(a, b)| (a - b).abs() > 0.1)
                || pop_targets.iter().zip(st.curr_pop.iter()).any(|(a, b)| (a - b).abs() > 0.01)
            {
                st.prev_y = st.curr_y.clone(); st.prev_pop = st.curr_pop.clone();
                st.curr_y = y_temps_target.clone(); st.curr_pop = pop_targets.clone();
                st.progress = 0.0; st.animating = true; st.last_tick = Instant::now();
            }

            let t_interp = st.progress.min(1.0);
            let mut y_temps = Vec::with_capacity(count);
            for i in 0..count {
                y_temps.push(st.prev_y[i] + (st.curr_y[i] - st.prev_y[i]) * t_interp);
            }

            // Ribbon
            let ribbon_offset = if st.animating { 0.0 } else { (st.ribbon_phase.sin()) * 2.0 };
            let ribbon_base = temp_top + (temp_bot - temp_top) * 0.85 + ribbon_offset;
            ctx.new_path();
            build_smooth_path(ctx, &xs, &y_temps);
            ctx.line_to(xs[count - 1], ribbon_base);
            ctx.line_to(xs[0], ribbon_base);
            ctx.close_path();

            let start_y = y_temps.iter().cloned().fold(f64::INFINITY, f64::min);
            let gradient = cairo::LinearGradient::new(0.0, start_y, 0.0, ribbon_base);
            gradient.add_color_stop_rgba(0.0, temp_fill_top.0, temp_fill_top.1, temp_fill_top.2, temp_fill_top.3);
            gradient.add_color_stop_rgba(1.0, temp_fill_bot.0, temp_fill_bot.1, temp_fill_bot.2, temp_fill_bot.3);
            let _ = ctx.set_source(&gradient);
            let _ = ctx.fill();

            // Precip Bars
            for i in 0..count {
                let pop = st.prev_pop[i] + (st.curr_pop[i] - st.prev_pop[i]) * t_interp;
                if pop <= 0.0 { continue; }
                let bar_width = 8.0;
                let bar_max_height = (precip_bottom - precip_top).max(1.0);
                let bar_height = bar_max_height * pop;
                let x_center = xs[i];
                let x_left = x_center - bar_width / 2.0;
                let bar_top_y = precip_bottom - bar_height;
                
                let (r, g, b, a) = if pop <= 0.2 { (110./255., 130./255., 170./255., 0.45) }
                else if pop <= 0.5 { (130./255., 165./255., 220./255., 0.55) }
                else if pop <= 0.8 { (135./255., 206./255., 250./255., 0.75) }
                else { (135./255., 240./255., 255./255., 0.90) };
                
                ctx.set_source_rgba(r, g, b, a);
                ctx.rectangle(x_left, bar_top_y, bar_width, bar_height);
                let _ = ctx.fill();
            }

            // Temp Line + Shadow
            ctx.save().ok();
            ctx.translate(0.0, 1.5);
            ctx.set_source_rgba(temp_line.0, temp_line.1, temp_line.2, 0.12);
            ctx.set_line_width(3.0);
            ctx.new_path();
            build_smooth_path(ctx, &xs, &y_temps);
            let _ = ctx.stroke();
            ctx.restore().ok();
            
            ctx.set_source_rgb(temp_line.0, temp_line.1, temp_line.2);
            ctx.set_line_width(2.0);
            ctx.new_path();
            build_smooth_path(ctx, &xs, &y_temps);
            let _ = ctx.stroke();

            // Markers
            ctx.set_source_rgb(marker_color.0, marker_color.1, marker_color.2);
            for i in 0..count {
                ctx.arc(xs[i], y_temps[i], 3.0, 0.0, std::f64::consts::PI * 2.0);
                let _ = ctx.fill();
            }

            // POP Labels
            ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            ctx.set_font_size(10.0);
            for i in 0..count {
                let pop = data_for_draw[i].pop.unwrap_or(0.0).clamp(0.0, 1.0);
                if pop < 0.05 { continue; }
                let label = format!("{:.0}%", pop * 100.0);
                if let Ok(ext) = ctx.text_extents(&label) {
                    let y = precip_bottom + 14.0;
                    ctx.set_source_rgba(pop_label.0, pop_label.1, pop_label.2, pop_label.3);
                    ctx.move_to(xs[i] - ext.width() / 2.0, y);
                    let _ = ctx.show_text(&label);
                }
            }

            // "Now" Marker
            let mut now_x: Option<f64> = None;
            if count == 1 {
                if (now_local - local_times[0]).abs() <= 3600 { now_x = Some(xs[0]); }
            } else {
                for i in 0..count - 1 {
                    let t0 = local_times[i];
                    let t1 = local_times[i + 1];
                    if now_local >= t0 && now_local <= t1 && t1 > t0 {
                        let ratio = (now_local - t0) as f64 / (t1 - t0) as f64;
                        now_x = Some(xs[i] + (xs[i + 1] - xs[i]) * ratio);
                        break;
                    }
                }
            }
            if let Some(x) = now_x {
                ctx.set_source_rgba(1.0, 1.0, 1.0, 0.5); // More opaque white
                ctx.set_line_width(1.5); // Thicker line
                ctx.move_to(x, temp_top);
                ctx.line_to(x, precip_bottom);
                let _ = ctx.stroke();
            }
        }
    });
    area
}


pub fn create_hourly_y_axis(
    y_metrics_rc: Rc<RefCell<YAxisMetrics>>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("hourly-y-axis-canvas");
    area.set_hexpand(false); // Fixed width
    area.set_vexpand(true);
    area.set_width_request(48); // Fixed width for Y-axis labels
    area.set_height_request(260); // Same height as plot area

    let draw_y_metrics = y_metrics_rc.clone();

    area.set_draw_func(move |_widget, ctx, w, _h| {
        let width = w as f64;
        let y_metrics = draw_y_metrics.borrow(); // Access shared Y-axis metrics

        // Colors (same as plot area for consistency)
        let tick_color = (96.0 / 255.0, 110.0 / 255.0, 170.0 / 255.0, 0.85);
        let tick_label_color = (0.58, 0.66, 0.87);

        // Use shared Y-axis parameters
        let temp_to_y = |t: f64| y_metrics.temp_to_y(t);

        // Temp scale ticks/labels (left)
        ctx.set_source_rgba(tick_color.0, tick_color.1, tick_color.2, tick_color.3);
        ctx.set_line_width(1.0);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(10.0);
        for t in &y_metrics.ticks {
            let y_tick = temp_to_y(*t);
            ctx.move_to(width - 6.0, y_tick); // Move tick line to right edge
            ctx.line_to(width - 2.0, y_tick); // Short tick mark
            let _ = ctx.stroke();

            let text = format!("{:.0}°", t);
            if let Ok(ext) = ctx.text_extents(&text) {
                ctx.set_source_rgb(tick_label_color.0, tick_label_color.1, tick_label_color.2);
                ctx.move_to(width - 8.0 - ext.width(), y_tick + ext.height() / 2.0); // Labels left of ticks
                let _ = ctx.show_text(&text);
            }
        }
    });

    area
}


fn build_smooth_path(ctx: &cairo::Context, xs: &[f64], ys: &[f64]) {
    let n = xs.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        ctx.move_to(xs[0], ys[0]);
        return;
    }

    ctx.move_to(xs[0], ys[0]);

    for i in 0..n - 1 {
        let p0x = if i == 0 { xs[0] } else { xs[i - 1] };
        let p0y = if i == 0 { ys[0] } else { ys[i - 1] };

        let p1x = xs[i];
        let p1y = ys[i];

        let p2x = xs[i + 1];
        let p2y = ys[i + 1];

        let p3x = if i + 2 >= n { xs[n - 1] } else { xs[i + 2] };
        let p3y = if i + 2 >= n { ys[n - 1] } else { ys[i + 2] };

        let c1x = p1x + (p2x - p0x) / 6.0;
        let c1y = p1y + (p2y - p0y) / 6.0;
        let c2x = p2x - (p3x - p1x) / 6.0;
        let c2y = p2y - (p3y - p1y) / 6.0;

        ctx.curve_to(c1x, c1y, c2x, c2y, p2x, p2y);
    }
}