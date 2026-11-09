//! A small display list that every export draws into.
//!
//! Layout code builds a [`Scene`] once; the raster (PNG/WebP/JPEG), SVG and PDF backends
//! then turn the same scene into their format. Coordinates are logical pixels with the
//! origin at the top left; each backend applies its own scale.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef};

pub use crate::role_icon::Seg;

pub static FONT_REGULAR_BYTES: &[u8] = include_bytes!("../../assets/fonts/LexendDeca-Regular.ttf");
pub static FONT_BOLD_BYTES: &[u8] = include_bytes!("../../assets/fonts/LexendDeca-Bold.ttf");
/// For the symbols Lexend Deca doesn't have (★, ●, arrows …).
pub static FALLBACK_REGULAR_BYTES: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");
pub static FALLBACK_BOLD_BYTES: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf");

/// Font family names written into SVG files. Lexend Deca is embedded in the file; the
/// others cover symbols it lacks.
pub const SVG_FONT_FAMILY: &str = "'Lexend Deca', 'DejaVu Sans', Verdana, Arial, sans-serif";

/// A bundled font, parsed once: Lexend Deca, or (`fallback`) DejaVu Sans.
pub fn face(fallback: bool, bold: bool) -> &'static FontRef<'static> {
    static FACES: [OnceLock<FontRef<'static>>; 4] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let (i, bytes) = match (fallback, bold) {
        (false, false) => (0, FONT_REGULAR_BYTES),
        (false, true) => (1, FONT_BOLD_BYTES),
        (true, false) => (2, FALLBACK_REGULAR_BYTES),
        (true, true) => (3, FALLBACK_BOLD_BYTES),
    };
    FACES[i].get_or_init(|| FontRef::try_from_slice(bytes).expect("bundled font parses"))
}

/// The main bundled font (Lexend Deca).
pub fn font(bold: bool) -> &'static FontRef<'static> {
    face(false, bold)
}

/// `text` split into runs that one font draws: `true` marks runs in the fallback font,
/// for characters Lexend Deca has no glyph for.
pub fn runs(text: &str, bold: bool) -> Vec<(bool, &str)> {
    let main = font(bold);
    let mut out: Vec<(bool, &str)> = Vec::new();
    let mut start = 0;
    let mut current = None;
    for (i, c) in text.char_indices() {
        let fallback = main.glyph_id(c).0 == 0 && !c.is_whitespace();
        match current {
            Some(f) if f == fallback => {}
            Some(f) => {
                out.push((f, &text[start..i]));
                start = i;
                current = Some(fallback);
            }
            None => current = Some(fallback),
        }
    }
    if let Some(f) = current {
        out.push((f, &text[start..]));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Parses `#rrggbb`. Panics on anything else, since every color is a constant.
    pub fn hex(text: &str) -> Color {
        let digits = text.strip_prefix('#').expect("color starts with #");
        let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex color");
        Color::rgb(channel(0), channel(2), channel(4))
    }

    pub fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }

    /// This color laid over white at `amount` (0-1) strength, as an opaque color.
    pub fn tint(self, amount: f32) -> Color {
        let mix = |c: u8| (255.0 - (255.0 - f32::from(c)) * amount).round() as u8;
        Color::rgb(mix(self.r), mix(self.g), mix(self.b))
    }

    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn opacity(self) -> f32 {
        f32::from(self.a) / 255.0
    }
}

/// Light-theme colors, with every panel background replaced by white to save ink.
pub mod palette {
    use super::Color;

    pub const TEXT: Color = Color::rgb(0x1b, 0x1e, 0x28);
    pub const MUTED: Color = Color::rgb(0x62, 0x6a, 0x80);
    pub const BORDER: Color = Color::rgb(0xd5, 0xd9, 0xe3);
    /// Tree edges: a little darker than panel borders so they survive printing.
    pub const EDGE: Color = Color::rgb(0xb4, 0xbb, 0xca);
    pub const BLUE: Color = Color::rgb(0x25, 0x63, 0xeb);
    pub const RED: Color = Color::rgb(0xdc, 0x26, 0x26);
    pub const GOLD: Color = Color::rgb(0xb7, 0x86, 0x0f);
    pub const BAN_SIGN: Color = Color::rgb(0xd4, 0x00, 0x00);
    /// Branches that hard fearless makes unreachable.
    pub const FEARLESS: Color = Color::rgb(0xd9, 0x46, 0xef);
    /// Conflicting branches (a value edit higher up broke the line).
    pub const CONFLICT: Color = Color::rgb(0x16, 0xa3, 0x4a);
}

/// How a champion portrait is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Color,
    /// Bans.
    Gray,
    /// Unreachable because of hard fearless.
    Purple,
    /// Unreachable because of soft fearless, for the blue or red side.
    Blue,
    Red,
}

impl Tone {
    pub const MONO: [Tone; 4] = [Tone::Gray, Tone::Purple, Tone::Blue, Tone::Red];

    pub fn from_fearless(tone: crate::fearless::Tone) -> Tone {
        match tone {
            crate::fearless::Tone::Both => Tone::Purple,
            crate::fearless::Tone::Blue => Tone::Blue,
            crate::fearless::Tone::Red => Tone::Red,
        }
    }

    /// The colors black and white map to: the portrait's brightness is laid on a ramp
    /// between them. `None` keeps the original colors.
    pub fn ramp(self) -> Option<([u8; 3], [u8; 3])> {
        match self {
            Tone::Color => None,
            Tone::Gray => Some(([0, 0, 0], [255, 255, 255])),
            Tone::Purple => Some(([42, 8, 64], [252, 214, 255])),
            Tone::Blue => Some(([8, 26, 72], [212, 228, 255])),
            Tone::Red => Some(([72, 8, 8], [255, 214, 214])),
        }
    }

    /// The line color for branches in this tone.
    pub fn accent(self) -> Color {
        match self {
            Tone::Blue => Color::rgb(0x3b, 0x82, 0xf6),
            Tone::Red => Color::rgb(0xef, 0x44, 0x44),
            _ => palette::FEARLESS,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Tone::Color => "color",
            Tone::Gray => "gray",
            Tone::Purple => "purple",
            Tone::Blue => "blue",
            Tone::Red => "red",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
        dashed: bool,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: Color,
        width: f32,
    },
    /// A cubic Bézier curve from `from` to `to`.
    Curve {
        from: (f32, f32),
        c1: (f32, f32),
        c2: (f32, f32),
        to: (f32, f32),
        color: Color,
        width: f32,
    },
    /// A champion portrait, square, with rounded corners of `radius`.
    Icon {
        champion: u8,
        x: f32,
        y: f32,
        size: f32,
        radius: f32,
        tone: Tone,
        opacity: f32,
    },
    /// A champion portrait in the square at (`x`, `y`) clipped to `poly`, given in
    /// fractions of the square (for split role slots).
    IconBand {
        champion: u8,
        x: f32,
        y: f32,
        size: f32,
        poly: Vec<(f32, f32)>,
        /// Moves the portrait (not the clip), in fractions of the square.
        shift: (f32, f32),
    },
    /// A filled polygon in scene coordinates.
    Poly {
        points: Vec<(f32, f32)>,
        fill: Color,
    },
    /// A filled outline in scene coordinates (role icons).
    Shape {
        segs: Vec<Seg>,
        fill: Color,
        even_odd: bool,
    },
    /// One line of text; `y` is the baseline.
    Text {
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        anchor: Anchor,
        text: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

impl Scene {
    pub fn new(width: f32, height: f32) -> Self {
        Scene {
            width,
            height,
            items: Vec::new(),
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, fill: Color) {
        self.items.push(Item::Rect {
            x,
            y,
            w,
            h,
            radius,
            fill: Some(fill),
            stroke: None,
            dashed: false,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn outline(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: Color,
        width: f32,
        dashed: bool,
    ) {
        self.items.push(Item::Rect {
            x,
            y,
            w,
            h,
            radius,
            fill: None,
            stroke: Some((color, width)),
            dashed,
        });
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
        self.items.push(Item::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        anchor: Anchor,
        text: impl Into<String>,
    ) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.items.push(Item::Text {
            x,
            y,
            size,
            color,
            bold,
            anchor,
            text,
        });
    }

    /// The red "prohibited" sign over a banned value in the square at (`x`, `y`).
    pub fn ban_sign(&mut self, x: f32, y: f32, size: f32) {
        // Same proportions as the app's CSS sign: a 100-unit box scaled to 86%.
        let unit = size * 0.86 / 100.0;
        let (cx, cy) = (x + size / 2.0, y + size / 2.0);
        let color = palette::BAN_SIGN.with_alpha(230);
        self.items.push(Item::Circle {
            cx,
            cy,
            r: 40.0 * unit,
            fill: None,
            stroke: Some((color, 11.0 * unit)),
        });
        let d = 26.0 * unit;
        self.line(cx - d, cy - d, cx + d, cy + d, color, 11.0 * unit);
    }

    /// Appends `other`'s items moved by (`dx`, `dy`).
    pub fn append(&mut self, other: &Scene, dx: f32, dy: f32) {
        let mv = |(x, y): (f32, f32)| (x + dx, y + dy);
        for item in &other.items {
            let mut item = item.clone();
            match &mut item {
                Item::Rect { x, y, .. }
                | Item::Icon { x, y, .. }
                | Item::IconBand { x, y, .. }
                | Item::Text { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Item::Circle { cx, cy, .. } => {
                    *cx += dx;
                    *cy += dy;
                }
                Item::Line { x1, y1, x2, y2, .. } => {
                    *x1 += dx;
                    *y1 += dy;
                    *x2 += dx;
                    *y2 += dy;
                }
                Item::Poly { points, .. } => {
                    for p in points.iter_mut() {
                        *p = mv(*p);
                    }
                }
                Item::Shape { segs, .. } => {
                    for seg in segs.iter_mut() {
                        *seg = seg.moved(dx, dy);
                    }
                }
                Item::Curve {
                    from, c1, c2, to, ..
                } => {
                    *from = mv(*from);
                    *c1 = mv(*c1);
                    *c2 = mv(*c2);
                    *to = mv(*to);
                }
            }
            self.items.push(item);
        }
    }

    /// Champion values drawn in this scene.
    pub fn champions(&self) -> impl Iterator<Item = (u8, Tone)> + '_ {
        self.items.iter().filter_map(|item| match item {
            Item::Icon { champion, tone, .. } => Some((*champion, *tone)),
            Item::IconBand { champion, .. } => Some((*champion, Tone::Color)),
            _ => None,
        })
    }
}

/// Width of `text` at `size` in logical pixels, using the bundled font's advances and
/// kerning. All backends position text with these metrics.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    runs(text, bold)
        .into_iter()
        .map(|(fallback, run)| run_width(run, size, fallback, bold))
        .sum()
}

/// Width of a run drawn in one font (see [`runs`]).
pub fn run_width(run: &str, size: f32, fallback: bool, bold: bool) -> f32 {
    let font = face(fallback, bold);
    let scale = size / font.units_per_em().unwrap_or(2048.0);
    let mut width = 0.0;
    let mut prev = None;
    for c in run.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            width += font.kern_unscaled(p, id);
        }
        width += font.h_advance_unscaled(id);
        prev = Some(id);
    }
    width * scale
}

/// `text` shortened with an ellipsis so it fits in `max_width`.
pub fn fit_text(text: &str, size: f32, bold: bool, max_width: f32) -> String {
    if text_width(text, size, bold) <= max_width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if text_width(&candidate, size, bold) <= max_width {
            return candidate;
        }
    }
    String::new()
}

/// Splits `text` into lines no wider than `max_width`, breaking at spaces (and inside
/// words only when a single word is too long).
pub fn wrap_text(text: &str, size: f32, bold: bool, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if text_width(&candidate, size, bold) <= max_width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if text_width(word, size, bold) <= max_width {
            line = word.to_string();
        } else {
            // Hard-break an over-long word.
            for c in word.chars() {
                let mut next = line.clone();
                next.push(c);
                if !line.is_empty() && text_width(&next, size, bold) > max_width {
                    lines.push(std::mem::take(&mut line));
                    line.push(c);
                } else {
                    line = next;
                }
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_fits_text() {
        let w = text_width("Alistar", 12.0, false);
        assert!(w > 30.0 && w < 60.0, "{w}");
        assert!(text_width("Alistar", 12.0, true) > w);
        assert_eq!(fit_text("Alistar", 12.0, false, 200.0), "Alistar");
        let short = fit_text("Aurelion Sol and friends", 12.0, false, 60.0);
        assert!(short.ends_with('…'));
        assert!(text_width(&short, 12.0, false) <= 60.0);
    }

    #[test]
    fn wraps_at_spaces() {
        let lines = wrap_text("one two three four five six", 12.0, false, 70.0);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(text_width(l, 12.0, false) <= 70.0, "{l}");
        }
        assert_eq!(lines.join(" "), "one two three four five six");
    }

    #[test]
    fn colors() {
        assert_eq!(Color::hex("#10b981"), Color::rgb(0x10, 0xb9, 0x81));
        assert_eq!(Color::rgb(0, 0, 0).tint(0.0), Color::WHITE);
        assert_eq!(Color::hex("#2563eb").css(), "#2563eb");
    }
}
