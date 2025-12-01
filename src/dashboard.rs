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
use crate::utils::{deg_to_dir, fmt_time, is_night, moon_phase_icon, pick_icon};
use crate::weather::{fetch_weather_for_loc, resolve_location, ApiResponse, Location, WeatherDesc};

// Constants
const MAX_UVI: f64 = 11.0;
const SPINNER_SIZE: i32 = 64;
const SECONDS_PER_HOUR: f64 = 3600.0;
const SECONDS_PER_DAY: i64 = 86_400;

// Humidity thresholds
const HUMIDITY_MUGGY: u8 = 70;
const HUMIDITY_COMFORTABLE: u8 = 40;

// UV Index thresholds
const UVI_VERY_HIGH: f64 = 8.0;
const UVI_HIGH: f64 = 6.0;
const UVI_MODERATE: f64 = 3.0;

/// Spawns an async task to fetch weather data and update the UI
fn spawn_weather_fetch(
    api_key: Rc<String>,
    location: Rc<Location>,
    units: Units,
    scroll_weak: glib::WeakRef<ScrolledWindow>,
    config: Rc<Config>,
) {
    let api_key = api_key.to_string();
    let location = (*location).clone();
    let config_clone = (*config).clone(); // Config needs to be cloneable

    glib::spawn_future_local(async move {
        // Offload network request to Tokio thread pool to avoid blocking GTK loop
        let api_key_for_fetch = api_key.clone();
        let location_for_fetch = location.clone();

        let fetch_result = tokio::spawn(async move {
            fetch_weather_for_loc(&api_key_for_fetch, &location_for_fetch, units).await
        })
        .await;

        match fetch_result {
            Ok(Ok(new_data)) => {
                if let Some(scroll) = scroll_weak.upgrade() {
                    let config_rc = Rc::new(config_clone);
                    refresh_content(&scroll, Some(&new_data), &location, units, &config_rc);
                }
            }
            Ok(Err(e)) => {
                if let Some(scroll) = scroll_weak.upgrade() {
                    show_error_ui(&scroll, &e.to_string());
                }
            }
            Err(e) => {
                // Join error (Tokio panic)
                if let Some(scroll) = scroll_weak.upgrade() {
                    show_error_ui(&scroll, &format!("Internal thread error: {}", e));
                }
            }
        }
    });
}

/// Handles location change by resolving new location and fetching weather
fn handle_location_change(api_key: Rc<String>, scroll_weak: glib::WeakRef<ScrolledWindow>) {
    let api_key_str = api_key.to_string();

    glib::spawn_future_local(async move {
        let new_config = load_config();
        let config_clone_for_resolve = new_config.clone();
        let config_clone_for_fetch = new_config.clone();
        let api_key_for_resolve = api_key_str.clone();
        let api_key_for_fetch = api_key_str.clone();

        // Offload location resolution
        let resolve_result = tokio::spawn(async move {
            resolve_location(&api_key_for_resolve, None, &config_clone_for_resolve).await
        })
        .await;

        match resolve_result {
            Ok(Ok(Some(new_location))) => {
                // Show loading state immediately
                if let Some(scroll) = scroll_weak.upgrade() {
                    refresh_content(
                        &scroll,
                        None,
                        &new_location,
                        new_config.units,
                        &Rc::new(new_config.clone()),
                    );
                }

                let location_clone = new_location.clone();

                // Offload weather fetch
                let fetch_result = tokio::spawn(async move {
                    fetch_weather_for_loc(
                        &api_key_for_fetch,
                        &location_clone,
                        config_clone_for_fetch.units,
                    )
                    .await
                })
                .await;

                match fetch_result {
                    Ok(Ok(new_data)) => {
                        if let Some(scroll) = scroll_weak.upgrade() {
                            refresh_content(
                                &scroll,
                                Some(&new_data),
                                &new_location,
                                new_config.units,
                                &Rc::new(new_config),
                            );
                        }
                    }
                    Ok(Err(e)) => {
                        if let Some(scroll) = scroll_weak.upgrade() {
                            show_error_ui(&scroll, &e.to_string());
                        }
                    }
                    Err(e) => {
                        if let Some(scroll) = scroll_weak.upgrade() {
                            show_error_ui(&scroll, &format!("Thread error: {}", e));
                        }
                    }
                }
            }
            Ok(Ok(None)) => {
                if let Some(scroll) = scroll_weak.upgrade() {
                    show_error_ui(&scroll, "No location found in configuration.");
                }
            }
            Ok(Err(e)) => {
                if let Some(scroll) = scroll_weak.upgrade() {
                    show_error_ui(&scroll, &format!("Location error: {}", e));
                }
            }
            Err(e) => {
                if let Some(scroll) = scroll_weak.upgrade() {
                    show_error_ui(&scroll, &format!("Thread error: {}", e));
                }
            }
        }
    });
}

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
    let dashboard_config = DashboardConfigResolved::from_config(&cfg.dashboard);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RedWeather Dashboard")
        .default_width(dashboard_config.window_width)
        .default_height(dashboard_config.window_height)
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

    // Apply a default size class once to avoid expensive thrashing on resize
    window.add_css_class("size-normal");

    let main_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    window.set_child(Some(&main_scroll));

    // Initial Draw

    refresh_content(&main_scroll, data.as_ref().as_ref(), &loc, units, &cfg);

    // Spawn background refresh
    spawn_weather_fetch(
        key.clone(),
        loc.clone(),
        units,
        main_scroll.downgrade(),
        cfg.clone(),
    );

    // Settings Button Logic
    let window_weak = window.downgrade();
    let api_key_for_dialog = key.clone();
    let scroll_weak_for_dialog = main_scroll.downgrade();

    settings_btn.connect_clicked(move |_| {
        if let Some(win) = window_weak.upgrade() {
            let api_key = api_key_for_dialog.clone();
            let config = load_config();
            let scroll_weak = scroll_weak_for_dialog.clone();
            let api_key_for_closure = api_key.clone();

            show_location_dialog(&win, &api_key, &config, move || {
                handle_location_change(api_key_for_closure.clone(), scroll_weak.clone());
            });
        }
    });

    window.show();
}

/// Builds the header section with date
fn build_header_section(_loc: &Location, data: &ApiResponse) -> GtkBox {
    let header_box = GtkBox::new(Orientation::Vertical, 4);
    header_box.set_halign(gtk::Align::Center);
    header_box.add_css_class("hero-header");

    let date_str = fmt_time(data.current.dt, data.timezone_offset, "%A, %B %d %Y");
    let date_label = Label::new(Some(&date_str));
    date_label.add_css_class("date-subtitle");

    header_box.append(&date_label);

    header_box
}

/// Builds the current weather section with temperature and icon
fn build_current_weather_section(data: &ApiResponse, units: Units) -> GtkBox {
    let current_box = GtkBox::new(Orientation::Vertical, 6);
    current_box.set_halign(gtk::Align::Center);
    current_box.add_css_class("hero-block");

    let current_desc = data.current.weather.get(0).cloned().unwrap_or(WeatherDesc {
        main: Some("Clear".into()),
        description: Some("Clear".into()),
    });
    let (sr, ss) = sun_window_for(data.current.dt, data)
        .or_else(|| data.current.sunrise.zip(data.current.sunset))
        .unwrap_or((0, 0));
    let is_night_now = is_night(data.current.dt, Some(sr), Some(ss));
    let moon_icon = Some(moon_phase_icon(data.current.dt, data.timezone_offset));
    let icon = pick_icon(&current_desc, is_night_now, moon_icon);

    let icon_label = Label::new(Some(&icon));
    icon_label.add_css_class("hero-icon");

    let (temp_unit, _speed_unit) = match units {
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

    current_box.append(&icon_label);
    current_box.append(&temp_label);
    current_box.append(&desc_label);
    current_box.append(&feels_label);

    current_box
}

/// Builds the gauges section with humidity, UV, wind, and daylight gauges
fn build_gauges_section(data: &ApiResponse, units: Units) -> GtkBox {
    let section_box = GtkBox::new(Orientation::Vertical, 10);

    let gauges_label = Label::new(Some("Live Gauges"));
    gauges_label.add_css_class("section-title");
    gauges_label.set_halign(gtk::Align::Start);
    section_box.append(&gauges_label);

    let (_, speed_unit) = match units {
        Units::Imperial => ("°F", "mph"),
        Units::Metric => ("°C", "m/s"),
    };

    let humidity = data.current.humidity.unwrap_or(0);
    let uvi = data.current.uvi.unwrap_or(0.0);
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_dir = deg_to_dir(data.current.wind_deg);

    let gauge_flow = FlowBox::new();
    gauge_flow.add_css_class("gauges-group");
    gauge_flow.set_selection_mode(gtk::SelectionMode::None);
    gauge_flow.set_max_children_per_line(4);
    gauge_flow.set_min_children_per_line(2);
    gauge_flow.set_row_spacing(12);
    gauge_flow.set_column_spacing(12);

    // Humidity gauge
    // Gradient: Cyan (#22d3ee) -> Blue (#3b82f6)
    let humidity_gauge = create_arc_gauge(
        (humidity as f64 / 100.0).clamp(0.0, 1.0),
        format!("{}%", humidity),
        (0.133, 0.827, 0.933), // Start: #22d3ee
        (0.231, 0.510, 0.965), // End: #3b82f6
    );
    let humidity_note = match humidity {
        h if h >= HUMIDITY_MUGGY => "Feels muggy",
        h if h >= HUMIDITY_COMFORTABLE => "Comfortable",
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

    // UV Index gauge
    // Gradient: Yellow (#facc15) -> Orange (#f97316)
    let uvi_gauge = create_arc_gauge(
        (uvi / MAX_UVI).clamp(0.0, 1.0),
        format!("{:.1}", uvi),
        (0.980, 0.800, 0.082), // Start: #facc15
        (0.976, 0.451, 0.086), // End: #f97316
    );
    let uv_note = match uvi {
        u if u >= UVI_VERY_HIGH => "Very high - protect skin",
        u if u >= UVI_HIGH => "High - limit midday sun",
        u if u >= UVI_MODERATE => "Moderate UV",
        _ => "Low UV risk",
    };
    gauge_flow.insert(
        &create_gauge_card("UV Index", uvi_gauge, uv_note, &format!("UV {:.1}", uvi)),
        -1,
    );

    // Wind gauge
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

    // Daylight gauge
    let sunrise = data.current.sunrise.unwrap_or(0);
    let sunset = data.current.sunset.unwrap_or(0);
    let daylight_caption = if sunrise > 0 && sunset > 0 && sunset > sunrise {
        let rise = fmt_time(sunrise, data.timezone_offset, "%I:%M %p");
        let set = fmt_time(sunset, data.timezone_offset, "%I:%M %p");
        format!("↑ {}  |  ↓ {}", rise, set)
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
        let hrs = ((sunrise - data.current.dt) as f64 / SECONDS_PER_HOUR).max(0.0);
        format!("{:.1}h to dawn", hrs)
    } else if data.current.dt > sunset {
        "Night".into()
    } else {
        let hrs = ((sunset - data.current.dt) as f64 / SECONDS_PER_HOUR).max(0.0);
        format!("{:.1}h", hrs)
    };
    // Daylight Gradient: Orange (#fbbf24) -> Yellow (#f59e0b)
    let daylight_gauge = create_arc_gauge(
        daylight_progress,
        daylight_text,
        (0.984, 0.749, 0.141), // Start: #fbbf24
        (0.961, 0.620, 0.043), // End: #f59e0b
    );
    gauge_flow.insert(
        &create_gauge_card(
            "Daylight",
            daylight_gauge,
            &daylight_caption,
            "Sun progress",
        ),
        -1,
    );

    section_box.append(&gauge_flow);
    section_box
}

/// Builds the hourly forecast section
fn build_hourly_forecast_section(
    data: &ApiResponse,
    dashboard_config: &DashboardConfigResolved,
) -> GtkBox {
    let section_box = GtkBox::new(Orientation::Vertical, 10);

    let hourly_label = Label::new(Some("Hourly Forecast"));
    hourly_label.add_css_class("section-title");
    hourly_label.set_halign(gtk::Align::Start);
    section_box.append(&hourly_label);

    let hourly_scroll = ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        // .min_content_height(200) // Removed fixed min height
        .build();
    enable_drag_scroll(&hourly_scroll);

    if dashboard_config.show_hourly_graph {
        // Graph View
        let graph = create_hourly_graph(
            &data.hourly,
            dashboard_config.forecast_hours,
            data.timezone_offset as i32,
        );
        graph.add_css_class("hourly-graph-canvas");
        let frame = GtkBox::new(Orientation::Vertical, 0);
        frame.add_css_class("panel-card");
        frame.append(&graph);
        hourly_scroll.set_child(Some(&frame));
    } else {
        // Card List View
        let hourly_box = GtkBox::new(Orientation::Horizontal, 15);
        hourly_box.set_margin_bottom(10);

        let mut last_day: Option<i64> = None;
        for h in data.hourly.iter().take(dashboard_config.forecast_hours) {
            let day_bucket = (h.dt + data.timezone_offset) / SECONDS_PER_DAY;
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

            let icon_str = h
                .weather
                .get(0)
                .map(|desc| {
                    let (sr, ss) = sun_window_for(h.dt, data)
                        .or_else(|| data.current.sunrise.zip(data.current.sunset))
                        .unwrap_or((0, 0));
                    let night = is_night(h.dt, Some(sr), Some(ss));
                    pick_icon(
                        desc,
                        night,
                        Some(moon_phase_icon(h.dt, data.timezone_offset)),
                    )
                })
                .unwrap_or_else(|| "❓".into());
            let icon_lbl = Label::new(Some(&icon_str));
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
        let frame = GtkBox::new(Orientation::Vertical, 0);
        frame.add_css_class("panel-card");
        frame.append(&hourly_box);
        hourly_scroll.set_child(Some(&frame));
    }

    section_box.append(&hourly_scroll);
    section_box
}

/// Builds the daily forecast section
fn build_daily_forecast_section(
    data: &ApiResponse,
    dashboard_config: &DashboardConfigResolved,
) -> GtkBox {
    let section_box = GtkBox::new(Orientation::Vertical, 10);

    let daily_label = Label::new(Some("Forecast"));
    daily_label.add_css_class("section-title");
    daily_label.set_halign(gtk::Align::Start);
    section_box.append(&daily_label);

    // Scrollable container for cards (horizontal drag/scroll like hourly)
    let daily_scroll = ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(180)
        .build();
    enable_drag_scroll(&daily_scroll);

    let daily_box = GtkBox::new(Orientation::Horizontal, 12);
    daily_box.set_margin_bottom(10);
    daily_box.set_margin_start(2);
    daily_box.set_margin_end(2);

    let forecast_days = dashboard_config.forecast_days.max(5).min(12);
    for (i, d) in data.daily.iter().take(forecast_days).enumerate() {
        let day_str = fmt_time(d.dt, data.timezone_offset, "%a").to_uppercase();
        let mid_dt = d.dt + 43_200; // midday heuristic
        let (sr, ss) = d.sunrise.zip(d.sunset).unwrap_or((0, 0));
        let night = is_night(mid_dt, Some(sr), Some(ss));
        let icon_str = d
            .weather
            .get(0)
            .map(|desc| {
                pick_icon(
                    desc,
                    night,
                    Some(moon_phase_icon(mid_dt, data.timezone_offset)),
                )
            })
            .unwrap_or_else(|| "❓".into());
        let hi = d.temp.max.or(d.temp.day).unwrap_or(0.0).round();
        let lo = d.temp.min.unwrap_or(0.0).round();
        let pop = d.pop.unwrap_or(0.0);

        let card = create_tokyo_forecast_card(&day_str, &icon_str, hi, lo, pop, i);
        daily_box.append(&card);
    }

    daily_scroll.set_child(Some(&daily_box));
    section_box.append(&daily_scroll);
    section_box
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

    let dashboard_config = DashboardConfigResolved::from_config(&cfg.dashboard);

    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(6);
    vbox.set_margin_bottom(6);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_hexpand(true);
    vbox.set_halign(gtk::Align::Fill);
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
            spinner.set_size_request(SPINNER_SIZE, SPINNER_SIZE);

            let label = Label::new(Some(&format!("Loading weather for {}...", loc.label)));
            label.add_css_class("date-subtitle");

            vbox.append(&spinner);
            vbox.append(&label);
            scroll.set_child(Some(&vbox));
            return;
        }
    };

    // Build and append all sections
    vbox.append(&build_header_section(loc, data));
    vbox.append(&build_current_weather_section(data, units));

    vbox.append(&create_section_divider());
    vbox.append(&build_gauges_section(data, units));

    vbox.append(&create_section_divider());
    vbox.append(&build_hourly_forecast_section(data, &dashboard_config));

    // Forecast stays scrollable horizontally; omit extra divider to save vertical space
    vbox.append(&build_daily_forecast_section(data, &dashboard_config));

    scroll.set_child(Some(&vbox));
}

fn sun_window_for(dt: i64, data: &ApiResponse) -> Option<(i64, i64)> {
    let bucket = (dt + data.timezone_offset) / SECONDS_PER_DAY;
    for d in &data.daily {
        let day_bucket = (d.dt + data.timezone_offset) / SECONDS_PER_DAY;
        if day_bucket == bucket {
            if let (Some(sr), Some(ss)) = (d.sunrise, d.sunset) {
                return Some((sr, ss));
            }
        }
    }
    data.current.sunrise.zip(data.current.sunset)
}

fn create_gauge_card(title: &str, gauge: DrawingArea, caption: &str, detail: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.add_css_class("gauge-item");
    card.set_vexpand(true);
    card.set_valign(gtk::Align::Fill);

    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("gauge-title");
    title_lbl.set_halign(gtk::Align::Start);

    let gauge_wrapper = GtkBox::new(Orientation::Vertical, 0);
    gauge_wrapper.set_halign(gtk::Align::Center);
    gauge_wrapper.set_valign(gtk::Align::Fill);
    gauge_wrapper.set_vexpand(true);
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

fn create_section_divider() -> Separator {
    let sep = Separator::new(Orientation::Horizontal);
    sep.add_css_class("section-divider");
    sep
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

fn create_tokyo_forecast_card(
    day_str: &str,
    icon: &str,
    hi: f64,
    lo: f64,
    pop: f64,
    index: usize,
) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("tokyo-card");
    // Apply varying neon bottom style based on index
    let neon_class = format!("tokyo-card-neon-{}", index % 6);
    card.add_css_class(&neon_class);

    // Top: Day Label
    let day_lbl = Label::new(Some(day_str));
    day_lbl.add_css_class("tokyo-day");
    day_lbl.set_halign(gtk::Align::Start);
    card.append(&day_lbl);

    // Spacer to push center content
    let top_spacer = GtkBox::new(Orientation::Vertical, 0);
    top_spacer.set_vexpand(true);
    card.append(&top_spacer);

    // Center: Icon + Temps
    let center_box = GtkBox::new(Orientation::Vertical, 4); // slight spacing between icon and temp
    center_box.set_halign(gtk::Align::Center);
    center_box.set_valign(gtk::Align::Center);

    let icon_lbl = Label::new(Some(icon));
    icon_lbl.add_css_class("tokyo-icon");
    center_box.append(&icon_lbl);

    // Temperatures with Pango markup
    // High: Golden orange (#fbbf24), larger/bolder
    // Low: Cool blue (#93c5fd), smaller
    // Slash: thin/light (#93c5fd)
    let hi_color = "#fbbf24";
    let lo_color = "#93c5fd";

    // We rely on relative sizes (x-large, medium) which scale with the widget's font size (set by CSS on window)
    let markup = format!(
        "<span foreground='{}' weight='bold' size='x-large'>{:.0}°</span><span foreground='{}' weight='light'> / </span><span foreground='{}' size='medium'>{:.0}°</span>",
        hi_color, hi, lo_color, lo_color, lo
    );
    let temp_lbl = Label::new(None);
    temp_lbl.set_markup(&markup);

    center_box.append(&temp_lbl);

    card.append(&center_box);

    // Spacer
    let bottom_spacer = GtkBox::new(Orientation::Vertical, 0);
    bottom_spacer.set_vexpand(true);
    card.append(&bottom_spacer);

    // Bottom: Precipitation
    let pop_box = GtkBox::new(Orientation::Horizontal, 4);
    pop_box.add_css_class("tokyo-pop-box");
    pop_box.set_halign(gtk::Align::Start);

    // Only show if pop > 0
    if pop > 0.0 {
        // Use a simple teardrop char, colored via CSS class
        let drop_icon = Label::new(Some("💧"));
        drop_icon.add_css_class("tokyo-pop-icon");

        let pop_pct = (pop * 100.0).round();
        let pop_lbl = Label::new(Some(&format!("{:.0}%", pop_pct)));
        pop_lbl.add_css_class("tokyo-pop-text");

        pop_box.append(&drop_icon);
        pop_box.append(&pop_lbl);
    } else {
        // Empty placeholder to maintain spacing/alignment if needed
        let spacer = Label::new(Some(" "));
        pop_box.append(&spacer);
    }

    card.append(&pop_box);

    card
}

const STYLE_CSS: &str = r#"
    window {
        background: linear-gradient(180deg, #0b0f1f 0%, #101831 45%, #0c132a 100%);
        color: #d9e1ff;
        font-family: "Cantarell", "Noto Sans", Sans;
        font-size: 1rem;
    }
    
    /* Responsive Scaling Buckets */
    window.size-compact { font-size: 0.75rem; }
    window.size-normal { font-size: 1rem; }
    window.size-large { font-size: 1.25rem; }
    window.size-huge { font-size: 2rem; }

    /* Height adjustments */
    window.height-short .main-container { padding-top: 0; padding-bottom: 0; }
    window.height-short .hero-block { padding-bottom: 0.25rem; }
    window.height-short .section-divider { margin-top: 0.4rem; margin-bottom: 0.4rem; }
    window.height-short .gauge-item { min-height: 6.25rem; padding: 0.55rem 0.55rem 0.7rem; }
    window.height-short .gauge-canvas { min-height: 4.75rem; min-width: 4.75rem; }
    window.height-short .hero-block { padding: 0.35rem 0.5rem 0.75rem; }
    window.height-short .hourly-graph-canvas { min-height: 7rem; }
    window.height-short .tokyo-card { min-height: 7.5rem; }
    
    headerbar {
        background-color: #0a0d18;
        color: #d9e1ff;
        border-bottom: 0.0625rem solid #151c2f;
    }

    .main-container { padding-bottom: 0.5rem; }

    .hero-header { padding: 0.25rem 0; }
    
    .date-subtitle {
        font-size: 0.9rem;
        font-weight: 600;
        color: #a6afd4;
        letter-spacing: 0.031rem;
    }
    
    .hero-block {
        padding: 0.25rem 0.55rem 0.5rem;
        border-radius: 0.92rem;
        background: rgba(255, 255, 255, 0.015);
    }

    .hero-icon { font-size: 3.4rem; color: #e6edff; }
    
    .hero-temp {
        font-size: 3.2rem;
        font-weight: 780;
        color: #f5ad2e;
        letter-spacing: -0.016rem;
    }
    
    .hero-desc {
        font-size: 1.125rem;
        color: #e9ecf8;
        text-transform: capitalize;
        letter-spacing: 0.012rem;
    }

    .hero-feels { font-size: 0.9rem; color: #a0accf; }
    
    .section-title {
        font-size: 1.02rem;
        font-weight: 740;
        color: #d8cffc;
        letter-spacing: 0.025rem;
        margin-bottom: 0.2rem;
        border-bottom: 1px solid rgba(122, 162, 247, 0.28);
        padding-bottom: 0.12rem;
    }
    
    .gauge-card {
        background: linear-gradient(180deg, #10172a 0%, #0c1224 100%);
        padding: 0.65rem 0.7rem 0.8rem;
        border-radius: 0.9rem;
        border: 0.065rem solid rgba(255, 255, 255, 0.06);
        min-width: 5.4rem;
    }
    
    .detail-title { font-size: 0.75rem; color: #9ca3af; }

    .gauge-title {
        font-size: 0.8rem;
        font-weight: 750;
        color: #f1f4ff;
        letter-spacing: 0.02rem;
        margin-bottom: 0.35rem;
    }

    .gauge-caption {
        font-size: 0.78rem;
        color: #b8c5e6;
        margin-top: 0.35rem;
    }
    
    .gauge-canvas { 
        min-width: 5rem; 
        min-height: 5rem; 
        transition: min-width 0.2s ease-out, min-height 0.2s ease-out;
    }
    window.size-compact .gauge-canvas { min-width: 4.2rem; min-height: 4.2rem; }
    window.size-large .gauge-canvas { min-width: 6rem; min-height: 6rem; }
    window.size-huge .gauge-canvas { min-width: 6.8rem; min-height: 6.8rem; }
    
    .hourly-graph-canvas { 
        min-height: 8.4rem; 
        transition: min-height 0.2s ease-out;
    }
    window.size-compact .hourly-graph-canvas { min-height: 7.2rem; }
    window.height-short .hourly-graph-canvas { min-height: 5.5rem; }
    window.size-large .hourly-graph-canvas { min-height: 9.5rem; }
    
    .forecast-card {
        background: linear-gradient(180deg, #101528 0%, #0c1222 100%);
        padding: 0.625rem;
        border-radius: 0.75rem;
        min-width: 3.75rem;
        border: 0.0625rem solid rgba(255, 255, 255, 0.06);
    }
    
    .forecast-time { font-size: 0.75rem; color: #9ca3af; }
    
    .forecast-icon { font-size: 1.5rem; margin-top: 0.3125rem; margin-bottom: 0.3125rem; }
    
    .forecast-temp { font-weight: bold; }

    .forecast-pop { font-size: 0.6875rem; color: #38bdf8; margin-top: 0.25rem; }

    .day-separator { background-color: rgba(255, 255, 255, 0.08); min-width: 0.125rem; }
    
    .daily-card {
        padding: 0.65rem 0.85rem;
        background: linear-gradient(180deg, #0f1423 0%, #0c1120 100%);
        border-radius: 0.78rem;
        border: 0.0625rem solid rgba(255, 255, 255, 0.08);
        min-width: 6rem;
        /* max-width: 8rem; */
        box-shadow: 0 0.25rem 0.75rem rgba(0, 0, 0, 0.3);
    }

    .daily-card-day {
        font-size: 0.875rem;
        font-weight: 700;
        color: #e5e7eb;
        letter-spacing: 0.031rem;
        margin-bottom: 0.375rem;
    }

    .daily-card-icon { font-size: 3rem; margin: 0.5rem 0; }

    .daily-card-temps {
        font-size: 1.125rem;
        font-weight: 700;
        color: #e5e7eb;
        margin-top: 0.375rem;
        margin-bottom: 0.375rem;
    }

    .daily-card-pop {
        font-size: 0.8125rem;
        color: #38bdf8;
        font-weight: 700;
        margin-top: 0.375rem;
    }

    .daily-card-pop-high {
        font-size: 0.8125rem;
        color: #38bdf8;
        font-weight: 800;
        margin-top: 0.375rem;
    }
    
    .section-divider {
        background-color: rgba(255, 255, 255, 0.08);
        margin-top: 0.75rem;
        margin-bottom: 0.75rem;
    }

    scrolledwindow { background-color: transparent; }

    scrollbar { background-color: transparent; }

    scrollbar slider {
        min-width: 0.5rem;
        min-height: 0.5rem;
        border-radius: 0.25rem;
        background-color: #334155;
    }

    scrollbar slider:hover { background-color: #475569; }

    scrollbar slider:active { background-color: #64748b; }

    .panel-card {
        background: linear-gradient(180deg, rgba(22, 30, 50, 0.82) 0%, rgba(14, 19, 35, 0.94) 100%);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 0.95rem;
        padding: 0.55rem 0.55rem 0.75rem;
    }

    /* TOKYO NIGHT FORECAST CARD STYLES */
    .tokyo-card {
        background: linear-gradient(180deg, #0f172a 0%, #0b1225 55%, #0a0f21 100%);
        border-radius: 0.78rem;
        padding: 0.22rem 0.18rem 0.3rem;
        min-width: 6rem;  
        min-height: 8rem; 
        margin-bottom: 0.25rem;
        
        border: 0.07rem solid rgba(255,255,255,0.07);
        
        transition: min-height 0.2s ease-out;
    }

    /* Neon Gradient Borders (Purple/Blue) */
    .tokyo-card-neon-0 { border-bottom: 0.18rem solid #8b5cf6; box-shadow: 0 0 0.35rem rgba(139, 92, 246, 0.35); }
    .tokyo-card-neon-1 { border-bottom: 0.18rem solid #6366f1; box-shadow: 0 0 0.35rem rgba(99, 102, 241, 0.35); }
    .tokyo-card-neon-2 { border-bottom: 0.18rem solid #3b82f6; box-shadow: 0 0 0.35rem rgba(59, 130, 246, 0.35); }
    .tokyo-card-neon-3 { border-bottom: 0.18rem solid #22d3ee; box-shadow: 0 0 0.35rem rgba(34, 211, 238, 0.35); }
    .tokyo-card-neon-4 { border-bottom: 0.18rem solid #0ea5e9; box-shadow: 0 0 0.35rem rgba(14, 165, 233, 0.35); }
    .tokyo-card-neon-5 { border-bottom: 0.18rem solid #8b5cf6; box-shadow: 0 0 0.35rem rgba(139, 92, 246, 0.35); }

    .tokyo-day {
        font-family: "Cantarell", "Noto Sans", Sans;
        font-weight: 700;
        font-size: 0.8125rem;
        color: #d2dcff;
        letter-spacing: 0.018rem;
        margin-top: 0.625rem;
        margin-left: 0.625rem;
    }

    .tokyo-icon { font-size: 2.4rem; margin-bottom: 0.35rem; }

    .tokyo-pop-box { margin-bottom: 0.625rem; margin-left: 0.625rem; }

    .tokyo-pop-icon {
        font-size: 0.75rem;
        color: #38bdf8;
        margin-right: 0.25rem;
    }

    .tokyo-pop-text {
        font-size: 0.75rem;
        color: #38bdf8;
        font-weight: 700;
    }

    /* Utility: Note text size */
    .note { font-size: 0.8rem; }
"#;
