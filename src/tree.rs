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
const STAGGER: Duration = Duration::from_millis(18);

struct TreeGraphics {
    icons: [[Option<kitty::TreeIcon>; 3]; WAVES + 1],
    start: Option<Instant>,
}

impl TreeGraphics {
    fn place(
        &mut self,
        out: &mut impl Write,
        theme: &Theme,
        row: usize,
        kind: usize,
    ) -> io::Result<()> {
        let wave = if self.start.is_some() {
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

    fn pace(&self, out: &mut impl Write, row: usize) -> io::Result<()> {
        if let Some(start) = self.start.filter(|_| row <= WAVES * WAVE_SIZE) {
            out.flush()?;
            // Time from the first row, so a slow filesystem never adds delay.
            if let Some(delay) =
                (start + STAGGER * row as u32).checked_duration_since(Instant::now())
            {
                std::thread::sleep(delay);
            }
        }
        Ok(())
    }
}

struct Tree<'a, W> {
    out: W,
    theme: &'a Theme,
    styled: bool,
    graphics: Option<TreeGraphics>,
    count: usize,
    failed: bool,
}

impl<W: Write> Tree<'_, W> {
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
            if let Some(graphics) = &self.graphics {
                graphics.pace(&mut self.out, self.count)?;
            }
            let last = index + 1 == len;
            let branch = if last { "└─ " } else { "├─ " };
            write!(
                self.out,
                "{prefix}{}",
                self.theme.paint(branch, "outline", None, self.styled)
            )?;
            if let Some(graphics) = self.graphics.as_mut().filter(|_| self.count <= 160) {
                let kind = match item.kind {
                    "dir" => 0,
                    "file" => 1,
                    _ => 2,
                };
                graphics.place(&mut self.out, self.theme, self.count, kind)?;
            }
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
            writeln!(
                self.out,
                "{}{suffix}{target}",
                self.theme.paint(
                    &render::safe(&item.name),
                    render::role(&item),
                    None,
                    self.styled
                )
            )?;
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
    let mut tree = Tree {
        out,
        theme,
        styled,
        graphics: graphics.then(|| TreeGraphics {
            icons: Default::default(),
            start: (kitty::motion_enabled() && !options.no_motion).then(Instant::now),
        }),
        count: 0,
        failed: false,
    };
    tree.entries(entries, "", options.all, options.depth)?;
    if tree.count == 1000 {
        writeln!(tree.out, "… display limit reached (1000 entries)")?;
    }
    Ok(u8::from(tree.failed))
}
