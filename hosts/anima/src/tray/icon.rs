//! The tray icon, drawn in code: the Anima ring with a status dot at its
//! lower right.
//!
//! A macOS template image keeps only the alpha channel (the system colours it
//! for the menu bar), so there the status is told by shape instead of colour:
//! no dot while running, a hollow dot while starting or waiting, a filled dot
//! for an update, and a faded ring when stopped. The menu says it in words too.

use super::menu::TrayIconKind;

/// Width and height in pixels.
pub const SIZE: u32 = 32;

const CENTER: f32 = 16.0;
const RING_OUTER: f32 = 13.5;
const RING_INNER: f32 = 9.5;
const DOT_CENTER: f32 = 25.0;
const DOT_RADIUS: f32 = 6.0;
const DOT_HOLE: f32 = 3.0;
/// The ring is cut away this far around the dot, so the two stay apart.
const DOT_CLEARANCE: f32 = 8.5;

const RING_COLOUR: [u8; 3] = [138, 143, 152];
const TEMPLATE_COLOUR: [u8; 3] = [0, 0, 0];
const STOPPED_TEMPLATE_ALPHA: f32 = 0.4;

/// How much of the pixel a shape's edge at signed distance `inside` covers.
fn coverage(inside: f32) -> f32 {
    (inside + 0.5).clamp(0.0, 1.0)
}

fn dot_colour(kind: TrayIconKind) -> [u8; 3] {
    match kind {
        TrayIconKind::Running => [52, 199, 89],
        TrayIconKind::Starting | TrayIconKind::Waiting => [255, 159, 10],
        TrayIconKind::Stopped => [255, 69, 58],
        TrayIconKind::Update => [10, 132, 255],
    }
}

/// `SIZE`×`SIZE` RGBA pixels: coloured, or a macOS template when `template`.
pub fn rgba(kind: TrayIconKind, template: bool) -> Vec<u8> {
    // The dot's hole radius (0 for a filled dot), or no dot at all.
    let dot = match (template, kind) {
        (true, TrayIconKind::Running | TrayIconKind::Stopped) => None,
        (true, TrayIconKind::Starting | TrayIconKind::Waiting) => Some(DOT_HOLE),
        _ => Some(0.0),
    };
    let ring_alpha = if template && kind == TrayIconKind::Stopped {
        STOPPED_TEMPLATE_ALPHA
    } else {
        1.0
    };
    let (ring_colour, dot_colour) = if template {
        (TEMPLATE_COLOUR, TEMPLATE_COLOUR)
    } else {
        (RING_COLOUR, dot_colour(kind))
    };
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let from_center = (px - CENTER).hypot(py - CENTER);
            let from_dot = (px - DOT_CENTER).hypot(py - DOT_CENTER);
            let mut ring =
                coverage(RING_OUTER - from_center).min(coverage(from_center - RING_INNER));
            let mut dot_alpha = 0.0;
            if let Some(hole) = dot {
                ring *= coverage(from_dot - DOT_CLEARANCE);
                dot_alpha = coverage(DOT_RADIUS - from_dot);
                if hole > 0.0 {
                    dot_alpha = dot_alpha.min(coverage(from_dot - hole));
                }
            }
            ring *= ring_alpha;
            let (colour, alpha) = if dot_alpha > ring {
                (dot_colour, dot_alpha)
            } else {
                (ring_colour, ring)
            };
            pixels.extend_from_slice(&colour);
            pixels.push((alpha * 255.0).round() as u8);
        }
    }
    pixels
}
