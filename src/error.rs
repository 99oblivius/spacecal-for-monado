use thiserror::Error;

/// Errors related to Monado IPC and device operations
#[derive(Error, Debug)]
pub enum MonadoError {
    #[error("Failed to connect to Monado: {0}")]
    ConnectionFailed(String),

    #[error("Failed to apply offset: {0}")]
    ApplyOffsetFailed(String),

    #[error("Failed to enumerate devices: {0}")]
    EnumerationFailed(String),

    #[error("Invalid device ID: {0}")]
    InvalidDeviceId(u32),

    #[error("Failed to get tracking origin: {0}")]
    TrackingOriginFailed(String),
}

/// Errors related to OpenXR session and extension operations
#[derive(Error, Debug)]
pub enum XrError {
    #[error("Failed to create OpenXR instance: {0}")]
    InstanceCreationFailed(String),

    #[error("Failed to create OpenXR session: {0}")]
    SessionCreationFailed(String),

    #[error("Required extension not available: {0}")]
    ExtensionNotAvailable(String),

    #[error("OpenXR runtime error: {0}")]
    RuntimeError(String),
}

/// Errors related to calibration algorithm and sampling
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum CalibrationError {
    #[error("Not enough samples collected: {collected}/{required}")]
    InsufficientSamples { collected: usize, required: usize },

    #[error("Sample collection failed: {0}")]
    SampleCollectionFailed(String),

    #[error("Matrix computation failed: {0}")]
    MatrixComputationFailed(String),

    #[error("SVD decomposition failed")]
    SvdFailed,

    #[error("Invalid rotation matrix: determinant = {0}")]
    InvalidRotationMatrix(f32),

    #[error("Linear least squares failed: {0}")]
    LeastSquaresFailed(String),

    #[error("Source device has no samples")]
    NoSourceSamples,

    #[error("Target device has no samples")]
    NoTargetSamples,

    #[error("Sample mismatch: source has {source_count} samples, target has {target_count} samples")]
    SampleMismatch { source_count: usize, target_count: usize },

    #[error("Calibration not initialized")]
    NotInitialized,

    #[error("Devices are identical: cannot calibrate device to itself")]
    IdenticalDevices,

    #[error("Invalid pose data: {0}")]
    InvalidPoseData(String),

    #[error("Calibration accuracy too low: {0}")]
    AccuracyTooLow(String),

    #[error("High variance in samples: {variance:.4}m (threshold: {threshold:.4}m)")]
    HighVariance { variance: f32, threshold: f32 },
}

/// Errors related to preset save/load operations
#[derive(Error, Debug)]
pub enum PresetError {
    #[error("Failed to load preset '{name}': {reason}")]
    LoadFailed { name: String, reason: String },

    #[error("Failed to save preset '{name}': {reason}")]
    SaveFailed { name: String, reason: String },

    #[error("Invalid preset format: {0}")]
    InvalidFormat(String),

    #[error("Failed to create preset directory: {0}")]
    DirectoryCreationFailed(String),

    #[error("Failed to delete preset '{0}': {1}")]
    DeleteFailed(String, String),

    #[error("Failed to parse TOML: {0}")]
    TomlParseError(String),
}

/// Errors related to configuration persistence
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to load config: {0}")]
    LoadFailed(String),

    #[error("Failed to save config: {0}")]
    SaveFailed(String),

    #[error("Invalid config format: {0}")]
    InvalidFormat(String),

    #[error("Config directory creation failed: {0}")]
    DirectoryCreationFailed(String),
}

impl From<toml::de::Error> for PresetError {
    fn from(e: toml::de::Error) -> Self {
        PresetError::TomlParseError(e.to_string())
    }
}

impl From<toml::ser::Error> for PresetError {
    fn from(e: toml::ser::Error) -> Self {
        PresetError::InvalidFormat(e.to_string())
    }
}

impl From<std::io::Error> for PresetError {
    fn from(e: std::io::Error) -> Self {
        PresetError::LoadFailed {
            name: "unknown".to_string(),
            reason: e.to_string(),
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::LoadFailed(e.to_string())
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::InvalidFormat(e.to_string())
    }
}
