#![allow(dead_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Livia Goette <livia@livingsilver94.net>

//! Real-time continuous calibration tracking.
//!
//! This module provides functionality for maintaining and updating calibration
//! transforms in real-time during active tracking. The continuous calibrator
//! smooths transforms over time to reduce jitter while maintaining responsiveness.
//!
//! # Overview
//!
//! The continuous calibration system operates as a sliding window filter that:
//! - Accumulates recent transform samples within a time window
//! - Computes smoothed transforms using weighted averaging
//! - Only updates when changes exceed a minimum threshold
//! - Automatically discards old samples
//!
//! # Example
//!
//! ```no_run
//! use monado_spacecal::calibration::continuous::{ContinuousCalibrator, ContinuousConfig};
//! use monado_spacecal::calibration::transform::TransformD;
//!
//! let mut calibrator = ContinuousCalibrator::with_default_config();
//! calibrator.start();
//!
//! // In your tracking loop:
//! let time = 1.0; // Current time in seconds
//! let new_transform = TransformD::identity(); // From calibration computation
//!
//! if let Some(updated) = calibrator.update(time, new_transform) {
//!     // Transform changed significantly, apply it
//!     println!("Updated calibration");
//! }
//! ```

use crate::calibration::transform::TransformD;
use std::collections::VecDeque;

/// Configuration parameters for continuous calibration.
///
/// Controls the behavior of the smoothing algorithm and update thresholds.
#[derive(Debug, Clone)]
pub struct ContinuousConfig {
    /// Number of samples to average for smoothing.
    ///
    /// Larger values produce smoother output but increase latency.
    /// Typical range: 5-20 samples.
    pub smoothing_window: usize,

    /// Maximum time between samples in seconds.
    ///
    /// Samples older than this are automatically discarded.
    /// Should match your expected frame rate (e.g., 0.5s for variable rate tracking).
    pub max_sample_age: f64,

    /// Minimum movement required to trigger an update (in meters).
    ///
    /// Changes smaller than this threshold are ignored to prevent jitter.
    /// Typical values: 0.001m (1mm) for high-precision tracking.
    pub min_movement_threshold: f64,
}

impl Default for ContinuousConfig {
    fn default() -> Self {
        Self {
            smoothing_window: 10,
            max_sample_age: 0.5,
            min_movement_threshold: 0.001,
        }
    }
}

/// State machine for continuous calibration mode.
///
/// Maintains a sliding window of recent transforms and computes smoothed
/// calibration results suitable for real-time application.
///
/// # State Machine
///
/// - **Inactive**: Default state, updates are ignored
/// - **Active**: Accumulating samples and computing smoothed transforms
///
/// # Thread Safety
///
/// This type is not thread-safe. If used from multiple threads, synchronization
/// is required.
pub struct ContinuousCalibrator {
    config: ContinuousConfig,
    /// Recent transforms for smoothing (time, transform)
    recent_transforms: VecDeque<(f64, TransformD)>,
    /// Current smoothed transform
    current_transform: TransformD,
    /// Whether calibration is active
    is_active: bool,
    /// Last update time (in seconds)
    last_update_time: f64,
}

impl ContinuousCalibrator {
    /// Creates a new continuous calibrator with the given configuration.
    ///
    /// The calibrator starts in the inactive state. Call [`start()`](Self::start)
    /// to begin accepting updates.
    pub fn new(config: ContinuousConfig) -> Self {
        Self {
            recent_transforms: VecDeque::with_capacity(config.smoothing_window),
            config,
            current_transform: TransformD::identity(),
            is_active: false,
            last_update_time: 0.0,
        }
    }

    /// Creates a new continuous calibrator with default configuration.
    ///
    /// # Default Configuration
    ///
    /// - Smoothing window: 10 samples
    /// - Max sample age: 0.5 seconds
    /// - Min movement threshold: 0.001 meters (1mm)
    pub fn with_default_config() -> Self {
        Self::new(ContinuousConfig::default())
    }

    /// Starts continuous calibration.
    ///
    /// Clears any previous samples and resets to identity transform.
    /// Subsequent calls to [`update()`](Self::update) will be processed.
    pub fn start(&mut self) {
        self.is_active = true;
        self.recent_transforms.clear();
        self.current_transform = TransformD::identity();
    }

    /// Stops continuous calibration.
    ///
    /// Subsequent calls to [`update()`](Self::update) will be ignored.
    /// The current smoothed transform is preserved.
    pub fn stop(&mut self) {
        self.is_active = false;
    }

    /// Returns whether continuous calibration is currently active.
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Gets the current smoothed transform.
    ///
    /// This is the most recent smoothed result from the sliding window filter.
    /// Returns identity transform if no updates have been processed yet.
    pub fn current_transform(&self) -> &TransformD {
        &self.current_transform
    }

    /// Updates the calibrator with a new instantaneous transform.
    ///
    /// # Arguments
    ///
    /// * `time` - Current time in seconds (monotonic)
    /// * `transform` - New calibration transform sample
    ///
    /// # Returns
    ///
    /// - `Some(transform)` if the smoothed transform changed significantly
    /// - `None` if inactive or change was below threshold
    ///
    /// # Behavior
    ///
    /// 1. Discards samples older than `max_sample_age`
    /// 2. Adds the new sample to the window
    /// 3. Computes smoothed transform from all samples
    /// 4. Only returns updated transform if position change exceeds threshold
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use monado_spacecal::calibration::continuous::ContinuousCalibrator;
    /// # use monado_spacecal::calibration::transform::TransformD;
    /// # let mut calibrator = ContinuousCalibrator::with_default_config();
    /// calibrator.start();
    ///
    /// for i in 0..100 {
    ///     let time = i as f64 * 0.016; // ~60 FPS
    ///     let transform = TransformD::identity(); // Your calibration result
    ///
    ///     if let Some(updated) = calibrator.update(time, transform) {
    ///         // Apply updated calibration
    ///     }
    /// }
    /// ```
    pub fn update(&mut self, time: f64, transform: TransformD) -> Option<TransformD> {
        if !self.is_active {
            return None;
        }

        // Remove old samples
        let cutoff = time - self.config.max_sample_age;
        while self.recent_transforms.front().map(|(t, _)| *t < cutoff).unwrap_or(false) {
            self.recent_transforms.pop_front();
        }

        // Add new sample
        self.recent_transforms.push_back((time, transform));

        // Keep window size
        while self.recent_transforms.len() > self.config.smoothing_window {
            self.recent_transforms.pop_front();
        }

        // Compute smoothed transform
        let new_transform = self.compute_smoothed_transform();

        // Check if change is significant
        let position_delta = (new_transform.origin - self.current_transform.origin).norm();

        if position_delta > self.config.min_movement_threshold {
            self.current_transform = new_transform.clone();
            self.last_update_time = time;
            Some(new_transform)
        } else {
            None
        }
    }

    /// Computes smoothed transform from recent samples using weighted average.
    ///
    /// Uses quaternion averaging for orientations and arithmetic mean for positions.
    fn compute_smoothed_transform(&self) -> TransformD {
        if self.recent_transforms.is_empty() {
            return TransformD::identity();
        }

        if self.recent_transforms.len() == 1 {
            return self.recent_transforms[0].1.clone();
        }

        // Use the average method from TransformD
        let transforms: Vec<TransformD> = self.recent_transforms.iter()
            .map(|(_, t)| t.clone())
            .collect();

        TransformD::average(transforms).unwrap_or_else(TransformD::identity)
    }

    /// Gets statistics about the current calibration state.
    ///
    /// Useful for monitoring and debugging the calibration system.
    pub fn stats(&self) -> ContinuousStats {
        ContinuousStats {
            sample_count: self.recent_transforms.len(),
            last_update_time: self.last_update_time,
            is_active: self.is_active,
        }
    }
}

/// Statistics about the continuous calibration state.
///
/// Provides insight into the current operation of the calibrator.
#[derive(Debug, Clone)]
pub struct ContinuousStats {
    /// Number of samples currently in the smoothing window
    pub sample_count: usize,

    /// Time of the last significant update (in seconds)
    pub last_update_time: f64,

    /// Whether the calibrator is currently active
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuous_inactive_by_default() {
        let cal = ContinuousCalibrator::with_default_config();
        assert!(!cal.is_active());
    }

    #[test]
    fn test_continuous_start_stop() {
        let mut cal = ContinuousCalibrator::with_default_config();
        cal.start();
        assert!(cal.is_active());
        cal.stop();
        assert!(!cal.is_active());
    }

    #[test]
    fn test_continuous_ignores_updates_when_inactive() {
        let mut cal = ContinuousCalibrator::with_default_config();
        let result = cal.update(1.0, TransformD::identity());
        assert!(result.is_none());
    }

    #[test]
    fn test_continuous_stats() {
        let mut cal = ContinuousCalibrator::with_default_config();
        cal.start();

        let stats = cal.stats();
        assert_eq!(stats.sample_count, 0);
        assert!(stats.is_active);

        cal.update(1.0, TransformD::identity());
        let stats = cal.stats();
        assert_eq!(stats.sample_count, 1);
    }

    #[test]
    fn test_continuous_discards_old_samples() {
        let config = ContinuousConfig {
            smoothing_window: 100,
            max_sample_age: 0.1,
            min_movement_threshold: 0.0,
        };
        let mut cal = ContinuousCalibrator::new(config);
        cal.start();

        // Add old sample
        cal.update(0.0, TransformD::identity());
        assert_eq!(cal.stats().sample_count, 1);

        // Add new sample after max_sample_age
        cal.update(0.2, TransformD::identity());
        assert_eq!(cal.stats().sample_count, 1); // Old sample should be removed
    }

    #[test]
    fn test_continuous_window_size_limit() {
        let config = ContinuousConfig {
            smoothing_window: 3,
            max_sample_age: 10.0,
            min_movement_threshold: 0.0,
        };
        let mut cal = ContinuousCalibrator::new(config);
        cal.start();

        for i in 0..5 {
            cal.update(i as f64, TransformD::identity());
        }

        assert_eq!(cal.stats().sample_count, 3); // Should be capped at window size
    }
}
