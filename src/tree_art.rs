//! Kitty graphics that settle after a single entrance in normal scrollback.
use crate::{kitty, palette::Theme};
use std::io::{self, Write};

const SWEEP_COLUMNS: usize = 24;
const BRANCH_COLUMNS: usize = 2;

pub struct Art {
    icons: [Option<kitty::Sprite>; 3],
    branches: [Option<kitty::Sprite>; 2],
    sweep: Option<kitty::Sprite>,
    cell: (usize, usize),
    playback: kitty::Playback,
}

#[derive(Clone, Copy)]
pub struct Available {
    pub columns: usize,
    pub cell: (usize, usize),
}

impl Available {
    pub fn stdout() -> Option<Self> {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
        // All placements go to stdout. TTYs can report cells without pixels.
        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
            return None;
        }
        let size = unsafe { size.assume_init() };
        if size.ws_col == 0 || size.ws_row == 0 {
            return None;
        }
        let width = usize::from(size.ws_xpixel / size.ws_col);
        let height = usize::from(size.ws_ypixel / size.ws_row);
        let cell = if (4..=64).contains(&width) && (8..=128).contains(&height) {
            (width, height)
        } else {
            (12, 24)
        };
        Some(Self {
            columns: usize::from(size.ws_col),
            cell,
        })
    }
}

impl Art {
    pub fn new(cell: (usize, usize), animated: bool) -> Self {
        Self {
            icons: Default::default(),
            branches: Default::default(),
            sweep: None,
            cell,
            playback: if animated {
                kitty::Playback::Once
            } else {
                kitty::Playback::Still
            },
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

    pub fn icon(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        kind: usize,
    ) -> io::Result<()> {
        let slot = &mut self.icons[kind];
        if slot.is_none() {
            *slot = Some(kitty::Sprite::upload(
                out,
                (24, 24),
                self.playback,
                |progress| kitty::icon_frame(kind, theme, progress),
            )?);
        }
        slot.as_ref()
            .expect("icon image prepared")
            .place(out, row, 2, 0, None)
    }

    pub fn label(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        columns: usize,
    ) -> io::Result<()> {
        let columns = columns.min(SWEEP_COLUMNS);
        if matches!(self.playback, kitty::Playback::Still) || columns == 0 {
            return Ok(());
        }
        let width = self.cell.0 * SWEEP_COLUMNS;
        let height = self.cell.1;
        if self.sweep.is_none() {
            self.sweep = Some(kitty::Sprite::upload(
                out,
                (width, height),
                self.playback,
                |progress| sweep_frame(width, height, theme, progress),
            )?);
        }
        // Keep real text in the cells. A translucent veil sweeps out above it.
        self.sweep.as_ref().expect("label image prepared").place(
            out,
            row,
            columns,
            1,
            Some(columns * self.cell.0),
        )
    }

    pub fn play(&self, out: &mut impl Write) -> io::Result<()> {
        for sprite in self
            .branches
            .iter()
            .chain(&self.icons)
            .flatten()
            .chain(&self.sweep)
        {
            sprite.play(out)?;
        }
        Ok(())
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

fn sweep_frame(width: usize, height: usize, theme: &Theme, progress: f32) -> Vec<u8> {
    let [r, g, b] = kitty::rgb(theme.color("surface"));
    let eased = 1. - (1. - progress).powi(2);
    let row: Vec<_> = (0..width)
        .flat_map(|x| {
            let position = x as f32 / width as f32;
            let edge = ((position - eased) * 7. + 0.88).clamp(0., 1.);
            [r, g, b, (118. * (1. - eased) * edge).round() as u8]
        })
        .collect();
    row.repeat(height)
}
