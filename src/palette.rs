use ratatui::style::Color;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
struct Palette {
    #[serde(default = "dark")]
    mode: String,
    colors: BTreeMap<String, String>,
}
fn dark() -> String {
    "dark".into()
}

pub struct Theme {
    pub mode: String,
    pub source: Option<PathBuf>,
    pub warning: Option<String>,
    pub colors: BTreeMap<String, String>,
}

fn xdg(var: &str, suffix: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(suffix))
}

pub fn rgb(value: &str) -> Option<(u8, u8, u8)> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    Some((
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ))
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: dark(),
            source: None,
            warning: None,
            colors: [
                ("primary", "#D0BCFF"),
                ("on_primary", "#381E72"),
                ("primary_container", "#4F378B"),
                ("on_primary_container", "#EADDFF"),
                ("secondary", "#CCC2DC"),
                ("secondary_container", "#4A4458"),
                ("on_secondary_container", "#E8DEF8"),
                ("tertiary", "#EFB8C8"),
                ("surface", "#141218"),
                ("surface_container", "#211F26"),
                ("surface_container_high", "#2B2930"),
                ("on_surface", "#E6E0E9"),
                ("on_surface_variant", "#CAC4D0"),
                ("outline", "#938F99"),
                ("error", "#F2B8B5"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        }
    }
}

impl Theme {
    pub fn contrast(&self, fg: &str, bg: &str) -> f64 {
        contrast(self.color(fg), self.color(bg))
    }

    /// Keep semantic accents when legible; otherwise fall back to readable text.
    pub fn accessible(&self, fg: &str, bg: &str) -> Color {
        let background = self.color(bg);
        let foreground = self.color(fg);
        if contrast(foreground, background) >= 4.5 {
            return foreground;
        }
        let text = self.color("on_surface");
        if contrast(text, background) >= 4.5 {
            return text;
        }
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        if contrast(black, background) > contrast(white, background) {
            black
        } else {
            white
        }
    }
    pub fn load(explicit: Option<&Path>) -> io::Result<Self> {
        if let Some(path) = explicit {
            return Self::from_path(path);
        }
        if let Some(path) = env::var_os("MYT_PALETTE") {
            return Self::from_path(Path::new(&path));
        }
        if let Some(path) = env::var_os("MYT_THEME_DIR") {
            return Self::from_path(&PathBuf::from(path).join("palette.json"));
        }
        let config_path = env::var_os("MYT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                xdg("XDG_CONFIG_HOME", ".config").join("material-you-terminal/config.json")
            });
        let config = match fs::read(&config_path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(io::Error::other)?,
            ),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let configured = config
            .as_ref()
            .and_then(|v| v.get("themeDir"))
            .and_then(|v| v.as_str())
            .map(|p| {
                if let Some(suffix) = p.strip_prefix("~/") {
                    PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(suffix)
                } else {
                    PathBuf::from(p)
                }
            });
        let path = configured
            .unwrap_or_else(|| {
                xdg("XDG_STATE_HOME", ".local/state").join("material-you-terminal/theme")
            })
            .join("palette.json");
        match Self::from_path(&path) {
            Ok(theme) => Ok(theme),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self {
                warning: Some(format!(
                    "No palette at {}; using built-in dark colors",
                    path.display()
                )),
                ..Self::default()
            }),
            Err(e) => Err(e),
        }
    }

    fn from_path(path: &Path) -> io::Result<Self> {
        let palette: Palette =
            serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        if !matches!(palette.mode.as_str(), "light" | "dark") {
            return Err(io::Error::other("palette mode must be light or dark"));
        }
        for (key, value) in &palette.colors {
            if rgb(value).is_none() {
                return Err(io::Error::other(format!("invalid palette color: {key}")));
            }
        }
        // A complete set avoids accidentally mixing dark fallback text with light surfaces.
        for role in Self::default().colors.keys() {
            if !palette.colors.contains_key(role) {
                return Err(io::Error::other(format!("palette is missing role: {role}")));
            }
        }
        Ok(Self {
            mode: palette.mode,
            source: Some(path.into()),
            warning: None,
            colors: palette.colors,
        })
    }

    pub fn color(&self, role: &str) -> Color {
        let (r, g, b) = self
            .colors
            .get(role)
            .and_then(|v| rgb(v))
            .unwrap_or((230, 224, 233));
        Color::Rgb(r, g, b)
    }

    pub fn paint(&self, text: &str, fg: &str, bg: Option<&str>, enabled: bool) -> String {
        if !enabled {
            return text.into();
        }
        let Color::Rgb(r, g, b) = self.accessible(fg, bg.unwrap_or("surface")) else {
            unreachable!()
        };
        let mut result = format!("\x1b[38;2;{r};{g};{b}m");
        if let Some(bg) = bg {
            let Color::Rgb(r, g, b) = self.color(bg) else {
                unreachable!()
            };
            result.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
        }
        result.push_str(text);
        result.push_str("\x1b[0m");
        result
    }
}

pub fn contrast(a: Color, b: Color) -> f64 {
    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            return 0.0;
        };
        let linear = |n: u8| {
            let n = f64::from(n) / 255.0;
            if n <= 0.04045 {
                n / 12.92
            } else {
                ((n + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_is_legible_for_six_generated_palettes_and_low_contrast_overrides() {
        for seed in ["purple", "blue", "green"] {
            for mode in ["dark", "light"] {
                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/palettes/{seed}-{mode}.json"));
                let theme = Theme::load(Some(&path)).unwrap();
                for pair in crate::tool_themes::contrasts(&theme) {
                    assert!(
                        pair.effective >= 4.5,
                        "{seed} {mode}: {} / {}",
                        pair.foreground,
                        pair.background
                    );
                }
            }
        }
        let mut theme = Theme::default();
        for value in theme.colors.values_mut() {
            *value = "#808080".into();
        }
        for pair in crate::tool_themes::contrasts(&theme) {
            assert!(pair.adjusted);
            assert!(pair.effective >= 4.5);
        }
    }
    #[test]
    fn validates_hex_without_panicking_on_unicode() {
        assert_eq!(rgb("#1234AB"), Some((18, 52, 171)));
        for bad in ["#12", "#GG0000", "ééé", "1234567"] {
            assert!(rgb(bad).is_none());
        }
    }
    #[test]
    fn refuses_partial_light_palette() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("palette.json");
        fs::write(
            &file,
            r##"{"mode":"light","colors":{"surface":"#ffffff"}}"##,
        )
        .unwrap();
        assert!(
            Theme::from_path(&file)
                .err()
                .unwrap()
                .to_string()
                .contains("missing role")
        );
    }
}
