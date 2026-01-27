use serde::{Deserialize, Serialize};
use crate::error::ConfigError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub source: String,
    pub target: String,
}

impl Config {
    /// Get the path to the configuration file
    ///
    /// Returns the path to the config.json file in the user's data directory.
    /// Falls back to the current directory if the data directory cannot be determined.
    pub fn path() -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("monado-spacecal")
            .join("config.json")
    }

    /// Load configuration from disk
    ///
    /// Attempts to load the configuration file from the default path.
    /// If the file does not exist or cannot be read/parsed, returns a default configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use monado_spacecal::config::Config;
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

    /// Save configuration to disk
    ///
    /// Writes the configuration to the default path as a JSON file.
    /// Creates the parent directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The config directory cannot be created
    /// - The configuration cannot be serialized to JSON
    /// - The file cannot be written
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use monado_spacecal::config::Config;
    /// # fn example() -> Result<(), monado_spacecal::error::ConfigError> {
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
