//! GTK4 Dashboard for displaying rich weather information
//!
//! This module implements the graphical dashboard view of the application.

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, DrawingArea, FlowBox, GestureDrag,
    HeaderBar, Label, Orientation, ScrolledWindow, Separator, Spinner,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::{load_config, Config, DashboardConfigResolved, Units};
use crate::gauges::{create_arc_gauge, create_compass_gauge};
use crate::graph::create_hourly_graph;
use crate::ui::show_location_dialog;
use crate::utils::{deg_to_dir, fmt_time, pick_icon};
use crate::weather::{fetch_weather_for_loc, resolve_location, ApiResponse, Location, WeatherDesc};

pub fn run_dashboard(
    data: Option<ApiResponse>,
    loc: Location,
    units: Units,
    key: String,
    cfg: Config,
) {
    let app = Application::builder()
        .application_id("com.shaun.redweather.dashboard")
        .build();

    let data = Rc::new(data);
    let loc = Rc::new(loc);
    let key = Rc::new(key);
    let cfg = Rc::new(cfg);

    app.connect_activate(move |app| {
        build_ui(
            app,
            data.clone(),
            loc.clone(),
            units,
            key.clone(),
            cfg.clone(),
        );
    });

    app.run_with_args::<String>(&[]);
}

fn build_ui(
    app: &Application,
    data: Rc<Option<ApiResponse>>,
    loc: Rc<Location>,
    units: Units,
    key: Rc<String>,
    cfg: Rc<Config>,
) {
    let d_cfg = DashboardConfigResolved::from_config(&cfg.dashboard);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RedWeather Dashboard")
        .default_width(d_cfg.window_width)
        .default_height(d_cfg.window_height)
        .build();

    // Save window geometry on close
    window.connect_close_request(move |win| {
        let width = win.width();
        let height = win.height();

        // Load fresh config to ensure we don't overwrite other changes
        let mut current_cfg = load_config();
        let dash_cfg = current_cfg
            .dashboard
            .get_or_insert_with(crate::config::DashboardConfig::default);
        dash_cfg.window_width = Some(width);
        dash_cfg.window_height = Some(height);

        if let Err(e) = crate::config::save_config(&current_cfg) {
            eprintln!("Failed to save window state: {}", e);
        }

        gtk::glib::Propagation::Proceed
    });

    // Apply custom CSS
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE_CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Could not connect to a display."),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Header Bar with Settings Button
    let header = HeaderBar::new();
    header.set_show_title_buttons(true);

    let settings_btn = Button::from_icon_name("emblem-system-symbolic");
    settings_btn.set_tooltip_text(Some("Change Location"));
    header.pack_end(&settings_btn);
    window.set_titlebar(Some(&header));

    let main_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    window.set_child(Some(&main_scroll));

    // Initial Draw

    refresh_content(&main_scroll, data.as_ref().as_ref(), &loc, units, &cfg);

    // Spawn background refresh

    let k = key.clone();

    let l = loc.clone();

    let s_weak = main_scroll.downgrade();

    let c_bg = cfg.clone();

    glib::spawn_future_local(async move {
        match fetch_weather_for_loc(&k, &l, units).await {
            Ok(new_data) => {
                if let Some(scroll) = s_weak.upgrade() {
                    refresh_content(&scroll, Some(&new_data), &l, units, &c_bg);
                }
            }

            Err(e) => {
                if let Some(scroll) = s_weak.upgrade() {
                    show_error_ui(&scroll, &e.to_string());
                }
            }
        }
    });

    // Settings Button Logic

    let win_weak = window.downgrade();

    let key_clone = key.clone();

    let scroll_weak = main_scroll.downgrade();

    settings_btn.connect_clicked(move |_| {
        if let Some(win) = win_weak.upgrade() {
            let k = key_clone.clone();

            let c = load_config();

            let s_weak = scroll_weak.clone();

            let k_for_fetch = k.clone();

            show_location_dialog(&win, &k, &c, move || {
                let k2 = k_for_fetch.clone();

                let s_weak2 = s_weak.clone();

                glib::spawn_future_local(async move {
                    let new_cfg = load_config();

                    if let Some(new_loc) = resolve_location(&k2, None, &new_cfg).await {
                        // Show loading state immediately

                        if let Some(scroll) = s_weak2.upgrade() {
                            refresh_content(&scroll, None, &new_loc, units, &new_cfg);
                        }

                        match fetch_weather_for_loc(&k2, &new_loc, new_cfg.units).await {
                            Ok(new_data) => {
                                if let Some(scroll) = s_weak2.upgrade() {
                                    refresh_content(
                                        &scroll,
                                        Some(&new_data),
                                        &new_loc,
                                        new_cfg.units,
                                        &new_cfg,
                                    );
                                }
                            }

                            Err(e) => {
                                if let Some(scroll) = s_weak2.upgrade() {
                                    show_error_ui(&scroll, &e.to_string());
                                }
                            }
                        }
                    }
                });
            });
        }
    });

    window.show();
}

fn show_error_ui(scroll: &ScrolledWindow, msg: &str) {
    if let Some(_child) = scroll.child() {
        scroll.set_child(gtk::Widget::NONE);
    }

    let vbox = GtkBox::new(Orientation::Vertical, 10);

    vbox.set_valign(gtk::Align::Center);

    vbox.set_halign(gtk::Align::Center);

    let icon = Label::new(Some("⚠️"));

    icon.add_css_class("hero-icon"); // Reuse large font

    vbox.append(&icon);

    let title = Label::new(Some("Weather Unavailable"));

    title.add_css_class("section-title");

    vbox.append(&title);

    let err_lbl = Label::new(Some(msg));

    err_lbl.set_wrap(true);

    err_lbl.set_max_width_chars(40);

    err_lbl.add_css_class("detail-title"); // Reuse gray text

    vbox.append(&err_lbl);

    scroll.set_child(Some(&vbox));
}

fn refresh_content(
    scroll: &ScrolledWindow,
    data_opt: Option<&ApiResponse>,
    loc: &Location,
    units: Units,
    cfg: &Config,
) {
    // Clear existing children
    if let Some(_child) = scroll.child() {
        scroll.set_child(gtk::Widget::NONE);
    }

    let d_cfg = DashboardConfigResolved::from_config(&cfg.dashboard);

    let vbox = GtkBox::new(Orientation::Vertical, 20);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);
    vbox.add_css_class("main-container");

    // Show spinner if no data
    let data = match data_opt {
        Some(d) => d,
        None => {
            let spinner = Spinner::new();
            spinner.start();
            spinner.set_vexpand(true);
            spinner.set_hexpand(true);
            spinner.set_halign(gtk::Align::Center);
            spinner.set_valign(gtk::Align::Center);
            spinner.set_size_request(64, 64);

            let label = Label::new(Some(&format!("Loading weather for {}...", loc.label)));
            label.add_css_class("date-subtitle");

            vbox.append(&spinner);
            vbox.append(&label);
            scroll.set_child(Some(&vbox));
            return;
        }
    };

    // --- Header Section ---
    let header_box = GtkBox::new(Orientation::Vertical, 5);
    let loc_label = Label::new(Some(&loc.label));
    loc_label.add_css_class("location-title");
    loc_label.set_wrap(true);
    loc_label.set_justify(gtk::Justification::Center);

    let date_str = fmt_time(data.current.dt, data.timezone_offset, "%A, %B %d %Y");
    let date_label = Label::new(Some(&date_str));
    date_label.add_css_class("date-subtitle");

    header_box.append(&loc_label);
    header_box.append(&date_label);
    vbox.append(&header_box);

    // --- Current Weather Section ---
    let current_box = GtkBox::new(Orientation::Horizontal, 20);
    current_box.set_halign(gtk::Align::Center);

    let current_desc = data.current.weather.get(0).cloned().unwrap_or(WeatherDesc {
        main: Some("Clear".into()),
        description: Some("Clear".into()),
    });
    let icon = pick_icon(&current_desc);

    let icon_label = Label::new(Some(icon));
    icon_label.add_css_class("hero-icon");

    let temp_info_box = GtkBox::new(Orientation::Vertical, 0);
    let (temp_unit, speed_unit) = match units {
        Units::Imperial => ("°F", "mph"),
        Units::Metric => ("°C", "m/s"),
    };
    let feels_like = data.current.feels_like.unwrap_or(data.current.temp).round();
    let temp_label = Label::new(Some(&format!(
        "{:.0}{}",
        data.current.temp.round(),
        temp_unit
    )));
    temp_label.add_css_class("hero-temp");

    let desc_text = current_desc
        .main
        .clone()
        .or(current_desc.description.clone())
        .unwrap_or_default();
    let desc_label = Label::new(Some(&desc_text));
    desc_label.add_css_class("hero-desc");

    let feels_label = Label::new(Some(&format!("Feels like {:.0}{}", feels_like, temp_unit)));
    feels_label.add_css_class("hero-feels");

    temp_info_box.append(&temp_label);
    temp_info_box.append(&desc_label);
    temp_info_box.append(&feels_label);

    current_box.append(&icon_label);
    current_box.append(&temp_info_box);
    vbox.append(&current_box);

    let humidity = data.current.humidity.unwrap_or(0);
    let uvi = data.current.uvi.unwrap_or(0.0);
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_dir = deg_to_dir(data.current.wind_deg);

    // --- Gauges ---
    vbox.append(&Separator::new(Orientation::Horizontal));
    let gauges_label = Label::new(Some("Live Gauges"));
    gauges_label.add_css_class("section-title");
    gauges_label.set_halign(gtk::Align::Start);
    vbox.append(&gauges_label);

    let gauge_flow = FlowBox::new();
    gauge_flow.set_selection_mode(gtk::SelectionMode::None);
    gauge_flow.set_max_children_per_line(4);
    gauge_flow.set_min_children_per_line(2);
    gauge_flow.set_row_spacing(8);
    gauge_flow.set_column_spacing(8);

    let humidity_gauge = create_arc_gauge(
        (humidity as f64 / 100.0).clamp(0.0, 1.0),
        format!("{}%", humidity),
        (0.35, 0.67, 0.96),
    );
    let humidity_note = match humidity {
        h if h >= 70 => "Feels muggy",
        h if h >= 40 => "Comfortable",
        _ => "Dry air",
    };
    gauge_flow.insert(
        &create_gauge_card(
            "Humidity",
            humidity_gauge,
            humidity_note,
            &format!("Rel. humidity: {}%", humidity),
        ),
        -1,
    );

    let uvi_gauge = create_arc_gauge(
        (uvi / 11.0).clamp(0.0, 1.0),
        format!("{:.1}", uvi),
        (0.95, 0.77, 0.34),
    );
    let uv_note = match uvi {
        u if u >= 8.0 => "Very high - protect skin",
        u if u >= 6.0 => "High - limit midday sun",
        u if u >= 3.0 => "Moderate UV",
        _ => "Low UV risk",
    };
    gauge_flow.insert(
        &create_gauge_card("UV Index", uvi_gauge, uv_note, &format!("UV {:.1}", uvi)),
        -1,
    );

    let wind_degrees = data.current.wind_deg.unwrap_or(0) as f64;
    let wind_speed_text = format!("{:.0} {}", wind_speed, speed_unit);
    let wind_gauge = create_compass_gauge(wind_degrees, wind_speed_text.clone());
    let wind_note = format!("{} winds", wind_dir);
    gauge_flow.insert(
        &create_gauge_card(
            "Wind",
            wind_gauge,
            &wind_note,
            &format!("{} @ {:.0}°", wind_speed_text, wind_degrees),
        ),
        -1,
    );

    let sunrise = data.current.sunrise.unwrap_or(0);
    let sunset = data.current.sunset.unwrap_or(0);
    let daylight_caption = if sunrise > 0 && sunset > 0 && sunset > sunrise {
        let rise = fmt_time(sunrise, data.timezone_offset, "%I:%M %p");
        let set = fmt_time(sunset, data.timezone_offset, "%I:%M %p");
        format!("↑ {}   ↓ {}", rise, set)
    } else {
        "Sun times unavailable".into()
    };

    let daylight_progress = if sunrise > 0 && sunset > sunrise {
        ((data.current.dt - sunrise) as f64 / (sunset - sunrise) as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let daylight_text = if sunrise == 0 || sunset == 0 || sunset <= sunrise {
        "—".into()
    } else if data.current.dt < sunrise {
        let hrs = ((sunrise - data.current.dt) as f64 / 3600.0).max(0.0);
        format!("{:.1}h to dawn", hrs)
    } else if data.current.dt > sunset {
        "Night".into()
    } else {
        let hrs = ((sunset - data.current.dt) as f64 / 3600.0).max(0.0);
        format!("{:.1}h left", hrs)
    };
    let daylight_gauge = create_arc_gauge(daylight_progress, daylight_text, (0.94, 0.76, 0.39));
    gauge_flow.insert(
        &create_gauge_card(
            "Daylight",
            daylight_gauge,
            &daylight_caption,
            "Sun progress",
        ),
        -1,
    );

    vbox.append(&gauge_flow);

    vbox.append(&Separator::new(Orientation::Horizontal));

    // --- Hourly Forecast ---
    let hourly_label = Label::new(Some("Hourly Forecast"));
    hourly_label.add_css_class("section-title");
    hourly_label.set_halign(gtk::Align::Start);
    vbox.append(&hourly_label);

    let hourly_scroll = ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(200) // Increased for graph breathing room
        .build();
    enable_drag_scroll(&hourly_scroll);

    if d_cfg.show_hourly_graph {
        // Graph View
        let graph = create_hourly_graph(
            &data.hourly,
            d_cfg.forecast_hours,
            data.timezone_offset,
            &data.current,
        );
        hourly_scroll.set_child(Some(&graph));
    } else {
        // Card List View
        let hourly_box = GtkBox::new(Orientation::Horizontal, 15);
        hourly_box.set_margin_bottom(10);

        let mut last_day: Option<i64> = None;
        for h in data.hourly.iter().take(d_cfg.forecast_hours) {
            let day_bucket = (h.dt + data.timezone_offset) / 86_400;
            if let Some(prev) = last_day {
                if day_bucket != prev {
                    let sep = Separator::new(Orientation::Vertical);
                    sep.add_css_class("day-separator");
                    sep.set_margin_start(4);
                    sep.set_margin_end(4);
                    hourly_box.append(&sep);
                }
            }
            last_day = Some(day_bucket);

            let card = GtkBox::new(Orientation::Vertical, 5);
            card.add_css_class("forecast-card");

            let time_str = fmt_time(h.dt, data.timezone_offset, "%H:%M");
            let time_lbl = Label::new(Some(&time_str));
            time_lbl.add_css_class("forecast-time");

            let icon_str = h.weather.get(0).map(pick_icon).unwrap_or("❓");
            let icon_lbl = Label::new(Some(icon_str));
            icon_lbl.add_css_class("forecast-icon");

            let temp_lbl = Label::new(Some(&format!("{:.0}°", h.temp.round())));
            temp_lbl.add_css_class("forecast-temp");

            if let Some(pop) = h.pop {
                if pop > 0.0 {
                    let pop_lbl = Label::new(Some(&format!("POP {:.0}%", pop * 100.0)));
                    pop_lbl.add_css_class("forecast-pop");
                    pop_lbl.set_halign(gtk::Align::Center);
                    card.append(&pop_lbl);
                }
            }

            card.append(&time_lbl);
            card.append(&icon_lbl);
            card.append(&temp_lbl);
            hourly_box.append(&card);
        }
        hourly_scroll.set_child(Some(&hourly_box));
    }
    vbox.append(&hourly_scroll);

    vbox.append(&Separator::new(Orientation::Horizontal));

    // --- Daily Forecast ---
    let daily_label = Label::new(Some("Forecast"));
    daily_label.add_css_class("section-title");
    daily_label.set_halign(gtk::Align::Start);
    vbox.append(&daily_label);

    let daily_box = GtkBox::new(Orientation::Vertical, 10);
    for d in data.daily.iter().take(d_cfg.forecast_days) {
        let row = GtkBox::new(Orientation::Horizontal, 10);
        row.add_css_class("daily-row");

        let day_str = fmt_time(d.dt, data.timezone_offset, "%A");
        let day_lbl = Label::new(Some(&day_str));
        day_lbl.set_hexpand(true);
        day_lbl.set_halign(gtk::Align::Start);

        let icon_str = d.weather.get(0).map(pick_icon).unwrap_or("❓");
        let icon_lbl = Label::new(Some(icon_str));

        let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
        let lo = d.temp.min.unwrap_or(0.0).round();
        let pop_pct = d.pop.unwrap_or(0.0) * 100.0;

        let pill_box = GtkBox::new(Orientation::Horizontal, 6);
        let hi_lbl = Label::new(Some(&format!("Hi {:.0}°", hi)));
        hi_lbl.add_css_class("pill-hi");
        let lo_lbl = Label::new(Some(&format!("Lo {:.0}°", lo)));
        lo_lbl.add_css_class("pill-lo");
        pill_box.append(&hi_lbl);
        pill_box.append(&lo_lbl);

        if pop_pct > 0.0 {
            let pop_lbl = Label::new(Some(&format!("POP {:.0}%", pop_pct)));
            if pop_pct >= 70.0 {
                pop_lbl.add_css_class("pill-pop-high");
            } else {
                pop_lbl.add_css_class("pill-pop");
            }
            pill_box.append(&pop_lbl);
        }

        row.append(&day_lbl);
        row.append(&icon_lbl);
        row.append(&pill_box);
        daily_box.append(&row);
    }
    vbox.append(&daily_box);

    scroll.set_child(Some(&vbox));
}

fn create_gauge_card(title: &str, gauge: DrawingArea, caption: &str, detail: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.add_css_class("gauge-card");

    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("gauge-title");
    title_lbl.set_halign(gtk::Align::Start);

    let gauge_wrapper = GtkBox::new(Orientation::Vertical, 0);
    gauge_wrapper.set_halign(gtk::Align::Center);
    gauge_wrapper.append(&gauge);

    let caption_lbl = Label::new(Some(caption));
    caption_lbl.add_css_class("gauge-caption");
    caption_lbl.set_wrap(true);
    caption_lbl.set_max_width_chars(16);
    caption_lbl.set_halign(gtk::Align::Center);

    card.append(&title_lbl);
    card.append(&gauge_wrapper);
    card.append(&caption_lbl);

    let tooltip_text = if detail.is_empty() {
        format!("{} • {}", title, caption)
    } else {
        format!("{} • {} • {}", title, caption, detail)
    };
    card.set_tooltip_text(Some(&tooltip_text));

    card
}

fn enable_drag_scroll(scroll: &ScrolledWindow) {
    let drag = GestureDrag::new();
    let start = Rc::new(RefCell::new(0.0));

    let scroll_weak = scroll.downgrade();
    let start_begin = start.clone();
    drag.connect_drag_begin(move |_g, _x, _y| {
        if let Some(sw) = scroll_weak.upgrade() {
            let adj = sw.hadjustment();
            *start_begin.borrow_mut() = adj.value();
        }
    });

    let scroll_weak_update = scroll.downgrade();
    let start_update = start.clone();
    drag.connect_drag_update(move |_g, offset_x, _offset_y| {
        if let Some(sw) = scroll_weak_update.upgrade() {
            let adj = sw.hadjustment();
            let max_val = (adj.upper() - adj.page_size()).max(adj.lower());
            let mut new_val = *start_update.borrow() - offset_x;
            new_val = new_val.clamp(adj.lower(), max_val);
            adj.set_value(new_val);
        }
    });

    scroll.add_controller(drag);
}

const STYLE_CSS: &str = r#"
    window {
        background-color: #24283b;
        color: #c0caf5;
        font-family: Sans;
    }
    
    headerbar {
        background-color: #1f2335;
        border-bottom: 1px solid #16161e;
    }

    .location-title {
        font-size: 24px;
        font-weight: bold;
        color: #7aa2f7;
    }
    
    .date-subtitle {
        font-size: 14px;
        color: #565f89;
    }
    
    .hero-icon {
        font-size: 64px;
    }
    
    .hero-temp {
        font-size: 48px;
        font-weight: bold;
        color: #e0af68;
    }
    
    .hero-desc {
        font-size: 18px;
        color: #9aa5ce;
        text-transform: capitalize;
    }

    .hero-feels {
        font-size: 14px;
        color: #565f89;
    }
    
    .section-title {
        font-size: 18px;
        font-weight: bold;
        color: #bb9af7;
        margin-bottom: 5px;
    }
    
    .gauge-card {
        background-color: #1b1f30;
        padding: 8px;
        border-radius: 10px;
    }
    
    .detail-title {
        font-size: 12px;
        color: #565f89;
    }

    .gauge-title {
        font-size: 12px;
        font-weight: bold;
        color: #9aa5ce;
        margin-bottom: 4px;
    }

    .gauge-caption {
        font-size: 11px;
        color: #565f89;
        margin-top: 6px;
    }
    
    .forecast-card {
        background-color: #1f2335;
        padding: 10px;
        border-radius: 8px;
        min-width: 60px;
    }
    
    .forecast-time {
        font-size: 12px;
        color: #565f89;
    }
    
    .forecast-icon {
        font-size: 24px;
        margin-top: 5px;
        margin-bottom: 5px;
    }
    
    .forecast-temp {
        font-weight: bold;
    }

    .forecast-pop {
        font-size: 11px;
        color: #7aa2f7;
        margin-top: 4px;
    }

    .day-separator {
        background-color: #414868;
        min-width: 2px;
    }
    
    .daily-row {
        padding: 10px;
        background-color: #1f2a3f;
        border-radius: 10px;
        align-items: center;
    }
    
    .pill-hi, .pill-lo, .pill-pop {
        font-weight: bold;
        padding: 6px 10px;
        border-radius: 14px;
        font-size: 12px;
    }

    .pill-hi {
        background: linear-gradient(90deg, #3c445f, #2f3650);
        color: #f6d7a5;
    }

    .pill-lo {
        background: linear-gradient(90deg, #2b314a, #22283d);
        color: #a3c9ff;
    }

    .pill-pop {
        background: linear-gradient(90deg, #2d3a4f, #243245);
        color: #87c8ff;
    }

    .pill-pop-high {
        background: linear-gradient(90deg, #255066, #1e3c4f);
        color: #7ee0ff;
        box-shadow: 0 0 8px rgba(80, 200, 255, 0.4);
    }
    
    separator {
        background-color: #414868;
        margin-top: 10px;
        margin-bottom: 10px;
    }
"#;
