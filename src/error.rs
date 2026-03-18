use thiserror::Error;

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

#[derive(Error, Debug)]
pub enum CalibrationError {
    #[error("Not enough samples collected: {collected}/{required}")]
    InsufficientSamples { collected: usize, required: usize },

    #[error("SVD decomposition failed")]
    SvdFailed,

    #[error("Invalid pose data: {0}")]
    InvalidPoseData(String),

    #[error("High variance in samples: {variance:.4}m (threshold: {threshold:.4}m)")]
    HighVariance { variance: f32, threshold: f32 },
}

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
