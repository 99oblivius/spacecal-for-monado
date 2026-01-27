#![allow(dead_code)]
use crate::error::CalibrationError;

/// Configuration for floor calibration
pub struct FloorConfig {
    /// Number of samples to collect
    pub sample_count: u32,
    /// Maximum allowed variance in samples (meters)
    pub max_variance: f32,
}

impl Default for FloorConfig {
    fn default() -> Self {
        Self {
            sample_count: 30,
            max_variance: 0.05, // 5cm max variance
        }
    }
}

/// State machine for floor calibration
pub struct FloorCalibrator {
    config: FloorConfig,
    samples: Vec<f32>,
    is_active: bool,
}

impl FloorCalibrator {
    pub fn new(config: FloorConfig) -> Self {
        Self {
            config,
            samples: Vec::new(),
            is_active: false,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(FloorConfig::default())
    }

    /// Start floor calibration
    pub fn start(&mut self) {
        self.samples.clear();
        self.is_active = true;
    }

    /// Stop floor calibration without computing result
    pub fn cancel(&mut self) {
        self.is_active = false;
        self.samples.clear();
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Add a floor height sample from hand tracking
    /// Returns Ok(Some(adjustment)) when calibration is complete
    pub fn add_sample(&mut self, height: f32) -> Result<Option<f32>, CalibrationError> {
        if !self.is_active {
            return Ok(None);
        }

        self.samples.push(height);

        if self.samples.len() >= self.config.sample_count as usize {
            self.is_active = false;
            return self.compute_adjustment().map(Some);
        }

        Ok(None)
    }

    /// Get collection progress
    pub fn progress(&self) -> (u32, u32) {
        (self.samples.len() as u32, self.config.sample_count)
    }

    /// Compute the floor adjustment from collected samples
    fn compute_adjustment(&self) -> Result<f32, CalibrationError> {
        if self.samples.is_empty() {
            return Err(CalibrationError::InsufficientSamples {
                required: self.config.sample_count as usize,
                collected: 0,
            });
        }

        // Use median for robustness against outliers
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = if sorted.len() % 2 == 0 {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        } else {
            sorted[sorted.len() / 2]
        };

        // Check variance
        let variance: f32 = self.samples.iter()
            .map(|s| (s - median).powi(2))
            .sum::<f32>() / self.samples.len() as f32;

        if variance.sqrt() > self.config.max_variance {
            return Err(CalibrationError::HighVariance {
                variance: variance.sqrt(),
                threshold: self.config.max_variance,
            });
        }

        // The samples are already the bottom of hand (position - radius)
        // Floor adjustment = -median (to make floor Y=0)
        Ok(-median)
    }

    /// Get statistics about collected samples
    pub fn stats(&self) -> FloorStats {
        if self.samples.is_empty() {
            return FloorStats {
                sample_count: 0,
                min_height: 0.0,
                max_height: 0.0,
                mean_height: 0.0,
            };
        }

        let min = self.samples.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = self.samples.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mean = self.samples.iter().sum::<f32>() / self.samples.len() as f32;

        FloorStats {
            sample_count: self.samples.len(),
            min_height: min,
            max_height: max,
            mean_height: mean,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FloorStats {
    pub sample_count: usize,
    pub min_height: f32,
    pub max_height: f32,
    pub mean_height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floor_calibration() {
        let mut cal = FloorCalibrator::new(FloorConfig {
            sample_count: 5,
            max_variance: 0.1,
        });

        cal.start();
        assert!(cal.is_active());

        // Simulate hand bottom at 0.03m (floor is 3cm above stage origin)
        // These samples are already position - radius
        for _ in 0..4 {
            assert!(cal.add_sample(0.03).unwrap().is_none());
        }

        // Last sample should trigger completion
        let result = cal.add_sample(0.03).unwrap();
        assert!(result.is_some());

        // Expected adjustment: -0.03 (to shift floor to Y=0)
        let adjustment = result.unwrap();
        assert!((adjustment - (-0.03)).abs() < 0.001);
    }
}
