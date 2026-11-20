use crate::palette::Theme;
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::ValueEnum;
use ratatui::style::Color;
use std::{
    env,
    fs::File,
    io::{self, Read, Write},
};

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum Graphics {
    #[default]
    Auto,
    Kitty,
    Never,
}

impl Graphics {
    pub fn enabled(self, tty: bool) -> bool {
        tty && match self {
            Self::Never => false,
            Self::Kitty => true,
            Self::Auto => {
                env::var_os("KITTY_WINDOW_ID").is_some()
                    && env::var("TERM").is_ok_and(|term| term == "xterm-kitty")
                    && env::var_os("TMUX").is_none()
                    && env::var_os("STY").is_none()
                    && env::var_os("NO_COLOR").is_none()
            }
        }
    }
}

pub fn number() -> io::Result<u32> {
    let mut bytes = [0; 4];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u32::from_ne_bytes(bytes).max(1))
}

fn transmit(out: &mut impl Write, parameters: &str, pixels: &[u8]) -> io::Result<()> {
    let data = STANDARD.encode(pixels);
    let chunks = data.as_bytes().chunks(4096);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        let keys = if index == 0 { parameters } else { "" };
        let separator = if keys.is_empty() { "" } else { "," };
        write!(
            out,
            "\x1b_G{keys}{separator}q=2,m={};",
            u8::from(index + 1 < count)
        )?;
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

fn rgb(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => unreachable!("Material color is RGB"),
    }
}

fn rounded(x: f32, y: f32, rect: [f32; 4], radius: f32) -> bool {
    let [left, top, right, bottom] = rect;
    let dx = x - x.clamp(left + radius, right - radius);
    let dy = y - y.clamp(top + radius, bottom - radius);
    dx * dx + dy * dy <= radius * radius
}

fn raster(width: usize, height: usize, sample: impl Fn(f32, f32) -> Option<[u8; 3]>) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let mut channels = [0u16; 3];
            let mut covered = 0;
            for dy in [0.25, 0.75] {
                for dx in [0.25, 0.75] {
                    if let Some(color) = sample(x as f32 + dx, y as f32 + dy) {
                        covered += 1;
                        for (sum, component) in channels.iter_mut().zip(color) {
                            *sum += u16::from(component);
                        }
                    }
                }
            }
            pixels.extend(channels.map(|sum| (sum / covered.max(1)) as u8));
            pixels.push((covered * 255 / 4) as u8);
        }
    }
    pixels
}

pub fn motion_enabled() -> bool {
    env::var_os("TNMY_REDUCED_MOTION").is_none() && env::var_os("MYT_NO_ANIMATION").is_none()
}

fn icon_frame(kind: usize, theme: &Theme, progress: f32) -> Vec<u8> {
    let (foreground, background) = match kind {
        0 => ("on_primary_container", "primary_container"),
        1 => ("on_secondary_container", "secondary_container"),
        _ => ("tertiary", "surface_container_high"),
    };
    let fg = rgb(theme.accessible(foreground, background));
    let bg = rgb(theme.color(background));
    // The cubic ease-out also drives the row reveal in Material-You-Terminal's
    // card studio. The endpoint uses the exact geometry of the static icon.
    let eased = 1. - (1. - progress).powi(3);
    let scale = 0.50 + 0.50 * eased;
    let mut pixels = raster(24, 24, |x, y| {
        let x = 12. + (x - 12.) / scale;
        let y = 12. + (y - 12.) / scale;
        let mark = match kind {
            0 => rounded(x, y, [5., 9., 19., 18.], 2.) || rounded(x, y, [5., 6., 12., 13.], 2.),
            1 => {
                rounded(x, y, [8., 5., 16., 19.], 2.)
                    && !((10. ..14.).contains(&x)
                        && ((10. ..12.).contains(&y) || (14. ..16.).contains(&y)))
            }
            _ => (x - 12.).abs() + (y - 12.).abs() < 6.,
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

pub struct TreeIcon {
    id: u32,
    animated: bool,
    started: bool,
}

impl TreeIcon {
    pub fn upload(
        out: &mut impl Write,
        kind: usize,
        theme: &Theme,
        animated: bool,
    ) -> io::Result<Self> {
        const STEPS: u8 = 7;
        const GAP_MS: u8 = 40;
        let id = number()?;
        let progress = if animated { 0. } else { 1. };
        transmit(
            out,
            &format!("a=t,I={id},f=32,s=24,v=24"),
            &icon_frame(kind, theme, progress),
        )?;
        if animated {
            write!(out, "\x1b_Ga=a,I={id},r=1,z={GAP_MS},q=2\x1b\\")?;
            for step in 1..=STEPS {
                transmit(
                    out,
                    &format!("a=f,I={id},f=32,s=24,v=24,X=1,z={GAP_MS}"),
                    &icon_frame(kind, theme, f32::from(step) / f32::from(STEPS)),
                )?;
            }
        }
        Ok(Self {
            id,
            animated,
            started: false,
        })
    }

    pub fn place(&mut self, out: &mut impl Write, placement: usize) -> io::Result<()> {
        let id = self.id;
        write!(
            out,
            "\x1b_Ga=p,I={id},p={placement},c=2,r=1,C=1,q=2\x1b\\  "
        )?;
        if self.animated && !self.started {
            // Loading mode holds the last frame while the image lives in scrollback.
            write!(out, "\x1b_Ga=a,I={id},s=2,c=1,q=2\x1b\\")?;
            self.started = true;
        }
        Ok(())
    }
}

pub struct Loader {
    id: u32,
    animated: bool,
}

impl Loader {
    pub fn start(theme: &Theme, styled: bool) -> io::Result<Self> {
        let id = number()?;
        let animated = motion_enabled();
        let loader = Self { id, animated };
        let mut out = io::stderr().lock();
        let width = crate::ui::terminal_area().map_or(32, |area| area.width);
        let fg = rgb(theme.color(if styled { "primary" } else { "on_surface" }));
        let bg = rgb(theme.color("surface_container_high"));
        let frame = |phase: usize| {
            let step = phase.min(12 - phase) as f32;
            let start = 3. + 10. * step;
            raster(96, 16, |x, y| {
                if rounded(x, y, [start, 4., start + 28., 12.], 4.) {
                    Some(fg)
                } else if rounded(x, y, [1., 3., 95., 13.], 5.) {
                    Some(bg)
                } else {
                    None
                }
            })
        };
        if width >= 14 {
            transmit(
                &mut out,
                &format!("a=T,I={id},f=32,s=96,v=16,c=12,r=1,C=1"),
                &frame(0),
            )?;
            if animated {
                write!(out, "\x1b_Ga=a,I={id},r=1,z=75,q=2\x1b\\")?;
                for phase in 1..12 {
                    transmit(
                        &mut out,
                        &format!("a=f,I={id},f=32,s=96,v=16,X=1,z=75"),
                        &frame(phase),
                    )?;
                }
                write!(out, "\x1b_Ga=a,I={id},s=3,v=1,c=1,q=2\x1b\\")?;
            }
            writeln!(out)?;
        }
        if animated {
            write!(out, "\x1b]9;4;3;0\x1b\\")?;
        }
        out.flush()?;
        Ok(loader)
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut out = io::stderr().lock();
        let _ = write!(out, "\x1b_Ga=d,d=N,I={},q=2\x1b\\", self.id);
        if self.animated {
            let _ = write!(out, "\x1b]9;4;0;0\x1b\\");
        }
        let _ = out.flush();
    }
}
