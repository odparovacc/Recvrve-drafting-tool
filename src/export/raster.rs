//! Raster backend: draws a [`Scene`] with tiny-skia and encodes PNG, WebP or JPEG.

use ab_glyph::{Font, OutlineCurve};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, Paint, PathBuilder, Pattern, Pixmap, Rect, SpreadMode,
    Stroke, StrokeDash, Transform,
};

use super::icons::Icons;
use super::scene::{self, Anchor, Color, Item, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    WebP,
    Png,
    Jpeg,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 3] = [ImageFormat::WebP, ImageFormat::Png, ImageFormat::Jpeg];

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::WebP => "webp",
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ImageFormat::WebP => "WebP",
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
        }
    }
}

/// Largest raster export, in pixels (about 480 MB of memory while drawing).
pub const MAX_PIXELS: u64 = 120_000_000;
/// Longest side a raster export may have.
pub const MAX_SIDE: u32 = 60_000;

/// Pixel size of `scene` drawn at `scale`.
pub fn pixel_size(scene_w: f32, scene_h: f32, scale: f32) -> (u32, u32) {
    (
        (scene_w * scale).ceil().max(1.0) as u32,
        (scene_h * scale).ceil().max(1.0) as u32,
    )
}

pub fn fits(w: u32, h: u32) -> bool {
    w <= MAX_SIDE && h <= MAX_SIDE && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

fn sk_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn solid(c: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(sk_color(c));
    paint.anti_alias = true;
    paint
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
    }
    // Circular corners from cubic arcs.
    const K: f32 = 0.552_284_8;
    let c = r * K;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + c, y, x + w, y + r - c, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + c, x + w - r + c, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - c, y + h, x, y + h - r + c, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    pb.close();
    pb.finish()
}

fn polygon(mut points: impl Iterator<Item = (f32, f32)>) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (x0, y0) = points.next()?;
    pb.move_to(x0, y0);
    for (x, y) in points {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn shape(segs: &[scene::Seg]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for seg in segs {
        match *seg {
            scene::Seg::Move(x, y) => pb.move_to(x, y),
            scene::Seg::Line(x, y) => pb.line_to(x, y),
            scene::Seg::Cubic(x1, y1, x2, y2, x, y) => pb.cubic_to(x1, y1, x2, y2, x, y),
            scene::Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

/// Draws `scene` at `scale` onto a white pixmap.
pub fn render(scene: &Scene, scale: f32, icons: &Icons) -> Option<Pixmap> {
    let (w, h) = pixel_size(scene.width, scene.height, scale);
    let mut pixmap = Pixmap::new(w, h)?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let ts = Transform::from_scale(scale, scale);
    for item in &scene.items {
        draw_item(&mut pixmap, item, ts, scale, icons);
    }
    Some(pixmap)
}

fn draw_item(pixmap: &mut Pixmap, item: &Item, ts: Transform, scale: f32, icons: &Icons) {
    match item {
        Item::Rect {
            x,
            y,
            w,
            h,
            radius,
            fill,
            stroke,
            dashed,
        } => {
            if let Some(fill) = fill
                && let Some(path) = rounded_rect(*x, *y, *w, *h, *radius)
            {
                pixmap.fill_path(&path, &solid(*fill), FillRule::Winding, ts, None);
            }
            if let Some((color, width)) = stroke {
                // Strokes sit inside the box, like CSS borders.
                let i = width / 2.0;
                if let Some(path) =
                    rounded_rect(x + i, y + i, w - width, h - width, (radius - i).max(0.0))
                {
                    let mut s = Stroke {
                        width: *width,
                        ..Stroke::default()
                    };
                    if *dashed {
                        s.dash = StrokeDash::new(vec![width * 3.0, width * 2.0], 0.0);
                    }
                    pixmap.stroke_path(&path, &solid(*color), &s, ts, None);
                }
            }
        }
        Item::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
        } => {
            let Some(path) = PathBuilder::from_circle(*cx, *cy, *r) else {
                return;
            };
            if let Some(fill) = fill {
                pixmap.fill_path(&path, &solid(*fill), FillRule::Winding, ts, None);
            }
            if let Some((color, width)) = stroke {
                let s = Stroke {
                    width: *width,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &solid(*color), &s, ts, None);
            }
        }
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => {
            let mut pb = PathBuilder::new();
            pb.move_to(*x1, *y1);
            pb.line_to(*x2, *y2);
            if let Some(path) = pb.finish() {
                let s = Stroke {
                    width: *width,
                    line_cap: LineCap::Butt,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &solid(*color), &s, ts, None);
            }
        }
        Item::Poly { points, fill } => {
            if let Some(path) = polygon(points.iter().copied()) {
                pixmap.fill_path(&path, &solid(*fill), FillRule::Winding, ts, None);
            }
        }
        Item::Shape {
            segs,
            fill,
            even_odd,
        } => {
            if let Some(path) = shape(segs) {
                let rule = if *even_odd {
                    FillRule::EvenOdd
                } else {
                    FillRule::Winding
                };
                pixmap.fill_path(&path, &solid(*fill), rule, ts, None);
            }
        }
        Item::IconBand {
            champion,
            x,
            y,
            size,
            poly,
            shift,
        } => {
            let Some(path) = polygon(poly.iter().map(|(px, py)| (x + px * size, y + py * size)))
            else {
                return;
            };
            match icons.get(*champion) {
                Some(icon) => {
                    let src = icon.pixmap(scene::Tone::Color);
                    let k = size / src.width() as f32;
                    let paint = Paint {
                        shader: Pattern::new(
                            src.as_ref(),
                            SpreadMode::Pad,
                            FilterQuality::Bicubic,
                            1.0,
                            Transform::from_row(
                                k,
                                0.0,
                                0.0,
                                k,
                                x + shift.0 * size,
                                y + shift.1 * size,
                            ),
                        ),
                        anti_alias: true,
                        ..Paint::default()
                    };
                    pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
                }
                None => pixmap.fill_path(
                    &path,
                    &solid(scene::palette::BORDER),
                    FillRule::Winding,
                    ts,
                    None,
                ),
            }
        }
        Item::Curve {
            from,
            c1,
            c2,
            to,
            color,
            width,
        } => {
            let mut pb = PathBuilder::new();
            pb.move_to(from.0, from.1);
            pb.cubic_to(c1.0, c1.1, c2.0, c2.1, to.0, to.1);
            if let Some(path) = pb.finish() {
                let s = Stroke {
                    width: *width,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &solid(*color), &s, ts, None);
            }
        }
        Item::Icon {
            champion,
            x,
            y,
            size,
            radius,
            tone,
            opacity,
        } => {
            let Some(path) = rounded_rect(*x, *y, *size, *size, *radius) else {
                return;
            };
            match icons.get(*champion) {
                Some(icon) => {
                    let src = icon.pixmap(*tone);
                    let k = size / src.width() as f32;
                    let paint = Paint {
                        shader: Pattern::new(
                            src.as_ref(),
                            SpreadMode::Pad,
                            FilterQuality::Bicubic,
                            *opacity,
                            Transform::from_row(k, 0.0, 0.0, k, *x, *y),
                        ),
                        anti_alias: true,
                        ..Paint::default()
                    };
                    pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
                }
                None => {
                    pixmap.fill_path(
                        &path,
                        &solid(scene::palette::BORDER),
                        FillRule::Winding,
                        ts,
                        None,
                    );
                }
            }
        }
        Item::Text {
            x,
            y,
            size,
            color,
            bold,
            anchor,
            text,
        } => draw_text(pixmap, *x, *y, *size, *color, *bold, *anchor, text, scale),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
    bold: bool,
    anchor: Anchor,
    text: &str,
    scale: f32,
) {
    let width = scene::text_width(text, size, bold);
    let mut pen = match anchor {
        Anchor::Start => x,
        Anchor::Middle => x - width / 2.0,
        Anchor::End => x - width,
    };
    // All glyphs go into one path so overlapping anti-aliased edges blend once.
    let mut pb = PathBuilder::new();
    // Symbols Lexend Deca lacks come from the fallback font.
    for (fallback, run) in scene::runs(text, bold) {
        let font = scene::face(fallback, bold);
        let units = size / font.units_per_em().unwrap_or(2048.0);
        let mut prev = None;
        for c in run.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                pen += font.kern_unscaled(p, id) * units;
            }
            if let Some(outline) = font.outline(id) {
                // Font units are y-up; flip into the scene's y-down space.
                let map = |p: ab_glyph::Point| (pen + p.x * units, y - p.y * units);
                let mut last: Option<(f32, f32)> = None;
                for curve in &outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _)
                        | OutlineCurve::Quad(a, _, _)
                        | OutlineCurve::Cubic(a, _, _, _) => map(*a),
                    };
                    if last != Some(start) {
                        if last.is_some() {
                            pb.close();
                        }
                        pb.move_to(start.0, start.1);
                    }
                    let end = match curve {
                        OutlineCurve::Line(_, b) => {
                            let b = map(*b);
                            pb.line_to(b.0, b.1);
                            b
                        }
                        OutlineCurve::Quad(_, c, b) => {
                            let (c, b) = (map(*c), map(*b));
                            pb.quad_to(c.0, c.1, b.0, b.1);
                            b
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (c1, c2, b) = (map(*c1), map(*c2), map(*b));
                            pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1);
                            b
                        }
                    };
                    last = Some(end);
                }
                if last.is_some() {
                    pb.close();
                }
            }
            pen += font.h_advance_unscaled(id) * units;
            prev = Some(id);
        }
    }
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &solid(color),
            FillRule::Winding,
            Transform::from_scale(scale, scale),
            None,
        );
    }
}

/// Encodes a rendered pixmap. WebP and PNG are lossless; JPEG uses quality 90.
pub fn encode(pixmap: Pixmap, format: ImageFormat) -> Result<Vec<u8>, String> {
    let (w, h) = (pixmap.width(), pixmap.height());
    // The background is opaque white, so premultiplied and straight colors are equal.
    let rgba = pixmap.take();
    let mut out = Vec::new();
    let result = match format {
        ImageFormat::Png => {
            use image::ImageEncoder;
            image::codecs::png::PngEncoder::new_with_quality(
                &mut out,
                image::codecs::png::CompressionType::Default,
                image::codecs::png::FilterType::Adaptive,
            )
            .write_image(&rgb(&rgba), w, h, image::ExtendedColorType::Rgb8)
        }
        ImageFormat::WebP => {
            use image::ImageEncoder;
            image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(
                &rgb(&rgba),
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )
        }
        ImageFormat::Jpeg => {
            use image::ImageEncoder;
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).write_image(
                &rgb(&rgba),
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )
        }
    };
    result.map_err(|e| e.to_string())?;
    Ok(out)
}

fn rgb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::icons;
    use crate::export::scene::palette;

    #[test]
    fn draws_and_encodes_every_format() {
        let mut scene = Scene::new(60.0, 40.0);
        scene.rect(2.0, 2.0, 20.0, 20.0, 4.0, palette::BLUE);
        scene.items.push(Item::Icon {
            champion: 16,
            x: 30.0,
            y: 2.0,
            size: 20.0,
            radius: 3.0,
            tone: scene::Tone::Gray,
            opacity: 0.8,
        });
        scene.ban_sign(30.0, 2.0, 20.0);
        scene.text(2.0, 36.0, 10.0, palette::TEXT, true, Anchor::Start, "Ab");
        let pixmap = render(&scene, 2.0, icons::for_tests()).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (120, 80));
        // The blue square is blue, the corner outside it white.
        let px = pixmap.pixel(20, 20).unwrap();
        assert_eq!((px.red(), px.green(), px.blue()), (0x25, 0x63, 0xeb));
        let px = pixmap.pixel(0, 79).unwrap();
        assert_eq!((px.red(), px.green(), px.blue()), (255, 255, 255));
        // Some text pixels are dark.
        let dark = (60..80)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .filter(|&(x, y)| pixmap.pixel(x, y).unwrap().red() < 100)
            .count();
        assert!(dark > 20, "{dark}");
        for format in ImageFormat::ALL {
            let bytes = encode(pixmap.clone(), format).unwrap();
            let back = image::load_from_memory(&bytes).unwrap();
            assert_eq!((back.width(), back.height()), (120, 80));
        }
    }
}
