//! Graph view of the draft tree.
//!
//! The tree is drawn top to bottom by default: each row is one draft step (the root first)
//! and each leaf gets its own column. A parent sits halfway between its first and last
//! child, so a linear draft is a single straight line and branches fan out sideways. Sizes
//! follow the chosen node size; the graph scrolls when it doesn't fit. The whole tree is
//! shifted right so the root sits in the middle of the panel, as far as the tree allows
//! (it never moves left of the label column).
//!
//! Left to right (the older view) is the same layout turned: columns are draft steps,
//! leaves get rows, and the graph scales down to fit the panel's width (see layout.rs).
//!
//! The layout code says column for the depth and row for the leaf order either way.

use dioxus::prelude::*;

use crate::draft::{Action, DRAFT_ORDER};
use slotmap::SecondaryMap;
use std::collections::HashSet;

use crate::fearless::Tone;
use crate::tree::{Cause, Conflict, DraftTree, EMPTY, NO_BAN, NodeId};
use crate::value_view::{RoleIcons, ValueIcon, group_tooltip, value_name};

/// The nodes new to the tree at its last change (smooth profile only), and when that
/// change came. A different tab (or the first draw) brings a whole tree, which counts as
/// nothing new; so do nodes that only come into the graph's drawn window.
#[derive(Clone, Default)]
pub struct Entering {
    pub nodes: std::rc::Rc<HashSet<NodeId>>,
    pub at: Option<std::time::Instant>,
}

impl PartialEq for Entering {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.nodes, &other.nodes) && self.at == other.at
    }
}

impl Entering {
    /// Whether `id` was just added: in the set, from a change under half a second ago
    /// (a later visit to the same node only moves into it).
    pub fn just_added(&self, id: NodeId) -> bool {
        self.at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(500))
            && self.nodes.contains(&id)
    }
}

/// The memo behind `ui::Ctx::entering`: compares each layout's nodes with the last one's.
/// Memos can run twice for one change, so a second run with the same nodes returns what
/// the first found.
pub fn use_entering(
    graph: Memo<CachedLayout>,
    active_tab: Signal<u64>,
    smooth: Memo<bool>,
) -> Memo<Entering> {
    type Seen = (u64, SecondaryMap<NodeId, ()>, Entering);
    let known = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None::<Seen>)));
    use_memo(move || {
        let graph = graph.read();
        let tab = *active_tab.peek();
        let mut known = known.borrow_mut();
        if !smooth() {
            *known = None;
            return Entering::default();
        }
        let ids: SecondaryMap<NodeId, ()> = graph.index.keys().map(|id| (id, ())).collect();
        let new = match &*known {
            Some((was, seen, found)) if *was == tab && *seen == ids => found.clone(),
            Some((was, seen, _)) if *was == tab => Entering {
                nodes: std::rc::Rc::new(
                    ids.keys()
                        .filter(|id| !seen.contains_key(*id))
                        .collect::<HashSet<NodeId>>(),
                ),
                at: Some(std::time::Instant::now()),
            },
            _ => Entering::default(),
        };
        *known = Some((tab, ids, new.clone()));
        new
    })
}

const NODE_SIZE: f64 = 36.0;
const COL_GAP: f64 = 64.0;
const ROW_GAP: f64 = 46.0;
const PAD: f64 = 12.0;
/// Space above the first row for the step labels.
const HEADER: f64 = 22.0;
/// Top to bottom: width of the step label column on the left.
const LABEL_W: f64 = 44.0;
/// Top to bottom: space above the first and below the last level.
const V_PAD: f64 = 2.0;
/// Top to bottom: space between two levels' nodes.
const LEVEL_GAP: f64 = 16.0;
/// The hidden-branches design (500 × 500 units, node circle of radius 75 at the centre):
/// for each diagonal the stub's ends and the satellite's centre.
const COLLAPSE_SATELLITES: [(f64, f64, f64, f64, f64, f64); 4] = [
    (303.03, 196.97, 320.71, 179.29, 331.32, 168.68),
    (196.97, 196.97, 179.29, 179.29, 168.68, 168.68),
    (196.97, 303.03, 179.29, 320.71, 168.68, 331.32),
    (303.03, 303.03, 320.71, 320.71, 331.32, 331.32),
];
/// Levels in a full draft: the start and the 20 draft steps.
const LEVELS: usize = DRAFT_ORDER.len() + 1;

/// Top to bottom: node size at 100% zoom.
const V_NODE: f64 = 28.0;

/// Where nodes go, for one orientation. Sizes are scaled by the zoom; the step labels and
/// the padding around the graph are not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geom {
    /// Left to right: `scale` multiplies the node size and gaps, `shift` moves the tree
    /// down (to centre the root).
    Horizontal { scale: f64, shift: f64 },
    /// Top to bottom: `level` is the height of one draft step's row, `node` the node size,
    /// `pitch` the distance between two leaves and `shift` how far the tree is moved right
    /// (to centre the root).
    Vertical {
        level: f64,
        node: f64,
        pitch: f64,
        shift: f64,
    },
}

impl Geom {
    /// Top to bottom at `zoom` (1 = 100%), not shifted.
    pub fn vertical(zoom: f64) -> Geom {
        let node = (V_NODE * zoom).max(4.0).round();
        Geom::Vertical {
            level: (node + LEVEL_GAP * zoom).round(),
            node,
            pitch: (node + (node * 0.3).max(3.0)).round(),
            shift: 0.0,
        }
    }

    /// Left to right at `zoom` in a panel `view_width` wide: 100% fits all draft steps
    /// into the width (never larger than the full size), zooming in goes past it.
    pub fn horizontal(zoom: f64, view_width: f64) -> Geom {
        let full = DRAFT_ORDER.len() as f64 * COL_GAP + NODE_SIZE;
        let fit = ((view_width - 2.0 - 2.0 * PAD) / full).clamp(0.2, 1.0);
        Geom::Horizontal {
            scale: zoom * fit,
            shift: 0.0,
        }
    }

    /// Moves the tree so the root (at leaf position `root_row`) starts in the middle of the
    /// panel (`view_width` × `view_height`): right when top to bottom, down when left to
    /// right, as far as there is room for that (nothing moves up or left).
    pub fn centered(self, root_row: f64, view_width: f64, view_height: f64) -> Geom {
        match self {
            Geom::Vertical {
                level, node, pitch, ..
            } => {
                let root_x = self.center(0, root_row).0;
                let shift = (view_width / 2.0 - root_x).max(0.0).floor();
                Geom::Vertical {
                    level,
                    node,
                    pitch,
                    shift,
                }
            }
            Geom::Horizontal { scale, .. } => {
                let root_y = self.center(0, root_row).1;
                let shift = (view_height / 2.0 - root_y).max(0.0).floor();
                Geom::Horizontal { scale, shift }
            }
        }
    }

    pub fn node(&self) -> f64 {
        match *self {
            Geom::Horizontal { scale, .. } => (NODE_SIZE * scale).round(),
            Geom::Vertical { node, .. } => node,
        }
    }

    /// Left to right: distance between two draft steps.
    fn col_gap(&self) -> f64 {
        match *self {
            Geom::Horizontal { scale, .. } => COL_GAP * scale,
            Geom::Vertical { level, .. } => level,
        }
    }

    /// Centre of the node at depth `col` and leaf position `row`.
    pub fn center(&self, col: usize, row: f64) -> (f64, f64) {
        match *self {
            Geom::Horizontal { scale, shift } => (
                PAD + col as f64 * COL_GAP * scale + self.node() / 2.0,
                PAD + HEADER + shift + row * ROW_GAP * scale + self.node() / 2.0,
            ),
            Geom::Vertical {
                level,
                node,
                pitch,
                shift,
            } => (
                LABEL_W + PAD + shift + row * pitch + node / 2.0,
                V_PAD + (col as f64 + 0.5) * level,
            ),
        }
    }

    /// Canvas size for `depth` levels below the root and `rows` leaves.
    pub fn size(&self, depth: usize, rows: usize) -> (f64, f64) {
        match *self {
            Geom::Horizontal { scale, shift } => (
                PAD * 2.0 + depth.max(DRAFT_ORDER.len()) as f64 * COL_GAP * scale + self.node(),
                PAD * 2.0 + HEADER + shift + (rows - 1) as f64 * ROW_GAP * scale + self.node(),
            ),
            Geom::Vertical {
                level,
                node,
                pitch,
                shift,
            } => (
                LABEL_W + PAD * 2.0 + shift + (rows - 1) as f64 * pitch + node,
                2.0 * V_PAD + LEVELS as f64 * level,
            ),
        }
    }

    /// The edge from a parent's centre to a child's centre, as an SVG path (see
    /// [`edge_segments`]).
    fn edge(&self, from: (f64, f64), to: (f64, f64)) -> String {
        let vertical = matches!(self, Geom::Vertical { .. });
        let segments = edge_segments(from, to, self.node(), vertical);
        let mut d = format!("M{:.2},{:.2}", segments[0][0].0, segments[0][0].1);
        for [_, c1, c2, p] in segments {
            d.push_str(&format!(
                " C{:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
                c1.0, c1.1, c2.0, c2.1, p.0, p.1
            ));
        }
        d
    }
}

/// Where a brace's bar lies between a parent and its children, as a fraction of the gap
/// between them (from the parent's edge).
const BAR_AT: f64 = 0.4;

/// The line from a parent (centre `from`) to a child (centre `to`), as cubic Bézier
/// segments, each starting where the one before ends. A child in line with its parent
/// gets a straight line. Any other child hangs from a brace: the line leaves the parent
/// along the draft steps, curls sideways onto a bar it shares with the parent's other
/// children on that side, runs along it and curls into the child. `vertical`: the draft
/// steps go down (else right); `node`: the nodes' size.
pub fn edge_segments(
    from: (f64, f64),
    to: (f64, f64),
    node: f64,
    vertical: bool,
) -> Vec<[(f64, f64); 4]> {
    // Worked out along the draft steps (a) and the axis the tree branches along (b).
    let ab = |(x, y): (f64, f64)| if vertical { (y, x) } else { (x, y) };
    let (a1, b1) = ab(from);
    let (a2, b2) = ab(to);
    let line = |p: (f64, f64), q: (f64, f64)| {
        let at = |t: f64| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
        [p, at(1.0 / 3.0), at(2.0 / 3.0), q]
    };
    let db = b2 - b1;
    let segments = if db.abs() < 0.5 {
        vec![line((a1, b1), (a2, b2))]
    } else {
        // A quarter circle as a cubic: its control points lie this far along the tangents.
        const K: f64 = 0.552_284_75;
        let s = db.signum();
        let start = a1 + node / 2.0;
        let end = a2 - node / 2.0;
        let bar = start + (end - start).max(0.0) * BAR_AT;
        let r1 = (bar - start).min(db.abs() / 2.0);
        let r2 = (end - bar).max(0.0).min(db.abs() / 2.0);
        let (p1, p2) = ((bar - r1, b1), (bar, b1 + s * r1));
        let (p3, p4) = ((bar, b2 - s * r2), (bar + r2, b2));
        vec![
            line((a1, b1), p1),
            [p1, (p1.0 + K * r1, b1), (bar, p2.1 - s * K * r1), p2],
            line(p2, p3),
            [p3, (bar, p3.1 + s * K * r2), (p4.0 - K * r2, b2), p4],
            line(p4, (a2, b2)),
        ]
    };
    segments
        .into_iter()
        .map(|seg| seg.map(|p| if vertical { (p.1, p.0) } else { p }))
        .collect()
}

/// A node's place in the layout, in grid units (column = depth, row may be fractional).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub col: usize,
    pub row: f64,
}

/// Lays out the tree, leaving out the nodes in `hidden` and everything below them.
/// Returns the placed nodes (parents before children) and the number of rows used.
pub fn layout(tree: &DraftTree, hidden: &SecondaryMap<NodeId, Tone>) -> (Vec<Placed>, usize) {
    let mut placed = Vec::with_capacity(tree.len());
    let mut next_leaf_row = 0usize;
    place(
        tree,
        hidden,
        tree.root(),
        None,
        0,
        &mut next_leaf_row,
        &mut placed,
    );
    (placed, next_leaf_row.max(1))
}

/// Places `id`'s subtree and returns the row `id` ended up on. Recursion depth is bounded by
/// the draft length (21 levels).
fn place(
    tree: &DraftTree,
    hidden: &SecondaryMap<NodeId, Tone>,
    id: NodeId,
    parent: Option<NodeId>,
    col: usize,
    next_leaf_row: &mut usize,
    out: &mut Vec<Placed>,
) -> f64 {
    // Reserve our slot now so parents come before children; the row is filled in below.
    let index = out.len();
    out.push(Placed {
        id,
        parent,
        col,
        row: 0.0,
    });

    // A node with hidden branches is drawn as a leaf. Only the first and last shown
    // child's rows matter, so nothing is collected.
    let mut ends: Option<(f64, f64)> = None;
    if let Some(node) = tree.get(id).filter(|n| !n.is_collapsed()) {
        for &c in node.children() {
            if hidden.contains_key(c) {
                continue;
            }
            let row = place(tree, hidden, c, Some(id), col + 1, next_leaf_row, out);
            ends = Some((ends.map_or(row, |(first, _)| first), row));
        }
    }
    let row = match ends {
        Some((first, last)) => (first + last) / 2.0,
        None => {
            let row = *next_leaf_row as f64;
            *next_leaf_row += 1;
            row
        }
    };
    out[index].row = row;
    row
}

/// `placed` indices sorted by row, ties in index order: what a stable sort by row gives.
/// Every row lies in `0..rows`, so the indices are first bucketed by whole row (in index
/// order) and only each bucket's few entries are sorted.
fn sort_by_row(placed: &[Placed], rows: usize) -> Vec<usize> {
    let bucket = |p: &Placed| (p.row as usize).min(rows - 1);
    // Where each bucket starts in the output.
    let mut start = vec![0usize; rows + 1];
    for p in placed {
        start[bucket(p) + 1] += 1;
    }
    for b in 1..=rows {
        start[b] += start[b - 1];
    }
    let mut by_row = vec![0usize; placed.len()];
    let mut next = start.clone();
    for (i, p) in placed.iter().enumerate() {
        let b = bucket(p);
        by_row[next[b]] = i;
        next[b] += 1;
    }
    for b in 0..rows {
        let part = &mut by_row[start[b]..start[b + 1]];
        let after = |a: usize, b: usize| placed[a].row.total_cmp(&placed[b].row).is_gt();
        if part.len() > 16 {
            part.sort_by(|&a, &b| placed[a].row.total_cmp(&placed[b].row));
            continue;
        }
        // Usually a handful of entries: insertion sort, which keeps ties in order too.
        for i in 1..part.len() {
            let mut j = i;
            while j > 0 && after(part[j - 1], part[j]) {
                part.swap(j - 1, j);
                j -= 1;
            }
        }
    }
    by_row
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// The layout as it was first written (a `Vec` per node, a full sort by row): the
    /// reference the faster one must match exactly.
    fn reference(t: &DraftTree, hidden: &SecondaryMap<NodeId, Tone>) -> CachedLayout {
        fn place(
            tree: &DraftTree,
            hidden: &SecondaryMap<NodeId, Tone>,
            id: NodeId,
            parent: Option<NodeId>,
            col: usize,
            next_leaf_row: &mut usize,
            out: &mut Vec<Placed>,
        ) -> f64 {
            let index = out.len();
            out.push(Placed {
                id,
                parent,
                col,
                row: 0.0,
            });
            let children: Vec<NodeId> = tree
                .get(id)
                .filter(|n| !n.is_collapsed())
                .map(|n| n.children())
                .unwrap_or_default()
                .iter()
                .copied()
                .filter(|&c| !hidden.contains_key(c))
                .collect();
            let row = if children.is_empty() {
                let row = *next_leaf_row as f64;
                *next_leaf_row += 1;
                row
            } else {
                let rows: Vec<f64> = children
                    .iter()
                    .map(|&c| place(tree, hidden, c, Some(id), col + 1, next_leaf_row, out))
                    .collect();
                (rows[0] + rows[rows.len() - 1]) / 2.0
            };
            out[index].row = row;
            row
        }
        let mut placed = Vec::new();
        let mut next_leaf_row = 0usize;
        place(
            t,
            hidden,
            t.root(),
            None,
            0,
            &mut next_leaf_row,
            &mut placed,
        );
        let mut index = SecondaryMap::new();
        for (i, p) in placed.iter().enumerate() {
            index.insert(p.id, i);
        }
        let mut by_row: Vec<usize> = (0..placed.len()).collect();
        by_row.sort_by(|&a, &b| placed[a].row.total_cmp(&placed[b].row));
        CachedLayout {
            placed,
            rows: next_leaf_row.max(1),
            starred: SecondaryMap::new(),
            index,
            by_row,
        }
    }

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    #[test]
    fn layout_matches_the_reference_on_random_trees() {
        for seed in 1..=40u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut t = DraftTree::new();
            let mut nodes = vec![t.root()];
            let size = 1 + rng.below(if seed % 4 == 0 { 3000 } else { 120 });
            // Few distinct values cap the tree's size, so the tries are bounded too.
            for _ in 0..size * 4 {
                if t.len() >= size {
                    break;
                }
                let at = nodes[rng.below(nodes.len())];
                // Mostly deep and narrow, sometimes wide: uneven rows and many ties.
                let value = 16 + rng.below(if seed % 3 == 0 { 6 } else { 60 }) as u8;
                if let Ok(id) = t.add_child(at, value) {
                    nodes.push(id);
                }
            }
            for _ in 0..rng.below(size / 10 + 2) {
                let n = nodes[rng.below(nodes.len())];
                t.set_collapsed(n, true);
            }
            for _ in 0..rng.below(4) {
                let n = nodes[rng.below(nodes.len())];
                t.set_marked(n, true);
            }
            let mut hidden = SecondaryMap::new();
            for _ in 0..rng.below(size / 8 + 2) {
                let n = nodes[rng.below(nodes.len())];
                if n != t.root() {
                    hidden.insert(n, Tone::Both);
                }
            }
            let want = reference(&t, &hidden);
            let (placed, rows) = layout(&t, &hidden);
            assert_eq!((&placed, rows), (&want.placed, want.rows), "seed {seed}");
            assert_eq!(sort_by_row(&placed, rows), want.by_row, "seed {seed}");
            // "Marked only" goes through the same layout.
            let got = CachedLayout::compute(&t, seed % 2 == 0);
            let mut left_out = SecondaryMap::new();
            if seed % 2 == 0 {
                for (id, _) in t.nodes_iter() {
                    if !got.starred.contains_key(id) {
                        left_out.insert(id, Tone::Both);
                    }
                }
            }
            let want = reference(&t, &left_out);
            assert!(
                got.placed == want.placed && got.rows == want.rows,
                "seed {seed}"
            );
            assert!(
                got.index == want.index && got.by_row == want.by_row,
                "seed {seed}"
            );
        }
    }
}

/// "Blue Ban 2" for the draft step of `id`.
fn step_label(t: &DraftTree, id: NodeId) -> String {
    let depth = t.path(id).len().saturating_sub(1);
    match depth.checked_sub(1).and_then(|i| DRAFT_ORDER.get(i)) {
        Some(s) => format!("{} {} {}", s.team.label(), s.action.label(), s.number),
        None => "the start".to_string(),
    }
}

/// A run of draft steps of one kind: a ban phase or a pick phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phase {
    /// First and last draft step (0 = Blue Ban 1).
    pub first: usize,
    pub last: usize,
    pub action: Action,
    /// The first ban phase or the first pick phase (drawn a little stronger).
    pub opening: bool,
}

impl Phase {
    /// CSS classes: `ban` / `pick`, plus `phase-1` / `phase-2`.
    fn class(&self) -> String {
        let kind = if self.action == Action::Ban {
            "ban"
        } else {
            "pick"
        };
        let n = if self.opening { 1 } else { 2 };
        format!("phase-{kind} phase-{n}")
    }
}

/// The draft's phases in order: ban, pick, ban, pick.
pub fn phases() -> Vec<Phase> {
    let mut out: Vec<Phase> = Vec::new();
    for (i, step) in DRAFT_ORDER.iter().enumerate() {
        match out.last_mut() {
            Some(p) if p.action == step.action => p.last = i,
            _ => {
                let opening = !out.iter().any(|p| p.action == step.action);
                out.push(Phase {
                    first: i,
                    last: i,
                    action: step.action,
                    opening,
                });
            }
        }
    }
    out
}

#[component]
pub fn TreeView(
    tree: Signal<DraftTree>,
    cursor: NodeId,
    /// Left click: jump to this node's draft state.
    on_select: EventHandler<NodeId>,
    /// Right click: delete this node's subtree.
    on_delete: EventHandler<NodeId>,
    /// Middle click: edit this node's role tags (node, x, y on screen).
    on_roles: EventHandler<(NodeId, f64, f64)>,
    /// Top to bottom (true) or left to right.
    vertical: bool,
    /// Zoom, 1 = 100% (see [`Geom`]).
    zoom: f64,
    /// Size of the graph's scroll area, for fitting and centring the root.
    view_width: f64,
    view_height: f64,
    /// Find in tree: the matching nodes, ringed (the selected one more strongly).
    found: Vec<NodeId>,
    /// The pointer entered a node (with its position on screen) or left it.
    on_hover: EventHandler<Option<(NodeId, f64, f64)>>,
    /// Show the browser's tooltip (with debug details) on nodes.
    tooltips: bool,
    /// Nodes selected together with the active one (Ctrl+click, box), ringed.
    selected: Vec<NodeId>,
    /// Ctrl+click: add or remove a node from the selection.
    on_toggle: EventHandler<NodeId>,
    /// Show only marked lines.
    marked_only: bool,
    /// Follow mode: the nodes on lines that fit the draft entered so far (everything else
    /// is dimmed), and the draft step to enter next.
    #[props(default)]
    followed: Option<(Vec<NodeId>, usize)>,
    /// The smooth animation profile (see `.anim-smooth` in style.css): new nodes pop in,
    /// nodes glide when an edit moves them, the selected path draws itself.
    #[props(default)]
    smooth: bool,
) -> Element {
    let ctx = use_context::<crate::ui::Ctx>();
    // Conflict and fearless marks, computed once per tree change (see `ui::TreeInfo`).
    let info = ctx.info.read();
    let crate::tree::Marks {
        conflicts,
        unreachable,
    } = &info.marks;
    let t = tree.read();
    // The layout only changes with the tree and "Marked only" (see `ui::Ctx::graph`).
    let _ = marked_only;
    let cached = ctx.graph.read();
    let CachedLayout {
        placed,
        rows,
        starred,
        index,
        ..
    } = &*cached;
    let rows = *rows;
    let geom = graph_geom(vertical, zoom, view_width, view_height, placed[0].row);

    // Smooth profile: the nodes new to the tree pop in (see `ui::Ctx::entering`).
    let entering = ctx.entering.read();
    let entering = &entering.nodes;
    // Nodes glide to new places only when an edit moved them: not on another tab,
    // direction, zoom or view size (those place everything anew).
    let last_view = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None)));
    let view_key = (
        *ctx.active_tab.peek(),
        vertical,
        zoom.to_bits(),
        view_width.to_bits(),
        view_height.to_bits(),
    );
    let glide = smooth && *last_view.borrow() == Some(view_key);
    *last_view.borrow_mut() = Some(view_key);

    // Only the nodes in a window around the view are drawn (see `GraphWindow`). It moves
    // to where the view is when scrolling takes the view near its edge (the layout script
    // reports that), and to the selected node when the selection leaves it.
    let mut graph_center = ctx.graph_center;
    // The selection and direction of the last draw (`None` before the first one).
    let mut seen = use_signal(|| None::<(NodeId, bool)>);
    let view_breadth = if vertical { view_width } else { view_height };
    let (span, min_span) = window_span(vertical, view_breadth, &geom);
    let cursor_row = index.get(cursor).map(|&i| placed[i].row);
    let stored = graph_center();
    let last = *seen.peek();
    // Just shown, or the direction changed: the old centre means nothing here.
    let fresh = last.is_none_or(|(_, v)| v != vertical);
    let moved = last.is_none_or(|(c, _)| c != cursor);
    let stored = stored
        .filter(|c| c.is_finite() && !fresh)
        .map(|c| c.clamp(0.0, rows as f64));
    let mut window = cached.window(
        stored.or(cursor_row).unwrap_or(placed[0].row),
        span,
        min_span,
    );
    // The selection moved out of the window: the window follows it.
    if moved
        && let Some(r) = cursor_row
        && !window.contains_row(r)
    {
        window = cached.window(r, span, min_span);
    }
    let center = window.center;
    // Drawn around the selection without a view to keep: show the selection.
    let reveal = stored.is_none();
    use_effect(use_reactive!(|cursor, vertical, center, reveal| {
        if *seen.peek() != Some((cursor, vertical)) {
            seen.set(Some((cursor, vertical)));
        }
        if *graph_center.peek() != Some(center) {
            graph_center.set(Some(center));
        }
        if reveal {
            document::eval(
                "document.querySelector('.tree-node.current')?.scrollIntoView({block: 'center', inline: 'center'})",
            );
        }
    }));
    let on_path: Vec<NodeId> = t.path(cursor);
    // The draft step chosen next from the selected node (past the end when complete).
    let next_step = followed
        .as_ref()
        .map_or(on_path.len() - 1, |(_, step)| *step);
    // Follow mode: lines that no longer fit are dimmed, the ones that do are lit.
    let follow_set: Option<HashSet<NodeId>> = followed
        .as_ref()
        .map(|(nodes, _)| nodes.iter().copied().collect());
    let follow_class = |id: NodeId| match &follow_set {
        None => "",
        Some(nodes) if nodes.contains(&id) => "follow-on",
        Some(_) => "follow-off",
    };
    let found: HashSet<NodeId> = found.iter().copied().collect();
    let selected: HashSet<NodeId> = selected.iter().copied().collect();
    let depth = placed.iter().map(|p| p.col).max().unwrap_or(0);

    let (width, height) = geom.size(depth, rows);
    let node_size = geom.node();
    let col_gap = geom.col_gap();
    let level = match geom {
        Geom::Vertical { level, .. } => level,
        Geom::Horizontal { .. } => 0.0,
    };

    // Keep the selected node in view as the draft grows.
    use_effect(use_reactive!(|cursor| {
        let _ = cursor;
        document::eval(
            "document.querySelector('.tree-node.current')?.scrollIntoView({block: 'nearest', inline: 'nearest', behavior: window.__recvrveSmooth ? 'smooth' : 'auto'})",
        );
    }));

    let center_of = |id: NodeId| {
        index
            .get(id)
            .map(|&i| geom.center(placed[i].col, placed[i].row))
    };

    let shown = window.indices(placed.len());
    let lines = window_lines(&t, &cached, &window, &shown);

    // Edge path, its class, and its glow class for unreachable and conflicting branches,
    // keyed by the node the line leads into.
    let mut ranked: Vec<_> = lines
        .into_iter()
        .filter_map(|i| {
            let p = &placed[i];
            let from = center_of(p.parent?)?;
            let d = geom.edge(from, geom.center(p.col, p.row));
            // The line into a conflicting node and every line below it are green.
            let tone = if conflicts.contains_key(p.id) {
                Some("conflict")
            } else {
                unreachable.get(p.id).map(|t| t.class())
            };
            let class = match (tone, on_path.contains(&p.id)) {
                (Some(tone), true) => format!("edge pruned {tone} on-path"),
                (Some(tone), false) => format!("edge pruned {tone}"),
                (None, true) => "edge on-path".to_string(),
                (None, false) => "edge".to_string(),
            };
            let follow = follow_class(p.id);
            let class = format!("{class} {follow}");
            let depth = p.col;
            // Siblings share part of their brace: the lines that matter most go on top.
            let rank = (
                on_path.contains(&p.id),
                follow != "follow-off",
                tone.is_some(),
            );
            Some((rank, (p.id, d, class, tone, depth)))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let edges: Vec<(NodeId, String, String, Option<&'static str>, usize)> =
        ranked.into_iter().map(|(_, edge)| edge).collect();

    // Separators between draft steps: a thin turn line between any two turns, and a
    // thicker phase line where the draft switches between bans and picks. Each sits
    // halfway between two columns (left to right) or on the border of two rows (top to
    // bottom).
    let separators: Vec<(f64, &'static str)> = (0..DRAFT_ORDER.len())
        .map(|col| {
            // The boundary after column `col` (0 is the start column).
            let phase = col > 0
                && DRAFT_ORDER
                    .get(col)
                    .is_some_and(|next| next.action != DRAFT_ORDER[col - 1].action);
            let at = match geom {
                Geom::Horizontal { .. } => geom.center(col, 0.0).0 + col_gap / 2.0,
                Geom::Vertical { level, .. } => V_PAD + (col + 1) as f64 * level,
            };
            (at, if phase { "phase-line" } else { "turn-line" })
        })
        .collect();

    // A tinted band behind each ban / pick phase, so it is clear at a glance which phase a
    // part of the graph belongs to.
    let bands: Vec<(String, String)> = phases()
        .into_iter()
        .map(|p| {
            let style = match geom {
                // Steps `first..=last` are columns `first + 1 ..= last + 1`.
                Geom::Horizontal { .. } => {
                    let left = geom.center(p.first, 0.0).0 + col_gap / 2.0;
                    let right = (geom.center(p.last + 1, 0.0).0 + col_gap / 2.0).min(width);
                    format!("left: {left}px; width: {}px;", (right - left).max(0.0))
                }
                Geom::Vertical { level, .. } => format!(
                    "top: {}px; height: {}px;",
                    V_PAD + (p.first + 1) as f64 * level,
                    (p.last - p.first + 1) as f64 * level
                ),
            };
            (p.class(), style)
        })
        .collect();
    let step_phase = |i: usize| {
        phases()
            .into_iter()
            .find(|p| (p.first..=p.last).contains(&i))
            .map(|p| p.class())
            .unwrap_or_default()
    };

    // What each node shows. Each node is its own component, so a render only touches the
    // nodes whose look changed (the selected line, Find matches, ...).
    let nodes: Vec<NodeView> = shown
        .into_iter()
        .map(|i| {
            let p = &placed[i];
            let id = p.id;
            let node = t.get(id).unwrap();
            let (cx, cy) = geom.center(p.col, p.row);
            let pos = node.pos();
            let value = node.value();
            let is_root = p.parent.is_none();
            let team = if is_root { "root" } else { pos.team().class() };
            let kind = if pos.action() == Action::Ban && !is_root {
                "ban"
            } else {
                "pick"
            };
            let banned = if kind == "ban" && value != EMPTY && value != NO_BAN {
                "banned"
            } else {
                ""
            };
            let current = if p.id == cursor {
                "current"
            } else if on_path.contains(&p.id) {
                "on-path"
            } else {
                ""
            };
            let pruned = match conflicts.get(id) {
                Some(Conflict::Root(_)) => "conflict conflict-root".to_string(),
                Some(Conflict::Below) => "conflict".to_string(),
                None => unreachable
                    .get(id)
                    .map_or(String::new(), |t| format!("pruned {}", t.class())),
            };
            // Hidden branches: the node turns round, with satellites and the number of
            // hidden nodes in the color of the line into it.
            let collapsed = node.is_collapsed();
            let mark = if conflicts.contains_key(id) {
                "var(--conflict)"
            } else if let Some(tone) = unreachable.get(id) {
                match tone {
                    Tone::Both => "var(--fl-both)",
                    Tone::Blue => "var(--fl-blue)",
                    Tone::Red => "var(--fl-red)",
                }
            } else if on_path.contains(&id) {
                "var(--edge-active, var(--gold))"
            } else {
                "var(--edge, var(--border))"
            };
            let collapsed_class = if collapsed { "collapsed" } else { "" };
            let multi_class = if !selected.is_empty() && (id == cursor || selected.contains(&id)) {
                "multi"
            } else {
                ""
            };
            let found_class = if !found.contains(&id) {
                ""
            } else if id == cursor {
                "find-match find-current"
            } else {
                "find-match"
            };
            let follow_node = follow_class(id);
            // The debug tooltip is only built while tooltips are on.
            let title = if tooltips {
                node_tooltip(&t, p, conflicts, unreachable)
            } else {
                String::new()
            };
            NodeView {
                id,
                class: format!(
                    "tree-node {team} {kind} {current} {banned} {pruned} {collapsed_class} {found_class} {multi_class} {follow_node}{}",
                    if entering.contains(&id) { " node-enter" } else { "" }
                ),
                // Smooth: placed with `translate`, which can glide (see `glide`).
                style: if smooth {
                    format!(
                        "left: 0; top: 0; translate: {}px {}px; width: {node_size}px; height: {node_size}px; --mark: {mark}; --node: {node_size}px;",
                        cx - node_size / 2.0,
                        cy - node_size / 2.0
                    )
                } else {
                    format!(
                        "left: {}px; top: {}px; width: {node_size}px; height: {node_size}px; --mark: {mark}; --node: {node_size}px;",
                        cx - node_size / 2.0,
                        cy - node_size / 2.0
                    )
                },
                title,
                depth: p.col,
                value,
                roles: node.roles(),
                is_root,
                empty_number: t.empty_number(id),
                hidden_count: collapsed.then(|| t.subtree_size(id)),
                star: starred.contains_key(id) && !is_root,
            }
        })
        .collect();

    rsx! {
        // For the layout script: how layout units map to pixels (`data-geom`: the
        // branching axis `win-axis`, then the draft step axis, each as position of 0 and
        // pixels per unit), so zooming and resizing keep the view on the same spot; and the
        // drawn window (none when the whole tree is drawn): rows `win-lo` to `win-hi`,
        // built around `win-center`, with `win-start` / `win-end` when it reaches the
        // tree's first / last row.
        div {
            class: match (vertical, glide) {
                (true, true) => "tree-canvas vertical glide",
                (true, false) => "tree-canvas vertical",
                (false, true) => "tree-canvas glide",
                (false, false) => "tree-canvas",
            },
            "data-geom": "{geom_attr(&geom)}",
            "data-rows": "{rows}",
            "data-win-axis": if vertical { "x" } else { "y" },
            "data-win-lo": if window.whole { String::new() } else { format!("{}", window.lo) },
            "data-win-hi": if window.whole { String::new() } else { format!("{}", window.hi) },
            "data-win-start": if window.start { "1" } else { "" },
            "data-win-end": if window.end { "1" } else { "" },
            "data-win-center": "{window.center}",
            style: "width: {width}px; height: {height}px;",
            for (class, style) in bands {
                // Keyed by direction, so switching it doesn't keep the other one's position.
                div { key: "{class}-{vertical}", class: "phase-band {class}", style: "{style}" }
            }
            for (at, class) in separators {
                div {
                    key: "sep{at}",
                    class: "{class}",
                    style: if vertical { format!("top: {at}px;") } else { format!("left: {at}px;") },
                }
            }
            if vertical {
                // Step labels, one per row, in a column that stays on the left while the
                // graph scrolls sideways. The step to choose next is underlined.
                div { class: "tree-steps vertical",
                    div { class: "tree-step root", style: "top: {V_PAD}px; height: {level}px;", "Start" }
                    for (i, s) in DRAFT_ORDER.iter().enumerate() {
                        div {
                            key: "step{i}",
                            class: if i == next_step { "tree-step {s.team.class()} {step_phase(i)} current" } else { "tree-step {s.team.class()} {step_phase(i)}" },
                            style: "top: {V_PAD + (i + 1) as f64 * level}px; height: {level}px;",
                            title: "{s.team.label()} {s.action.label()} {s.number}",
                            "{s.code()}"
                        }
                    }
                }
            } else {
            // Column headers, one per draft step, in a band that stays at the top while the
            // graph scrolls. The step to choose next is underlined.
            div { class: "tree-steps", style: "width: {width}px;",
                div { class: "tree-step root", style: "left: {PAD + node_size / 2.0 - col_gap / 2.0}px; width: {col_gap}px;", "Start" }
                for (i, s) in DRAFT_ORDER.iter().enumerate() {
                    div {
                        key: "step{i}",
                        class: if i == next_step { "tree-step {s.team.class()} {step_phase(i)} current" } else { "tree-step {s.team.class()} {step_phase(i)}" },
                        style: "left: {PAD + node_size / 2.0 + (i as f64 + 0.5) * col_gap}px; width: {col_gap}px;",
                        title: "{s.team.label()} {s.action.label()} {s.number}",
                        "{s.code()}"
                    }
                }
            }
            }
            svg {
                class: "tree-edges",
                width: "{width}",
                height: "{height}",
                for (id, d, class, glow, depth) in edges {
                    GraphEdge { key: "{id:?}", d, class, glow, depth, smooth, glide }
                }
            }
            for n in nodes {
                GraphNode {
                    key: "{n.id:?}",
                    view: n,
                    on_select,
                    on_toggle,
                    on_hover,
                    on_roles,
                    on_delete,
                }
            }
        }
    }
}

/// The graph's layout, cached until the tree or "Marked only" changes (see `ui::Ctx`).
#[derive(Clone, PartialEq)]
pub struct CachedLayout {
    pub placed: Vec<Placed>,
    pub rows: usize,
    /// Nodes on a marked line (they get a star).
    pub starred: SecondaryMap<NodeId, ()>,
    /// Where each node is in `placed`.
    pub index: SecondaryMap<NodeId, usize>,
    /// `placed` indices sorted by row: the axis the drawn window slides along.
    by_row: Vec<usize>,
}

impl CachedLayout {
    pub fn compute(t: &DraftTree, marked_only: bool) -> CachedLayout {
        let starred = t.starred();
        // Branches left out: unmarked ones while only marked lines show.
        let mut left_out: SecondaryMap<NodeId, Tone> = SecondaryMap::new();
        if marked_only {
            for (id, _) in t.nodes_iter() {
                if !starred.contains_key(id) {
                    left_out.insert(id, Tone::Both);
                }
            }
        }
        let (placed, rows) = layout(t, &left_out);
        let mut index = SecondaryMap::with_capacity(placed.len());
        for (i, p) in placed.iter().enumerate() {
            index.insert(p.id, i);
        }
        let by_row = sort_by_row(&placed, rows);
        CachedLayout {
            placed,
            rows,
            starred,
            index,
            by_row,
        }
    }

    /// The nodes to draw around row `center`: those within `span` rows of it, at most
    /// [`WINDOW_MAX_NODES`] (the ones nearest the centre) but always every node within
    /// `min_span` rows. Only whole rows: the nodes of a row are drawn together or not at all.
    pub fn window(&self, center: f64, span: f64, min_span: f64) -> GraphWindow {
        let row = |i: usize| self.placed[self.by_row[i]].row;
        let n = self.by_row.len();
        // First node at or past `r` / past `r`: both fall between rows.
        let first_at = |r: f64| self.by_row.partition_point(|&i| self.placed[i].row < r);
        let first_after = |r: f64| self.by_row.partition_point(|&i| self.placed[i].row <= r);
        let min_span = min_span.min(span);
        let mut lo = first_at(center - span);
        let mut hi = first_after(center + span).max(lo);
        if hi - lo > WINDOW_MAX_NODES {
            let mid = first_at(center).clamp(lo, hi);
            let mut l = mid.saturating_sub(WINDOW_MAX_NODES / 2).max(lo);
            let h = (l + WINDOW_MAX_NODES).min(hi);
            l = h.saturating_sub(WINDOW_MAX_NODES).max(lo);
            // A row cut in two is left out.
            let l = if l > 0 && row(l - 1) == row(l) {
                first_after(row(l))
            } else {
                l
            };
            let h = if h < n && h > 0 && row(h - 1) == row(h) {
                first_at(row(h))
            } else {
                h
            };
            lo = l.min(first_at(center - min_span));
            hi = h.max(first_after(center + min_span)).max(lo);
        }
        if lo == 0 && hi == n {
            return GraphWindow {
                whole: true,
                nodes: Vec::new(),
                lo: f64::NEG_INFINITY,
                hi: f64::INFINITY,
                start: true,
                end: true,
                center,
            };
        }
        let mut nodes: Vec<usize> = self.by_row[lo..hi].to_vec();
        nodes.sort_unstable();
        let (lo_row, hi_row) = if hi > lo {
            (row(lo), row(hi - 1))
        } else {
            (center, center)
        };
        GraphWindow {
            whole: false,
            nodes,
            lo: lo_row,
            hi: hi_row,
            start: lo == 0,
            end: hi == n,
            center,
        }
    }
}

/// The lines to draw for a window showing `shown`, each as the `placed` index of the node
/// it leads into: into every node shown, from a node shown to its children outside the
/// window (so lines leaving the window still show where they go), and lines that only
/// pass through the window. Those all start at an ancestor of a node shown: a branch's
/// leaves fill every row between its top and the line's far end, and the window holds
/// whole rows.
fn window_lines(
    t: &DraftTree,
    layout: &CachedLayout,
    window: &GraphWindow,
    shown: &[usize],
) -> Vec<usize> {
    let CachedLayout { placed, index, .. } = layout;
    let in_window = |i: usize| window.whole || window.nodes.binary_search(&i).is_ok();
    let parent = |i: usize| placed[i].parent.and_then(|p| index.get(p).copied());
    let children = |i: usize| -> Vec<usize> {
        t.get(placed[i].id)
            .map(|n| n.children())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| index.get(*c).copied())
            .collect()
    };
    let mut lines: Vec<usize> = Vec::new();
    let mut ancestors: Vec<usize> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for &i in shown {
        if placed[i].parent.is_some() {
            lines.push(i);
        }
        if window.whole {
            continue;
        }
        lines.extend(children(i).into_iter().filter(|&c| !in_window(c)));
        let mut up = parent(i);
        while let Some(j) = up {
            if in_window(j) || !seen.insert(j) {
                break;
            }
            ancestors.push(j);
            up = parent(j);
        }
    }
    for j in ancestors {
        let from = placed[j].row;
        lines.extend(children(j).into_iter().filter(|&c| {
            let to = placed[c].row;
            !in_window(c) && window.overlaps(from.min(to), from.max(to))
        }));
    }
    lines
}

/// The part of the graph that is drawn: `placed` indices (ascending), or the whole tree.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphWindow {
    pub whole: bool,
    nodes: Vec<usize>,
    /// Rows of the first and last node drawn.
    pub lo: f64,
    pub hi: f64,
    /// The window reaches the tree's first / last row.
    pub start: bool,
    pub end: bool,
    /// The row the window was built around.
    pub center: f64,
}

impl GraphWindow {
    /// Whether rows `from..=to` reach into the window.
    pub fn overlaps(&self, from: f64, to: f64) -> bool {
        self.whole || ((self.start || to >= self.lo) && (self.end || from <= self.hi))
    }

    pub fn contains_row(&self, row: f64) -> bool {
        self.overlaps(row, row)
    }

    /// The `placed` indices to draw (all of them for the whole tree).
    pub fn indices(&self, len: usize) -> Vec<usize> {
        if self.whole {
            (0..len).collect()
        } else {
            self.nodes.clone()
        }
    }
}

/// Screens of graph drawn along the axis the tree branches along, the visible one in the
/// middle: sideways top to bottom, up and down left to right. The draft steps always fit,
/// so the other axis is drawn whole.
const WINDOW_SCREENS_SIDE: f64 = 5.0;
const WINDOW_SCREENS_UP_DOWN: f64 = 3.0;
/// Most nodes drawn at once; past that the window shrinks around its centre, but never
/// below the visible screen and one more on each side.
pub const WINDOW_MAX_NODES: usize = 1000;

/// How many rows the window reaches on each side of its centre, and the least it may
/// shrink to.
fn window_span(vertical: bool, view_breadth: f64, geom: &Geom) -> (f64, f64) {
    let screens = if vertical {
        WINDOW_SCREENS_SIDE
    } else {
        WINDOW_SCREENS_UP_DOWN
    };
    let screen = view_breadth.max(100.0) / breadth_pitch(geom);
    (screens * screen / 2.0, 1.5 * screen)
}

/// Pixels between two rows along the branching axis.
fn breadth_pitch(geom: &Geom) -> f64 {
    let at = |row: f64| breadth(geom, row);
    at(1.0) - at(0.0)
}

fn breadth(geom: &Geom, row: f64) -> f64 {
    let (x, y) = geom.center(0, row);
    match geom {
        Geom::Vertical { .. } => x,
        Geom::Horizontal { .. } => y,
    }
}

/// `data-geom`: where row 0 and draft step 0 are and pixels per row and per step, along
/// the branching axis and then the step axis.
fn geom_attr(geom: &Geom) -> String {
    let (x0, y0) = geom.center(0, 0.0);
    let (x1, y1) = geom.center(1, 1.0);
    match geom {
        Geom::Vertical { .. } => format!("{x0} {} {y0} {}", x1 - x0, y1 - y0),
        Geom::Horizontal { .. } => format!("{y0} {} {x0} {}", y1 - y0, x1 - x0),
    }
}

/// The graph's geometry for a direction, zoom and view size (see [`Geom`]).
pub fn graph_geom(
    vertical: bool,
    zoom: f64,
    view_width: f64,
    view_height: f64,
    root_row: f64,
) -> Geom {
    if vertical {
        Geom::vertical(zoom)
    } else {
        Geom::horizontal(zoom, view_width)
    }
    .centered(root_row, view_width, view_height)
}

/// The nodes whose square touches the rectangle (canvas pixels): a selection box. Uses
/// the layout rather than the page, so nodes outside the drawn window count too.
pub fn nodes_in_rect(
    layout: &CachedLayout,
    geom: &Geom,
    (x0, y0, x1, y1): (f64, f64, f64, f64),
) -> Vec<NodeId> {
    let half = geom.node() / 2.0;
    let (left, right) = (x0.min(x1), x0.max(x1));
    let (top, bottom) = (y0.min(y1), y0.max(y1));
    layout
        .placed
        .iter()
        .filter(|p| {
            let (cx, cy) = geom.center(p.col, p.row);
            cx + half >= left && cx - half <= right && cy + half >= top && cy - half <= bottom
        })
        .map(|p| p.id)
        .collect()
}

/// What one graph node shows.
#[derive(Clone, PartialEq)]
struct NodeView {
    id: NodeId,
    class: String,
    style: String,
    title: String,
    depth: usize,
    value: u8,
    roles: u8,
    is_root: bool,
    empty_number: Option<usize>,
    /// Hidden branches: how many nodes they hold.
    hidden_count: Option<usize>,
    star: bool,
}

/// The debug tooltip of a node (Tooltips on).
fn node_tooltip(
    t: &DraftTree,
    p: &Placed,
    conflicts: &SecondaryMap<NodeId, Conflict>,
    unreachable: &SecondaryMap<NodeId, Tone>,
) -> String {
    let id = p.id;
    let node = t.get(id).unwrap();
    let value = node.value();
    let name = if p.parent.is_none() {
        "Root (empty draft)".to_string()
    } else if let Some(g) = t.groups().get(value) {
        group_tooltip(g)
    } else {
        value_name(t.groups(), value)
    };
    let note = match conflicts.get(id) {
        Some(Conflict::Root(cause)) => {
            let why = match *cause {
                Cause::Repeats(at) => format!(
                    "{} is already on this line at {}",
                    value_name(t.groups(), value),
                    step_label(t, at)
                ),
                Cause::GroupFull(g) => format!(
                    "every champion of {} is already accounted for on this line",
                    value_name(t.groups(), g)
                ),
                Cause::Unknown => "this group is not defined any more".to_string(),
            };
            format!(
                "\nConflict: {why}. Change this node's value{}, change the value above, or remove the branch.",
                crate::keys::hint_suffix(crate::keys::Action::SetValue)
            )
        }
        Some(Conflict::Below) => {
            "\nIn a conflicting branch: nothing new can be added below it".to_string()
        }
        None if unreachable.contains_key(id) => {
            "\nUnreachable: fearless bans rule this branch out".to_string()
        }
        None => String::new(),
    };
    format!(
        "{name}{note}\nid {:?}\nposition {:08b}\nvalue {value}\nchildren {}\nroles {}\n\nClick: go to this state\nMiddle-click: role tags (picks)\nRight-click: delete this subtree",
        id,
        node.pos().bits(),
        node.children().len(),
        crate::roles::describe(node.roles())
    )
}

/// One line of the graph, with its glow for unreachable and conflicting branches. Its own
/// component, so a render only touches the lines that changed.
#[component]
fn GraphEdge(
    d: String,
    class: String,
    glow: Option<&'static str>,
    /// The draft step the line leads into: the selected path draws in step by step.
    depth: usize,
    /// The smooth animation profile, and whether nodes glide this render (see `TreeView`).
    smooth: bool,
    glide: bool,
) -> Element {
    // A line whose nodes glide fades back in where the webview can't move it along with
    // them (CSS `d` transitions, see style.css): `moved-a` / `moved-b` alternate, so each
    // move starts the fade again. Nothing until the line first moves.
    let moves = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new((d.clone(), None::<bool>))));
    let moved = {
        let mut m = moves.borrow_mut();
        if glide && m.0 != d {
            m.1 = Some(!m.1.unwrap_or(false));
        }
        m.0 = d.clone();
        // Kept between moves, so other renders don't start the fade again.
        match m.1 {
            Some(true) => "moved-a",
            Some(false) => "moved-b",
            None => "",
        }
    };
    let length = smooth.then_some("1");
    let step = smooth.then(|| format!("--depth: {depth};"));
    rsx! {
        g { class: moved,
            if let Some(tone) = glow {
                path { d: "{d}", class: "edge-glow {tone}", "pathLength": length }
            }
            path { d: "{d}", class: "{class}", "pathLength": length, style: step }
        }
    }
}

/// One graph node. Its own component, so it only re-renders when what it shows changes.
#[component]
fn GraphNode(
    view: NodeView,
    on_select: EventHandler<NodeId>,
    on_toggle: EventHandler<NodeId>,
    on_hover: EventHandler<Option<(NodeId, f64, f64)>>,
    on_roles: EventHandler<(NodeId, f64, f64)>,
    on_delete: EventHandler<NodeId>,
) -> Element {
    let NodeView {
        id,
        class,
        style,
        title,
        depth,
        value,
        roles,
        is_root,
        empty_number,
        hidden_count,
        star,
    } = view;
    // A node the window drops while the pointer is on it never sees the pointer leave:
    // it ends its preview itself.
    let mut hovered = use_signal(|| false);
    use_drop(move || {
        if *hovered.peek() {
            on_hover.call(None);
        }
    });
    rsx! {
        div {
            class: "{class}",
            style: "{style}",
            title: "{title}",
            "data-node": "{crate::drag::node_attr(id)}",
            "data-depth": "{depth}",
            onclick: move |e: MouseEvent| {
                let m = e.modifiers();
                if m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META) {
                    on_toggle.call(id);
                } else {
                    on_select.call(id);
                }
            },
            onmouseenter: move |e: MouseEvent| {
                let p = e.client_coordinates();
                hovered.set(true);
                on_hover.call(Some((id, p.x, p.y)));
            },
            onmouseleave: move |_| {
                hovered.set(false);
                on_hover.call(None);
            },
            onmousedown: move |e: MouseEvent| {
                if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                    e.prevent_default();
                    let p = e.client_coordinates();
                    on_roles.call((id, p.x, p.y));
                }
            },
            oncontextmenu: move |e| {
                // Suppress the webview's own context menu.
                e.prevent_default();
                on_delete.call(id);
            },
            if hidden_count.is_some() {
                // The attached design: four satellites on the diagonals, joined
                // to the round node by short stubs (viewBox cut to that part).
                svg {
                    class: "collapse-mark",
                    view_box: "150 150 200 200",
                    for (x1, y1, x2, y2, cx, cy) in COLLAPSE_SATELLITES {
                        line { x1: "{x1}", y1: "{y1}", x2: "{x2}", y2: "{y2}" }
                        circle { cx: "{cx}", cy: "{cy}", r: "15" }
                    }
                }
            }
            if is_root {
                span { "●" }
            } else {
                ValueIcon { value }
                RoleIcons { mask: roles }
                if let Some(n) = empty_number {
                    span { class: "empty-no", title: "Empty branch {n} of this state", "{n}" }
                }
            }
            if let Some(n) = hidden_count {
                span { class: "hidden-count", title: "{n} hidden nodes", "{n}" }
            }
            if star {
                span { class: "node-star", title: "On a marked line", "★" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draft_has_four_phases() {
        let p = phases();
        let spans: Vec<(usize, usize, Action, bool)> = p
            .iter()
            .map(|p| (p.first, p.last, p.action, p.opening))
            .collect();
        assert_eq!(
            spans,
            [
                (0, 5, Action::Ban, true),
                (6, 11, Action::Pick, true),
                (12, 15, Action::Ban, false),
                (16, 19, Action::Pick, false),
            ]
        );
        assert_eq!(p[0].class(), "phase-ban phase-1");
        assert_eq!(p[3].class(), "phase-pick phase-2");
    }

    /// A complete binary tree `depth` steps deep.
    fn binary_tree(depth: u8) -> DraftTree {
        let mut t = DraftTree::new();
        let mut level = vec![t.root()];
        for d in 0..depth {
            let mut next = Vec::new();
            for &n in &level {
                for v in [16 + 2 * d, 17 + 2 * d] {
                    next.push(t.add_child(n, v).unwrap());
                }
            }
            level = next;
        }
        t
    }

    /// A binary tree `depth` levels deep with a straight line `tail` nodes long under
    /// each leaf: the nodes of a line share a row.
    fn tree_with_lines(depth: u8, tail: u8) -> DraftTree {
        let mut t = binary_tree(depth);
        let leaves: Vec<NodeId> = t
            .nodes_iter()
            .filter(|(id, _)| t.get(*id).is_some_and(|n| n.children().is_empty()))
            .map(|(id, _)| id)
            .collect();
        for leaf in leaves {
            let mut at = leaf;
            for d in depth..depth + tail {
                at = t.add_child(at, 16 + 2 * d).unwrap();
            }
        }
        t
    }

    #[test]
    fn edges_are_braces_except_straight_down() {
        let node = 28.0;
        // In line: one straight segment.
        let straight = edge_segments((100.0, 50.0), (100.0, 94.0), node, true);
        assert_eq!(straight.len(), 1);
        assert!(straight[0].iter().all(|p| p.0 == 100.0));
        for to in [(30.0, 94.0), (180.0, 94.0)] {
            let segs = edge_segments((100.0, 50.0), to, node, true);
            assert!(segs.len() > 1);
            // From centre to centre, each segment starting where the one before ends.
            assert_eq!(segs[0][0], (100.0, 50.0));
            assert_eq!(segs.last().unwrap()[3], to);
            for w in segs.windows(2) {
                assert_eq!(w[0][3], w[1][0]);
            }
            // Out of the parent and into the child straight along the draft steps, and
            // never past either of them.
            assert_eq!(segs[0][3].0, 100.0);
            assert_eq!(segs.last().unwrap()[0].0, to.0);
            for seg in &segs {
                for p in seg {
                    assert!((50.0..=94.0).contains(&p.1), "{p:?}");
                }
            }
        }
        // Siblings on one side share the curl out of the parent and the bar.
        let near = edge_segments((100.0, 50.0), (136.0, 94.0), node, true);
        let far = edge_segments((100.0, 50.0), (300.0, 94.0), node, true);
        assert_eq!(near[..2], far[..2]);
        assert_eq!(near[2][0].1, far[2][0].1);
        // Left to right is the same shape, turned.
        let turned = edge_segments((50.0, 100.0), (94.0, 30.0), node, false);
        let down = edge_segments((100.0, 50.0), (30.0, 94.0), node, true);
        let swap = |p: (f64, f64)| (p.1, p.0);
        assert_eq!(turned, down.iter().map(|s| s.map(swap)).collect::<Vec<_>>());
    }

    #[test]
    fn small_trees_are_drawn_whole() {
        let t = binary_tree(6);
        let layout = CachedLayout::compute(&t, false);
        let w = layout.window(0.0, 1000.0, 10.0);
        assert!(w.whole && w.contains_row(-1e9) && w.contains_row(1e9));
        assert_eq!(w.indices(layout.placed.len()).len(), layout.placed.len());
    }

    #[test]
    fn large_trees_are_drawn_in_a_window() {
        let t = binary_tree(14);
        let layout = CachedLayout::compute(&t, false);
        let n = layout.placed.len();
        assert!(n > WINDOW_MAX_NODES);
        // A narrow window: only nodes within the span, the centre covered.
        let center = layout.rows as f64 / 3.0;
        let w = layout.window(center, 40.0, 40.0);
        assert!(!w.whole && !w.start && !w.end && w.contains_row(center));
        let drawn = w.indices(n);
        assert!(!drawn.is_empty() && drawn.len() < n);
        assert!(drawn.windows(2).all(|p| p[0] < p[1]), "in layout order");
        for &i in &drawn {
            assert!((layout.placed[i].row - center).abs() <= 40.0 + 1e-9);
        }
        // Every node inside the covered rows is drawn.
        let covered = layout
            .placed
            .iter()
            .filter(|p| w.lo <= p.row && p.row <= w.hi)
            .count();
        assert_eq!(covered, drawn.len());
        // A huge span is cut to the node budget, still around the centre.
        let w = layout.window(center, 1e9, 0.0);
        assert!(!w.whole && w.contains_row(center));
        let drawn = w.indices(n).len();
        assert!(drawn <= WINDOW_MAX_NODES && drawn > WINDOW_MAX_NODES - 30);
        // Near the tree's start the window says it reaches it, with real numbers.
        let w = layout.window(0.0, 40.0, 40.0);
        assert!(w.start && !w.end && w.lo.is_finite() && w.hi.is_finite());
        assert!(w.contains_row(-5.0) && !w.contains_row(w.hi + 1.0));
    }

    #[test]
    fn capped_windows_hold_whole_rows_and_the_view() {
        let t = tree_with_lines(9, 10);
        let layout = CachedLayout::compute(&t, false);
        let n = layout.placed.len();
        for center in [3.0, 100.5, 255.0, 400.25, 511.0] {
            // Cut to the budget: whatever rows it covers, it draws all of their nodes.
            let w = layout.window(center, 1e9, 0.0);
            let drawn = w.indices(n);
            assert!(drawn.len() <= WINDOW_MAX_NODES, "{center}");
            let covered = layout
                .placed
                .iter()
                .filter(|p| w.contains_row(p.row))
                .count();
            assert_eq!(covered, drawn.len(), "{center}");
            // Never less than the rows the view needs, whatever the budget.
            let w = layout.window(center, 1e9, 150.0);
            let drawn = w.indices(n);
            assert!(drawn.len() > WINDOW_MAX_NODES, "{center}");
            for p in &layout.placed {
                if (p.row - center).abs() <= 150.0 {
                    assert!(w.contains_row(p.row), "{center}: row {}", p.row);
                }
            }
        }
    }

    #[test]
    fn lines_crossing_the_window_are_drawn() {
        for t in [binary_tree(13), tree_with_lines(9, 10)] {
            let layout = CachedLayout::compute(&t, false);
            let n = layout.placed.len();
            for center in [0.0, 37.5, 300.0, 1000.0, layout.rows as f64] {
                let w = layout.window(center, 30.0, 30.0);
                let shown = w.indices(n);
                let mut lines = window_lines(&t, &layout, &w, &shown);
                let count = lines.len();
                lines.sort_unstable();
                lines.dedup();
                assert_eq!(lines.len(), count, "each line once");
                let drawn = |i: usize| shown.binary_search(&i).is_ok();
                let expected: Vec<usize> = (0..n)
                    .filter(|&i| {
                        let Some(p) = layout.placed[i].parent else {
                            return false;
                        };
                        let j = layout.index[p];
                        let (a, b) = (layout.placed[i].row, layout.placed[j].row);
                        drawn(i) || drawn(j) || w.overlaps(a.min(b), a.max(b))
                    })
                    .collect();
                assert_eq!(lines, expected, "{center}");
            }
        }
    }

    #[test]
    fn selection_boxes_use_the_layout() {
        let t = binary_tree(5);
        let layout = CachedLayout::compute(&t, false);
        let geom = graph_geom(true, 1.0, 800.0, 600.0, layout.placed[0].row);
        let half = geom.node() / 2.0;
        let rect = (100.0, 0.0, 400.0, 300.0);
        let expected: Vec<NodeId> = layout
            .placed
            .iter()
            .filter(|p| {
                let (x, y) = geom.center(p.col, p.row);
                x + half >= 100.0 && x - half <= 400.0 && y + half >= 0.0 && y - half <= 300.0
            })
            .map(|p| p.id)
            .collect();
        assert!(!expected.is_empty() && expected.len() < layout.placed.len());
        assert_eq!(nodes_in_rect(&layout, &geom, rect), expected);
        // Corners in any order.
        assert_eq!(
            nodes_in_rect(&layout, &geom, (400.0, 300.0, 100.0, 0.0)),
            expected
        );
    }

    #[test]
    fn linear_draft_is_a_straight_line() {
        let mut tree = DraftTree::new();
        let mut cursor = tree.root();
        for v in 16..26 {
            cursor = tree.add_child(cursor, v).unwrap();
        }
        let (placed, rows) = layout(&tree, &SecondaryMap::new());
        assert_eq!(rows, 1);
        assert_eq!(placed.len(), 11);
        for (i, p) in placed.iter().enumerate() {
            assert_eq!((p.col, p.row), (i, 0.0));
        }
    }

    #[test]
    fn parents_are_centred_over_their_children() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.add_child(root, 16).unwrap();
        let b = tree.add_child(root, 17).unwrap();
        let c = tree.add_child(root, 18).unwrap();
        let a1 = tree.add_child(a, 20).unwrap();
        let a2 = tree.add_child(a, 21).unwrap();

        let (placed, rows) = layout(&tree, &SecondaryMap::new());
        let row_of = |id| placed.iter().find(|p| p.id == id).unwrap().row;
        assert_eq!(rows, 4);
        assert_eq!((row_of(a1), row_of(a2)), (0.0, 1.0));
        assert_eq!(row_of(a), 0.5);
        assert_eq!(row_of(b), 2.0);
        assert_eq!(row_of(c), 3.0);
        assert_eq!(row_of(root), (0.5 + 3.0) / 2.0);
        // Children keep insertion order top to bottom.
        assert!(row_of(b) < row_of(c));
    }

    #[test]
    fn top_to_bottom_sizes_follow_the_zoom() {
        let g = Geom::vertical(1.0);
        let Geom::Vertical {
            level,
            node,
            pitch,
            shift,
        } = g
        else {
            panic!()
        };
        assert_eq!((node, level, shift), (28.0, 44.0, 0.0));
        assert!(pitch > node);
        // The height is the same for any tree: every level, scrolled when needed.
        assert_eq!(g.size(3, 1).1, g.size(20, 40).1);
        // Zooming scales nodes and rows; the label column stays.
        let big = Geom::vertical(2.0);
        assert_eq!(big.node(), 56.0);
        assert!(big.size(20, 1).1 > 1.9 * g.size(20, 1).1);
        assert_eq!(Geom::vertical(0.3).node(), 8.0);
        // Leaves go left to right, depth goes down.
        let (x0, y0) = g.center(3, 0.0);
        let (x1, y1) = g.center(3, 1.0);
        assert_eq!((x1 - x0, y0), (pitch, y1));
        assert!(g.center(4, 0.0).1 > y0);
        assert!(g.size(20, 10).0 > g.size(20, 2).0);
    }

    #[test]
    fn left_to_right_fits_the_width_at_100_percent() {
        // A wide panel: full size, never larger.
        let wide = Geom::horizontal(1.0, 3000.0);
        assert_eq!(wide.node(), NODE_SIZE);
        // A narrow panel: every step fits its width.
        let narrow = Geom::horizontal(1.0, 700.0);
        assert!(narrow.size(20, 1).0 <= 700.0 - 2.0 + 1.0);
        assert!(narrow.node() < NODE_SIZE);
        // Zooming in goes past the fit, and scrolls.
        let zoomed = Geom::horizontal(2.0, 700.0);
        assert!(zoomed.size(20, 1).0 > 700.0);
    }

    #[test]
    fn the_root_is_centred_when_there_is_room() {
        let g = Geom::vertical(1.0);
        // A narrow tree in a wide panel: the root lands in the middle.
        let c = g.centered(1.0, 1000.0, 400.0);
        assert!((c.center(0, 1.0).0 - 500.0).abs() <= 1.0);
        assert!(c.size(20, 3).0 > g.size(20, 3).0);
        // A root far right in a wide tree stays put (nothing moves left).
        let wide = g.centered(30.0, 600.0, 400.0);
        assert_eq!(wide, g);
        // Left to right the root lands halfway down the panel, at any zoom.
        for (zoom, width) in [(1.0, 1400.0), (1.0, 670.0), (0.5, 1400.0)] {
            let h = Geom::horizontal(zoom, width).centered(0.0, width, 500.0);
            assert!((h.center(0, 0.0).1 - 250.0).abs() <= 1.0, "{zoom} {width}");
        }
        // A root already below the middle stays put.
        let low = Geom::horizontal(1.0, 1400.0);
        assert_eq!(low.centered(20.0, 1400.0, 300.0), low);
    }
}
