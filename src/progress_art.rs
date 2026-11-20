use crate::{kitty, palette::Theme};
use std::f32::consts::TAU;

pub const HEIGHT: usize = 24;
pub const MAX_COLUMNS: usize = 18;

pub fn width(columns: usize) -> usize {
    columns * 12
}

pub fn busy(width: usize, theme: &Theme, styled: bool, phase: f32) -> Vec<u8> {
    let time = phase * TAU;
    let end = width as f32 - 7.;
    // The endpoints move out of phase, stretching the wave as it travels.
    let start = 35. + (end - 35.) * (0.14 - 0.14 * time.cos());
    let tip = 35. + (end - 35.) * (0.70 - 0.23 * (time - 0.65).cos());
    let (ink, background) = track_colors(theme, styled);
    let (fill, container) = if styled {
        ("on_primary_container", "primary_container")
    } else {
        ("on_surface", "surface_container_high")
    };
    let chip = kitty::rgb(theme.color(container));
    let shape = kitty::rgb(theme.accessible(fill, container));
    let lobes = 0.06 + 0.11 * (0.5 + 0.5 * time.sin());
    kitty::raster(width, HEIGHT, |x, y| {
        let (dx, dy) = (x - 12., y - 12.);
        let radius = dx.hypot(dy);
        if x < 24. && radius <= 11. {
            let angle = 4. * dy.atan2(dx) - time;
            return Some(if radius <= 7. * (1. + lobes * angle.cos()) {
                shape
            } else {
                chip
            });
        }
        if wave(x, y, start, tip, time * 2.) {
            Some(ink)
        } else if kitty::rounded(x, y, [28., 5., width as f32 - 1., 19.], 7.) {
            Some(background)
        } else {
            None
        }
    })
}

pub fn measured(width: usize, theme: &Theme, styled: bool, percent: u8) -> Vec<u8> {
    let end = width as f32 - 7.;
    let tip = 7. + (end - 7.) * f32::from(percent) / 100.;
    let (ink, background) = track_colors(theme, styled);
    let stop = kitty::rgb(theme.accessible("outline", "surface_container_high"));
    kitty::raster(width, HEIGHT, |x, y| {
        if percent > 0 && wave(x, y, 7., tip, 0.) {
            Some(ink)
        } else if percent < 100 && x > tip + 4. && (x - end).hypot(y - 12.) <= 1.5 {
            Some(stop)
        } else if kitty::rounded(x, y, [1., 5., width as f32 - 1., 19.], 7.) {
            Some(background)
        } else {
            None
        }
    })
}

fn track_colors(theme: &Theme, styled: bool) -> ([u8; 3], [u8; 3]) {
    let role = if styled { "primary" } else { "on_surface" };
    (
        kitty::rgb(theme.accessible(role, "surface_container_high")),
        kitty::rgb(theme.color("surface_container_high")),
    )
}

fn wave(x: f32, y: f32, from: f32, to: f32, shift: f32) -> bool {
    const RADIUS: f32 = 2.5;
    if x < from - RADIUS || x > to + RADIUS {
        return false;
    }
    let point = x.clamp(from, to);
    let middle = 12. + 3.1 * (point * TAU / 34. - shift).sin();
    (x - point).powi(2) + (y - middle).powi(2) <= RADIUS.powi(2)
}
