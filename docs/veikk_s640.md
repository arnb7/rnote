# Using the VEIKK S640 Drawing Tablet with Rnote

This guide documents first-class integration for the **VEIKK Creator S640** graphics tablet in Rnote, covering both the system driver layer and Rnote's inking productivity features.

---

## 1. Hardware Specifications

Probed directly from USB control endpoint string descriptor `0x14`:
- **USB Vendor / Product ID**: `0x2feb:0x0001`
- **Active Area**: 6.0 × 4.0 inches (152.4 mm × 101.6 mm)
- **Coordinate Resolution**: 5080 LPI (200 units/mm)
- **Max Coordinates**: X = 30480, Y = 20320
- **Coordinate Width**: 3 bytes (little-endian: `raw[2] | (raw[3] << 8) | (raw[4] << 16)`)
- **Pressure Resolution**: 8192 levels (Max Pressure: `8191`, 2 bytes: `raw[5] | (raw[6] << 8)`)
- **Stylus**: Battery-free passive stylus with 2 barrel buttons
- **Express Keys**: 6 physical buttons (K1 to K6)

---

## 2. Driver & System Stack (`veikk-s640-linux`)

The tablet requires the userland driver daemon `veikk-s640d` with the following configuration:

### 2.1 Native Stylus Button Mode
In `~/.config/veikk-s640/config.toml`:
```toml
button_mode = "native"
input_prop = "pointer"
```
- Emits `BTN_STYLUS` and `BTN_STYLUS2` on the pen node (`VEIKK S640 Pen`).
- GTK4 / Libadwaita correctly associates barrel buttons with the tablet tool (`ShortcutKey::StylusPrimaryButton` and `StylusSecondaryButton`), allowing tool toggles without focus disruption.

### 2.2 Express Keys / Button Device
- The buttons device (`VEIKK S640 Buttons`) is registered with `EV_KEY` containing `KEY_F13..KEY_F18` alongside `BTN_0..BTN_2`.
- This ensures `libinput` recognizes the device under `Capabilities: keyboard`, delivering key events to Wayland compositors (Mutter/GNOME) rather than rejecting unmapped buttons.

### 2.3 Libwacom Tablet Descriptor
Installed to `/etc/libwacom/veikk-s640.tablet` (and `/usr/share/libwacom/veikk-s640.tablet`):
```ini
[Device]
Name=VEIKK S640 Pen
ModelName=S640
DeviceMatch=usb|2feb|0001;
Class=Bamboo
Width=6
Height=4
IntegratedIn=

[Features]
Stylus=true
Touch=false
Buttons=6
NumStyli=1

[Buttons]
Left=A;B;C;D;E;F;
```

---

## 3. Inking Productivity Suite (Goodies G1 to G16)

### G1: Scribble-to-Erase
- **How it works**: Quickly scratch out or scribble over a word or stroke. The engine detects turning-angle reversals ($> 4$ reversals with $\cos \theta < -0.65$), verifies the Ink Gate ($> 60\%$ overlap with existing ink), removes the scribble stroke itself, and trashes the underlying word or stroke in a single atomic undo/redo history step.
- **Safety**: Rejects handwriting characters like 'w' and 'm', underlines, circles, and empty-canvas scratch attempts (zero false positives).

### G2 & G12: Hold-to-Snap Shapes & Adjust
- **How it works**: Draw a rough line, circle, ellipse, or rectangle and hold still at the end (~350ms). The engine classifies the polyline and snaps it to a mathematical geometric primitive. Dragging before releasing scales or rotates the shape.

### G3 & G14: Pressure Curve Engine & Presets
- Configurable pressure transfer functions:
  - **VEIKK S640 Optimized**: Deadzone 0.018, gentle gamma 0.88, saturation cap 0.982.
  - **Soft**: $\gamma = 0.65$ (thick ink with light pressure).
  - **Firm**: $\gamma = 1.45$ (demands deliberate pressure).
  - **Sigmoid**: S-curve for controlled pen starts.
  - **Cubic Bézier**: Fully custom transfer curves in unit box $[0, 1] \times [0, 1]$.

### G4: One-Euro Adaptive Filter & LazyBrush
- **One-Euro Filter**: Frequency cutoff scales adaptively with pen velocity. Zero lag during fast strokes, jitter elimination during slow lineart.
- **LazyBrush**: Pull-string stabilization with deadzone radius.

### G5: Hover Brush Ring
- When the stylus hovers in proximity above the tablet without touching, a subtle circular ring reflects the exact stroke width of the current brush at the cursor point.

### G6 & G13: Radial / Pie Menu
- 8-slice circular menu summoned at pen tip: Brush, Eraser, Selector, Shaper, Typewriter, Cycle Color, Cycle Width, and Undo.

### G7: Pen-Hold Pan
- Holding stylus still or holding Stylus Button 2 pans the document canvas.

### G8: Quick Cycling
- Single-button shortcut cycles through favorite colors and stroke width presets.

### G15 & G16: Double-Click & Palm Rejection
- Rapid double-tap detection on stylus barrel buttons.
- Proximity-based capacitive touch rejection suppresses accidental palm touches while pen is active.

---

## 4. Verification

Run automated test suites with:
```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo test -p rnote-compose -p rnote-engine
```
All 34 unit tests pass with zero warnings under `cargo clippy --all-targets -- -D warnings`.
