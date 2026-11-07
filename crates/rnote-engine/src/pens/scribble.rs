// Rnote - Sketch and take handwritten notes.
// Copyright (C) The Rnote Authors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Scribble-to-erase gesture recognition and target selection engine (G1).
//!
//! Evaluates candidate pen strokes on pen-up. When a stroke represents a rapid,
//! dense back-and-forth zigzag or scribble gesture over existing ink, it discards
//! the scribble stroke itself and atomically trashes the targeted word or strokes.

use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

/// Mode for scribble target selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename = "scribble_mode")]
pub enum ScribbleMode {
    /// Erase only the strokes directly covered by the scribble.
    #[serde(rename = "stroke")]
    Stroke,
    /// Group covered strokes into words by proximity and erase the entire word (Default).
    #[default]
    #[serde(rename = "word")]
    Word,
    /// Erase the entire horizontal text line cluster.
    #[serde(rename = "line")]
    Line,
}

/// Configuration options for scribble-to-erase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "scribble_config")]
pub struct ScribbleConfig {
    /// Whether scribble-to-erase is enabled.
    #[serde(rename = "enable")]
    pub enable: bool,
    /// Minimum direction reversals required (typically 3 to 6; default 4).
    #[serde(rename = "sensitivity")]
    pub sensitivity: usize,
    /// Target selection mode (Word, Stroke, or Line).
    #[serde(rename = "mode")]
    pub mode: ScribbleMode,
    /// Enable strike-through erasure (straight horizontal strike). Off by default.
    #[serde(rename = "strike_through")]
    pub strike_through: bool,
    /// Minimum path length to bounding box diagonal ratio (default 3.2).
    #[serde(rename = "path_to_diag_ratio")]
    pub path_to_diag_ratio: f64,
    /// Minimum coverage fraction of a stroke to qualify for erasure (default 0.50).
    #[serde(rename = "coverage_threshold")]
    pub coverage_threshold: f64,
    /// Minimum fraction of scribble points that must overlap existing ink (Ink Gate, default 0.60).
    #[serde(rename = "ink_gate_threshold")]
    pub ink_gate_threshold: f64,
    /// Proximity radius in pixels for considering an ink point covered (default 18.0).
    #[serde(rename = "ink_gate_proximity_px")]
    pub ink_gate_proximity_px: f64,
    /// Maximum allowed duration for a scribble in seconds (default 1.5s).
    #[serde(rename = "max_duration_secs")]
    pub max_duration_secs: f64,
}

impl Default for ScribbleConfig {
    fn default() -> Self {
        Self {
            enable: true,
            sensitivity: 4,
            mode: ScribbleMode::Word,
            strike_through: false,
            path_to_diag_ratio: 2.5,
            coverage_threshold: 0.35,
            ink_gate_threshold: 0.35,
            ink_gate_proximity_px: 16.0,
            max_duration_secs: 2.2,
        }
    }
}

/// Result of evaluating a stroke for scribble gestures.
#[derive(Debug, Clone, PartialEq)]
pub struct ScribbleAnalysis {
    /// True if the stroke qualifies as a scribble or strike-through gesture.
    pub is_scribble: bool,
    /// Count of sharp directional reversals detected.
    pub reversal_count: usize,
    /// Total path length of the stroke.
    pub path_length: f64,
    /// Bounding box diagonal.
    pub diagonal: f64,
    /// Bounding box of the scribble.
    pub bounds: Aabb,
    /// Uniformly resampled points along the stroke.
    pub resampled_points: Vec<Vector2>,
}

impl Default for ScribbleAnalysis {
    fn default() -> Self {
        Self {
            is_scribble: false,
            reversal_count: 0,
            path_length: 0.0,
            diagonal: 0.0,
            bounds: Aabb::new_invalid(),
            resampled_points: Vec::new(),
        }
    }
}

/// Core gesture analysis algorithms.
#[derive(Debug)]
pub struct ScribbleDetector;

impl ScribbleDetector {
    /// Resample a sequence of polyline points to a uniform arc length step.
    pub fn resample(points: &[Vector2], step: f64) -> Vec<Vector2> {
        if points.len() < 2 || step <= 0.0 {
            return points.to_vec();
        }

        let mut resampled = Vec::with_capacity(points.len());
        resampled.push(points[0]);

        let mut dist_accum = 0.0;
        let mut curr_idx = 0;
        let mut curr_pt = points[0];

        while curr_idx + 1 < points.len() {
            let next_pt = points[curr_idx + 1];
            let segment_len = (next_pt - curr_pt).length();

            if segment_len < 1e-6 {
                curr_idx += 1;
                curr_pt = next_pt;
                continue;
            }

            if dist_accum + segment_len >= step {
                let t = (step - dist_accum) / segment_len;
                let interp = curr_pt + (next_pt - curr_pt) * t;
                resampled.push(interp);
                curr_pt = interp;
                dist_accum = 0.0;
            } else {
                dist_accum += segment_len;
                curr_idx += 1;
                curr_pt = next_pt;
            }
        }

        if points
            .last()
            .copied()
            .is_some_and(|last| (last - *resampled.last().unwrap()).length() > step * 0.5)
        {
            resampled.push(*points.last().unwrap());
        }

        resampled
    }

    /// Ramer-Douglas-Peucker (RDP) polyline simplification.
    pub fn rdp_simplify(points: &[Vector2], epsilon: f64) -> Vec<Vector2> {
        if points.len() < 3 || epsilon <= 0.0 {
            return points.to_vec();
        }

        let mut max_dist = 0.0;
        let mut max_idx = 0;
        let start = points[0];
        let end = *points.last().unwrap();
        let line_vec = end - start;
        let line_len = line_vec.length();

        for (i, &pt) in points.iter().enumerate().take(points.len() - 1).skip(1) {
            let dist = if line_len < 1e-6 {
                (pt - start).length()
            } else {
                let cross = (pt.x - start.x) * line_vec.y - (pt.y - start.y) * line_vec.x;
                cross.abs() / line_len
            };

            if dist > max_dist {
                max_dist = dist;
                max_idx = i;
            }
        }

        if max_dist > epsilon {
            let mut left = Self::rdp_simplify(&points[..=max_idx], epsilon);
            let right = Self::rdp_simplify(&points[max_idx..], epsilon);
            left.pop();
            left.extend(right);
            left
        } else {
            vec![start, end]
        }
    }

    /// Count direction reversals where cosine of turning angle < cos_threshold.
    ///
    /// For 90 degree turning reversals: cos(90 deg) = 0.0.
    pub fn count_reversals(simplified: &[Vector2], cos_threshold: f64) -> usize {
        if simplified.len() < 3 {
            return 0;
        }

        let mut reversals = 0;
        for i in 1..simplified.len() - 1 {
            let v1 = simplified[i] - simplified[i - 1];
            let v2 = simplified[i + 1] - simplified[i];
            let len1 = v1.length();
            let len2 = v2.length();

            if len1 > 1e-4 && len2 > 1e-4 {
                let cos_theta = v1.dot(v2) / (len1 * len2);
                if cos_theta < cos_threshold {
                    reversals += 1;
                }
            }
        }
        reversals
    }


    fn count_1d_reversals(projections: &[f64], min_amplitude: f64) -> usize {
        let min_proj = projections.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_proj = projections.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let span = max_proj - min_proj;
        if span < min_amplitude {
            return 0;
        }

        let hysteresis = (span * 0.22).max(7.0);
        let mut reversals = 0;
        enum Dir {
            Init,
            Increasing(f64),
            Decreasing(f64),
        }
        let mut state = Dir::Init;
        let init_val = projections[0];

        for &val in &projections[1..] {
            match state {
                Dir::Init => {
                    if val - init_val >= hysteresis {
                        state = Dir::Increasing(val);
                    } else if init_val - val >= hysteresis {
                        state = Dir::Decreasing(val);
                    }
                }
                Dir::Increasing(peak) => {
                    if val > peak {
                        state = Dir::Increasing(val);
                    } else if peak - val >= hysteresis {
                        reversals += 1;
                        state = Dir::Decreasing(val);
                    }
                }
                Dir::Decreasing(valley) => {
                    if val < valley {
                        state = Dir::Decreasing(val);
                    } else if val - valley >= hysteresis {
                        reversals += 1;
                        state = Dir::Increasing(val);
                    }
                }
            }
        }

        if reversals < 3 {
            0
        } else {
            reversals
        }
    }

    /// Count directional oscillations / reversals along the principal and perpendicular axes of the stroke.
    /// This is robust against rounded corners from Google stroke-modeler and orientation variations.
    pub fn count_principal_reversals(points: &[Vector2], min_amplitude: f64) -> usize {
        if points.len() < 4 {
            return 0;
        }

        let mut mean_x = 0.0;
        let mut mean_y = 0.0;
        for p in points {
            mean_x += p.x;
            mean_y += p.y;
        }
        mean_x /= points.len() as f64;
        mean_y /= points.len() as f64;

        let mut c_xx = 0.0;
        let mut c_yy = 0.0;
        let mut c_xy = 0.0;
        for p in points {
            let dx = p.x - mean_x;
            let dy = p.y - mean_y;
            c_xx += dx * dx;
            c_yy += dy * dy;
            c_xy += dx * dy;
        }

        let phi = 0.5 * (2.0 * c_xy).atan2(c_xx - c_yy);
        let u = Vector2::new(phi.cos(), phi.sin());
        let v = Vector2::new(-u.y, u.x);

        let proj_u: Vec<f64> = points
            .iter()
            .map(|p| (p.x - mean_x) * u.x + (p.y - mean_y) * u.y)
            .collect();
        let proj_v: Vec<f64> = points
            .iter()
            .map(|p| (p.x - mean_x) * v.x + (p.y - mean_y) * v.y)
            .collect();

        let rev_u = Self::count_1d_reversals(&proj_u, min_amplitude);
        let rev_v = Self::count_1d_reversals(&proj_v, min_amplitude);

        rev_u.max(rev_v)
    }

    /// Analyze a stroke's geometric trajectory and verify scribble criteria.
    pub fn analyze(
        points: &[Vector2],
        duration_secs: f64,
        config: &ScribbleConfig,
    ) -> ScribbleAnalysis {
        if points.len() < 4 {
            return ScribbleAnalysis {
                is_scribble: false,
                reversal_count: 0,
                path_length: 0.0,
                diagonal: 0.0,
                bounds: Aabb::from_points(points.iter().copied()),
                resampled_points: points.to_vec(),
            };
        }

        let bounds = Aabb::from_points(points.iter().copied());
        let extents = bounds.extents();
        let diagonal = (extents.x * extents.x + extents.y * extents.y).sqrt();

        // 1. Resample to uniform arc length (step ~5.0 document units)
        let resampled = Self::resample(points, 5.0);

        // Compute total path length
        let mut path_len = 0.0;
        for w in resampled.windows(2) {
            path_len += (w[1] - w[0]).length();
        }

        // 2. RDP simplification with epsilon ~ 3.0
        let simplified = Self::rdp_simplify(&resampled, 3.0);

        // 3. Count directional reversals: combine angle reversals (for sharp corners)
        // and principal axis oscillations (for smoothed/modeled curves).
        let reversal_ang = Self::count_reversals(&simplified, -0.55); // turning angle > ~123°
        let reversal_pca = Self::count_principal_reversals(&resampled, 14.0);
        let reversal_count = reversal_ang.max(reversal_pca);

        // Duration guard: Scribbles are rapid gestures (< max_duration_secs)
        let duration_valid = duration_secs <= config.max_duration_secs;

        // Path-to-diagonal ratio guard: scribbles pack high path length into small diagonal
        let ratio = if diagonal > 1e-3 {
            path_len / diagonal
        } else {
            0.0
        };

        // Standard zigzag scribble criterion
        let is_zigzag_scribble = duration_valid
            && reversal_count >= config.sensitivity
            && ratio >= config.path_to_diag_ratio
            && diagonal >= 10.0;

        // Strike-through criterion (optional horizontal strike)
        let is_strike_through = if config.strike_through && duration_valid {
            let is_horizontal = extents.x > 25.0 && extents.y < 15.0;
            let straightness = if path_len > 1e-3 {
                extents.x / path_len
            } else {
                0.0
            };
            is_horizontal && straightness > 0.80 && reversal_count <= 1
        } else {
            false
        };

        let is_scribble = is_zigzag_scribble || is_strike_through;

        ScribbleAnalysis {
            is_scribble,
            reversal_count,
            path_length: path_len,
            diagonal,
            bounds,
            resampled_points: resampled,
        }
    }

    /// Evaluate the Ink Gate: verify that >= ink_gate_threshold of scribble points
    /// are situated within distance of existing ink strokes (points or line segments).
    pub fn passes_ink_gate(
        scribble_points: &[Vector2],
        existing_stroke_points: &[Vector2],
        proximity_radius: f64,
        gate_threshold: f64,
    ) -> bool {
        if scribble_points.is_empty() || existing_stroke_points.is_empty() {
            return false;
        }

        let radius_sq = proximity_radius * proximity_radius;
        let mut on_ink_count = 0;

        for sp in scribble_points {
            let hits = if existing_stroke_points.len() == 1 {
                (*sp - existing_stroke_points[0]).length_squared() <= radius_sq
            } else {
                existing_stroke_points.windows(2).any(|w| {
                    let a = w[0];
                    let b = w[1];
                    let ab = b - a;
                    let l2 = ab.length_squared();
                    if l2 < 1e-6 {
                        (*sp - a).length_squared() <= radius_sq
                    } else {
                        let t = ((*sp - a).dot(ab) / l2).clamp(0.0, 1.0);
                        let proj = a + ab * t;
                        (*sp - proj).length_squared() <= radius_sq
                    }
                })
            };

            if hits {
                on_ink_count += 1;
            }
        }

        let ratio = on_ink_count as f64 / scribble_points.len() as f64;
        ratio >= gate_threshold
    }

    /// Direct evaluation of ink gate using stroke bounding hitboxes.
    pub fn passes_ink_gate_hitboxes(
        scribble_points: &[Vector2],
        candidate_hitboxes: &[Aabb],
        proximity_radius: f64,
        gate_threshold: f64,
    ) -> bool {
        if scribble_points.is_empty() || candidate_hitboxes.is_empty() {
            return false;
        }

        let mut on_ink_count = 0;
        for &sp in scribble_points {
            let hits = candidate_hitboxes.iter().any(|hb| {
                sp.x >= hb.mins.x - proximity_radius
                    && sp.x <= hb.maxs.x + proximity_radius
                    && sp.y >= hb.mins.y - proximity_radius
                    && sp.y <= hb.maxs.y + proximity_radius
            });
            if hits {
                on_ink_count += 1;
            }
        }

        let ratio = on_ink_count as f64 / scribble_points.len() as f64;
        ratio >= gate_threshold
    }

    /// Compute coverage of an existing target stroke by the scribble region.
    pub fn compute_stroke_coverage(target_points: &[Vector2], scribble_bounds: &Aabb) -> f64 {
        if target_points.is_empty() {
            return 0.0;
        }

        let mut inside_count = 0;
        for pt in target_points {
            if pt.x >= scribble_bounds.mins.x
                && pt.x <= scribble_bounds.maxs.x
                && pt.y >= scribble_bounds.mins.y
                && pt.y <= scribble_bounds.maxs.y
            {
                inside_count += 1;
            }
        }

        inside_count as f64 / target_points.len() as f64
    }
}

/// Spatial and temporal word clustering for natural word-level erasure.
#[derive(Debug, Clone)]
pub struct WordGroup<K: Copy + PartialEq> {
    pub keys: Vec<K>,
    pub bounds: Aabb,
}

impl<K: Copy + PartialEq> WordGroup<K> {
    /// Group strokes into words by horizontal gap and vertical overlap.
    pub fn cluster_words(
        strokes: &[(K, Aabb)],
        median_height: f64,
        gap_multiplier: f64,
    ) -> Vec<WordGroup<K>> {
        if strokes.is_empty() {
            return Vec::new();
        }

        let max_horizontal_gap = (median_height * gap_multiplier).max(12.0);
        let mut groups: Vec<WordGroup<K>> = Vec::new();

        for &(key, bounds) in strokes {
            let mut merged = false;

            for group in groups.iter_mut() {
                // Check if stroke belongs to word group (adjacent horizontally on same text baseline)
                let y_overlap = (group.bounds.maxs.y.min(bounds.maxs.y)
                    - group.bounds.mins.y.max(bounds.mins.y))
                .max(0.0);
                let x_gap = if bounds.mins.x > group.bounds.maxs.x {
                    bounds.mins.x - group.bounds.maxs.x
                } else if group.bounds.mins.x > bounds.maxs.x {
                    group.bounds.mins.x - bounds.maxs.x
                } else {
                    0.0
                };

                if y_overlap > 0.3 * median_height && x_gap <= max_horizontal_gap {
                    group.keys.push(key);
                    group.bounds.merge(&bounds);
                    merged = true;
                    break;
                }
            }

            if !merged {
                groups.push(WordGroup {
                    keys: vec![key],
                    bounds,
                });
            }
        }

        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_zigzag(
        turns: usize,
        width: f64,
        height: f64,
        origin: Vector2,
        points_per_turn: usize,
    ) -> Vec<Vector2> {
        let mut points = Vec::new();
        let step_y = height / turns.max(1) as f64;

        for t in 0..turns {
            let y0 = origin.y + t as f64 * step_y;
            let y1 = y0 + step_y;
            let (x0, x1) = if t % 2 == 0 {
                (origin.x, origin.x + width)
            } else {
                (origin.x + width, origin.x)
            };

            for p in 0..points_per_turn {
                let frac = p as f64 / points_per_turn as f64;
                points.push(Vector2::new(x0 + (x1 - x0) * frac, y0 + (y1 - y0) * frac));
            }
        }
        points
    }

    #[test]
    fn test_synthetic_zigzag_positive() {
        let config = ScribbleConfig::default();
        let zigzag = generate_zigzag(5, 80.0, 30.0, Vector2::new(100.0, 100.0), 10);
        let analysis = ScribbleDetector::analyze(&zigzag, 0.45, &config);

        assert!(
            analysis.is_scribble,
            "Synthetic zigzag must be recognized as a scribble"
        );
        assert!(
            analysis.reversal_count >= config.sensitivity,
            "Reversal count must meet or exceed threshold: {}",
            analysis.reversal_count
        );
        assert!(
            analysis.path_length / analysis.diagonal >= config.path_to_diag_ratio,
            "Ratio must meet density requirement"
        );
    }

    #[test]
    fn test_synthetic_hatching_positive() {
        let config = ScribbleConfig::default();
        let mut hatching = Vec::new();
        for i in 0..6 {
            let x = 100.0 + i as f64 * 8.0;
            if i % 2 == 0 {
                hatching.push(Vector2::new(x, 50.0));
                hatching.push(Vector2::new(x + 4.0, 85.0));
            } else {
                hatching.push(Vector2::new(x, 85.0));
                hatching.push(Vector2::new(x + 4.0, 50.0));
            }
        }
        let analysis = ScribbleDetector::analyze(&hatching, 0.35, &config);
        assert!(analysis.is_scribble);
    }

    #[test]
    fn test_synthetic_circle_negative() {
        let config = ScribbleConfig::default();
        let mut circle = Vec::new();
        let radius = 25.0;
        let center = Vector2::new(100.0, 100.0);
        for i in 0..64 {
            let theta = (i as f64 / 64.0) * std::f64::consts::TAU;
            circle.push(Vector2::new(
                center.x + radius * theta.cos(),
                center.y + radius * theta.sin(),
            ));
        }

        let analysis = ScribbleDetector::analyze(&circle, 0.50, &config);
        assert!(
            !analysis.is_scribble,
            "Circle must NEVER trigger scribble-to-erase"
        );
        assert_eq!(
            analysis.reversal_count, 0,
            "Circle has 0 sharp direction reversals"
        );
    }

    #[test]
    fn test_synthetic_underline_negative() {
        let config = ScribbleConfig::default();
        let underline: Vec<Vector2> = (0..50)
            .map(|i| Vector2::new(50.0 + i as f64 * 3.0, 150.0 + (i as f64 * 0.1).sin()))
            .collect();

        let analysis = ScribbleDetector::analyze(&underline, 0.20, &config);
        assert!(
            !analysis.is_scribble,
            "Straight underline must NEVER trigger scribble-to-erase"
        );
        assert_eq!(analysis.reversal_count, 0);
    }

    #[test]
    fn test_synthetic_single_wave_negative() {
        let config = ScribbleConfig::default();
        let wave: Vec<Vector2> = (0..60)
            .map(|i| {
                let x = i as f64 * 2.0;
                let y = (x * 0.1).sin() * 10.0;
                Vector2::new(x, y)
            })
            .collect();

        let analysis = ScribbleDetector::analyze(&wave, 0.30, &config);
        assert!(
            !analysis.is_scribble,
            "Gentle wave must not be recognized as scribble"
        );
    }

    #[test]
    fn test_handwriting_letter_w_negative() {
        let config = ScribbleConfig::default();
        // Cursive / fast-written 'w': 2 valleys, 1 peak = at most 2-3 reversals
        let w_letter = vec![
            Vector2::new(10.0, 20.0),
            Vector2::new(15.0, 45.0),
            Vector2::new(20.0, 50.0),
            Vector2::new(25.0, 25.0),
            Vector2::new(30.0, 50.0),
            Vector2::new(35.0, 45.0),
            Vector2::new(40.0, 20.0),
        ];

        let analysis = ScribbleDetector::analyze(&w_letter, 0.25, &config);
        assert!(
            !analysis.is_scribble,
            "Handwritten 'w' must NEVER trigger scribble-to-erase"
        );
        assert!(
            analysis.reversal_count < config.sensitivity,
            "Handwritten 'w' reversals ({}) must stay below sensitivity ({})",
            analysis.reversal_count,
            config.sensitivity
        );
    }

    #[test]
    fn test_handwriting_letter_m_negative() {
        let config = ScribbleConfig::default();
        // Handwritten 'm': 3 down-strokes, 2 arches
        let m_letter = vec![
            Vector2::new(10.0, 50.0),
            Vector2::new(10.0, 20.0),
            Vector2::new(15.0, 15.0),
            Vector2::new(20.0, 50.0),
            Vector2::new(25.0, 15.0),
            Vector2::new(30.0, 50.0),
        ];

        let analysis = ScribbleDetector::analyze(&m_letter, 0.25, &config);
        assert!(
            !analysis.is_scribble,
            "Handwritten 'm' must NEVER trigger scribble-to-erase"
        );
    }

    #[test]
    fn test_property_rotation_and_scale_invariance() {
        let config = ScribbleConfig::default();
        let base_zigzag = generate_zigzag(6, 90.0, 35.0, Vector2::new(50.0, 50.0), 8);

        // Test scaling by 0.5x, 1.5x, 2.5x
        for &scale in &[0.5, 1.0, 1.8, 2.5] {
            let scaled: Vec<Vector2> = base_zigzag.iter().map(|p| *p * scale).collect();
            let analysis = ScribbleDetector::analyze(&scaled, 0.40, &config);
            assert!(
                analysis.is_scribble,
                "Scribble must remain positive under scale {}",
                scale
            );
        }

        // Test rotation by 30°, 45°, 90°, 180°
        for &angle_deg in &[30.0, 45.0, 90.0, 180.0, 270.0] {
            let rad = angle_deg * std::f64::consts::PI / 180.0;
            let cos_a = rad.cos();
            let sin_a = rad.sin();
            let rotated: Vec<Vector2> = base_zigzag
                .iter()
                .map(|p| Vector2::new(p.x * cos_a - p.y * sin_a, p.x * sin_a + p.y * cos_a))
                .collect();

            let analysis = ScribbleDetector::analyze(&rotated, 0.40, &config);
            assert!(
                analysis.is_scribble,
                "Scribble must remain positive under rotation {}°",
                angle_deg
            );
        }
    }

    #[test]
    fn test_ink_gate_rejection_on_empty_canvas() {
        let scribble = generate_zigzag(5, 70.0, 30.0, Vector2::new(500.0, 500.0), 10);
        let existing_ink = vec![
            Vector2::new(10.0, 10.0),
            Vector2::new(20.0, 10.0),
            Vector2::new(30.0, 10.0),
        ];

        let passes = ScribbleDetector::passes_ink_gate(&scribble, &existing_ink, 15.0, 0.60);
        assert!(
            !passes,
            "Scribble on empty canvas with no nearby ink must FAIL ink gate"
        );
    }

    #[test]
    fn test_ink_gate_acceptance_over_ink() {
        let scribble = generate_zigzag(5, 70.0, 30.0, Vector2::new(100.0, 100.0), 10);
        // Place ink directly under scribble
        let mut existing_ink = Vec::new();
        for x in 90..180 {
            existing_ink.push(Vector2::new(x as f64, 115.0));
        }

        let passes = ScribbleDetector::passes_ink_gate(&scribble, &existing_ink, 18.0, 0.60);
        assert!(
            passes,
            "Scribble drawn over existing text line must PASS ink gate"
        );
    }

    #[test]
    fn test_word_clustering() {
        let strokes = vec![
            // Word 1: "cat"
            (
                1,
                Aabb::new(Vector2::new(10.0, 50.0), Vector2::new(25.0, 80.0)),
            ),
            (
                2,
                Aabb::new(Vector2::new(28.0, 50.0), Vector2::new(42.0, 80.0)),
            ),
            (
                3,
                Aabb::new(Vector2::new(45.0, 50.0), Vector2::new(60.0, 80.0)),
            ),
            // Large gap (space between words)
            // Word 2: "dog"
            (
                4,
                Aabb::new(Vector2::new(110.0, 50.0), Vector2::new(125.0, 80.0)),
            ),
            (
                5,
                Aabb::new(Vector2::new(128.0, 50.0), Vector2::new(142.0, 80.0)),
            ),
        ];

        let words = WordGroup::cluster_words(&strokes, 30.0, 1.2);
        assert_eq!(words.len(), 2, "Expected 2 clustered words");
        assert_eq!(words[0].keys, vec![1, 2, 3]);
        assert_eq!(words[1].keys, vec![4, 5]);
    }
}
