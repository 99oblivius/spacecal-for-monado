//! Live tracking: incremental origin adjustment after initial sampled calibration.

use crate::calibration::TransformD;
use crate::calibration::sampled::PoseSample;

pub const MOTION_GATE: f32 = 0.7;
pub const LERP_FACTOR: f64 = 0.15;

pub fn compute_rigid_offset(samples: &[PoseSample], calibration: &TransformD) -> Option<TransformD> {
    let offsets = samples.iter().map(|s| {
        let source = TransformD::from_position_orientation(s.source_position, s.source_orientation);
        let target = TransformD::from_position_orientation(s.target_position, s.target_orientation);
        &source.inverse() * &(calibration * &target)
    });
    TransformD::average(offsets)
}

pub struct ContinuousTracker {
    offset: TransformD,
}

impl ContinuousTracker {
    pub fn new(offset: TransformD) -> Self {
        Self { offset }
    }

    pub fn tick(
        &mut self,
        source_pose: TransformD,
        target_pose: TransformD,
        source_speed: f32,
        target_speed: f32,
    ) -> Option<TransformD> {
        if source_speed > MOTION_GATE || target_speed > MOTION_GATE {
            return None;
        }

        Some(&source_pose * &(&self.offset * &target_pose.inverse()))
    }
}

pub fn apply_correction(origin: &mut TransformD, error: &TransformD) {
    use nalgebra::UnitQuaternion;

    origin.origin += error.origin * LERP_FACTOR;

    let error_q = UnitQuaternion::from_rotation_matrix(&error.basis);
    let partial = UnitQuaternion::identity().slerp(&error_q, LERP_FACTOR);
    origin.basis = partial.to_rotation_matrix() * origin.basis;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{UnitQuaternion, Vector3};

    fn identity_sample() -> PoseSample {
        PoseSample {
            source_position: Vector3::zeros(),
            source_orientation: UnitQuaternion::identity(),
            target_position: Vector3::zeros(),
            target_orientation: UnitQuaternion::identity(),
        }
    }

    fn sample_at(source_pos: Vector3<f64>, target_pos: Vector3<f64>) -> PoseSample {
        PoseSample {
            source_position: source_pos,
            source_orientation: UnitQuaternion::identity(),
            target_position: target_pos,
            target_orientation: UnitQuaternion::identity(),
        }
    }

    #[test]
    fn rigid_offset_identity_calibration() {
        let samples = vec![
            identity_sample(),
            sample_at(Vector3::new(1.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        ];
        let s = compute_rigid_offset(&samples, &TransformD::identity()).unwrap();
        assert!(s.approx_eq(&TransformD::identity(), 1e-10));
    }

    #[test]
    fn rigid_offset_with_translation() {
        let offset = Vector3::new(1.0, 0.0, 0.0);
        let samples = vec![
            sample_at(Vector3::new(1.0, 0.0, 0.0), Vector3::zeros()),
            sample_at(Vector3::new(2.0, 1.0, 0.0), Vector3::new(1.0, 1.0, 0.0)),
            sample_at(Vector3::new(3.0, 2.0, 1.0), Vector3::new(2.0, 2.0, 1.0)),
        ];

        let s = compute_rigid_offset(&samples, &TransformD::identity()).unwrap();
        let expected = TransformD::from_translation(-offset);
        assert!(s.approx_eq(&expected, 1e-10));
    }

    #[test]
    fn rigid_offset_empty_samples() {
        assert!(compute_rigid_offset(&[], &TransformD::identity()).is_none());
    }

    #[test]
    fn velocity_gate_rejects_fast_source() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let result = cal.tick(
            TransformD::identity(), TransformD::identity(),
            0.8, 0.0,
        );
        assert!(result.is_none());
    }

    #[test]
    fn velocity_gate_rejects_fast_target() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let result = cal.tick(
            TransformD::identity(), TransformD::identity(),
            0.0, 0.8,
        );
        assert!(result.is_none());
    }

    #[test]
    fn velocity_gate_accepts_slow() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let result = cal.tick(
            TransformD::identity(), TransformD::identity(),
            0.5, 0.5,
        );
        assert!(result.is_some());
    }

    #[test]
    fn accepts_large_position_delta() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let result = cal.tick(
            TransformD::from_translation(Vector3::new(5.0, 0.0, 0.0)),
            TransformD::identity(),
            0.0, 0.0,
        );
        assert!(result.is_some());
    }

    #[test]
    fn accepts_large_rotation_delta() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let rotated = TransformD::from_position_orientation(
            Vector3::zeros(),
            UnitQuaternion::from_euler_angles(0.0, 1.5, 0.0), // ~86°
        );
        let result = cal.tick(rotated, TransformD::identity(), 0.0, 0.0);
        assert!(result.is_some());
    }

    #[test]
    fn no_drift_produces_identity_delta() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let delta = cal.tick(
            TransformD::identity(), TransformD::identity(),
            0.0, 0.0,
        ).unwrap();
        assert!(delta.approx_eq(&TransformD::identity(), 1e-10));
    }

    #[test]
    fn drift_produces_matching_delta() {
        let mut cal = ContinuousTracker::new(TransformD::identity());
        let drift = Vector3::new(0.1, 0.0, 0.0);
        let delta = cal.tick(
            TransformD::from_translation(drift),
            TransformD::identity(),
            0.0, 0.0,
        ).unwrap();
        assert!((delta.origin - drift).norm() < 1e-10);
    }

    #[test]
    fn apply_correction_incremental_position() {
        let mut origin = TransformD::identity();
        let delta = TransformD::from_translation(Vector3::new(1.0, 0.0, 0.0));

        apply_correction(&mut origin, &delta);

        assert!((origin.origin.x - LERP_FACTOR).abs() < 1e-10);
    }

    #[test]
    fn apply_correction_converges() {
        let mut origin = TransformD::identity();
        let delta = TransformD::from_translation(Vector3::new(0.5, 0.0, 0.0));

        for _ in 0..200 {
            apply_correction(&mut origin, &delta);
        }

        let expected = 200.0 * LERP_FACTOR * 0.5;
        assert!((origin.origin.x - expected).abs() < 1e-10);
    }

    #[test]
    fn apply_correction_rotation() {
        let mut origin = TransformD::identity();
        let delta = TransformD::from_position_orientation(
            Vector3::zeros(),
            UnitQuaternion::from_euler_angles(0.0, 0.1, 0.0),
        );

        apply_correction(&mut origin, &delta);

        let angle = origin.rotation_angle();
        assert!(angle > 0.001);
        assert!(angle < 0.1 * LERP_FACTOR * 2.0);
    }
}
