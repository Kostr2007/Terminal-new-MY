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
}

struct Tree<'a, W> {
    out: W,
    theme: &'a Theme,
    styled: bool,
    ids: Option<[u32; 3]>,
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
            let last = index + 1 == len;
            let branch = if last { "└─ " } else { "├─ " };
            write!(
                self.out,
                "{prefix}{}",
                self.theme.paint(branch, "outline", None, self.styled)
            )?;
            if let Some(ids) = self.ids.filter(|_| self.count <= 160) {
                let kind = match item.kind {
                    "dir" => 0,
                    "file" => 1,
                    _ => 2,
                };
                kitty::place_icon(&mut self.out, ids[kind], self.count)?;
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
    let ids = if graphics {
        let ids = [kitty::number()?, kitty::number()?, kitty::number()?];
        for (kind, id) in ids.into_iter().enumerate() {
            kitty::upload_icon(&mut out, id, kind, theme)?;
        }
        Some(ids)
    } else {
        None
    };
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
        ids,
        count: 0,
        failed: false,
    };
    tree.entries(entries, "", options.all, options.depth)?;
    if tree.count == 1000 {
        writeln!(tree.out, "… display limit reached (1000 entries)")?;
    }
    Ok(u8::from(tree.failed))
}
