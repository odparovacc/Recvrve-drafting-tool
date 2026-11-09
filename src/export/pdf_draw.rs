//! Draws [`Scene`]s onto krilla PDF surfaces as vector graphics.
//!
//! Text stays text (the bundled font is embedded and subset), shapes stay paths, and each
//! champion portrait becomes one PDF image object that every page refers to, so a PDF
//! with thousands of drafts stays small.

use std::collections::HashMap;

use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Point, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, LineCap, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};

use super::icons::Icons;
use super::scene::{self, Anchor, Color, Item, Scene, Seg, Tone};

pub struct PdfPainter<'a> {
    icons: &'a Icons,
    regular: Font,
    bold: Font,
    fallback_regular: Font,
    fallback_bold: Font,
    images: HashMap<(u8, Tone), Option<Image>>,
}

fn paint(c: Color) -> krilla::paint::Paint {
    rgb::Color::new(c.r, c.g, c.b).into()
}

fn opacity(v: f32) -> NormalizedF32 {
    NormalizedF32::new(v.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

pub fn fill(c: Color) -> Fill {
    Fill {
        paint: paint(c),
        opacity: opacity(c.opacity()),
        rule: FillRule::NonZero,
    }
}

fn stroke(c: Color, width: f32, dashed: bool) -> Stroke {
    Stroke {
        paint: paint(c),
        width,
        opacity: opacity(c.opacity()),
        line_cap: LineCap::Butt,
        dash: dashed.then(|| StrokeDash {
            array: vec![width * 3.0, width * 2.0],
            offset: 0.0,
        }),
        ..Stroke::default()
    }
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
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

fn polygon(mut points: impl Iterator<Item = (f32, f32)>) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (x0, y0) = points.next()?;
    pb.move_to(x0, y0);
    for (x, y) in points {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    const K: f32 = 0.552_284_8;
    let c = r * K;
    let mut pb = PathBuilder::new();
    pb.move_to(cx + r, cy);
    pb.cubic_to(cx + r, cy + c, cx + c, cy + r, cx, cy + r);
    pb.cubic_to(cx - c, cy + r, cx - r, cy + c, cx - r, cy);
    pb.cubic_to(cx - r, cy - c, cx - c, cy - r, cx, cy - r);
    pb.cubic_to(cx + c, cy - r, cx + r, cy - c, cx + r, cy);
    pb.close();
    pb.finish()
}

impl<'a> PdfPainter<'a> {
    pub fn new(icons: &'a Icons) -> Self {
        PdfPainter {
            icons,
            regular: Font::new(scene::FONT_REGULAR_BYTES.into(), 0).expect("bundled font"),
            bold: Font::new(scene::FONT_BOLD_BYTES.into(), 0).expect("bundled font"),
            fallback_regular: Font::new(scene::FALLBACK_REGULAR_BYTES.into(), 0)
                .expect("bundled font"),
            fallback_bold: Font::new(scene::FALLBACK_BOLD_BYTES.into(), 0).expect("bundled font"),
            images: HashMap::new(),
        }
    }

    /// The PDF image for a portrait, created once per (champion, tone) pair. krilla
    /// writes each distinct image to the file only once, however often it is drawn.
    fn image(&mut self, champion: u8, tone: Tone) -> Option<Image> {
        let icons = self.icons;
        self.images
            .entry((champion, tone))
            .or_insert_with(|| {
                let icon = icons.get(champion)?;
                if tone == Tone::Color {
                    Image::from_png(icon.png.clone().into(), true).ok()
                } else {
                    Some(Image::from_rgba8(
                        icon.rgba(tone),
                        icon.width(),
                        icon.height(),
                    ))
                }
            })
            .clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        surface: &mut Surface,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        text: &str,
    ) {
        surface.set_stroke(None);
        surface.set_fill(Some(fill(color)));
        // Symbols Lexend Deca lacks come from the fallback font, run by run.
        let mut pen = x;
        for (fallback, run) in scene::runs(text, bold) {
            let font = match (fallback, bold) {
                (false, false) => &self.regular,
                (false, true) => &self.bold,
                (true, false) => &self.fallback_regular,
                (true, true) => &self.fallback_bold,
            };
            surface.draw_text(
                Point::from_xy(pen, y),
                font.clone(),
                size,
                run,
                false,
                TextDirection::Auto,
            );
            pen += scene::run_width(run, size, fallback, bold);
        }
    }

    /// Draws `scene` with `transform` applied (scene units to PDF points).
    pub fn draw(&mut self, surface: &mut Surface, scene: &Scene, transform: Transform) {
        surface.push_transform(&transform);
        for item in &scene.items {
            self.draw_item(surface, item);
        }
        surface.pop();
    }

    fn draw_item(&mut self, surface: &mut Surface, item: &Item) {
        match item {
            Item::Rect {
                x,
                y,
                w,
                h,
                radius,
                fill: f,
                stroke: s,
                dashed,
            } => {
                if let Some(c) = f
                    && let Some(path) = rounded_rect(*x, *y, *w, *h, *radius)
                {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*c)));
                    surface.draw_path(&path);
                }
                if let Some((c, width)) = s {
                    let i = width / 2.0;
                    if let Some(path) =
                        rounded_rect(x + i, y + i, w - width, h - width, (radius - i).max(0.0))
                    {
                        surface.set_fill(None);
                        surface.set_stroke(Some(stroke(*c, *width, *dashed)));
                        surface.draw_path(&path);
                    }
                }
            }
            Item::Circle {
                cx,
                cy,
                r,
                fill: f,
                stroke: s,
            } => {
                let Some(path) = circle(*cx, *cy, *r) else {
                    return;
                };
                surface.set_fill(f.map(fill));
                surface.set_stroke(s.map(|(c, w)| stroke(c, w, false)));
                surface.draw_path(&path);
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
                    surface.set_fill(None);
                    surface.set_stroke(Some(stroke(*color, *width, false)));
                    surface.draw_path(&path);
                }
            }
            Item::Poly { points, fill: f } => {
                if let Some(path) = polygon(points.iter().copied()) {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*f)));
                    surface.draw_path(&path);
                }
            }
            Item::Shape {
                segs,
                fill: f,
                even_odd,
            } => {
                let mut pb = PathBuilder::new();
                for seg in segs {
                    match *seg {
                        Seg::Move(x, y) => pb.move_to(x, y),
                        Seg::Line(x, y) => pb.line_to(x, y),
                        Seg::Cubic(x1, y1, x2, y2, x, y) => pb.cubic_to(x1, y1, x2, y2, x, y),
                        Seg::Close => pb.close(),
                    }
                }
                if let Some(path) = pb.finish() {
                    let mut paint = fill(*f);
                    if *even_odd {
                        paint.rule = FillRule::EvenOdd;
                    }
                    surface.set_stroke(None);
                    surface.set_fill(Some(paint));
                    surface.draw_path(&path);
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
                let Some(clip) = polygon(poly.iter().map(|(px, py)| (px * size, py * size))) else {
                    return;
                };
                if let (Some(image), Some(sz)) = (
                    self.image(*champion, Tone::Color),
                    Size::from_wh(*size, *size),
                ) {
                    surface.push_transform(&Transform::from_translate(*x, *y));
                    surface.push_clip_path(&clip, &FillRule::NonZero);
                    surface
                        .push_transform(&Transform::from_translate(shift.0 * size, shift.1 * size));
                    surface.draw_image(image, sz);
                    surface.pop();
                    surface.pop();
                    surface.pop();
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
                    surface.set_fill(None);
                    surface.set_stroke(Some(stroke(*color, *width, false)));
                    surface.draw_path(&path);
                }
            }
            Item::Icon {
                champion,
                x,
                y,
                size,
                radius,
                tone,
                opacity: o,
            } => {
                let Some(clip) = rounded_rect(0.0, 0.0, *size, *size, *radius) else {
                    return;
                };
                surface.push_transform(&Transform::from_translate(*x, *y));
                match (self.image(*champion, *tone), Size::from_wh(*size, *size)) {
                    (Some(image), Some(sz)) => {
                        surface.push_clip_path(&clip, &FillRule::NonZero);
                        let faded = *o < 1.0;
                        if faded {
                            surface.push_opacity(opacity(*o));
                        }
                        surface.draw_image(image, sz);
                        if faded {
                            surface.pop();
                        }
                        surface.pop();
                    }
                    _ => {
                        surface.set_stroke(None);
                        surface.set_fill(Some(fill(scene::palette::BORDER)));
                        surface.draw_path(&clip);
                    }
                }
                surface.pop();
            }
            Item::Text {
                x,
                y,
                size,
                color,
                bold,
                anchor,
                text,
            } => {
                let width = scene::text_width(text, *size, *bold);
                let x = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - width / 2.0,
                    Anchor::End => x - width,
                };
                self.text(surface, x, *y, *size, *color, *bold, text);
            }
        }
    }
}
