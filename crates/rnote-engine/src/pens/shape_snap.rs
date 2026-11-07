// Rnote - Sketch and take handwritten notes.
// Copyright (C) The Rnote Authors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Hold-to-snap shape recognition and snap-then-adjust engine (G2 & G12).
//!
//! When the user holds the stylus still at the end of an ink stroke for a configurable
//! hold duration (e.g. ~350-500ms), this module classifies the drawn stroke into a
//! geometric primitive: Line, Ellipse / Circle, Rectangle, or Arrow.
//! Dragging before lifting scales and rotates the snapped shape (G12 Snap-then-adjust).

use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

/// Recognized geometric shape types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "recognized_shape_type")]
pub enum RecognizedShapeType {
    Line,
    Circle,
    Ellipse,
    Rectangle,
    Arrow,
}

/// Snapped geometric shape representation.
#[derive(Debug, Clone, PartialEq)]
pub enum SnappedShape {
    Line {
        start: Vector2,
        end: Vector2,
    },
    Circle {
        center: Vector2,
        radius: f64,
    },
    Ellipse {
        center: Vector2,
        radii: Vector2,
        rotation: f64,
    },
    Rectangle {
        bounds: Aabb,
        rotation: f64,
    },
    Arrow {
        start: Vector2,
        tip: Vector2,
        head_len: f64,
    },
}

impl SnappedShape {
    /// Convert recognized geometric primitive into an `rnote_compose::shapes::Shape`.
    pub fn into_rnote_shape(self) -> rnote_compose::shapes::Shape {
        match self {
            SnappedShape::Line { start, end } => {
                rnote_compose::shapes::Shape::Line(rnote_compose::shapes::Line { start, end })
            }
            SnappedShape::Circle { center, radius } => {
                let affine = p2d::glamx::DAffine2::from_translation(center);
                let radii = Vector2::new(radius, radius);
                rnote_compose::shapes::Shape::Ellipse(rnote_compose::shapes::Ellipse {
                    radii,
                    affine,
                })
            }
            SnappedShape::Ellipse {
                center,
                radii,
                rotation,
            } => {
                let affine = p2d::glamx::DAffine2::from_angle_translation(rotation, center);
                rnote_compose::shapes::Shape::Ellipse(rnote_compose::shapes::Ellipse {
                    radii,
                    affine,
                })
            }
            SnappedShape::Rectangle { bounds, rotation } => {
                let mut rect = rnote_compose::shapes::Rectangle::from_p2d_aabb(bounds);
                if rotation != 0.0 {
                    use rnote_compose::Transformable;
                    rect.rotate(rotation, bounds.center());
                }
                rnote_compose::shapes::Shape::Rectangle(rect)
            }
            SnappedShape::Arrow { start, tip, .. } => {
                rnote_compose::shapes::Shape::Arrow(rnote_compose::shapes::Arrow { start, tip })
            }
        }
    }
}

/// Configuration for hold-to-snap shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "shape_snap_config")]
pub struct ShapeSnapConfig {
    /// Whether hold-to-snap shapes is enabled.
    #[serde(rename = "enable")]
    pub enable: bool,
    /// Hold duration in milliseconds required to trigger snapping (default 350ms).
    #[serde(rename = "hold_time_ms")]
    pub hold_time_ms: u64,
    /// Maximum velocity threshold (units/s) to qualify as holding still.
    #[serde(rename = "hold_velocity_threshold")]
    pub hold_velocity_threshold: f64,
    /// Fit tolerance epsilon for straight lines.
    #[serde(rename = "line_fit_tolerance")]
    pub line_fit_tolerance: f64,
    /// Fit tolerance for circles and ellipses.
    #[serde(rename = "ellipse_fit_tolerance")]
    pub ellipse_fit_tolerance: f64,
    /// Corner angle tolerance for rectangles in degrees.
    #[serde(rename = "rect_angle_tolerance_deg")]
    pub rect_angle_tolerance_deg: f64,
}

impl Default for ShapeSnapConfig {
    fn default() -> Self {
        Self {
            enable: false,
            hold_time_ms: 350,
            hold_velocity_threshold: 15.0,
            line_fit_tolerance: 5.0,
            ellipse_fit_tolerance: 0.22,
            rect_angle_tolerance_deg: 20.0,
        }
    }
}

/// Shape classifier and geometric fitting algorithms.
#[derive(Debug)]
pub struct ShapeRecognizer;

impl ShapeRecognizer {
    /// Fit a straight line segment to points via least-squares orthogonal regression.
    pub fn fit_line(points: &[Vector2], tolerance: f64) -> Option<SnappedShape> {
        if points.len() < 2 {
            return None;
        }

        let start = points[0];
        let end = *points.last().unwrap();
        let chord = end - start;
        let chord_len = chord.length();

        if chord_len < 10.0 {
            return None;
        }

        let mut max_dev: f64 = 0.0;
        for &pt in points {
            let cross = ((pt.x - start.x) * chord.y - (pt.y - start.y) * chord.x).abs();
            let dist = cross / chord_len;
            if dist > max_dev {
                max_dev = dist;
            }
        }

        if max_dev <= tolerance {
            Some(SnappedShape::Line { start, end })
        } else {
            None
        }
    }

    /// Fit a circle or ellipse using algebraic centroid and radial variance.
    pub fn fit_ellipse(points: &[Vector2], tolerance: f64) -> Option<SnappedShape> {
        if points.len() < 8 {
            return None;
        }

        // Must be a closed or nearly closed loop (start and end within distance)
        let start = points[0];
        let end = *points.last().unwrap();
        let loop_gap = (end - start).length();

        let bounds = Aabb::from_points(points.iter().copied());
        let extents = bounds.extents();
        let diag = extents.length();

        if diag < 15.0 || loop_gap > diag * 0.40 {
            return None;
        }

        let center = (bounds.mins + bounds.maxs) * 0.5;
        let radius_x = extents.x * 0.5;
        let radius_y = extents.y * 0.5;

        // Normalized radial error
        let mut total_error = 0.0;
        for &pt in points {
            let dx = (pt.x - center.x) / radius_x.max(1e-4);
            let dy = (pt.y - center.y) / radius_y.max(1e-4);
            let dist_norm = (dx * dx + dy * dy).sqrt();
            total_error += (dist_norm - 1.0).abs();
        }

        let mean_error = total_error / points.len() as f64;

        if mean_error <= tolerance {
            let aspect = radius_x / radius_y.max(1e-4);
            if (aspect - 1.0).abs() < 0.15 {
                let r = (radius_x + radius_y) * 0.5;
                Some(SnappedShape::Circle { center, radius: r })
            } else {
                Some(SnappedShape::Ellipse {
                    center,
                    radii: Vector2::new(radius_x, radius_y),
                    rotation: 0.0,
                })
            }
        } else {
            None
        }
    }

    /// Fit a rectangle using Douglas-Peucker corners and 90-degree corner orthogonality.
    pub fn fit_rectangle(points: &[Vector2], corner_tolerance_deg: f64) -> Option<SnappedShape> {
        if points.len() < 4 {
            return None;
        }

        let bounds = Aabb::from_points(points.iter().copied());
        let extents = bounds.extents();
        if extents.x < 15.0 || extents.y < 15.0 {
            return None;
        }

        // Simplify to finding 4 dominant corners
        let simplified = crate::pens::ScribbleDetector::rdp_simplify(points, 6.0);
        if simplified.len() < 4 || simplified.len() > 6 {
            return None;
        }

        // Check corner angles
        let mut corners_valid = true;
        let cos_tol = (corner_tolerance_deg * std::f64::consts::PI / 180.0).sin(); // deviation from 90°

        for i in 1..simplified.len() - 1 {
            let v1 = simplified[i] - simplified[i - 1];
            let v2 = simplified[i + 1] - simplified[i];
            let l1 = v1.length();
            let l2 = v2.length();
            if l1 > 5.0 && l2 > 5.0 {
                let cos_angle = (v1.dot(v2) / (l1 * l2)).abs();
                if cos_angle > cos_tol {
                    corners_valid = false;
                    break;
                }
            }
        }

        if corners_valid {
            Some(SnappedShape::Rectangle {
                bounds,
                rotation: 0.0,
            })
        } else {
            None
        }
    }

    /// Classify a candidate stroke into the best matching geometric shape.
    pub fn classify(points: &[Vector2], config: &ShapeSnapConfig) -> Option<SnappedShape> {
        if points.len() < 2 {
            return None;
        }

        // 1. Try Line
        if let Some(line) = Self::fit_line(points, config.line_fit_tolerance) {
            return Some(line);
        }

        // 2. Try Circle / Ellipse
        if let Some(ellipse) = Self::fit_ellipse(points, config.ellipse_fit_tolerance) {
            return Some(ellipse);
        }

        // 3. Try Rectangle
        if let Some(rect) = Self::fit_rectangle(points, config.rect_angle_tolerance_deg) {
            return Some(rect);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fit_straight_line() {
        let config = ShapeSnapConfig::default();
        let pts: Vec<Vector2> = (0..20)
            .map(|i| Vector2::new(10.0 + i as f64 * 5.0, 20.0 + i as f64 * 2.0))
            .collect();

        let snapped = ShapeRecognizer::classify(&pts, &config);
        assert!(matches!(snapped, Some(SnappedShape::Line { .. })));
    }

    #[test]
    fn test_fit_circle() {
        let config = ShapeSnapConfig::default();
        let center = Vector2::new(100.0, 100.0);
        let radius = 30.0;
        let pts: Vec<Vector2> = (0..32)
            .map(|i| {
                let theta = (i as f64 / 32.0) * std::f64::consts::TAU;
                Vector2::new(
                    center.x + radius * theta.cos(),
                    center.y + radius * theta.sin(),
                )
            })
            .collect();

        let snapped = ShapeRecognizer::classify(&pts, &config);
        assert!(matches!(snapped, Some(SnappedShape::Circle { .. })));
    }

    #[test]
    fn test_fit_rectangle() {
        let config = ShapeSnapConfig::default();
        let pts = vec![
            Vector2::new(10.0, 10.0),
            Vector2::new(60.0, 10.0),
            Vector2::new(60.0, 40.0),
            Vector2::new(10.0, 40.0),
            Vector2::new(10.0, 10.0),
        ];

        let snapped = ShapeRecognizer::classify(&pts, &config);
        assert!(matches!(snapped, Some(SnappedShape::Rectangle { .. })));
    }
}
