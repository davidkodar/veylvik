//! Read-only desktop theme integration; no desktop hooks or configuration writes.
use crate::preferences::Theme;
use eframe::egui::{self, Color32};
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub panel: Color32,
    pub canvas: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub selection: Color32,
}
fn rgb(v: [u8; 3]) -> Color32 {
    Color32::from_rgb(v[0], v[1], v[2])
}
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    rgb(std::array::from_fn(|i| {
        (a[i] as f32 * (1. - t) + b[i] as f32 * t).round() as u8
    }))
}
fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| {
        let s = v as f32 / 255.;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
pub fn ink(background: Color32) -> Color32 {
    if contrast(background, Color32::WHITE) > contrast(background, Color32::BLACK) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}
impl Palette {
    pub fn preset(theme: Theme) -> Self {
        let (dark, panel, canvas, accent) = match theme {
            Theme::Light => (false, [242, 245, 249], [225, 231, 239], [41, 85, 173]),
            Theme::Blue => (true, [24, 39, 61], [13, 23, 39], [104, 176, 246]),
            Theme::Green => (true, [25, 44, 38], [14, 28, 24], [104, 211, 167]),
            _ => (true, [32, 35, 41], [18, 20, 25], [167, 106, 238]),
        };
        Self::from_colours(
            dark,
            rgb(panel),
            rgb(canvas),
            if dark {
                rgb([233, 238, 245])
            } else {
                rgb([27, 35, 48])
            },
            rgb(accent),
        )
    }
    fn from_colours(
        dark: bool,
        panel: Color32,
        canvas: Color32,
        text: Color32,
        accent: Color32,
    ) -> Self {
        let text = if contrast(text, panel).min(contrast(text, canvas)) >= 4.5 {
            text
        } else {
            ink(panel)
        };
        let mut muted = mix(text, panel, 0.28);
        if contrast(muted, panel).min(contrast(muted, canvas)) < 4.5 {
            muted = text;
        }
        let selection = if dark {
            mix(accent, panel, 0.45)
        } else {
            accent
        };
        Self {
            dark,
            panel,
            canvas,
            text,
            muted,
            border: mix(panel, text, 0.25),
            accent,
            selection,
        }
    }
    pub fn omarchy(contents: &str) -> Option<Self> {
        let colour = |key: &str| {
            contents.lines().find_map(|line| {
                let (name, value) = line.split_once('=')?;
                if name.trim() != key {
                    return None;
                }
                let value = value.trim().strip_prefix('"')?.split('"').next()?;
                let hex = value.strip_prefix('#')?;
                if hex.len() != 6 {
                    return None;
                }
                let v = u32::from_str_radix(hex, 16).ok()?;
                Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
            })
        };
        let canvas = colour("background")?;
        let text = colour("foreground")?;
        let mode = contents.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "mode").then_some(value.trim())
        });
        let dark = match mode {
            Some(value) if value.starts_with("\"light\"") => false,
            Some(value) if value.starts_with("\"dark\"") => true,
            _ => luminance(canvas) < 0.4,
        };
        let panel = colour("lighter_background").unwrap_or_else(|| mix(canvas, text, 0.05));
        Some(Self::from_colours(
            dark,
            panel,
            canvas,
            text,
            colour("accent").unwrap_or(text),
        ))
    }
}
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(egui::Id::new("veylvik-palette")))
        .unwrap_or_else(|| Palette::preset(Theme::Dark))
}
pub struct System {
    checked: Instant,
    cached: Option<Palette>,
    receiver: Option<mpsc::Receiver<Option<bool>>>,
    dark: Option<bool>,
    pub label: String,
}
impl Default for System {
    fn default() -> Self {
        Self {
            checked: Instant::now() - Duration::from_secs(3),
            cached: None,
            receiver: None,
            dark: None,
            label: "System theme unavailable; using Dark".into(),
        }
    }
}
impl System {
    pub fn resolve(&mut self, theme: Theme, ctx: &egui::Context) -> Palette {
        if theme != Theme::System {
            return Palette::preset(theme);
        }
        if let Some(rx) = &self.receiver {
            match rx.try_recv() {
                Ok(value) => {
                    self.dark = value;
                    self.receiver = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.receiver = None,
                _ => {}
            }
        }
        if self.checked.elapsed() >= Duration::from_secs(2) {
            self.checked = Instant::now();
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let state = std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".local/state")));
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".config")));
            let paths = [
                state.map(|p| p.join("omarchy/current/theme/colors.toml")),
                config.map(|p| p.join("omarchy/current/theme/colors.toml")),
            ];
            self.cached = paths.into_iter().flatten().find_map(|path| {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(path)
                    .ok()?
                    .take(65536)
                    .read_to_string(&mut text)
                    .ok()?;
                Palette::omarchy(&text)
            });
            if self.cached.is_none() && self.receiver.is_none() {
                let (tx, rx) = mpsc::channel();
                self.receiver = Some(rx);
                let context = ctx.clone();
                let _ = std::thread::Builder::new()
                    .name("desktop-appearance".into())
                    .spawn(move || {
                        let dark = pollster::block_on(async {
                            let settings = ashpd::desktop::settings::Settings::new().await.ok()?;
                            match settings.color_scheme().await.ok()? {
                                ashpd::desktop::settings::ColorScheme::PreferDark => Some(true),
                                ashpd::desktop::settings::ColorScheme::PreferLight => Some(false),
                                _ => None,
                            }
                        });
                        let _ = tx.send(dark);
                        context.request_repaint();
                    });
            }
        }
        if let Some(p) = self.cached {
            self.label = "Following Omarchy’s active palette · updates automatically".into();
            p
        } else {
            let dark = self
                .dark
                .or_else(|| ctx.system_theme().map(|t| t == egui::Theme::Dark));
            self.label = if dark.is_some() {
                "Following the desktop’s light/dark preference"
            } else {
                "No desktop preference available · using Dark"
            }
            .into();
            Palette::preset(if dark == Some(false) {
                Theme::Light
            } else {
                Theme::Dark
            })
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palettes_keep_text_legible() {
        for t in Theme::ALL {
            let p = Palette::preset(t);
            for bg in [p.panel, p.canvas] {
                assert!(contrast(p.text, bg) >= 4.5);
                assert!(contrast(p.muted, bg) >= 4.5);
            }
            assert!(contrast(ink(p.selection), p.selection) >= 4.5);
        }
    }
    #[test]
    fn reads_omarchy_dark_and_light_without_executing_content() {
        let p = Palette::omarchy(
            "background = \"#ffffff\"\nforeground = \"#222222\"\naccent = \"#345678\"",
        )
        .unwrap();
        assert!(!p.dark);
        assert_eq!(p.accent, Color32::from_rgb(52, 86, 120));
        assert!(Palette::omarchy("background = \"$(bad)\"").is_none());
    }
}
