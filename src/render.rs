use crate::{
    file_art::{self, Icon, Viewport},
    files::{self, Entry},
    kitty,
    palette::Theme,
};
use std::{
    io::{self, Write},
    path::Path,
};
use unicode_width::UnicodeWidthStr;

/// Escape terminal controls, bidi formatting and invisible separators in untrusted text.
pub fn safe(text: &str) -> String {
    text.chars().flat_map(|c| {
        if c.is_control() || matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}') {
            c.escape_default().collect::<Vec<_>>()
        } else { vec![c] }
    }).collect()
}

pub fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for c in text.chars() {
        let mut next = out.clone();
        next.push(c);
        if next.width() > width - 1 {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

pub fn role(entry: &Entry) -> &'static str {
    match entry.kind {
        "dir" => "primary",
        "link" => "tertiary",
        _ => "on_surface",
    }
}

pub struct ListOptions {
    pub long: bool,
    pub zero: bool,
    pub styled: bool,
    pub density: crate::ui::Density,
}

pub struct ListGraphics {
    art: file_art::Art,
    columns: usize,
    row: usize,
}

impl ListGraphics {
    pub fn stdout(long: bool, animated: bool) -> Option<Self> {
        let size = Viewport::stdout()?;
        if size.columns < if long { 32 } else { 14 } {
            return None;
        }
        Some(Self {
            art: file_art::Art::new(size.cell, kitty::Playback::entrance(animated)),
            columns: size.columns,
            row: 0,
        })
    }

    pub fn play(&self, out: &mut impl Write) -> io::Result<()> {
        self.art.play(out)
    }

    fn entry(
        &mut self,
        out: &mut impl Write,
        entry: &Entry,
        theme: &Theme,
        long: bool,
        git: bool,
        name: &str,
    ) -> io::Result<()> {
        self.row += 1;
        out.write_all(b"  ")?;
        self.art
            .icon(out, theme, self.row, Icon::for_entry(entry))?;
        out.write_all(b"   ")?;
        if git {
            let status = entry.git_status.as_deref().unwrap_or("  ");
            let field = format!(" {status:2} ");
            if status.trim().is_empty() {
                write!(out, "{field} ")?;
            } else {
                write!(
                    out,
                    "{} ",
                    theme.paint(
                        &field,
                        "on_secondary_container",
                        Some("secondary_container"),
                        true
                    )
                )?;
            }
        }
        let mut prefix = if git { 10 } else { 5 };
        if long {
            let size = if entry.kind == "dir" {
                "—".into()
            } else {
                files::size(entry.size)
            };
            let details = format!("{:04o}  {size:>10}  ", entry.permissions);
            prefix += details.width();
            write!(
                out,
                "{}",
                theme.paint(&details, "on_surface_variant", None, true)
            )?;
        }
        self.art.label(
            out,
            theme,
            self.row,
            name.width().min(self.columns.saturating_sub(prefix)),
        )?;
        let target = entry
            .link_target
            .as_ref()
            .map(|path| format!(" → {}", safe(&path.to_string_lossy())))
            .unwrap_or_default();
        writeln!(
            out,
            "{}{target}",
            theme.paint(name, role(entry), None, true)
        )
    }
}

pub fn listing(
    out: &mut impl Write,
    path: &Path,
    entries: &[Entry],
    options: ListOptions,
    theme: &Theme,
    mut graphics: Option<&mut ListGraphics>,
) -> io::Result<()> {
    let ListOptions {
        long,
        zero,
        styled,
        density,
    } = options;
    if zero {
        for entry in entries {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                out.write_all(entry.path.as_os_str().as_bytes())?;
            }
            #[cfg(not(unix))]
            out.write_all(entry.path.to_string_lossy().as_bytes())?;
            out.write_all(&[0])?;
        }
        return Ok(());
    }
    let width = graphics.as_ref().map_or_else(
        || {
            crossterm::terminal::size()
                .map(|s| usize::from(s.0))
                .unwrap_or(80)
        },
        |art| art.columns,
    );
    if styled {
        writeln!(
            out,
            "\n{}",
            theme.paint(
                &format!(
                    "  {}  ",
                    truncate(&safe(&path.to_string_lossy()), width.saturating_sub(4))
                ),
                "on_primary_container",
                Some("primary_container"),
                true
            )
        )?;
        writeln!(
            out,
            "{}",
            theme.paint(
                &format!("  {} items", entries.len()),
                "on_surface_variant",
                None,
                true
            )
        )?;
        writeln!(out)?;
    }
    let show_git = graphics.is_some()
        && entries.iter().any(|entry| {
            entry
                .git_status
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
        });
    for entry in entries {
        let name = safe(&entry.name);
        if let Some(graphics) = &mut graphics {
            graphics.entry(out, entry, theme, long, show_git, &name)?;
        } else if styled {
            let badge = format!(
                " {:5} {} ",
                entry.kind,
                entry.git_status.as_deref().unwrap_or("  ")
            );
            let meta = if long {
                format!(
                    " {:04o} {:>10}  ",
                    entry.permissions,
                    if entry.kind == "dir" {
                        "—".into()
                    } else {
                        files::size(entry.size)
                    }
                )
            } else {
                String::new()
            };
            let suffix = entry
                .link_target
                .as_ref()
                .map(|p| format!(" → {}", safe(&p.to_string_lossy())))
                .unwrap_or_default();
            let available = width.saturating_sub(2 + badge.width() + meta.width() + 2);
            let label = truncate(&format!("{name}{suffix}"), available);
            writeln!(
                out,
                "  {} {}{}",
                theme.paint(
                    &badge,
                    "on_secondary_container",
                    Some("secondary_container"),
                    true
                ),
                theme.paint(&meta, "on_surface_variant", None, true),
                theme.paint(&label, role(entry), None, true)
            )?;
        } else if long {
            writeln!(
                out,
                "{:7} {:04o} {:>10}  {}",
                entry.kind,
                entry.permissions,
                files::size(entry.size),
                name
            )?;
        } else {
            writeln!(out, "{name}")?;
        }
        if styled && matches!(density, crate::ui::Density::Comfortable) {
            writeln!(out)?;
        }
    }
    if styled {
        writeln!(out)?;
    }
    Ok(())
}

pub fn doctor(report: &serde_json::Value, styled: bool, theme: &Theme) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{}",
        theme.paint(
            " Terminal New MY · diagnostics ",
            "on_primary_container",
            Some("primary_container"),
            styled
        )
    )?;
    for (name, value) in report.as_object().unwrap() {
        writeln!(out, "  {name:18} {}", safe(&value.to_string()))?;
    }
    Ok(())
}

pub fn theme(theme: &Theme, styled: bool) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "Terminal New MY · {}", theme.mode)?;
    for (name, color) in &theme.colors {
        writeln!(
            out,
            "{} {name:28} {color}",
            theme.paint("  ", "on_surface", Some(name), styled)
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_cannot_inject_terminal_sequences() {
        let text = safe("name\x1b[2J\n\u{202e}txt");
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\n'));
        assert!(!text.contains('\u{202e}'));
        assert!(text.contains("name"));
    }
    #[test]
    fn unicode_truncation_counts_cells() {
        assert_eq!(truncate("日本語", 5), "日本…");
        assert_eq!(truncate("привет", 4), "при…");
        assert_eq!(truncate("hello", 0), "");
    }
}
