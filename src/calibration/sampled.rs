#![allow(dead_code)]
//! Sampled calibration algorithm
//!
//! Implements the math from OpenVR-SpaceCalibrator by pushrax
//! https://github.com/pushrax/OpenVR-SpaceCalibrator/blob/master/math.pdf
//!
//! The algorithm finds the transformation between two tracking spaces by observing
//! how two devices (held together) move over time. By comparing the rotation axes
//! of their movements, we can find the rotation that aligns the coordinate systems.

use nalgebra::{Dyn, Matrix3, OMatrix, Rotation3, RowVector3, Vector3, U1, U3};
use crate::calibration::transform::TransformD;
use crate::error::CalibrationError;

/// A single pose sample from both source and target devices
#[derive(Debug, Clone)]
pub struct PoseSample {
    pub source_position: Vector3<f64>,
    pub source_orientation: nalgebra::UnitQuaternion<f64>,
    pub target_position: Vector3<f64>,
    pub target_orientation: nalgebra::UnitQuaternion<f64>,
}

impl PoseSample {
    /// Create a new pose sample from raw position and orientation data
    pub fn new(
        source_position: Vector3<f64>,
        source_orientation: nalgebra::UnitQuaternion<f64>,
        target_position: Vector3<f64>,
        target_orientation: nalgebra::UnitQuaternion<f64>,
    ) -> Self {
        Self {
            source_position,
            source_orientation,
            target_position,
            target_orientation,
        }
    }

    /// Create from OpenXR pose data (f32 arrays converted to f64)
    pub fn from_xr_poses(
        source_pos: [f32; 3],
        source_ori: [f32; 4],
        target_pos: [f32; 3],
        target_ori: [f32; 4],
    ) -> Self {
        let source_position = Vector3::new(
            source_pos[0] as f64,
            source_pos[1] as f64,
            source_pos[2] as f64,
        );
        let target_position = Vector3::new(
            target_pos[0] as f64,
            target_pos[1] as f64,
            target_pos[2] as f64,
        );

        // OpenXR quaternions are [x, y, z, w]
        let source_orientation = nalgebra::UnitQuaternion::from_quaternion(
            nalgebra::Quaternion::new(
                source_ori[3] as f64, // w
                source_ori[0] as f64, // x
                source_ori[1] as f64, // y
                source_ori[2] as f64, // z
            ),
        );
        let target_orientation = nalgebra::UnitQuaternion::from_quaternion(
            nalgebra::Quaternion::new(
                target_ori[3] as f64, // w
                target_ori[0] as f64, // x
                target_ori[1] as f64, // y
                target_ori[2] as f64, // z
            ),
        );

        Self {
            source_position,
            source_orientation,
            target_position,
            target_orientation,
        }
    }

    /// Validate that the sample contains reasonable values
    pub fn validate(&self) -> Result<(), CalibrationError> {
        // Check for NaN or infinite values
        if !self.source_position.iter().all(|&v| v.is_finite()) {
            return Err(CalibrationError::InvalidPoseData(
                "Source position contains NaN or infinite values".to_string(),
            ));
        }
        if !self.target_position.iter().all(|&v| v.is_finite()) {
            return Err(CalibrationError::InvalidPoseData(
                "Target position contains NaN or infinite values".to_string(),
            ));
        }

        // Check that quaternions are normalized
        let source_norm = (self.source_orientation.w.powi(2)
            + self.source_orientation.i.powi(2)
            + self.source_orientation.j.powi(2)
            + self.source_orientation.k.powi(2))
        .sqrt();
        if (source_norm - 1.0).abs() > 1e-6 {
            return Err(CalibrationError::InvalidPoseData(
                "Source quaternion is not normalized".to_string(),
            ));
        }

        let target_norm = (self.target_orientation.w.powi(2)
            + self.target_orientation.i.powi(2)
            + self.target_orientation.j.powi(2)
            + self.target_orientation.k.powi(2))
        .sqrt();
        if (target_norm - 1.0).abs() > 1e-6 {
            return Err(CalibrationError::InvalidPoseData(
                "Target quaternion is not normalized".to_string(),
            ));
        }

        Ok(())
    }
}

/// Internal sample representation using TransformD
#[derive(Clone)]
struct Sample {
    a: TransformD, // source
    b: TransformD, // target
}

impl Sample {
    fn from_pose_sample(ps: &PoseSample) -> Self {
        Self {
            a: TransformD::from_position_orientation(ps.source_position, ps.source_orientation),
            b: TransformD::from_position_orientation(ps.target_position, ps.target_orientation),
        }
    }
}

/// Delta rotation sample - rotation axes extracted from movement between two samples
struct DeltaRotSample {
    a: RowVector3<f64>, // Normalized rotation axis for source
    b: RowVector3<f64>, // Normalized rotation axis for target
}

impl DeltaRotSample {
    /// Create a delta rotation sample from two consecutive pose samples
    /// Returns None if the rotation is too small (noise filtering)
    fn new(new: &Sample, old: &Sample) -> Option<Self> {
        // Compute delta rotations: how each device rotated between samples
        let delta_a = new.a.basis * old.a.basis.transpose();
        let delta_b = new.b.basis * old.b.basis.transpose();

        // Extract angle and axis from rotation matrices
        let angle_a = angle_from_rotation_matrix(delta_a.matrix());
        let angle_b = angle_from_rotation_matrix(delta_b.matrix());

        let axis_a = axis_from_rotation_matrix(delta_a.matrix());
        let axis_b = axis_from_rotation_matrix(delta_b.matrix());

        // Filter out small rotations (these are noise)
        // Threshold: ~23 degrees (0.4 radians)
        if angle_a < 0.4
            || angle_b < 0.4
            || axis_a.norm_squared() < 0.1
            || axis_b.norm_squared() < 0.1
        {
            None
        } else {
            Some(Self {
                a: axis_a.normalize(),
                b: axis_b.normalize(),
            })
        }
    }
}

/// Extract rotation axis from rotation matrix (skew-symmetric part)
/// The axis is scaled by sin(angle), so it needs normalization
fn axis_from_rotation_matrix(mat: &Matrix3<f64>) -> RowVector3<f64> {
    // For a rotation matrix R, the skew-symmetric part (R - R^T) / 2 gives
    // a matrix whose off-diagonal elements encode the rotation axis scaled by sin(angle)
    RowVector3::new(
        mat[(2, 1)] - mat[(1, 2)],
        mat[(0, 2)] - mat[(2, 0)],
        mat[(1, 0)] - mat[(0, 1)],
    )
}

/// Extract rotation angle from rotation matrix using trace formula
/// cos(angle) = (trace(R) - 1) / 2
fn angle_from_rotation_matrix(mat: &Matrix3<f64>) -> f64 {
    let trace = mat[(0, 0)] + mat[(1, 1)] + mat[(2, 2)];
    let cos_angle = ((trace - 1.0) / 2.0).clamp(-1.0, 1.0);
    cos_angle.acos()
}

/// Collected samples for calibration
pub struct SampleCollector {
    samples: Vec<PoseSample>,
    target_count: u32,
}

impl SampleCollector {
    /// Create a new sample collector with a target number of samples
    pub fn new(target_count: u32) -> Self {
        Self {
            samples: Vec::with_capacity(target_count as usize),
            target_count,
        }
    }

    /// Add a sample to the collection
    /// Returns an error if the sample is invalid
    pub fn add_sample(&mut self, sample: PoseSample) -> Result<(), CalibrationError> {
        sample.validate()?;
        self.samples.push(sample);
        Ok(())
    }

    /// Add a sample without validation (use with caution)
    pub fn add_sample_unchecked(&mut self, sample: PoseSample) {
        self.samples.push(sample);
    }

    /// Get the current number of collected samples
    pub fn sample_count(&self) -> u32 {
        self.samples.len() as u32
    }

    /// Check if the target number of samples has been collected
    pub fn is_complete(&self) -> bool {
        self.samples.len() >= self.target_count as usize
    }

    /// Get the progress as (collected, target) tuple
    pub fn progress(&self) -> (u32, u32) {
        (self.sample_count(), self.target_count)
    }

    /// Clear all collected samples
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Get a reference to the collected samples
    pub fn samples(&self) -> &[PoseSample] {
        &self.samples
    }

    /// Compute the calibration transform from collected samples
    ///
    /// Uses the OpenVR-SpaceCalibrator algorithm:
    /// 1. Extract delta rotations (how each device rotates between sample pairs)
    /// 2. Use Kabsch algorithm on rotation axes to find the rotation alignment
    /// 3. Solve a linear system for the translation
    ///
    /// Returns offset O where O × Target = Source
    pub fn compute_calibration(&self) -> Result<TransformD, CalibrationError> {
        if self.samples.len() < 3 {
            return Err(CalibrationError::InsufficientSamples {
                required: 3,
                collected: self.samples.len(),
            });
        }

        // Convert to internal sample format
        let samples: Vec<Sample> = self.samples.iter().map(Sample::from_pose_sample).collect();

        // Step 1: Compute rotation using Kabsch on rotation axes
        let rotation = calibrate_rotation(&samples)?;

        // Step 2: Compute translation using linear system
        let translation = calibrate_translation(&samples, &rotation)?;

        Ok(TransformD {
            basis: rotation,
            origin: translation,
        })
    }
}

/// Compute optimal rotation using Kabsch algorithm on rotation axes
///
/// For each pair of samples (i, j), we compute:
/// - delta_a: how source device rotated from sample i to sample j
/// - delta_b: how target device rotated from sample i to sample j
///
/// We extract the rotation axes from these delta rotations and use Kabsch
/// to find the rotation R that best aligns the source axes to target axes.
fn calibrate_rotation(samples: &[Sample]) -> Result<Rotation3<f64>, CalibrationError> {
    // Collect delta rotation samples from all pairs
    let mut deltas = Vec::with_capacity(samples.len() * samples.len() / 2);

    for i in 0..samples.len() {
        for j in 0..i {
            if let Some(delta) = DeltaRotSample::new(&samples[i], &samples[j]) {
                deltas.push(delta);
            }
        }
    }

    if deltas.is_empty() {
        return Err(CalibrationError::InsufficientSamples {
            required: 1,
            collected: 0,
        });
    }

    // Compute centroids of rotation axis point clouds
    let mut a_centroid = RowVector3::zeros();
    let mut b_centroid = RowVector3::zeros();

    for d in deltas.iter() {
        a_centroid += d.a;
        b_centroid += d.b;
    }

    let len_recip = 1.0 / deltas.len() as f64;
    a_centroid *= len_recip;
    b_centroid *= len_recip;

    // Center the point clouds
    let mut a_points = OMatrix::<f64, Dyn, U3>::zeros(deltas.len());
    let mut b_points = OMatrix::<f64, Dyn, U3>::zeros(deltas.len());

    for (i, d) in deltas.iter().enumerate() {
        a_points.set_row(i, &(d.a - a_centroid));
        b_points.set_row(i, &(d.b - b_centroid));
    }

    // Compute cross-covariance matrix: H = A^T × B
    let cross_cv = a_points.transpose() * b_points;

    // SVD decomposition
    let svd = cross_cv.svd(true, true);
    let u = svd.u.ok_or(CalibrationError::SvdFailed)?;
    let v = svd.v_t.ok_or(CalibrationError::SvdFailed)?.transpose();

    // Compute rotation with determinant correction to ensure proper rotation (not reflection)
    let mut i_mat = Matrix3::identity();
    if (u * v.transpose()).determinant() < 0.0 {
        i_mat[(2, 2)] = -1.0;
    }

    let rot = v * i_mat * u.transpose();
    let rot = rot.transpose();

    Ok(Rotation3::from_matrix_unchecked(rot))
}

/// Compute translation using a least-squares linear system
///
/// After finding the rotation R, we solve for translation t such that:
/// R × target_pose + t ≈ source_pose for all samples
///
/// This is done by building an overdetermined linear system and solving with SVD.
fn calibrate_translation(samples: &[Sample], rot: &Rotation3<f64>) -> Result<Vector3<f64>, CalibrationError> {
    let mut deltas: Vec<(Vector3<f64>, Matrix3<f64>)> = Vec::with_capacity(samples.len() * samples.len());

    for i in 0..samples.len() {
        // Apply rotation to target poses
        let mut si = samples[i].clone();
        si.b.basis = rot * si.b.basis;
        si.b.origin = rot * si.b.origin;

        for j in 0..i {
            let mut sj = samples[j].clone();
            sj.b.basis = rot * sj.b.basis;
            sj.b.origin = rot * sj.b.origin;

            // Build equations from source device
            let rot_a_i = si.a.basis.transpose();
            let rot_a_j = sj.a.basis.transpose();
            let delta_rot_a: Matrix3<f64> = rot_a_j.matrix() - rot_a_i.matrix();

            let ca = rot_a_j * (sj.a.origin - sj.b.origin) - rot_a_i * (si.a.origin - si.b.origin);
            deltas.push((ca, delta_rot_a));

            // Build equations from target device
            let rot_b_i = si.b.basis.transpose();
            let rot_b_j = sj.b.basis.transpose();
            let delta_rot_b: Matrix3<f64> = rot_b_j.matrix() - rot_b_i.matrix();

            let cb = rot_b_j * (sj.a.origin - sj.b.origin) - rot_b_i * (si.a.origin - si.b.origin);
            deltas.push((cb, delta_rot_b));
        }
    }

    if deltas.is_empty() {
        return Err(CalibrationError::InsufficientSamples {
            required: 1,
            collected: 0,
        });
    }

    // Build the linear system: coeffs × t = constants
    let mut constants = OMatrix::<f64, Dyn, U1>::zeros(deltas.len() * 3);
    let mut coeffs = OMatrix::<f64, Dyn, U3>::zeros(deltas.len() * 3);

    for (i, (c, delta_rot)) in deltas.iter().enumerate() {
        for axis in 0..3 {
            constants[i * 3 + axis] = c[axis];
            coeffs.set_row(i * 3 + axis, &delta_rot.row(axis));
        }
    }

    // Solve using SVD least-squares
    coeffs
        .svd(true, true)
        .solve(&constants, f32::EPSILON as f64)
        .map_err(|_| CalibrationError::SvdFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_calibration() {
        // If source == target with same orientations and movements, should get near-identity transform
        let mut collector = SampleCollector::new(10);

        // Create samples with rotation (needed for delta rotation detection)
        for i in 0..10 {
            let angle = i as f64 * 0.5; // Rotate over time
            let rotation = nalgebra::UnitQuaternion::from_euler_angles(angle, 0.0, 0.0);
            let pos = Vector3::new(i as f64 * 0.1, 0.0, 0.0);

            collector.add_sample_unchecked(PoseSample {
                source_position: pos,
                source_orientation: rotation,
                target_position: pos,
                target_orientation: rotation,
            });
        }

        let result = collector.compute_calibration().unwrap();
        // Should be near identity
        assert!(result.origin.norm() < 0.1, "Translation too large: {:?}", result.origin);
    }

    #[test]
    fn test_pure_rotation_offset() {
        // Test where target coordinate system is rotated 90 degrees around Y axis
        let mut collector = SampleCollector::new(20);

        // The offset rotation we want to find
        let offset_rotation = nalgebra::UnitQuaternion::from_euler_angles(0.0, std::f64::consts::FRAC_PI_2, 0.0);
        let offset_rot3 = Rotation3::from(offset_rotation);

        // Simulate movement with various orientations
        for i in 0..20 {
            let t = i as f64 * 0.2;
            let source_rotation = nalgebra::UnitQuaternion::from_euler_angles(
                t.sin() * 0.5,
                t.cos() * 0.3,
                t * 0.2
            );
            let source_pos = Vector3::new(t.cos() * 0.5, t.sin() * 0.3, t * 0.1);

            // Target sees the same physical pose but in its rotated coordinate system
            // O × T = S, so T = O^-1 × S
            let target_rotation = offset_rotation.inverse() * source_rotation;
            let target_pos = offset_rot3.inverse() * source_pos;

            collector.add_sample_unchecked(PoseSample {
                source_position: source_pos,
                source_orientation: source_rotation,
                target_position: target_pos,
                target_orientation: target_rotation,
            });
        }

        let result = collector.compute_calibration().unwrap();

        // Check that the computed rotation matches the offset
        let result_quat = nalgebra::UnitQuaternion::from_rotation_matrix(&result.basis);
        let dot = offset_rotation.quaternion().dot(result_quat.quaternion()).abs();
        assert!(dot > 0.99, "Rotation mismatch, dot product: {}", dot);

        // Translation should be near zero (no translation offset in this test)
        assert!(result.origin.norm() < 0.1, "Translation too large: {:?}", result.origin);
    }

    #[test]
    fn test_combined_offset() {
        // Test with both rotation and translation offset
        let mut collector = SampleCollector::new(30);

        // The offset we want to find
        let offset_rotation = nalgebra::UnitQuaternion::from_euler_angles(0.3, 0.5, 0.2);
        let offset_rot3 = Rotation3::from(offset_rotation);
        let offset_translation = Vector3::new(1.0, -0.5, 2.0);

        // Simulate movement
        for i in 0..30 {
            let t = i as f64 * 0.15;
            let source_rotation = nalgebra::UnitQuaternion::from_euler_angles(
                t.sin() * 0.6,
                t.cos() * 0.4,
                (t * 1.5).sin() * 0.3
            );
            let source_pos = Vector3::new(
                t.cos() * 0.8,
                t.sin() * 0.6 + 1.5,
                (t * 0.5).cos() * 0.4
            );

            // Target sees the same physical pose but in its offset coordinate system
            // O × T = S, so T = O^-1 × S
            // T.rotation = O.rotation^-1 × S.rotation
            // T.position = O.rotation^-1 × (S.position - O.translation)
            let target_rotation = offset_rotation.inverse() * source_rotation;
            let target_pos = offset_rot3.inverse() * (source_pos - offset_translation);

            collector.add_sample_unchecked(PoseSample {
                source_position: source_pos,
                source_orientation: source_rotation,
                target_position: target_pos,
                target_orientation: target_rotation,
            });
        }

        let result = collector.compute_calibration().unwrap();

        // Check rotation
        let result_quat = nalgebra::UnitQuaternion::from_rotation_matrix(&result.basis);
        let dot = offset_rotation.quaternion().dot(result_quat.quaternion()).abs();
        assert!(dot > 0.95, "Rotation mismatch, dot product: {}", dot);

        // Check translation
        let translation_error = (result.origin - offset_translation).norm();
        assert!(translation_error < 0.5, "Translation error: {}", translation_error);
    }

    #[test]
    fn test_insufficient_samples() {
        let mut collector = SampleCollector::new(10);

        // Add only 2 samples (need at least 3)
        for i in 0..2 {
            let pos = Vector3::new(i as f64, 0.0, 0.0);
            collector.add_sample_unchecked(PoseSample {
                source_position: pos,
                source_orientation: nalgebra::UnitQuaternion::identity(),
                target_position: pos,
                target_orientation: nalgebra::UnitQuaternion::identity(),
            });
        }

        let result = collector.compute_calibration();
        assert!(matches!(result, Err(CalibrationError::InsufficientSamples { .. })));
    }

    #[test]
    fn test_sample_validation() {
        let mut collector = SampleCollector::new(1);

        // Invalid sample with NaN
        let invalid_sample = PoseSample {
            source_position: Vector3::new(f64::NAN, 0.0, 0.0),
            source_orientation: nalgebra::UnitQuaternion::identity(),
            target_position: Vector3::zeros(),
            target_orientation: nalgebra::UnitQuaternion::identity(),
        };

        let result = collector.add_sample(invalid_sample);
        assert!(matches!(result, Err(CalibrationError::InvalidPoseData(_))));
    }

    #[test]
    fn test_from_xr_poses() {
        let source_pos = [1.0f32, 2.0f32, 3.0f32];
        let source_ori = [0.0f32, 0.0f32, 0.0f32, 1.0f32]; // Identity [x,y,z,w]
        let target_pos = [4.0f32, 5.0f32, 6.0f32];
        let target_ori = [0.0f32, 0.0f32, 0.0f32, 1.0f32];

        let sample = PoseSample::from_xr_poses(source_pos, source_ori, target_pos, target_ori);

        assert!((sample.source_position.x - 1.0).abs() < 1e-6);
        assert!((sample.source_position.y - 2.0).abs() < 1e-6);
        assert!((sample.source_position.z - 3.0).abs() < 1e-6);
        assert!((sample.target_position.x - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_progress_tracking() {
        let mut collector = SampleCollector::new(5);

        assert_eq!(collector.progress(), (0, 5));
        assert!(!collector.is_complete());

        for i in 0..5 {
            let pos = Vector3::new(i as f64, 0.0, 0.0);
            collector.add_sample_unchecked(PoseSample {
                source_position: pos,
                source_orientation: nalgebra::UnitQuaternion::identity(),
                target_position: pos,
                target_orientation: nalgebra::UnitQuaternion::identity(),
            });
        }

        assert_eq!(collector.progress(), (5, 5));
        assert!(collector.is_complete());
    }
}
