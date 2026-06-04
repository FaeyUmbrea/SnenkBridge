use crate::vitamins::DelayBuffer;

/// Runtime state for a delay-buffer parameter: a ring buffer that delays the
/// referenced parameter's value by `delay_count` steps, range-maps it, and
/// applies exponential smoothing. Stepped once per frame via [`Self::update`].
pub struct DelayBufferState {
    config: DelayBuffer,
    ring: Vec<f64>,
    write_pos: usize,
    smoothed: f64,
}

impl DelayBufferState {
    #[must_use]
    pub fn new(config: DelayBuffer) -> Self {
        let ring = vec![0.0; config.delay_count.max(1)];
        Self {
            config,
            ring,
            write_pos: 0,
            smoothed: 0.0,
        }
    }

    /// Feeds the referenced parameter's current value and returns the smoothed,
    /// delayed, range-mapped output.
    pub fn update(&mut self, input: f64) -> f64 {
        // Range-map input from [in_min, in_max] to [out_min, out_max].
        let clamped = input.clamp(self.config.in_min, self.config.in_max);
        let span = self.config.in_max - self.config.in_min;
        let normalized = if span == 0.0 {
            0.0
        } else {
            (clamped - self.config.in_min) / span
        };
        let mapped = normalized * (self.config.out_max - self.config.out_min) + self.config.out_min;

        // Store in ring buffer.
        self.ring[self.write_pos] = mapped;
        self.write_pos = (self.write_pos + 1) % self.ring.len();

        // Read delayed value (oldest in ring buffer).
        let delayed = self.ring[self.write_pos % self.ring.len()];

        // Exponential smoothing. A smoothing factor < 1 would overshoot, so
        // guard against division by zero / values below 1.
        let smoothing = self.config.smoothing.max(1.0);
        self.smoothed += (delayed - self.smoothed) / smoothing;
        self.smoothed
    }

    /// The current smoothed output without advancing the buffer.
    #[must_use]
    pub fn current(&self) -> f64 {
        self.smoothed
    }

    /// Name of the parameter this buffer follows.
    #[must_use]
    pub fn ref_param(&self) -> &str {
        &self.config.ref_param
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(delay_count: usize, smoothing: f64) -> DelayBuffer {
        DelayBuffer {
            ref_param: "Src".to_string(),
            smoothing,
            delay_count,
            in_min: -1.0,
            in_max: 1.0,
            out_min: -1.0,
            out_max: 1.0,
        }
    }

    #[test]
    fn settles_toward_mapped_input() {
        let mut state = DelayBufferState::new(cfg(4, 2.0));
        let mut last = state.current();
        assert_eq!(last, 0.0);
        for _ in 0..200 {
            last = state.update(1.0);
        }
        assert!((last - 1.0).abs() < 1e-3, "settled value was {last}");
        assert!((state.current() - last).abs() < 1e-12);
    }

    #[test]
    fn range_maps_input() {
        // Identity mapping here; a constant 0 input settles to 0 output.
        let mut state = DelayBufferState::new(cfg(2, 1.0));
        let mut v = 0.0;
        for _ in 0..50 {
            v = state.update(0.0);
        }
        assert!(v.abs() < 1e-9);
    }

    #[test]
    fn tolerates_zero_smoothing_and_delay() {
        // Guards should keep these from panicking (div-by-zero / empty ring).
        let mut state = DelayBufferState::new(cfg(0, 0.0));
        let _ = state.update(0.5);
    }
}
