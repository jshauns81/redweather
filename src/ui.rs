use anyhow::Result;
use glib::clone;
use gtk::prelude::*;
use gtk::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, Entry, Label, ListBox,
    ListBoxRow, Orientation, ScrolledWindow, Separator, SpinButton, Switch, Window,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use crate::config::{
    save_config, save_location_preset, update_active_preset, Config, DashboardConfig,
};
use crate::weather::{geocode_direct, geocode_zip, Location};

/// Reloads Waybar by sending SIGUSR2 signal
fn reload_waybar() {
    let _ = Command::new("pkill").arg("-SIGUSR2").arg("waybar").status();
}

const DIALOG_CSS: &str = r#"
    window {
        background-color: #24283b;
        color: #c0caf5;
    }
    .dialog-title {
        font-size: 18px;
        font-weight: bold;
        color: #7aa2f7;
        margin-bottom: 10px;
    }
    .result-box {
        background-color: #1f2335;
        border-radius: 8px;
        padding: 10px;
        margin-top: 10px;
        margin-bottom: 10px;
    }
    .result-label {
        font-weight: bold;
        color: #9aa5ce;
    }
    button.suggested-action {
        background-color: #7aa2f7;
        color: #1d202f;
        font-weight: bold;
    }
    .settings-section-title {
        font-weight: bold;
        color: #bb9af7;
        margin-top: 15px;
        margin-bottom: 5px;
    }
"#;

/// Shows a modal dialog for location configuration
pub fn show_location_dialog<W, F>(parent: &W, key: &str, cfg: &Config, on_update: F)
where
    W: IsA<Window>,
    F: Fn() + 'static,
{
    let dialog = Window::builder()
        .transient_for(parent)
        .title("Settings")
        .default_width(400)
        .default_height(650) // Increased height
        .resizable(false)
        .build();

    if let Some(app) = parent.application() {
        dialog.set_application(Some(&app));
    }

    // Apply CSS
    let provider = gtk::CssProvider::new();
    provider.load_from_data(DIALOG_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let main_vbox = GtkBox::new(Orientation::Vertical, 0);
    main_vbox.set_margin_top(20);
    main_vbox.set_margin_bottom(20);
    main_vbox.set_margin_start(20);
    main_vbox.set_margin_end(20);

    // Title
    let title = Label::new(Some("Settings"));
    title.add_css_class("dialog-title");
    main_vbox.append(&title);

    // Config clone for mutable access
    let current_cfg = Rc::new(RefCell::new(cfg.clone()));
    let on_update_rc = Rc::new(on_update); // Create Rc once

    // --- Dashboard Options Section ---
    main_vbox.append(&Separator::new(Orientation::Horizontal));
    let dash_opts_title = Label::new(Some("Dashboard Display Options"));
    dash_opts_title.add_css_class("settings-section-title");
    dash_opts_title.set_halign(gtk::Align::Start);
    main_vbox.append(&dash_opts_title);

    let initial_dash_cfg = current_cfg.borrow().dashboard.clone().unwrap_or_default();

    // Units
    let units_row = GtkBox::new(Orientation::Horizontal, 10);
    let units_label = Label::new(Some("Units:"));
    units_label.set_halign(gtk::Align::Start);
    let units_combo = ComboBoxText::new();
    units_combo.append(Some("imperial"), "Imperial (°F, mph)");
    units_combo.append(Some("metric"), "Metric (°C, m/s)");
    
    let current_unit_id = match cfg.units {
        crate::config::Units::Imperial => "imperial",
        crate::config::Units::Metric => "metric",
    };
    units_combo.set_active_id(Some(current_unit_id));
    
    units_row.append(&units_label);
    units_row.append(&units_combo);
    main_vbox.append(&units_row);

    let cfg_clone_u = current_cfg.clone();
    let on_update_clone_u = on_update_rc.clone();
    units_combo.connect_changed(move |combo| {
        if let Some(id) = combo.active_id() {
            let mut cfg_borrow = cfg_clone_u.borrow_mut();
            cfg_borrow.units = match id.as_str() {
                "metric" => crate::config::Units::Metric,
                _ => crate::config::Units::Imperial,
            };
            if let Err(e) = save_config(&cfg_borrow) {
                eprintln!("Failed to save config: {}", e);
            }
            on_update_clone_u();
        }
    });

    // Show Hourly Graph
    let graph_row = GtkBox::new(Orientation::Horizontal, 10);
    let graph_label = Label::new(Some("Show Hourly Graph:"));
    graph_label.set_halign(gtk::Align::Start);
    let graph_switch = Switch::new();
    graph_switch.set_active(initial_dash_cfg.show_hourly_graph);
    graph_row.append(&graph_label);
    graph_row.append(&graph_switch);
    main_vbox.append(&graph_row);

    let cfg_clone_g = current_cfg.clone();
    let on_update_clone_g = on_update_rc.clone();
    graph_switch.connect_active_notify(move |sw| {
        let mut cfg_borrow = cfg_clone_g.borrow_mut();
        let dash_cfg = cfg_borrow
            .dashboard
            .get_or_insert_with(DashboardConfig::default);
        dash_cfg.show_hourly_graph = sw.is_active();
        if let Err(e) = save_config(&cfg_borrow) {
            eprintln!("Failed to save config: {}", e);
        }
        on_update_clone_g();
    });

    // Forecast Hours
    let hours_row = GtkBox::new(Orientation::Horizontal, 10);
    let hours_label = Label::new(Some("Forecast Hours:"));
    hours_label.set_halign(gtk::Align::Start);
    let hours_adj = Adjustment::new(
        initial_dash_cfg.forecast_hours as f64,
        12.0,
        48.0,
        12.0,
        0.0,
        0.0,
    );
    let hours_spin = SpinButton::new(Some(&hours_adj), 1.0, 0);
    hours_row.append(&hours_label);
    hours_row.append(&hours_spin);
    main_vbox.append(&hours_row);

    let cfg_clone_h = current_cfg.clone();
    let on_update_clone_h = on_update_rc.clone();
    hours_spin.connect_value_changed(move |sb| {
        let mut cfg_borrow = cfg_clone_h.borrow_mut();
        let dash_cfg = cfg_borrow
            .dashboard
            .get_or_insert_with(DashboardConfig::default);
        dash_cfg.forecast_hours = sb.value() as usize;
        if let Err(e) = save_config(&cfg_borrow) {
            eprintln!("Failed to save config: {}", e);
        }
        on_update_clone_h();
    });

    // Forecast Days
    let days_row = GtkBox::new(Orientation::Horizontal, 10);
    let days_label = Label::new(Some("Forecast Days:"));
    days_label.set_halign(gtk::Align::Start);
    let days_adj = Adjustment::new(
        initial_dash_cfg.forecast_days as f64,
        3.0,
        10.0,
        1.0,
        0.0,
        0.0,
    );
    let days_spin = SpinButton::new(Some(&days_adj), 1.0, 0);
    days_row.append(&days_label);
    days_row.append(&days_spin);
    main_vbox.append(&days_row);

    let cfg_clone_d = current_cfg.clone();
    let on_update_clone_d = on_update_rc.clone();
    days_spin.connect_value_changed(move |sb| {
        let mut cfg_borrow = cfg_clone_d.borrow_mut();
        let dash_cfg = cfg_borrow
            .dashboard
            .get_or_insert_with(DashboardConfig::default);
        dash_cfg.forecast_days = sb.value() as usize;
        if let Err(e) = save_config(&cfg_borrow) {
            eprintln!("Failed to save config: {}", e);
        }
        on_update_clone_d();
    });

    main_vbox.append(&Separator::new(Orientation::Horizontal));

    // --- Location Settings Section ---
    let loc_opts_title = Label::new(Some("Location Settings"));
    loc_opts_title.add_css_class("settings-section-title");
    loc_opts_title.set_halign(gtk::Align::Start);
    main_vbox.append(&loc_opts_title);

    // Saved Locations Section (Quick Switch) - inline list to avoid popover hitbox issues
    let saved_box = GtkBox::new(Orientation::Vertical, 6);
    saved_box.set_margin_bottom(15); // Add some space below
    let saved_label = Label::new(Some("Quick Switch:"));
    saved_label.set_halign(gtk::Align::Start);

    let mut preset_ids: Vec<String> = Vec::new();
    let preset_list = ListBox::new();
    preset_list.set_selection_mode(gtk::SelectionMode::Single);
    preset_list.set_activate_on_single_click(true);
    preset_list.set_vexpand(true);

    if let Some(presets) = &cfg.location_presets {
        for preset in presets {
            let row = ListBoxRow::new();
            let row_box = GtkBox::new(Orientation::Horizontal, 6);
            let lbl = Label::new(Some(&preset.label));
            lbl.set_halign(gtk::Align::Start);
            lbl.set_hexpand(true);
            row_box.append(&lbl);
            row.set_child(Some(&row_box));
            preset_list.append(&row);

            preset_ids.push(preset.name.clone());

            if cfg
                .active_preset
                .as_deref()
                .map(|active| active == preset.name)
                .unwrap_or(false)
            {
                preset_list.select_row(Some(&row));
            }
        }
    }

    let has_presets = !preset_ids.is_empty();
    saved_box.set_visible(has_presets);

    let quick_switch_rows = 6;
    let approx_row_height = 28; // GTK default row height ~24-30px; keep 6 visible before scroll
    let preset_scroll = ScrolledWindow::builder()
        .min_content_width(220)
        .max_content_height(quick_switch_rows * approx_row_height)
        .child(&preset_list)
        .build();
    preset_scroll.set_overlay_scrolling(false);

    saved_box.append(&saved_label);
    saved_box.append(&preset_scroll);
    main_vbox.append(&saved_box);

    // Search Section
    let search_row = GtkBox::new(Orientation::Horizontal, 10);
    let search_entry = Entry::builder()
        .placeholder_text("Search City or ZIP...")
        .hexpand(true)
        .build();
    let search_btn = Button::with_label("Search");

    search_row.append(&search_entry);
    search_row.append(&search_btn);
    main_vbox.append(&search_row);

    // Result Section (Dynamic)
    let result_box = GtkBox::new(Orientation::Vertical, 5);
    result_box.add_css_class("result-box");
    result_box.set_visible(false);

    let result_label = Label::new(None);
    result_label.add_css_class("result-label");
    result_label.set_wrap(true);
    result_box.append(&result_label);

    let use_btn = Button::with_label("Use This Location");
    use_btn.add_css_class("suggested-action");
    result_box.append(&use_btn);

    main_vbox.append(&result_box);

    // Spacer
    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    main_vbox.append(&spacer);

    // Footer
    let cancel_btn = Button::with_label("Done");
    cancel_btn.set_halign(gtk::Align::End);
    main_vbox.append(&cancel_btn);

    dialog.set_child(Some(&main_vbox));

    // State
    let current_loc: Rc<RefCell<Option<Location>>> = Rc::new(RefCell::new(None));
    let is_preset_selection: Rc<RefCell<bool>> = Rc::new(RefCell::new(false)); // Track if current selection is from preset

    // Search Logic
    let key_str = key.to_string();
    let s_entry = search_entry.clone();
    let r_box = result_box.clone();
    let r_label = result_label.clone();
    let cur_loc = current_loc.clone();
    let is_preset = is_preset_selection.clone();

    let perform_search = move || {
        let query = s_entry.text().to_string();
        if query.trim().is_empty() {
            return;
        }

        r_label.set_text("Searching...");
        r_box.set_visible(true);

        let k = key_str.clone();
        let c_loc = cur_loc.clone();
        let r_lbl = r_label.clone();
        let is_p = is_preset.clone();

        glib::spawn_future_local(async move {
            let res = if let Some(loc) = geocode_zip(&k, &query).await {
                Some(loc)
            } else {
                geocode_direct(&k, &query).await
            };

            match res {
                Some(loc) => {
                    r_lbl.set_text(&format!("📍 {}", loc.label));
                    *c_loc.borrow_mut() = Some(loc);
                    *is_p.borrow_mut() = false;
                }
                None => {
                    r_lbl.set_text("❌ No location found.");
                    *c_loc.borrow_mut() = None;
                }
            }
        });
    };

    search_btn.connect_clicked(clone!(@strong perform_search => move |_| perform_search()));
    search_entry.connect_activate(clone!(@strong perform_search => move |_| perform_search()));

    // Preset Logic - IMMEDIATE ACTION
    let on_update_rc_preset = on_update_rc.clone();
    let dlg_preset = dialog.clone();
    let preset_ids = Rc::new(preset_ids);
    preset_list.connect_row_activated(move |_list, row| {
        let idx = row.index();
        if idx < 0 {
            return;
        }
        if let Some(preset_id) = preset_ids.get(idx as usize) {
            if let Err(e) = update_active_preset(preset_id.as_str()) {
                eprintln!("Failed to activate preset: {}", e);
                // Optionally show error to user in dialog
            } else {
                reload_waybar();
                on_update_rc_preset();
                dlg_preset.close();
            }
        }
    });

    // Use Button Logic (for search results)
    let on_update_rc_use = on_update_rc.clone();
    let dlg_use = dialog.clone();
    let cur_loc_use = current_loc.clone();

    use_btn.connect_clicked(move |_| {
        if let Some(loc) = cur_loc_use.borrow().as_ref() {
            // Generate a safe name from label (e.g. "Wichita, US" -> "Wichita")
            let name = loc
                .label
                .split(',')
                .next()
                .unwrap_or("Custom")
                .trim()
                .to_string();

            if let Err(e) = save_location_preset(&name, loc.lat, loc.lon, &loc.label) {
                eprintln!("Failed to save preset: {}", e);
            } else {
                reload_waybar();
                on_update_rc_use();
                dlg_use.close();
            }
        }
    });

    // Cancel button handler
    let dlg_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dlg_cancel.close();
    });

    dialog.show();
}

/// Runs the GTK prompt application (legacy wrapper)
pub fn run_prompt(key: &str, cfg: &Config) -> Result<()> {
    let key = key.to_string();
    let cfg_clone = cfg.clone();
    let app = Application::builder()
        .application_id("com.shaun.redweather.prompt")
        .build();

    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Set Weather Location")
            .default_width(360)
            .default_height(180)
            .build();

        let label = Label::new(Some("Opening configuration dialog..."));
        window.set_child(Some(&label));
        window.show();

        let win_ref = window.clone();
        show_location_dialog(&window, &key, &cfg_clone, move || {
            win_ref.close();
        });
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}
