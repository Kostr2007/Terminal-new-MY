use crate::{
    files::{self, Entry, Sort},
    git,
    palette::Theme,
    preview, render,
    ui::{self, Density},
    worker::Worker,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{io, path::PathBuf, time::Duration};

struct Restore;
fn unavailable_size_probe(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || matches!(error.raw_os_error(), Some(libc::ENOTTY | libc::ENOENT))
}
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Pick {
    None,
    Path,
    Directory,
}
pub struct Options {
    pub path: PathBuf,
    pub all: bool,
    pub density: Density,
    pub git: bool,
    pub pick: Pick,
}
struct Load {
    path: PathBuf,
    all: bool,
    sort: Sort,
}
type Directory = io::Result<(PathBuf, Vec<Entry>)>;
struct App {
    path: PathBuf,
    entries: Vec<Entry>,
    search: Vec<String>,
    labels: Vec<String>,
    filtered: Vec<usize>,
    selected: usize,
    offset: usize,
    query: String,
    searching: bool,
    all: bool,
    sort: Sort,
    density: Density,
    loading: bool,
    preview_pending: bool,
    git_pending: bool,
    preview: String,
    preview_scroll: u16,
    preview_max: u16,
    preview_focus: bool,
    git: git::Snapshot,
    message: String,
    pick: Pick,
}
impl App {
    fn entry(&self) -> Option<&Entry> {
        self.filtered
            .get(self.selected)
            .and_then(|i| self.entries.get(*i))
    }
    fn filter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self
            .search
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.contains(&q).then_some(i))
            .collect();
        self.selected = 0;
        self.offset = 0;
    }
    fn update_preview(&mut self, worker: &Worker<Entry, String>) {
        self.preview_scroll = 0;
        if let Some(entry) = self.entry() {
            worker.submit(entry.clone());
            self.preview = "Loading preview…".into();
            self.preview_pending = true;
        } else {
            worker.cancel();
            self.preview = "No matching files".into();
            self.preview_pending = false;
        }
    }
    fn draw(&mut self, frame: &mut Frame, theme: &Theme, color: bool) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(ui::style(theme, "on_surface", "surface", color)),
            area,
        );
        let rows = Layout::default()
            .margin(self.density.margin())
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(area);
        let status = if self.loading {
            "Loading folder…".into()
        } else {
            let git = if self.git_pending {
                "Git: loading…".into()
            } else if !self.git.branch.is_empty() {
                format!("Git: {}", render::safe(&self.git.branch))
            } else if self.git.warning.is_some() {
                "Git: unavailable".into()
            } else {
                String::new()
            };
            format!(
                " {} entries · {:?} · {:?} · {git}",
                self.filtered.len(),
                self.sort,
                self.density
            )
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(ui::chip("Terminal New MY  /  files", theme, color)),
                Line::from(format!(" {}", render::safe(&self.path.to_string_lossy()))),
                Line::from(status),
            ]),
            rows[0],
        );
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
        let height = self.density.row_height();
        let visible = usize::from(columns[0].height.saturating_sub(2)) / height;
        let visible = visible.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .skip(self.offset)
            .take(visible)
            .map(|i| {
                let entry = &self.entries[*i];
                let git = self.git.label(&entry.path);
                let mut lines = vec![Line::from(vec![
                    Span::styled(
                        format!(" {git:2} "),
                        ui::style(theme, "tertiary", "surface_container", color),
                    ),
                    Span::styled(
                        format!("{:5} ", entry.kind),
                        ui::style(theme, "on_surface_variant", "surface_container", color),
                    ),
                    Span::styled(
                        self.labels[*i].clone(),
                        ui::style(theme, render::role(entry), "surface_container", color),
                    ),
                ])];
                if height == 2 {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "           {} · {:04o}",
                            if entry.directory {
                                "folder".into()
                            } else {
                                files::size(entry.size)
                            },
                            entry.permissions
                        ),
                        ui::style(theme, "on_surface_variant", "surface_container", color),
                    )));
                }
                ListItem::new(lines)
            })
            .collect();
        let mut state = ListState::default();
        if !self.filtered.is_empty() {
            state.select(Some(self.selected.saturating_sub(self.offset)));
        }
        frame.render_stateful_widget(
            List::new(items)
                .block(ui::surface(" Contents ", theme, color))
                .highlight_style(ui::selected(theme, color))
                .highlight_symbol("› "),
            columns[0],
            &mut state,
        );
        if columns.len() > 1 {
            self.preview_max = Paragraph::new(self.preview.as_str())
                .wrap(Wrap { trim: false })
                .line_count(columns[1].width.saturating_sub(2))
                .saturating_sub(1)
                .min(u16::MAX as usize) as u16;
            self.preview_scroll = self.preview_scroll.min(self.preview_max);
            let title = format!(
                " Preview{} · line {} ",
                if self.preview_focus { " [focus]" } else { "" },
                self.preview_scroll + 1
            );
            frame.render_widget(
                Paragraph::new(self.preview.as_str())
                    .wrap(Wrap { trim: false })
                    .scroll((self.preview_scroll, 0))
                    .block(ui::surface(title, theme, color)),
                columns[1],
            );
        }
        let help = if self.searching {
            format!(
                " Find: {} · Enter done · Esc clear",
                render::safe(&self.query)
            )
        } else if !self.message.is_empty() {
            self.message.clone()
        } else if self.preview_focus {
            " ↑↓ scroll preview · PgUp/Dn page · Tab files · q exit".into()
        } else if self.pick != Pick::None {
            " Enter open/select · c choose current folder · p choose entry · q cancel".into()
        } else {
            ui::actions().into()
        };
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .block(ui::surface(
                    format!(" {} matches ", self.filtered.len()),
                    theme,
                    color,
                )),
            rows[2],
        );
    }
}

pub fn run(options: Options, mut theme: Theme, color: bool) -> io::Result<Option<PathBuf>> {
    let load = Worker::new(
        |request: Load, cancel: &crate::worker::Cancel| -> Directory {
            let path = std::fs::canonicalize(request.path)?;
            if !path.is_dir() {
                return Err(io::Error::other("files expects a directory"));
            }
            let entries = files::read_cancel(&path, request.all, request.sort, cancel)?;
            Ok((path, entries))
        },
    );
    let preview = Worker::new(preview::load);
    let mut cache = git::Cache::default();
    let git = Worker::new(
        move |(path, force): (PathBuf, bool), cancel: &crate::worker::Cancel| {
            cache.scan(&path, force, cancel)
        },
    );
    let path = if options.path.is_absolute() {
        options.path
    } else {
        std::env::current_dir()?.join(options.path)
    };
    let mut app = App {
        path,
        entries: Vec::new(),
        search: Vec::new(),
        labels: Vec::new(),
        filtered: Vec::new(),
        selected: 0,
        offset: 0,
        query: String::new(),
        searching: false,
        all: options.all,
        sort: Sort::Name,
        density: options.density,
        loading: true,
        preview_pending: false,
        git_pending: false,
        preview: String::new(),
        preview_scroll: 0,
        preview_max: 0,
        preview_focus: false,
        git: git::Snapshot::default(),
        message: String::new(),
        pick: options.pick,
    };
    load.submit(Load {
        path: app.path.clone(),
        all: app.all,
        sort: app.sort,
    });
    enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stderr(), EnterAlternateScreen)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen, crossterm::cursor::Show);
        previous_hook(info);
    }));
    let mut area = ui::terminal_area()?;
    let mut terminal = Terminal::with_options(
        ui::StderrBackend::new(),
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Fixed(area),
        },
    )?;
    let mut dirty = true;
    let mut refresh_git = false;
    loop {
        let current_area = ui::terminal_area()?;
        if current_area != area {
            area = current_area;
            terminal.resize(area)?;
            dirty = true;
            if let Ok(next) = Theme::load(theme.source.as_deref()) {
                theme = next;
            }
        }
        if let Some(result) = load.result() {
            app.loading = false;
            match result {
                Ok((path, entries)) => {
                    app.path = path;
                    app.entries = entries;
                    app.search = app.entries.iter().map(|e| e.name.to_lowercase()).collect();
                    app.labels = app.entries.iter().map(|e| render::safe(&e.name)).collect();
                    app.filter();
                    app.update_preview(&preview);
                    app.message.clear();
                    app.git = git::Snapshot::default();
                    if options.git {
                        git.submit((app.path.clone(), refresh_git));
                        app.git_pending = true;
                        refresh_git = false;
                    }
                }
                Err(e) => {
                    if app.entries.is_empty() {
                        return Err(e);
                    }
                    app.message = render::safe(&e.to_string());
                }
            }
            dirty = true;
        }
        if let Some(result) = preview.result() {
            app.preview = result;
            app.preview_pending = false;
            dirty = true;
        }
        if let Some(result) = git.result() {
            app.git = result;
            app.git_pending = false;
            dirty = true;
        }
        if dirty {
            terminal.draw(|frame| app.draw(frame, &theme, color))?;
            dirty = false;
        }
        let timeout = if app.loading || app.preview_pending || app.git_pending {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(250)
        };
        let ready = match event::poll(timeout) {
            Ok(ready) => ready,
            // Crossterm's SIGWINCH path probes stdout, then /dev/tty/tput.
            // A picker may have none of these; stderr was already resized above.
            Err(e) if unavailable_size_probe(&e) => continue,
            Err(e) => return Err(e),
        };
        if !ready {
            continue;
        }
        let input = match event::read() {
            Ok(input) => input,
            Err(e) if unavailable_size_probe(&e) => continue,
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("reading terminal input: {e}"),
                ));
            }
        };
        if matches!(input, Event::Resize(_, _)) {
            if let Ok(next) = Theme::load(theme.source.as_deref()) {
                theme = next;
            }
            dirty = true;
            continue;
        }
        let Event::Key(key) = input else {
            continue;
        };
        if !key.is_press() {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(None);
        }
        dirty = true;
        if app.searching {
            match key.code {
                KeyCode::Esc => {
                    app.searching = false;
                    app.query.clear();
                }
                KeyCode::Enter => app.searching = false,
                KeyCode::Backspace => {
                    app.query.pop();
                }
                KeyCode::Char(c) => app.query.push(c),
                _ => (),
            }
            app.filter();
            app.update_preview(&preview);
            continue;
        }
        let previous = app.selected;
        let mut destination = None;
        let mut reload = false;
        app.message.clear();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
            KeyCode::Tab => app.preview_focus = !app.preview_focus,
            KeyCode::PageDown => {
                app.preview_scroll = app.preview_scroll.saturating_add(10).min(app.preview_max)
            }
            KeyCode::PageUp => app.preview_scroll = app.preview_scroll.saturating_sub(10),
            KeyCode::Down | KeyCode::Char('j') if app.preview_focus => {
                app.preview_scroll = app.preview_scroll.saturating_add(1).min(app.preview_max)
            }
            KeyCode::Up | KeyCode::Char('k') if app.preview_focus => {
                app.preview_scroll = app.preview_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.selected = (app.selected + 1).min(app.filtered.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => app.selected = app.selected.saturating_sub(1),
            KeyCode::Home => app.selected = 0,
            KeyCode::End => app.selected = app.filtered.len().saturating_sub(1),
            KeyCode::Char('/') => app.searching = true,
            KeyCode::Char('?') => {
                app.message =
                    " ← parent · a hidden · s sort · r reload · Tab preview · PgUp/Dn scroll".into()
            }
            KeyCode::Char('d') => {
                app.density = match app.density {
                    Density::Compact => Density::Comfortable,
                    Density::Comfortable => Density::Compact,
                }
            }
            KeyCode::Char('a') => {
                app.all = !app.all;
                reload = true;
            }
            KeyCode::Char('s') => {
                app.sort = match app.sort {
                    Sort::Name => Sort::Size,
                    Sort::Size => Sort::Modified,
                    Sort::Modified => Sort::Name,
                };
                reload = true;
            }
            KeyCode::Char('r') => {
                reload = true;
                refresh_git = true;
                if let Ok(next) = Theme::load(theme.source.as_deref()) {
                    theme = next;
                }
            }
            KeyCode::Char('c') if app.pick != Pick::None && !app.loading => {
                return Ok(Some(app.path.clone()));
            }
            KeyCode::Char('p') if app.pick != Pick::None && !app.loading => {
                if let Some(entry) = app
                    .entry()
                    .filter(|e| app.pick == Pick::Path || e.directory)
                {
                    return Ok(Some(entry.path.clone()));
                }
            }
            KeyCode::Enter | KeyCode::Right if !app.loading => {
                if let Some(entry) = app.entry() {
                    if entry.directory {
                        destination = Some(entry.path.clone());
                    } else if app.pick == Pick::Path {
                        return Ok(Some(entry.path.clone()));
                    }
                }
            }
            KeyCode::Left | KeyCode::Backspace => {
                destination = app.path.parent().map(PathBuf::from)
            }
            _ => (),
        }
        if let Some(path) = destination {
            app.query.clear();
            load.submit(Load {
                path,
                all: app.all,
                sort: app.sort,
            });
            app.loading = true;
            preview.cancel();
            git.cancel();
            app.preview_pending = false;
            app.git_pending = false;
        } else if reload {
            load.submit(Load {
                path: app.path.clone(),
                all: app.all,
                sort: app.sort,
            });
            app.loading = true;
            preview.cancel();
            git.cancel();
            app.preview_pending = false;
            app.git_pending = false;
        } else if previous != app.selected {
            app.update_preview(&preview);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(density: Density) -> App {
        let entries = vec![
            Entry {
                name: "Documents".into(),
                path: "/demo/Documents".into(),
                kind: "dir",
                size: 0,
                modified: None,
                link_target: None,
                permissions: 0o755,
                directory: true,
                git_status: None,
            },
            Entry {
                name: "Привет.md".into(),
                path: "/demo/Привет.md".into(),
                kind: "file",
                size: 128,
                modified: None,
                link_target: None,
                permissions: 0o644,
                directory: false,
                git_status: None,
            },
        ];
        let search = entries.iter().map(|e| e.name.to_lowercase()).collect();
        let labels = entries.iter().map(|e| e.name.clone()).collect();
        let mut git = git::Snapshot {
            branch: "main".into(),
            ..git::Snapshot::default()
        };
        git.changes.insert("/demo/Привет.md".into(), " M".into());
        App {
            path: "/demo".into(),
            entries,
            search,
            labels,
            filtered: vec![0, 1],
            selected: 1,
            offset: 0,
            query: String::new(),
            searching: false,
            all: false,
            sort: Sort::Name,
            density,
            loading: false,
            preview_pending: false,
            git_pending: false,
            preview: "Привет.md\n\nMaterial You\nРусский текст".into(),
            preview_scroll: 0,
            preview_max: 0,
            preview_focus: false,
            git,
            message: String::new(),
            pick: Pick::None,
        }
    }
    #[test]
    fn layout_reference_snapshots() {
        for (name, density) in [
            ("compact", Density::Compact),
            ("comfortable", Density::Comfortable),
        ] {
            let mut app = sample(density);
            let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 18)).unwrap();
            terminal
                .draw(|frame| app.draw(frame, &Theme::default(), true))
                .unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .chunks(100)
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            let text = text.trim_end().to_owned() + "\n";
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/snapshots/{name}.txt"));
            if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &text).unwrap();
            }
            assert_eq!(
                text,
                std::fs::read_to_string(&path).unwrap(),
                "{name} layout changed"
            );
        }
    }
    #[test]
    fn filtering_keeps_unicode_names() {
        let mut app = sample(Density::Compact);
        app.query = "ПРИВЕТ".into();
        app.filter();
        assert_eq!(app.filtered, vec![1]);
    }
}
