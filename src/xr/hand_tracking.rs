#![allow(dead_code)]
use openxr as xr;
use crate::error::XrError;

/// Hand tracking extension wrapper for EXT_hand_tracking
///
/// Provides access to hand joint positions for calibration purposes,
/// particularly for detecting hand positions near the floor.
///
/// # Safety
/// This wrapper assumes the EXT_hand_tracking extension is available.
/// Always check extension availability before creating a HandTracker.
pub struct HandTracker {
    left_hand: xr::HandTracker,
    right_hand: xr::HandTracker,
}

/// Which hand to query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

/// Joint locations for a single hand
///
/// Contains positions of key joints used for floor calibration.
/// All positions are in meters relative to the base space.
#[derive(Debug, Clone)]
pub struct HandJoints {
    /// Position of the palm joint [x, y, z] in meters
    pub palm_position: [f32; 3],
    /// Radius of the palm joint (distance from center to skin surface)
    pub palm_radius: f32,
    /// Position of the wrist joint [x, y, z] in meters
    pub wrist_position: [f32; 3],
    /// Radius of the wrist joint
    pub wrist_radius: f32,
    /// Whether this hand is currently being tracked
    pub is_active: bool,
}

impl HandTracker {
    /// Create hand trackers if the EXT_hand_tracking extension is available
    ///
    /// Returns None if the extension is not supported by the runtime.
    ///
    /// # Safety
    /// The session must remain valid for the lifetime of the HandTracker.
    /// The caller must ensure the session is not dropped while the tracker
    /// is still in use.
    ///
    /// # Example
    /// ```ignore
    /// if let Some(tracker) = HandTracker::new(&session)? {
    ///     // Hand tracking is available
    /// } else {
    ///     // Fall back to other calibration methods
    /// }
    /// ```
    pub fn new<G>(session: &xr::Session<G>) -> Result<Option<Self>, XrError> {
        // Check if the extension is available by attempting to create hand trackers
        // If creation fails, assume the extension is not supported

        let left_hand = match session.create_hand_tracker(xr::Hand::LEFT) {
            Ok(tracker) => tracker,
            Err(_) => return Ok(None),
        };

        let right_hand = match session.create_hand_tracker(xr::Hand::RIGHT) {
            Ok(tracker) => tracker,
            Err(_) => return Ok(None),
        };

        Ok(Some(Self {
            left_hand,
            right_hand,
        }))
    }

    /// Get joint positions for a hand at a given time
    ///
    /// Returns None if the hand is not currently being tracked or if
    /// the tracking data is invalid.
    ///
    /// # Arguments
    /// * `hand` - Which hand to query
    /// * `base_space` - Reference space for the positions
    /// * `time` - Time at which to sample the hand tracking
    ///
    /// # Example
    /// ```ignore
    /// if let Some(joints) = tracker.locate_hand(Hand::Left, &base_space, time) {
    ///     println!("Palm Y: {}", joints.palm_position[1]);
    /// }
    /// ```
    pub fn locate_hand(
        &self,
        hand: Hand,
        base_space: &xr::Space,
        time: xr::Time,
    ) -> Option<HandJoints> {
        let tracker = match hand {
            Hand::Left => &self.left_hand,
            Hand::Right => &self.right_hand,
        };

        // Locate the hand joints using Space::locate_hand_joints
        let joint_locations = base_space.locate_hand_joints(tracker, time).ok()??;

        // Extract palm and wrist positions
        // In OpenXR, joint indices are defined by the spec:
        // PALM = 0, WRIST = 1
        let palm_location = &joint_locations[xr::HandJoint::PALM.into_raw() as usize];
        let wrist_location = &joint_locations[xr::HandJoint::WRIST.into_raw() as usize];

        // Check if tracking is active (at least one joint has valid position)
        let is_active = palm_location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
            || wrist_location.location_flags.contains(xr::SpaceLocationFlags::POSITION_VALID);

        Some(HandJoints {
            palm_position: [
                palm_location.pose.position.x,
                palm_location.pose.position.y,
                palm_location.pose.position.z,
            ],
            palm_radius: palm_location.radius,
            wrist_position: [
                wrist_location.pose.position.x,
                wrist_location.pose.position.y,
                wrist_location.pose.position.z,
            ],
            wrist_radius: wrist_location.radius,
            is_active,
        })
    }

    /// Get the lowest hand position (bottom of joint sphere) across both hands
    ///
    /// This is useful for floor calibration - the user places their hand
    /// on the floor, and this returns the Y coordinate of the lowest point.
    /// Like motoc, we compute position.y - radius to find the actual bottom
    /// of the hand joint sphere (the point touching the floor).
    ///
    /// Returns None if neither hand is currently being tracked.
    ///
    /// # Arguments
    /// * `base_space` - Reference space for the positions
    /// * `time` - Time at which to sample the hand tracking
    pub fn get_lowest_hand_position(
        &self,
        base_space: &xr::Space,
        time: xr::Time,
    ) -> Option<f32> {
        let left = self.locate_hand(Hand::Left, base_space, time);
        let right = self.locate_hand(Hand::Right, base_space, time);

        let mut lowest = None;

        if let Some(l) = left
            && l.is_active
        {
            // Bottom of palm = position.y - radius (like motoc)
            let palm_bottom = l.palm_position[1] - l.palm_radius;
            let wrist_bottom = l.wrist_position[1] - l.wrist_radius;
            let y = palm_bottom.min(wrist_bottom);
            lowest = Some(y);
        }

        if let Some(r) = right
            && r.is_active
        {
            let palm_bottom = r.palm_position[1] - r.palm_radius;
            let wrist_bottom = r.wrist_position[1] - r.wrist_radius;
            let y = palm_bottom.min(wrist_bottom);
            lowest = Some(match lowest {
                Some(l) => l.min(y),
                None => y,
            });
        }

        lowest
    }

    /// Get both hands' joint data simultaneously
    ///
    /// More efficient than calling locate_hand twice if you need both hands.
    ///
    /// # Returns
    /// A tuple of (left_hand, right_hand), where each is None if not tracked.
    pub fn locate_both_hands(
        &self,
        base_space: &xr::Space,
        time: xr::Time,
    ) -> (Option<HandJoints>, Option<HandJoints>) {
        let left = self.locate_hand(Hand::Left, base_space, time);
        let right = self.locate_hand(Hand::Right, base_space, time);
        (left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hand_joints_default() {
        let joints = HandJoints {
            palm_position: [0.0, 0.0, 0.0],
            palm_radius: 0.02,
            wrist_position: [0.0, 0.0, 0.0],
            wrist_radius: 0.015,
            is_active: false,
        };
        assert!(!joints.is_active);
    }

    #[test]
    fn test_hand_enum() {
        assert_eq!(Hand::Left, Hand::Left);
        assert_ne!(Hand::Left, Hand::Right);
    }
}
