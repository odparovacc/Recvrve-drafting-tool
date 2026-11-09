//! Role tags: which of the five roles a picked champion (or group, or empty pick) can play.
//!
//! A node's tags are a 5-bit mask; `0` means "not specified", i.e. any role. On one draft
//! line, each team's tagged picks must fit into distinct roles: there has to be at least one
//! way to give every tagged pick a different role it is tagged with. Untagged picks never
//! conflict, since a team has at most five picks and they can fill whatever is left.

/// The five roles, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Top,
    Jungle,
    Mid,
    Bot,
    Support,
}

pub const ALL: u8 = 0b1_1111;

impl Role {
    pub const ALL: [Role; 5] = [Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support];

    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Top => "Top",
            Role::Jungle => "Jungle",
            Role::Mid => "Mid",
            Role::Bot => "Bot",
            Role::Support => "Support",
        }
    }

    /// The name used in config files and XML.
    pub fn key(self) -> &'static str {
        match self {
            Role::Top => "top",
            Role::Jungle => "jungle",
            Role::Mid => "mid",
            Role::Bot => "bot",
            Role::Support => "support",
        }
    }

    pub fn from_key(key: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key() == key)
    }

    /// The role a protected selection group stands for (`Top`, `Jungle`, `Mid`, `ADC`,
    /// `Support`), for auto-tagging.
    pub fn from_selection_name(name: &str) -> Option<Role> {
        match name.trim() {
            "Top" => Some(Role::Top),
            "Jungle" => Some(Role::Jungle),
            "Mid" => Some(Role::Mid),
            "ADC" => Some(Role::Bot),
            "Support" => Some(Role::Support),
            _ => None,
        }
    }
}

/// The roles in `mask`, in display order.
pub fn roles_in(mask: u8) -> impl Iterator<Item = Role> {
    Role::ALL.into_iter().filter(move |r| mask & r.bit() != 0)
}

/// `top,jungle` for config files and XML.
pub fn keys(mask: u8) -> String {
    roles_in(mask).map(Role::key).collect::<Vec<_>>().join(",")
}

/// `Top / Jungle` for tooltips; "any role" for 0.
pub fn describe(mask: u8) -> String {
    if mask == 0 {
        return "any role".into();
    }
    roles_in(mask)
        .map(Role::label)
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Whether every tagged mask (non-zero) can get a different role it allows.
pub fn assignable(masks: &[u8]) -> bool {
    let tagged: Vec<u8> = masks.iter().copied().filter(|&m| m != 0).collect();
    fit(&tagged, 0)
}

fn fit(masks: &[u8], used: u8) -> bool {
    let Some((&first, rest)) = masks.split_first() else {
        return true;
    };
    roles_in(first & !used).any(|r| fit(rest, used | r.bit()))
}

/// For each pick (`0` = any role), the roles it can take in at least one way of giving every
/// pick a different role. A pick that can't be placed at all gets `0`.
pub fn candidates(masks: &[u8]) -> Vec<u8> {
    let full: Vec<u8> = masks
        .iter()
        .map(|&m| if m == 0 { ALL } else { m })
        .collect();
    let mut out = vec![0u8; full.len()];
    let mut chosen = vec![0u8; full.len()];
    collect(&full, 0, 0, &mut chosen, &mut out);
    out
}

fn collect(masks: &[u8], i: usize, used: u8, chosen: &mut [u8], out: &mut [u8]) {
    if i == masks.len() {
        for (o, c) in out.iter_mut().zip(chosen.iter()) {
            *o |= c;
        }
        return;
    }
    for r in roles_in(masks[i] & !used) {
        chosen[i] = r.bit();
        collect(masks, i + 1, used | r.bit(), chosen, out);
    }
}

/// The outline of part `k` of `n` when a square tile is shared between `n` portraits, as
/// fractions of the tile (x right, y down).
///
/// Two portraits split along the diagonal from bottom-left to top-right (into equal
/// halves, the first top-left). Three or more get evenly spaced pie wedges from the centre,
/// the first centred on the top edge and the rest clockwise, so each stays large enough to
/// recognise.
pub fn band_polygon(k: usize, n: usize) -> Vec<(f32, f32)> {
    match n {
        0 | 1 => vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        2 if k == 0 => vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)],
        2 => vec![(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        _ => wedge(k, n),
    }
}

/// Angle (radians, clockwise from the right since y points down) where wedge `k` of `n`
/// starts.
fn wedge_start(k: usize, n: usize) -> f32 {
    let step = std::f32::consts::TAU / n as f32;
    -std::f32::consts::FRAC_PI_2 - step / 2.0 + k as f32 * step
}

/// Where a ray from the tile's centre at `angle` leaves the tile.
fn edge_point(angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    let t = 0.5 / c.abs().max(s.abs());
    (0.5 + t * c, 0.5 + t * s)
}

fn wedge(k: usize, n: usize) -> Vec<(f32, f32)> {
    use std::f32::consts::{FRAC_PI_4, TAU};
    let step = TAU / n as f32;
    let a0 = wedge_start(k, n);
    let mut points = vec![(0.5, 0.5), edge_point(a0)];
    // Tile corners inside the wedge, in clockwise order.
    let mut corners: Vec<(f32, (f32, f32))> = [
        (-3.0 * FRAC_PI_4, (0.0, 0.0)),
        (-FRAC_PI_4, (1.0, 0.0)),
        (FRAC_PI_4, (1.0, 1.0)),
        (3.0 * FRAC_PI_4, (0.0, 1.0)),
    ]
    .into_iter()
    .map(|(angle, p)| ((angle - a0).rem_euclid(TAU), p))
    .filter(|&(rel, _)| rel > 1e-5 && rel < step - 1e-5)
    .collect();
    corners.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.extend(corners.into_iter().map(|(_, p)| p));
    points.push(edge_point(a0 + step));
    points
}

/// How far to move a portrait (as fractions of the tile) so its centre, usually the face,
/// sits in the middle of part `k` of `n` instead of being cut off by the split.
pub fn band_shift(k: usize, n: usize) -> (f32, f32) {
    if n <= 1 {
        return (0.0, 0.0);
    }
    let p = band_polygon(k, n);
    let (mut a, mut cx, mut cy) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..p.len() {
        let (x0, y0) = p[i];
        let (x1, y1) = p[(i + 1) % p.len()];
        let cross = x0 * y1 - x1 * y0;
        a += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    (cx / (3.0 * a) - 0.5, cy / (3.0 * a) - 0.5)
}

/// Separator lines between the parts of a shared tile, as (x1, y1, x2, y2) in fractions.
pub fn split_lines(n: usize) -> Vec<(f32, f32, f32, f32)> {
    match n {
        0 | 1 => Vec::new(),
        2 => vec![(0.0, 1.0, 1.0, 0.0)],
        _ => (0..n)
            .map(|k| {
                let (x, y) = edge_point(wedge_start(k, n));
                (0.5, 0.5, x, y)
            })
            .collect(),
    }
}

/// A CSS `clip-path` for [`band_polygon`].
pub fn band_css(k: usize, n: usize) -> String {
    let points: Vec<String> = band_polygon(k, n)
        .iter()
        .map(|(x, y)| format!("{:.2}% {:.2}%", x * 100.0, y * 100.0))
        .collect();
    format!("polygon({})", points.join(", "))
}

/// Most portraits one role slot is split between.
pub const MAX_SPLIT: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: u8 = 1;
    const JG: u8 = 2;
    const MID: u8 = 4;
    const BOT: u8 = 8;
    const SUP: u8 = 16;

    #[test]
    fn single_tags_must_differ() {
        assert!(assignable(&[TOP, JG]));
        assert!(!assignable(&[TOP, TOP]));
        assert!(
            assignable(&[TOP, 0, 0, 0, 0]),
            "untagged picks never conflict"
        );
    }

    #[test]
    fn flex_picks_need_some_permutation() {
        // Poppy top/jungle, Irelia top/mid: fine.
        assert!(assignable(&[TOP | JG, TOP | MID]));
        // Three picks over two roles: impossible.
        assert!(!assignable(&[TOP | JG, TOP | JG, TOP | JG]));
        assert!(!assignable(&[TOP | JG, TOP, JG]));
    }

    #[test]
    fn candidates_drop_impossible_roles() {
        // Vi jungle only, Poppy top/jungle: Poppy must be top.
        assert_eq!(candidates(&[JG, TOP | JG]), [JG, TOP]);
        // Irelia and Yasuo both top/mid: both could be either.
        assert_eq!(candidates(&[TOP | MID, TOP | MID]), [TOP | MID, TOP | MID]);
        // Four tagged picks force the untagged fifth to support.
        assert_eq!(candidates(&[TOP, JG, MID, BOT, 0])[4], SUP);
        // Nothing tagged: anything goes.
        assert_eq!(candidates(&[0, 0]), [ALL, ALL]);
    }

    #[test]
    fn shared_tiles_split_into_halves_or_wedges() {
        fn area(p: &[(f32, f32)]) -> f32 {
            let n = p.len();
            (0..n)
                .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
                .sum::<f32>()
                .abs()
                / 2.0
        }
        for n in 1..=MAX_SPLIT {
            let areas: Vec<f32> = (0..n).map(|k| area(&band_polygon(k, n))).collect();
            let total: f32 = areas.iter().sum();
            assert!((total - 1.0).abs() < 1e-4, "{n}: {total}");
            // Every part keeps a good share of the tile (a fifth of it is 0.2).
            for a in &areas {
                assert!(*a > 0.15, "{n}: {areas:?}");
            }
            assert_eq!(
                split_lines(n).len(),
                if n == 1 {
                    0
                } else if n == 2 {
                    1
                } else {
                    n
                }
            );
        }
        // Two portraits: halves along the bottom-left to top-right diagonal.
        assert_eq!(band_polygon(0, 2), [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]);
        assert!(band_css(0, 2).starts_with("polygon(0.00% 0.00%"));
        // Four: wedges centred on each edge, i.e. the four triangles of an X.
        for k in 0..4 {
            assert!((area(&band_polygon(k, 4)) - 0.25).abs() < 1e-4);
            assert_eq!(band_polygon(k, 4).len(), 3);
        }
        // Portraits move towards their own part: up for the top wedge, down for the bottom.
        assert!(band_shift(0, 4).1 < -0.2 && band_shift(0, 4).0.abs() < 1e-4);
        assert!(band_shift(2, 4).1 > 0.2);
        assert_eq!(band_shift(0, 1), (0.0, 0.0));
        // Three: the first wedge is centred on the top edge, so it holds both top corners.
        let first = band_polygon(0, 3);
        assert!(first.contains(&(0.0, 0.0)) && first.contains(&(1.0, 0.0)));
    }

    #[test]
    fn keys_and_labels() {
        assert_eq!(keys(TOP | SUP), "top,support");
        assert_eq!(describe(0), "any role");
        assert_eq!(Role::from_key("bot"), Some(Role::Bot));
        assert_eq!(Role::from_selection_name("ADC"), Some(Role::Bot));
    }
}
