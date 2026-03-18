use nalgebra::{Rotation3, UnitQuaternion, Vector3};

/// Double-precision 3D rigid transform (rotation + translation).
#[derive(Debug, Clone)]
pub struct TransformD {
    pub basis: Rotation3<f64>,
    pub origin: Vector3<f64>,
}

impl TransformD {
    pub fn identity() -> Self {
        Self {
            basis: Rotation3::identity(),
            origin: Vector3::zeros(),
        }
    }

    pub fn from_position_orientation(position: Vector3<f64>, orientation: UnitQuaternion<f64>) -> Self {
        Self {
            basis: orientation.to_rotation_matrix(),
            origin: position,
        }
    }

    /// Convert from OpenXR pose (f32 → f64). Quaternion order: [x, y, z, w].
    pub fn from_xr_pose(position: [f32; 3], orientation: [f32; 4]) -> Self {
        let pos = Vector3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        let quat = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            orientation[3] as f64,
            orientation[0] as f64,
            orientation[1] as f64,
            orientation[2] as f64,
        ));
        Self::from_position_orientation(pos, quat)
    }

    pub fn inverse(&self) -> Self {
        let inv_basis = self.basis.inverse();
        Self {
            origin: inv_basis * (-self.origin),
            basis: inv_basis,
        }
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self {
            basis: self.basis * other.basis,
            origin: self.origin + self.basis * other.origin,
        }
    }

    pub fn transform_point(&self, point: &Vector3<f64>) -> Vector3<f64> {
        self.basis * point + self.origin
    }

    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        let pos = self.origin.lerp(&other.origin, t);
        let quat_self = UnitQuaternion::from_rotation_matrix(&self.basis);
        let quat_other = UnitQuaternion::from_rotation_matrix(&other.basis);
        let quat = quat_self.slerp(&quat_other, t);
        Self::from_position_orientation(pos, quat)
    }

    pub fn position_f32(&self) -> [f32; 3] {
        [self.origin.x as f32, self.origin.y as f32, self.origin.z as f32]
    }

    /// Quaternion as [x, y, z, w] f32 for OpenXR.
    pub fn orientation_f32(&self) -> [f32; 4] {
        let quat = UnitQuaternion::from_rotation_matrix(&self.basis);
        [quat.i as f32, quat.j as f32, quat.k as f32, quat.w as f32]
    }

    pub fn position_f64(&self) -> [f64; 3] {
        [self.origin.x, self.origin.y, self.origin.z]
    }

    /// Quaternion as [x, y, z, w] f64 for libmonado.
    pub fn orientation_f64(&self) -> [f64; 4] {
        let quat = UnitQuaternion::from_rotation_matrix(&self.basis);
        [quat.i, quat.j, quat.k, quat.w]
    }

    pub fn from_translation(translation: Vector3<f64>) -> Self {
        Self {
            basis: Rotation3::identity(),
            origin: translation,
        }
    }

    pub fn rotation_quaternion(&self) -> UnitQuaternion<f64> {
        UnitQuaternion::from_rotation_matrix(&self.basis)
    }

    /// Compares transforms allowing for quaternion double-cover (q and -q are the same rotation).
    pub fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        let pos_diff = (self.origin - other.origin).norm();
        if pos_diff > epsilon {
            return false;
        }

        let q1 = self.rotation_quaternion();
        let q2 = other.rotation_quaternion();
        let dot = q1.w * q2.w + q1.i * q2.i + q1.j * q2.j + q1.k * q2.k;
        dot.abs() > 1.0 - epsilon
    }

    pub fn euler_angles(&self) -> (f64, f64, f64) {
        self.basis.euler_angles()
    }

    pub fn rotation_angle(&self) -> f64 {
        self.rotation_quaternion().angle()
    }

    /// Mean position + quaternion-averaged rotation.
    pub fn average<I>(transforms: I) -> Option<Self>
    where
        I: IntoIterator<Item = Self>,
    {
        let transforms_vec: Vec<Self> = transforms.into_iter().collect();
        if transforms_vec.is_empty() {
            return None;
        }

        let avg_pos = transforms_vec
            .iter()
            .fold(Vector3::zeros(), |acc, t| acc + t.origin)
            / transforms_vec.len() as f64;

        let quaternions: Vec<UnitQuaternion<f64>> = transforms_vec
            .iter()
            .map(|t| UnitQuaternion::from_rotation_matrix(&t.basis))
            .collect();

        let avg_quat = average_quaternions(&quaternions)?;

        Some(Self::from_position_orientation(avg_pos, avg_quat))
    }
}

impl Default for TransformD {
    fn default() -> Self {
        Self::identity()
    }
}

impl std::ops::Mul for TransformD {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        TransformD::mul(&self, &rhs)
    }
}

impl std::ops::Mul<&TransformD> for &TransformD {
    type Output = TransformD;
    fn mul(self, rhs: &TransformD) -> Self::Output {
        TransformD::mul(self, rhs)
    }
}

/// Iterative quaternion mean, handling the double-cover problem (q and -q are the same rotation).
pub fn average_quaternions(quaternions: &[UnitQuaternion<f64>]) -> Option<UnitQuaternion<f64>> {
    if quaternions.is_empty() {
        return None;
    }

    if quaternions.len() == 1 {
        return Some(quaternions[0]);
    }

    let mut avg = quaternions[0];

    for _ in 0..5 {
        let mut sum = nalgebra::Vector4::zeros();

        for q in quaternions {
            let dot = avg.w * q.w + avg.i * q.i + avg.j * q.j + avg.k * q.k;
            let sign = if dot < 0.0 { -1.0 } else { 1.0 };

            sum.x += sign * q.i;
            sum.y += sign * q.j;
            sum.z += sign * q.k;
            sum.w += sign * q.w;
        }

        sum /= quaternions.len() as f64;
        let norm = sum.norm();

        if norm < 1e-10 {
            break;
        }

        sum /= norm;
        avg = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(sum.w, sum.x, sum.y, sum.z));
    }

    Some(avg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity() {
        let t = TransformD::identity();
        let point = Vector3::new(1.0, 2.0, 3.0);
        let transformed = t.transform_point(&point);
        assert!((transformed - point).norm() < 1e-10);
    }

    #[test]
    fn test_inverse() {
        let pos = Vector3::new(1.0, 2.0, 3.0);
        let quat = UnitQuaternion::from_euler_angles(0.1, 0.2, 0.3);
        let t = TransformD::from_position_orientation(pos, quat);
        let inv = t.inverse();
        let identity = t.mul(&inv);

        assert!(identity.approx_eq(&TransformD::identity(), 1e-10));
    }

    #[test]
    fn test_composition() {
        let t1 = TransformD::from_translation(Vector3::new(1.0, 0.0, 0.0));
        let t2 = TransformD::from_translation(Vector3::new(0.0, 1.0, 0.0));
        let t3 = t1.mul(&t2);

        let expected = Vector3::new(1.0, 1.0, 0.0);
        assert!((t3.origin - expected).norm() < 1e-10);
    }

    #[test]
    fn test_xr_pose_conversion() {
        let pos = [1.0f32, 2.0f32, 3.0f32];
        let ori = [0.0f32, 0.0f32, 0.0f32, 1.0f32]; // Identity quaternion [x,y,z,w]

        let t = TransformD::from_xr_pose(pos, ori);
        let back_pos = t.position_f32();
        let back_ori = t.orientation_f32();

        assert!((back_pos[0] - pos[0]).abs() < 1e-6);
        assert!((back_pos[1] - pos[1]).abs() < 1e-6);
        assert!((back_pos[2] - pos[2]).abs() < 1e-6);
        assert!((back_ori[3] - ori[3]).abs() < 1e-6); // w component
    }

    #[test]
    fn test_lerp() {
        let t1 = TransformD::from_translation(Vector3::new(0.0, 0.0, 0.0));
        let t2 = TransformD::from_translation(Vector3::new(10.0, 0.0, 0.0));

        let mid = t1.lerp(&t2, 0.5);
        assert!((mid.origin.x - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_average() {
        let t1 = TransformD::from_translation(Vector3::new(0.0, 0.0, 0.0));
        let t2 = TransformD::from_translation(Vector3::new(10.0, 0.0, 0.0));
        let t3 = TransformD::from_translation(Vector3::new(5.0, 0.0, 0.0));

        let avg = TransformD::average(vec![t1, t2, t3]).unwrap();
        assert!((avg.origin.x - 5.0).abs() < 1e-10);
    }
}
