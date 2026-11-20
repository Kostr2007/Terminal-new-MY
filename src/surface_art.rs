use crate::{kitty, motion, palette::Theme};
use std::io::{self, Write};

pub const CELL: (usize, usize) = (6, 12);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Surface {
    pub fill: &'static str,
    pub accent: &'static str,
    pub rows: usize,
}

impl Surface {
    pub fn upload(
        self,
        out: &mut impl Write,
        theme: &Theme,
        columns: usize,
        (cw, ch): (usize, usize),
        playback: kitty::Playback,
    ) -> io::Result<kitty::Sprite> {
        // Use a small canvas for the flat surfaces. Kitty scales it to the
        // placement, independently of the user's font size and zoom.
        let size = (columns * cw, self.rows * ch);
        let fill = kitty::rgb(theme.color(self.fill));
        let accent = kitty::rgb(theme.color(self.accent));
        kitty::Sprite::upload(out, size, playback, |phase| {
            let (position, opacity) = if phase == 1. {
                (1., 1.)
            } else {
                let seconds =
                    phase * f32::from(motion::SECTION_STEPS) * f32::from(motion::SECTION_FRAME_MS)
                        / 1000.;
                (
                    motion::SECTION.sample(0., 0., 1., seconds).0,
                    motion::effect(seconds),
                )
            };
            let opening = 1. - position;
            let (cw, ch) = (cw as f32, ch as f32);
            let left = ch * 0.075 + opening * cw * 0.46;
            let right = size.0 as f32 - cw - opening * (size.0 as f32 * 0.42).min(cw * 12.);
            let vertical_inset = ch * (0.08 + opening * if self.rows == 1 { 0.26 } else { 0.5 });
            let bottom = size.1 as f32 - vertical_inset;
            let radius = (ch * (0.52 + opening * 0.16)).min((bottom - vertical_inset) / 2.);
            pixels(
                size,
                [left, vertical_inset, right, bottom],
                radius,
                (cw, ch),
                (fill, accent),
                opacity,
            )
        })
    }
}

fn pixels(
    (width, height): (usize, usize),
    [left, top, right, bottom]: [f32; 4],
    radius: f32,
    (cw, ch): (f32, f32),
    (fill, accent): ([u8; 3], [u8; 3]),
    opacity: f32,
) -> Vec<u8> {
    let rail = left + cw * 0.75;
    let inset = (ch * 0.48).min((bottom - top) * 0.5);
    let start = top + inset;
    let end = bottom - inset;
    let center = (top + bottom) * 0.5;
    let straight = ((bottom - top) * 0.5 - radius).max(0.);
    let mut pixels = vec![0; width * height * 4];
    if opacity == 0. {
        return pixels;
    }
    for (y, line) in pixels.chunks_exact_mut(width * 4).enumerate() {
        let position = y as f32 + 0.5;
        let vertical = (bottom.min(y as f32 + 1.) - top.max(y as f32)).clamp(0., 1.);
        if vertical == 0. {
            continue;
        }
        let arc = ((position - center).abs() - straight).clamp(0., radius);
        let indent = radius - (radius * radius - arc * arc).sqrt();
        let (from, to) = (left + indent, right - indent);
        let first = from.floor().max(0.) as usize;
        let end_pixel = (to.ceil() as usize).min(width);
        let line = line.as_chunks_mut::<4>().0;
        let alpha = 255. * opacity * vertical;
        line[first..end_pixel].fill([fill[0], fill[1], fill[2], alpha.round() as u8]);
        for x in [first, end_pixel - 1] {
            let coverage = (to.min(x as f32 + 1.) - from.max(x as f32)).clamp(0., 1.);
            line[x][3] = (alpha * coverage).round() as u8;
        }
        let node = ch * 0.12;
        let cap = ((position - start).abs().min((position - end).abs())).min(node);
        let node_width = (node * node - cap * cap).sqrt();
        let line_width = if height as f32 > ch && (start..=end).contains(&position) {
            ch * 0.05
        } else {
            0.
        };
        let stroke = node_width.max(line_width);
        if stroke == 0. {
            continue;
        }
        let left_edge = first.max((rail - stroke - 1.).floor() as usize);
        let right_edge = end_pixel.min((rail + stroke + 1.).ceil() as usize);
        for (x, pixel) in line.iter_mut().enumerate().take(right_edge).skip(left_edge) {
            let distance = (x as f32 + 0.5 - rail).abs();
            let ink = (stroke - distance + 0.5).clamp(0., 1.);
            for (channel, (&base, &highlight)) in pixel.iter_mut().zip(fill.iter().zip(&accent)) {
                *channel = (f32::from(base) + ink * (f32::from(highlight) - f32::from(base)))
                    .round() as u8;
            }
        }
    }
    pixels
}
