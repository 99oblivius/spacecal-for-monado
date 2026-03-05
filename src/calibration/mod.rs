pub mod continuous;
pub mod floor;
pub mod sampled;
pub mod transform;

pub use transform::TransformD;

/// Commands sent from the main thread to the calibration background thread
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum CalibrationCommand {
    /// Start sampled calibration between source and target devices (by serial/unique ID)
    /// The TARGET tracking origin will be adjusted to align with SOURCE
    StartSampled {
        /// Source device serial number (unique identifier)
        source_serial: String,
        /// Target device serial number (unique identifier)
        target_serial: String,
        /// Tracking origin index of target device (this is what we adjust)
        target_origin_index: u32,
        sample_count: u32,
        /// Stage reference space offset from Monado (position, orientation)
        /// Used to transform poses to a common world frame like motoc does
        stage_offset: Option<([f64; 3], [f64; 4])>,
    },
    /// Start continuous calibration mode (by name)
    StartContinuous {
        source_name: String,
        target_name: String,
    },
    /// Stop continuous calibration
    StopContinuous,
    /// Calibrate floor using target device position
    /// Place the target device on the floor, its Y position becomes floor level
    CalibrateFloor {
        /// Target device serial (the device placed on floor)
        target_serial: String,
    },
    /// Reset floor offset (STAGE reference space Y)
    ResetFloor,
    /// Reset offset for a device category
    ResetOffset { category_index: u32 },
    /// Recenter forward direction using source device orientation
    /// Applies rotation to ALL tracking origins to preserve calibration
    Recenter {
        /// Source device serial (HMD) to get current forward direction
        source_serial: String,
    },
    /// Start movement detection for all devices
    StartMovementDetection,
    /// Stop movement detection
    StopMovementDetection,
    /// Shutdown the calibration thread
    Shutdown,
}

/// Movement intensity for a device (0.0 = still, 1.0 = actively moving)
#[derive(Debug, Clone)]
pub struct DeviceMovement {
    /// Device unique ID (serial or name fallback) for matching with UI device list
    pub device_id: String,
    pub intensity: f32,
}

/// Messages sent from the calibration thread back to the main GTK thread
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum CalibrationMessage {
    /// Countdown before sampled calibration starts (seconds remaining)
    Countdown { seconds: u32 },
    /// Countdown before recenter (seconds remaining)
    RecenterCountdown { seconds: u32 },
    /// Progress update during sampled calibration
    Progress { collected: u32, total: u32 },
    /// Progress update during floor calibration (separate so UI shows on correct button)
    FloorProgress { collected: u32, total: u32 },
    /// Sampled calibration completed
    SampledComplete(CalibrationResult),
    /// Continuous mode transform update
    ContinuousUpdate { transform: TransformD },
    /// Floor calibration completed - height_adjustment is delta for display only
    /// The actual adjustment is applied to ALL tracking origins to maintain calibration
    FloorComplete {
        height_adjustment: f32,
    },
    /// Reset completed
    ResetComplete { category_index: u32 },
    /// Floor reset completed
    ResetFloorComplete,
    /// Recenter completed with HMD pose to apply
    RecenterComplete {
        /// HMD position in STAGE coords [x, y, z]
        position: [f32; 3],
        /// HMD orientation as quaternion [x, y, z, w]
        orientation: [f32; 4],
    },
    /// Movement detection update - devices with their movement intensities (fading)
    MovementUpdate { movements: Vec<DeviceMovement> },
    /// Error occurred
    Error(String),
}

/// Result of a successful sampled calibration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CalibrationResult {
    /// Transform to apply to target tracking origin to align it with source
    pub transform: TransformD,
    pub source_name: String,
    pub target_name: String,
    /// Tracking origin index to apply the offset to (target device's origin)
    pub target_origin_index: u32,
    pub sample_count: u32,
    /// Median angular residual in degrees (lower = better fit)
    pub median_error_degrees: f32,
    /// Axis diversity from Kabsch SVD (0-1): how well the motion covered all three rotation axes.
    /// Near 1.0 = excellent coverage, near 0.0 = motion was nearly coplanar.
    pub axis_diversity: f32,
}
