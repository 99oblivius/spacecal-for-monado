use crate::error::CalibrationError;

const DEFAULT_SAMPLE_COUNT: u32 = 30;
const DEFAULT_MAX_VARIANCE: f32 = 0.05; // 5cm

pub struct FloorCalibrator {
    sample_count: u32,
    max_variance: f32,
    samples: Vec<f32>,
    is_active: bool,
}

impl FloorCalibrator {
    pub fn with_default_config() -> Self {
        Self {
            sample_count: DEFAULT_SAMPLE_COUNT,
            max_variance: DEFAULT_MAX_VARIANCE,
            samples: Vec::new(),
            is_active: false,
        }
    }

    pub fn start(&mut self) {
        self.samples.clear();
        self.is_active = true;
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Returns Ok(Some(adjustment)) when enough samples are collected.
    pub fn add_sample(&mut self, height: f32) -> Result<Option<f32>, CalibrationError> {
        if !self.is_active {
            return Ok(None);
        }

        self.samples.push(height);

        if self.samples.len() >= self.sample_count as usize {
            self.is_active = false;
            return self.compute_adjustment().map(Some);
        }

        Ok(None)
    }

    pub fn progress(&self) -> (u32, u32) {
        (self.samples.len() as u32, self.sample_count)
    }

    fn compute_adjustment(&self) -> Result<f32, CalibrationError> {
        if self.samples.is_empty() {
            return Err(CalibrationError::InsufficientSamples {
                required: self.sample_count as usize,
                collected: 0,
            });
        }

        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = if sorted.len().is_multiple_of(2) {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        } else {
            sorted[sorted.len() / 2]
        };

        let variance: f32 = self.samples.iter()
            .map(|s| (s - median).powi(2))
            .sum::<f32>() / self.samples.len() as f32;

        if variance.sqrt() > self.max_variance {
            return Err(CalibrationError::HighVariance {
                variance: variance.sqrt(),
                threshold: self.max_variance,
            });
        }

        // Floor adjustment = -median (to make floor Y=0)
        Ok(-median)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floor_calibration() {
        let mut cal = FloorCalibrator::with_default_config();
        cal.sample_count = 5;
        cal.max_variance = 0.1;

        cal.start();
        assert!(cal.is_active());

        for _ in 0..4 {
            assert!(cal.add_sample(0.03).unwrap().is_none());
        }

        let result = cal.add_sample(0.03).unwrap();
        assert!(result.is_some());

        let adjustment = result.unwrap();
        assert!((adjustment - (-0.03)).abs() < 0.001);
    }
}
