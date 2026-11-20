//! Kitty graphics that settle after a single entrance in normal scrollback.
use crate::{file_art, kitty, palette::Theme};
use std::io::{self, Write};

const BRANCH_COLUMNS: usize = 2;

pub struct Art {
    pub files: file_art::Art,
    branches: [Option<kitty::Sprite>; 2],
    cell: (usize, usize),
    playback: kitty::Playback,
}

impl Art {
    pub fn new(cell: (usize, usize), animated: bool) -> Self {
        let playback = kitty::Playback::entrance(animated);
        Self {
            files: file_art::Art::new(cell, playback),
            branches: Default::default(),
            cell,
            playback,
        }
    }

    pub fn branch(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        last: bool,
    ) -> io::Result<()> {
        let slot = &mut self.branches[usize::from(last)];
        if slot.is_none() {
            let size = (self.cell.0 * BRANCH_COLUMNS, self.cell.1);
            *slot = Some(kitty::Sprite::upload(
                out,
                size,
                self.playback,
                |progress| branch_frame(self.cell, theme, last, progress),
            )?);
        }
        slot.as_ref()
            .expect("branch image prepared")
            .place(out, row, BRANCH_COLUMNS, 0, None)
    }

    pub fn play(&self, out: &mut impl Write) -> io::Result<()> {
        for sprite in self.branches.iter().flatten() {
            sprite.play(out)?;
        }
        self.files.play(out)
    }
}

fn branch_frame((cw, ch): (usize, usize), theme: &Theme, last: bool, progress: f32) -> Vec<u8> {
    let width = cw as f32;
    let height = ch as f32;
    let trunk = width / 2.;
    let mid = height / 2.;
    let bend = (width * 0.62).min(mid * 0.82);
    let elbow = (trunk + bend, mid - bend);
    let stroke = (height * 0.036).max(0.6);
    let end = width * 1.6;
    let radius = (width * 0.25).min(mid * 0.5);
    let ink = kitty::rgb(theme.color("outline"));
    let node = kitty::rgb(theme.color("primary"));
    let eased = 1. - (1. - progress).powi(2);
    let tip = trunk + (end - trunk) * (0.25 + 0.75 * eased);
    let dot_radius = radius * (0.45 + 0.55 * eased);
    kitty::raster(cw * BRANCH_COLUMNS, ch, |x, y| {
        if (x - tip).hypot(y - mid) <= dot_radius {
            return Some(node);
        }
        let vertical = (x - trunk).abs() <= stroke && (!last || y <= elbow.1);
        let curve = x >= trunk - stroke
            && x <= elbow.0
            && y >= elbow.1
            && ((x - elbow.0).hypot(y - elbow.1) - bend).abs() <= stroke;
        let horizontal = x >= elbow.0 && (y - mid).abs() <= stroke;
        (vertical || (x <= tip && (curve || horizontal))).then_some(ink)
    })
}
