//! Centralized application state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::calibration::DeviceMovement;
use crate::config::Config;
use crate::monado::{self, MonadoConnection};
use crate::ui::device_selector::{Category, Device};

type StateListener = Box<dyn Fn(&AppState)>;

/// Battery snapshot for the status bar (e.g. "Tracker 790B7B56 85%").
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub short_name: String,
    pub serial_suffix: String,
    pub charge: Option<f32>,
    pub charging: bool,
    pub online: bool,
}


fn shorten_device_name(name: &str) -> String {
    let lower = name.to_lowercase();

    // Knuckles controllers
    if lower.contains("knuckles") || lower.contains("index") {
        if lower.contains("left") {
            return "Knuckles L".to_string();
        } else if lower.contains("right") {
            return "Knuckles R".to_string();
        }
        return "Knuckles".to_string();
    }

    // Generic controllers
    if lower.contains("controller") {
        if lower.contains("left") {
            return "Controller L".to_string();
        } else if lower.contains("right") {
            return "Controller R".to_string();
        }
        return "Controller".to_string();
    }

    // Trackers — just "Tracker" (serial suffix distinguishes them)
    if lower.contains("tracker") {
        return "Tracker".to_string();
    }

    // HMDs
    if lower.contains("hmd") || lower.contains("headset") {
        return "HMD".to_string();
    }

    // Gamepad
    if lower.contains("gamepad") {
        return "Gamepad".to_string();
    }

    // Fallback: first two words
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() <= 2 {
        name.to_string()
    } else {
        words[..2].join(" ")
    }
}

pub struct AppState {
    connection: Option<MonadoConnection>,
    categories: Vec<Category>,
    source_id: Option<String>,
    target_id: Option<String>,
    movement_intensities: HashMap<String, f32>,
    /// Tracks battery devices across disconnects to show offline status.
    known_battery_devices: HashMap<String, BatteryInfo>,
    hide_calibration_help: bool,
    sample_count: u32,
    continuous_enabled: bool,
    listeners: Vec<StateListener>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Config::load();
        let connection = monado::try_connect();
        let categories = if connection.is_some() {
            monado::enumerate_devices()
        } else {
            Vec::new()
        };

        let mut state = Self {
            connection,
            categories,
            source_id: if config.source.is_empty() { None } else { Some(config.source) },
            target_id: if config.target.is_empty() { None } else { Some(config.target) },
            movement_intensities: HashMap::new(),
            known_battery_devices: HashMap::new(),
            hide_calibration_help: config.hide_calibration_help,
            sample_count: config.sample_count,
            continuous_enabled: config.continuous_enabled,
            listeners: Vec::new(),
        };
        state.update_known_battery_devices();
        state
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    pub fn connection(&self) -> Option<&MonadoConnection> {
        self.connection.as_ref()
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    pub fn all_devices(&self) -> Vec<Device> {
        self.categories.iter()
            .flat_map(|c| c.devices.clone())
            .collect()
    }

    pub fn source_devices(&self) -> Vec<Device> {
        let exclude_category = self.selected_target().map(|d| d.category.clone());
        self.categories.iter()
            .flat_map(|c| c.devices.clone())
            .filter(|d| {
                if let Some(ref exc) = exclude_category {
                    &d.category != exc
                } else {
                    true
                }
            })
            .collect()
    }

    pub fn target_devices(&self) -> Vec<Device> {
        let exclude_category = self.selected_source().map(|d| d.category.clone());
        self.categories.iter()
            .flat_map(|c| c.devices.clone())
            .filter(|d| {
                if let Some(ref exc) = exclude_category {
                    &d.category != exc
                } else {
                    true
                }
            })
            .collect()
    }

    pub fn selected_source(&self) -> Option<Device> {
        self.source_id.as_ref().and_then(|id| {
            self.all_devices().into_iter().find(|d| d.unique_id() == id)
        })
    }

    pub fn selected_target(&self) -> Option<Device> {
        self.target_id.as_ref().and_then(|id| {
            self.all_devices().into_iter().find(|d| d.unique_id() == id)
        })
    }

    pub fn source_name(&self) -> Option<&str> {
        self.source_id.as_deref()
    }

    pub fn target_name(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    pub fn set_source(&mut self, id: Option<String>) {
        if self.source_id != id {
            self.source_id = id;
            self.save_config();
            self.notify_listeners();
        }
    }

    pub fn set_target(&mut self, id: Option<String>) {
        if self.target_id != id {
            self.target_id = id;
            self.save_config();
            self.notify_listeners();
        }
    }

    /// Only attempts when disconnected — avoids IPC churn. Returns true if state changed.
    pub fn refresh_connection(&mut self) -> bool {
        let was_connected = self.is_connected();

        if !was_connected {
            // Only try to connect when not already connected
            self.connection = monado::try_connect();
        }

        let is_connected = self.is_connected();

        if is_connected && !was_connected {
            // Just connected — do full enumeration using our stored connection
            if let Some(ref conn) = self.connection {
                self.categories = conn.enumerate_devices().unwrap_or_default();
            }
            self.update_known_battery_devices();
        } else if !is_connected {
            self.categories.clear();
        }

        let changed = was_connected != is_connected;
        if changed {
            self.notify_listeners();
        }
        changed
    }

    pub fn force_refresh(&mut self) {
        self.connection = monado::try_connect();
        if let Some(ref conn) = self.connection {
            self.categories = conn.enumerate_devices().unwrap_or_default();
        } else {
            self.categories.clear();
        }
        self.update_known_battery_devices();
        self.notify_listeners();
    }

    pub fn refresh_batteries(&mut self) {
        if let Some(ref conn) = self.connection
            && conn.refresh_batteries(&mut self.categories).is_err()
        {
            // Connection lost — clear it so next refresh_connection will reconnect
            self.connection = None;
            self.categories.clear();
        }
        self.update_known_battery_devices();
    }

    fn update_known_battery_devices(&mut self) {
        // Collect current device IDs that have batteries
        let mut current_ids = std::collections::HashSet::new();

        for cat in &self.categories {
            for dev in &cat.devices {
                if let Some(charge) = dev.battery_charge {
                    let id = dev.unique_id().to_string();
                    current_ids.insert(id.clone());
                    let serial_suffix = if !dev.serial.is_empty() && dev.serial != dev.name {
                        if dev.serial.len() > 8 {
                            dev.serial[dev.serial.len() - 8..].to_string()
                        } else {
                            dev.serial.clone()
                        }
                    } else {
                        String::new()
                    };
                    self.known_battery_devices.insert(id, BatteryInfo {
                        short_name: shorten_device_name(&dev.name),
                        serial_suffix,
                        charge: Some(charge),
                        charging: dev.battery_charging,
                        online: true,
                    });
                }
            }
        }

        // Mark previously known devices as offline if they disappeared
        for (id, info) in self.known_battery_devices.iter_mut() {
            if !current_ids.contains(id) {
                info.online = false;
                info.charge = None;
                info.charging = false;
            }
        }
    }

    pub fn battery_status_list(&self) -> Vec<&BatteryInfo> {
        let mut list: Vec<&BatteryInfo> = self.known_battery_devices.values().collect();
        list.sort_by(|a, b| a.short_name.cmp(&b.short_name).then(a.serial_suffix.cmp(&b.serial_suffix)));
        list
    }

    pub fn set_movement_intensities(&mut self, movements: Vec<DeviceMovement>) {
        self.movement_intensities.clear();
        for m in movements {
            self.movement_intensities.insert(m.device_id, m.intensity);
        }
        self.notify_listeners();
    }

    pub fn movement_intensities(&self) -> &HashMap<String, f32> {
        &self.movement_intensities
    }

    pub fn hide_calibration_help(&self) -> bool {
        self.hide_calibration_help
    }

    pub fn set_hide_calibration_help(&mut self, hide: bool) {
        if self.hide_calibration_help != hide {
            self.hide_calibration_help = hide;
            self.save_config();
        }
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    pub fn set_sample_count(&mut self, count: u32) {
        if self.sample_count != count {
            self.sample_count = count;
            self.save_config();
        }
    }

    pub fn continuous_enabled(&self) -> bool {
        self.continuous_enabled
    }

    pub fn set_continuous_enabled(&mut self, enabled: bool) {
        if self.continuous_enabled != enabled {
            self.continuous_enabled = enabled;
            self.save_config();
        }
    }

    fn save_config(&self) {
        let config = Config {
            source: self.source_id.clone().unwrap_or_default(),
            target: self.target_id.clone().unwrap_or_default(),
            hide_calibration_help: self.hide_calibration_help,
            sample_count: self.sample_count,
            continuous_enabled: self.continuous_enabled,
        };
        if let Err(e) = config.save() {
            eprintln!("Warning: Failed to save config: {}", e);
        }
    }

    fn notify_listeners(&self) {
        for listener in &self.listeners {
            listener(self);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedState = Rc<RefCell<AppState>>;

pub fn create_shared_state() -> SharedState {
    Rc::new(RefCell::new(AppState::new()))
}
