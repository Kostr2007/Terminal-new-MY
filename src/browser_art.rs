use crate::{
    file_art::{Icon, Viewport},
    kitty::{Playback, Sprite},
    palette::Theme,
    preview::Png,
};
use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    queue,
};
use ratatui::layout::Rect;
use std::io::{self, Write};

#[derive(Default)]
pub struct Layout {
    pub badge: Option<Rect>,
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

#[derive(Default)]
struct Layer(Option<(Sprite, Rect)>);

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
        area: Option<Rect>,
        upload: impl FnOnce(&mut W) -> io::Result<Sprite>,
    ) -> io::Result<()> {
        let Some(area) = area else {
            return self.clear(out);
        };
        if self.0.as_ref().is_some_and(|(_, shown)| *shown == area) {
            return Ok(());
        }
        let new = self.0.is_none();
        if new {
            self.0 = Some((upload(out)?, area));
        }
        let (sprite, shown) = self.0.as_mut().expect("image uploaded");
        queue!(out, SavePosition, MoveTo(area.x, area.y))?;
        sprite.place_in(out, 1, (area.width.into(), area.height.into()), 1, None)?;
        queue!(out, RestorePosition)?;
        if new {
            sprite.play(out)?;
        }
        *shown = area;
        Ok(())
    }
}

pub struct Art {
    badge: Layer,
    preview: Layer,
    playback: Playback,
}

impl Art {
    pub fn new(animated: bool) -> Self {
        Self {
            badge: Layer::default(),
            preview: Layer::default(),
            playback: Playback::entrance(animated),
        }
    }

    pub fn draw(&mut self, layout: Layout, image: Option<&Png>, theme: &Theme) -> io::Result<()> {
        let mut out = io::stderr().lock();
        self.badge.present(&mut out, layout.badge, |out| {
            Icon::Folder.upload(out, theme, self.playback)
        })?;
        let preview = layout.preview.zip(image).map(|(area, image)| {
            let cell = Viewport::stderr().map_or((12, 24), |size| size.cell);
            (fit(area, image.size, cell), image)
        });
        self.preview
            .present(&mut out, preview.map(|(area, _)| area), |out| {
                let (_, image) = preview.expect("PNG layout computed");
                Sprite::png(out, image.size, &image.data)
            })?;
        out.flush()
    }

    pub fn clear(&mut self, badge: bool, preview: bool) -> io::Result<()> {
        let mut out = io::stderr().lock();
        if badge {
            self.badge.clear(&mut out)?;
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
