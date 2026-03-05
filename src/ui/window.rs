//! Main application window
//!
//! Uses centralized state management for clean, predictable UI updates.

use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;

use std::cell::RefCell;

use crate::calibration::{CalibrationCommand, CalibrationMessage};
use crate::ui::device_list::DeviceList;
use crate::ui::state::{SharedState, create_shared_state};
use crate::xr::xr_event_loop;

/// Wrapper for toast overlay that dismisses previous toast before showing new one
#[derive(Clone)]
struct ToastManager {
    overlay: adw::ToastOverlay,
    current_toast: Rc<RefCell<Option<adw::Toast>>>,
}

impl ToastManager {
    fn new(overlay: adw::ToastOverlay) -> Self {
        Self {
            overlay,
            current_toast: Rc::new(RefCell::new(None)),
        }
    }

    /// Show a toast, dismissing any existing one first
    fn show(&self, message: &str) {
        // Dismiss previous toast if any
        if let Some(prev) = self.current_toast.borrow_mut().take() {
            prev.dismiss();
        }

        let toast = adw::Toast::new(message);
        self.current_toast.borrow_mut().replace(toast.clone());
        self.overlay.add_toast(toast);
    }
}

pub fn build_ui(app: &adw::Application) {
    // Use AdwStyleManager for theme (avoids GtkSettings warning)
    let style_manager = adw::StyleManager::default();
    style_manager.set_color_scheme(adw::ColorScheme::PreferDark);

    // Create centralized state
    let state = create_shared_state();

    // Create channels for calibration thread communication
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (msg_tx, msg_rx) = async_channel::bounded::<CalibrationMessage>(100);

    // Spawn calibration background thread
    thread::spawn(move || {
        xr_event_loop(cmd_rx, msg_tx);
    });

    let cmd_tx = Rc::new(cmd_tx);

    // Build the window
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("SpaceCal for Monado")
        .default_width(720)
        .default_height(180)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    window.set_content(Some(&toolbar_view));

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&Label::new(Some("SpaceCal for Monado"))));
    toolbar_view.add_top_bar(&header_bar);

    let toast_overlay = adw::ToastOverlay::new();
    toolbar_view.set_content(Some(&toast_overlay));
    let toasts = ToastManager::new(toast_overlay.clone());

    let main_box = GtkBox::new(Orientation::Vertical, 20);
    main_box.set_margin_top(20);
    main_box.set_margin_bottom(20);
    main_box.set_margin_start(24);
    main_box.set_margin_end(24);
    toast_overlay.set_child(Some(&main_box));

    // Top row: dropdowns (centered, fixed width)
    let top_row = GtkBox::new(Orientation::Horizontal, 16);
    top_row.set_halign(Align::Center);
    top_row.set_hexpand(false);
    main_box.append(&top_row);

    let source_list = DeviceList::new("Source");
    source_list.widget().set_width_request(250);
    source_list.widget().set_tooltip_text(Some("Reference device with correct tracking"));
    top_row.append(source_list.widget());

    let target_list = DeviceList::new("Target");
    target_list.widget().set_width_request(250);
    target_list.widget().set_tooltip_text(Some("Device to calibrate (its origin is adjusted)"));
    top_row.append(target_list.widget());

    // Button row
    let button_box = GtkBox::new(Orientation::Horizontal, 12);
    button_box.set_halign(Align::Center);
    main_box.append(&button_box);

    let refresh_btn = Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.set_tooltip_text(Some("Refresh Devices"));
    button_box.append(&refresh_btn);

    let calibrate_btn = Button::with_label("Calibrate");
    calibrate_btn.add_css_class("suggested-action");
    calibrate_btn.set_width_request(110);
    calibrate_btn.set_tooltip_text(Some("Align target tracking to source (hold together)"));
    button_box.append(&calibrate_btn);

    let floor_btn = Button::with_label("Floor");
    floor_btn.set_width_request(90);
    floor_btn.set_tooltip_text(Some("Set floor height using target device position"));
    button_box.append(&floor_btn);

    let recenter_btn = Button::with_label("Recenter");
    recenter_btn.set_width_request(90);
    recenter_btn.set_tooltip_text(Some("Set STAGE origin to current HMD position and heading"));
    button_box.append(&recenter_btn);

    // Reset split button - main action resets target, dropdown allows choosing any tracking origin
    let reset_btn = adw::SplitButton::new();
    reset_btn.set_label("Reset");
    reset_btn.add_css_class("destructive-action");
    reset_btn.set_tooltip_text(Some("Reset target tracking origin (dropdown for more)"));

    // Create popover with tracking origin list for reset
    let reset_popover = gtk4::Popover::new();
    let reset_list = gtk4::ListBox::new();
    reset_list.set_selection_mode(gtk4::SelectionMode::None);
    reset_list.add_css_class("boxed-list");
    reset_list.set_margin_start(6);
    reset_list.set_margin_end(6);
    reset_list.set_margin_top(6);
    reset_list.set_margin_bottom(6);
    reset_popover.set_child(Some(&reset_list));
    reset_btn.set_popover(Some(&reset_popover));

    button_box.append(&reset_btn);

    // Status indicator
    let status_label = Label::new(None);
    status_label.set_halign(Align::Center);
    status_label.add_css_class("dim-label");
    status_label.set_margin_top(8);
    main_box.append(&status_label);

    // Battery status bar
    let battery_bar = GtkBox::new(Orientation::Horizontal, 16);
    battery_bar.set_halign(Align::Center);
    battery_bar.set_margin_top(4);
    main_box.append(&battery_bar);

    // Function to sync UI with state
    fn update_ui_from_state(
        state: &SharedState,
        source_list: &Rc<DeviceList>,
        target_list: &Rc<DeviceList>,
        status_label: &Label,
        battery_bar: &GtkBox,
    ) {
        let s = state.borrow();

        // Update status
        if s.is_connected() {
            status_label.set_text("Connected");
            status_label.remove_css_class("warning");
        } else {
            status_label.set_text("Waiting for WiVRn...");
            if !status_label.has_css_class("warning") {
                status_label.add_css_class("warning");
            }
        }

        // Update source list with target-filtered devices
        let source_devices = s.source_devices();
        source_list.set_devices(source_devices, s.source_name());

        // Update target list with source-filtered devices
        let target_devices = s.target_devices();
        target_list.set_devices(target_devices, s.target_name());

        // Update battery bar
        update_battery_bar(battery_bar, &s);
    }

    /// Update the battery status bar with current device battery levels
    fn update_battery_bar(bar: &GtkBox, state: &crate::ui::state::AppState) {
        // Clear existing children
        while let Some(child) = bar.first_child() {
            bar.remove(&child);
        }

        let battery_list = state.battery_status_list();
        if battery_list.is_empty() {
            return;
        }

        for info in &battery_list {
            let item = GtkBox::new(Orientation::Vertical, 2);
            item.set_halign(Align::Center);

            if !info.online {
                let top = GtkBox::new(Orientation::Horizontal, 4);
                top.set_halign(Align::Center);
                let icon = gtk4::Image::from_icon_name("network-offline-symbolic");
                icon.set_pixel_size(16);
                icon.add_css_class("dim-label");
                let label = Label::new(Some(&format!("{} Offline", info.short_name)));
                label.add_css_class("caption");
                label.add_css_class("dim-label");
                top.append(&icon);
                top.append(&label);
                item.append(&top);
            } else if let Some(charge) = info.charge {
                let pct = (charge * 100.0).round() as u32;
                let icon_name = if info.charging {
                    if pct > 75 { "battery-full-charging-symbolic" }
                    else if pct > 40 { "battery-good-charging-symbolic" }
                    else if pct > 15 { "battery-low-charging-symbolic" }
                    else { "battery-caution-charging-symbolic" }
                } else if pct > 75 { "battery-full-symbolic" }
                else if pct > 40 { "battery-good-symbolic" }
                else if pct > 15 { "battery-low-symbolic" }
                else { "battery-caution-symbolic" };

                let top = GtkBox::new(Orientation::Horizontal, 4);
                top.set_halign(Align::Center);
                let icon = gtk4::Image::from_icon_name(icon_name);
                icon.set_pixel_size(16);
                let text = format!("{} {}%", info.short_name, pct);
                let label = Label::new(Some(&text));
                label.add_css_class("caption");
                if pct <= 15 {
                    label.add_css_class("error");
                    icon.add_css_class("error");
                } else if pct <= 40 {
                    label.add_css_class("warning");
                    icon.add_css_class("warning");
                } else {
                    label.add_css_class("dim-label");
                    icon.add_css_class("dim-label");
                }
                top.append(&icon);
                top.append(&label);
                item.append(&top);
            }

            if !info.serial_suffix.is_empty() {
                let serial_label = Label::new(Some(&info.serial_suffix));
                serial_label.add_css_class("caption");
                serial_label.add_css_class("dim-label");
                serial_label.set_halign(Align::Center);
                item.append(&serial_label);
            }

            bar.append(&item);
        }
    }

    // Function to update only movement intensities (live, no rebuild)
    fn update_movement_only(
        state: &SharedState,
        source_list: &Rc<DeviceList>,
        target_list: &Rc<DeviceList>,
    ) {
        // Clone intensities and drop borrow before updating lists
        let intensities = state.borrow().movement_intensities().clone();
        source_list.update_movement(&intensities);
        target_list.update_movement(&intensities);
    }

    // Initial UI sync
    update_ui_from_state(&state, &source_list, &target_list, &status_label, &battery_bar);

    // Source selection changed
    let state_for_source = Rc::clone(&state);
    let source_list_for_source = Rc::clone(&source_list);
    let target_list_for_source = Rc::clone(&target_list);
    let status_for_source = status_label.clone();
    source_list.connect_changed(move |device| {
        let unique_id = device.map(|d| d.unique_id().to_string());
        state_for_source.borrow_mut().set_source(unique_id);
        // Re-sync target dropdown (its available devices may have changed)
        let s = state_for_source.borrow();
        let target_devices = s.target_devices();
        target_list_for_source.set_devices(target_devices, s.target_name());
        drop(s);
        // Don't need full update_ui_from_state, just the target dropdown
        let _ = (&source_list_for_source, &status_for_source); // silence warnings
    });

    // Target selection changed
    let state_for_target = Rc::clone(&state);
    let source_list_for_target = Rc::clone(&source_list);
    let target_list_for_target = Rc::clone(&target_list);
    let status_for_target = status_label.clone();
    target_list.connect_changed(move |device| {
        let unique_id = device.map(|d| d.unique_id().to_string());
        state_for_target.borrow_mut().set_target(unique_id);
        // Re-sync source dropdown (its available devices may have changed)
        let s = state_for_target.borrow();
        let source_devices = s.source_devices();
        source_list_for_target.set_devices(source_devices, s.source_name());
        drop(s);
        let _ = (&target_list_for_target, &status_for_target); // silence warnings
    });

    // Refresh button
    let state_for_refresh = Rc::clone(&state);
    let source_list_for_refresh = Rc::clone(&source_list);
    let target_list_for_refresh = Rc::clone(&target_list);
    let status_for_refresh = status_label.clone();
    let battery_bar_for_refresh = battery_bar.clone();
    refresh_btn.connect_clicked(move |_| {
        state_for_refresh.borrow_mut().force_refresh();
        update_ui_from_state(
            &state_for_refresh,
            &source_list_for_refresh,
            &target_list_for_refresh,
            &status_for_refresh,
            &battery_bar_for_refresh,
        );
    });

    // Automatic connection monitoring + battery polling
    // - When disconnected: try to connect every 500ms (no IPC churn — just socket check)
    // - When connected: refresh batteries every 5s using existing connection (no reconnect)
    let state_for_monitor = Rc::clone(&state);
    let source_list_for_monitor = Rc::clone(&source_list);
    let target_list_for_monitor = Rc::clone(&target_list);
    let status_for_monitor = status_label.clone();
    let battery_bar_for_monitor = battery_bar.clone();

    fn schedule_poll(
        state: SharedState,
        source_list: Rc<DeviceList>,
        target_list: Rc<DeviceList>,
        status_label: Label,
        battery_bar: GtkBox,
    ) {
        let is_connected = state.borrow().is_connected();
        let interval_ms = if is_connected { 5000 } else { 500 };

        glib::timeout_add_local_once(
            std::time::Duration::from_millis(interval_ms),
            move || {
                if state.borrow().is_connected() {
                    // Already connected — just refresh batteries (cheap, reuses connection)
                    state.borrow_mut().refresh_batteries();
                    update_battery_bar(&battery_bar, &state.borrow());

                    // If connection was lost during battery refresh, update full UI
                    if !state.borrow().is_connected() {
                        update_ui_from_state(&state, &source_list, &target_list, &status_label, &battery_bar);
                    }
                } else {
                    // Not connected — try to connect
                    let changed = state.borrow_mut().refresh_connection();
                    if changed {
                        update_ui_from_state(&state, &source_list, &target_list, &status_label, &battery_bar);
                    }
                }

                schedule_poll(state, source_list, target_list, status_label, battery_bar);
            },
        );
    }

    schedule_poll(
        state_for_monitor,
        source_list_for_monitor,
        target_list_for_monitor,
        status_for_monitor,
        battery_bar_for_monitor,
    );

    // Track open popovers - only run movement detection when at least one is open
    let open_popovers = Rc::new(std::cell::RefCell::new(0u32));

    // Source popover visibility
    let cmd_tx_source_vis = Rc::clone(&cmd_tx);
    let open_popovers_source = Rc::clone(&open_popovers);
    let state_source_vis = Rc::clone(&state);
    source_list.connect_popover_visibility(move |opened| {
        let mut count = open_popovers_source.borrow_mut();
        if opened {
            if *count == 0 && state_source_vis.borrow().is_connected() {
                let _ = cmd_tx_source_vis.send(CalibrationCommand::StartMovementDetection);
            }
            *count += 1;
        } else {
            *count = count.saturating_sub(1);
            if *count == 0 {
                let _ = cmd_tx_source_vis.send(CalibrationCommand::StopMovementDetection);
            }
        }
    });

    // Target popover visibility
    let cmd_tx_target_vis = Rc::clone(&cmd_tx);
    let open_popovers_target = Rc::clone(&open_popovers);
    let state_target_vis = Rc::clone(&state);
    target_list.connect_popover_visibility(move |opened| {
        let mut count = open_popovers_target.borrow_mut();
        if opened {
            if *count == 0 && state_target_vis.borrow().is_connected() {
                let _ = cmd_tx_target_vis.send(CalibrationCommand::StartMovementDetection);
            }
            *count += 1;
        } else {
            *count = count.saturating_sub(1);
            if *count == 0 {
                let _ = cmd_tx_target_vis.send(CalibrationCommand::StopMovementDetection);
            }
        }
    });

    // Calibrate button
    let toasts_for_calibrate = toasts.clone();
    let state_for_calibrate = Rc::clone(&state);
    let cmd_tx_calibrate = Rc::clone(&cmd_tx);
    calibrate_btn.connect_clicked(move |btn| {
        let s = state_for_calibrate.borrow();
        let src = s.selected_source();
        let tgt = s.selected_target();

        if src.is_none() || tgt.is_none() {
            toasts_for_calibrate.show("Select both source and target devices");
            return;
        }

        let src = src.unwrap();
        let tgt = tgt.unwrap();

        // Get stage offset from Monado (like motoc does) to transform poses to common frame
        let stage_offset = s.connection().and_then(|conn| conn.get_stage_offset().ok());

        if let Err(e) = cmd_tx_calibrate.send(CalibrationCommand::StartSampled {
            source_serial: src.unique_id().to_string(),
            target_serial: tgt.unique_id().to_string(),
            target_origin_index: tgt.category_index,
            sample_count: 500,
            stage_offset,
        }) {
            toasts_for_calibrate.show(&format!("Failed to start calibration: {}", e));
            return;
        }

        btn.set_label("Calibrating...");
        btn.set_sensitive(false);
    });

    // Floor button - uses target device position to set floor level
    let toasts_for_floor = toasts.clone();
    let cmd_tx_floor = Rc::clone(&cmd_tx);
    let state_for_floor = Rc::clone(&state);
    floor_btn.connect_clicked(move |btn| {
        // Get target device - floor calibration uses its position
        let target_serial = {
            let s = state_for_floor.borrow();
            match s.selected_target() {
                Some(dev) => dev.unique_id().to_string(),
                None => {
                    toasts_for_floor.show("Select a target device first, then place it on the floor");
                    return;
                }
            }
        };

        if let Err(e) = cmd_tx_floor.send(CalibrationCommand::CalibrateFloor { target_serial }) {
            toasts_for_floor.show(&format!("Failed to start floor calibration: {}", e));
            return;
        }

        btn.set_label("Detecting...");
        btn.set_sensitive(false);
    });

    // Reset button - main click resets target tracking origin
    let toasts_for_reset = toasts.clone();
    let state_for_reset = Rc::clone(&state);
    reset_btn.connect_clicked(move |_| {
        let s = state_for_reset.borrow();
        let tgt = s.selected_target();

        if let Some(device) = tgt {
            if let Some(conn) = s.connection() {
                match conn.reset_tracking_origin(device.category_index) {
                    Ok(_) => toasts_for_reset.show(&format!("Reset {}", device.category)),
                    Err(e) => toasts_for_reset.show(&format!("Reset failed: {}", e)),
                }
            } else {
                toasts_for_reset.show("Not connected to Monado");
            }
        } else {
            toasts_for_reset.show("Select a target device first");
        }
    });

    // Populate reset dropdown when popover is shown
    let state_for_reset_popover = Rc::clone(&state);
    let reset_list_for_show = reset_list.clone();
    reset_popover.connect_show(move |_| {
        // Clear existing rows
        while let Some(row) = reset_list_for_show.first_child() {
            reset_list_for_show.remove(&row);
        }

        let s = state_for_reset_popover.borrow();
        let categories = s.categories();

        // Add tracking origin categories (skip source - nothing to reset there)
        for category in categories {
            let row = gtk4::ListBoxRow::new();
            let label = Label::new(Some(&category.name));
            label.set_xalign(0.0);
            label.set_margin_start(12);
            label.set_margin_end(12);
            label.set_margin_top(8);
            label.set_margin_bottom(8);
            row.set_child(Some(&label));
            reset_list_for_show.append(&row);
        }

        // Add separator
        let sep = gtk4::Separator::new(Orientation::Horizontal);
        let sep_row = gtk4::ListBoxRow::new();
        sep_row.set_child(Some(&sep));
        sep_row.set_selectable(false);
        sep_row.set_activatable(false);
        reset_list_for_show.append(&sep_row);

        // Add Floor reset option
        let floor_row = gtk4::ListBoxRow::new();
        let floor_label = Label::new(Some("Floor Level"));
        floor_label.set_xalign(0.0);
        floor_label.set_margin_start(12);
        floor_label.set_margin_end(12);
        floor_label.set_margin_top(8);
        floor_label.set_margin_bottom(8);
        floor_row.set_child(Some(&floor_label));
        reset_list_for_show.append(&floor_row);

        // Add Center reset option
        let center_row = gtk4::ListBoxRow::new();
        let center_label = Label::new(Some("Center"));
        center_label.set_xalign(0.0);
        center_label.set_margin_start(12);
        center_label.set_margin_end(12);
        center_label.set_margin_top(8);
        center_label.set_margin_bottom(8);
        center_row.set_child(Some(&center_label));
        reset_list_for_show.append(&center_row);
    });

    // Handle reset dropdown row activation
    let state_for_reset_list = Rc::clone(&state);
    let toasts_for_reset_list = toasts.clone();
    let reset_popover_for_list = reset_popover.clone();
    reset_list.connect_row_activated(move |_, row| {
        let index = row.index() as usize;
        let s = state_for_reset_list.borrow();
        let categories = s.categories();
        let num_categories = categories.len();

        // Check if it's a tracking origin or special option
        if index < num_categories {
            // Reset tracking origin
            if let Some(category) = categories.get(index) {
                if let Some(conn) = s.connection() {
                    match conn.reset_tracking_origin(category.index) {
                        Ok(_) => toasts_for_reset_list.show(&format!("Reset {}", category.name)),
                        Err(e) => toasts_for_reset_list.show(&format!("Reset failed: {}", e)),
                    }
                } else {
                    toasts_for_reset_list.show("Not connected to Monado");
                }
            }
        } else if index == num_categories + 1 {
            // Reset Floor (index after separator)
            if let Some(conn) = s.connection() {
                match conn.reset_floor() {
                    Ok(_) => toasts_for_reset_list.show("Floor reset"),
                    Err(e) => toasts_for_reset_list.show(&format!("Reset floor failed: {}", e)),
                }
            } else {
                toasts_for_reset_list.show("Not connected to Monado");
            }
        } else if index == num_categories + 2 {
            // Reset Center
            if let Some(conn) = s.connection() {
                match conn.reset_center() {
                    Ok(_) => toasts_for_reset_list.show("Center reset"),
                    Err(e) => toasts_for_reset_list.show(&format!("Reset center failed: {}", e)),
                }
            } else {
                toasts_for_reset_list.show("Not connected to Monado");
            }
        }

        reset_popover_for_list.popdown();
    });

    // Recenter button - uses source device orientation to set forward direction
    let toasts_for_recenter = toasts.clone();
    let state_for_recenter = Rc::clone(&state);
    let cmd_tx_recenter = Rc::clone(&cmd_tx);
    recenter_btn.connect_clicked(move |btn| {
        // Get source device (HMD) for orientation reference
        let source_serial = {
            let s = state_for_recenter.borrow();
            match s.selected_source() {
                Some(dev) => dev.unique_id().to_string(),
                None => {
                    toasts_for_recenter.show("Select a source device first");
                    return;
                }
            }
        };

        if let Err(e) = cmd_tx_recenter.send(CalibrationCommand::Recenter { source_serial }) {
            toasts_for_recenter.show(&format!("Failed to start recenter: {}", e));
            return;
        }

        btn.set_label("Recentering...");
        btn.set_sensitive(false);
    });

    // Message handler for calibration results and movement updates
    let calibrate_btn_for_msg = calibrate_btn.clone();
    let floor_btn_for_msg = floor_btn.clone();
    let recenter_btn_for_msg = recenter_btn.clone();
    let toasts_for_msg = toasts.clone();
    let state_for_msg = Rc::clone(&state);
    let source_list_for_msg = Rc::clone(&source_list);
    let target_list_for_msg = Rc::clone(&target_list);

    glib::source::idle_add_local(move || {
        while let Ok(msg) = msg_rx.try_recv() {
            match msg {
                CalibrationMessage::Countdown { seconds } => {
                    calibrate_btn_for_msg.set_label(&format!("{}...", seconds));
                }
                CalibrationMessage::RecenterCountdown { seconds } => {
                    recenter_btn_for_msg.set_label(&format!("{}...", seconds));
                }
                CalibrationMessage::Progress { collected, total } => {
                    calibrate_btn_for_msg.set_label(&format!("{}/{}...", collected, total));
                }
                CalibrationMessage::FloorProgress { collected, total } => {
                    floor_btn_for_msg.set_label(&format!("{}/{}...", collected, total));
                }
                CalibrationMessage::SampledComplete(result) => {
                    calibrate_btn_for_msg.set_label("Calibrate");
                    calibrate_btn_for_msg.set_sensitive(true);

                    // Apply the calibration offset to the TARGET tracking origin
                    let s = state_for_msg.borrow();
                    if let Some(conn) = s.connection() {
                        let position = result.transform.position_f64();
                        let orientation = result.transform.orientation_f64();
                        match conn.apply_offset(result.target_origin_index, position, orientation) {
                            Ok(_) => toasts_for_msg.show(&format!(
                                "Calibration applied ({} samples)",
                                result.sample_count
                            )),
                            Err(e) => toasts_for_msg.show(&format!("Failed to apply calibration: {}", e)),
                        }
                    } else {
                        toasts_for_msg.show("Cannot apply calibration: not connected");
                    }
                }
                CalibrationMessage::FloorComplete { height_adjustment } => {
                    floor_btn_for_msg.set_label("Floor");
                    floor_btn_for_msg.set_sensitive(true);

                    // Floor calibration:
                    // height_adjustment = -(measured_floor_in_stage), so measured = -height_adjustment
                    // set_floor_absolute converts to native coords internally
                    let measured_floor = -height_adjustment as f64;

                    let apply_result = {
                        let s = state_for_msg.borrow();
                        if let Some(conn) = s.connection() {
                            Some(conn.set_floor_absolute(measured_floor))
                        } else {
                            None
                        }
                    };

                    match apply_result {
                        Some(Ok(_)) => {
                            let delta_cm = measured_floor * 100.0;
                            let direction = if measured_floor >= 0.0 { "up" } else { "down" };
                            toasts_for_msg.show(&format!(
                                "Floor set {:.1}cm {}",
                                delta_cm.abs(), direction
                            ));
                        }
                        Some(Err(e)) => toasts_for_msg.show(&format!("Failed to apply floor: {}", e)),
                        None => toasts_for_msg.show("Cannot apply floor: not connected"),
                    }
                }
                CalibrationMessage::RecenterComplete { position, orientation } => {
                    recenter_btn_for_msg.set_label("Recenter");
                    recenter_btn_for_msg.set_sensitive(true);

                    // Apply recenter using STAGE reference space (motoc approach)
                    let s = state_for_msg.borrow();
                    if let Some(conn) = s.connection() {
                        match conn.apply_recenter_absolute(position, orientation) {
                            Ok(_) => {
                                // Show the X-Z offset and yaw applied
                                let x_cm = position[0] * 100.0;
                                let z_cm = position[2] * 100.0;

                                // Extract yaw from quaternion
                                let q = orientation;
                                let yaw = (2.0 * (q[3] * q[1] + q[0] * q[2]) as f64)
                                    .atan2(1.0 - 2.0 * (q[1] * q[1] + q[2] * q[2]) as f64);
                                let yaw_deg = yaw.to_degrees();

                                // Format X direction
                                let x_str = if x_cm.abs() < 1.0 {
                                    String::new()
                                } else if x_cm > 0.0 {
                                    format!("{:.0}cm right", x_cm)
                                } else {
                                    format!("{:.0}cm left", -x_cm)
                                };

                                // Format Z direction (negative Z is forward in OpenXR)
                                let z_str = if z_cm.abs() < 1.0 {
                                    String::new()
                                } else if z_cm > 0.0 {
                                    format!("{:.0}cm back", z_cm)
                                } else {
                                    format!("{:.0}cm forward", -z_cm)
                                };

                                // Format yaw (positive = turned right, negative = turned left)
                                let yaw_str = if yaw_deg.abs() < 1.0 {
                                    String::new()
                                } else if yaw_deg > 0.0 {
                                    format!("{:.0}° right", yaw_deg)
                                } else {
                                    format!("{:.0}° left", -yaw_deg)
                                };

                                // Build message from non-empty parts
                                let parts: Vec<&str> = [x_str.as_str(), z_str.as_str(), yaw_str.as_str()]
                                    .into_iter()
                                    .filter(|s| !s.is_empty())
                                    .collect();

                                if parts.is_empty() {
                                    toasts_for_msg.show("Centered");
                                } else {
                                    toasts_for_msg.show(&format!("Centered ({})", parts.join(", ")));
                                }
                            }
                            Err(e) => toasts_for_msg.show(&format!("Failed to apply recenter: {}", e)),
                        }
                    } else {
                        toasts_for_msg.show("Cannot apply recenter: not connected");
                    }
                }
                CalibrationMessage::ResetFloorComplete => {
                    toasts_for_msg.show("Floor reset");
                }
                CalibrationMessage::MovementUpdate { movements } => {
                    state_for_msg.borrow_mut().set_movement_intensities(movements);
                    update_movement_only(
                        &state_for_msg,
                        &source_list_for_msg,
                        &target_list_for_msg,
                    );
                }
                CalibrationMessage::Error(e) => {
                    calibrate_btn_for_msg.set_label("Calibrate");
                    calibrate_btn_for_msg.set_sensitive(true);
                    floor_btn_for_msg.set_label("Floor");
                    floor_btn_for_msg.set_sensitive(true);
                    recenter_btn_for_msg.set_label("Recenter");
                    recenter_btn_for_msg.set_sensitive(true);
                    toasts_for_msg.show(&format!("Error: {}", e));
                }
                _ => {}
            }
        }
        glib::ControlFlow::Continue
    });

    // Shutdown handler
    let cmd_tx_for_shutdown = Rc::clone(&cmd_tx);
    let app_for_shutdown = app.clone();
    window.connect_close_request(move |_| {
        let _ = cmd_tx_for_shutdown.send(CalibrationCommand::Shutdown);
        app_for_shutdown.quit();
        glib::Propagation::Proceed
    });

    window.present();
}
