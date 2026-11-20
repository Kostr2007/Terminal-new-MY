use crate::{
    files::{self, Sort},
    kitty,
    palette::Theme,
    render,
};
use clap::Args;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

#[derive(Args)]
pub struct Options {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(short, long)]
    all: bool,
    /// Number of levels below the root.
    #[arg(short = 'L', long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=16))]
    depth: u8,
    /// Display the complete tree immediately.
    #[arg(long)]
    no_motion: bool,
}

const WAVE_SIZE: usize = 4;
const WAVES: usize = 3;
const TEXT_ROWS: usize = 6;
const REVEAL: Duration = Duration::from_millis(120);
const FRAME: Duration = Duration::from_millis(20);
const TRAVEL: usize = 4;

struct Row {
    prefix: String,
    branch: &'static str,
    label: String,
    suffix: String,
    role: &'static str,
    kind: usize,
    number: usize,
}

impl Row {
    fn width(&self) -> usize {
        self.prefix.width() + self.branch.width() + 2 + self.label.width() + self.suffix.width()
    }
}

fn stdout_size() -> Option<(usize, usize)> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::uninit();
    // The animation is printed to stdout, which has already been checked for a TTY.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return None;
    }
    let size = unsafe { size.assume_init() };
    Some((usize::from(size.ws_col), usize::from(size.ws_row)))
}

fn faded(
    out: &mut impl Write,
    theme: &Theme,
    text: &str,
    role: &str,
    progress: f32,
) -> io::Result<()> {
    use ratatui::style::Color::Rgb;
    let (Rgb(r, g, b), Rgb(br, bg, bb)) =
        (theme.accessible(role, "surface"), theme.color("surface"))
    else {
        unreachable!("Material roles are RGB");
    };
    let mix =
        |fg: u8, bg: u8| (f32::from(bg) + (f32::from(fg) - f32::from(bg)) * progress).round() as u8;
    write!(
        out,
        "\x1b[38;2;{};{};{}m{text}\x1b[0m",
        mix(r, br),
        mix(g, bg),
        mix(b, bb)
    )
}

fn reveal(
    out: &mut impl Write,
    theme: &Theme,
    row: &Row,
    size: (usize, usize),
) -> io::Result<bool> {
    if row.width() + TRAVEL >= size.0 || stdout_size() != Some(size) {
        return Ok(false);
    }
    // Paint only the current line, without scrolling or touching finished rows.
    let start = Instant::now();
    let mut frame = Vec::new();
    let mut painted_width = 0usize;
    for index in 0.. {
        let target = (FRAME * index).min(REVEAL);
        if let Some(wait) = (start + target).checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let elapsed = start.elapsed();
        let current_size = stdout_size();
        let finished = current_size != Some(size) || elapsed >= REVEAL;
        frame.clear();
        frame.write_all(b"\x1b[?2026h\r")?;
        if let Some((width, _)) = current_size.filter(|&(width, _)| width > 0 && width < size.0) {
            // Kitty reflows the preview when a window shrinks. Recover its
            // first row before replacing it with the final, naturally wrapped text.
            let wrapped = painted_width.saturating_sub(1) / width;
            if wrapped != 0 {
                write!(frame, "\x1b[{wrapped}A\x1b[J")?;
            }
        }
        frame.write_all(b"\x1b[2K")?;
        if !finished {
            let progress = elapsed.as_secs_f32() / REVEAL.as_secs_f32();
            // Quadratic easing keeps the displacement visible through the first frames.
            let eased = 1. - (1. - progress).powi(2);
            let shift = (TRAVEL as f32 * (1. - eased)).round() as usize;
            painted_width = row.width() + shift;
            write!(frame, "{:shift$}", "")?;
            faded(&mut frame, theme, &row.prefix, "outline", eased)?;
            faded(&mut frame, theme, row.branch, "outline", eased)?;
            frame.write_all(b"  ")?;
            faded(&mut frame, theme, &row.label, row.role, eased)?;
            faded(&mut frame, theme, &row.suffix, "on_surface", eased)?;
            frame.write_all(b"\x1b[?2026l")?;
        }
        out.write_all(&frame)?;
        if finished {
            // Keep this update open through the final text and Kitty placement.
            return Ok(true);
        }
        out.flush()?;
    }
    unreachable!()
}

struct TreeGraphics {
    icons: [[Option<kitty::TreeIcon>; 3]; WAVES + 1],
    animated: bool,
}

impl TreeGraphics {
    fn place(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        kind: usize,
    ) -> io::Result<()> {
        let wave = if self.animated {
            ((row - 1) / WAVE_SIZE).min(WAVES)
        } else {
            WAVES
        };
        let slot = &mut self.icons[wave][kind];
        if slot.is_none() {
            *slot = Some(kitty::TreeIcon::upload(out, kind, theme, wave < WAVES)?);
        }
        slot.as_mut().expect("icon was uploaded").place(out, row)
    }
}

struct Tree<'a, W> {
    out: W,
    theme: &'a Theme,
    styled: bool,
    graphics: Option<TreeGraphics>,
    motion_size: Option<(usize, usize)>,
    count: usize,
    failed: bool,
}

impl<W: Write> Tree<'_, W> {
    fn write_row(&mut self, row: Row) -> io::Result<()> {
        let synchronized = match self.motion_size.filter(|_| row.number <= TEXT_ROWS) {
            Some(size) => reveal(&mut self.out, self.theme, &row, size)?,
            None => false,
        };
        let result = (|| {
            write!(
                self.out,
                "{}{}",
                row.prefix,
                self.theme.paint(row.branch, "outline", None, self.styled)
            )?;
            if let Some(graphics) = self.graphics.as_mut().filter(|_| row.number <= 160) {
                graphics.place(&mut self.out, self.theme, row.number, row.kind)?;
            }
            writeln!(
                self.out,
                "{}{}",
                self.theme.paint(&row.label, row.role, None, self.styled),
                row.suffix
            )
        })();
        let finish = if synchronized {
            self.out
                .write_all(b"\x1b[?2026l")
                .and_then(|()| self.out.flush())
        } else {
            Ok(())
        };
        result.and(finish)
    }

    fn branch(&mut self, path: &Path, prefix: &str, all: bool, depth: u8) -> io::Result<()> {
        let entries = match files::read(path, all, Sort::Name) {
            Ok(entries) => entries,
            Err(error) => {
                self.failed = true;
                return writeln!(
                    self.out,
                    "{prefix}└─ [{}]",
                    render::safe(&error.to_string())
                );
            }
        };
        self.entries(entries, prefix, all, depth)
    }

    fn entries(
        &mut self,
        entries: Vec<files::Entry>,
        prefix: &str,
        all: bool,
        depth: u8,
    ) -> io::Result<()> {
        let len = entries.len();
        for (index, item) in entries.into_iter().enumerate() {
            if self.count == 1000 {
                break;
            }
            self.count += 1;
            let last = index + 1 == len;
            let kind = match item.kind {
                "dir" => 0,
                "file" => 1,
                _ => 2,
            };
            let suffix = match item.kind {
                "dir" => "/",
                "link" => "@",
                _ => "",
            };
            let target = item
                .link_target
                .as_ref()
                .map(|p| format!(" → {}", render::safe(&p.to_string_lossy())))
                .unwrap_or_default();
            let row = Row {
                prefix: prefix.into(),
                branch: if last { "└─ " } else { "├─ " },
                label: render::safe(&item.name),
                suffix: format!("{suffix}{target}"),
                kind,
                role: render::role(&item),
                number: self.count,
            };
            self.write_row(row)?;
            if item.kind == "dir" && depth > 1 {
                self.branch(
                    &item.path,
                    &format!("{prefix}{}", if last { "   " } else { "│  " }),
                    all,
                    depth - 1,
                )?;
            }
        }
        Ok(())
    }
}

pub fn run(options: Options, theme: &Theme, styled: bool, graphics: bool) -> io::Result<u8> {
    let root = std::fs::canonicalize(&options.path)?;
    if !root.is_dir() {
        return Err(io::Error::other("tree expects a directory"));
    }
    let entries = files::read(&root, options.all, Sort::Name)?;
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{}",
        theme.paint(
            &format!(" {} ", render::safe(&options.path.to_string_lossy())),
            "on_primary_container",
            Some("primary_container"),
            styled
        )
    )?;
    let animated = graphics && kitty::motion_enabled() && !options.no_motion;
    let motion_size = (animated && styled)
        .then(stdout_size)
        .flatten()
        .filter(|&(width, height)| width >= 24 && height >= 4);
    let mut tree = Tree {
        out,
        theme,
        styled,
        graphics: graphics.then(|| TreeGraphics {
            icons: Default::default(),
            animated,
        }),
        motion_size,
        count: 0,
        failed: false,
    };
    tree.entries(entries, "", options.all, options.depth)?;
    if tree.count == 1000 {
        writeln!(tree.out, "… display limit reached (1000 entries)")?;
    }
    Ok(u8::from(tree.failed))
}
