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

        Self {
            connection,
            categories,
            source_id: if config.source.is_empty() { None } else { Some(config.source) },
            target_id: if config.target.is_empty() { None } else { Some(config.target) },
            movement_intensities: HashMap::new(),
            listeners: Vec::new(),
        }
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

    /// Try to connect/reconnect to Monado and refresh devices
    /// Returns true if connection state changed
    pub fn refresh_connection(&mut self) -> bool {
        let was_connected = self.is_connected();
        self.connection = monado::try_connect();
        let is_connected = self.is_connected();

        if is_connected {
            self.categories = monado::enumerate_devices();
        } else {
            self.categories.clear();
        }

        let changed = was_connected != is_connected;
        if changed {
            self.notify_listeners();
        }
        changed
    }

    /// Force refresh device list (when already connected)
    #[allow(dead_code)]
    pub fn refresh_devices(&mut self) {
        if self.is_connected() {
            self.categories = monado::enumerate_devices();
            self.notify_listeners();
        }
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

    /// Save current selection to config file
    fn save_config(&self) {
        let config = Config {
            source: self.source_id.clone().unwrap_or_default(),
            target: self.target_id.clone().unwrap_or_default(),
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
