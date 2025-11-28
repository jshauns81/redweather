//! GTK4 user interface for location configuration
//!
//! This module provides a GTK4 window for setting and testing location lookups.

use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Button, Entry, Label, Orientation};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use crate::weather::{geocode_direct, geocode_zip, save_home_location, Location};

/// Reloads Waybar by sending SIGUSR2 signal
fn reload_waybar() {
    let _ = Command::new("pkill").arg("-SIGUSR2").arg("waybar").status();
}

/// Helper function to handle geocoding checks in the prompt UI
fn handle_geocode_check(
    key: String,
    query: String,
    status_label: Label,
    result_label: Label,
    save_button: Button,
    current_location: Rc<RefCell<Option<Location>>>,
) {
    result_label.set_text("");
    save_button.set_sensitive(false);

    if query.is_empty() {
        status_label.set_text("Enter a ZIP or city");
        return;
    }

    // Spawn async task for geocoding
    glib::spawn_future_local(async move {
        let info = if let Some(loc) = geocode_zip(&key, &query).await {
            Some(loc)
        } else {
            geocode_direct(&key, &query).await
        };

        match info {
            Some(loc) => {
                result_label.set_text(&format!("→ {}", loc.label));
                status_label.set_text("OK");
                *current_location.borrow_mut() = Some(loc);
                save_button.set_sensitive(true);
            }
            None => {
                status_label.set_text("No result");
            }
        }
    });
}

/// Runs the GTK prompt window for location configuration
pub fn run_prompt(key: &str) -> Result<()> {
    let key = key.to_string();
    let app = Application::builder()
        .application_id("com.shaun.redweather.prompt")
        .build();

    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Set Weather Location")
            .default_width(360)
            .default_height(180)
            .resizable(false)
            .build();

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(10);
        vbox.set_margin_bottom(10);
        vbox.set_margin_start(10);
        vbox.set_margin_end(10);

        let row = GtkBox::new(Orientation::Horizontal, 6);
        let label = Label::new(Some("ZIP or city,country:"));
        let entry = Entry::new();
        entry.set_hexpand(true);
        row.append(&label);
        row.append(&entry);

        let status = Label::new(Some("Enter location and press Check"));
        status.set_halign(gtk::Align::Start);
        let result = Label::new(None);
        result.set_halign(gtk::Align::Start);
        result.add_css_class("title-4");

        let buttons = GtkBox::new(Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        let check_btn = Button::with_label("Check");
        let save_btn = Button::with_label("Save");
        save_btn.set_sensitive(false);
        let cancel_btn = Button::with_label("Cancel");
        buttons.append(&check_btn);
        buttons.append(&save_btn);
        buttons.append(&cancel_btn);

        vbox.append(&row);
        vbox.append(&status);
        vbox.append(&result);
        vbox.append(&buttons);
        window.set_child(Some(&vbox));

        let current_location: Rc<RefCell<Option<Location>>> = Rc::new(RefCell::new(None));

        // Check button handler
        let entry_check = entry.clone();
        let status_check = status.clone();
        let result_check = result.clone();
        let save_check = save_btn.clone();
        let key_check = key.clone();
        let current_for_check = current_location.clone();
        check_btn.connect_clicked(move |_| {
            let q = entry_check.text().trim().to_string();
            handle_geocode_check(
                key_check.clone(),
                q,
                status_check.clone(),
                result_check.clone(),
                save_check.clone(),
                current_for_check.clone(),
            );
        });

        // Save button handler
        let current_save = current_location.clone();
        let window_for_save = window.clone();
        save_btn.connect_clicked(move |_| {
            if let Some(loc) = current_save.borrow().as_ref() {
                // Save as home location
                if let Err(e) = save_home_location(loc) {
                    eprintln!("Failed to save home location: {}", e);
                }
                reload_waybar();
            }
            window_for_save.close();
        });

        // Entry activate (Enter key) handler
        let entry_return = entry.clone();
        let status_return = status.clone();
        let result_return = result.clone();
        let save_return = save_btn.clone();
        let key_return = key.clone();
        let current_for_return = current_location.clone();
        entry.connect_activate(move |_| {
            let q = entry_return.text().trim().to_string();
            handle_geocode_check(
                key_return.clone(),
                q,
                status_return.clone(),
                result_return.clone(),
                save_return.clone(),
                current_for_return.clone(),
            );
        });

        // Cancel button handler
        let window_for_cancel = window.clone();
        cancel_btn.connect_clicked(move |_| {
            window_for_cancel.close();
        });

        window.show();
    });

    app.run_with_args::<String>(&[]);
    Ok(())
}
