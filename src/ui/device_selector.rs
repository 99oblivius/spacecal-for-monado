//! Device types for tracking origin calibration

/// A tracked device from Monado
#[derive(Debug, Clone)]
pub struct Device {
    /// Device name (e.g., "WiVRn HMD") - for display
    pub name: String,
    /// Device serial number - unique identifier for matching
    /// This is important for devices like HTC Vive trackers where all have the same name
    pub serial: String,
    /// Optional description
    pub description: String,
    /// Tracking origin category name
    pub category: String,
    /// Tracking origin index for libmonado operations
    pub category_index: u32,
    /// libmonado device index (for querying battery etc.)
    pub device_index: u32,
    /// Battery charge level (0.0 - 1.0), None if no battery present
    pub battery_charge: Option<f32>,
    /// Whether battery is currently charging
    pub battery_charging: bool,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        // Battery state doesn't affect device identity
        self.name == other.name
            && self.serial == other.serial
            && self.category == other.category
            && self.category_index == other.category_index
            && self.device_index == other.device_index
    }
}

impl Eq for Device {}

impl Device {
    /// Get display name, including serial suffix for disambiguation
    pub fn display_name(&self) -> String {
        // Show serial suffix if name might be ambiguous (non-empty serial different from name)
        if !self.serial.is_empty() && self.serial != self.name {
            // Show last 8 chars of serial for brevity
            let serial_suffix = if self.serial.len() > 8 {
                &self.serial[self.serial.len() - 8..]
            } else {
                &self.serial
            };
            format!("{} [{}]", self.name, serial_suffix)
        } else if !self.description.is_empty() {
            format!("{} ({})", self.name, self.description)
        } else {
            self.name.clone()
        }
    }

    /// Get the unique identifier for this device (serial or name fallback)
    pub fn unique_id(&self) -> &str {
        if !self.serial.is_empty() {
            &self.serial
        } else {
            &self.name
        }
    }
}

/// A group of devices sharing a tracking origin
#[derive(Debug, Clone)]
pub struct Category {
    /// Tracking origin index
    pub index: u32,
    /// Category name
    pub name: String,
    /// Devices in this category
    pub devices: Vec<Device>,
}
