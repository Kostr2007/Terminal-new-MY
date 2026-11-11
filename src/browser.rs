use crate::{
    files::{self, Entry, Sort},
    palette::Theme,
    render,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    fs::OpenOptions,
    io::{self, Read},
    path::{Path, PathBuf},
};

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn style(theme: &Theme, fg: &str, bg: &str, color: bool) -> Style {
    if color {
        Style::default().fg(theme.color(fg)).bg(theme.color(bg))
    } else {
        Style::default()
    }
}

fn panel(title: String, theme: &Theme, color: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(style(theme, "on_surface", "surface_container", color))
        .border_style(style(theme, "outline", "surface_container", color))
}

fn preview(entry: Option<&Entry>) -> String {
    let Some(entry) = entry else {
        return "No matching files".into();
    };
    let mut text = format!(
        "{}\n\n{} · {}\nPermissions: {:04o}\n",
        render::safe(&entry.name),
        entry.kind,
        files::size(entry.size),
        entry.permissions
    );
    if let Some(target) = &entry.link_target {
        text.push_str(&format!(
            "\nLink → {}\n",
            render::safe(&target.to_string_lossy())
        ));
    }
    // Do not follow symlinks or read devices/FIFOs while generating previews.
    if entry.kind != "file" {
        return text;
    }
    let result = (|| -> io::Result<Vec<u8>> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&entry.path)?;
        if !file.metadata()?.is_file() {
            return Ok(Vec::new());
        }
        let mut bytes = Vec::new();
        file.take(64 * 1024).read_to_end(&mut bytes)?;
        Ok(bytes)
    })();
    match result {
        Ok(bytes) if bytes.contains(&0) => {
            text.push_str("\nBinary file · text preview unavailable")
        }
        Ok(bytes) => {
            text.push('\n');
            for line in String::from_utf8_lossy(&bytes).lines().take(200) {
                text.push_str(&render::safe(line));
                text.push('\n');
            }
            if entry.size > 64 * 1024 {
                text.push_str("\n[preview limited to 64 KiB]");
            }
        }
        Err(e) => text.push_str(&format!("\nPreview: {}", render::safe(&e.to_string()))),
    }
    text
}

pub fn run(path: PathBuf, mut all: bool, mut theme: Theme, color: bool) -> io::Result<()> {
    let mut path = std::fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(io::Error::other("files expects a directory"));
    }
    let mut sort = Sort::Name;
    let mut entries = files::read(&path, all, sort)?;
    enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
        previous_hook(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut query = String::new();
    let mut searching = false;
    let mut state = ListState::default();
    state.select(Some(0));
    let mut message = String::new();
    let mut cached_path = None;
    let mut cached_preview = preview(None);
    loop {
        let query_lowercase = query.to_lowercase();
        let filtered: Vec<&Entry> = entries
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&query_lowercase))
            .collect();
        if filtered.is_empty() {
            state.select(None);
        } else {
            state.select(Some(state.selected().unwrap_or(0).min(filtered.len() - 1)));
        }
        let selected = state.selected().and_then(|i| filtered.get(i)).copied();
        let selected_path = selected.map(|e| e.path.clone());
        if cached_path != selected_path {
            cached_preview = preview(selected);
            cached_path = selected_path;
        }
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                Block::default().style(style(&theme, "on_surface", "surface", color)),
                area,
            );
            let rows = Layout::default()
                .margin(1)
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(3),
                ])
                .split(area);
            let header = Paragraph::new(vec![
                Line::from(Span::styled(
                    " Terminal New MY  /  files ",
                    style(&theme, "on_primary_container", "primary_container", color)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!(" {}", render::safe(&path.to_string_lossy()))),
                Line::from(format!(
                    " {} entries · sort: {sort:?} · hidden: {}",
                    filtered.len(),
                    if all { "shown" } else { "hidden" }
                )),
            ]);
            frame.render_widget(header, rows[0]);
            let columns = if area.width >= 90 {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
                    .split(rows[1])
            } else {
                Layout::default()
                    .constraints([Constraint::Percentage(100)])
                    .split(rows[1])
            };
            let items: Vec<ListItem> = filtered
                .iter()
                .map(|e| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!(" {:7} ", e.kind),
                            style(&theme, "on_surface_variant", "surface_container", color),
                        ),
                        Span::styled(
                            render::safe(&e.name),
                            style(&theme, render::role(e), "surface_container", color),
                        ),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(panel(" Contents ".into(), &theme, color))
                .highlight_style(
                    style(
                        &theme,
                        "on_secondary_container",
                        "secondary_container",
                        color,
                    )
                    .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› ");
            frame.render_stateful_widget(list, columns[0], &mut state);
            if columns.len() > 1 {
                frame.render_widget(
                    Paragraph::new(cached_preview.as_str())
                        .wrap(Wrap { trim: false })
                        .block(panel(" Preview ".into(), &theme, color)),
                    columns[1],
                );
            }
            let status = if searching {
                format!(" Find: {query}  · Enter done · Esc clear")
            } else if !message.is_empty() {
                message.clone()
            } else {
                " ↑↓ move · Enter open · ← parent · / find · a hidden · s sort · r refresh · q exit"
                    .into()
            };
            frame.render_widget(
                Paragraph::new(status)
                    .wrap(Wrap { trim: false })
                    .block(panel(
                        format!(" {} matches ", filtered.len()),
                        &theme,
                        color,
                    )),
                rows[2],
            );
        })?;
        let input = event::read()?;
        if matches!(input, Event::Resize(_, _)) {
            if let Ok(next) = Theme::load(theme.source.as_deref()) {
                theme = next;
            }
            continue;
        }
        let Event::Key(key) = input else {
            continue;
        };
        if !key.is_press() {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(());
        }
        if searching {
            match key.code {
                KeyCode::Esc => {
                    searching = false;
                    query.clear();
                }
                KeyCode::Enter => searching = false,
                KeyCode::Backspace => {
                    query.pop();
                    state.select(Some(0));
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    state.select(Some(0));
                }
                _ => (),
            }
            continue;
        }
        let mut destination: Option<PathBuf> = None;
        let mut reload = false;
        message.clear();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Down | KeyCode::Char('j') => {
                if !filtered.is_empty() {
                    state.select(Some(
                        (state.selected().unwrap_or(0) + 1).min(filtered.len() - 1),
                    ));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.select(Some(state.selected().unwrap_or(0).saturating_sub(1)))
            }
            KeyCode::Home => state.select(Some(0)),
            KeyCode::End => state.select(filtered.len().checked_sub(1)),
            KeyCode::Char('/') => searching = true,
            KeyCode::Char('a') => {
                all = !all;
                reload = true;
            }
            KeyCode::Char('s') => {
                sort = match sort {
                    Sort::Name => Sort::Size,
                    Sort::Size => Sort::Modified,
                    Sort::Modified => Sort::Name,
                };
                reload = true;
            }
            KeyCode::Char('r') => reload = true,
            KeyCode::Enter | KeyCode::Right => {
                if let Some(entry) = selected.filter(|e| e.directory) {
                    destination = Some(entry.path.clone());
                }
            }
            KeyCode::Left | KeyCode::Backspace => {
                destination = path.parent().map(Path::to_path_buf)
            }
            _ => (),
        }
        if let Some(destination) = destination {
            match std::fs::canonicalize(&destination)
                .and_then(|p| files::read(&p, all, sort).map(|e| (p, e)))
            {
                Ok((p, e)) => {
                    path = p;
                    entries = e;
                    query.clear();
                    state.select(Some(0));
                    cached_path = None;
                }
                Err(e) => message = render::safe(&e.to_string()),
            }
        } else if reload {
            match Theme::load(theme.source.as_deref()) {
                Ok(next) => theme = next,
                Err(e) => message = render::safe(&e.to_string()),
            }
            match files::read(&path, all, sort) {
                Ok(e) => {
                    entries = e;
                    state.select(Some(0));
                    cached_path = None;
                }
                Err(e) => message = render::safe(&e.to_string()),
            }
        }
    }
}
