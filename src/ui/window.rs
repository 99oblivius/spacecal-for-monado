//! Main application window.

use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CssProvider, Label, Orientation, Overlay};
use libadwaita as adw;
use libadwaita::prelude::*;

use std::cell::{Cell, RefCell};

use crate::calibration::{CalibrationCommand, CalibrationMessage};
use crate::ui::device_list::DeviceList;
use crate::ui::state::{SharedState, create_shared_state};
use crate::xr::xr_event_loop;

/// Dismisses previous toast before showing a new one.
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

    fn show(&self, message: &str) {
        // Dismiss previous toast if any
        if let Some(prev) = self.current_toast.borrow_mut().take() {
            prev.dismiss();
        }

        let toast = adw::Toast::new(message);
        self.current_toast.borrow_mut().replace(toast.clone());
        self.overlay.add_toast(toast);
    }

    fn show_calibration_result(&self, confidence: u32, axis_diversity: f32) {
        if let Some(prev) = self.current_toast.borrow_mut().take() {
            prev.dismiss();
        }

        let toast = adw::Toast::new("");
        toast.set_timeout(0); // Stay until dismissed

        let color = if confidence < 50 {
            "#a51d2d" // dark red
        } else if confidence < 75 {
            "#e01b24" // red
        } else if confidence < 80 {
            "#e66100" // orange
        } else if confidence < 90 {
            "#c88800" // yellow
        } else {
            "#2ec27e" // green
        };

        let diversity_warning = if axis_diversity < 0.15 {
            "\n<span color=\"#e01b24\">Motion too linear \u{2014} sweep a wide figure-eight next time</span>"
        } else if axis_diversity < 0.333 {
            "\n<span color=\"#e66100\">Limited axis coverage \u{2014} tilt into each curve of the figure-eight</span>"
        } else {
            ""
        };

        let label = Label::new(None);
        label.set_markup(&format!(
            "Calibration complete \u{2014} <span color=\"{}\"><b>{}%</b></span> confidence{}",
            color, confidence, diversity_warning
        ));
        label.set_wrap(true);
        toast.set_custom_title(Some(&label));

        self.current_toast.borrow_mut().replace(toast.clone());
        self.overlay.add_toast(toast);
    }
}

fn play_sound(sound_id: &str) {
    let _ = std::process::Command::new("canberra-gtk-play")
        .arg("-i")
        .arg(sound_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn restore_idle_visuals(background_box: &GtkBox, countdown_label: &Label, progress_fill: &GtkBox) {
    background_box.remove_css_class("calibration-bg-active");
    countdown_label.set_visible(false);
    progress_fill.set_visible(false);
    progress_fill.remove_css_class("progress-fill-active");
    progress_fill.set_size_request(0, -1);
}

fn update_progress_fill(fill: &GtkBox, window: &adw::ApplicationWindow, fraction: f64) {
    let fill_width = (window.width() as f64 * fraction) as i32;
    fill.set_size_request(fill_width, -1);
}

const CALIBRATION_MOTION_SVG: &str = include_str!("../../assets/calibration_motion.svg");

fn build_help_dialog(state: &SharedState, calibrate_action: bool) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading("Calibration Instructions")
        .build();

    let extra_box = GtkBox::new(Orientation::Vertical, 16);

    // Instructions label with Pango markup for bold key phrases
    let instructions = Label::new(None);
    instructions.set_markup(
        "1. Grip both selected devices <b>firmly together</b> \u{2014} they must \
         not shift relative to each other\n\n\
         2. Wait for the countdown beeps to finish, then \
         <b>slowly sweep a wide figure-eight</b> (\u{221E}) through the air, \
         tilting into each curve as shown below\n\n\
         3. <b>Keep moving</b> until the progress bar completes or you hear \
         the chime",
    );
    instructions.set_xalign(0.0);
    instructions.set_wrap(true);
    instructions.set_max_width_chars(56);
    extra_box.append(&instructions);

    // SVG motion diagram
    let svg_bytes = glib::Bytes::from_static(CALIBRATION_MOTION_SVG.as_bytes());
    if let Ok(texture) = gtk4::gdk::Texture::from_bytes(&svg_bytes) {
        let picture = gtk4::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_halign(Align::Center);
        extra_box.append(&picture);
    }

    // Tip label — centered, smaller, dimmed
    let tip = Label::new(Some(
        "Tip: Open a device dropdown and move a device to see which one it is",
    ));
    tip.set_halign(Align::Center);
    tip.add_css_class("dim-label");
    tip.add_css_class("caption");
    extra_box.append(&tip);

    // "Don't show before calibrating" toggle
    let pref_box = GtkBox::new(Orientation::Horizontal, 12);
    pref_box.set_halign(Align::Center);
    let pref_label = Label::new(Some("Don't show before calibrating"));
    let pref_switch = gtk4::Switch::new();
    pref_switch.set_active(state.borrow().hide_calibration_help());

    let state_for_switch = Rc::clone(state);
    pref_switch.connect_active_notify(move |switch| {
        state_for_switch
            .borrow_mut()
            .set_hide_calibration_help(switch.is_active());
    });

    pref_box.append(&pref_label);
    pref_box.append(&pref_switch);
    extra_box.append(&pref_box);

    dialog.set_extra_child(Some(&extra_box));

    if calibrate_action {
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("calibrate", "Calibrate");
        dialog.set_response_appearance("calibrate", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("calibrate"));
        dialog.set_close_response("cancel");
    } else {
        dialog.add_response("got_it", "Got it");
    }

    dialog
}

pub fn build_ui(app: &adw::Application) {
    // Use AdwStyleManager for theme (avoids GtkSettings warning)
    let style_manager = adw::StyleManager::default();
    style_manager.set_color_scheme(adw::ColorScheme::PreferDark);

    // Load calibration feedback CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(
        ".calibration-bg-active { \
            background-color: rgba(53, 132, 228, 0.12); \
            transition: background-color 500ms ease; \
        } \
        .progress-fill-active { \
            background-color: rgba(53, 132, 228, 0.38); \
        } \
        .countdown-text { \
            font-size: 420px; \
            font-weight: 900; \
            font-family: Impact, sans-serif; \
            color: rgba(53, 132, 228, 0.35); \
        }",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to a display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

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
        .default_width(800)
        .default_height(630)
        .build();

    window.set_size_request(800, 630);

    let toolbar_view = adw::ToolbarView::new();

    // Overlay layers: background (child) → countdown label → toolbar (existing UI)
    let overlay = Overlay::new();
    window.set_content(Some(&overlay));

    let background_box = GtkBox::new(Orientation::Vertical, 0);
    background_box.set_hexpand(true);
    background_box.set_vexpand(true);
    overlay.set_child(Some(&background_box));

    let progress_fill = GtkBox::new(Orientation::Vertical, 0);
    progress_fill.set_halign(Align::Start);
    progress_fill.set_vexpand(true);
    progress_fill.set_visible(false);
    background_box.append(&progress_fill);

    let countdown_label = Label::new(None);
    countdown_label.set_halign(Align::Center);
    countdown_label.set_valign(Align::Center);
    countdown_label.add_css_class("countdown-text");
    countdown_label.set_visible(false);
    overlay.add_overlay(&countdown_label);

    overlay.add_overlay(&toolbar_view);
    overlay.set_measure_overlay(&toolbar_view, true);

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&Label::new(Some("SpaceCal for Monado"))));
    header_bar.set_decoration_layout(Some(":close"));
    toolbar_view.add_top_bar(&header_bar);

    // Help button in header bar
    let help_btn = Button::from_icon_name("dialog-question-symbolic");
    help_btn.set_tooltip_text(Some("Calibration Instructions"));
    header_bar.pack_end(&help_btn);

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
    source_list
        .widget()
        .set_tooltip_text(Some("Reference device with correct tracking"));
    top_row.append(source_list.widget());

    let target_list = DeviceList::new("Target");
    target_list.widget().set_width_request(250);
    target_list
        .widget()
        .set_tooltip_text(Some("Device to calibrate (its origin is adjusted)"));
    top_row.append(target_list.widget());

    // Button row
    let button_box = GtkBox::new(Orientation::Horizontal, 12);
    button_box.set_halign(Align::Center);
    main_box.append(&button_box);

    let refresh_btn = Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.set_tooltip_text(Some("Refresh Devices"));
    button_box.append(&refresh_btn);

    let calibrate_btn = adw::SplitButton::new();
    calibrate_btn.set_label("Calibrate");
    calibrate_btn.add_css_class("suggested-action");
    calibrate_btn.set_width_request(110);
    calibrate_btn.set_tooltip_text(Some(
        "Align target tracking to source (dropdown: sample count)",
    ));

    // Sample count dropdown (200, 400, 600)
    let sample_count = Rc::new(Cell::new(state.borrow().sample_count()));
    let sample_popover = gtk4::Popover::new();
    let sample_list = gtk4::ListBox::new();
    sample_list.set_selection_mode(gtk4::SelectionMode::None);
    sample_list.add_css_class("boxed-list");
    sample_list.set_margin_start(6);
    sample_list.set_margin_end(6);
    sample_list.set_margin_top(6);
    sample_list.set_margin_bottom(6);

    let sample_options: &[(u32, &str)] = &[(200, "Quick"), (400, "Balanced"), (600, "Precise")];
    let check_labels: Rc<RefCell<Vec<Label>>> = Rc::new(RefCell::new(Vec::new()));
    for &(count, name) in sample_options {
        let row = gtk4::ListBoxRow::new();
        let hbox = GtkBox::new(Orientation::Horizontal, 8);
        hbox.set_margin_start(12);
        hbox.set_margin_end(12);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);

        let current = sample_count.get();
        let check = Label::new(if count == current {
            Some("\u{2713}")
        } else {
            None
        });
        check.set_width_request(16);
        check_labels.borrow_mut().push(check.clone());

        let label = Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);

        hbox.append(&check);
        hbox.append(&label);
        row.set_child(Some(&hbox));
        sample_list.append(&row);
    }

    // Continuous toggle row (below sample counts, separated)
    let separator = gtk4::Separator::new(Orientation::Horizontal);
    separator.set_margin_top(4);
    separator.set_margin_bottom(4);
    sample_list.append(&separator);

    let continuous_row = gtk4::ListBoxRow::new();
    let continuous_hbox = GtkBox::new(Orientation::Horizontal, 8);
    continuous_hbox.set_margin_start(12);
    continuous_hbox.set_margin_end(12);
    continuous_hbox.set_margin_top(8);
    continuous_hbox.set_margin_bottom(8);

    let continuous_check = Label::new(
        if state.borrow().continuous_enabled() { Some("\u{2713}") } else { None }
    );
    continuous_check.set_width_request(16);

    let continuous_label = Label::new(Some("Continuous"));
    continuous_label.set_xalign(0.0);
    continuous_label.set_hexpand(true);

    continuous_hbox.append(&continuous_check);
    continuous_hbox.append(&continuous_label);
    continuous_row.set_child(Some(&continuous_hbox));
    sample_list.append(&continuous_row);
    let continuous_row_index = continuous_row.index();

    let is_tracking = Rc::new(Cell::new(false));

    let sample_count_for_list = Rc::clone(&sample_count);
    let sample_popover_for_list = sample_popover.clone();
    let check_labels_for_list = Rc::clone(&check_labels);
    let state_for_list = Rc::clone(&state);
    sample_list.connect_row_activated(move |_, row| {
        let index = row.index();
        let counts = [200u32, 400, 600];
        if let Some(&count) = counts.get(index as usize) {
            sample_count_for_list.set(count);
            state_for_list.borrow_mut().set_sample_count(count);
            let labels = check_labels_for_list.borrow();
            for (i, label) in labels.iter().enumerate() {
                label.set_text(if i == index as usize { "\u{2713}" } else { "" });
            }
        } else if index == continuous_row_index {
            let mut s = state_for_list.borrow_mut();
            let new_val = !s.continuous_enabled();
            s.set_continuous_enabled(new_val);
            continuous_check.set_text(if new_val { "\u{2713}" } else { "" });
        }
        sample_popover_for_list.popdown();
    });

    sample_popover.set_child(Some(&sample_list));
    calibrate_btn.set_popover(Some(&sample_popover));
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
            status_label.set_text("Waiting for Monado...");
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
                    if pct > 75 {
                        "battery-full-charging-symbolic"
                    } else if pct > 40 {
                        "battery-good-charging-symbolic"
                    } else if pct > 15 {
                        "battery-low-charging-symbolic"
                    } else {
                        "battery-caution-charging-symbolic"
                    }
                } else if pct > 75 {
                    "battery-full-symbolic"
                } else if pct > 40 {
                    "battery-good-symbolic"
                } else if pct > 15 {
                    "battery-low-symbolic"
                } else {
                    "battery-caution-symbolic"
                };

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
    update_ui_from_state(
        &state,
        &source_list,
        &target_list,
        &status_label,
        &battery_bar,
    );

    // Source selection changed
    let state_for_source = Rc::clone(&state);
    let target_list_for_source = Rc::clone(&target_list);
    source_list.connect_changed(move |device| {
        let unique_id = device.map(|d| d.unique_id().to_string());
        state_for_source.borrow_mut().set_source(unique_id);
        let s = state_for_source.borrow();
        target_list_for_source.set_devices(s.target_devices(), s.target_name());
    });

    // Target selection changed
    let state_for_target = Rc::clone(&state);
    let source_list_for_target = Rc::clone(&source_list);
    target_list.connect_changed(move |device| {
        let unique_id = device.map(|d| d.unique_id().to_string());
        state_for_target.borrow_mut().set_target(unique_id);
        let s = state_for_target.borrow();
        source_list_for_target.set_devices(s.source_devices(), s.source_name());
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

        glib::timeout_add_local_once(std::time::Duration::from_millis(interval_ms), move || {
            if state.borrow().is_connected() {
                // Already connected — just refresh batteries (cheap, reuses connection)
                state.borrow_mut().refresh_batteries();
                update_battery_bar(&battery_bar, &state.borrow());

                // If connection was lost during battery refresh, update full UI
                if !state.borrow().is_connected() {
                    update_ui_from_state(
                        &state,
                        &source_list,
                        &target_list,
                        &status_label,
                        &battery_bar,
                    );
                }
            } else {
                // Not connected — try to connect
                let changed = state.borrow_mut().refresh_connection();
                if changed {
                    update_ui_from_state(
                        &state,
                        &source_list,
                        &target_list,
                        &status_label,
                        &battery_bar,
                    );
                }
            }

            schedule_poll(state, source_list, target_list, status_label, battery_bar);
        });
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

    let toasts_for_calibrate = toasts.clone();
    let state_for_calibrate = Rc::clone(&state);
    let cmd_tx_calibrate = Rc::clone(&cmd_tx);
    let window_for_calibrate = window.clone();
    let sample_count_for_calibrate = Rc::clone(&sample_count);
    let is_tracking_for_calibrate = Rc::clone(&is_tracking);
    calibrate_btn.connect_clicked(move |btn| {
        if is_tracking_for_calibrate.get() {
            let _ = cmd_tx_calibrate.send(CalibrationCommand::StopContinuous);
            return;
        }

        let s = state_for_calibrate.borrow();
        let (Some(src), Some(tgt)) = (s.selected_source(), s.selected_target()) else {
            toasts_for_calibrate.show("Select both source and target devices");
            return;
        };

        // Get stage offset from Monado (like motoc does) to transform poses to common frame
        let stage_offset = s.connection().and_then(|conn| conn.get_stage_offset().ok());
        let hide_help = s.hide_calibration_help();
        let continuous = s.continuous_enabled();
        drop(s);

        let cmd = CalibrationCommand::StartSampled {
            source_serial: src.unique_id().to_string(),
            target_serial: tgt.unique_id().to_string(),
            target_origin_index: tgt.category_index,
            sample_count: sample_count_for_calibrate.get(),
            stage_offset,
            continuous,
        };

        if hide_help {
            if let Err(e) = cmd_tx_calibrate.send(cmd) {
                toasts_for_calibrate.show(&format!("Failed to start calibration: {}", e));
                return;
            }
            btn.set_label("Calibrating...");
            btn.set_sensitive(false);
        } else {
            let dialog = build_help_dialog(&state_for_calibrate, true);
            let cmd_tx = Rc::clone(&cmd_tx_calibrate);
            let btn = btn.clone();
            let toasts = toasts_for_calibrate.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "calibrate" {
                    if let Err(e) = cmd_tx.send(cmd.clone()) {
                        toasts.show(&format!("Failed to start calibration: {}", e));
                        return;
                    }
                    btn.set_label("Calibrating...");
                    btn.set_sensitive(false);
                }
            });
            dialog.present(Some(&window_for_calibrate));
        }
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
                    toasts_for_floor
                        .show("Select a target device first, then place it on the floor");
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

    let toasts_for_recenter = toasts.clone();
    let cmd_tx_recenter = Rc::clone(&cmd_tx);
    recenter_btn.connect_clicked(move |btn| {
        if let Err(e) = cmd_tx_recenter.send(CalibrationCommand::Recenter) {
            toasts_for_recenter.show(&format!("Failed to start recenter: {}", e));
            return;
        }
        btn.set_label("Recentering...");
        btn.set_sensitive(false);
    });

    // Help button — always shows the dialog regardless of toggle (info-only mode)
    let state_for_help = Rc::clone(&state);
    let window_for_help = window.clone();
    help_btn.connect_clicked(move |_| {
        let dialog = build_help_dialog(&state_for_help, false);
        dialog.present(Some(&window_for_help));
    });

    // Message handler for calibration results and movement updates
    let calibrate_btn_for_msg = calibrate_btn.clone();
    let floor_btn_for_msg = floor_btn.clone();
    let recenter_btn_for_msg = recenter_btn.clone();
    let toasts_for_msg = toasts.clone();
    let state_for_msg = Rc::clone(&state);
    let source_list_for_msg = Rc::clone(&source_list);
    let is_tracking_for_msg = Rc::clone(&is_tracking);
    let target_list_for_msg = Rc::clone(&target_list);
    let background_box_for_msg = background_box.clone();
    let countdown_label_for_msg = countdown_label.clone();
    let progress_fill_for_msg = progress_fill.clone();
    let window_for_msg = window.clone();

    let continuous_baseline: Rc<RefCell<Option<crate::calibration::TransformD>>> = Rc::new(RefCell::new(None));
    let baseline_for_msg = Rc::clone(&continuous_baseline);

    glib::source::idle_add_local(move || {
        while let Ok(msg) = msg_rx.try_recv() {
            match msg {
                CalibrationMessage::Countdown { seconds } => {
                    // Dismiss previous result toast when new calibration starts
                    if let Some(prev) = toasts_for_msg.current_toast.borrow_mut().take() {
                        prev.dismiss();
                    }
                    calibrate_btn_for_msg.set_label(&format!("{}...", seconds));
                    background_box_for_msg.add_css_class("calibration-bg-active");
                    countdown_label_for_msg.set_text(&format!("{}", seconds));
                    countdown_label_for_msg.set_visible(true);
                    play_sound("message");
                }
                CalibrationMessage::RecenterCountdown { seconds } => {
                    recenter_btn_for_msg.set_label(&format!("{}...", seconds));
                    background_box_for_msg.add_css_class("calibration-bg-active");
                    countdown_label_for_msg.set_text(&format!("{}", seconds));
                    countdown_label_for_msg.set_visible(true);
                    play_sound("message");
                }
                CalibrationMessage::Progress { collected, total } => {
                    calibrate_btn_for_msg.set_label(&format!("{}/{}...", collected, total));
                    countdown_label_for_msg.set_visible(false);
                    progress_fill_for_msg.set_visible(true);
                    progress_fill_for_msg.add_css_class("progress-fill-active");
                    let fraction = collected as f64 / total as f64;
                    update_progress_fill(&progress_fill_for_msg, &window_for_msg, fraction);
                }
                CalibrationMessage::FloorProgress { collected, total } => {
                    floor_btn_for_msg.set_label(&format!("{}/{}...", collected, total));
                    background_box_for_msg.add_css_class("calibration-bg-active");
                    countdown_label_for_msg.set_visible(false);
                    progress_fill_for_msg.set_visible(true);
                    progress_fill_for_msg.add_css_class("progress-fill-active");
                    let fraction = collected as f64 / total as f64;
                    update_progress_fill(&progress_fill_for_msg, &window_for_msg, fraction);
                }
                CalibrationMessage::SampledComplete(result) => {
                    calibrate_btn_for_msg.set_label("Calibrate");
                    calibrate_btn_for_msg.set_sensitive(true);
                    restore_idle_visuals(
                        &background_box_for_msg,
                        &countdown_label_for_msg,
                        &progress_fill_for_msg,
                    );
                    play_sound("complete");

                    // Apply the calibration offset to the TARGET tracking origin
                    let s = state_for_msg.borrow();
                    if let Some(conn) = s.connection() {
                        let position = result.transform.position_f64();
                        let orientation = result.transform.orientation_f64();
                        match conn.apply_offset(result.target_origin_index, position, orientation) {
                            Ok(full_offset) => {
                                *baseline_for_msg.borrow_mut() = Some(full_offset);

                                let d = result.median_error_degrees as f64;
                                let confidence = (100.0 * (-0.028 * d.powf(1.6)).exp()) as u32;
                                toasts_for_msg
                                    .show_calibration_result(confidence, result.axis_diversity);
                            }
                            Err(e) => {
                                toasts_for_msg.show(&format!("Failed to apply calibration: {}", e))
                            }
                        }
                    } else {
                        toasts_for_msg.show("Cannot apply calibration: not connected");
                    }
                }
                CalibrationMessage::FloorComplete { height_adjustment } => {
                    floor_btn_for_msg.set_label("Floor");
                    floor_btn_for_msg.set_sensitive(true);
                    restore_idle_visuals(
                        &background_box_for_msg,
                        &countdown_label_for_msg,
                        &progress_fill_for_msg,
                    );
                    play_sound("complete");

                    // Floor calibration:
                    // height_adjustment = -(measured_floor_in_stage), so measured = -height_adjustment
                    // set_floor_absolute converts to native coords internally
                    let measured_floor = -height_adjustment as f64;

                    let apply_result = {
                        let s = state_for_msg.borrow();
                        s.connection()
                            .map(|conn| conn.set_floor_absolute(measured_floor))
                    };

                    match apply_result {
                        Some(Ok(_)) => {
                            let delta_cm = measured_floor * 100.0;
                            let direction = if measured_floor >= 0.0 { "up" } else { "down" };
                            toasts_for_msg.show(&format!(
                                "Floor set {:.1}cm {}",
                                delta_cm.abs(),
                                direction
                            ));
                        }
                        Some(Err(e)) => {
                            toasts_for_msg.show(&format!("Failed to apply floor: {}", e))
                        }
                        None => toasts_for_msg.show("Cannot apply floor: not connected"),
                    }
                }
                CalibrationMessage::RecenterComplete {
                    position,
                    orientation,
                } => {
                    recenter_btn_for_msg.set_label("Recenter");
                    recenter_btn_for_msg.set_sensitive(true);
                    restore_idle_visuals(
                        &background_box_for_msg,
                        &countdown_label_for_msg,
                        &progress_fill_for_msg,
                    );
                    play_sound("complete");

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
                                let parts: Vec<&str> =
                                    [x_str.as_str(), z_str.as_str(), yaw_str.as_str()]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect();

                                if parts.is_empty() {
                                    toasts_for_msg.show("Centered");
                                } else {
                                    toasts_for_msg
                                        .show(&format!("Centered ({})", parts.join(", ")));
                                }
                            }
                            Err(e) => {
                                toasts_for_msg.show(&format!("Failed to apply recenter: {}", e))
                            }
                        }
                    } else {
                        toasts_for_msg.show("Cannot apply recenter: not connected");
                    }
                }
                CalibrationMessage::MovementUpdate { movements } => {
                    state_for_msg
                        .borrow_mut()
                        .set_movement_intensities(movements);
                    update_movement_only(
                        &state_for_msg,
                        &source_list_for_msg,
                        &target_list_for_msg,
                    );
                }
                CalibrationMessage::ContinuousStarted => {
                    is_tracking_for_msg.set(true);
                    calibrate_btn_for_msg.set_label("Tracking");
                    calibrate_btn_for_msg.set_sensitive(true);
                }
                CalibrationMessage::ContinuousStopped => {
                    is_tracking_for_msg.set(false);
                    calibrate_btn_for_msg.set_label("Calibrate");
                    calibrate_btn_for_msg.set_sensitive(true);
                }
                CalibrationMessage::ContinuousCorrection { target_origin_index, delta } => {
                    if let Some(ref mut origin) = *baseline_for_msg.borrow_mut() {
                        crate::calibration::continuous::apply_correction(origin, &delta);
                        let s = state_for_msg.borrow();
                        if let Some(conn) = s.connection() {
                            let _ = conn.set_offset_absolute(target_origin_index, origin);
                        }
                    }
                }
                CalibrationMessage::Error(e) => {
                    is_tracking_for_msg.set(false);
                    calibrate_btn_for_msg.set_label("Calibrate");
                    calibrate_btn_for_msg.set_sensitive(true);
                    floor_btn_for_msg.set_label("Floor");
                    floor_btn_for_msg.set_sensitive(true);
                    recenter_btn_for_msg.set_label("Recenter");
                    recenter_btn_for_msg.set_sensitive(true);
                    restore_idle_visuals(
                        &background_box_for_msg,
                        &countdown_label_for_msg,
                        &progress_fill_for_msg,
                    );
                    play_sound("complete");
                    toasts_for_msg.show(&format!("Error: {}", e));
                }
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
