use crate::{
    file_art::Viewport,
    kitty::{Playback, Sprite},
    motion,
    palette::Theme,
    preview::Png,
    surface_art::{self, Surface},
};
use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    queue,
};
use ratatui::layout::Rect;
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Layout {
    pub badge: Option<Rect>,
    pub selected: Option<Selection>,
    pub preview: Option<Rect>,
}

fn fit(area: Rect, (width, height): (usize, usize), (cw, ch): (usize, usize)) -> Rect {
    let max_width = usize::from(area.width) * cw;
    let max_height = usize::from(area.height) * ch;
    // Give Kitty only the limiting dimension so it preserves the aspect ratio.
    if max_width * height <= max_height * width {
        let rows = (max_width * height).div_ceil(width * ch) as u16;
        Rect::new(area.x, area.y + (area.height - rows) / 2, area.width, 0)
    } else {
        let columns = (max_height * width).div_ceil(height * cw) as u16;
        Rect::new(area.x + (area.width - columns) / 2, area.y, 0, area.height)
    }
}

#[derive(Clone, Copy)]
pub struct Selection {
    pub row: Rect,
    pub top: u16,
    pub bottom: u16,
}

#[derive(Clone, Copy, PartialEq)]
struct Placement {
    area: Rect,
    offset: usize,
}

impl From<Rect> for Placement {
    fn from(area: Rect) -> Self {
        Self { area, offset: 0 }
    }
}

struct Flight {
    from: f32,
    velocity: f32,
    start: Instant,
}

struct Focus {
    target: Selection,
    flight: Option<Flight>,
}

impl Focus {
    fn retarget(&mut self, target: Selection, now: Instant, animate: bool) {
        if self.target.row == target.row {
            self.target = target;
            return;
        }
        let (from, velocity) = self.sample(now);
        self.flight = (animate
            && self.target.row.x == target.row.x
            && self.target.row.width == target.row.width)
            .then_some(Flight {
                from,
                velocity,
                start: now,
            });
        self.target = target;
    }

    fn sample(&mut self, now: Instant) -> (f32, f32) {
        let y = f32::from(self.target.row.y);
        if let Some(flight) = &self.flight {
            let elapsed = now.duration_since(flight.start).as_secs_f32();
            let (position, velocity) =
                motion::FOCUS.sample(flight.from, flight.velocity, y, elapsed);
            if elapsed < 0.65 && ((position - y).abs() > 0.015 || velocity.abs() > 0.20) {
                return (position, velocity);
            }
            self.flight = None;
        }
        (y, 0.)
    }

    fn placement(&mut self, now: Instant, cell_height: usize) -> Placement {
        let (position, _) = self.sample(now);
        let position = position.clamp(f32::from(self.target.top), f32::from(self.target.bottom));
        let pixels = (position * cell_height as f32).round() as usize;
        let mut area = self.target.row;
        area.y = (pixels / cell_height) as u16;
        // An explicit row count ends at the cell boundary and would squeeze
        // the pill as its pixel offset grows. Automatic height preserves it.
        area.height = 0;
        Placement {
            area,
            offset: pixels % cell_height,
        }
    }
}

#[derive(Default)]
struct Layer(Option<(Sprite, Placement)>);

impl Layer {
    fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        if let Some((sprite, _)) = self.0.take() {
            sprite.delete(out)?;
        }
        Ok(())
    }

    fn present<W: Write>(
        &mut self,
        out: &mut W,
        placement: Option<Placement>,
        z: i8,
        upload: impl FnOnce(&mut W) -> io::Result<Sprite>,
    ) -> io::Result<()> {
        let Some(placement) = placement else {
            return self.clear(out);
        };
        if self.0.is_none() {
            let sprite = upload(out)?;
            place(out, &sprite, placement, z)?;
            sprite.play(out)?;
            self.0 = Some((sprite, placement));
        } else {
            self.move_to(out, placement, z)?;
        }
        Ok(())
    }

    fn move_to(&mut self, out: &mut impl Write, placement: Placement, z: i8) -> io::Result<()> {
        if let Some((sprite, shown)) = &mut self.0
            && *shown != placement
        {
            place(out, sprite, placement, z)?;
            *shown = placement;
        }
        Ok(())
    }
}

fn place(out: &mut impl Write, sprite: &Sprite, placement: Placement, z: i8) -> io::Result<()> {
    let area = placement.area;
    queue!(out, SavePosition, MoveTo(area.x, area.y))?;
    sprite.place_offset(
        out,
        1,
        (area.width.into(), area.height.into()),
        z,
        None,
        placement.offset,
    )?;
    queue!(out, RestorePosition)
}

pub struct Art {
    badge: Layer,
    selected: Layer,
    preview: Layer,
    focus: Option<Focus>,
    cell_height: usize,
    playback: Playback,
}

impl Art {
    pub fn new(animated: bool) -> Self {
        Self {
            badge: Layer::default(),
            selected: Layer::default(),
            preview: Layer::default(),
            focus: None,
            cell_height: 24,
            playback: Playback::spring(animated),
        }
    }

    pub fn draw(&mut self, layout: Layout, image: Option<&Png>, theme: &Theme) -> io::Result<()> {
        let mut out = io::stderr().lock();
        let cell = Viewport::stderr().map_or((12, 24), |size| size.cell);
        self.cell_height = cell.1;
        self.badge
            .present(&mut out, layout.badge.map(Placement::from), -1, |out| {
                Surface {
                    fill: "primary_container",
                    accent: "on_primary_container",
                    rows: 1,
                }
                .upload(
                    out,
                    theme,
                    layout.badge.expect("path geometry computed").width.into(),
                    surface_art::CELL,
                    self.playback,
                )
            })?;
        let now = Instant::now();
        let placement = if let Some(target) = layout.selected {
            let focus = self.focus.get_or_insert(Focus {
                target,
                flight: None,
            });
            focus.retarget(target, now, matches!(self.playback, Playback::Spring));
            Some(focus.placement(now, self.cell_height))
        } else {
            self.focus = None;
            None
        };
        self.selected.present(&mut out, placement, -1, |out| {
            Surface {
                fill: "secondary_container",
                accent: "tertiary",
                rows: 1,
            }
            .upload(
                out,
                theme,
                placement
                    .expect("selection geometry computed")
                    .area
                    .width
                    .into(),
                cell,
                Playback::Still,
            )
        })?;
        let preview = layout
            .preview
            .zip(image)
            .map(|(area, image)| (fit(area, image.size, cell), image));
        self.preview
            .present(&mut out, preview.map(|(area, _)| area.into()), 1, |out| {
                let (_, image) = preview.expect("PNG layout computed");
                Sprite::png(out, image.size, &image.data)
            })?;
        out.flush()
    }

    pub fn tick(&mut self) -> io::Result<()> {
        if let Some(focus) = &mut self.focus
            && focus.flight.is_some()
        {
            let placement = focus.placement(Instant::now(), self.cell_height);
            let mut out = io::stderr().lock();
            self.selected.move_to(&mut out, placement, -1)?;
            out.flush()?;
        }
        Ok(())
    }

    pub fn frame_interval(&self) -> Option<Duration> {
        self.focus.as_ref()?.flight.as_ref()?;
        Some(Duration::from_millis(16))
    }

    pub fn clear(&mut self, badge: bool, preview: bool) -> io::Result<()> {
        let mut out = io::stderr().lock();
        if badge {
            self.badge.clear(&mut out)?;
            self.selected.clear(&mut out)?;
            self.focus = None;
        }
        if preview {
            self.preview.clear(&mut out)?;
        }
        out.flush()
    }
}

impl Drop for Art {
    fn drop(&mut self) {
        let _ = self.clear(true, true);
    }
}
