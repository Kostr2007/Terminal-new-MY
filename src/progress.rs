use crate::{
    kitty::{self, Playback, Sprite},
    palette::Theme,
    progress_art as art, ui,
};
use clap::Args;
use std::{
    ffi::OsString,
    io::{self, BufRead, IsTerminal, Read, Write},
    num::NonZeroU64,
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Args)]
pub struct Options {
    /// Read completed units, one count per line, from stdin
    #[arg(long)]
    total: Option<NonZeroU64>,
    /// Run a command with an activity indicator; pass it after --
    #[arg(
        last = true,
        required_unless_present = "total",
        conflicts_with = "total"
    )]
    command: Vec<OsString>,
}

pub fn run(options: Options, theme: &Theme, styled: bool, graphics: bool) -> io::Result<u8> {
    if let Some(total) = options.total {
        return measure(io::stdin().lock(), total.get(), theme, styled, graphics);
    }
    let (program, args) = options
        .command
        .split_first()
        .expect("command checked by clap");
    let _activity = Activity::start(theme, styled, graphics)?;
    if !graphics && interactive() {
        eprintln!("  Running command…");
    }
    wait(Command::new(program).args(args))
}

fn interactive() -> bool {
    io::stderr().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

/// A child shares the terminal and receives its own Ctrl+C. Catch SIGINT in the
/// parent so it can remove the indicator and return the child's exit status.
pub fn wait(command: &mut Command) -> io::Result<u8> {
    let registration = signal_hook::flag::register(
        signal_hook::consts::SIGINT,
        Arc::new(AtomicBool::new(false)),
    )?;
    let result = command.spawn().and_then(|mut child| child.wait());
    signal_hook::low_level::unregister(registration);
    let status = result?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)) as u8)
    }
    #[cfg(not(unix))]
    {
        Ok(status.code().unwrap_or(1) as u8)
    }
}

pub struct Activity {
    sprite: Option<Sprite>,
    motion: bool,
}

impl Activity {
    pub fn start(theme: &Theme, styled: bool, graphics: bool) -> io::Result<Option<Self>> {
        if !graphics {
            return Ok(None);
        }
        let mut activity = Self {
            sprite: None,
            motion: kitty::motion_enabled(),
        };
        let columns = columns().saturating_sub(1).min(art::MAX_COLUMNS);
        let mut out = io::stderr().lock();
        if columns >= 6 {
            let width = art::width(columns);
            let playback = if activity.motion {
                Playback::Loop
            } else {
                Playback::Still
            };
            let sprite = Sprite::upload(&mut out, (width, art::HEIGHT), playback, |phase| {
                art::busy(width, theme, styled, phase)
            })?;
            activity.sprite = Some(sprite);
            let sprite = activity.sprite.as_ref().expect("activity image prepared");
            sprite.place(&mut out, 1, columns, 0, None)?;
            sprite.play(&mut out)?;
            writeln!(out)?;
        }
        if activity.motion {
            write!(out, "\x1b]9;4;3;0\x1b\\")?;
        }
        out.flush()?;
        Ok(Some(activity))
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        let mut out = io::stderr().lock();
        if let Some(sprite) = &self.sprite {
            let _ = sprite.delete(&mut out);
        }
        if self.motion {
            let _ = write!(out, "\x1b]9;4;0;0\x1b\\");
        }
        let _ = out.flush();
    }
}

fn columns() -> usize {
    ui::terminal_area().map_or(0, |area| usize::from(area.width))
}

fn measure(
    mut input: impl BufRead,
    total: u64,
    theme: &Theme,
    styled: bool,
    graphics: bool,
) -> io::Result<u8> {
    let mut meter = interactive()
        .then(|| Meter::new(theme, styled, graphics))
        .transpose()?;
    let mut count = 0;
    let mut input_line = Vec::new();
    while input.by_ref().take(64).read_until(b'\n', &mut input_line)? != 0 {
        let sample = std::str::from_utf8(&input_line)
            .ok()
            .filter(|_| input_line.len() < 64)
            .and_then(|line| line.trim().parse::<u64>().ok())
            .filter(|value| *value <= total);
        count = sample.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("progress counts must be integers from 0 to {total}, one per line"),
            )
        })?;
        input_line.clear();
        if let Some(meter) = &mut meter {
            meter.show(percent(count, total))?;
        }
    }
    if meter.is_none() {
        eprintln!("{}% · {count}/{total}", percent(count, total));
    }
    Ok(u8::from(count != total))
}

fn percent(count: u64, total: u64) -> u8 {
    (u128::from(count) * 100 / u128::from(total)) as u8
}

struct Meter<'a> {
    theme: &'a Theme,
    styled: bool,
    sprite: Option<(Sprite, usize)>,
    last_percent: Option<u8>,
}

impl<'a> Meter<'a> {
    fn new(theme: &'a Theme, styled: bool, graphics: bool) -> io::Result<Self> {
        let mut meter = Self {
            theme,
            styled,
            sprite: None,
            last_percent: None,
        };
        let columns = columns().saturating_sub(6).min(art::MAX_COLUMNS);
        if graphics && columns >= 6 {
            let width = art::width(columns);
            let mut out = io::stderr().lock();
            let sprite = Sprite::upload(&mut out, (width, art::HEIGHT), Playback::Still, |_| {
                art::measured(width, theme, styled, 0)
            })?;
            write!(out, "\r")?;
            sprite.place(&mut out, 1, columns, 0, None)?;
            meter.sprite = Some((sprite, columns));
        }
        meter.show(0)?;
        Ok(meter)
    }

    fn show(&mut self, percent: u8) -> io::Result<()> {
        if self.last_percent == Some(percent) {
            return Ok(());
        }
        let width = columns();
        let mut out = io::stderr().lock();
        write!(out, "\r")?;
        if let Some((sprite, columns)) = &self.sprite
            && width < columns + 6
        {
            sprite.delete(&mut out)?;
            self.sprite = None;
        }
        if let Some((sprite, columns)) = &self.sprite {
            if self.last_percent.is_some() {
                sprite.replace(
                    &mut out,
                    &art::measured(art::width(*columns), self.theme, self.styled, percent),
                )?;
            }
            write!(out, "\x1b[{columns}C ")?;
        } else {
            let track = width.saturating_sub(6).min(art::MAX_COLUMNS);
            if track >= 4 {
                let done = track * usize::from(percent) / 100;
                write!(
                    out,
                    "{}{} ",
                    self.theme
                        .paint(&"━".repeat(done), "primary", None, self.styled),
                    self.theme
                        .paint(&"─".repeat(track - done), "outline", None, self.styled),
                )?;
            }
        }
        write!(out, "{percent:>3}%\x1b[K")?;
        self.last_percent = Some(percent);
        out.flush()
    }
}

impl Drop for Meter<'_> {
    fn drop(&mut self) {
        if self.last_percent.is_some() {
            let _ = writeln!(io::stderr().lock());
        }
    }
}
