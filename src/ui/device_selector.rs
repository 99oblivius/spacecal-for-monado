#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    /// Unique ID. Important for Vive trackers which share the same name.
    pub serial: String,
    pub description: String,
    pub category: String,
    pub category_index: u32,
    pub device_index: u32,
    pub battery_charge: Option<f32>,
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

    pub fn unique_id(&self) -> &str {
        if !self.serial.is_empty() {
            &self.serial
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone)]
pub struct Category {
    pub index: u32,
    pub name: String,
    pub devices: Vec<Device>,
}
