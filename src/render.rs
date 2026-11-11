use crate::{
    files::{self, Entry},
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

pub fn listing(
    out: &mut impl Write,
    path: &Path,
    entries: &[Entry],
    long: bool,
    zero: bool,
    styled: bool,
    theme: &Theme,
) -> io::Result<()> {
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
    let width = crossterm::terminal::size()
        .map(|s| usize::from(s.0))
        .unwrap_or(80);
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
    for entry in entries {
        let name = safe(&entry.name);
        if styled {
            let badge = format!(" {:7} ", entry.kind);
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
