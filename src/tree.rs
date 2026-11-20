use crate::{
    file_art::{Icon, Viewport},
    palette::Theme,
    render,
    tree_art::Art,
};
use clap::Args;
use std::{
    fs::{self, FileType},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
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
    /// Display the complete tree immediately with static graphics.
    #[arg(long)]
    no_motion: bool,
}

const ENTRY_LIMIT: usize = 1000;

struct Item {
    entry: fs::DirEntry,
    kind: FileType,
}

fn directory(path: &Path, all: bool) -> io::Result<Vec<Item>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if all || entry.file_name().as_encoded_bytes().first() != Some(&b'.') {
            let kind = entry.file_type()?;
            entries.push(Item { entry, kind });
        }
    }
    // Trees need names and types. DirEntry can get the latter from the directory
    // record on common filesystems, without reading size, times and permissions.
    entries.sort_by_cached_key(|item| {
        let name = item.entry.file_name();
        let directory =
            item.kind.is_dir() || (item.kind.is_symlink() && item.entry.path().is_dir());
        (!directory, name.to_string_lossy().to_lowercase(), name)
    });
    Ok(entries)
}

struct Tree<'a, W> {
    out: W,
    theme: &'a Theme,
    styled: bool,
    graphics: Option<(Art, usize)>,
    count: usize,
    truncated: bool,
    failed: bool,
}

impl<W: Write> Tree<'_, W> {
    fn branch(&mut self, path: &Path, prefix: &str, all: bool, depth: u8) -> io::Result<()> {
        match directory(path, all) {
            Ok(entries) => self.entries(entries, prefix, all, depth),
            Err(error) => {
                self.failed = true;
                writeln!(
                    self.out,
                    "{prefix}└─ [{}]",
                    render::safe(&error.to_string())
                )
            }
        }
    }

    fn entries(
        &mut self,
        entries: Vec<Item>,
        prefix: &str,
        all: bool,
        depth: u8,
    ) -> io::Result<()> {
        let len = entries.len();
        for (index, item) in entries.into_iter().enumerate() {
            if self.count == ENTRY_LIMIT {
                self.truncated = true;
                break;
            }
            self.count += 1;
            let last = index + 1 == len;
            let name = render::safe(&item.entry.file_name().to_string_lossy());
            let (kind, suffix, role) = if item.kind.is_dir() {
                (Icon::Folder, "/", "primary")
            } else if item.kind.is_symlink() {
                (Icon::Link, "@", "tertiary")
            } else if item.kind.is_file() {
                (Icon::File, "", "on_surface")
            } else {
                (Icon::Special, "", "on_surface")
            };
            let target = item
                .kind
                .is_symlink()
                .then(|| fs::read_link(item.entry.path()).ok())
                .flatten();
            let target = target
                .map(|path| format!(" → {}", render::safe(&path.to_string_lossy())))
                .unwrap_or_default();
            let branch = if last { "└─ " } else { "├─ " };
            write!(
                self.out,
                "{}",
                self.theme.paint(prefix, "outline", None, self.styled)
            )?;
            // Place artwork only when both connector and badge fit on this row.
            if let Some((art, columns)) = &mut self.graphics
                && prefix.width() + 5 < *columns
            {
                art.branch(&mut self.out, self.theme, self.count, last)?;
                self.out.write_all(b"   ")?;
                art.files
                    .icon(&mut self.out, self.theme, self.count, kind)?;
                self.out.write_all(b"  ")?;
                art.files.label(
                    &mut self.out,
                    self.theme,
                    self.count,
                    name.width().min(*columns - prefix.width() - 5),
                )?;
            } else {
                write!(
                    self.out,
                    "{}",
                    self.theme.paint(branch, "outline", None, self.styled)
                )?;
            }
            writeln!(
                self.out,
                "{}{suffix}{target}",
                self.theme.paint(&name, role, None, self.styled)
            )?;
            if item.kind.is_dir() && depth > 1 {
                let indentation = if last { "   " } else { "│  " };
                self.branch(
                    &item.entry.path(),
                    &format!("{prefix}{indentation}"),
                    all,
                    depth - 1,
                )?;
                if self.truncated {
                    break;
                }
            }
        }
        Ok(())
    }
}

pub fn run(options: Options, theme: &Theme, styled: bool, graphics: bool) -> io::Result<u8> {
    let root = fs::canonicalize(&options.path)?;
    if !root.is_dir() {
        return Err(io::Error::other("tree expects a directory"));
    }
    let entries = directory(&root, options.all)?;
    let mut out = BufWriter::new(io::stdout().lock());
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
    out.flush()?;
    let graphics = graphics
        .then(Viewport::stdout)
        .flatten()
        .map(|size| (Art::new(size.cell, !options.no_motion), size.columns));
    let mut tree = Tree {
        out,
        theme,
        styled,
        graphics,
        count: 0,
        truncated: false,
        failed: false,
    };
    tree.entries(entries, "", options.all, options.depth)?;
    if tree.truncated {
        writeln!(tree.out, "… display limit reached ({ENTRY_LIMIT} entries)")?;
    }
    if let Some((art, _)) = tree.graphics {
        art.play(&mut tree.out)?;
    }
    tree.out.flush()?;
    Ok(u8::from(tree.failed))
}
