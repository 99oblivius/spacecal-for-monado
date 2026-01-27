pub mod hand_tracking;
pub mod mndx;

use openxr as xr;
use std::sync::mpsc;
use std::ptr;
use std::time::{Duration, Instant};
use crate::calibration::{CalibrationCommand, CalibrationMessage, CalibrationResult, DeviceMovement, TransformD};
use crate::calibration::sampled::{SampleCollector, PoseSample};
use crate::calibration::floor::FloorCalibrator;
use crate::error::XrError;

/// A trait for sending calibration messages back to the UI thread
pub trait MessageSender: Send {
    fn send(&self, msg: CalibrationMessage) -> Result<(), String>;
}

/// Implementation for std::sync::mpsc::Sender
impl MessageSender for mpsc::Sender<CalibrationMessage> {
    fn send(&self, msg: CalibrationMessage) -> Result<(), String> {
        mpsc::Sender::send(self, msg).map_err(|e| e.to_string())
    }
}

/// Implementation for async_channel::Sender
impl MessageSender for async_channel::Sender<CalibrationMessage> {
    fn send(&self, msg: CalibrationMessage) -> Result<(), String> {
        self.send_blocking(msg).map_err(|e| e.to_string())
    }
}

pub use mndx::Mndx;

// XR_MND_headless extension structures
const XR_TYPE_GRAPHICS_BINDING_HEADLESS_MND: i32 = 1000246000;

#[repr(C)]
struct GraphicsBindingHeadlessMND {
    ty: xr::sys::StructureType,
    next: *const std::ffi::c_void,
}

// Minimal Graphics implementation for headless mode
struct Headless;

impl xr::Graphics for Headless {
    type Requirements = ();
    type SessionCreateInfo = GraphicsBindingHeadlessMND;
    type Format = i64;
    type SwapchainImage = ();

    fn raise_format(x: i64) -> Self::Format {
        x
    }

    fn lower_format(x: Self::Format) -> i64 {
        x
    }

    fn requirements(_instance: &xr::Instance, _system: xr::SystemId) -> xr::Result<Self::Requirements> {
        Ok(())
    }

    unsafe fn create_session(
        instance: &xr::Instance,
        system: xr::SystemId,
        info: &Self::SessionCreateInfo,
    ) -> xr::Result<xr::sys::Session> {
        let session_create_info = xr::sys::SessionCreateInfo {
            ty: xr::sys::SessionCreateInfo::TYPE,
            next: info as *const _ as *const _,
            create_flags: xr::sys::SessionCreateFlags::EMPTY,
            system_id: system,
        };

        let mut session_raw: xr::sys::Session = xr::sys::Session::NULL;
        // SAFETY: Calling OpenXR FFI function with valid parameters
        let result = unsafe {
            (instance.fp().create_session)(
                instance.as_raw(),
                &session_create_info,
                &mut session_raw,
            )
        };

        if result != xr::sys::Result::SUCCESS {
            return Err(result);
        }

        Ok(session_raw)
    }

    fn enumerate_swapchain_images(_swapchain: &xr::Swapchain<Self>) -> xr::Result<Vec<Self::SwapchainImage>> {
        // Headless mode doesn't use swapchains
        Ok(Vec::new())
    }
}

/// Manages the OpenXR session and device spaces
pub struct XrSession {
    #[allow(dead_code)]
    instance: xr::Instance,
    #[allow(dead_code)]
    session: xr::Session<xr::AnyGraphics>,
    mndx: Option<Mndx>,
    // hand tracking will be added later
}

impl XrSession {
    /// Create a new headless OpenXR session
    #[allow(dead_code)]
    pub fn new() -> Result<Self, XrError> {
        // 1. Load OpenXR runtime
        let entry = unsafe { xr::Entry::load() }
            .map_err(|e| XrError::RuntimeError(format!("Failed to load OpenXR: {:?}", e)))?;

        // 2. Enumerate available extensions
        let available = entry.enumerate_extensions()
            .map_err(|e| XrError::RuntimeError(format!("Failed to enumerate extensions: {:?}", e)))?;

        // 3. Check for required MND_headless extension
        if !available.mnd_headless {
            return Err(XrError::ExtensionNotAvailable("MND_headless".to_string()));
        }

        // 4. Build extension set
        let mut exts = xr::ExtensionSet::default();
        exts.mnd_headless = true;

        // Enable optional extensions if available
        let has_mndx = available.other.iter().any(|e| e == "XR_MNDX_xdev_space");
        if has_mndx {
            exts.other.push("XR_MNDX_xdev_space".to_string());
        }
        if available.ext_hand_tracking {
            exts.ext_hand_tracking = true;
        }
        // Enable time conversion for getting current time
        if available.khr_convert_timespec_time {
            exts.khr_convert_timespec_time = true;
        }

        // 5. Create instance
        let instance = entry.create_instance(
            &xr::ApplicationInfo {
                application_name: "monado-spacecal",
                application_version: 1,
                engine_name: "none",
                engine_version: 0,
                api_version: xr::Version::new(1, 0, 0),
            },
            &exts,
            &[],
        ).map_err(|e| XrError::InstanceCreationFailed(format!("{:?}", e)))?;

        // 6. Get system
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|e| XrError::RuntimeError(format!("Failed to get system: {:?}", e)))?;

        // 7. Create headless session
        let graphics_binding = GraphicsBindingHeadlessMND {
            ty: xr::sys::StructureType::from_raw(XR_TYPE_GRAPHICS_BINDING_HEADLESS_MND),
            next: ptr::null(),
        };

        let (session, _frame_waiter, _frame_stream) = unsafe {
            instance.create_session::<Headless>(system, &graphics_binding)
                .map_err(|e| XrError::SessionCreationFailed(format!("{:?}", e)))?
        };

        // Convert to AnyGraphics to match the struct field type
        let session = session.into_any_graphics();

        // 8. Load MNDX extension
        let mndx = if has_mndx {
            Mndx::new(&instance).ok()
        } else {
            None
        };

        Ok(Self {
            instance,
            session,
            mndx,
        })
    }

    /// Check if MNDX_xdev_space is available
    #[allow(dead_code)]
    pub fn has_mndx(&self) -> bool {
        self.mndx.is_some()
    }

    /// Check if hand tracking is available
    #[allow(dead_code)]
    pub fn has_hand_tracking(&self) -> bool {
        // Will be implemented with hand_tracking module
        false
    }

    /// Get current time from the OpenXR runtime
    pub fn now(&self) -> xr::Time {
        self.instance.now().unwrap_or(xr::Time::from_nanos(1))
    }
}

// Movement detection constants
const MOVEMENT_LINEAR_THRESHOLD: f32 = 0.2; // m/s
const MOVEMENT_ANGULAR_THRESHOLD: f32 = 0.5; // rad/s (about 30 deg/s)
const MOVEMENT_FADE_DURATION: f32 = 2.0;

struct DeviceMovementState {
    device_id: String,  // Serial or name fallback
    last_moving_time: Option<Instant>,
}

/// Run the OpenXR event loop in a background thread.
///
/// This function is the main entry point for the calibration background thread. It:
/// 1. Attempts to create an OpenXR headless session
/// 2. If OpenXR is unavailable, continues in fallback mode (sends errors for XR operations)
/// 3. Processes CalibrationCommand messages from the channel
/// 4. Sends CalibrationMessage results back to the main thread
/// 5. Exits cleanly when receiving a Shutdown command or when the channel closes
pub fn xr_event_loop<S: MessageSender>(
    cmd_rx: mpsc::Receiver<CalibrationCommand>,
    msg_tx: S,
) {
    // Try to create XR session (mutable for reconnection)
    // Use catch_unwind to handle panics from unimplemented code gracefully
    let mut xr_session = std::panic::catch_unwind(XrSession::new)
        .ok()
        .and_then(|result| result.ok());

    // Movement detection state
    let mut movement_detection_active = false;
    let mut movement_state: Vec<DeviceMovementState> = Vec::new();
    let mut last_movement_update = Instant::now();
    let mut last_reconnect_attempt = Instant::now();
    let mut consecutive_failures = 0u32;

    // Main event loop - use timeout when movement detection is active
    loop {
        let recv_result = if movement_detection_active {
            // Non-blocking with short timeout for responsive movement updates
            cmd_rx.recv_timeout(Duration::from_millis(50))
        } else {
            // Blocking when idle
            cmd_rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };

        // Try to reconnect OpenXR if disconnected (every 2 seconds)
        // or if we've had too many consecutive failures (session became stale)
        if (xr_session.is_none() || consecutive_failures >= 5) && last_reconnect_attempt.elapsed() >= Duration::from_secs(2) {
            last_reconnect_attempt = Instant::now();
            consecutive_failures = 0;
            xr_session = std::panic::catch_unwind(XrSession::new)
                .ok()
                .and_then(|result| result.ok());
        }

        // Handle movement detection polling using OpenXR velocity (like motoc)
        if movement_detection_active && last_movement_update.elapsed() >= Duration::from_millis(100) {
            let mut operation_succeeded = false;

            if let Some(ref xr_session) = xr_session
                && let Some(ref mndx) = xr_session.mndx
                && let Ok(list) = mndx.create_list(&xr_session.session)
                && let Ok(devices) = list.enumerate_xdevs()
                && let Ok(reference_space) = xr_session.session.create_reference_space(
                    xr::ReferenceSpaceType::LOCAL,
                    xr::Posef::IDENTITY,
                )
            {
                operation_succeeded = true;
                let time = xr_session.now();
                let now = Instant::now();

                for device in devices.iter() {
                    if !device.can_create_space() {
                        continue;
                    }

                    if let Ok(space) = device.create_space(xr_session.session.clone()) {
                        // Use relate() to get velocity directly from OpenXR (like motoc)
                        if let Ok((_location, velocity)) = space.relate(&reference_space, time) {
                            let linear_valid = velocity.velocity_flags.contains(
                                xr::SpaceVelocityFlags::LINEAR_VALID
                            );
                            let angular_valid = velocity.velocity_flags.contains(
                                xr::SpaceVelocityFlags::ANGULAR_VALID
                            );

                            // Check linear velocity (translation speed)
                            let linear_moving = if linear_valid {
                                let vel = velocity.linear_velocity;
                                let speed = (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt();
                                speed > MOVEMENT_LINEAR_THRESHOLD
                            } else {
                                false
                            };

                            // Check angular velocity (rotation speed)
                            let angular_moving = if angular_valid {
                                let ang = velocity.angular_velocity;
                                let spin = (ang.x * ang.x + ang.y * ang.y + ang.z * ang.z).sqrt();
                                spin > MOVEMENT_ANGULAR_THRESHOLD
                            } else {
                                false
                            };

                            // Device is moving if either translating or rotating
                            let is_moving = linear_moving || angular_moving;

                            if linear_valid || angular_valid {
                                // Use serial as unique ID, fallback to name
                                let device_id = {
                                    let serial = device.serial();
                                    if serial.is_empty() {
                                        device.name().to_string()
                                    } else {
                                        serial.to_string()
                                    }
                                };

                                // Find or create state entry
                                let state = movement_state.iter_mut()
                                    .find(|s| s.device_id == device_id);

                                if let Some(s) = state {
                                    if is_moving {
                                        s.last_moving_time = Some(now);
                                    }
                                } else {
                                    movement_state.push(DeviceMovementState {
                                        device_id,
                                        last_moving_time: if is_moving {
                                            Some(now)
                                        } else {
                                            None
                                        },
                                    });
                                }
                            }
                        }
                    }
                }

                // Compute movement intensities with fade
                let movements: Vec<DeviceMovement> = movement_state
                    .iter()
                    .filter_map(|s| {
                        let last_moving = s.last_moving_time?;
                        let elapsed = now.duration_since(last_moving).as_secs_f32();
                        if elapsed < MOVEMENT_FADE_DURATION {
                            // Intensity fades from 1.0 to 0.0 over MOVEMENT_FADE_DURATION
                            let intensity = 1.0 - (elapsed / MOVEMENT_FADE_DURATION);
                            Some(DeviceMovement {
                                device_id: s.device_id.clone(),
                                intensity,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                let _ = msg_tx.send(CalibrationMessage::MovementUpdate { movements });
            }

            // Track consecutive failures to detect stale sessions
            if operation_succeeded {
                consecutive_failures = 0;
            } else if xr_session.is_some() {
                consecutive_failures += 1;
            }

            last_movement_update = Instant::now();
        }

        match recv_result {
            Ok(CalibrationCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Continue loop for movement detection polling
                continue;
            }
            Ok(CalibrationCommand::StartMovementDetection) => {
                movement_detection_active = true;
                movement_state.clear();
                last_movement_update = Instant::now();
            }
            Ok(CalibrationCommand::StopMovementDetection) => {
                movement_detection_active = false;
                movement_state.clear();
                // Send empty update to clear highlights
                let _ = msg_tx.send(CalibrationMessage::MovementUpdate { movements: vec![] });
            }
            Ok(CalibrationCommand::StartSampled { source_serial, target_serial, target_origin_index, sample_count, stage_offset }) => {

                // Check if XR is available
                if xr_session.is_none() {
                    let _ = msg_tx.send(CalibrationMessage::Error(
                        "Connect to WiVRn to enable calibration".to_string()
                    ));
                    continue;
                }

                // 3-second countdown before calibration starts
                for seconds_left in (1..=3).rev() {
                    let _ = msg_tx.send(CalibrationMessage::Countdown { seconds: seconds_left });
                    std::thread::sleep(Duration::from_secs(1));
                }

                // Implementation of sampled calibration
                if let Some(ref xr_session) = xr_session {
                    // Get MNDX extension
                    let mndx = match &xr_session.mndx {
                        Some(m) => m,
                        None => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                "MNDX_xdev_space extension not available".to_string()
                            ));
                            continue;
                        }
                    };

                    // Create device list
                    let list = match mndx.create_list(&xr_session.session) {
                        Ok(l) => l,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create device list: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Enumerate devices and find source/target by serial (unique ID)
                    let devices = match list.enumerate_xdevs() {
                        Ok(d) => d,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to enumerate devices: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Find source device by serial (or name as fallback for backward compat)
                    let source_dev = match devices.iter().find(|d| d.serial() == source_serial || d.name() == source_serial) {
                        Some(d) => d,
                        None => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Source device '{}' not found", source_serial)
                            ));
                            continue;
                        }
                    };

                    // Find target device by serial (or name as fallback)
                    let target_dev = match devices.iter().find(|d| d.serial() == target_serial || d.name() == target_serial) {
                        Some(d) => d,
                        None => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Target device '{}' not found", target_serial)
                            ));
                            continue;
                        }
                    };

                    // Check if both devices support space creation
                    if !source_dev.can_create_space() {
                        let _ = msg_tx.send(CalibrationMessage::Error(
                            format!("Source device '{}' does not support space creation", source_dev.name())
                        ));
                        continue;
                    }

                    if !target_dev.can_create_space() {
                        let _ = msg_tx.send(CalibrationMessage::Error(
                            format!("Target device '{}' does not support space creation", target_dev.name())
                        ));
                        continue;
                    }

                    // Create spaces for both devices
                    let source_space = match source_dev.create_space(xr_session.session.clone()) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create space for source device: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    let target_space = match target_dev.create_space(xr_session.session.clone()) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create space for target device: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Create reference space (STAGE - like motoc uses)
                    // Using STAGE ensures poses are in a consistent floor-relative frame
                    let reference_space = match xr_session.session.create_reference_space(
                        xr::ReferenceSpaceType::STAGE,
                        xr::Posef::IDENTITY,
                    ) {
                        Ok(space) => space,
                        Err(_) => {
                            // Fall back to LOCAL if STAGE not available
                            match xr_session.session.create_reference_space(
                                xr::ReferenceSpaceType::LOCAL,
                                xr::Posef::IDENTITY,
                            ) {
                                Ok(s) => s,
                                Err(e) => {
                                    let _ = msg_tx.send(CalibrationMessage::Error(
                                        format!("Failed to create reference space: {:?}", e)
                                    ));
                                    continue;
                                }
                            }
                        }
                    };

                    // Create sample collector
                    let mut collector = SampleCollector::new(sample_count);

                    // Sample collection loop (~30Hz)
                    let sample_interval = Duration::from_millis(33);
                    let mut next_sample_time = Instant::now();

                    while !collector.is_complete() {
                        // Get current time from the OpenXR runtime
                        let time = xr_session.now();

                        // Locate source space relative to reference space
                        let source_location = match source_space.locate(&reference_space, time) {
                            Ok(loc) => loc,
                            Err(e) => {
                                let _ = msg_tx.send(CalibrationMessage::Error(
                                    format!("Failed to locate source space: {:?}", e)
                                ));
                                break;
                            }
                        };

                        // Locate target space relative to reference space
                        let target_location = match target_space.locate(&reference_space, time) {
                            Ok(loc) => loc,
                            Err(e) => {
                                let _ = msg_tx.send(CalibrationMessage::Error(
                                    format!("Failed to locate target space: {:?}", e)
                                ));
                                break;
                            }
                        };

                        // Check if poses are valid (tracked)
                        let source_valid = source_location.location_flags.contains(
                            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID
                        );
                        let target_valid = target_location.location_flags.contains(
                            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID
                        );

                        if !source_valid || !target_valid {
                            // Pose not valid, skip this sample
                            next_sample_time += sample_interval;
                            let now = Instant::now();
                            if next_sample_time > now {
                                std::thread::sleep(next_sample_time - now);
                            }
                            continue;
                        }

                        // Extract poses
                        let source_pose = source_location.pose;
                        let target_pose = target_location.pose;

                        // Convert poses to TransformD for potential stage offset transformation
                        let mut source_transform = TransformD::from_xr_pose(
                            [source_pose.position.x, source_pose.position.y, source_pose.position.z],
                            [source_pose.orientation.x, source_pose.orientation.y,
                             source_pose.orientation.z, source_pose.orientation.w],
                        );
                        let mut target_transform = TransformD::from_xr_pose(
                            [target_pose.position.x, target_pose.position.y, target_pose.position.z],
                            [target_pose.orientation.x, target_pose.orientation.y,
                             target_pose.orientation.z, target_pose.orientation.w],
                        );

                        // Apply stage offset transformation (like motoc does)
                        // This transforms poses to a common world frame
                        if let Some((pos, ori)) = &stage_offset {
                            let stage = TransformD::from_xr_pose(
                                [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                                [ori[0] as f32, ori[1] as f32, ori[2] as f32, ori[3] as f32],
                            );
                            source_transform = stage.mul(&source_transform);
                            target_transform = stage.mul(&target_transform);
                        }

                        // Convert to PoseSample
                        let sample = PoseSample::from_xr_poses(
                            source_transform.position_f32(),
                            source_transform.orientation_f32(),
                            target_transform.position_f32(),
                            target_transform.orientation_f32(),
                        );

                        // Add sample to collector
                        if let Err(e) = collector.add_sample(sample) {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Invalid pose sample: {:?}", e)
                            ));
                            break;
                        }

                        // Send progress update
                        let (collected, total) = collector.progress();
                        let _ = msg_tx.send(CalibrationMessage::Progress { collected, total });

                        // Wait before next sample (deadline-based timing)
                        if !collector.is_complete() {
                            next_sample_time += sample_interval;
                            let now = Instant::now();
                            if next_sample_time > now {
                                std::thread::sleep(next_sample_time - now);
                            }
                        }
                    }

                    // Compute calibration if we have enough samples
                    if collector.is_complete() {
                        match collector.compute_calibration() {
                            Ok(offset) => {
                                // The calibration computes: O = S × T⁻¹ (averaged over samples)
                                // where S = source pose, T = target pose
                                // This offset satisfies: O × T = S
                                // Apply O to target tracking origin to align it with source.
                                let result = CalibrationResult {
                                    transform: offset,
                                    // Use device names for display, serials were used for lookup
                                    source_name: source_dev.name().to_string(),
                                    target_name: target_dev.name().to_string(),
                                    target_origin_index,
                                    sample_count: collector.sample_count(),
                                };

                                let _ = msg_tx.send(CalibrationMessage::SampledComplete(result));
                            }
                            Err(e) => {
                                let _ = msg_tx.send(CalibrationMessage::Error(
                                    format!("Failed to compute calibration: {:?}", e)
                                ));
                            }
                        }
                    } else {
                        let _ = msg_tx.send(CalibrationMessage::Error(
                            "Sample collection incomplete".to_string()
                        ));
                    }
                }
            }
            Ok(CalibrationCommand::StartContinuous { source_name: _, target_name: _ }) => {
                if xr_session.is_none() {
                    let _ = msg_tx.send(CalibrationMessage::Error(
                        "Connect to WiVRn to enable calibration".to_string()
                    ));
                    continue;
                }

                // TODO: Implement continuous calibration
                let _ = msg_tx.send(CalibrationMessage::Error(
                    "Continuous calibration not yet implemented".to_string()
                ));
            }
            Ok(CalibrationCommand::StopContinuous) => {
                // TODO: Implement stopping continuous calibration
            }
            Ok(CalibrationCommand::CalibrateFloor { target_serial }) => {
                if xr_session.is_none() {
                    let _ = msg_tx.send(CalibrationMessage::Error(
                        "Connect to WiVRn to enable floor calibration".to_string()
                    ));
                    continue;
                }

                // Floor calibration using target device position
                if let Some(ref xr_session) = xr_session {
                    // Get MNDX extension to query device poses
                    let mndx = match &xr_session.mndx {
                        Some(m) => m,
                        None => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                "MNDX extension not available".to_string()
                            ));
                            continue;
                        }
                    };

                    // Create device list
                    let list = match mndx.create_list(&xr_session.session) {
                        Ok(l) => l,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create device list: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Enumerate devices
                    let devices = match list.enumerate_xdevs() {
                        Ok(d) => d,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to enumerate devices: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Find target device by serial (or name as fallback)
                    let target_dev = match devices.iter().find(|d| d.serial() == target_serial || d.name() == target_serial) {
                        Some(d) => d,
                        None => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Target device '{}' not found", target_serial)
                            ));
                            continue;
                        }
                    };

                    // Check if device supports space creation
                    if !target_dev.can_create_space() {
                        let _ = msg_tx.send(CalibrationMessage::Error(
                            format!("Target device '{}' does not support pose tracking", target_dev.name())
                        ));
                        continue;
                    }

                    // Create space for target device
                    let target_space = match target_dev.create_space(xr_session.session.clone()) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create space for target device: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Create reference space (STAGE for floor-relative)
                    let reference_space = match xr_session.session.create_reference_space(
                        xr::ReferenceSpaceType::STAGE,
                        xr::Posef::IDENTITY,
                    ) {
                        Ok(space) => space,
                        Err(_) => {
                            match xr_session.session.create_reference_space(
                                xr::ReferenceSpaceType::LOCAL,
                                xr::Posef::IDENTITY,
                            ) {
                                Ok(space) => space,
                                Err(e) => {
                                    let _ = msg_tx.send(CalibrationMessage::Error(
                                        format!("Failed to create reference space: {:?}", e)
                                    ));
                                    continue;
                                }
                            }
                        }
                    };

                    // Collect floor samples from target device Y position
                    let mut floor_cal = FloorCalibrator::with_default_config();
                    floor_cal.start();

                    let sample_interval = Duration::from_millis(33); // ~30Hz
                    let mut next_sample_time = Instant::now();

                    while floor_cal.is_active() {
                        let time = xr_session.now();

                        // Locate target device relative to STAGE
                        let location = match target_space.locate(&reference_space, time) {
                            Ok(loc) => loc,
                            Err(_) => {
                                // Skip this sample if locate fails
                                next_sample_time += sample_interval;
                                let now = Instant::now();
                                if next_sample_time > now {
                                    std::thread::sleep(next_sample_time - now);
                                }
                                continue;
                            }
                        };

                        // Check if pose is valid
                        let is_valid = location.location_flags.contains(
                            xr::SpaceLocationFlags::POSITION_VALID
                        );

                        if is_valid {
                            // Use the device's Y position as floor height
                            let height = location.pose.position.y;

                            match floor_cal.add_sample(height) {
                                Ok(Some(adjustment)) => {
                                    let _ = msg_tx.send(CalibrationMessage::FloorComplete {
                                        height_adjustment: adjustment,
                                    });
                                    break;
                                }
                                Ok(None) => {
                                    let (collected, total) = floor_cal.progress();
                                    let _ = msg_tx.send(CalibrationMessage::FloorProgress { collected, total });
                                }
                                Err(e) => {
                                    let _ = msg_tx.send(CalibrationMessage::Error(
                                        format!("Floor calibration failed: {:?}", e)
                                    ));
                                    break;
                                }
                            }
                        }

                        next_sample_time += sample_interval;
                        let now = Instant::now();
                        if next_sample_time > now {
                            std::thread::sleep(next_sample_time - now);
                        }
                    }
                }
            }
            Ok(CalibrationCommand::ResetFloor) => {
                // Reset floor is handled directly via libmonado in UI thread
                let _ = msg_tx.send(CalibrationMessage::ResetFloorComplete);
            }
            Ok(CalibrationCommand::ResetOffset { category_index: _ }) => {
                // Reset offset is handled directly via libmonado in UI thread
                let _ = msg_tx.send(CalibrationMessage::Error(
                    "Reset offset should be called from UI thread".to_string()
                ));
            }
            Ok(CalibrationCommand::Recenter { source_serial: _ }) => {
                if xr_session.is_none() {
                    let _ = msg_tx.send(CalibrationMessage::Error(
                        "Connect to WiVRn to enable recenter".to_string()
                    ));
                    continue;
                }

                // Recenter using HMD pose (VIEW space relative to STAGE)
                if let Some(ref xr_session) = xr_session {
                    // 3-second countdown (use separate message so only recenter button updates)
                    for seconds_left in (1..=3).rev() {
                        let _ = msg_tx.send(CalibrationMessage::RecenterCountdown { seconds: seconds_left });
                        std::thread::sleep(Duration::from_secs(1));
                    }

                    // Create VIEW reference space (current HMD pose)
                    let view_space = match xr_session.session.create_reference_space(
                        xr::ReferenceSpaceType::VIEW,
                        xr::Posef::IDENTITY,
                    ) {
                        Ok(space) => space,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create VIEW space: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Create STAGE reference space (world-fixed)
                    let stage_space = match xr_session.session.create_reference_space(
                        xr::ReferenceSpaceType::STAGE,
                        xr::Posef::IDENTITY,
                    ) {
                        Ok(space) => space,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to create STAGE space: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    // Get HMD pose: locate VIEW origin relative to STAGE
                    // VIEW origin is at the current HMD position/orientation
                    let time = xr_session.now();
                    let location = match view_space.locate(&stage_space, time) {
                        Ok(loc) => loc,
                        Err(e) => {
                            let _ = msg_tx.send(CalibrationMessage::Error(
                                format!("Failed to locate HMD: {:?}", e)
                            ));
                            continue;
                        }
                    };

                    let pos_valid = location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID);
                    let ori_valid = location.location_flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID);

                    if !pos_valid || !ori_valid {
                        let _ = msg_tx.send(CalibrationMessage::Error(
                            format!("HMD pose not valid (pos={}, ori={})", pos_valid, ori_valid)
                        ));
                        continue;
                    }

                    // Get full HMD pose in STAGE coords
                    let position = [
                        location.pose.position.x,
                        location.pose.position.y,
                        location.pose.position.z,
                    ];
                    let orientation = [
                        location.pose.orientation.x,
                        location.pose.orientation.y,
                        location.pose.orientation.z,
                        location.pose.orientation.w,
                    ];

                    // Send full HMD pose - monado.rs will handle the transform math
                    let _ = msg_tx.send(CalibrationMessage::RecenterComplete {
                        position,
                        orientation,
                    });
                }
            }
        }
    }
}
