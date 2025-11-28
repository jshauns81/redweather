//! GTK4 Dashboard for displaying rich weather information
//!
//! This module implements the graphical dashboard view of the application.

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Label, Orientation,
    ScrolledWindow, Separator, Spinner,
};
use std::rc::Rc;

use crate::config::{Config, DashboardConfigResolved, Units, load_config};
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
        build_ui(app, data.clone(), loc.clone(), units, key.clone(), cfg.clone());
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
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RedWeather Dashboard")
        .default_width(500)
        .default_height(700)
        .build();

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
            Err(e) => eprintln!("Background fetch failed: {}", e),
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
                         match fetch_weather_for_loc(&k2, &new_loc, units).await {
                             Ok(new_data) => {
                                 if let Some(scroll) = s_weak2.upgrade() {
                                     refresh_content(&scroll, Some(&new_data), &new_loc, units, &new_cfg);
                                 }
                             }
                             Err(e) => eprintln!("Failed to refresh dashboard: {}", e),
                         }
                    }
                });
            });
        }
    });

    window.show();
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
    let (temp_unit, _, _) = match units {
        Units::Imperial => ("°F", "mph", "mi"),
        Units::Metric => ("°C", "m/s", "km"),
    };
    let temp_label = Label::new(Some(&format!("{:.0}{}", data.current.temp.round(), temp_unit)));
    temp_label.add_css_class("hero-temp");
    
    let desc_label = Label::new(Some(&current_desc.description.unwrap_or_default()));
    desc_label.add_css_class("hero-desc");
    
    temp_info_box.append(&temp_label);
    temp_info_box.append(&desc_label);
    
    current_box.append(&icon_label);
    current_box.append(&temp_info_box);
    vbox.append(&current_box);

    vbox.append(&Separator::new(Orientation::Horizontal));

    // --- Details Grid ---
    let details_box = GtkBox::new(Orientation::Vertical, 10);
    
    // Row 1
    let row1 = GtkBox::new(Orientation::Horizontal, 10);
    row1.set_homogeneous(true);
    
    let feels_like = data.current.feels_like.unwrap_or(data.current.temp).round();
    let humidity = data.current.humidity.unwrap_or(0);
    let uvi = data.current.uvi.unwrap_or(0.0);
    
    row1.append(&create_detail_card("Feels Like", &format!("{:.0}{}", feels_like, temp_unit)));
    row1.append(&create_detail_card("Humidity", &format!("{}%", humidity)));
    
    // Row 2
    let row2 = GtkBox::new(Orientation::Horizontal, 10);
    row2.set_homogeneous(true);
    
    let wind_speed = data.current.wind_speed.unwrap_or(0.0).round();
    let wind_dir = deg_to_dir(data.current.wind_deg);
    
    row2.append(&create_detail_card("UV Index", &format!("{:.1}", uvi)));
    row2.append(&create_detail_card("Wind", &format!("{:.0} {} {}", wind_speed, match units {
        Units::Imperial => "mph",
        Units::Metric => "m/s",
    }, wind_dir)));

    details_box.append(&row1);
    details_box.append(&row2);
    vbox.append(&details_box);

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
    
    if d_cfg.show_hourly_graph {
        // Graph View
        let graph = create_hourly_graph(&data.hourly, d_cfg.forecast_hours, data.timezone_offset);
        hourly_scroll.set_child(Some(&graph));
    } else {
        // Card List View
        let hourly_box = GtkBox::new(Orientation::Horizontal, 15);
        hourly_box.set_margin_bottom(10);
        
        for h in data.hourly.iter().take(d_cfg.forecast_hours) {
            let card = GtkBox::new(Orientation::Vertical, 5);
            card.add_css_class("forecast-card");
            
            let time_str = fmt_time(h.dt, data.timezone_offset, "%l %p");
            let time_lbl = Label::new(Some(&time_str));
            time_lbl.add_css_class("forecast-time");
            
            let icon_str = h.weather.get(0).map(pick_icon).unwrap_or("❓");
            let icon_lbl = Label::new(Some(icon_str));
            icon_lbl.add_css_class("forecast-icon");
            
            let temp_lbl = Label::new(Some(&format!("{:.0}°", h.temp.round())));
            temp_lbl.add_css_class("forecast-temp");
            
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
        let temp_lbl = Label::new(Some(&format!("{:.0}° / {:.0}°", hi, lo)));
        temp_lbl.add_css_class("daily-temp");
        
        row.append(&day_lbl);
        row.append(&icon_lbl);
        row.append(&temp_lbl);
        daily_box.append(&row);
    }
    vbox.append(&daily_box);

    scroll.set_child(Some(&vbox));
}

fn create_detail_card(title: &str, value: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 5);
    card.add_css_class("detail-card");
    
    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("detail-title");
    
    let val_lbl = Label::new(Some(value));
    val_lbl.add_css_class("detail-value");
    
    card.append(&title_lbl);
    card.append(&val_lbl);
    card
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
    
    .section-title {
        font-size: 18px;
        font-weight: bold;
        color: #bb9af7;
        margin-bottom: 5px;
    }
    
    .detail-card {
        background-color: #1f2335;
        padding: 10px;
        border-radius: 8px;
    }
    
    .detail-title {
        font-size: 12px;
        color: #565f89;
    }
    
    .detail-value {
        font-size: 16px;
        font-weight: bold;
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
    
    .daily-row {
        padding: 8px;
        background-color: #1f2335;
        border-radius: 8px;
    }
    
    .daily-temp {
        font-weight: bold;
        margin-left: 10px;
    }
    
    separator {
        background-color: #414868;
        margin-top: 10px;
        margin-bottom: 10px;
    }
"#;
