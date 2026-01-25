use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib::{timeout_add_local, ControlFlow};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DropDown, Label, Orientation, Separator, StringList};
use libadwaita as adw;
use libadwaita::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Device {
    index: u32,
    name: String,
    description: String,
    category: String,
    category_index: u32,
}

impl Device {
    fn display_name(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.description)
        }
    }
}

#[derive(Debug, Clone)]
struct Category {
    index: u32,
    name: String,
    devices: Vec<Device>,
}

fn parse_motoc_show(output: &str) -> Vec<Category> {
    let category_re = Regex::new(r"^\[(\d+)\]\s+(.+)$").unwrap();
    let device_re = Regex::new(r#"^[│├└].*\[(\d+)\]\s+"([^"]+)"(?:\s+\((.+)\))?"#).unwrap();

    let mut categories: Vec<Category> = Vec::new();

    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(caps) = category_re.captures(line) {
            categories.push(Category {
                index: caps[1].parse().unwrap_or(0),
                name: caps[2].trim().to_string(),
                devices: Vec::new(),
            });
        } else if let Some(caps) = device_re.captures(line) {
            if let Some(cat) = categories.last_mut() {
                cat.devices.push(Device {
                    index: caps[1].parse().unwrap_or(0),
                    name: caps[2].to_string(),
                    description: caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default(),
                    category: cat.name.clone(),
                    category_index: cat.index,
                });
            }
        }
    }

    categories
}

fn run_motoc_show() -> Vec<Category> {
    match Command::new("motoc").arg("show").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_motoc_show(&stdout)
        }
        Err(_) => Vec::new(),
    }
}

fn run_motoc_command(args: &[&str]) -> Result<String, String> {
    match Command::new("motoc").args(args).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                Ok(stdout.to_string())
            } else {
                Err(format!("{}{}", stdout, stderr))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Config {
    source: String,
    target: String,
}

impl Config {
    fn path() -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("motoc-gui")
            .join("config.json")
    }

    fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

struct DeviceSelector {
    container: GtkBox,
    dropdown: DropDown,
    string_list: StringList,
    all_devices: RefCell<Vec<Device>>,
    filtered_devices: RefCell<Vec<Device>>,
    excluded_category: RefCell<Option<String>>,
    selected_device: RefCell<Option<Device>>,
}

impl DeviceSelector {
    fn new(label_text: &str) -> Rc<Self> {
        let container = GtkBox::new(Orientation::Vertical, 4);

        let label = Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.add_css_class("heading");
        container.append(&label);

        let string_list = StringList::new(&[]);
        let dropdown = DropDown::new(Some(string_list.clone()), gtk4::Expression::NONE);
        dropdown.set_hexpand(true);
        dropdown.set_enable_search(true);
        container.append(&dropdown);

        Rc::new(Self {
            container,
            dropdown,
            string_list,
            all_devices: RefCell::new(Vec::new()),
            filtered_devices: RefCell::new(Vec::new()),
            excluded_category: RefCell::new(None),
            selected_device: RefCell::new(None),
        })
    }

    fn widget(&self) -> &GtkBox {
        &self.container
    }

    fn set_devices(&self, devices: Vec<Device>) {
        *self.all_devices.borrow_mut() = devices;
        self.rebuild_list();
    }

    fn set_excluded_category(&self, category: Option<String>) {
        let current = self.excluded_category.borrow().clone();
        if current == category {
            return;
        }
        *self.excluded_category.borrow_mut() = category;
        self.rebuild_list();
    }

    fn rebuild_list(&self) {
        let excluded = self.excluded_category.borrow().clone();
        let all = self.all_devices.borrow();

        let mut filtered: Vec<Device> = if let Some(ref exc) = excluded {
            all.iter().filter(|d| &d.category != exc).cloned().collect()
        } else {
            all.clone()
        };

        filtered.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
        *self.filtered_devices.borrow_mut() = filtered.clone();

        while self.string_list.n_items() > 0 {
            self.string_list.remove(0);
        }

        self.string_list.append("(None)");

        let mut current_category = String::new();
        for device in &filtered {
            if device.category != current_category {
                current_category = device.category.clone();
                self.string_list.append(&format!("── {} ──", current_category));
            }
            self.string_list.append(&format!("    {}", device.display_name()));
        }

        let selected = self.selected_device.borrow();
        if let Some(ref sel) = *selected {
            let mut idx = 1u32;
            let mut current_cat = String::new();
            for device in &filtered {
                if device.category != current_cat {
                    current_cat = device.category.clone();
                    idx += 1;
                }
                if device.name == sel.name {
                    self.dropdown.set_selected(idx);
                    return;
                }
                idx += 1;
            }
        }
        self.dropdown.set_selected(0);
    }

    fn get_selected(&self) -> Option<Device> {
        self.selected_device.borrow().clone()
    }

    fn select_by_name(&self, name: &str) {
        let filtered = self.filtered_devices.borrow();
        for device in filtered.iter() {
            if device.name == name {
                *self.selected_device.borrow_mut() = Some(device.clone());
                break;
            }
        }
        drop(filtered);
        self.rebuild_list();
    }

    fn connect_changed<F: Fn(Option<Device>) + 'static>(self: &Rc<Self>, callback: F) {
        let this = Rc::clone(self);
        self.dropdown.connect_selected_notify(move |dropdown| {
            let idx = dropdown.selected();
            let filtered = this.filtered_devices.borrow();

            if idx == 0 {
                *this.selected_device.borrow_mut() = None;
                drop(filtered);
                callback(None);
                return;
            }

            let mut current_cat = String::new();
            let mut item_idx = 1u32;
            let mut found_device: Option<Device> = None;

            for device in filtered.iter() {
                if device.category != current_cat {
                    current_cat = device.category.clone();
                    item_idx += 1;
                }
                if item_idx == idx {
                    found_device = Some(device.clone());
                    break;
                }
                item_idx += 1;
            }

            drop(filtered);

            if let Some(device) = found_device {
                *this.selected_device.borrow_mut() = Some(device.clone());
                callback(Some(device));
            } else {
                *this.selected_device.borrow_mut() = None;
                callback(None);
            }
        });
    }
}

fn build_ui(app: &adw::Application) {
    let config = Rc::new(RefCell::new(Config::load()));
    let countdown = Rc::new(Cell::new(0i32));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Motoc Calibration")
        .default_width(420)
        .default_height(340)
        .build();

    let toast_overlay = adw::ToastOverlay::new();
    window.set_content(Some(&toast_overlay));

    let main_box = GtkBox::new(Orientation::Vertical, 16);
    main_box.set_margin_top(16);
    main_box.set_margin_bottom(16);
    main_box.set_margin_start(16);
    main_box.set_margin_end(16);
    toast_overlay.set_child(Some(&main_box));

    let header = Label::new(Some("Tracking Origin Calibration"));
    header.add_css_class("title-2");
    main_box.append(&header);

    let dropdowns_box = GtkBox::new(Orientation::Vertical, 12);
    main_box.append(&dropdowns_box);

    let source_selector = DeviceSelector::new("Source");
    dropdowns_box.append(source_selector.widget());

    let target_selector = DeviceSelector::new("Target");
    dropdowns_box.append(target_selector.widget());

    let separator = Separator::new(Orientation::Horizontal);
    separator.set_margin_top(8);
    separator.set_margin_bottom(8);
    main_box.append(&separator);

    let button_grid = gtk4::Grid::new();
    button_grid.set_row_spacing(8);
    button_grid.set_column_spacing(8);
    button_grid.set_halign(Align::Center);
    main_box.append(&button_grid);

    let calibrate_btn = Button::with_label("CALIBRATE");
    calibrate_btn.add_css_class("suggested-action");
    calibrate_btn.add_css_class("pill");
    calibrate_btn.set_width_request(140);
    button_grid.attach(&calibrate_btn, 0, 0, 1, 1);

    let floor_btn = Button::with_label("FLOOR");
    floor_btn.set_width_request(140);
    button_grid.attach(&floor_btn, 1, 0, 1, 1);

    let reset_btn = Button::with_label("RESET");
    reset_btn.add_css_class("destructive-action");
    reset_btn.set_width_request(140);
    button_grid.attach(&reset_btn, 0, 1, 1, 1);

    let recenter_btn = Button::with_label("RECENTER");
    recenter_btn.set_width_request(140);
    button_grid.attach(&recenter_btn, 1, 1, 1, 1);

    let refresh_btn = Button::with_label("Refresh Devices");
    refresh_btn.set_margin_top(8);
    main_box.append(&refresh_btn);

    let categories = run_motoc_show();
    let all_devices: Vec<Device> = categories.into_iter().flat_map(|c| c.devices).collect();

    source_selector.set_devices(all_devices.clone());
    target_selector.set_devices(all_devices.clone());

    {
        let cfg = config.borrow();
        if !cfg.source.is_empty() {
            source_selector.select_by_name(&cfg.source);
        }
        if !cfg.target.is_empty() {
            target_selector.select_by_name(&cfg.target);
        }
    }

    if let Some(dev) = source_selector.get_selected() {
        target_selector.set_excluded_category(Some(dev.category));
    }
    if let Some(dev) = target_selector.get_selected() {
        source_selector.set_excluded_category(Some(dev.category));
    }

    let target_for_source = Rc::clone(&target_selector);
    let config_for_source = Rc::clone(&config);
    source_selector.connect_changed(move |device| {
        if let Some(dev) = &device {
            config_for_source.borrow_mut().source = dev.name.clone();
            target_for_source.set_excluded_category(Some(dev.category.clone()));
        } else {
            config_for_source.borrow_mut().source.clear();
            target_for_source.set_excluded_category(None);
        }
        config_for_source.borrow().save();
    });

    let source_for_target = Rc::clone(&source_selector);
    let config_for_target = Rc::clone(&config);
    target_selector.connect_changed(move |device| {
        if let Some(dev) = &device {
            config_for_target.borrow_mut().target = dev.name.clone();
            source_for_target.set_excluded_category(Some(dev.category.clone()));
        } else {
            config_for_target.borrow_mut().target.clear();
            source_for_target.set_excluded_category(None);
        }
        config_for_target.borrow().save();
    });

    let toast_for_calibrate = toast_overlay.clone();
    let source_for_calibrate = Rc::clone(&source_selector);
    let target_for_calibrate = Rc::clone(&target_selector);
    let countdown_for_calibrate = Rc::clone(&countdown);
    let floor_btn_for_calibrate = floor_btn.clone();
    let reset_btn_for_calibrate = reset_btn.clone();
    let recenter_btn_for_calibrate = recenter_btn.clone();

    calibrate_btn.connect_clicked(move |btn| {
        let src = source_for_calibrate.get_selected();
        let tgt = target_for_calibrate.get_selected();

        if src.is_none() || tgt.is_none() {
            let toast = adw::Toast::new("Select both source and target devices");
            toast_for_calibrate.add_toast(toast);
            return;
        }

        if countdown_for_calibrate.get() > 0 {
            return;
        }

        countdown_for_calibrate.set(5);
        btn.set_label("5...");
        floor_btn_for_calibrate.set_sensitive(false);
        reset_btn_for_calibrate.set_sensitive(false);
        recenter_btn_for_calibrate.set_sensitive(false);

        let countdown = Rc::clone(&countdown_for_calibrate);
        let btn = btn.clone();
        let toast_overlay = toast_for_calibrate.clone();
        let src = src.unwrap();
        let tgt = tgt.unwrap();
        let floor_btn = floor_btn_for_calibrate.clone();
        let reset_btn = reset_btn_for_calibrate.clone();
        let recenter_btn = recenter_btn_for_calibrate.clone();

        timeout_add_local(Duration::from_secs(1), move || {
            let remaining = countdown.get() - 1;
            countdown.set(remaining);

            if remaining > 0 {
                btn.set_label(&format!("{}...", remaining));
                ControlFlow::Continue
            } else {
                btn.set_label("Calibrating...");

                match run_motoc_command(&["calibrate", "--src", &src.name, "--dst", &tgt.name]) {
                    Ok(_) => {
                        let toast = adw::Toast::new("Calibration complete");
                        toast_overlay.add_toast(toast);
                    }
                    Err(e) => {
                        let msg = format!("Calibration failed: {}", e.chars().take(50).collect::<String>());
                        let toast = adw::Toast::new(&msg);
                        toast_overlay.add_toast(toast);
                    }
                }

                btn.set_label("CALIBRATE");
                floor_btn.set_sensitive(true);
                reset_btn.set_sensitive(true);
                recenter_btn.set_sensitive(true);
                ControlFlow::Break
            }
        });
    });

    let toast_for_floor = toast_overlay.clone();
    floor_btn.connect_clicked(move |_| {
        match run_motoc_command(&["floor"]) {
            Ok(_) => {
                let toast = adw::Toast::new("Floor level adjusted");
                toast_for_floor.add_toast(toast);
            }
            Err(e) => {
                let msg = format!("Floor failed: {}", e.chars().take(50).collect::<String>());
                let toast = adw::Toast::new(&msg);
                toast_for_floor.add_toast(toast);
            }
        }
    });

    let toast_for_reset = toast_overlay.clone();
    let target_for_reset = Rc::clone(&target_selector);
    reset_btn.connect_clicked(move |_| {
        let tgt = target_for_reset.get_selected();
        if let Some(device) = tgt {
            match run_motoc_command(&["reset", &device.category_index.to_string()]) {
                Ok(_) => {
                    let toast = adw::Toast::new(&format!("Reset {}", device.category));
                    toast_for_reset.add_toast(toast);
                }
                Err(e) => {
                    let msg = format!("Reset failed: {}", e.chars().take(50).collect::<String>());
                    let toast = adw::Toast::new(&msg);
                    toast_for_reset.add_toast(toast);
                }
            }
        } else {
            let toast = adw::Toast::new("Select a target device first");
            toast_for_reset.add_toast(toast);
        }
    });

    let toast_for_recenter = toast_overlay.clone();
    recenter_btn.connect_clicked(move |_| {
        match run_motoc_command(&["recenter"]) {
            Ok(_) => {
                let toast = adw::Toast::new("Recentered");
                toast_for_recenter.add_toast(toast);
            }
            Err(e) => {
                let msg = format!("Recenter failed: {}", e.chars().take(50).collect::<String>());
                let toast = adw::Toast::new(&msg);
                toast_for_recenter.add_toast(toast);
            }
        }
    });

    let source_for_refresh = Rc::clone(&source_selector);
    let target_for_refresh = Rc::clone(&target_selector);
    refresh_btn.connect_clicked(move |_| {
        let categories = run_motoc_show();
        let all_devices: Vec<Device> = categories.into_iter().flat_map(|c| c.devices).collect();
        source_for_refresh.set_devices(all_devices.clone());
        target_for_refresh.set_devices(all_devices);
    });

    window.present();
}

fn ensure_desktop_entry() {
    let Some(data_dir) = dirs::data_dir() else { return };
    let desktop_dir = data_dir.join("applications");
    let desktop_file = desktop_dir.join("motoc-gui.desktop");

    if desktop_file.exists() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else { return };
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Motoc Calibration\n\
         Comment=VR Tracking Origin Calibration Tool\n\
         Exec={}\n\
         Icon=preferences-desktop-display\n\
         Categories=Utility;\n\
         Terminal=false\n",
        exe.display()
    );

    let _ = std::fs::create_dir_all(&desktop_dir);
    let _ = std::fs::write(&desktop_file, content);
}

fn main() {
    ensure_desktop_entry();

    let app = adw::Application::builder()
        .application_id("io.github.galister.motoc-gui")
        .build();

    app.connect_activate(build_ui);
    app.run();
}
