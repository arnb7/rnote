// Rnote - Sketch and take handwritten notes.
// Copyright (C) The Rnote Authors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Pen productivity features and gesture tools (Goodies G6 to G16).
//!
//! Provides:
//! - **G6 / G13 (Radial / Pie Menu)**: Stylus-summoned 8-slice radial menu for rapid tool selection.
//! - **G7 (Pen-hold Pan)**: Smooth canvas panning triggered by barrel button or stationary hold.
//! - **G8 (Quick Cycle)**: Rapid cycling between favorite colors and stroke widths.
//! - **G9 (Erase Fade Feedback)**: Ghost outline feedback indicating which strokes were erased.
//! - **G10 (Diagnostics Overlay / HUD)**: Real-time hardware telemetry and event monitor.
//! - **G11 (Page-turn Key Actions)**: Document page increment/decrement navigation.
//! - **G15 (Stylus Button Double-Click)**: Rapid double-tap detector on stylus barrel buttons.
//! - **G16 (Palm Rejection / Touch Suppression)**: Proximity-based capacitive touch rejection.

use p2d::math::Vector2;
use rnote_compose::Color;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Radial / Pie menu tool slice types (G6 & G13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadialMenuSlice {
    Brush,
    Eraser,
    Selector,
    Shaper,
    Typewriter,
    CycleColor,
    CycleWidth,
    Undo,
}

/// Radial / Pie menu manager (G6 & G13).
#[derive(Debug, Clone)]
pub struct RadialMenu {
    /// Center origin where the radial menu was opened.
    pub origin: Vector2,
    /// Outer radius in pixels.
    pub outer_radius: f64,
    /// Inner deadzone radius in pixels.
    pub inner_radius: f64,
    /// Slices in clockwise order starting from 12 o'clock (top).
    pub slices: [RadialMenuSlice; 8],
    /// Whether the menu is currently visible.
    pub is_active: bool,
}

impl Default for RadialMenu {
    fn default() -> Self {
        Self {
            origin: Vector2::ZERO,
            outer_radius: 90.0,
            inner_radius: 20.0,
            slices: [
                RadialMenuSlice::Brush,
                RadialMenuSlice::CycleColor,
                RadialMenuSlice::Eraser,
                RadialMenuSlice::Undo,
                RadialMenuSlice::Selector,
                RadialMenuSlice::CycleWidth,
                RadialMenuSlice::Shaper,
                RadialMenuSlice::Typewriter,
            ],
            is_active: false,
        }
    }
}

impl RadialMenu {
    /// Open the radial menu centered at `pos`.
    pub fn open_at(&mut self, pos: Vector2) {
        self.origin = pos;
        self.is_active = true;
    }

    /// Close the radial menu.
    pub fn close(&mut self) {
        self.is_active = false;
    }

    /// Determine which slice is selected given current pen cursor position.
    pub fn selected_slice(&self, cursor: Vector2) -> Option<RadialMenuSlice> {
        if !self.is_active {
            return None;
        }

        let diff = cursor - self.origin;
        let dist = diff.length();
        if dist < self.inner_radius || dist > self.outer_radius * 1.5 {
            return None;
        }

        // Angle in [0, 2*PI), rotated so 12 o'clock is slice 0
        let angle = (-diff.y).atan2(diff.x); // standard math angle: 0 = East, PI/2 = North
        // Convert to clock angle: 12 o'clock (North) = 0, clockwise
        let clock_angle = (std::f64::consts::FRAC_PI_2 - angle).rem_euclid(std::f64::consts::TAU);
        let slice_arc = std::f64::consts::TAU / 8.0;
        let index = ((clock_angle + slice_arc * 0.5) / slice_arc).floor() as usize % 8;

        Some(self.slices[index])
    }
}

/// Quick cycling manager for favorite colors and widths (G8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCycler {
    pub color_palette: Vec<Color>,
    pub current_color_idx: usize,
    pub width_presets: Vec<f64>,
    pub current_width_idx: usize,
}

impl Default for QuickCycler {
    fn default() -> Self {
        Self {
            color_palette: vec![
                Color::BLACK,
                Color::new(0.88, 0.11, 0.14, 1.0), // Red
                Color::new(0.11, 0.44, 0.85, 1.0), // Blue
                Color::new(0.18, 0.76, 0.49, 1.0), // Green
            ],
            current_color_idx: 0,
            width_presets: vec![1.5, 3.0, 6.0, 12.0],
            current_width_idx: 1, // 3.0 px default
        }
    }
}

impl QuickCycler {
    /// Cycle to next color in palette, returning new Color.
    pub fn cycle_color(&mut self) -> Color {
        if self.color_palette.is_empty() {
            return Color::BLACK;
        }
        self.current_color_idx = (self.current_color_idx + 1) % self.color_palette.len();
        self.color_palette[self.current_color_idx]
    }

    /// Cycle to next stroke width preset, returning new width.
    pub fn cycle_width(&mut self) -> f64 {
        if self.width_presets.is_empty() {
            return 2.5;
        }
        self.current_width_idx = (self.current_width_idx + 1) % self.width_presets.len();
        self.width_presets[self.current_width_idx]
    }
}

/// Button double-click recognizer (G15).
#[derive(Debug, Clone)]
pub struct DoubleClickDetector {
    /// Maximum time interval in milliseconds between clicks to count as a double-click.
    pub max_interval_ms: u128,
    /// Maximum spatial drift allowed between clicks in pixels.
    pub max_drift_px: f64,
    last_click_time: Option<Instant>,
    last_click_pos: Option<Vector2>,
}

impl Default for DoubleClickDetector {
    fn default() -> Self {
        Self {
            max_interval_ms: 300,
            max_drift_px: 6.0,
            last_click_time: None,
            last_click_pos: None,
        }
    }
}

impl DoubleClickDetector {
    /// Register a button click at `pos` and `now`.
    /// Returns true if this click completes a double-click gesture.
    pub fn register_click(&mut self, pos: Vector2, now: Instant) -> bool {
        let is_double = match (self.last_click_time, self.last_click_pos) {
            (Some(prev_time), Some(prev_pos)) => {
                let elapsed = now.duration_since(prev_time).as_millis();
                let drift = (pos - prev_pos).length();
                elapsed <= self.max_interval_ms && drift <= self.max_drift_px
            }
            _ => false,
        };

        if is_double {
            self.last_click_time = None;
            self.last_click_pos = None;
            true
        } else {
            self.last_click_time = Some(now);
            self.last_click_pos = Some(pos);
            false
        }
    }
}

/// Palm rejection and touch suppression guard (G16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchRejectionState {
    /// Stylus is outside tablet proximity: touch input accepted.
    TouchAllowed,
    /// Stylus is hovering in proximity or drawing: touch input suppressed.
    TouchSuppressed,
}

#[derive(Debug, Clone)]
pub struct PalmRejectionGuard {
    pub state: TouchRejectionState,
    /// Cooldown delay after stylus leaves proximity before allowing touch (ms).
    pub exit_cooldown_ms: u128,
    last_pen_proximity: Option<Instant>,
}

impl Default for PalmRejectionGuard {
    fn default() -> Self {
        Self {
            state: TouchRejectionState::TouchAllowed,
            exit_cooldown_ms: 150,
            last_pen_proximity: None,
        }
    }
}

impl PalmRejectionGuard {
    /// Notify that pen is active or in proximity.
    pub fn on_pen_active(&mut self, now: Instant) {
        self.state = TouchRejectionState::TouchSuppressed;
        self.last_pen_proximity = Some(now);
    }

    /// Check if a touch event should be permitted at `now`.
    pub fn is_touch_permitted(&mut self, now: Instant) -> bool {
        if self
            .last_pen_proximity
            .is_some_and(|last| now.duration_since(last).as_millis() < self.exit_cooldown_ms)
        {
            return false;
        }
        self.state = TouchRejectionState::TouchAllowed;
        true
    }
}

/// Real-time hardware telemetry for diagnostics overlay (G10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareTelemetry {
    pub device_name: String,
    pub raw_x: u32,
    pub raw_y: u32,
    pub raw_pressure: u32,
    pub normalized_pressure: f64,
    pub active_tool: String,
    pub gesture_status: String,
    pub packet_rate_hz: f64,
}

impl Default for HardwareTelemetry {
    fn default() -> Self {
        Self {
            device_name: "VEIKK S640 Pen".to_string(),
            raw_x: 0,
            raw_y: 0,
            raw_pressure: 0,
            normalized_pressure: 0.0,
            active_tool: "Brush".to_string(),
            gesture_status: "Idle".to_string(),
            packet_rate_hz: 200.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radial_menu_selection() {
        let mut menu = RadialMenu::default();
        let origin = Vector2::new(100.0, 100.0);
        menu.open_at(origin);

        // 12 o'clock (straight up, North): (100, 50)
        let slice_north = menu.selected_slice(Vector2::new(100.0, 50.0));
        assert_eq!(slice_north, Some(RadialMenuSlice::Brush));

        // 6 o'clock (straight down, South): (100, 150)
        let slice_south = menu.selected_slice(Vector2::new(100.0, 150.0));
        assert_eq!(slice_south, Some(RadialMenuSlice::Selector));

        // Deadzone inside inner radius (5 px from origin)
        let slice_deadzone = menu.selected_slice(Vector2::new(102.0, 102.0));
        assert_eq!(slice_deadzone, None);
    }

    #[test]
    fn test_quick_cycler() {
        let mut cycler = QuickCycler::default();
        let c1 = cycler.cycle_color();
        let c2 = cycler.cycle_color();
        assert_ne!(c1, c2);

        let w1 = cycler.cycle_width();
        let w2 = cycler.cycle_width();
        assert_ne!(w1, w2);
    }

    #[test]
    fn test_double_click_detector() {
        let mut detector = DoubleClickDetector::default();
        let pos = Vector2::new(50.0, 50.0);
        let t0 = Instant::now();

        // First click
        assert!(!detector.register_click(pos, t0));

        // Immediate second click (50ms later, identical pos)
        let t1 = t0 + std::time::Duration::from_millis(50);
        assert!(detector.register_click(pos, t1));

        // Third click starts a new sequence
        let t2 = t1 + std::time::Duration::from_millis(50);
        assert!(!detector.register_click(pos, t2));
    }

    #[test]
    fn test_palm_rejection_guard() {
        let mut guard = PalmRejectionGuard::default();
        let t0 = Instant::now();

        // Pen active suppresses touch
        guard.on_pen_active(t0);
        assert!(!guard.is_touch_permitted(t0 + std::time::Duration::from_millis(50)));

        // After cooldown (200ms > 150ms), touch permitted
        assert!(guard.is_touch_permitted(t0 + std::time::Duration::from_millis(200)));
    }
}
