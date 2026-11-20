use base64::{Engine, engine::general_purpose::STANDARD};
use clap::ValueEnum;
use flate2::{Compression, write::ZlibEncoder};
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

pub fn rgb(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => unreachable!("Material color is RGB"),
    }
}

pub fn rounded(x: f32, y: f32, rect: [f32; 4], radius: f32) -> bool {
    let [left, top, right, bottom] = rect;
    let dx = x - x.clamp(left + radius, right - radius);
    let dy = y - y.clamp(top + radius, bottom - radius);
    dx * dx + dy * dy <= radius * radius
}

pub fn raster(
    width: usize,
    height: usize,
    sample: impl Fn(f32, f32) -> Option<[u8; 3]>,
) -> Vec<u8> {
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

#[derive(Clone, Copy)]
pub enum Playback {
    Still,
    Once,
    Loop,
}

impl Playback {
    pub fn entrance(animated: bool) -> Self {
        if animated && motion_enabled() {
            Self::Once
        } else {
            Self::Still
        }
    }
}

/// An image that can be placed many times. Animation starts after placement.
pub struct Sprite {
    id: u32,
    size: (usize, usize),
    playback: Playback,
}

impl Sprite {
    pub fn png(out: &mut impl Write, size: (usize, usize), data: &[u8]) -> io::Result<Self> {
        let id = number()?;
        transmit(out, &format!("a=t,I={id},f=100"), data)?;
        Ok(Self {
            id,
            size,
            playback: Playback::Still,
        })
    }

    pub fn upload(
        out: &mut impl Write,
        size: (usize, usize),
        playback: Playback,
        frame: impl Fn(f32) -> Vec<u8>,
    ) -> io::Result<Self> {
        let id = number()?;
        let sprite = Self { id, size, playback };
        let (frames, divisions, gap): (u8, u8, u8) = match playback {
            Playback::Still => {
                sprite.send(out, "a=t", &frame(1.))?;
                return Ok(sprite);
            }
            Playback::Once => (8, 7, 36),
            Playback::Loop => (24, 24, 55),
        };
        sprite.send(out, "a=t", &frame(0.))?;
        write!(out, "\x1b_Ga=a,I={id},r=1,z={gap},q=2\x1b\\")?;
        let action = format!("a=f,X=1,z={gap}");
        for step in 1..frames {
            sprite.send(out, &action, &frame(f32::from(step) / f32::from(divisions)))?;
        }
        Ok(sprite)
    }

    fn send(&self, out: &mut impl Write, action: &str, pixels: &[u8]) -> io::Result<()> {
        let (width, height) = self.size;
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(pixels)?;
        transmit(
            out,
            &format!("{action},I={},f=32,s={width},v={height},o=z", self.id),
            &compressed.finish()?,
        )
    }

    pub fn replace(&self, out: &mut impl Write, pixels: &[u8]) -> io::Result<()> {
        self.send(out, "a=f,r=1,X=1", pixels)
    }

    pub fn delete(&self, out: &mut impl Write) -> io::Result<()> {
        write!(out, "\x1b_Ga=d,d=N,I={},q=2\x1b\\", self.id)
    }

    pub fn place(
        &self,
        out: &mut impl Write,
        placement: usize,
        columns: usize,
        layer: i8,
        crop_width: Option<usize>,
    ) -> io::Result<()> {
        self.place_in(out, placement, (columns, 1), layer, crop_width)
    }

    pub fn place_in(
        &self,
        out: &mut impl Write,
        placement: usize,
        (columns, rows): (usize, usize),
        layer: i8,
        crop_width: Option<usize>,
    ) -> io::Result<()> {
        write!(
            out,
            "\x1b_Ga=p,I={},p={placement},c={columns},r={rows},z={layer},C=1,q=2",
            self.id
        )?;
        if let Some(width) = crop_width {
            write!(out, ",w={width}")?;
        }
        out.write_all(b"\x1b\\")
    }

    pub fn play(&self, out: &mut impl Write) -> io::Result<()> {
        let state = match self.playback {
            Playback::Still => return Ok(()),
            // Kitty's loading mode holds the last frame.
            Playback::Once => "s=2,c=1",
            Playback::Loop => "s=3,v=1,c=1",
        };
        write!(out, "\x1b_Ga=a,I={},{state},q=2\x1b\\", self.id)
    }
}
