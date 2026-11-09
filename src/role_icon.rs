//! The official Riot position icons, from `assets/roles/*.svg`.
//!
//! Each icon has one highlighted layer (the lane) and some faint layers (the rest of the
//! map). The app draws them in the current text color, with the faint layers at
//! [`DIM_OPACITY`], and the exports trace the same outlines as vector shapes.
//!
//! The files are embedded at build time and parsed once. Only what these icons use is
//! understood: `viewBox`, and `<path>` elements with `d`, `fill-opacity` and `fill-rule`.

use std::sync::OnceLock;

use crate::roles::Role;

/// Opacity of the faint layers relative to the highlighted one.
pub const DIM_OPACITY: f32 = 0.3;

/// One step of an outline, in absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

impl Seg {
    /// The segment scaled by `k`, then moved by (`dx`, `dy`).
    pub fn placed(self, k: f32, dx: f32, dy: f32) -> Seg {
        let p = |x: f32, y: f32| (x * k + dx, y * k + dy);
        match self {
            Seg::Move(x, y) => {
                let (x, y) = p(x, y);
                Seg::Move(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = p(x, y);
                Seg::Line(x, y)
            }
            Seg::Cubic(x1, y1, x2, y2, x, y) => {
                let ((x1, y1), (x2, y2), (x, y)) = (p(x1, y1), p(x2, y2), p(x, y));
                Seg::Cubic(x1, y1, x2, y2, x, y)
            }
            Seg::Close => Seg::Close,
        }
    }

    pub fn moved(self, dx: f32, dy: f32) -> Seg {
        self.placed(1.0, dx, dy)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// The original path data, for the app's inline SVG.
    pub d: String,
    pub segs: Vec<Seg>,
    pub dim: bool,
    pub even_odd: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub width: f32,
    pub height: f32,
    pub layers: Vec<Layer>,
}

impl Icon {
    /// `viewBox` value for the app's inline SVG.
    pub fn view_box(&self) -> String {
        format!("0 0 {} {}", self.width, self.height)
    }

    /// Scale and offset that fit the icon, centred, into a `size` square at (`x`, `y`).
    pub fn fit(&self, x: f32, y: f32, size: f32) -> (f32, f32, f32) {
        let k = size / self.width.max(self.height);
        (
            k,
            x + (size - self.width * k) / 2.0,
            y + (size - self.height * k) / 2.0,
        )
    }
}

const SOURCES: [&str; 5] = [
    include_str!("../assets/roles/top.svg"),
    include_str!("../assets/roles/jungle.svg"),
    include_str!("../assets/roles/mid.svg"),
    include_str!("../assets/roles/bot.svg"),
    include_str!("../assets/roles/support.svg"),
];

/// The icon for `role`.
pub fn icon(role: Role) -> &'static Icon {
    static ICONS: OnceLock<Vec<Icon>> = OnceLock::new();
    &ICONS.get_or_init(|| SOURCES.iter().map(|s| parse_svg(s)).collect())[role as usize]
}

/// The value of `name="…"` inside one tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = &rest[i + name.len()..];
        if matches!(before, Some(c) if c.is_whitespace())
            && let Some(value) = after.strip_prefix("=\"")
        {
            return value.find('"').map(|end| &value[..end]);
        }
        rest = after;
    }
    None
}

fn parse_svg(source: &str) -> Icon {
    let (mut width, mut height) = (16.0, 16.0);
    let mut layers = Vec::new();
    for tag in source.split('<').filter_map(|t| t.split('>').next()) {
        if tag.starts_with("svg") {
            if let Some(vb) = attr(tag, "viewBox") {
                let v: Vec<f32> = numbers(vb);
                if let [_, _, w, h] = v[..] {
                    (width, height) = (w, h);
                }
            }
        } else if tag.starts_with("path")
            && let Some(d) = attr(tag, "d")
        {
            let dim = attr(tag, "fill-opacity")
                .and_then(|o| o.parse::<f32>().ok())
                .is_some_and(|o| o < 1.0);
            layers.push(Layer {
                d: d.to_string(),
                segs: parse_path(d),
                dim,
                even_odd: attr(tag, "fill-rule") == Some("evenodd"),
            });
        }
    }
    // Faint layers first, so the highlighted one sits on top.
    layers.sort_by_key(|l| !l.dim);
    Icon {
        width,
        height,
        layers,
    }
}

fn numbers(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut tokens = Tokens::new(s);
    while let Some(v) = tokens.number() {
        out.push(v);
    }
    out
}

/// Reads path data: commands and numbers (`1.5-2`, `.5.5` and `1e-3` included).
struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Tokens<'a> {
    fn new(s: &'a str) -> Self {
        Tokens {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn skip(&mut self) {
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
        {
            self.i += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        c.is_ascii_alphabetic().then(|| {
            self.i += 1;
            c
        })
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        let mut i = self.i;
        if matches!(self.s.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut dot = false;
        while let Some(&c) = self.s.get(i) {
            if c.is_ascii_digit() {
                i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                i += 1;
            } else {
                break;
            }
        }
        if matches!(self.s.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(self.s.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if self.s.get(j).is_some_and(u8::is_ascii_digit) {
                i = j;
                while self.s.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
            }
        }
        let v = std::str::from_utf8(&self.s[start..i]).ok()?.parse().ok()?;
        self.i = i;
        Some(v)
    }
}

/// Path data to absolute segments. Handles M, L, H, V, C and Z, upper- and lowercase.
fn parse_path(d: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut t = Tokens::new(d);
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut cmd = b'M';
    loop {
        if let Some(c) = t.command() {
            cmd = c;
            if c.eq_ignore_ascii_case(&b'z') {
                segs.push(Seg::Close);
                (cx, cy) = (sx, sy);
                continue;
            }
        } else if t.i >= t.s.len() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        let parsed = match cmd.to_ascii_uppercase() {
            b'M' | b'L' => t.number().zip(t.number()).map(|(x, y)| {
                (cx, cy) = (ox + x, oy + y);
                if cmd.eq_ignore_ascii_case(&b'm') {
                    (sx, sy) = (cx, cy);
                    // Further pairs after a move are lines.
                    cmd = if rel { b'l' } else { b'L' };
                    Seg::Move(cx, cy)
                } else {
                    Seg::Line(cx, cy)
                }
            }),
            b'H' => t.number().map(|x| {
                cx = ox + x;
                Seg::Line(cx, cy)
            }),
            b'V' => t.number().map(|y| {
                cy = oy + y;
                Seg::Line(cx, cy)
            }),
            b'C' => {
                let v: Vec<f32> = (0..6).map_while(|_| t.number()).collect();
                (v.len() == 6).then(|| {
                    (cx, cy) = (ox + v[4], oy + v[5]);
                    Seg::Cubic(ox + v[0], oy + v[1], ox + v[2], oy + v[3], cx, cy)
                })
            }
            _ => None,
        };
        match parsed {
            Some(seg) => segs.push(seg),
            None => break,
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_path("M1 2H5V6L1.5-2C1 1 2 2 3 3Z"),
            vec![
                Seg::Move(1.0, 2.0),
                Seg::Line(5.0, 2.0),
                Seg::Line(5.0, 6.0),
                Seg::Line(1.5, -2.0),
                Seg::Cubic(1.0, 1.0, 2.0, 2.0, 3.0, 3.0),
                Seg::Close,
            ]
        );
        assert_eq!(
            parse_path("m1 1 2 0h1z"),
            vec![
                Seg::Move(1.0, 1.0),
                Seg::Line(3.0, 1.0),
                Seg::Line(4.0, 1.0),
                Seg::Close,
            ]
        );
        assert_eq!(numbers("7.26922e-06 2.5"), vec![7.26922e-06, 2.5]);
    }

    #[test]
    fn loads_all_icons() {
        for role in Role::ALL {
            let icon = icon(role);
            assert!(icon.width > 100.0 && icon.height > 100.0, "{role:?}");
            let lit: Vec<_> = icon.layers.iter().filter(|l| !l.dim).collect();
            assert!(!lit.is_empty(), "{role:?} has a highlighted layer");
            assert!(!icon.layers.last().unwrap().dim, "highlight drawn last");
            for layer in &icon.layers {
                assert!(matches!(layer.segs.first(), Some(Seg::Move(..))));
                assert_eq!(layer.segs.last(), Some(&Seg::Close), "{role:?}");
            }
        }
        // Lane icons carry the map; jungle and support are a single highlighted shape.
        assert!(icon(Role::Top).layers.iter().any(|l| l.dim));
        assert!(icon(Role::Mid).layers.iter().any(|l| l.even_odd));
        assert!(icon(Role::Jungle).layers.iter().all(|l| !l.dim));
        // Every point lies inside the view box.
        for role in Role::ALL {
            let icon = icon(role);
            for layer in &icon.layers {
                for seg in &layer.segs {
                    if let Seg::Move(x, y) | Seg::Line(x, y) = *seg {
                        assert!((-0.5..=icon.width + 0.5).contains(&x), "{role:?} x {x}");
                        assert!((-0.5..=icon.height + 0.5).contains(&y), "{role:?} y {y}");
                    }
                }
            }
        }
    }
}
