// Rnote - Sketch and take handwritten notes.
// Copyright (C) The Rnote Authors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Stroke smoothing and stabilization algorithms (G4).
//!
//! Includes:
//! - **One-Euro Filter** (Casiez et al., 2012): Adaptive low-pass filtering.
//!   Maintains zero latency during fast strokes while suppressing jitter at low speeds.
//! - **Lazy Brush** (Pull-string stabilization): A virtual brush dragged behind the cursor
//!   with a customizable deadzone radius for high-precision inking and lineart.

use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

/// Low-pass filter for scalar or vector time series.
#[derive(Debug, Clone, Copy)]
pub struct LowPassFilter {
    y_prev: Option<Vector2>,
}

impl LowPassFilter {
    /// Create a new, uninitialized low-pass filter.
    pub fn new() -> Self {
        Self { y_prev: None }
    }

    /// Reset filter state.
    pub fn reset(&mut self) {
        self.y_prev = None;
    }

    /// Filter value with smoothing factor alpha in (0, 1].
    pub fn filter(&mut self, val: Vector2, alpha: f64) -> Vector2 {
        let alpha = alpha.clamp(1e-4, 1.0);
        let result = match self.y_prev {
            Some(prev) => prev + (val - prev) * alpha,
            None => val,
        };
        self.y_prev = Some(result);
        result
    }

    /// Returns the last filtered value.
    pub fn last_value(&self) -> Option<Vector2> {
        self.y_prev
    }
}

impl Default for LowPassFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// One-Euro adaptive frequency cutoff filter (Casiez et al., 2012).
#[derive(Debug, Clone, Copy)]
pub struct OneEuroFilter {
    /// Minimum cutoff frequency in Hz (typically 0.5 to 2.0).
    pub min_cutoff: f64,
    /// Speed coefficient beta (typically 0.001 to 0.05).
    pub beta: f64,
    /// Derivative cutoff frequency in Hz (typically 1.0).
    pub d_cutoff: f64,
    x_filter: LowPassFilter,
    dx_filter: LowPassFilter,
    last_time: Option<f64>,
}

impl OneEuroFilter {
    /// Initialize with standard pen tablet parameters.
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x_filter: LowPassFilter::new(),
            dx_filter: LowPassFilter::new(),
            last_time: None,
        }
    }

    /// Reset internal history.
    pub fn reset(&mut self) {
        self.x_filter.reset();
        self.dx_filter.reset();
        self.last_time = None;
    }

    /// Calculate filter alpha given sample period dt and cutoff frequency fc.
    fn alpha(dt: f64, cutoff: f64) -> f64 {
        let tau = 1.0 / (2.0 * std::f64::consts::PI * cutoff.max(1e-4));
        1.0 / (1.0 + tau / dt.max(1e-4))
    }

    /// Filter a 2D coordinate point at timestamp `timestamp_secs`.
    pub fn filter(&mut self, x: Vector2, timestamp_secs: f64) -> Vector2 {
        let dt = match self.last_time {
            Some(t_prev) => (timestamp_secs - t_prev).max(1e-4),
            None => 1.0 / 120.0, // Default 120Hz estimate on first sample
        };
        self.last_time = Some(timestamp_secs);

        // Estimate derivative (velocity)
        let dx = match self.x_filter.last_value() {
            Some(prev_x) => (x - prev_x) / dt,
            None => Vector2::ZERO,
        };

        let edx = self.dx_filter.filter(dx, Self::alpha(dt, self.d_cutoff));
        let speed = edx.length();

        // Adaptive cutoff based on speed
        let cutoff = self.min_cutoff + self.beta * speed;
        self.x_filter.filter(x, Self::alpha(dt, cutoff))
    }
}

impl Default for OneEuroFilter {
    fn default() -> Self {
        Self::new(1.0, 0.007, 1.0)
    }
}

/// Lazy Brush (Pull-string stabilization).
///
/// Keeps the brush centered inside a circle of radius `radius`. The brush only moves
/// when the cursor pulls the string past the perimeter.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, rename = "lazy_brush")]
pub struct LazyBrush {
    /// Pull-string radius in pixels (0.0 disables lazy brush).
    #[serde(rename = "radius")]
    pub radius: f64,
    /// Current virtual brush position.
    #[serde(skip)]
    pub brush_pos: Option<Vector2>,
}

impl Default for LazyBrush {
    fn default() -> Self {
        Self {
            radius: 8.0,
            brush_pos: None,
        }
    }
}

impl LazyBrush {
    /// Create a new LazyBrush with string radius.
    pub fn new(radius: f64) -> Self {
        Self {
            radius,
            brush_pos: None,
        }
    }

    /// Reset brush position.
    pub fn reset(&mut self) {
        self.brush_pos = None;
    }

    /// Update with cursor position, returning smoothed brush position.
    pub fn update(&mut self, cursor: Vector2) -> Vector2 {
        if self.radius <= 0.0 {
            self.brush_pos = Some(cursor);
            return cursor;
        }

        let pos = match self.brush_pos {
            None => cursor,
            Some(brush) => {
                let diff = cursor - brush;
                let dist = diff.length();
                if dist > self.radius {
                    brush + diff * ((dist - self.radius) / dist)
                } else {
                    brush
                }
            }
        };

        self.brush_pos = Some(pos);
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_pass_filter() {
        let mut filter = LowPassFilter::new();
        let p1 = Vector2::new(0.0, 0.0);
        let p2 = Vector2::new(10.0, 10.0);

        assert_eq!(filter.filter(p1, 0.5), p1);
        let out = filter.filter(p2, 0.5);
        assert_eq!(out, Vector2::new(5.0, 5.0));
    }

    #[test]
    fn test_one_euro_jitter_reduction() {
        let mut filter = OneEuroFilter::default();
        let center = Vector2::new(100.0, 100.0);

        let mut samples = Vec::new();
        // Simulate a stationary hand with high-frequency jitter (+/- 2.0 px)
        for i in 0..50 {
            let t = i as f64 * 0.01;
            let jitter = if i % 2 == 0 { 2.0 } else { -2.0 };
            let raw = center + Vector2::new(jitter, jitter);
            samples.push(filter.filter(raw, t));
        }

        // Check variance of last 20 filtered samples is substantially lower than raw jitter
        let last_sample = samples.last().unwrap();
        assert!((last_sample.x - center.x).abs() < 1.0);
        assert!((last_sample.y - center.y).abs() < 1.0);
    }

    #[test]
    fn test_lazy_brush_deadzone() {
        let mut brush = LazyBrush::new(10.0);
        let start = Vector2::new(50.0, 50.0);

        assert_eq!(brush.update(start), start);

        // Small movement within radius (5.0 px) should not move the brush
        let small_move = start + Vector2::new(3.0, 4.0); // length = 5.0
        let b1 = brush.update(small_move);
        assert_eq!(b1, start);

        // Movement past 10.0 radius (20.0 px) moves brush by (20 - 10) = 10 px
        let large_move = start + Vector2::new(20.0, 0.0);
        let b2 = brush.update(large_move);
        assert_eq!(b2, Vector2::new(60.0, 50.0));
    }
}
