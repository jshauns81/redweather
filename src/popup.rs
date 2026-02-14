//! GTK4 Layer Shell weather popup
//!
//! A lightweight overlay panel that appears above the Waybar when triggered
//! via `--popup`. Uses gtk4-layer-shell to anchor to the top edge on the
//! Overlay layer so it floats over windows but below the bar.

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, EventControllerKey, Grid, Label, Orientation,
    Separator,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{ColorsResolved, Config, TempBand, UiConfigResolved, Units};
use crate::utils::{
    deg_to_dir, fmt_time, humidity_color, is_night, moon_phase_icon, pick_icon, temp_color,
    uvi_color,
};
use crate::weather::ApiResponse;

// ── Colour palette (matches dashboard dark theme) ──────────────────────
const _BG: &str = "#0b0f1f";
const _BG_CARD: &str = "#101831";
const _FG: &str = "#d9e1ff";
const _FG_DIM: &str = "#a0accf";
const _FG_MUTED: &str = "#7a8baf";
const _ACCENT: &str = "#c4a7e7";
const _BORDER: &str = "rgba(255,255,255,0.06)";

// ── CSS ────────────────────────────────────────────────────────────────
const POPUP_CSS: &str = r#"
    .popup-window {
        background: #0b0f1f;
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 12px;
        padding: 0;
    }

    .popup-main {
        padding: 14px 18px 12px;
    }

    /* ── Current conditions ── */
    .current-icon  { font-size: 2.4rem; }
    .current-temp  { font-size: 1.8rem; font-weight: 780; }
    .current-desc  { font-size: 0.95rem; color: #d9e1ff; text-transform: capitalize; }
    .current-feels { font-size: 0.82rem; color: #a0accf; }

    .detail-label  { font-size: 0.78rem; color: #7a8baf; }
    .detail-value  { font-size: 0.82rem; color: #d9e1ff; font-weight: 600; }

    /* ── Sections ── */
    .section-header {
        font-size: 0.78rem;
        font-weight: 700;
        color: #c4a7e7;
        letter-spacing: 0.04rem;
    }

    .separator {
        background: rgba(255,255,255,0.06);
        min-height: 1px;
    }

    /* ── Hourly / Daily rows ── */
    .hour-time, .day-name  { font-size: 0.80rem; color: #a0accf; min-width: 40px; }
    .hour-icon, .day-icon  { font-size: 1.05rem; min-width: 24px; }
    .hour-temp, .day-hi    { font-size: 0.82rem; font-weight: 600; min-width: 36px; }
    .day-lo                { font-size: 0.82rem; color: #7a8baf; min-width: 36px; }
    .hour-desc, .day-desc  { font-size: 0.78rem; color: #a0accf; }
    .hour-pop, .day-pop    { font-size: 0.78rem; color: #89b4fa; min-width: 40px; }
    .hour-pop-zero, .day-pop-zero { font-size: 0.78rem; color: #3b4261; min-width: 40px; }
"#;

// ═══════════════════════════════════════════════════════════════════════
//  Public entry point
// ═══════════════════════════════════════════════════════════════════════

/// Launch the popup overlay.
///
/// * Anchored to top-right of the screen (near a typical Waybar weather module)
/// * Escape key or focus-loss closes the popup
/// * Displays the same data that the Pango tooltip showed, but with proper layout
pub fn run_popup(data: ApiResponse, location_label: &str, cfg: &Config) {
    let label = location_label.to_owned();
    let cfg = cfg.clone();

    let app = Application::builder()
        .application_id("com.shaun.redweather.popup")
        .build();

    app.connect_activate(move |app| {
        build_popup(app, &data, &label, &cfg);
    });

    app.run_with_args::<String>(&[]);
}

// ═══════════════════════════════════════════════════════════════════════
//  Window construction
// ═══════════════════════════════════════════════════════════════════════

fn build_popup(app: &Application, data: &ApiResponse, location_label: &str, cfg: &Config) {
    // ── Load CSS ───────────────────────────────────────────────────────
    let provider = gtk::CssProvider::new();
    provider.load_from_data(POPUP_CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("No display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // ── Resolve config ─────────────────────────────────────────────────
    let ui = UiConfigResolved::from_config(&cfg.ui);
    let _colors = ColorsResolved::from_config(&cfg.colors);
    let bands = TempBand::from_config(&cfg.temp_bands);
    let units = cfg.units;

    // ── Create window ──────────────────────────────────────────────────
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(1)   // layer-shell sizes to content
        .default_height(1)
        .decorated(false)
        .resizable(false)
        .build();

    // ── Layer Shell setup ──────────────────────────────────────────────
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("redweather-popup"));
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    // Anchor to top-right, floating below bar
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 40);   // below typical waybar height
    window.set_margin(Edge::Right, 10);

    window.add_css_class("popup-window");

    // ── Close on Escape ────────────────────────────────────────────────
    let key_ctl = EventControllerKey::new();
    {
        let win = window.downgrade();
        key_ctl.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(w) = win.upgrade() {
                    w.close();
                }
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }
    window.add_controller(key_ctl);

    // ── Close on focus loss ────────────────────────────────────────────
    {
        let win = window.downgrade();
        window.connect_is_active_notify(move |_| {
            if let Some(w) = win.upgrade() {
                if !w.is_active() {
                    w.close();
                }
            }
        });
    }

    // ── Build content ──────────────────────────────────────────────────
    let main_box = GtkBox::new(Orientation::Vertical, 8);
    main_box.add_css_class("popup-main");

    // Current conditions
    main_box.append(&build_current_section(data, location_label, &bands, units));

    // Divider
    main_box.append(&make_sep());

    // Hourly header + rows
    main_box.append(&section_header("HOURLY FORECAST"));
    main_box.append(&build_hourly_grid(data, &bands, &ui));

    // Divider
    main_box.append(&make_sep());

    // Daily header + rows
    main_box.append(&section_header("5-DAY FORECAST"));
    main_box.append(&build_daily_grid(data, &bands, &ui));

    window.set_child(Some(&main_box));
    window.present();
}

// ═══════════════════════════════════════════════════════════════════════
//  Current conditions
// ═══════════════════════════════════════════════════════════════════════

fn build_current_section(
    data: &ApiResponse,
    location_label: &str,
    bands: &[TempBand],
    units: Units,
) -> GtkBox {
    let (temp_unit, speed_unit, dist_unit) = match units {
        Units::Imperial => ("°F", "mph", "mi"),
        Units::Metric => ("°C", "m/s", "km"),
    };

    let desc = data
        .current
        .weather
        .first()
        .cloned()
        .unwrap_or(crate::weather::WeatherDesc {
            main: Some("Clear".into()),
            description: Some("Clear".into()),
        });

    let night = is_night(data.current.dt, data.current.sunrise, data.current.sunset);
    let moon = Some(moon_phase_icon(data.current.dt, data.timezone_offset));
    let icon = pick_icon(&desc, night, moon);

    let temp = data.current.temp.round();
    let feels = data.current.feels_like.map(|f| f.round()).unwrap_or(temp);
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
        .first()
        .and_then(|h| h.pop)
        .map(|p| (p * 100.0).round() as i64)
        .unwrap_or(0);
    let sunrise = data
        .current
        .sunrise
        .map(|t| fmt_time(t, data.timezone_offset, "%H:%M"))
        .unwrap_or_else(|| "—".into());
    let sunset = data
        .current
        .sunset
        .map(|t| fmt_time(t, data.timezone_offset, "%H:%M"))
        .unwrap_or_else(|| "—".into());

    // Today hi/lo from daily
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

    let outer = GtkBox::new(Orientation::Vertical, 6);

    // ── Row 1: icon + temp + desc ──────────────────────────────────────
    let top_row = GtkBox::new(Orientation::Horizontal, 12);
    top_row.set_halign(gtk::Align::Center);

    let icon_lbl = Label::new(Some(&icon));
    icon_lbl.add_css_class("current-icon");

    let temp_lbl = markup_label(
        &format!(
            "<span foreground='{}'>{:.0}{}</span>",
            temp_color(temp, bands),
            temp,
            temp_unit
        ),
        "current-temp",
    );

    let info_col = GtkBox::new(Orientation::Vertical, 2);
    info_col.set_valign(gtk::Align::Center);
    let loc_lbl = Label::new(Some(location_label));
    loc_lbl.add_css_class("current-desc");
    loc_lbl.set_halign(gtk::Align::Start);
    let desc_text = desc.description.as_deref().unwrap_or("N/A");
    let desc_lbl = Label::new(Some(desc_text));
    desc_lbl.add_css_class("current-desc");
    desc_lbl.set_halign(gtk::Align::Start);
    let feels_lbl = Label::new(Some(&format!("Feels {:.0}°", feels)));
    feels_lbl.add_css_class("current-feels");
    feels_lbl.set_halign(gtk::Align::Start);
    info_col.append(&loc_lbl);
    info_col.append(&desc_lbl);
    info_col.append(&feels_lbl);

    top_row.append(&icon_lbl);
    top_row.append(&temp_lbl);
    top_row.append(&info_col);
    outer.append(&top_row);

    // ── Row 2: detail grid (2 columns × 4 rows) ───────────────────────
    let grid = Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(16);
    grid.set_halign(gtk::Align::Center);
    grid.set_margin_top(6);

    let hi_col_hex = temp_color(today_hi, bands);
    let lo_col_hex = temp_color(today_lo, bands);

    let details: Vec<(&str, String)> = vec![
        (
            "Hi / Lo",
            format!(
                "<span foreground='{}'>{:.0}°</span> / <span foreground='{}'>{:.0}°</span>",
                hi_col_hex, today_hi, lo_col_hex, today_lo
            ),
        ),
        (
            "Humidity",
            format!(
                "<span foreground='{}'>{humidity}%</span>",
                humidity_color(humidity)
            ),
        ),
        (
            "Dew Point",
            dew_point
                .map(|d| format!("{:.0}°", d))
                .unwrap_or_else(|| "—".into()),
        ),
        (
            "Wind",
            {
                let gust = wind_gust
                    .filter(|&g| g > wind_speed + 5.0)
                    .map(|g| format!(" (gust {:.0})", g))
                    .unwrap_or_default();
                format!("{:.0} {} {}{}", wind_speed, speed_unit, wind_dir, gust)
            },
        ),
        (
            "UV Index",
            format!(
                "<span foreground='{}'>{:.0}</span>",
                uvi_color(uvi),
                uvi.round()
            ),
        ),
        ("Pressure", format!("{} hPa", pressure)),
        (
            "Visibility",
            format!("{:.0} {}", visibility, dist_unit),
        ),
        (
            "Sunrise / Set",
            format!("☀ {} — {}", sunrise, sunset),
        ),
    ];

    // Rain chance as extra row
    let mut extra_details = details;
    if pop > 0 {
        extra_details.push(("Rain Chance", format!("{}%", pop)));
    }

    // Lay out in two columns (label | value, label | value)
    let cols = 2;
    for (i, (lbl_text, val_text)) in extra_details.iter().enumerate() {
        let col = (i % cols) as i32 * 2; // col 0 or 2
        let row = (i / cols) as i32;

        let lbl = Label::new(Some(lbl_text));
        lbl.add_css_class("detail-label");
        lbl.set_halign(gtk::Align::End);

        let val = Label::new(None);
        val.set_markup(val_text);
        val.add_css_class("detail-value");
        val.set_halign(gtk::Align::Start);

        grid.attach(&lbl, col, row, 1, 1);
        grid.attach(&val, col + 1, row, 1, 1);
    }

    outer.append(&grid);
    outer
}

// ═══════════════════════════════════════════════════════════════════════
//  Hourly grid
// ═══════════════════════════════════════════════════════════════════════

fn build_hourly_grid(data: &ApiResponse, bands: &[TempBand], _ui: &UiConfigResolved) -> Grid {
    let grid = Grid::new();
    grid.set_row_spacing(3);
    grid.set_column_spacing(8);

    // Columns: Time | Icon | Temp | Description | PoP
    for (i, h) in data.hourly.iter().take(12).enumerate() {
        let row = i as i32;

        // Time
        let time_str = fmt_time(h.dt, data.timezone_offset, "%-I%p");
        let time_lbl = Label::new(Some(&time_str));
        time_lbl.add_css_class("hour-time");
        time_lbl.set_halign(gtk::Align::End);

        // Icon
        let local_hour = ((h.dt + data.timezone_offset) / 3600) % 24;
        let night = local_hour < 6 || local_hour >= 18;
        let icon = h
            .weather
            .first()
            .map(|d| pick_icon(d, night, Some(moon_phase_icon(h.dt, data.timezone_offset))))
            .unwrap_or_else(|| "❓".into());
        let icon_lbl = Label::new(Some(&icon));
        icon_lbl.add_css_class("hour-icon");

        // Temp (coloured)
        let t = h.temp.round();
        let temp_lbl = markup_label(
            &format!(
                "<span foreground='{}'>{:.0}°</span>",
                temp_color(t, bands),
                t
            ),
            "hour-temp",
        );
        temp_lbl.set_halign(gtk::Align::End);

        // Description
        let raw_desc = h
            .weather
            .first()
            .and_then(|w| w.description.as_ref())
            .cloned()
            .unwrap_or_else(|| "—".into());
        let desc_lbl = Label::new(Some(&raw_desc));
        desc_lbl.add_css_class("hour-desc");
        desc_lbl.set_halign(gtk::Align::Start);
        desc_lbl.set_hexpand(true);

        // PoP
        let pop = h.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
        let pop_lbl = if pop > 0 {
            let l = Label::new(Some(&format!("💧{}%", pop)));
            l.add_css_class("hour-pop");
            l
        } else {
            let l = Label::new(Some("—"));
            l.add_css_class("hour-pop-zero");
            l
        };
        pop_lbl.set_halign(gtk::Align::End);

        grid.attach(&time_lbl, 0, row, 1, 1);
        grid.attach(&icon_lbl, 1, row, 1, 1);
        grid.attach(&temp_lbl, 2, row, 1, 1);
        grid.attach(&desc_lbl, 3, row, 1, 1);
        grid.attach(&pop_lbl, 4, row, 1, 1);
    }

    grid
}

// ═══════════════════════════════════════════════════════════════════════
//  Daily grid
// ═══════════════════════════════════════════════════════════════════════

fn build_daily_grid(data: &ApiResponse, bands: &[TempBand], _ui: &UiConfigResolved) -> Grid {
    let grid = Grid::new();
    grid.set_row_spacing(3);
    grid.set_column_spacing(8);

    // Columns: Day | Icon | Hi | Lo | Description | PoP
    for (i, d) in data.daily.iter().take(5).enumerate() {
        let row = i as i32;

        let day_str = fmt_time(d.dt, data.timezone_offset, "%a");
        let day_lbl = Label::new(Some(&day_str));
        day_lbl.add_css_class("day-name");
        day_lbl.set_halign(gtk::Align::End);

        let icon = d
            .weather
            .first()
            .map(|desc| pick_icon(desc, false, None))
            .unwrap_or_else(|| "❓".into());
        let icon_lbl = Label::new(Some(&icon));
        icon_lbl.add_css_class("day-icon");

        let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
        let lo = d.temp.min.unwrap_or(0.0).round();

        let hi_lbl = markup_label(
            &format!(
                "<span foreground='{}'>{:.0}°</span>",
                temp_color(hi, bands),
                hi
            ),
            "day-hi",
        );
        hi_lbl.set_halign(gtk::Align::End);

        let lo_lbl = markup_label(
            &format!(
                "<span foreground='{}'>{:.0}°</span>",
                temp_color(lo, bands),
                lo
            ),
            "day-lo",
        );
        lo_lbl.set_halign(gtk::Align::End);

        let raw_desc = d
            .weather
            .first()
            .and_then(|w| w.description.as_ref())
            .cloned()
            .unwrap_or_else(|| "—".into());
        let desc_lbl = Label::new(Some(&raw_desc));
        desc_lbl.add_css_class("day-desc");
        desc_lbl.set_halign(gtk::Align::Start);
        desc_lbl.set_hexpand(true);

        let pop = d.pop.map(|p| (p * 100.0).round() as i64).unwrap_or(0);
        let pop_lbl = if pop > 0 {
            let l = Label::new(Some(&format!("💧{}%", pop)));
            l.add_css_class("day-pop");
            l
        } else {
            let l = Label::new(Some("—"));
            l.add_css_class("day-pop-zero");
            l
        };
        pop_lbl.set_halign(gtk::Align::End);

        grid.attach(&day_lbl, 0, row, 1, 1);
        grid.attach(&icon_lbl, 1, row, 1, 1);
        grid.attach(&hi_lbl, 2, row, 1, 1);
        grid.attach(&lo_lbl, 3, row, 1, 1);
        grid.attach(&desc_lbl, 4, row, 1, 1);
        grid.attach(&pop_lbl, 5, row, 1, 1);
    }

    grid
}

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

/// Create a Label with Pango markup and a CSS class.
fn markup_label(markup: &str, css_class: &str) -> Label {
    let lbl = Label::new(None);
    lbl.set_markup(markup);
    lbl.add_css_class(css_class);
    lbl
}

/// Horizontal separator with muted colour.
fn make_sep() -> Separator {
    let sep = Separator::new(Orientation::Horizontal);
    sep.add_css_class("separator");
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    sep
}

/// Section header label.
fn section_header(text: &str) -> Label {
    let lbl = Label::new(Some(text));
    lbl.add_css_class("section-header");
    lbl.set_halign(gtk::Align::Start);
    lbl.set_margin_bottom(2);
    lbl
}
