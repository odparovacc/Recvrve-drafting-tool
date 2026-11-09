//! The tree image: a title, the group legend, then the tree drawn like the app's tree view.
//! Portrait draws it top to bottom (draft steps as rows, labels on the left), landscape left
//! to right (draft steps as columns, labels on top).

use crate::draft::{Action, DRAFT_ORDER};
use crate::tree::{EMPTY, NO_BAN};

use super::draw::{full_legend, value_tile};
use super::model::Snapshot;
use super::scene::{Anchor, Color, Item, Scene, palette};

// Same geometry as the app's tree view.
const NODE_SIZE: f32 = 36.0;
const COL_GAP: f32 = 64.0;
const ROW_GAP: f32 = 46.0;
const PAD: f32 = 12.0;
const HEADER: f32 = 22.0;
/// Outer margin around the whole image.
const MARGIN: f32 = 16.0;
const BORDER: f32 = 2.0;
const EDGE_WIDTH: f32 = 2.0;
const HIGHLIGHT_WIDTH: f32 = 3.5;
/// Extra width for each further highlight sharing an edge.
const HIGHLIGHT_STEP: f32 = 3.0;
/// Width of the soft glow behind unreachable branches.
const GLOW_WIDTH: f32 = 8.0;
/// Narrowest image, so the title line fits.
const MIN_WIDTH: f32 = 440.0;
/// Portrait: width of the step label column.
const LABEL_W: f32 = 44.0;
/// Portrait: height of one draft step's row.
const LEVEL_H: f32 = 56.0;
/// Levels in a full draft: the start and the 20 draft steps.
const LEVELS: usize = DRAFT_ORDER.len() + 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeImageOptions {
    pub legend: bool,
    /// Top to bottom (true) or left to right (false).
    pub portrait: bool,
}

/// Rows per node (leaves get whole rows, parents sit between their first and last child)
/// and the number of rows.
fn rows(snap: &Snapshot) -> (Vec<f32>, usize) {
    let mut row = vec![0.0f32; snap.nodes.len()];
    let mut next = 0usize;
    // Leaves are numbered top to bottom, which is pre-order.
    for (i, n) in snap.nodes.iter().enumerate() {
        if n.children.is_empty() {
            row[i] = next as f32;
            next += 1;
        }
    }
    // Reversed pre-order visits children before their parents.
    for i in (0..snap.nodes.len()).rev() {
        let n = &snap.nodes[i];
        if let (Some(&f), Some(&l)) = (n.children.first(), n.children.last()) {
            row[i] = (row[f] + row[l]) / 2.0;
        }
    }
    (row, next.max(1))
}

/// Size of the tree drawing alone (without title and legend).
pub fn tree_size(snap: &Snapshot, portrait: bool) -> (f32, f32) {
    let (_, rows) = rows(snap);
    if portrait {
        return (
            LABEL_W + PAD * 2.0 + (rows - 1) as f32 * ROW_GAP + NODE_SIZE,
            PAD * 2.0 + LEVELS as f32 * LEVEL_H,
        );
    }
    let cols = snap.max_depth().max(DRAFT_ORDER.len());
    (
        PAD * 2.0 + cols as f32 * COL_GAP + NODE_SIZE,
        PAD * 2.0 + HEADER + (rows - 1) as f32 * ROW_GAP + NODE_SIZE,
    )
}

/// Centre of the node at depth `col` and leaf position `row`.
fn center(portrait: bool, col: usize, row: f32) -> (f32, f32) {
    if portrait {
        (
            LABEL_W + PAD + row * ROW_GAP + NODE_SIZE / 2.0,
            PAD + (col as f32 + 0.5) * LEVEL_H,
        )
    } else {
        (
            PAD + col as f32 * COL_GAP + NODE_SIZE / 2.0,
            PAD + HEADER + row * ROW_GAP + NODE_SIZE / 2.0,
        )
    }
}

fn team_color(snap: &Snapshot, i: usize) -> Color {
    if i == 0 {
        return palette::BORDER;
    }
    match snap.nodes[i].pos.team() {
        crate::draft::Team::Blue => palette::BLUE,
        crate::draft::Team::Red => palette::RED,
    }
}

/// The tree drawing at (0, 0).
fn tree_scene(snap: &Snapshot, portrait: bool) -> Scene {
    let (width, height) = tree_size(snap, portrait);
    let (row, _) = rows(snap);
    let mut s = Scene::new(width, height);
    let at = |i: usize| center(portrait, snap.nodes[i].depth, row[i]);

    // Phase separators.
    for (i, pair) in DRAFT_ORDER.windows(2).enumerate() {
        if pair[0].action != pair[1].action {
            if portrait {
                let y = PAD + (i + 2) as f32 * LEVEL_H;
                s.line(0.0, y, width, y, palette::BORDER, 1.0);
            } else {
                let x = center(false, i + 1, 0.0).0 + COL_GAP / 2.0;
                s.line(x, 0.0, x, height, palette::BORDER, 1.0);
            }
        }
    }

    // Step labels: a column on the left (portrait) or a row on top (landscape).
    let label_at = |level: usize| {
        if portrait {
            (LABEL_W / 2.0, PAD + (level as f32 + 0.5) * LEVEL_H + 3.5)
        } else {
            (
                PAD + level as f32 * COL_GAP + NODE_SIZE / 2.0,
                PAD + 8.0 + 9.0,
            )
        }
    };
    let (x, y) = label_at(0);
    s.text(x, y, 10.0, palette::MUTED, true, Anchor::Middle, "Start");
    for (i, step) in DRAFT_ORDER.iter().enumerate() {
        let color = match step.team {
            crate::draft::Team::Blue => palette::BLUE,
            crate::draft::Team::Red => palette::RED,
        };
        let (x, y) = label_at(i + 1);
        s.text(x, y, 10.0, color, true, Anchor::Middle, step.code());
    }

    // Which highlights pass through each node: a node is on a highlighted line if it is the
    // highlighted node or one of its ancestors.
    let mut through: Vec<Vec<Color>> = vec![Vec::new(); snap.nodes.len()];
    for &(target, color) in &snap.highlights {
        for i in snap.path(target) {
            through[i].push(color);
        }
    }

    // Plain edges first, highlighted ones on top.
    let edge = |s: &mut Scene, i: usize, color: Color, width: f32| {
        let Some(p) = snap.nodes[i].parent else {
            return;
        };
        let (x1, y1) = at(p);
        let (x2, y2) = at(i);
        // The same braces as the app's graph.
        let point = |(x, y): (f64, f64)| (x as f32, y as f32);
        let segments = crate::tree_view::edge_segments(
            (f64::from(x1), f64::from(y1)),
            (f64::from(x2), f64::from(y2)),
            f64::from(NODE_SIZE),
            portrait,
        );
        for [from, c1, c2, to] in segments {
            s.items.push(Item::Curve {
                from: point(from),
                c1: point(c1),
                c2: point(c2),
                to: point(to),
                color,
                width,
            });
        }
    };
    for (i, colors) in through.iter().enumerate().skip(1) {
        if !colors.is_empty() {
            continue;
        }
        if snap.nodes[i].conflict.is_some() {
            // Conflicting branches glow green: the line into the node where the line
            // breaks, and every line below it.
            edge(&mut s, i, palette::CONFLICT.with_alpha(80), GLOW_WIDTH);
            edge(&mut s, i, palette::CONFLICT, EDGE_WIDTH + 0.5);
            continue;
        }
        match snap.nodes[i].pruned {
            // Unreachable branches glow in their fearless tone.
            Some(tone) => {
                edge(&mut s, i, tone.accent().with_alpha(80), GLOW_WIDTH);
                edge(&mut s, i, tone.accent(), EDGE_WIDTH + 0.5);
            }
            None => edge(&mut s, i, palette::EDGE, EDGE_WIDTH),
        }
    }
    for (i, colors) in through.iter().enumerate().skip(1) {
        // Shared edges show every color: the first highlight widest, later ones on top.
        let n = colors.len();
        for (k, &color) in colors.iter().enumerate() {
            let width = HIGHLIGHT_WIDTH + HIGHLIGHT_STEP * (n - 1 - k) as f32;
            edge(&mut s, i, color, width);
        }
    }

    // Nodes.
    for (i, node) in snap.nodes.iter().enumerate() {
        let (cx, cy) = at(i);
        let (x, y) = (cx - NODE_SIZE / 2.0, cy - NODE_SIZE / 2.0);
        if i == 0 {
            s.items.push(Item::Circle {
                cx,
                cy,
                r: NODE_SIZE / 2.0 - BORDER / 2.0,
                fill: Some(Color::WHITE),
                stroke: Some((palette::BORDER, BORDER)),
            });
            s.text(
                cx,
                cy + 5.5,
                16.0,
                palette::MUTED,
                false,
                Anchor::Middle,
                "●",
            );
        } else {
            s.rect(x, y, NODE_SIZE, NODE_SIZE, 6.0, Color::WHITE);
            let ban =
                node.pos.action() == Action::Ban && node.value != EMPTY && node.value != NO_BAN;
            let inner = NODE_SIZE - 2.0 * BORDER;
            value_tile(
                &mut s,
                snap,
                node.value,
                x + BORDER,
                y + BORDER,
                inner,
                4.0,
                ban,
                0.8,
                node.pruned,
            );
            super::draw::role_row(&mut s, node.roles, x + BORDER, y + BORDER, inner);
            s.outline(
                x,
                y,
                NODE_SIZE,
                NODE_SIZE,
                6.0,
                team_color(snap, i),
                BORDER,
                false,
            );
            if let Some(first) = node.conflict {
                // Faded like the graph view: a white veil, plus a green ring where the
                // line breaks.
                s.rect(
                    x,
                    y,
                    NODE_SIZE,
                    NODE_SIZE,
                    6.0,
                    Color::WHITE.with_alpha(130),
                );
                if first {
                    s.outline(
                        x - 2.0,
                        y - 2.0,
                        NODE_SIZE + 4.0,
                        NODE_SIZE + 4.0,
                        8.0,
                        palette::CONFLICT,
                        2.0,
                        false,
                    );
                }
            }
        }
        // Highlighted target nodes get a ring per highlight, the first outermost.
        let rings: Vec<Color> = snap
            .highlights
            .iter()
            .filter(|(t, _)| *t == i)
            .map(|(_, c)| *c)
            .collect();
        for (k, &color) in rings.iter().enumerate() {
            let grow = 3.0 * (rings.len() - k) as f32;
            let r = if i == 0 {
                NODE_SIZE / 2.0 + grow
            } else {
                6.0 + grow
            };
            s.outline(
                x - grow,
                y - grow,
                NODE_SIZE + 2.0 * grow,
                NODE_SIZE + 2.0 * grow,
                r,
                color,
                3.0,
                false,
            );
        }
        // After the rings, so a highlight doesn't cover it.
        if i != 0 && node.starred {
            // On a marked line: a gold star in the bottom-right corner, as in the app.
            let (sx, sy) = (x + NODE_SIZE - 1.0, y + NODE_SIZE - 1.0);
            s.items.push(Item::Circle {
                cx: sx,
                cy: sy,
                r: 7.5,
                fill: Some(Color::WHITE),
                stroke: Some((palette::GOLD, 1.0)),
            });
            s.text(
                sx,
                sy + 4.0,
                11.0,
                palette::GOLD,
                false,
                Anchor::Middle,
                "★",
            );
        }
    }
    s
}

/// Title line text: what was exported.
fn summary(snap: &Snapshot) -> String {
    let lines = snap.lines();
    let nodes = snap.nodes.len() - 1;
    format!(
        "{} line{} · {} node{}",
        lines,
        if lines == 1 { "" } else { "s" },
        nodes,
        if nodes == 1 { "" } else { "s" }
    )
}

/// The full image scene.
pub fn build(snap: &Snapshot, options: TreeImageOptions, title: &str, date: &str) -> Scene {
    let tree = tree_scene(snap, options.portrait);
    // Room for the title and the summary next to it, even for a narrow portrait tree.
    let width = (tree.width + 2.0 * MARGIN).max(MIN_WIDTH);
    let mut s = Scene::new(width, 0.0);
    let mut y = MARGIN;

    s.text(
        MARGIN,
        y + 16.0,
        18.0,
        palette::TEXT,
        true,
        Anchor::Start,
        title,
    );
    s.text(
        width - MARGIN,
        y + 16.0,
        11.0,
        palette::MUTED,
        false,
        Anchor::End,
        format!("{} · {}", summary(snap), date),
    );
    y += 30.0;

    let groups = snap.used_groups();
    let legend = full_legend(snap, &groups, width - 2.0 * MARGIN, "Groups");
    if options.legend && legend.height > 0.0 {
        y += 6.0;
        s.append(&legend, MARGIN, y);
        y += legend.height + 14.0;
        s.line(MARGIN, y, width - MARGIN, y, palette::BORDER, 1.0);
        y += 6.0;
    }

    s.append(&tree, MARGIN, y);
    y += tree.height + MARGIN;
    s.height = y;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::icons;
    use crate::export::raster;
    use crate::tree::DraftTree;
    use std::collections::HashMap;

    #[test]
    fn rows_match_the_tree_view() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        t.add_child(root, 17).unwrap();
        t.add_child(root, 18).unwrap();
        t.add_child(a, 20).unwrap();
        t.add_child(a, 21).unwrap();
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let (row, n) = rows(&snap);
        // Pre-order: root, a, a1, a2, b, c.
        assert_eq!(n, 4);
        assert_eq!(row, [1.75, 0.5, 0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn renders_a_tree_with_groups_and_highlights() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        let b = t.add_child(a, 1).unwrap();
        let c = t.add_child(b, 3).unwrap();
        t.add_child(a, 17).unwrap();
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[c], false, true);
        let scene = build(
            &snap,
            TreeImageOptions {
                legend: true,
                portrait: false,
            },
            "Draft tree",
            "today",
        );
        assert!(scene.height > tree_size(&snap, false).1);
        let pixmap = raster::render(&scene, 1.0, icons::for_tests()).unwrap();
        assert!(pixmap.width() > 1000);
        // Portrait: tall and narrow for a narrow tree, labels on the left.
        let portrait = build(
            &snap,
            TreeImageOptions {
                legend: false,
                portrait: true,
            },
            "Draft tree",
            "today",
        );
        let (w, h) = tree_size(&snap, true);
        assert!(h > w, "{w} × {h}");
        assert!(portrait.height > h);
        assert!(raster::render(&portrait, 1.0, icons::for_tests()).is_some());
    }

    #[test]
    fn portrait_is_the_landscape_layout_turned() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        t.add_child(root, 17).unwrap();
        t.add_child(a, 20).unwrap();
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        // Depth goes down, leaves go right.
        let (x0, y0) = center(true, 0, 0.0);
        let (x1, y1) = center(true, 1, 0.0);
        let (x2, _) = center(true, 1, 1.0);
        assert_eq!(x0, x1);
        assert!(y1 > y0 && x2 > x1);
        // Every level fits; wider trees get wider, never taller.
        let (_, h) = tree_size(&snap, true);
        let (_, bottom) = center(true, DRAFT_ORDER.len(), 0.0);
        assert!(bottom + NODE_SIZE / 2.0 <= h);
        t.add_child(root, 18).unwrap();
        let wider = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        assert_eq!(tree_size(&wider, true).1, h);
        assert!(tree_size(&wider, true).0 > tree_size(&snap, true).0);
    }
}
