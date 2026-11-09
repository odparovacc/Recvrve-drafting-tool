//! Champion portraits for exports, decoded once and shared by every export after that.

use std::path::Path;
use std::sync::OnceLock;

use rayon::prelude::*;
use tiny_skia::{IntSize, Pixmap};

use crate::champions::{self, CHAMPIONS};

use super::scene::Tone;

pub struct Icon {
    /// The original PNG file, embedded as-is in SVG files.
    pub png: Vec<u8>,
    /// Straight (not premultiplied) RGBA pixels.
    rgba: Vec<u8>,
    size: IntSize,
    /// Decoded pixels (premultiplied), for raster output.
    pub color: Pixmap,
    /// Grayscale, purple, blue and red versions, made the first time they are needed.
    toned: [OnceLock<Pixmap>; 4],
}

impl Icon {
    pub fn width(&self) -> u32 {
        self.size.width()
    }

    pub fn height(&self) -> u32 {
        self.size.height()
    }

    /// Straight RGBA pixels in `tone`, e.g. for PDF images.
    pub fn rgba(&self, tone: Tone) -> Vec<u8> {
        let mut data = self.rgba.clone();
        if let Some((dark, light)) = tone.ramp() {
            for px in data.chunks_exact_mut(4) {
                let luma = (0.2126 * f32::from(px[0])
                    + 0.7152 * f32::from(px[1])
                    + 0.0722 * f32::from(px[2]))
                    / 255.0;
                for c in 0..3 {
                    px[c] = (f32::from(dark[c]) + luma * (f32::from(light[c]) - f32::from(dark[c])))
                        .round() as u8;
                }
            }
        }
        data
    }

    pub fn pixmap(&self, tone: Tone) -> &Pixmap {
        let slot = match tone {
            Tone::Color => return &self.color,
            Tone::Gray => 0,
            Tone::Purple => 1,
            Tone::Blue => 2,
            Tone::Red => 3,
        };
        self.toned[slot].get_or_init(|| {
            let mut data = self.rgba(tone);
            for px in data.chunks_exact_mut(4) {
                premultiply(px);
            }
            Pixmap::from_vec(data, self.size).expect("same size as the original")
        })
    }
}

/// Icons indexed by node value. Champions whose file is missing or unreadable have `None`
/// and are drawn as a plain placeholder.
pub struct Icons {
    by_value: Vec<Option<Icon>>,
}

impl Icons {
    /// Reads and decodes every champion icon in `dir`, in parallel.
    pub fn load(dir: Option<&Path>) -> Icons {
        let decoded: Vec<Option<Icon>> = CHAMPIONS
            .par_iter()
            .map(|c| {
                let png = std::fs::read(dir?.join(format!("{}.png", c.key))).ok()?;
                decode(png)
            })
            .collect();
        let mut by_value: Vec<Option<Icon>> = (0..256).map(|_| None).collect();
        for (i, icon) in decoded.into_iter().enumerate() {
            by_value[usize::from(champions::value_of(i))] = icon;
        }
        Icons { by_value }
    }

    pub fn get(&self, champion: u8) -> Option<&Icon> {
        self.by_value[usize::from(champion)].as_ref()
    }
}

fn decode(png: Vec<u8>) -> Option<Icon> {
    let rgba = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let size = IntSize::from_wh(rgba.width(), rgba.height())?;
    let rgba = rgba.into_raw();
    let mut color = rgba.clone();
    for px in color.chunks_exact_mut(4) {
        premultiply(px);
    }
    Some(Icon {
        png,
        rgba,
        size,
        color: Pixmap::from_vec(color, size)?,
        toned: Default::default(),
    })
}

fn premultiply(px: &mut [u8]) {
    let a = u16::from(px[3]);
    for c in &mut px[..3] {
        *c = ((u16::from(*c) * a + 127) / 255) as u8;
    }
}

/// The shared icon set, loaded on first use.
pub fn shared() -> &'static Icons {
    static ICONS: OnceLock<Icons> = OnceLock::new();
    ICONS.get_or_init(|| Icons::load(crate::ICON_DIR.get().and_then(Option::as_deref)))
}

#[cfg(test)]
pub fn for_tests() -> &'static Icons {
    static ICONS: OnceLock<Icons> = OnceLock::new();
    ICONS.get_or_init(|| {
        Icons::load(Some(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(crate::ICON_DIR_NAME),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_icons_with_gray_copies() {
        let icons = for_tests();
        let ahri = icons
            .get(champions::FIRST_CHAMPION_VALUE)
            .expect("Alistar icon");
        let gray = ahri.pixmap(Tone::Gray);
        assert_eq!(ahri.color.width(), gray.width());
        let px = gray.pixels()[gray.pixels().len() / 2];
        assert_eq!(px.red(), px.green());
        assert_eq!(px.green(), px.blue());
        let px = ahri.pixmap(Tone::Purple).pixels()[gray.pixels().len() / 2];
        assert!(
            px.red() > px.green() && px.blue() > px.green(),
            "purple: {px:?}"
        );
        assert!(icons.get(0).is_none());
    }
}
