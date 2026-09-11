//! Monochrome palette ported 1-to-1 from the Swift `Theme.swift`.
//!
//! Light mode: near-black ink on Apple grouped grays.
//! Dark mode: white ink on near-black groups.
//! Blue (`brand`) is reserved for live progress, links, the overall-score
//! gauge, and the active-character outline — everything else stays grayscale
//! so lesson text keeps the visual focus. Typing errors keep one muted
//! system red, matching the original app.
//!
//! Concrete RGB values mirror the Swift dynamic colors:
//! * light: literal values from `Theme.swift`;
//! * dark: macOS system colors resolved to sRGB approximations that match
//!   the Swift screenshots (`windowBackground` ~ #2B2D31, control surfaces,
//!   separator, secondary/tertiary labels, `#0A84FF` brand).

use egui::{Color32, Stroke, Visuals};

/// Detect dark mode from the current egui style (follows the OS).
pub fn is_dark(ctx: &egui::Context) -> bool {
    ctx.style().visuals.dark_mode
}

// --- Light (literal from Swift) ---
const LIGHT_BG: Color32 = Color32::from_rgb(245, 245, 247); // 0.961
const LIGHT_PANEL: Color32 = Color32::WHITE;
const LIGHT_PANEL2: Color32 = Color32::from_rgb(232, 232, 237); // 0.910
const LIGHT_INK: Color32 = Color32::from_rgb(29, 29, 31); // 0.114
const LIGHT_MUTED: Color32 = Color32::from_rgb(110, 110, 115); // 0.431
const LIGHT_BORDER: Color32 = Color32::from_rgb(210, 210, 216); // 0.824
const LIGHT_PENDING: Color32 = Color32::from_rgb(174, 174, 178); // 0.682
const LIGHT_KEYCAP: Color32 = Color32::WHITE;
const LIGHT_BRAND: Color32 = Color32::from_rgb(0, 122, 255); // #007AFF
const LIGHT_ERROR: Color32 = Color32::from_rgb(215, 0, 21); // 0.843,0,0.082
const LIGHT_ERROR_TIMEOUT: Color32 = Color32::from_rgb(165, 0, 0); // 0.647,0,0

// --- Dark (resolved system colors, matched to Swift screenshots) ---
const DARK_BG: Color32 = Color32::from_rgb(43, 45, 49); // windowBackground
const DARK_PANEL: Color32 = Color32::from_rgb(30, 30, 32); // controlBackground
const DARK_PANEL2: Color32 = Color32::from_rgb(58, 60, 64); // separator ~0.45 over bg
const DARK_INK: Color32 = Color32::WHITE;
const DARK_MUTED: Color32 = Color32::from_rgb(152, 152, 157); // secondaryLabel
const DARK_BORDER: Color32 = Color32::from_rgb(58, 58, 60); // separator
const DARK_PENDING: Color32 = Color32::from_rgb(108, 108, 112); // tertiaryLabel
const DARK_KEYCAP: Color32 = Color32::from_rgb(58, 60, 64); // controlColor
const DARK_BRAND: Color32 = Color32::from_rgb(10, 132, 255); // #0A84FF
const DARK_ERROR: Color32 = Color32::from_rgb(255, 69, 58); // 1.0,0.271,0.227
const DARK_ERROR_TIMEOUT: Color32 = Color32::from_rgb(217, 51, 46); // 0.85,0.2,0.18

// --- Backward-compatible aliases (dark defaults, used by non-UI code) ---
pub const BG: Color32 = DARK_BG;
pub const PANEL: Color32 = DARK_PANEL;
pub const PANEL2: Color32 = DARK_PANEL2;
pub const ACCENT: Color32 = DARK_BRAND;
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0, 105, 219);
pub const ACCENT2: Color32 = DARK_BRAND;
pub const TEXT: Color32 = DARK_INK;
pub const MUTED: Color32 = DARK_MUTED;
pub const CORRECT: Color32 = DARK_INK;
pub const TIMEOUT: Color32 = DARK_MUTED;
pub const ERROR: Color32 = DARK_ERROR;
pub const ERROR_TIMEOUT: Color32 = DARK_ERROR_TIMEOUT;
pub const KEY: Color32 = DARK_KEYCAP;
pub const BORDER: Color32 = DARK_BORDER;
pub const SUCCESS_BTN: Color32 = DARK_BRAND;

pub fn background(dark: bool) -> Color32 {
    if dark { DARK_BG } else { LIGHT_BG }
}
pub fn panel(dark: bool) -> Color32 {
    if dark { DARK_PANEL } else { LIGHT_PANEL }
}
pub fn panel2(dark: bool) -> Color32 {
    if dark { DARK_PANEL2 } else { LIGHT_PANEL2 }
}
pub fn ink(dark: bool) -> Color32 {
    if dark { DARK_INK } else { LIGHT_INK }
}
pub fn muted(dark: bool) -> Color32 {
    if dark { DARK_MUTED } else { LIGHT_MUTED }
}
pub fn border(dark: bool) -> Color32 {
    if dark { DARK_BORDER } else { LIGHT_BORDER }
}
pub fn pending(dark: bool) -> Color32 {
    if dark { DARK_PENDING } else { LIGHT_PENDING }
}
pub fn keycap(dark: bool) -> Color32 {
    if dark { DARK_KEYCAP } else { LIGHT_KEYCAP }
}
pub fn brand(dark: bool) -> Color32 {
    if dark { DARK_BRAND } else { LIGHT_BRAND }
}
pub fn error(dark: bool) -> Color32 {
    if dark { DARK_ERROR } else { LIGHT_ERROR }
}
pub fn error_timeout(dark: bool) -> Color32 {
    if dark { DARK_ERROR_TIMEOUT } else { LIGHT_ERROR_TIMEOUT }
}

// ---------------------------------------------------------------------------
// Finger zones (Swift `FingerZone` + `fingerTint`, 4-color tutor scheme)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerZone {
    Pinky,
    Ring,
    Middle,
    Index,
    Thumb,
}

/// Subtle pastel per finger (neutral keycap for the thumb/space bar),
/// shared by the keycaps and the legend. Exact Swift values.
pub fn finger_tint(zone: FingerZone, dark: bool) -> Color32 {
    match (zone, dark) {
        (FingerZone::Pinky, false) => Color32::from_rgb(215, 230, 247),
        (FingerZone::Ring, false) => Color32::from_rgb(214, 237, 221),
        (FingerZone::Middle, false) => Color32::from_rgb(244, 236, 210),
        (FingerZone::Index, false) => Color32::from_rgb(242, 218, 210),
        (FingerZone::Thumb, false) => LIGHT_KEYCAP,
        (FingerZone::Pinky, true) => Color32::from_rgb(43, 60, 82),
        (FingerZone::Ring, true) => Color32::from_rgb(41, 64, 52),
        (FingerZone::Middle, true) => Color32::from_rgb(74, 66, 39),
        (FingerZone::Index, true) => Color32::from_rgb(78, 51, 43),
        (FingerZone::Thumb, true) => DARK_KEYCAP,
    }
}

/// Standard finger per key position. Exact maps for the usual 4-row
/// shapes; proportional fallback for anything else (e.g. custom layouts
/// with different row lengths). Port of Swift `fingerZone(row:column:count:)`.
pub fn finger_zone(row: usize, column: usize, count: usize) -> FingerZone {
    // Exact Swift maps (13 / 13 / 11 / 10 entries).
    const ROW0: [FingerZone; 13] = [
        FingerZone::Pinky, FingerZone::Pinky, FingerZone::Ring, FingerZone::Middle,
        FingerZone::Index, FingerZone::Index, FingerZone::Index, FingerZone::Index,
        FingerZone::Middle, FingerZone::Ring, FingerZone::Pinky, FingerZone::Pinky,
        FingerZone::Pinky,
    ];
    const ROW1: [FingerZone; 13] = [
        FingerZone::Pinky, FingerZone::Ring, FingerZone::Middle, FingerZone::Index,
        FingerZone::Index, FingerZone::Index, FingerZone::Index, FingerZone::Middle,
        FingerZone::Ring, FingerZone::Pinky, FingerZone::Pinky, FingerZone::Pinky,
        FingerZone::Pinky,
    ];
    const ROW2: [FingerZone; 11] = [
        FingerZone::Pinky, FingerZone::Ring, FingerZone::Middle, FingerZone::Index,
        FingerZone::Index, FingerZone::Index, FingerZone::Index, FingerZone::Middle,
        FingerZone::Ring, FingerZone::Pinky, FingerZone::Pinky,
    ];
    const ROW3: [FingerZone; 10] = [
        FingerZone::Pinky, FingerZone::Ring, FingerZone::Middle, FingerZone::Index,
        FingerZone::Index, FingerZone::Index, FingerZone::Index, FingerZone::Middle,
        FingerZone::Ring, FingerZone::Pinky,
    ];
    let exact: Option<FingerZone> = match row {
        0 if count == ROW0.len() => Some(ROW0[column]),
        1 if count == ROW1.len() => Some(ROW1[column]),
        2 if count == ROW2.len() => Some(ROW2[column]),
        3 if count == ROW3.len() => Some(ROW3[column]),
        _ => None,
    };
    if let Some(zone) = exact {
        return zone;
    }
    if count <= 1 {
        return FingerZone::Index;
    }
    let position = column as f64 / (count - 1).max(1) as f64;
    if position < 0.125 {
        FingerZone::Pinky
    } else if position < 0.25 {
        FingerZone::Ring
    } else if position < 0.375 {
        FingerZone::Middle
    } else if position < 0.625 {
        FingerZone::Index
    } else if position < 0.75 {
        FingerZone::Middle
    } else if position < 0.875 {
        FingerZone::Ring
    } else {
        FingerZone::Pinky
    }
}

/// Legacy 8-zone helper kept for compatibility; maps onto the 4-zone scheme.
pub const FINGER_COLORS: [Color32; 8] = [
    Color32::from_rgb(43, 60, 82),
    Color32::from_rgb(41, 64, 52),
    Color32::from_rgb(74, 66, 39),
    Color32::from_rgb(78, 51, 43),
    Color32::from_rgb(78, 51, 43),
    Color32::from_rgb(74, 66, 39),
    Color32::from_rgb(41, 64, 52),
    Color32::from_rgb(43, 60, 82),
];

pub fn finger_color(col: usize, len: usize) -> Color32 {
    if len == 0 {
        return DARK_KEYCAP;
    }
    let zone = (col * 8 / len.max(1)).min(7);
    FINGER_COLORS[zone]
}

// ---------------------------------------------------------------------------
// Character states: Swift maps correct->ink, slow->muted (NOT green/yellow)
// ---------------------------------------------------------------------------

/// Swift `CharacterCell.color`: correct = ink, timeout(slow) = muted,
/// error = red, errorTimeout = dark red, pending = pending gray.
pub fn state_color(state: &str) -> Color32 {
    state_color_dark(state, true)
}

pub fn state_color_dark(state: &str, dark: bool) -> Color32 {
    match state {
        "correct" => ink(dark),
        "timeout" => muted(dark),
        "error" => error(dark),
        "error_timeout" => error_timeout(dark),
        _ => pending(dark),
    }
}

// ---------------------------------------------------------------------------
// Capsule button helpers (Swift `AppButtonStyle`: 32pt pills)
// ---------------------------------------------------------------------------

/// Primary pill: ink fill (white in dark / near-black in light).
pub fn primary_button(label: egui::RichText, dark: bool) -> egui::Button<'static> {
    let fill = ink(dark);
    let text = if dark { Color32::BLACK } else { Color32::WHITE };
    egui::Button::new(label.color(text))
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(16))
        .min_size(egui::vec2(0.0, 32.0))
}

/// Secondary pill: neutral panel2 fill, ink text.
pub fn secondary_button(label: egui::RichText, dark: bool) -> egui::Button<'static> {
    egui::Button::new(label.color(ink(dark)))
        .fill(panel2(dark))
        .corner_radius(egui::CornerRadius::same(16))
        .min_size(egui::vec2(0.0, 32.0))
}

/// Brand pill: blue fill (progress-linked actions: Start/Pause, Save, Next).
pub fn brand_button(label: egui::RichText, dark: bool) -> egui::Button<'static> {
    egui::Button::new(label.color(Color32::WHITE))
        .fill(brand(dark))
        .corner_radius(egui::CornerRadius::same(16))
        .min_size(egui::vec2(0.0, 32.0))
}

/// Grouped card: panel fill + hairline border, radius 12 (Swift `Card`).
pub fn card_frame(dark: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(panel(dark))
        .stroke(egui::Stroke::new(1.0_f32, border(dark)))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

/// Custom toggle switch (Swift `Toggle`). Returns clicked state change.
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, dark: bool) -> egui::Response {
    let size = egui::vec2(42.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let radius = 12.0;
    let bg = if *on {
        brand(dark)
    } else {
        panel2(dark)
    };
    ui.painter().rect_filled(rect, radius, bg);
    let knob_x = if *on { rect.right() - 12.0 } else { rect.left() + 12.0 };
    ui.painter().circle_filled(
        egui::pos2(knob_x, rect.center().y),
        9.0,
        Color32::WHITE,
    );
    response
}

pub fn apply(ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.dark_mode = dark;
    visuals.override_text_color = Some(ink(dark));
    visuals.panel_fill = background(dark);
    visuals.window_fill = panel(dark);
    visuals.faint_bg_color = panel2(dark);
    visuals.extreme_bg_color = panel(dark);
    visuals.code_bg_color = panel(dark);
    visuals.warn_fg_color = muted(dark);
    visuals.error_fg_color = error(dark);
    visuals.window_stroke = Stroke::new(1.0_f32, border(dark));
    visuals.window_corner_radius = egui::CornerRadius::same(14);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.noninteractive.bg_fill = panel(dark);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, ink(dark));
    visuals.widgets.inactive.bg_fill = panel2(dark);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, ink(dark));
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.hovered.bg_fill = panel2(dark);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.active.bg_fill = brand(dark);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    visuals.selection.bg_fill = brand(dark);
    visuals.selection.stroke = Stroke::new(1.0_f32, ink(dark));
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(22.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}
