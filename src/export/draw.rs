//! Drawing pieces shared by the tree image, the draft state cards and the PDF: value tiles
//! and the group legend.

use crate::champions;
use crate::fearless::{HALF, Mode};
use crate::groups::Group;
use crate::tree::{EMPTY, NO_BAN};

use super::model::Snapshot;
use super::scene::{Anchor, Color, Item, Scene, Tone, fit_text, palette, wrap_text};

/// A grayscale version of `c`, for banned groups.
pub fn gray(c: Color) -> Color {
    let l =
        (0.2126 * f32::from(c.r) + 0.7152 * f32::from(c.g) + 0.0722 * f32::from(c.b)).round() as u8;
    Color {
        r: l,
        g: l,
        b: l,
        a: c.a,
    }
}

/// A group's square tile: its label on a tint of its color, inside a border of that color.
#[allow(clippy::too_many_arguments)]
pub fn group_tile(
    scene: &mut Scene,
    color: Color,
    label: &str,
    x: f32,
    y: f32,
    size: f32,
    radius: f32,
    border: f32,
    banned: bool,
) {
    let color = if banned { gray(color) } else { color };
    scene.rect(x, y, size, size, radius, color.tint(0.22));
    scene.outline(x, y, size, size, radius, color, border, false);
    let font = (size * 0.32).clamp(6.0, 16.0);
    let label = fit_text(label, font, true, size - 2.0 * border - 2.0);
    scene.text(
        x + size / 2.0,
        y + size / 2.0 + font * 0.36,
        font,
        palette::TEXT,
        true,
        Anchor::Middle,
        label,
    );
}

/// Draws what a node value looks like in the box at (`x`, `y`): a champion portrait, a
/// group tile, or the "nothing chosen" sign. `ban` grays champions and groups out and adds
/// the red ban sign. `pruned` draws an unreachable node in its fearless tone instead.
#[allow(clippy::too_many_arguments)]
pub fn value_tile(
    scene: &mut Scene,
    snap: &Snapshot,
    value: u8,
    x: f32,
    y: f32,
    size: f32,
    radius: f32,
    ban: bool,
    gray_opacity: f32,
    pruned: Option<Tone>,
) {
    if champions::by_value(value).is_some() {
        let (tone, opacity) = match (pruned, ban) {
            (Some(t), _) => (t, 1.0),
            (None, true) => (Tone::Gray, gray_opacity),
            (None, false) => (Tone::Color, 1.0),
        };
        scene.items.push(Item::Icon {
            champion: value,
            x,
            y,
            size,
            radius,
            tone,
            opacity,
        });
        if ban {
            scene.ban_sign(x, y, size);
        }
    } else if let Some(g) = snap.groups.get(value) {
        let border = (size * 0.06).clamp(1.0, 2.0);
        group_tile(
            scene,
            pruned.map_or(snap.group_color(value), Tone::accent),
            &g.label,
            x,
            y,
            size,
            radius,
            border,
            ban && pruned.is_none(),
        );
        if ban {
            scene.ban_sign(x, y, size);
        }
    } else {
        // EMPTY, NO_BAN, or a group value that isn't defined.
        let sign = if snap.is_group(value) { "?" } else { "∅" };
        let font = (size * 0.42).clamp(7.0, 22.0);
        scene.text(
            x + size / 2.0,
            y + size / 2.0 + font * 0.36,
            font,
            palette::MUTED,
            false,
            Anchor::Middle,
            sign,
        );
    }
}

/// Display name of a value: champion, group name, "No ban" or "Empty".
pub fn value_label(snap: &Snapshot, value: u8) -> String {
    if let Some(c) = champions::by_value(value) {
        return c.name.to_string();
    }
    if let Some(g) = snap.groups.get(value) {
        return g.name.clone();
    }
    match value {
        NO_BAN => "No ban".into(),
        EMPTY => "Empty".into(),
        v => format!("Unknown value {v}"),
    }
}

/// Members of a group as text: "all champions" for groups covering the whole roster.
fn members_text(group: &Group) -> String {
    let names = group.member_names();
    if names.len() == champions::CHAMPIONS.len() {
        return format!("All {} champions", names.len());
    }
    format!("{} champions: {}", names.len(), names.join(", "))
}

/// Text sizes of the legend.
const NAME_SIZE: f32 = 12.0;
const META_SIZE: f32 = 9.5;
const NOTE_SIZE: f32 = 10.0;
const MEMBER_SIZE: f32 = 9.5;
const TILE: f32 = 30.0;
const TILE_GAP: f32 = 10.0;
const ENTRY_GAP: f32 = 12.0;
const COLUMN_GAP: f32 = 24.0;
const MIN_COLUMN: f32 = 300.0;

/// One legend entry laid out at (0, 0), `width` wide.
fn legend_entry(snap: &Snapshot, group: &Group, width: f32) -> Scene {
    let mut s = Scene::new(width, 0.0);
    group_tile(
        &mut s,
        snap.group_color(group.value),
        &group.label,
        0.0,
        0.0,
        TILE,
        5.0,
        2.0,
        false,
    );
    let tx = TILE + TILE_GAP;
    let text_w = (width - tx).max(40.0);
    let mut y = NAME_SIZE;
    let meta = format!("{} · value {}", group.label, group.value);
    let meta_w = super::scene::text_width(&meta, META_SIZE, false);
    let name = fit_text(
        &group.name,
        NAME_SIZE,
        true,
        (text_w - meta_w - 8.0).max(30.0),
    );
    let name_w = super::scene::text_width(&name, NAME_SIZE, true);
    s.text(tx, y, NAME_SIZE, palette::TEXT, true, Anchor::Start, name);
    s.text(
        tx + name_w + 8.0,
        y,
        META_SIZE,
        palette::MUTED,
        false,
        Anchor::Start,
        meta,
    );
    if !group.note.is_empty() {
        for line in wrap_text(&group.note, NOTE_SIZE, false, text_w) {
            y += NOTE_SIZE * 1.35;
            s.text(tx, y, NOTE_SIZE, palette::MUTED, false, Anchor::Start, line);
        }
    }
    for line in wrap_text(&members_text(group), MEMBER_SIZE, false, text_w) {
        y += MEMBER_SIZE * 1.4;
        s.text(
            tx,
            y,
            MEMBER_SIZE,
            palette::TEXT,
            false,
            Anchor::Start,
            line,
        );
    }
    s.height = (y + 4.0).max(TILE);
    s
}

/// The group legend: a title and one entry per group, flowed into as many columns as fit
/// in `width` (each column gets the next entry while it is the shortest).
pub fn legend(snap: &Snapshot, groups: &[&Group], width: f32, title: &str) -> Scene {
    let columns = (((width + COLUMN_GAP) / (MIN_COLUMN + COLUMN_GAP)).floor() as usize)
        .clamp(1, groups.len().max(1));
    let col_w = (width - COLUMN_GAP * (columns - 1) as f32) / columns as f32;
    let mut scene = Scene::new(width, 0.0);
    let title_h: f32 = if title.is_empty() {
        0.0
    } else {
        scene.text(0.0, 14.0, 14.0, palette::TEXT, true, Anchor::Start, title);
        26.0
    };
    let mut heights = vec![title_h; columns];
    for group in groups {
        let entry = legend_entry(snap, group, col_w);
        let col = (0..columns)
            .min_by(|&a, &b| heights[a].total_cmp(&heights[b]))
            .unwrap_or(0);
        scene.append(&entry, col as f32 * (col_w + COLUMN_GAP), heights[col]);
        heights[col] += entry.height + ENTRY_GAP;
    }
    let tallest = heights.into_iter().fold(title_h, f32::max);
    scene.height = if groups.is_empty() {
        title_h
    } else {
        tallest - ENTRY_GAP
    };
    scene
}

const FL_ICON: f32 = 22.0;
const FL_GAP: f32 = 3.0;
const FL_LABEL: f32 = 58.0;
/// Space between the blue-side and red-side halves of a row.
const FL_HALF_GAP: f32 = 12.0;

/// The fearless block of the legend: one row of small portraits per earlier game, with
/// dashed boxes for slots left empty. `None` without fearless bans.
pub fn fearless_legend(snap: &Snapshot, width: f32) -> Option<Scene> {
    let f = &snap.fearless;
    if !f.is_active() {
        return None;
    }
    let mut s = Scene::new(width, 0.0);
    let mode = match f.mode {
        Mode::Hard => "hard fearless",
        Mode::Soft => "soft fearless",
    };
    let count = f.count();
    s.text(
        0.0,
        12.0,
        12.0,
        palette::TEXT,
        true,
        Anchor::Start,
        fit_text(
            &format!(
                "Fearless bans · game {} · {mode} · {count} champion{}",
                f.game,
                if count == 1 { "" } else { "s" }
            ),
            12.0,
            true,
            width,
        ),
    );
    let mut y = 22.0;
    for (g, row) in f.active_rows().iter().enumerate() {
        s.text(
            0.0,
            y + FL_ICON / 2.0 + 3.5,
            10.0,
            palette::MUTED,
            true,
            Anchor::Start,
            format!("Game {}", g + 1),
        );
        for (slot, &value) in row.iter().enumerate() {
            let half_gap = if slot >= HALF { FL_HALF_GAP } else { 0.0 };
            let x = FL_LABEL + slot as f32 * (FL_ICON + FL_GAP) + half_gap;
            let tone = match (f.mode, slot < HALF) {
                (Mode::Hard, _) => Tone::Purple,
                (Mode::Soft, true) => Tone::Blue,
                (Mode::Soft, false) => Tone::Red,
            };
            if value == 0 {
                s.outline(x, y, FL_ICON, FL_ICON, 4.0, palette::BORDER, 1.0, true);
            } else {
                s.items.push(Item::Icon {
                    champion: value,
                    x,
                    y,
                    size: FL_ICON,
                    radius: 4.0,
                    tone,
                    opacity: 1.0,
                });
            }
        }
        y += FL_ICON + 5.0;
    }
    if f.mode == Mode::Soft {
        y += 9.0;
        for line in wrap_text(
            "Soft fearless: the first five of each game were picked by the team on blue side in this game, the last five by the red-side team. Each team can't pick its own again.",
            9.0,
            false,
            width,
        ) {
            s.text(0.0, y, 9.0, palette::MUTED, false, Anchor::Start, line);
            y += 12.0;
        }
        y -= 9.0;
    }
    s.height = y;
    Some(s)
}

/// The group legend followed by the fearless block, as one scene. Empty (height 0) when
/// there is neither.
pub fn full_legend(snap: &Snapshot, groups: &[&Group], width: f32, title: &str) -> Scene {
    let mut scene = Scene::new(width, 0.0);
    let mut y = 0.0;
    if !groups.is_empty() {
        let g = legend(snap, groups, width, title);
        scene.append(&g, 0.0, 0.0);
        y = g.height + 16.0;
    }
    if let Some(f) = fearless_legend(snap, width) {
        scene.append(&f, 0.0, y);
        y += f.height;
    } else if y > 0.0 {
        y -= 16.0;
    }
    scene.height = y;
    scene
}

/// The role tags of a tile: a light bar along the bottom edge of the square at (`x`, `y`)
/// with one position icon per role. Nothing for untagged tiles.
pub fn role_row(scene: &mut Scene, mask: u8, x: f32, y: f32, size: f32) {
    if mask == 0 {
        return;
    }
    let h = size * 0.3;
    let top = y + size - h;
    scene.rect(x, top, size, h, 0.0, Color::WHITE.with_alpha(215));
    // Shrink the icons when five of them would not fit across the tile.
    let n = mask.count_ones() as f32;
    let icon = (h - 2.0).min((size - 3.0) / n - 1.0);
    for (k, role) in crate::roles::roles_in(mask).enumerate() {
        let gx = x + 1.5 + k as f32 * (icon + 1.0);
        role_glyph(scene, role, gx, top + (h - icon) / 2.0, icon, palette::TEXT);
    }
}

/// One role slot: the values that can play the role. Two share it along a diagonal, three
/// to five as pie wedges (empty slots get a dashed outline).
pub fn role_slot(scene: &mut Scene, snap: &Snapshot, values: &[u8], x: f32, y: f32, size: f32) {
    let n = values.len().min(crate::roles::MAX_SPLIT);
    if n == 0 {
        scene.outline(x, y, size, size, 4.0, palette::BORDER, 1.0, true);
        return;
    }
    if n == 1 {
        value_tile(scene, snap, values[0], x, y, size, 4.0, false, 1.0, None);
        return;
    }
    for (k, &v) in values.iter().take(n).enumerate() {
        let poly = crate::roles::band_polygon(k, n);
        if champions::by_value(v).is_some() {
            scene.items.push(Item::IconBand {
                champion: v,
                x,
                y,
                size,
                poly,
                shift: crate::roles::band_shift(k, n),
            });
        } else {
            let fill = if snap.groups.get(v).is_some() {
                snap.group_color(v).tint(0.45)
            } else {
                palette::BORDER
            };
            scene.items.push(Item::Poly {
                points: poly
                    .iter()
                    .map(|(px, py)| (x + px * size, y + py * size))
                    .collect(),
                fill,
            });
        }
    }
    for (x1, y1, x2, y2) in crate::roles::split_lines(n) {
        scene.line(
            x + x1 * size,
            y + y1 * size,
            x + x2 * size,
            y + y2 * size,
            Color::WHITE,
            (size * 0.04).max(0.8),
        );
    }
    scene.outline(x, y, size, size, 4.0, palette::BORDER, 1.0, false);
}

/// The Riot position icon for `role`, fitted into the square at (`x`, `y`) in `color`, with
/// the faint parts of the map dimmed.
pub fn role_glyph(
    scene: &mut Scene,
    role: crate::roles::Role,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
) {
    let icon = crate::role_icon::icon(role);
    let (k, dx, dy) = icon.fit(x, y, size);
    let dim = color.with_alpha((color.a as f32 * crate::role_icon::DIM_OPACITY).round() as u8);
    for layer in &icon.layers {
        scene.items.push(Item::Shape {
            segs: layer.segs.iter().map(|s| s.placed(k, dx, dy)).collect(),
            fill: if layer.dim { dim } else { color },
            even_odd: layer.even_odd,
        });
    }
}
