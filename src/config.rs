use serde::{Deserialize, Serialize};
use crate::error::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub hide_calibration_help: bool,
    #[serde(default = "default_sample_count")]
    pub sample_count: u32,
    #[serde(default)]
    pub continuous_enabled: bool,
}

fn default_sample_count() -> u32 {
    400
}

impl Config {
    /// Config file path. Falls back to current dir if data dir unavailable.
    pub fn path() -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("spacecal-for-monado")
            .join("config.json")
    }

    /// # Examples
    ///
    /// ```
    /// use spacecal_for_monado::config::Config;
    /// let config = Config::load();
    /// ```
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(config) => config,
                    Err(e) => {
                        eprintln!("Warning: Failed to parse config file: {}. Using defaults.", e);
                        Self::default()
                    }
                },
                Err(e) => {
                    eprintln!("Warning: Failed to read config file: {}. Using defaults.", e);
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }

    /// # Examples
    ///
    /// ```no_run
    /// use spacecal_for_monado::config::Config;
    /// # fn example() -> Result<(), spacecal_for_monado::error::ConfigError> {
    /// let config = Config::default();
    /// config.save()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::DirectoryCreationFailed(e.to_string()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ConfigError::SaveFailed(format!("Failed to serialize config: {}", e)))?;
        std::fs::write(&path, content)
            .map_err(|e| ConfigError::SaveFailed(format!("Failed to write config file: {}", e)))?;
        Ok(())
    }
}
