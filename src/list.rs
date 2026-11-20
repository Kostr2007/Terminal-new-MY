//! Connected file sections with live text and Kitty surfaces in scrollback.
use crate::{
    file_art::{Icon, Viewport},
    files::{self, Entry},
    kitty::{self, Playback, Sprite},
    palette::Theme,
    render,
    surface_art::{self, Surface},
    ui::Density,
};
use crossterm::{
    cursor::{MoveToColumn, MoveUp, RestorePosition, SavePosition},
    queue,
};
use std::{
    collections::HashMap,
    fmt::Display,
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthStr;

const MAX_NATIVE_ROWS: usize = 36;

pub struct Design<'a> {
    theme: &'a Theme,
    density: Density,
    long: bool,
    columns: usize,
    placement: usize,
    wrote_path: bool,
    graphics: Option<(Viewport, Playback)>,
    surfaces: HashMap<Surface, Sprite>,
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
                .map(|size| (size, Playback::spring(animated))),
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
        let limit = self.graphics.map_or(0, |(size, _)| {
            size.rows.saturating_sub(1).min(MAX_NATIVE_ROWS)
        });
        // Only a contiguous suffix of complete screen rows can share an image.
        let band = items
            .chunks(slots)
            .rev()
            .take(limit)
            .take_while(|row| {
                if slots > 1 {
                    return true;
                }
                let item = &row[0];
                let details = if self.long {
                    metadata(item.entry).width() + 2
                } else {
                    0
                };
                item.width + 4 + details <= self.columns
            })
            .count();
        let native_header = band == rows && band < limit;
        let first_native = rows - band;
        let heading = render::truncate(&format!("  {title}  {}", items.len()), self.columns);
        self.text(out, &heading, accent, fill, native_header)?;
        self.finish_row(out, heading.width(), fill, native_header)?;
        for (index, row) in items.chunks(slots).enumerate() {
            let details = self.long.then(|| metadata(row[0].entry));
            let placed = index >= first_native;
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
        self.place(
            out,
            Surface {
                fill,
                accent,
                rows: band + usize::from(native_header),
            },
        )
    }

    fn place(&mut self, out: &mut impl Write, surface: Surface) -> io::Result<()> {
        let Some((_, playback)) = self.graphics.filter(|_| surface.rows != 0) else {
            return Ok(());
        };
        let columns = self.columns + 1;
        out.flush()?;
        if let std::collections::hash_map::Entry::Vacant(slot) = self.surfaces.entry(surface) {
            slot.insert(surface.upload(out, self.theme, columns, surface_art::CELL, playback)?);
        }
        self.placement += 1;
        queue!(
            out,
            SavePosition,
            MoveUp(surface.rows as u16),
            MoveToColumn(0)
        )?;
        self.surfaces[&surface].place_in(out, self.placement, (columns, surface.rows), -1, None)?;
        queue!(out, RestorePosition)
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
