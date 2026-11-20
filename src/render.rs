use crate::{
    files::{self, Entry},
    palette::Theme,
};
use std::io::{self, Write};
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

pub fn plain_listing(
    out: &mut impl Write,
    entries: &[Entry],
    long: bool,
    zero: bool,
) -> io::Result<()> {
    for entry in entries {
        if zero {
            out.write_all(entry.path.as_os_str().as_encoded_bytes())?;
            out.write_all(&[0])?;
        } else {
            let name = safe(&entry.name);
            if long {
                writeln!(
                    out,
                    "{:7} {:04o} {:>10}  {name}",
                    entry.kind,
                    entry.permissions,
                    files::size(entry.size)
                )?;
            } else {
                writeln!(out, "{name}")?;
            }
        }
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
