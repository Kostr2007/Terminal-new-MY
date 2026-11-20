//! Shared file badges and text reveal for Kitty scrollback.
use crate::{
    files::Entry,
    kitty::{self, rounded},
    palette::Theme,
};
use std::io::{self, Write};

const SWEEP_COLUMNS: usize = 24;

#[derive(Clone, Copy)]
pub enum Icon {
    Folder,
    File,
    Link,
    Special,
}

impl Icon {
    pub fn for_entry(entry: &Entry) -> Self {
        match entry.kind {
            "dir" => Self::Folder,
            "file" => Self::File,
            "link" => Self::Link,
            _ => Self::Special,
        }
    }

    pub fn upload(
        self,
        out: &mut impl Write,
        theme: &Theme,
        playback: kitty::Playback,
    ) -> io::Result<kitty::Sprite> {
        kitty::Sprite::upload(out, (24, 24), playback, |phase| {
            icon_frame(self, theme, phase)
        })
    }
}

pub struct Art {
    icons: [Option<kitty::Sprite>; 4],
    sweep: Option<kitty::Sprite>,
    cell: (usize, usize),
    playback: kitty::Playback,
}

impl Art {
    pub fn new(cell: (usize, usize), playback: kitty::Playback) -> Self {
        Self {
            icons: Default::default(),
            sweep: None,
            cell,
            playback,
        }
    }

    pub fn icon(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        kind: Icon,
    ) -> io::Result<()> {
        let slot = &mut self.icons[kind as usize];
        if slot.is_none() {
            *slot = Some(kind.upload(out, theme, self.playback)?);
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
        for sprite in self.icons.iter().flatten().chain(&self.sweep) {
            sprite.play(out)?;
        }
        Ok(())
    }
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

fn icon_frame(kind: Icon, theme: &Theme, progress: f32) -> Vec<u8> {
    let (foreground, background) = match kind {
        Icon::Folder => ("on_primary_container", "primary_container"),
        Icon::File => ("on_secondary_container", "secondary_container"),
        Icon::Link | Icon::Special => ("tertiary", "surface_container_high"),
    };
    let fg = kitty::rgb(theme.accessible(foreground, background));
    let bg = kitty::rgb(theme.color(background));
    // The cubic ease-out comes from Material-You-Terminal's card studio.
    let eased = 1. - (1. - progress).powi(3);
    let scale = 0.50 + 0.50 * eased;
    let mut pixels = kitty::raster(24, 24, |x, y| {
        let x = 12. + (x - 12.) / scale;
        let y = 12. + (y - 12.) / scale;
        let mark = match kind {
            Icon::Folder => {
                rounded(x, y, [5., 9., 19., 18.], 2.) || rounded(x, y, [5., 6., 12., 13.], 2.)
            }
            Icon::File => {
                rounded(x, y, [8., 5., 16., 19.], 2.)
                    && !((10. ..14.).contains(&x)
                        && ((10. ..12.).contains(&y) || (14. ..16.).contains(&y)))
            }
            Icon::Link => {
                let u = 12. + (x - y) * std::f32::consts::FRAC_1_SQRT_2;
                let v = 12. + (x + y - 24.) * std::f32::consts::FRAC_1_SQRT_2;
                (rounded(u, v, [4., 9., 12., 15.], 3.) && !rounded(u, v, [6., 11., 10., 13.], 1.))
                    || (rounded(u, v, [12., 9., 20., 15.], 3.)
                        && !rounded(u, v, [14., 11., 18., 13.], 1.))
                    || ((9. ..15.).contains(&u) && (11. ..13.).contains(&v))
            }
            Icon::Special => (x - 12.).abs() + (y - 12.).abs() < 6.,
        };
        if mark {
            Some(fg)
        } else if rounded(x, y, [1., 1., 23., 23.], 8.) {
            Some(bg)
        } else {
            None
        }
    });
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = (f32::from(pixel[3]) * (0.10 + 0.90 * eased)).round() as u8;
    }
    pixels
}

#[derive(Clone, Copy)]
pub struct Viewport {
    pub columns: usize,
    pub cell: (usize, usize),
}

impl Viewport {
    pub fn stdout() -> Option<Self> {
        Self::read(libc::STDOUT_FILENO)
    }

    pub fn stderr() -> Option<Self> {
        Self::read(libc::STDERR_FILENO)
    }

    fn read(fd: libc::c_int) -> Option<Self> {
        let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
        // TTYs can report cells without pixel sizes.
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
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
