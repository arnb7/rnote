// Rnote - Sketch and take handwritten notes.
// Copyright (C) The Rnote Authors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Advanced pressure curve engine and presets (G3 & G14).
//!
//! Provides customizable pressure response mapping with:
//! - Deadzone trimming and maximum saturation threshold.
//! - Gamma curves (Soft / Hard / Custom gamma).
//! - Sigmoid (S-curve) response.
//! - Cubic Bézier transfer curves (normalized unit box [0, 1]x[0, 1]).
//! - Presets calibrated for graphic tablets including the VEIKK S640 (8192 levels).

use serde::{Deserialize, Serialize};

/// Type of mathematical curve response.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename = "pressure_response_type")]
pub enum PressureResponseType {
    /// Direct linear transfer (f(x) = x).
    #[default]
    Linear,
    /// Constant response regardless of input.
    Constant,
    /// Gamma power curve: f(x) = x^gamma.
    /// gamma < 1.0 produces a Soft touch; gamma > 1.0 produces a Firm/Hard touch.
    Gamma(f64),
    /// Sigmoidal S-curve with steepness parameter k (typically 6.0 to 12.0).
    Sigmoid {
        /// Steepness factor controlling transition slope.
        steepness: f64,
        /// Center midpoint of sigmoid transition in [0.0, 1.0].
        midpoint: f64,
    },
    /// Cubic Bézier defined by two control points (p1_x, p1_y) and (p2_x, p2_y).
    CubicBezier {
        /// First control point in unit box [0, 1]x[0, 1].
        p1: (f64, f64),
        /// Second control point in unit box [0, 1]x[0, 1].
        p2: (f64, f64),
    },
}

/// Comprehensive pressure calibration profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename = "pressure_profile")]
pub struct PressureProfile {
    /// Deadzone threshold in [0.0, 1.0]. Input pressure below this yields 0.0.
    #[serde(rename = "deadzone")]
    pub deadzone: f64,
    /// Saturation threshold in [0.0, 1.0]. Input pressure above this yields 1.0.
    #[serde(rename = "saturation")]
    pub saturation: f64,
    /// Curve response algorithm.
    #[serde(rename = "response")]
    pub response: PressureResponseType,
}

impl Default for PressureProfile {
    fn default() -> Self {
        Self {
            deadzone: 0.0,
            saturation: 1.0,
            response: PressureResponseType::Linear,
        }
    }
}

impl PressureProfile {
    /// Preset: Standard linear mapping.
    pub fn preset_linear() -> Self {
        Self::default()
    }

    /// Preset: Soft response (gamma = 0.65). Requires very little pen pressure for thick lines.
    pub fn preset_soft() -> Self {
        Self {
            deadzone: 0.01,
            saturation: 0.99,
            response: PressureResponseType::Gamma(0.65),
        }
    }

    /// Preset: Firm/Hard response (gamma = 1.45). Requires deliberate, heavy pressure for thick lines.
    pub fn preset_firm() -> Self {
        Self {
            deadzone: 0.03,
            saturation: 1.0,
            response: PressureResponseType::Gamma(1.45),
        }
    }

    /// Preset: Sigmoid S-curve. Gentle initial ink flow, responsive middle range, smooth saturation.
    pub fn preset_sigmoid() -> Self {
        Self {
            deadzone: 0.015,
            saturation: 0.985,
            response: PressureResponseType::Sigmoid {
                steepness: 8.0,
                midpoint: 0.5,
            },
        }
    }

    /// Preset: Specially calibrated for VEIKK S640 (8192 hardware pressure levels).
    /// Eliminates initial pen-tap spikes with a 0.018 deadzone, gentle 0.88 gamma, and 0.982 saturation.
    pub fn preset_veikk_s640() -> Self {
        Self {
            deadzone: 0.018,
            saturation: 0.982,
            response: PressureResponseType::Gamma(0.88),
        }
    }

    /// Preset: Custom Cubic Bézier.
    pub fn preset_bezier(p1: (f64, f64), p2: (f64, f64)) -> Self {
        Self {
            deadzone: 0.01,
            saturation: 0.99,
            response: PressureResponseType::CubicBezier { p1, p2 },
        }
    }

    /// Maps an input raw pressure value in [0.0, 1.0] to a calibrated pressure in [0.0, 1.0].
    pub fn evaluate(&self, raw_pressure: f64) -> f64 {
        let p = raw_pressure.clamp(0.0, 1.0);

        // 1. Deadzone and saturation clamping with linear normalization
        let dz = self.deadzone.clamp(0.0, 0.5);
        let sat = self.saturation.clamp(dz + 0.01, 1.0);

        if p <= dz {
            return 0.0;
        }
        if p >= sat {
            return 1.0;
        }

        let normalized = (p - dz) / (sat - dz);

        // 2. Response transfer function
        match self.response {
            PressureResponseType::Constant => 1.0,
            PressureResponseType::Linear => normalized,
            PressureResponseType::Gamma(gamma) => {
                let g = if gamma <= 0.0 { 1.0 } else { gamma };
                normalized.powf(g)
            }
            PressureResponseType::Sigmoid {
                steepness,
                midpoint,
            } => {
                let s_val = 1.0 / (1.0 + (-steepness * (normalized - midpoint)).exp());
                let s0 = 1.0 / (1.0 + (-steepness * (0.0 - midpoint)).exp());
                let s1 = 1.0 / (1.0 + (-steepness * (1.0 - midpoint)).exp());
                ((s_val - s0) / (s1 - s0)).clamp(0.0, 1.0)
            }
            PressureResponseType::CubicBezier { p1, p2 } => {
                Self::solve_cubic_bezier(normalized, p1, p2)
            }
        }
    }

    /// Apply the evaluated pressure to stroke width.
    pub fn apply(&self, base_width: f64, raw_pressure: f64) -> f64 {
        base_width * self.evaluate(raw_pressure)
    }

    /// Evaluates y for a given x in a cubic Bézier from (0,0) through (p1.x, p1.y), (p2.x, p2.y) to (1,1).
    /// Uses Newton-Raphson iteration with bisection fallback.
    fn solve_cubic_bezier(x: f64, p1: (f64, f64), p2: (f64, f64)) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }

        // B(t) = 3(1-t)^2 * t * p1 + 3(1-t) * t^2 * p2 + t^3
        let sample_x = |t: f64| -> f64 {
            let one_minus_t = 1.0 - t;
            3.0 * one_minus_t * one_minus_t * t * p1.0
                + 3.0 * one_minus_t * t * t * p2.0
                + t * t * t
        };

        let sample_dx = |t: f64| -> f64 {
            let one_minus_t = 1.0 - t;
            3.0 * one_minus_t * one_minus_t * p1.0
                + 6.0 * one_minus_t * t * (p2.0 - p1.0)
                + 3.0 * t * t * (1.0 - p2.0)
        };

        let sample_y = |t: f64| -> f64 {
            let one_minus_t = 1.0 - t;
            3.0 * one_minus_t * one_minus_t * t * p1.1
                + 3.0 * one_minus_t * t * t * p2.1
                + t * t * t
        };

        // Newton-Raphson search for t where sample_x(t) == x
        let mut t = x;
        for _ in 0..8 {
            let current_x = sample_x(t) - x;
            if current_x.abs() < 1e-5 {
                return sample_y(t).clamp(0.0, 1.0);
            }
            let dx = sample_dx(t);
            if dx.abs() < 1e-6 {
                break;
            }
            t -= current_x / dx;
            t = t.clamp(0.0, 1.0);
        }

        // Fallback: binary search
        let mut t0 = 0.0;
        let mut t1 = 1.0;
        t = x;
        for _ in 0..12 {
            let current_x = sample_x(t);
            if (current_x - x).abs() < 1e-5 {
                break;
            }
            if x > current_x {
                t0 = t;
            } else {
                t1 = t;
            }
            t = (t1 + t0) * 0.5;
        }

        sample_y(t).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_profile() {
        let profile = PressureProfile::preset_linear();
        assert_eq!(profile.evaluate(0.0), 0.0);
        assert_eq!(profile.evaluate(0.5), 0.5);
        assert_eq!(profile.evaluate(1.0), 1.0);
    }

    #[test]
    fn test_deadzone_and_saturation() {
        let profile = PressureProfile {
            deadzone: 0.1,
            saturation: 0.9,
            response: PressureResponseType::Linear,
        };

        assert_eq!(profile.evaluate(0.05), 0.0);
        assert_eq!(profile.evaluate(0.10), 0.0);
        assert_eq!(profile.evaluate(0.50), 0.5);
        assert_eq!(profile.evaluate(0.90), 1.0);
        assert_eq!(profile.evaluate(0.95), 1.0);
    }

    #[test]
    fn test_soft_and_firm_presets() {
        let soft = PressureProfile::preset_soft();
        let firm = PressureProfile::preset_firm();

        let mid_soft = soft.evaluate(0.5);
        let mid_firm = firm.evaluate(0.5);

        // Soft gamma (< 1.0) maps 0.5 to higher pressure; firm gamma (> 1.0) maps to lower
        assert!(mid_soft > 0.5);
        assert!(mid_firm < 0.5);
    }

    #[test]
    fn test_sigmoid_preset() {
        let sigmoid = PressureProfile::preset_sigmoid();
        assert_eq!(sigmoid.evaluate(0.0), 0.0);
        assert_eq!(sigmoid.evaluate(1.0), 1.0);
        let mid = sigmoid.evaluate(0.5);
        assert!((mid - 0.5).abs() < 0.05);
    }

    #[test]
    fn test_cubic_bezier_monotonicity() {
        let bezier = PressureProfile::preset_bezier((0.25, 0.1), (0.25, 1.0));
        assert_eq!(bezier.evaluate(0.0), 0.0);
        assert_eq!(bezier.evaluate(1.0), 1.0);

        let mut prev = 0.0;
        for i in 1..=20 {
            let p = i as f64 / 20.0;
            let val = bezier.evaluate(p);
            assert!(val >= prev, "Non-monotonic at p = {p}: {val} < {prev}");
            prev = val;
        }
    }

    #[test]
    fn test_veikk_s640_preset() {
        let veikk = PressureProfile::preset_veikk_s640();
        // Zero in deadzone (< 0.018)
        assert_eq!(veikk.evaluate(0.01), 0.0);
        // Saturated at top (> 0.982)
        assert_eq!(veikk.evaluate(0.99), 1.0);
    }
}
