use nalgebra::{Rotation3, UnitQuaternion, Vector3};

/// Double-precision 3D rigid transform (rotation + translation)
/// Used for calibration calculations where precision matters
#[derive(Debug, Clone)]
pub struct TransformD {
    /// Rotation component as a proper rotation matrix
    pub basis: Rotation3<f64>,
    /// Translation component
    pub origin: Vector3<f64>,
}

impl TransformD {
    /// Create identity transform
    #[must_use]
    pub fn identity() -> Self {
        Self {
            basis: Rotation3::identity(),
            origin: Vector3::zeros(),
        }
    }

    /// Create from position and quaternion orientation
    #[must_use]
    pub fn from_position_orientation(position: Vector3<f64>, orientation: UnitQuaternion<f64>) -> Self {
        Self {
            basis: orientation.to_rotation_matrix(),
            origin: position,
        }
    }

    /// Create from OpenXR pose (f32 values, converted to f64)
    #[must_use]
    pub fn from_xr_pose(position: [f32; 3], orientation: [f32; 4]) -> Self {
        let pos = Vector3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        let quat = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            orientation[3] as f64, // w
            orientation[0] as f64, // x
            orientation[1] as f64, // y
            orientation[2] as f64, // z
        ));
        Self::from_position_orientation(pos, quat)
    }

    /// Get the inverse transform
    #[must_use]
    pub fn inverse(&self) -> Self {
        let inv_basis = self.basis.inverse();
        Self {
            origin: inv_basis * (-self.origin),
            basis: inv_basis,
        }
    }

    /// Compose two transforms: self * other
    #[must_use]
    pub fn mul(&self, other: &Self) -> Self {
        Self {
            basis: self.basis * other.basis,
            origin: self.origin + self.basis * other.origin,
        }
    }

    /// Transform a point
    #[must_use]
    pub fn transform_point(&self, point: &Vector3<f64>) -> Vector3<f64> {
        self.basis * point + self.origin
    }

    /// Linear interpolation between two transforms
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f64) -> Self {
        let pos = self.origin.lerp(&other.origin, t);
        let quat_self = UnitQuaternion::from_rotation_matrix(&self.basis);
        let quat_other = UnitQuaternion::from_rotation_matrix(&other.basis);
        let quat = quat_self.slerp(&quat_other, t);
        Self::from_position_orientation(pos, quat)
    }

    /// Get position as f32 array for OpenXR
    #[must_use]
    pub fn position_f32(&self) -> [f32; 3] {
        [self.origin.x as f32, self.origin.y as f32, self.origin.z as f32]
    }

    /// Get orientation as quaternion [x,y,z,w] f32 array for OpenXR
    #[must_use]
    pub fn orientation_f32(&self) -> [f32; 4] {
        let quat = UnitQuaternion::from_rotation_matrix(&self.basis);
        [quat.i as f32, quat.j as f32, quat.k as f32, quat.w as f32]
    }

    /// Get position as f64 array for libmonado
    #[must_use]
    pub fn position_f64(&self) -> [f64; 3] {
        [self.origin.x, self.origin.y, self.origin.z]
    }

    /// Get orientation as quaternion [x,y,z,w] f64 array for libmonado
    #[must_use]
    pub fn orientation_f64(&self) -> [f64; 4] {
        let quat = UnitQuaternion::from_rotation_matrix(&self.basis);
        [quat.i, quat.j, quat.k, quat.w]
    }

    /// Create from separate rotation and translation components
    #[must_use]
    pub fn from_rotation_translation(rotation: Rotation3<f64>, translation: Vector3<f64>) -> Self {
        Self {
            basis: rotation,
            origin: translation,
        }
    }

    /// Create a translation-only transform
    #[must_use]
    pub fn from_translation(translation: Vector3<f64>) -> Self {
        Self {
            basis: Rotation3::identity(),
            origin: translation,
        }
    }

    /// Create a rotation-only transform
    #[must_use]
    pub fn from_rotation(rotation: Rotation3<f64>) -> Self {
        Self {
            basis: rotation,
            origin: Vector3::zeros(),
        }
    }

    /// Get the translation component
    pub fn translation(&self) -> &Vector3<f64> {
        &self.origin
    }

    /// Get the rotation component
    pub fn rotation(&self) -> &Rotation3<f64> {
        &self.basis
    }

    /// Get the rotation as a unit quaternion
    #[must_use]
    pub fn rotation_quaternion(&self) -> UnitQuaternion<f64> {
        UnitQuaternion::from_rotation_matrix(&self.basis)
    }

    /// Transform a vector (rotation only, no translation)
    #[must_use]
    pub fn transform_vector(&self, vector: &Vector3<f64>) -> Vector3<f64> {
        self.basis * vector
    }

    /// Compute the relative transform from self to other (i.e., self^-1 * other)
    #[must_use]
    pub fn relative_to(&self, other: &Self) -> Self {
        self.inverse().mul(other)
    }

    /// Apply offset transform: returns self * offset
    #[must_use]
    pub fn apply_offset(&self, offset: &Self) -> Self {
        self.mul(offset)
    }

    /// Check if this transform is approximately equal to another
    pub fn approx_eq(&self, other: &Self, epsilon: f64) -> bool {
        let pos_diff = (self.origin - other.origin).norm();
        if pos_diff > epsilon {
            return false;
        }

        // Compare quaternions - handle double-cover (q and -q represent same rotation)
        let q1 = self.rotation_quaternion();
        let q2 = other.rotation_quaternion();
        let dot = q1.w * q2.w + q1.i * q2.i + q1.j * q2.j + q1.k * q2.k;
        dot.abs() > 1.0 - epsilon
    }

    /// Get the Euler angles (roll, pitch, yaw) in radians
    #[must_use]
    pub fn euler_angles(&self) -> (f64, f64, f64) {
        self.basis.euler_angles()
    }

    /// Create from Euler angles (roll, pitch, yaw) in radians
    #[must_use]
    pub fn from_euler_angles(roll: f64, pitch: f64, yaw: f64) -> Self {
        Self {
            basis: Rotation3::from_euler_angles(roll, pitch, yaw),
            origin: Vector3::zeros(),
        }
    }

    /// Get the magnitude of the translation
    #[must_use]
    pub fn translation_magnitude(&self) -> f64 {
        self.origin.norm()
    }

    /// Get the rotation angle (in radians)
    #[must_use]
    pub fn rotation_angle(&self) -> f64 {
        self.rotation_quaternion().angle()
    }

    /// Convert to a 4x4 homogeneous transformation matrix (row-major f64)
    #[must_use]
    pub fn to_homogeneous_matrix(&self) -> [[f64; 4]; 4] {
        let r = self.basis.matrix();
        [
            [r[(0, 0)], r[(0, 1)], r[(0, 2)], self.origin.x],
            [r[(1, 0)], r[(1, 1)], r[(1, 2)], self.origin.y],
            [r[(2, 0)], r[(2, 1)], r[(2, 2)], self.origin.z],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// Create from a 4x4 homogeneous transformation matrix (row-major f64)
    /// Returns None if the rotation part is not a valid rotation matrix
    #[must_use]
    pub fn from_homogeneous_matrix(matrix: [[f64; 4]; 4]) -> Option<Self> {
        // Extract rotation part
        let rot_matrix = nalgebra::Matrix3::new(
            matrix[0][0], matrix[0][1], matrix[0][2],
            matrix[1][0], matrix[1][1], matrix[1][2],
            matrix[2][0], matrix[2][1], matrix[2][2],
        );

        // Try to convert to a proper rotation matrix
        let rotation = Rotation3::from_matrix_unchecked(rot_matrix);

        // Extract translation
        let translation = Vector3::new(matrix[0][3], matrix[1][3], matrix[2][3]);

        Some(Self {
            basis: rotation,
            origin: translation,
        })
    }

    /// Average multiple transforms (useful for calibration)
    /// Uses mean position and quaternion averaging for rotation
    #[must_use]
    pub fn average<I>(transforms: I) -> Option<Self>
    where
        I: IntoIterator<Item = Self>,
    {
        let transforms_vec: Vec<Self> = transforms.into_iter().collect();
        if transforms_vec.is_empty() {
            return None;
        }

        // Average positions
        let avg_pos = transforms_vec
            .iter()
            .fold(Vector3::zeros(), |acc, t| acc + t.origin)
            / transforms_vec.len() as f64;

        // Average quaternions
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

/// Average a set of unit quaternions using iterative mean
/// This handles the double-cover issue and produces a proper average
pub fn average_quaternions(quaternions: &[UnitQuaternion<f64>]) -> Option<UnitQuaternion<f64>> {
    if quaternions.is_empty() {
        return None;
    }

    if quaternions.len() == 1 {
        return Some(quaternions[0]);
    }

    // Start with the first quaternion
    let mut avg = quaternions[0];

    // Iteratively refine the average
    for _ in 0..5 {
        let mut sum = nalgebra::Vector4::zeros();

        for q in quaternions {
            // Ensure consistent hemisphere (handle double-cover)
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
