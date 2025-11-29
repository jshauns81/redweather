//! Hourly graph renderer: temperature ribbon/line + precip bars + labels.

use gtk::prelude::*;
use gtk::DrawingArea;

use crate::utils::fmt_time;
use crate::weather::Hourly;
use gtk::glib::ControlFlow;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

struct AnimState {
    prev_y: Vec<f64>,
    curr_y: Vec<f64>,
    prev_pop: Vec<f64>,
    curr_pop: Vec<f64>,
    progress: f64,
    animating: bool,
    last_tick: Instant,
    ribbon_phase: f64,
}
use chrono::Utc;

pub fn create_hourly_graph(
    hourly: &[Hourly],
    forecast_hours: usize,
    tz_offset: i32,
) -> DrawingArea {
    let data: Rc<Vec<Hourly>> = Rc::new(hourly.iter().take(forecast_hours).cloned().collect());
    let area = DrawingArea::new();
    area.set_content_height(220);
    area.set_content_width((forecast_hours as i32).max(1) * 40);

    let anim_state = Rc::new(RefCell::new(AnimState {
        prev_y: Vec::new(),
        curr_y: Vec::new(),
        prev_pop: Vec::new(),
        curr_pop: Vec::new(),
        progress: 1.0,
        animating: false,
        last_tick: Instant::now(),
        ribbon_phase: 0.0,
    }));

    // Animation tick: advance lerp and breathing; redraw each frame
    {
        let state_for_tick = anim_state.clone();
        area.add_tick_callback(move |area, _clock| {
            let mut st = state_for_tick.borrow_mut();
            let now = Instant::now();
            let dt = now - st.last_tick;
            st.last_tick = now;

            if st.animating {
                let duration = 0.4_f64; // seconds
                let frac = dt.as_secs_f64() / duration;
                st.progress = (st.progress + frac).min(1.0);
                if st.progress >= 1.0 {
                    st.animating = false;
                }
            } else {
                st.ribbon_phase += dt.as_secs_f64() * 1.0;
            }

            area.queue_draw();
            ControlFlow::Continue
        });
    }

    let anim_state_for_draw = anim_state.clone();
    let data_for_draw = data.clone();
    area.set_draw_func(move |_widget, ctx, w, h| {
        ctx.set_antialias(gtk::cairo::Antialias::Best);

        let width = w as f64;
        let height = h as f64;
        let count = data_for_draw.len().min(forecast_hours);
        let now_local = Utc::now().timestamp() + tz_offset as i64;
        let mut st = anim_state_for_draw.borrow_mut();

        // Colors
        let bg = (
            0x1f as f64 / 255.0,
            0x23 as f64 / 255.0,
            0x35 as f64 / 255.0,
        );
        let grid_color = (65.0 / 255.0, 72.0 / 255.0, 104.0 / 255.0, 0.18);
        let tick_color = (86.0 / 255.0, 95.0 / 255.0, 137.0 / 255.0, 0.9);
        let tick_label_color = (
            0x56 as f64 / 255.0,
            0x5f as f64 / 255.0,
            0x89 as f64 / 255.0,
        );
        let temp_line = (0.48, 0.64, 0.96);
        let temp_fill_top = (122.0 / 255.0, 162.0 / 255.0, 247.0 / 255.0, 0.35);
        let temp_fill_bot = (122.0 / 255.0, 162.0 / 255.0, 247.0 / 255.0, 0.0);
        let _pop_bar = (135.0 / 255.0, 206.0 / 255.0, 250.0 / 255.0, 0.85);
        let pop_label = (135.0 / 255.0, 206.0 / 255.0, 250.0 / 255.0, 0.8);
        let marker_color = (0.84, 0.89, 1.0);
        let time_color = (
            0x56 as f64 / 255.0,
            0x5f as f64 / 255.0,
            0x89 as f64 / 255.0,
        );

        // Background
        ctx.set_source_rgb(bg.0, bg.1, bg.2);
        let _ = ctx.paint();

        if count == 0 {
            ctx.set_source_rgb(1.0, 1.0, 1.0);
            ctx.select_font_face(
                "Sans",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Normal,
            );
            ctx.set_font_size(12.0);
            let text = "No hourly data";
            if let Ok(ext) = ctx.text_extents(text) {
                ctx.move_to(
                    width / 2.0 - ext.width() / 2.0,
                    height / 2.0 + ext.height() / 2.0,
                );
                let _ = ctx.show_text(text);
            }
            return;
        }

        // Geometry
        let top_margin = 24.0;
        let bottom_margin = 48.0;
        let left_margin = 32.0;
        let right_margin = 16.0;

        let plot_width = (width - left_margin - right_margin).max(1.0);
        let plot_height = (height - top_margin - bottom_margin).max(1.0);

        let temp_top = top_margin;
        let temp_bot = top_margin + plot_height * 0.70;
        let precip_top = top_margin + plot_height * 0.72;
        let precip_bottom = top_margin + plot_height * 0.96;
        let time_axis_y = height - 8.0;

        // Temps
        let mut temp_min = data_for_draw.iter().map(|h| h.temp).fold(f64::INFINITY, f64::min);
        let mut temp_max = data_for_draw
            .iter()
            .map(|h| h.temp)
            .fold(f64::NEG_INFINITY, f64::max);
        if !temp_min.is_finite() || !temp_max.is_finite() {
            temp_min = 0.0;
            temp_max = 1.0;
        }
        temp_min -= 2.0;
        temp_max += 2.0;
        let temp_range = (temp_max - temp_min).max(1.0);

        let tick_min = temp_min.round();
        let tick_max = temp_max.round();
        let tick_mid = (tick_min + tick_max) / 2.0;
        let ticks = [tick_max, tick_mid, tick_min];

        let temp_to_y =
            |t: f64| -> f64 { temp_top + (temp_max - t) / temp_range * (temp_bot - temp_top) };

        // X positions
        let dx = if count == 1 {
            0.0
        } else {
            plot_width / (count as f64 - 1.0)
        };
        let mut xs = Vec::with_capacity(count);
        for i in 0..count {
            let x = if count == 1 {
                left_margin + plot_width / 2.0
            } else {
                left_margin + dx * i as f64
            };
            xs.push(x);
        }

        // Y temps (target) and local times
        let mut y_temps_target = Vec::with_capacity(count);
        let mut local_times = Vec::with_capacity(count);
        for h in data_for_draw.iter().take(count) {
            let y = temp_to_y(h.temp);
            y_temps_target.push(y);
            local_times.push(h.dt + tz_offset as i64);
        }

        // Pop targets
        let mut pop_targets = Vec::with_capacity(count);
        for h in data_for_draw.iter().take(count) {
            let pop = h.pop.unwrap_or(0.0).clamp(0.0, 1.0);
            pop_targets.push(pop);
        }

        // Initialize or update animation targets
        if st.curr_y.is_empty() {
            st.curr_y = y_temps_target.clone();
            st.prev_y = y_temps_target.clone();
            st.curr_pop = pop_targets.clone();
            st.prev_pop = pop_targets.clone();
            st.progress = 1.0;
            st.animating = false;
        } else if st.curr_y.len() != y_temps_target.len()
            || y_temps_target
                .iter()
                .zip(st.curr_y.iter())
                .any(|(a, b)| (a - b).abs() > 0.1)
            || pop_targets
                .iter()
                .zip(st.curr_pop.iter())
                .any(|(a, b)| (a - b).abs() > 0.01)
        {
            st.prev_y = st.curr_y.clone();
            st.prev_pop = st.curr_pop.clone();
            st.curr_y = y_temps_target.clone();
            st.curr_pop = pop_targets.clone();
            st.progress = 0.0;
            st.animating = true;
            st.last_tick = Instant::now();
        }

        let t_interp = st.progress.min(1.0);

        // Interpolated temps/pops for drawing
        let mut y_temps = Vec::with_capacity(count);
        for i in 0..count {
            let y = st.prev_y[i] + (st.curr_y[i] - st.prev_y[i]) * t_interp;
            y_temps.push(y);
        }

        // Day/night shading (before grid/ribbon)
        let mut shade_dx = dx;
        if count == 1 {
            shade_dx = plot_width;
        }
        for i in 0..count {
            let hour_local = data_for_draw[i].dt + tz_offset as i64;
            let hour_of_day = ((hour_local / 3600) % 24) as i32;
            let is_night = hour_of_day < 6 || hour_of_day >= 18;
            let x0 = xs[i];
            let x1 = x0 + shade_dx;
            if is_night {
                ctx.set_source_rgba(0.10, 0.12, 0.20, 0.20);
            } else {
                ctx.set_source_rgba(0.12, 0.14, 0.22, 0.10);
            }
            ctx.rectangle(x0, temp_top, (x1 - x0).max(1.0), precip_bottom - temp_top);
            let _ = ctx.fill();
        }

        // 3-hour block shading (light)
        if shade_dx > 20.0 {
            ctx.set_source_rgba(0.14, 0.16, 0.25, 0.12);
            for i in (0..count).step_by(3) {
                let x0 = xs[i];
                let x1 = xs.get(i + 3).cloned().unwrap_or(*xs.last().unwrap());
                ctx.rectangle(x0, temp_top, (x1 - x0).max(1.0), precip_bottom - temp_top);
                let _ = ctx.fill();
            }
        }

        // Grid lines at ticks
        ctx.set_source_rgba(grid_color.0, grid_color.1, grid_color.2, grid_color.3);
        ctx.set_line_width(1.0);
        for t in ticks {
            let y_tick = temp_to_y(t);
            ctx.move_to(left_margin, y_tick);
            ctx.line_to(width - right_margin, y_tick);
        }
        let _ = ctx.stroke();

        // Temp ribbon fill (using smooth path) with subtle breathing when idle
        let ribbon_offset = if st.animating {
            0.0
        } else {
            (st.ribbon_phase.sin()) * 2.0
        };
        let ribbon_base = temp_top + (temp_bot - temp_top) * 0.85 + ribbon_offset;
        ctx.new_path();
        build_smooth_path(ctx, &xs, &y_temps);
        ctx.line_to(xs[count - 1], ribbon_base);
        ctx.line_to(xs[0], ribbon_base);
        ctx.close_path();

        let start_y = y_temps.iter().cloned().fold(f64::INFINITY, f64::min);
        let gradient = gtk::cairo::LinearGradient::new(0.0, start_y, 0.0, ribbon_base);
        gradient.add_color_stop_rgba(
            0.0,
            temp_fill_top.0,
            temp_fill_top.1,
            temp_fill_top.2,
            temp_fill_top.3,
        );
        gradient.add_color_stop_rgba(
            1.0,
            temp_fill_bot.0,
            temp_fill_bot.1,
            temp_fill_bot.2,
            temp_fill_bot.3,
        );
        let _ = ctx.set_source(&gradient);
        let _ = ctx.fill();

        // Day-change markers (midnight only, since sunrise/sunset not available)
        for i in 1..count {
            let day_prev = (data_for_draw[i - 1].dt + tz_offset as i64) / 86_400;
            let day_curr = (data_for_draw[i].dt + tz_offset as i64) / 86_400;
            if day_curr != day_prev {
                let x = xs[i];
                ctx.set_source_rgba(120.0 / 255.0, 130.0 / 255.0, 170.0 / 255.0, 0.25);
                ctx.set_line_width(1.0);
                ctx.move_to(x, temp_top);
                ctx.line_to(x, precip_bottom);
                let _ = ctx.stroke();
            }
        }

        // Precip bars with semantic colors
        for i in 0..count {
            let pop = st.prev_pop[i] + (st.curr_pop[i] - st.prev_pop[i]) * t_interp;
            if pop <= 0.0 {
                continue;
            }
            let bar_width = 8.0;
            let bar_max_height = (precip_bottom - precip_top).max(1.0);
            let bar_height = bar_max_height * pop;
            let x_center = xs[i];
            let x_left = x_center - bar_width / 2.0;
            let bar_top_y = precip_bottom - bar_height;

            // Color semantics
            let (r, g, b, a) = if pop <= 0.20 {
                (110.0 / 255.0, 130.0 / 255.0, 170.0 / 255.0, 0.45)
            } else if pop <= 0.50 {
                (130.0 / 255.0, 165.0 / 255.0, 220.0 / 255.0, 0.55)
            } else if pop <= 0.80 {
                (135.0 / 255.0, 206.0 / 255.0, 250.0 / 255.0, 0.75)
            } else {
                (135.0 / 255.0, 240.0 / 255.0, 255.0 / 255.0, 0.90)
            };
            ctx.set_source_rgba(r, g, b, a);
            ctx.rectangle(x_left, bar_top_y, bar_width, bar_height);
            let _ = ctx.fill();

            if pop >= 0.80 {
                // subtle glow
                ctx.set_source_rgba(135.0 / 255.0, 240.0 / 255.0, 255.0 / 255.0, 0.25);
                ctx.rectangle(
                    x_left - 1.5,
                    bar_top_y - 1.5,
                    bar_width + 3.0,
                    bar_height + 4.0,
                );
                let _ = ctx.fill();
            }
        }

        // Temp line shadow
        ctx.save().ok();
        ctx.translate(0.0, 1.5);
        ctx.set_source_rgba(temp_line.0, temp_line.1, temp_line.2, 0.12);
        ctx.set_line_width(3.0);
        ctx.new_path();
        build_smooth_path(ctx, &xs, &y_temps);
        let _ = ctx.stroke();
        ctx.restore().ok();

        // Temp line (smooth)
        ctx.set_source_rgb(temp_line.0, temp_line.1, temp_line.2);
        ctx.set_line_width(2.0);
        ctx.new_path();
        build_smooth_path(ctx, &xs, &y_temps);
        let _ = ctx.stroke();

        // Temp markers
        ctx.set_source_rgb(marker_color.0, marker_color.1, marker_color.2);
        for i in 0..count {
            ctx.arc(xs[i], y_temps[i], 3.0, 0.0, std::f64::consts::PI * 2.0);
            let _ = ctx.fill();
        }

        // POP labels
        ctx.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        ctx.set_font_size(10.0);
        for i in 0..count {
            let pop = data_for_draw[i].pop.unwrap_or(0.0).clamp(0.0, 1.0);
            if pop < 0.05 {
                continue;
            }
            let label = format!("{:.0}%", pop * 100.0);
            if let Ok(ext) = ctx.text_extents(&label) {
                let y = precip_bottom + 14.0;
                if pop >= 0.90 {
                    ctx.set_source_rgba(pop_label.0, pop_label.1, pop_label.2, 0.85);
                    ctx.select_font_face(
                        "Sans",
                        gtk::cairo::FontSlant::Normal,
                        gtk::cairo::FontWeight::Bold,
                    );
                    ctx.move_to(xs[i] - ext.width() / 2.0, y);
                    let _ = ctx.show_text(&label);
                    // glow
                    ctx.set_source_rgba(pop_label.0, pop_label.1, pop_label.2, 0.35);
                    ctx.move_to(xs[i] - ext.width() / 2.0, y);
                    let _ = ctx.show_text(&label);
                } else {
                    if pop >= 0.50 {
                        ctx.select_font_face(
                            "Sans",
                            gtk::cairo::FontSlant::Normal,
                            gtk::cairo::FontWeight::Bold,
                        );
                    } else {
                        ctx.select_font_face(
                            "Sans",
                            gtk::cairo::FontSlant::Normal,
                            gtk::cairo::FontWeight::Normal,
                        );
                    }
                    ctx.set_source_rgba(pop_label.0, pop_label.1, pop_label.2, pop_label.3);
                    ctx.move_to(xs[i] - ext.width() / 2.0, y);
                    let _ = ctx.show_text(&label);
                }
            }
        }

        // Time labels
        ctx.set_font_size(10.0);
        for i in 0..count {
            let local_t = data_for_draw[i].dt + tz_offset as i64;
            let time_str = fmt_time(data_for_draw[i].dt, tz_offset as i64, "%H:%M");
            if let Ok(ext) = ctx.text_extents(&time_str) {
                let past = local_t < now_local;
                let alpha = if past { 0.4 } else { 1.0 };
                ctx.set_source_rgba(time_color.0, time_color.1, time_color.2, alpha);
                ctx.move_to(xs[i] - ext.width() / 2.0, time_axis_y);
                let _ = ctx.show_text(&time_str);
            }
        }

        // "Now" marker: find nearest interval
        let mut now_x: Option<f64> = None;
        if count == 1 {
            if (now_local - local_times[0]).abs() <= 3600 {
                now_x = Some(xs[0]);
            }
        } else {
            for i in 0..count - 1 {
                let t0 = local_times[i];
                let t1 = local_times[i + 1];
                if now_local >= t0 && now_local <= t1 && t1 > t0 {
                    let ratio = (now_local - t0) as f64 / (t1 - t0) as f64;
                    let x = xs[i] + (xs[i + 1] - xs[i]) * ratio;
                    now_x = Some(x);
                    break;
                }
            }
        }

        if let Some(x) = now_x {
            let pulse = 4.0 + (st.ribbon_phase.sin() + 1.0) * 1.5;
            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.2);
            ctx.set_line_width(1.0);
            ctx.move_to(x, temp_top);
            ctx.line_to(x, precip_bottom);
            let _ = ctx.stroke();

            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.12);
            ctx.arc(
                x,
                temp_top + (temp_bot - temp_top) * 0.5,
                pulse,
                0.0,
                std::f64::consts::PI * 2.0,
            );
            let _ = ctx.fill();
        }

        // Temp scale ticks/labels (left)
        ctx.set_source_rgba(tick_color.0, tick_color.1, tick_color.2, tick_color.3);
        ctx.set_line_width(1.0);
        ctx.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        ctx.set_font_size(10.0);
        for t in ticks {
            let y_tick = temp_to_y(t);
            // tick line
            ctx.move_to(left_margin - 6.0, y_tick);
            ctx.line_to(left_margin - 2.0, y_tick);
            let _ = ctx.stroke();

            // label
            let text = format!("{:.0}°", t);
            if let Ok(ext) = ctx.text_extents(&text) {
                ctx.set_source_rgb(tick_label_color.0, tick_label_color.1, tick_label_color.2);
                ctx.move_to(left_margin - 8.0 - ext.width(), y_tick + ext.height() / 2.0);
                let _ = ctx.show_text(&text);
            }
            ctx.set_source_rgba(tick_color.0, tick_color.1, tick_color.2, tick_color.3);
        }
    });

    area
}

fn build_smooth_path(ctx: &gtk::cairo::Context, xs: &[f64], ys: &[f64]) {
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
