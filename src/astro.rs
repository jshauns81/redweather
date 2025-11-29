//! Custom widget for visualizing the sun's position
//!
//! Draws a semi-circle arc representing the daylight period.

use gtk::prelude::*;
use gtk::DrawingArea;
use std::f64::consts::PI;

use crate::utils::fmt_time;

#[allow(dead_code)]
pub fn create_sun_widget(sunrise: i64, sunset: i64, current: i64, tz: i64) -> DrawingArea {
    let canvas = DrawingArea::new();
    canvas.set_content_height(120);
    canvas.set_hexpand(true);
    
    canvas.set_draw_func(move |_area, ctx, w, h| {
        let width = w as f64;
        let height = h as f64;
        
        // Colors
        let color_arc = (1.0, 1.0, 1.0, 0.2); // Faint white
        let color_sun = (0.98, 0.7, 0.2, 1.0); // Orange/Yellow
        let color_text = (0.7, 0.7, 0.8, 1.0); // Text gray

        // Geometry
        let center_x = width / 2.0;
        let center_y = height - 25.0; // Leave space for labels
        let radius = (width.min(height * 2.0) / 2.0) - 40.0;

        // Calculate Progress
        // Clamp current time to sunrise/sunset for the visual arc
        let progress = if current < sunrise {
            0.0
        } else if current > sunset {
            1.0
        } else {
            (current as f64 - sunrise as f64) / (sunset as f64 - sunrise as f64)
        };

        // Draw Arc (Background)
        ctx.set_source_rgba(color_arc.0, color_arc.1, color_arc.2, color_arc.3);
        ctx.set_line_width(2.0);
        // Dashed line
        ctx.set_dash(&[4.0, 4.0], 0.0);
        // Arc from PI (left) to 0 (right) - Cairo angles are clockwise from 3 o'clock (0).
        // We want Left (PI) -> Top -> Right (0).
        // So arc goes from PI to 0 negative direction (counter clockwise)?
        // actually arc_negative(xc, yc, r, angle1, angle2).
        ctx.arc(center_x, center_y, radius, PI, 0.0); 
        ctx.stroke().expect("Failed to stroke arc");
        ctx.set_dash(&[], 0.0); // Reset dash

        // Draw Progress Arc (Active daylight) - Optional?
        // Maybe just draw the sun.

        // Draw Sun
        let angle = PI + (progress * (0.0 - PI)); // Interpolate PI -> 0
        // Wait, PI + p * (-PI) = PI * (1 - p).
        // If p=0 (rise), angle = PI (Left). Correct.
        // If p=0.5 (noon), angle = 0.5 PI (Top). Wait, Top is 1.5 PI or -0.5 PI.
        // Cairo: 0 is Right. PI is Left. 
        // Clockwise: 0 -> 0.5PI (Down) -> PI (Left).
        // We want PI (Left) -> Up -> 0 (Right).
        // This is Counter-Clockwise (Negative).
        // So angles are PI down to 0.
        
        let sun_x = center_x + radius * angle.cos();
        let sun_y = center_y + radius * angle.sin();

        // Draw Sun Glow
        let gradient = gtk::cairo::RadialGradient::new(sun_x, sun_y, 2.0, sun_x, sun_y, 15.0);
        gradient.add_color_stop_rgba(0.0, 0.98, 0.8, 0.2, 0.8);
        gradient.add_color_stop_rgba(1.0, 0.98, 0.8, 0.2, 0.0);
        let _ = ctx.set_source(&gradient);
        ctx.arc(sun_x, sun_y, 15.0, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed sun glow");

        // Draw Sun Core
        ctx.set_source_rgba(color_sun.0, color_sun.1, color_sun.2, color_sun.3);
        ctx.arc(sun_x, sun_y, 6.0, 0.0, 2.0 * PI);
        ctx.fill().expect("Failed sun core");

        // Draw Labels
        ctx.set_source_rgba(color_text.0, color_text.1, color_text.2, color_text.3);
        ctx.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
        ctx.set_font_size(12.0);

        let rise_text = fmt_time(sunrise, tz, "%H:%M");
        let set_text = fmt_time(sunset, tz, "%H:%M");

        // Rise Label (Left)
        let ext_r = ctx.text_extents(&rise_text).unwrap();
        // Position near the start of arc
        ctx.move_to(center_x - radius - ext_r.width() / 2.0, center_y + 20.0);
        let _ = ctx.show_text(&rise_text);
        
        // Set Label (Right)
        let ext_s = ctx.text_extents(&set_text).unwrap();
        ctx.move_to(center_x + radius - ext_s.width() / 2.0, center_y + 20.0);
        let _ = ctx.show_text(&set_text);
        
        // "Horizon" labels
        ctx.set_font_size(10.0);
        ctx.move_to(center_x - radius - 10.0, center_y + 35.0);
        let _ = ctx.show_text("Sunrise");
        
        ctx.move_to(center_x + radius - 10.0, center_y + 35.0);
        let _ = ctx.show_text("Sunset");
    });

    canvas
}
