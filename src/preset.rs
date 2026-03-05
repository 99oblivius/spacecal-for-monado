#![allow(dead_code)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::error::PresetError;

/// A saved calibration preset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Preset metadata
    pub preset: PresetMetadata,
    /// Device selections
    pub devices: PresetDevices,
    /// Calibration offset
    pub calibration: PresetCalibration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetMetadata {
    pub name: String,
    pub created: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetDevices {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetCalibration {
    /// Position offset [x, y, z]
    pub position: [f64; 3],
    /// Orientation quaternion [x, y, z, w]
    pub orientation: [f64; 4],
    /// Tracking origin index this calibration applies to
    pub target_origin_index: u32,
    /// Calibration accuracy percentage (0-100)
    #[serde(default)]
    pub accuracy_percent: u8,
    /// RMS error in millimeters
    #[serde(default)]
    pub rms_error_mm: f32,
}

impl Preset {
    /// Create a new preset
    pub fn new(
        name: String,
        source: String,
        target: String,
        position: [f64; 3],
        orientation: [f64; 4],
        target_origin_index: u32,
        accuracy_percent: u8,
        rms_error_mm: f32,
    ) -> Self {
        Self {
            preset: PresetMetadata {
                name,
                created: Utc::now(),
                description: None,
            },
            devices: PresetDevices { source, target },
            calibration: PresetCalibration {
                position,
                orientation,
                target_origin_index,
                accuracy_percent,
                rms_error_mm,
            },
        }
    }

    /// Get the presets directory path
    pub fn presets_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("spacecal-for-monado")
            .join("presets")
    }

    /// Get the path for a preset by name
    pub fn preset_path(name: &str) -> PathBuf {
        Self::presets_dir().join(format!("{}.toml", sanitize_filename(name)))
    }

    /// Save the preset to disk
    pub fn save(&self) -> Result<PathBuf, PresetError> {
        let dir = Self::presets_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| PresetError::DirectoryCreationFailed(e.to_string()))?;

        let path = Self::preset_path(&self.preset.name);
        let content = toml::to_string_pretty(self)
            .map_err(|e| PresetError::InvalidFormat(e.to_string()))?;

        std::fs::write(&path, content)
            .map_err(|e| PresetError::SaveFailed {
                name: self.preset.name.clone(),
                reason: e.to_string(),
            })?;

        Ok(path)
    }

    /// Load a preset by name
    pub fn load(name: &str) -> Result<Self, PresetError> {
        let path = Self::preset_path(name);
        Self::load_from_path(&path)
    }

    /// Load a preset from a specific path
    pub fn load_from_path(path: &PathBuf) -> Result<Self, PresetError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| PresetError::LoadFailed {
                name: path.display().to_string(),
                reason: e.to_string(),
            })?;

        toml::from_str(&content)
            .map_err(|e| PresetError::TomlParseError(e.to_string()))
    }

    /// Delete a preset by name
    pub fn delete(name: &str) -> Result<(), PresetError> {
        let path = Self::preset_path(name);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| PresetError::DeleteFailed(name.to_string(), e.to_string()))?;
        }
        Ok(())
    }

    /// List all available presets
    pub fn list_all() -> Result<Vec<String>, PresetError> {
        let dir = Self::presets_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| PresetError::LoadFailed {
                name: dir.display().to_string(),
                reason: e.to_string(),
            })?
            .flatten()
        {
            let path = entry.path();
            if path.extension().map(|e| e == "toml").unwrap_or(false)
                && let Some(stem) = path.file_stem()
            {
                names.push(stem.to_string_lossy().to_string());
            }
        }

        names.sort();
        Ok(names)
    }

    /// Load all presets
    pub fn load_all() -> Result<Vec<Self>, PresetError> {
        let names = Self::list_all()?;
        let mut presets = Vec::new();
        for name in names {
            if let Ok(preset) = Self::load(&name) {
                presets.push(preset);
            }
        }
        Ok(presets)
    }
}

/// Sanitize a string for use as a filename
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else if c == ' ' {
                '-'
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("My Preset"), "My-Preset");
        assert_eq!(sanitize_filename("preset/with:special"), "preset_with_special");
    }

    #[test]
    fn test_preset_serialization() {
        let preset = Preset::new(
            "Test".to_string(),
            "WiVRn HMD".to_string(),
            "LHR-EEBBC131".to_string(),
            [0.0, 1.5, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            1,  // target_origin_index
            95, // accuracy_percent
            5.2, // rms_error_mm
        );

        let toml = toml::to_string_pretty(&preset).unwrap();
        let loaded: Preset = toml::from_str(&toml).unwrap();

        assert_eq!(preset.preset.name, loaded.preset.name);
        assert_eq!(preset.devices.source, loaded.devices.source);
        assert_eq!(preset.calibration.target_origin_index, loaded.calibration.target_origin_index);
    }
}
