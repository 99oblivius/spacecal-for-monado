pub mod floor;
pub mod sampled;
pub mod transform;

pub use transform::TransformD;

/// Commands from the UI thread to the XR background thread.
#[derive(Debug, Clone)]
pub enum CalibrationCommand {
    StartSampled {
        source_serial: String,
        target_serial: String,
        target_origin_index: u32,
        sample_count: u32,
        stage_offset: Option<([f64; 3], [f64; 4])>,
    },
    CalibrateFloor {
        target_serial: String,
    },
    Recenter,
    StartMovementDetection,
    StopMovementDetection,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct DeviceMovement {
    pub device_id: String,
    pub intensity: f32,
}

/// Messages from the XR background thread back to the UI.
#[derive(Debug, Clone)]
pub enum CalibrationMessage {
    Countdown { seconds: u32 },
    RecenterCountdown { seconds: u32 },
    Progress { collected: u32, total: u32 },
    FloorProgress { collected: u32, total: u32 },
    SampledComplete(CalibrationResult),
    FloorComplete { height_adjustment: f32 },
    RecenterComplete {
        position: [f32; 3],
        orientation: [f32; 4],
    },
    MovementUpdate { movements: Vec<DeviceMovement> },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct CalibrationResult {
    pub transform: TransformD,
    pub target_origin_index: u32,
    pub median_error_degrees: f32,
    pub axis_diversity: f32,
}
