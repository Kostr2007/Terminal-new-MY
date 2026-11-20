use crate::{
    browser_art,
    file_art::Icon,
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
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};
use std::{io, path::PathBuf, time::Duration};
use unicode_width::UnicodeWidthStr;

struct Restore;
fn unavailable_size_probe(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::WouldBlock
    ) || matches!(error.raw_os_error(), Some(libc::ENOTTY | libc::ENOENT))
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
    pub graphics: bool,
    pub animated: bool,
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
    preview_png: Option<preview::Png>,
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
    fn update_preview(&mut self, worker: &Worker<Entry, preview::Content>) {
        self.preview_scroll = 0;
        self.preview_png = None;
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
    fn draw(
        &mut self,
        frame: &mut Frame,
        theme: &Theme,
        color: bool,
        graphics: bool,
    ) -> browser_art::Layout {
        let mut art = browser_art::Layout::default();
        let area = frame.area();
        frame.render_widget(
            Block::default().style(ui::style(theme, "on_surface", "surface", color)),
            area,
        );
        let margin = u16::from(area.width >= 32 && matches!(self.density, Density::Comfortable));
        let content = Rect::new(
            area.x + margin,
            area.y,
            area.width.saturating_sub(2 * margin),
            area.height,
        );
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(if area.width < 70 { 2 } else { 1 }),
        ])
        .split(content);
        let native = color && graphics && content.width >= 16;
        let path_area = Rect::new(rows[0].x, rows[0].y, rows[0].width, rows[0].height.min(1));
        if native && !path_area.is_empty() {
            art.badge = Some(path_area);
        }
        let path = format!("  {}", render::safe(&self.path.to_string_lossy()));
        let mut path_style = ui::style(theme, "on_primary_container", "primary_container", color)
            .add_modifier(Modifier::BOLD);
        if art.badge.is_some() {
            path_style = path_style.bg(theme.color("surface"));
        }
        let status = if self.loading {
            "  Contents  ·  Loading folder…".into()
        } else {
            let branch = if self.git_pending {
                "  ·  Git scanning".into()
            } else if !self.git.branch.is_empty() {
                format!("  ·  {}", render::safe(&self.git.branch))
            } else if self.git.warning.is_some() {
                "  ·  Git unavailable".into()
            } else {
                String::new()
            };
            format!(
                "  Contents  ·  {}  ·  {:?}{branch}",
                self.filtered.len(),
                self.sort
            )
        };
        frame.render_widget(Paragraph::new(Line::styled(path, path_style)), rows[0]);

        let available = rows[1];
        if area.width >= 90 {
            let gutter = if margin == 0 { 1 } else { 2 };
            let list_width = (u32::from(available.width) * 46 / 100) as u16;
            let list = Rect::new(available.x, available.y, list_width, available.height);
            let preview = Rect::new(
                list.right() + gutter,
                available.y,
                available.width - list_width - gutter,
                available.height,
            );
            art.selected = self.draw_list(frame, list, theme, color, native, &status);
            art.preview = self.draw_preview(frame, preview, theme, color);
        } else {
            art.selected = self.draw_list(frame, available, theme, color, native, &status);
        }

        let help = if self.searching {
            format!(
                "  Find  {}  ·  Enter done  ·  Esc clear",
                render::safe(&self.query)
            )
        } else if !self.message.is_empty() {
            self.message.clone()
        } else if self.preview_focus {
            "  ↑↓ scroll  ·  PgUp/Dn page  ·  Tab files  ·  q exit".into()
        } else if self.pick != Pick::None {
            "  c choose folder  ·  p choose entry  ·  Enter open  ·  q cancel".into()
        } else {
            "  ↑↓ move  ·  Enter open  ·  / find  ·  Tab preview  ·  ? keys  ·  q exit".into()
        };
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .style(ui::style(
                    theme,
                    "on_surface_variant",
                    "surface_container",
                    color,
                )),
            rows[2],
        );
        art
    }

    fn draw_list(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        color: bool,
        graphics: bool,
        status: &str,
    ) -> Option<Rect> {
        let title = Rect::new(area.x, area.y, area.width, area.height.min(1));
        frame.render_widget(
            Paragraph::new(status).style(
                ui::style(theme, "primary", "surface_container_high", color)
                    .add_modifier(Modifier::BOLD),
            ),
            title,
        );
        let visible = usize::from(area.height.saturating_sub(1));
        if visible == 0 {
            return None;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
        let mut selection = None;
        for (row, &index) in self
            .filtered
            .iter()
            .skip(self.offset)
            .take(visible)
            .enumerate()
        {
            let entry = &self.entries[index];
            let selected = self.offset + row == self.selected;
            let native = selected && graphics;
            let rect = Rect::new(area.x, area.y + row as u16 + 1, area.width, 1);
            if native {
                selection = Some(rect);
            }
            let surface = if selected {
                "secondary_container"
            } else if entry.directory {
                "surface_container_high"
            } else {
                "surface_container"
            };
            let ink = if selected {
                "on_secondary_container"
            } else {
                render::role(entry)
            };
            let lead = if selected && !native { "› " } else { "  " };
            let git = self.git.label(&entry.path);
            let suffix = if git.trim().is_empty() {
                String::new()
            } else {
                format!("  {git}")
            };
            let details = if matches!(self.density, Density::Comfortable)
                && area.width >= 36
                && !entry.directory
            {
                format!("  {:>9}", files::size(entry.size))
            } else {
                String::new()
            };
            let prefix = format!("{lead}{} ", Icon::for_entry(entry).glyph());
            let max = usize::from(area.width)
                .saturating_sub(prefix.width() + suffix.width() + details.width() + 1);
            let name = render::truncate(&self.labels[index], max);
            let pad = if details.is_empty() {
                0
            } else {
                max.saturating_sub(name.width())
            };
            let label = format!("{prefix}{name}{suffix}{:pad$}{details}", "");
            let mut style = ui::style(theme, ink, surface, color);
            if selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            if native {
                // Kitty supplies the silhouette. Leave the cells beneath its
                // transparent corners in the surrounding surface color.
                style = style.bg(theme.color("surface"));
            }
            frame.render_widget(Paragraph::new(label).style(style), rect);
        }
        selection
    }

    fn draw_preview(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        color: bool,
    ) -> Option<Rect> {
        let surface = "surface_container";
        frame.render_widget(
            Block::default().style(ui::style(theme, "on_surface", surface, color)),
            area,
        );
        let title = Rect::new(area.x, area.y, area.width, area.height.min(1));
        let body = Rect::new(
            area.x + 1,
            title.bottom(),
            area.width.saturating_sub(2),
            area.height.saturating_sub(1),
        );
        let focus = if self.preview_focus {
            "  ·  Focus"
        } else {
            ""
        };
        let image = self
            .preview_png
            .as_ref()
            .filter(|_| body.width >= 8 && body.height >= 5);
        let style = ui::style(theme, "on_surface_variant", "surface_container_high", color)
            .add_modifier(Modifier::BOLD);
        if let Some(image) = image {
            self.preview_max = 0;
            self.preview_scroll = 0;
            let (width, height) = image.size;
            frame.render_widget(
                Paragraph::new(format!("  Preview  ·  PNG {width} × {height}{focus}")).style(style),
                title,
            );
            return Some(Rect::new(body.x, body.y + 1, body.width, body.height - 2));
        }
        let paragraph = Paragraph::new(self.preview.as_str()).wrap(Wrap { trim: false });
        self.preview_max = paragraph
            .line_count(body.width)
            .saturating_sub(1)
            .min(u16::MAX as usize) as u16;
        self.preview_scroll = self.preview_scroll.min(self.preview_max);
        let line = if self.preview_scroll > 0 {
            format!("  ·  line {}", self.preview_scroll + 1)
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(format!("  Preview{line}{focus}")).style(style),
            title,
        );
        frame.render_widget(paragraph.scroll((self.preview_scroll, 0)), body);
        None
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
    let preview = Worker::new(move |entry, cancel: &crate::worker::Cancel| {
        preview::load(entry, cancel, options.graphics)
    });
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
        preview_png: None,
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
    let mut art = options
        .graphics
        .then(|| browser_art::Art::new(options.animated));
    let mut dirty = true;
    let mut refresh_git = false;
    loop {
        let current_area = ui::terminal_area()?;
        if current_area != area {
            if let Some(art) = &mut art {
                art.clear(true, true)?;
            }
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
                    if path != app.path
                        && let Some(art) = &mut art
                    {
                        art.clear(true, false)?;
                    }
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
            if let Some(art) = &mut art {
                art.clear(false, true)?;
            }
            app.preview = result.text;
            app.preview_png = result.png;
            app.preview_pending = false;
            dirty = true;
        }
        if let Some(result) = git.result() {
            app.git = result;
            app.git_pending = false;
            dirty = true;
        }
        if dirty {
            let mut layout = browser_art::Layout::default();
            terminal.draw(|frame| layout = app.draw(frame, &theme, color, art.is_some()))?;
            if let Some(art) = &mut art {
                art.draw(layout, app.preview_png.as_ref(), &theme)?;
            }
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
            // A picker may have none of these. A failed tput can also report
            // a stale EAGAIN; the UI probes stderr independently.
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
                    if let Some(art) = &mut art {
                        art.clear(true, false)?;
                    }
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
            app.preview_png = None;
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
            app.preview_png = None;
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
            preview_png: None,
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
                .draw(|frame| {
                    app.draw(frame, &Theme::default(), true, false);
                })
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
