use crate::{kitty, palette::Theme};
use std::io::{self, Write};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Top,
    Middle,
    Bottom,
    Single,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Row {
    pub fill: &'static str,
    pub accent: &'static str,
    pub edge: Edge,
}

impl Row {
    pub fn upload(
        self,
        out: &mut impl Write,
        theme: &Theme,
        cell: (usize, usize),
        columns: usize,
        playback: kitty::Playback,
    ) -> io::Result<kitty::Sprite> {
        // Kitty scales placements to the font. Bound the raster work when the
        // window is zoomed, using the same scale factor on both pixel axes.
        let scale = cell.0.div_ceil(16).max(cell.1.div_ceil(32));
        let cell = (cell.0.div_ceil(scale), cell.1.div_ceil(scale));
        let size = (cell.0 * columns, cell.1);
        let fill = kitty::rgb(theme.color(self.fill));
        let accent = kitty::rgb(theme.color(self.accent));
        let pixels = self.pixels(size, cell.0, fill, accent);
        kitty::Sprite::upload(out, size, playback, |phase| reveal(&pixels, size, phase))
    }

    fn pixels(
        self,
        (width, height): (usize, usize),
        cell_width: usize,
        fill: [u8; 3],
        accent: [u8; 3],
    ) -> Vec<u8> {
        let w = width as f32;
        let h = height as f32;
        let radius = (h * 0.42).min(w / 2.);
        let rail = cell_width as f32 * 0.83;
        let stroke = (h * 0.05).max(0.65);
        let top = matches!(self.edge, Edge::Top | Edge::Single);
        let bottom = matches!(self.edge, Edge::Bottom | Edge::Single);
        kitty::raster(width, height, |x, y| {
            // Apply the radii only to the outer edges of a connected section.
            let corner_y = if top && y < radius {
                radius
            } else if bottom && y > h - radius {
                h - radius
            } else {
                y
            };
            if (x - x.clamp(radius, w - radius)).hypot(y - corner_y) > radius {
                return None;
            }
            let start = if top { h * 0.54 } else { 0. };
            let end = if bottom { h * 0.46 } else { h };
            let on_rail =
                self.edge != Edge::Single && (x - rail).hypot(y - y.clamp(start, end)) <= stroke;
            let on_node = (top || bottom) && (x - rail).hypot(y - h * 0.5) <= stroke * 1.9;
            Some(if on_rail || on_node { accent } else { fill })
        })
    }
}

fn reveal(pixels: &[u8], (width, height): (usize, usize), phase: f32) -> Vec<u8> {
    let mut frame = pixels.to_vec();
    let eased = 1. - (1. - phase).powi(3);
    let opacity = (0..width)
        .map(|x| {
            let edge = (eased * 1.6 - x as f32 / width as f32 + 0.5).clamp(0., 1.);
            0.30 + 0.70 * edge
        })
        .collect::<Vec<_>>();
    for row in frame.chunks_exact_mut(width * 4).take(height) {
        for (pixel, alpha) in row.as_chunks_mut::<4>().0.iter_mut().zip(&opacity) {
            pixel[3] = (f32::from(pixel[3]) * alpha).round() as u8;
        }
    }
    frame
}
