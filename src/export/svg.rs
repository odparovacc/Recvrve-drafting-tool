//! SVG backend. Each champion portrait used is embedded once in `<defs>` and placed with
//! `<use>`, so a large tree stays small.

use std::collections::BTreeSet;
use std::fmt::Write;

use base64::Engine;

use super::icons::Icons;
use super::scene::{Anchor, Color, Item, SVG_FONT_FAMILY, Scene, Seg, Tone};

/// Escapes text for XML content and attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

fn paint(c: Color) -> String {
    if c.a == 255 {
        c.css()
    } else {
        format!("{}\" fill-opacity=\"{:.3}", c.css(), c.opacity())
    }
}

fn stroke_attrs(c: Color, width: f32) -> String {
    let mut s = format!("stroke=\"{}\" stroke-width=\"{}\"", c.css(), num(width));
    if c.a != 255 {
        let _ = write!(s, " stroke-opacity=\"{:.3}\"", c.opacity());
    }
    s
}

/// A number with at most two decimals and no trailing zeros.
fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Size in the icons' own pixels that the rounded-corner clip paths are drawn in.
const ICON_UNITS: f32 = 128.0;

pub fn render(scene: &Scene, icons: &Icons) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{SVG_FONT_FAMILY}">"#,
        w = num(scene.width),
        h = num(scene.height),
    );

    // The font, embedded so the file looks the same everywhere: only the weights used.
    let weights: BTreeSet<bool> = scene
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { bold, .. } => Some(*bold),
            _ => None,
        })
        .collect();
    if !weights.is_empty() {
        out.push_str("<style>\n");
        for bold in weights {
            let bytes = if bold {
                super::scene::FONT_BOLD_BYTES
            } else {
                super::scene::FONT_REGULAR_BYTES
            };
            let _ = writeln!(
                out,
                "@font-face {{ font-family: 'Lexend Deca'; font-weight: {}; src: url(data:font/ttf;base64,{}) format('truetype'); }}",
                if bold { "bold" } else { "normal" },
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
        }
        out.push_str("</style>\n");
    }

    // Definitions: each portrait once, a grayscale filter and one clip path per corner
    // radius (relative to the icon size).
    let used: BTreeSet<u8> = scene.champions().map(|(c, _)| c).collect();
    let mut clips: BTreeSet<u32> = BTreeSet::new();
    for item in &scene.items {
        if let Item::Icon { size, radius, .. } = item {
            clips.insert(clip_key(*size, *radius));
        }
    }
    out.push_str("<defs>\n");
    let tones: BTreeSet<&'static str> = scene
        .champions()
        .filter(|(_, t)| *t != Tone::Color)
        .map(|(_, t)| t.id())
        .collect();
    for tone in Tone::MONO {
        if tones.contains(tone.id()) {
            out.push_str(&tone_filter(tone));
        }
    }
    for key in &clips {
        let _ = writeln!(
            out,
            "<clipPath id=\"r{key}\"><rect width=\"{u}\" height=\"{u}\" rx=\"{rx}\"/></clipPath>",
            u = num(ICON_UNITS),
            rx = num(*key as f32 / 1000.0 * ICON_UNITS),
        );
    }
    for &c in &used {
        let Some(icon) = icons.get(c) else { continue };
        let data = base64::engine::general_purpose::STANDARD.encode(&icon.png);
        let (w, h) = (icon.color.width(), icon.color.height());
        // Scaled to ICON_UNITS so every <use> shares one coordinate system.
        let _ = writeln!(
            out,
            "<image id=\"c{c}\" width=\"{u}\" height=\"{u}\" preserveAspectRatio=\"xMidYMid slice\" data-px=\"{w}x{h}\" href=\"data:image/png;base64,{data}\"/>",
            u = num(ICON_UNITS),
        );
    }
    out.push_str("</defs>\n");
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");

    for (i, item) in scene.items.iter().enumerate() {
        write_item(&mut out, item, icons, i);
    }
    out.push_str("</svg>\n");
    out
}

/// A filter laying a portrait's brightness on the tone's color ramp (see [`Tone::ramp`]).
fn tone_filter(tone: Tone) -> String {
    let (dark, light) = tone.ramp().expect("mono tone");
    let mut rows = String::new();
    for c in 0..3 {
        let k = (f32::from(light[c]) - f32::from(dark[c])) / 255.0;
        let _ = write!(
            rows,
            "{:.4} {:.4} {:.4} 0 {:.4} ",
            k * 0.2126,
            k * 0.7152,
            k * 0.0722,
            f32::from(dark[c]) / 255.0
        );
    }
    rows.push_str("0 0 0 1 0");
    format!(
        "<filter id=\"{}\" color-interpolation-filters=\"sRGB\"><feColorMatrix type=\"matrix\" values=\"{rows}\"/></filter>\n",
        tone.id()
    )
}

/// Corner radius as thousandths of the icon size, the key of its clip path.
fn clip_key(size: f32, radius: f32) -> u32 {
    if size <= 0.0 {
        return 0;
    }
    (radius / size * 1000.0).round().clamp(0.0, 500.0) as u32
}

fn write_item(out: &mut String, item: &Item, icons: &Icons, index: usize) {
    match item {
        Item::Poly { points, fill } => {
            let pts: Vec<String> = points
                .iter()
                .map(|(x, y)| format!("{},{}", num(*x), num(*y)))
                .collect();
            let _ = writeln!(
                out,
                "<polygon points=\"{}\" fill=\"{}\"/>",
                pts.join(" "),
                paint(*fill)
            );
        }
        Item::Shape {
            segs,
            fill,
            even_odd,
        } => {
            let mut d = String::new();
            for seg in segs {
                match *seg {
                    Seg::Move(x, y) => {
                        let _ = write!(d, "M{} {}", num(x), num(y));
                    }
                    Seg::Line(x, y) => {
                        let _ = write!(d, "L{} {}", num(x), num(y));
                    }
                    Seg::Cubic(x1, y1, x2, y2, x, y) => {
                        let _ = write!(
                            d,
                            "C{} {} {} {} {} {}",
                            num(x1),
                            num(y1),
                            num(x2),
                            num(y2),
                            num(x),
                            num(y)
                        );
                    }
                    Seg::Close => d.push('Z'),
                }
            }
            let rule = if *even_odd {
                " fill-rule=\"evenodd\""
            } else {
                ""
            };
            let _ = writeln!(out, "<path d=\"{d}\" fill=\"{}\"{rule}/>", paint(*fill));
        }
        Item::IconBand {
            champion,
            x,
            y,
            size,
            poly,
            shift,
        } => {
            // The clip polygon is in the portrait's own units, like the corner clips.
            let pts: Vec<String> = poly
                .iter()
                .map(|(px, py)| format!("{},{}", num(px * ICON_UNITS), num(py * ICON_UNITS)))
                .collect();
            let _ = writeln!(
                out,
                "<clipPath id=\"band{index}\"><polygon points=\"{}\"/></clipPath>",
                pts.join(" ")
            );
            if icons.get(*champion).is_some() {
                let _ = writeln!(
                    out,
                    "<g transform=\"translate({} {}) scale({})\" clip-path=\"url(#band{index})\"><use href=\"#c{champion}\" xlink:href=\"#c{champion}\" transform=\"translate({} {})\"/></g>",
                    num(*x),
                    num(*y),
                    size / ICON_UNITS,
                    num(shift.0 * ICON_UNITS),
                    num(shift.1 * ICON_UNITS)
                );
            }
        }
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
            // Strokes sit inside the box, like CSS borders.
            let inset = stroke.map_or(0.0, |(_, sw)| sw / 2.0);
            if let Some(fill) = fill {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\"/>",
                    num(*x),
                    num(*y),
                    num(*w),
                    num(*h),
                    num(*radius),
                    paint(*fill)
                );
            }
            if let Some((color, width)) = stroke {
                let dash = if *dashed {
                    format!(
                        " stroke-dasharray=\"{} {}\"",
                        num(width * 3.0),
                        num(width * 2.0)
                    )
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"none\" {}{dash}/>",
                    num(x + inset),
                    num(y + inset),
                    num(w - 2.0 * inset),
                    num(h - 2.0 * inset),
                    num((radius - inset).max(0.0)),
                    stroke_attrs(*color, *width)
                );
            }
        }
        Item::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
        } => {
            let fill = fill.map_or("none".to_string(), paint);
            let stroke = stroke.map_or(String::new(), |(c, w)| stroke_attrs(c, w));
            let _ = writeln!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\" {stroke}/>",
                num(*cx),
                num(*cy),
                num(*r)
            );
        }
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => {
            let _ = writeln!(
                out,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {}/>",
                num(*x1),
                num(*y1),
                num(*x2),
                num(*y2),
                stroke_attrs(*color, *width)
            );
        }
        Item::Curve {
            from,
            c1,
            c2,
            to,
            color,
            width,
        } => {
            let _ = writeln!(
                out,
                "<path d=\"M{} {}C{} {} {} {} {} {}\" fill=\"none\" {}/>",
                num(from.0),
                num(from.1),
                num(c1.0),
                num(c1.1),
                num(c2.0),
                num(c2.1),
                num(to.0),
                num(to.1),
                stroke_attrs(*color, *width)
            );
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
            if icons.get(*champion).is_none() {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{s}\" height=\"{s}\" rx=\"{}\" fill=\"#d5d9e3\"/>",
                    num(*x),
                    num(*y),
                    num(*radius),
                    s = num(*size)
                );
                return;
            }
            let k = size / ICON_UNITS;
            let mut extra = String::new();
            if *tone != Tone::Color {
                let _ = write!(extra, " filter=\"url(#{})\"", tone.id());
            }
            if *opacity < 1.0 {
                let _ = write!(extra, " opacity=\"{:.2}\"", opacity);
            }
            let _ = writeln!(
                out,
                "<use href=\"#c{champion}\" xlink:href=\"#c{champion}\" transform=\"translate({} {}) scale({})\" clip-path=\"url(#r{})\"{extra}/>",
                num(*x),
                num(*y),
                k,
                clip_key(*size, *radius),
            );
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
            let anchor = match anchor {
                Anchor::Start => "",
                Anchor::Middle => " text-anchor=\"middle\"",
                Anchor::End => " text-anchor=\"end\"",
            };
            let weight = if *bold { " font-weight=\"bold\"" } else { "" };
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\"{weight}{anchor} xml:space=\"preserve\">{}</text>",
                num(*x),
                num(*y),
                num(*size),
                paint(*color),
                escape(text)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::icons;
    use crate::export::scene::palette;

    #[test]
    fn embeds_each_icon_once() {
        let mut scene = Scene::new(100.0, 50.0);
        for i in 0..5 {
            scene.items.push(Item::Icon {
                champion: 16,
                x: i as f32 * 20.0,
                y: 0.0,
                size: 18.0,
                radius: 3.0,
                tone: if i % 2 == 0 { Tone::Gray } else { Tone::Color },
                opacity: 1.0,
            });
        }
        scene.text(
            0.0,
            40.0,
            10.0,
            palette::TEXT,
            false,
            Anchor::Start,
            "A & <B>",
        );
        let svg = render(&scene, icons::for_tests());
        assert_eq!(svg.matches("<image ").count(), 1);
        assert_eq!(svg.matches("<use ").count(), 5);
        assert_eq!(svg.matches("url(#gray)").count(), 3);
        assert_eq!(svg.matches("<filter ").count(), 1);
        assert!(svg.contains("A &amp; &lt;B&gt;"));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(1.234), "1.23");
        assert_eq!(num(-0.001), "0");
    }
}
