use crate::palette::Theme;
/// Route output and every size query to the UI stream, including Ratatui's
/// internal viewport clearing path. Crossterm's default size() probes stdout.
pub struct StderrBackend(ratatui::backend::CrosstermBackend<std::io::Stderr>);
impl StderrBackend {
    pub fn new() -> Self {
        Self(ratatui::backend::CrosstermBackend::new(std::io::stderr()))
    }
}
impl ratatui::backend::Backend for StderrBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, cells: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.0.draw(cells)
    }
    fn append_lines(&mut self, n: u16) -> std::io::Result<()> {
        self.0.append_lines(n)
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        self.0.hide_cursor()
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        self.0.show_cursor()
    }
    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        self.0.get_cursor_position()
    }
    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        p: P,
    ) -> std::io::Result<()> {
        self.0.set_cursor_position(p)
    }
    fn clear(&mut self) -> std::io::Result<()> {
        self.0.clear()
    }
    fn clear_region(&mut self, kind: ratatui::backend::ClearType) -> std::io::Result<()> {
        self.0.clear_region(kind)
    }
    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        Ok(terminal_area()?.as_size())
    }
    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        Ok(ratatui::backend::WindowSize {
            columns_rows: self.size()?,
            pixels: ratatui::layout::Size::new(0, 0),
        })
    }
    fn flush(&mut self) -> std::io::Result<()> {
        ratatui::backend::Backend::flush(&mut self.0)
    }
}
pub fn terminal_area() -> std::io::Result<ratatui::layout::Rect> {
    #[cfg(unix)]
    {
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        // Query the actual UI descriptor. stdout may be a pipe in picker mode,
        // and /dev/tty is not guaranteed to exist in containers or test PTYs.
        if unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut size) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(ratatui::layout::Rect::new(0, 0, size.ws_col, size.ws_row))
    }
    #[cfg(not(unix))]
    {
        let (w, h) = crossterm::terminal::size()?;
        Ok(ratatui::layout::Rect::new(0, 0, w, h))
    }
}
use clap::ValueEnum;
use ratatui::{
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders},
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
}
impl Density {
    pub fn margin(self) -> u16 {
        match self {
            Self::Compact => 0,
            Self::Comfortable => 1,
        }
    }
    pub fn row_height(self) -> usize {
        match self {
            Self::Compact => 1,
            Self::Comfortable => 2,
        }
    }
}
pub fn style(theme: &Theme, fg: &str, bg: &str, color: bool) -> Style {
    if color {
        Style::default()
            .fg(theme.accessible(fg, bg))
            .bg(theme.color(bg))
    } else {
        Style::default()
    }
}
pub fn surface(title: impl Into<String>, theme: &Theme, color: bool) -> Block<'static> {
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(style(theme, "on_surface", "surface_container", color))
        .border_style(if color {
            Style::default()
                .fg(theme.color("outline"))
                .bg(theme.color("surface_container"))
        } else {
            Style::default()
        })
}
pub fn chip(text: impl Into<String>, theme: &Theme, color: bool) -> Span<'static> {
    Span::styled(
        format!(" {} ", text.into()),
        style(theme, "on_primary_container", "primary_container", color)
            .add_modifier(Modifier::BOLD),
    )
}
pub fn selected(theme: &Theme, color: bool) -> Style {
    style(
        theme,
        "on_secondary_container",
        "secondary_container",
        color,
    )
    .add_modifier(Modifier::BOLD)
}
pub fn actions() -> &'static str {
    " ↑↓ move · Enter open · / find · d density · ? keys · q exit"
}
