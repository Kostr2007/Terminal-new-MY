//! Connected file sections with live text and Kitty surfaces in scrollback.
use crate::{
    file_art::{Icon, Viewport},
    files::{self, Entry},
    kitty::{self, Playback, Sprite},
    palette::Theme,
    render,
    surface_art::{Edge, Row},
    ui::Density,
};
use std::{
    collections::HashMap,
    fmt::Display,
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthStr;

const MAX_NATIVE_ROWS: usize = 256;

pub struct Design<'a> {
    theme: &'a Theme,
    density: Density,
    long: bool,
    columns: usize,
    placement: usize,
    wrote_path: bool,
    graphics: Option<(Viewport, Playback)>,
    surfaces: HashMap<Row, Sprite>,
}

struct Item<'a> {
    entry: &'a Entry,
    label: String,
    git: &'a str,
    width: usize,
}

impl<'a> Item<'a> {
    fn new(entry: &'a Entry) -> Self {
        let mut label = render::safe(&entry.name);
        if let Some(target) = &entry.link_target {
            label.push_str(" → ");
            label.push_str(&render::safe(&target.to_string_lossy()));
        }
        let git = entry
            .git_status
            .as_deref()
            .filter(|status| !status.trim().is_empty())
            .unwrap_or("");
        let width = label.width() + if git.is_empty() { 0 } else { 2 + git.width() };
        Self {
            entry,
            label,
            git,
            width,
        }
    }
}

impl<'a> Design<'a> {
    pub fn new(
        theme: &'a Theme,
        density: Density,
        long: bool,
        native: bool,
        animated: bool,
    ) -> Self {
        let viewport = Viewport::stdout();
        let columns = viewport.map_or(80, |size| size.columns);
        Self {
            theme,
            density,
            long,
            columns: columns.saturating_sub(1).min(112),
            placement: 0,
            wrote_path: false,
            graphics: viewport
                .filter(|size| native && size.columns >= 15)
                .map(|size| (size, Playback::entrance(animated))),
            surfaces: HashMap::new(),
        }
    }

    pub fn play(&self, out: &mut impl Write) -> io::Result<()> {
        for sprite in self.surfaces.values() {
            sprite.play(out)?;
        }
        Ok(())
    }

    pub fn write(
        &mut self,
        out: &mut impl Write,
        path: &Path,
        entries: &[Entry],
    ) -> io::Result<()> {
        if self.columns < 14 {
            return render::plain_listing(out, entries, self.long, false);
        }
        if self.wrote_path {
            writeln!(out)?;
        }
        self.wrote_path = true;
        let path = render::safe(&path.to_string_lossy());
        writeln!(
            out,
            "  {}",
            self.theme.paint(
                &format!(" {} ", render::truncate(&path, self.columns - 4)),
                "on_primary_container",
                Some("primary_container"),
                true
            )
        )?;
        if entries.is_empty() {
            return writeln!(out, "  Empty");
        }
        let folders = entries.partition_point(|entry| entry.directory);
        for (section, title, accent, fill) in [
            (
                &entries[..folders],
                "Folders",
                "primary",
                "surface_container_high",
            ),
            (
                &entries[folders..],
                "Files",
                "tertiary",
                "surface_container",
            ),
        ] {
            if !section.is_empty() {
                self.section(out, section, (title, accent, fill))?;
            }
        }
        Ok(())
    }

    fn section(
        &mut self,
        out: &mut impl Write,
        section: &[Entry],
        (title, accent, fill): (&str, &'static str, &'static str),
    ) -> io::Result<()> {
        let items: Vec<_> = section.iter().map(Item::new).collect();
        let gutter = if matches!(self.density, Density::Compact) {
            2
        } else {
            4
        };
        let max_item = items.iter().map(|item| item.width + 2).max().unwrap_or(0);
        let slots = if self.long {
            1
        } else {
            ((self.columns - 3 + gutter) / (max_item.max(16) + gutter))
                .max(1)
                .min(items.len())
        };
        let slot_width = (self.columns - 3 + gutter) / slots - gutter;
        let rows = items.len().div_ceil(slots);
        let first_native = rows.saturating_sub(MAX_NATIVE_ROWS);
        let header = Row {
            fill,
            accent,
            edge: Edge::Top,
        };
        let heading = render::truncate(&format!("  {title}  {}", items.len()), self.columns);
        let placed = rows <= MAX_NATIVE_ROWS && self.place(out, header)?;
        self.text(out, &heading, accent, fill, placed)?;
        self.finish_row(out, heading.width(), fill, placed)?;
        for (index, row) in items.chunks(slots).enumerate() {
            let edge = if index + 1 == rows {
                Edge::Bottom
            } else {
                Edge::Middle
            };
            let width = row.iter().map(|item| item.width + 2).max().unwrap_or(0);
            let details = self.long.then(|| metadata(row[0].entry));
            let used = 2 + width + details.as_deref().map_or(0, |s| s.width() + 2);
            // Let exceptionally long names wrap as terminal text.
            let fits = slots > 1 || used <= self.columns;
            let placed =
                index >= first_native && fits && self.place(out, Row { edge, ..header })?;
            self.text(out, "  ", accent, fill, placed)?;
            for (col, item) in row.iter().enumerate() {
                if col != 0 {
                    self.text(out, format!("{:gutter$}", ""), accent, fill, placed)?;
                }
                self.text(
                    out,
                    Icon::for_entry(item.entry).glyph(),
                    render::role(item.entry),
                    fill,
                    placed,
                )?;
                self.text(out, " ", accent, fill, placed)?;
                self.text(out, &item.label, render::role(item.entry), fill, placed)?;
                if !item.git.is_empty() {
                    self.text(out, format!("  {}", item.git), "tertiary", fill, placed)?;
                }
                if col + 1 < row.len() {
                    let padding = slot_width - item.width - 2;
                    self.text(out, format!("{:padding$}", ""), accent, fill, placed)?;
                }
            }
            let mut written = 2
                + (row.len() - 1) * (slot_width + gutter)
                + row.last().expect("nonempty row").width
                + 2;
            if let Some(details) = details {
                let pad = self
                    .columns
                    .saturating_sub(written + details.width() + 1)
                    .max(2);
                self.text(
                    out,
                    format!("{:pad$}{details}", ""),
                    "on_surface_variant",
                    fill,
                    placed,
                )?;
                written += pad + details.width();
            }
            self.finish_row(out, written, fill, placed)?;
        }
        Ok(())
    }

    fn place(&mut self, out: &mut impl Write, row: Row) -> io::Result<bool> {
        let Some((size, playback)) = self.graphics else {
            return Ok(false);
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = self.surfaces.entry(row) {
            slot.insert(row.upload(out, self.theme, size.cell, self.columns, playback)?);
        }
        self.placement += 1;
        self.surfaces[&row].place(out, self.placement, self.columns, -1, None)?;
        Ok(true)
    }

    fn text(
        &self,
        out: &mut impl Write,
        text: impl Display,
        role: &str,
        surface: &str,
        placed: bool,
    ) -> io::Result<()> {
        let [r, g, b] = kitty::rgb(self.theme.accessible(role, surface));
        if placed {
            write!(out, "\x1b[38;2;{r};{g};{b}m{text}\x1b[0m")
        } else {
            let [br, bg, bb] = kitty::rgb(self.theme.color(surface));
            write!(
                out,
                "\x1b[38;2;{r};{g};{b};48;2;{br};{bg};{bb}m{text}\x1b[0m"
            )
        }
    }

    fn finish_row(
        &self,
        out: &mut impl Write,
        written: usize,
        fill: &str,
        placed: bool,
    ) -> io::Result<()> {
        if !placed && written < self.columns {
            let pad = self.columns - written;
            self.text(out, format!("{:pad$}", ""), "on_surface", fill, false)?;
        }
        writeln!(out)
    }
}

fn metadata(entry: &Entry) -> String {
    let size = if entry.kind == "dir" {
        "—".into()
    } else {
        files::size(entry.size)
    };
    format!("{size:>11}  {:04o} ", entry.permissions)
}
