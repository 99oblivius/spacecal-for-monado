//! Centralized application state management
//!
//! This module provides a single source of truth for the application state,
//! with unidirectional data flow and derived state computation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::calibration::DeviceMovement;
use crate::config::Config;
use crate::monado::{self, MonadoConnection};
use crate::ui::device_selector::{Category, Device};

/// Type alias for state change listeners
type StateListener = Box<dyn Fn(&AppState)>;

/// Snapshot of a device's battery state for the status bar
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    /// Short device name for compact display (e.g. "Tracker", "Knuckles L")
    pub short_name: String,
    /// Serial suffix for disambiguation (e.g. "790B7B56"), empty if not needed
    pub serial_suffix: String,
    /// Battery charge (0.0-1.0), None if offline
    pub charge: Option<f32>,
    /// Whether currently charging
    pub charging: bool,
    /// Whether this device is currently online
    pub online: bool,
}

/// Abbreviate a full device name for compact battery display
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

/// Central application state - single source of truth
pub struct AppState {
    /// Monado connection (if connected)
    connection: Option<MonadoConnection>,

    /// All available devices grouped by category
    categories: Vec<Category>,

    /// Selected source device unique ID (serial or name fallback, persisted)
    source_id: Option<String>,

    /// Selected target device unique ID (serial or name fallback, persisted)
    target_id: Option<String>,

    /// Device movement intensities (device unique_id -> intensity 0.0-1.0)
    movement_intensities: HashMap<String, f32>,

    /// Previously seen battery devices (unique_id -> last known info)
    /// Used to show offline status when devices disconnect
    known_battery_devices: HashMap<String, BatteryInfo>,

    /// Whether to skip showing the calibration help dialog
    hide_calibration_help: bool,

    /// Number of samples for calibration (200/400/600)
    sample_count: u32,

    /// Listeners to notify on state change
    listeners: Vec<StateListener>,
}

impl AppState {
    /// Create new state, loading persisted config
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
            listeners: Vec::new(),
        };
        state.update_known_battery_devices();
        state
    }

    /// Check if connected to Monado
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    /// Get the Monado connection (if any)
    pub fn connection(&self) -> Option<&MonadoConnection> {
        self.connection.as_ref()
    }

    /// Get all tracking origin categories
    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    /// Get all devices as a flat list
    pub fn all_devices(&self) -> Vec<Device> {
        self.categories.iter()
            .flat_map(|c| c.devices.clone())
            .collect()
    }

    /// Get devices available for source selection (excludes target's category)
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

    /// Get devices available for target selection (excludes source's category)
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

    /// Get the selected source device (if any and still valid)
    pub fn selected_source(&self) -> Option<Device> {
        self.source_id.as_ref().and_then(|id| {
            self.all_devices().into_iter().find(|d| d.unique_id() == id)
        })
    }

    /// Get the selected target device (if any and still valid)
    pub fn selected_target(&self) -> Option<Device> {
        self.target_id.as_ref().and_then(|id| {
            self.all_devices().into_iter().find(|d| d.unique_id() == id)
        })
    }

    /// Get selected source unique ID (even if device not currently available)
    pub fn source_name(&self) -> Option<&str> {
        self.source_id.as_deref()
    }

    /// Get selected target unique ID (even if device not currently available)
    pub fn target_name(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    /// Set the source selection by unique ID
    pub fn set_source(&mut self, id: Option<String>) {
        if self.source_id != id {
            self.source_id = id;
            self.save_config();
            self.notify_listeners();
        }
    }

    /// Set the target selection by unique ID
    pub fn set_target(&mut self, id: Option<String>) {
        if self.target_id != id {
            self.target_id = id;
            self.save_config();
            self.notify_listeners();
        }
    }

    /// Try to connect to Monado if not already connected, and refresh devices.
    /// Only attempts connection when disconnected — avoids IPC churn.
    /// Returns true if connection state changed.
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

    /// Force re-enumerate devices (reconnects to Monado)
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

    /// Refresh battery status for all devices (lightweight, no re-enumeration).
    /// If the connection is lost during refresh, marks it as disconnected.
    pub fn refresh_batteries(&mut self) {
        if let Some(ref conn) = self.connection {
            if conn.refresh_batteries(&mut self.categories).is_err() {
                // Connection lost — clear it so next refresh_connection will reconnect
                self.connection = None;
                self.categories.clear();
            }
        }
        self.update_known_battery_devices();
    }

    /// Update the known battery devices map from current categories
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

    /// Get all known battery devices (online + offline), sorted by name then serial
    pub fn battery_status_list(&self) -> Vec<&BatteryInfo> {
        let mut list: Vec<&BatteryInfo> = self.known_battery_devices.values().collect();
        list.sort_by(|a, b| a.short_name.cmp(&b.short_name).then(a.serial_suffix.cmp(&b.serial_suffix)));
        list
    }

    /// Update device movement intensities
    pub fn set_movement_intensities(&mut self, movements: Vec<DeviceMovement>) {
        self.movement_intensities.clear();
        for m in movements {
            self.movement_intensities.insert(m.device_id, m.intensity);
        }
        self.notify_listeners();
    }

    /// Get movement intensity for a device by unique ID (0.0 = still, 1.0 = moving)
    #[allow(dead_code)]
    pub fn movement_intensity(&self, device_id: &str) -> f32 {
        self.movement_intensities.get(device_id).copied().unwrap_or(0.0)
    }

    /// Get all movement intensities
    pub fn movement_intensities(&self) -> &HashMap<String, f32> {
        &self.movement_intensities
    }

    /// Whether to hide the calibration help dialog before calibrating
    pub fn hide_calibration_help(&self) -> bool {
        self.hide_calibration_help
    }

    /// Set whether to hide the calibration help dialog
    pub fn set_hide_calibration_help(&mut self, hide: bool) {
        if self.hide_calibration_help != hide {
            self.hide_calibration_help = hide;
            self.save_config();
        }
    }

    /// Get the configured sample count
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Set the sample count and persist
    pub fn set_sample_count(&mut self, count: u32) {
        if self.sample_count != count {
            self.sample_count = count;
            self.save_config();
        }
    }

    /// Save current selection to config file
    fn save_config(&self) {
        let config = Config {
            source: self.source_id.clone().unwrap_or_default(),
            target: self.target_id.clone().unwrap_or_default(),
            hide_calibration_help: self.hide_calibration_help,
            sample_count: self.sample_count,
        };
        if let Err(e) = config.save() {
            eprintln!("Warning: Failed to save config: {}", e);
        }
    }

    /// Notify all listeners of state change
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

/// Shared reference to app state for use across UI components
pub type SharedState = Rc<RefCell<AppState>>;

/// Create a new shared state instance
pub fn create_shared_state() -> SharedState {
    Rc::new(RefCell::new(AppState::new()))
}
