//! A frozen copy of the part of the tree being exported.
//!
//! Exports run on background threads while the user keeps editing, so they work on this
//! snapshot rather than on the live [`DraftTree`]. Nodes are stored in pre-order (parents
//! before children, siblings top to bottom as in the tree view) and refer to each other by
//! index.

use std::collections::HashMap;
use std::sync::Arc;

use crate::draft::{Action, DRAFT_ORDER, DraftPos, Team};
use crate::groups::{self, Group, GroupCatalog};
use crate::tree::{DraftTree, NodeId};

use crate::fearless::Fearless;

use super::scene::{Color, Tone};

#[derive(Clone, Debug)]
pub struct XNode {
    pub pos: DraftPos,
    pub value: u8,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Draft actions taken: 0 for the root, 20 after the last pick.
    pub depth: usize,
    /// Set when fearless bans make this node unreachable: the tone to draw it in.
    pub pruned: Option<Tone>,
    /// Set for a node in a conflicting branch (see `DraftTree::conflicts`): `true` where
    /// the line first breaks, `false` below it.
    pub conflict: Option<bool>,
    /// Role tags (`0` = any role).
    pub roles: u8,
    /// On a marked line (Ctrl+M): the node is marked or leads to a marked node.
    pub starred: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Pre-order; `nodes[0]` is the root.
    pub nodes: Vec<XNode>,
    pub groups: Arc<GroupCatalog>,
    colors: HashMap<u8, usize>,
    /// Index of the node the export is scoped to (`0` for the whole tree). Its ancestors
    /// are kept as a single line leading to it.
    pub scope_root: usize,
    /// Highlighted nodes for the tree image, with their line colors.
    pub highlights: Vec<(usize, Color)>,
    pub fearless: Fearless,
    /// Group colors of the app's theme, so exports match what is on screen.
    pub group_colors: Vec<String>,
}

/// Colors for highlighted lines in the tree image, in the order nodes are added.
pub const HIGHLIGHT_COLORS: [&str; 8] = [
    "#059669", "#a16207", "#ea580c", "#0891b2", "#db2777", "#65a30d", "#475569", "#e11d48",
];

pub fn highlight_color(i: usize) -> Color {
    Color::hex(HIGHLIGHT_COLORS[i % HIGHLIGHT_COLORS.len()])
}

impl Snapshot {
    /// Copies `tree`, limited to `scope` (its subtree plus the line leading to it) when
    /// given. `highlights` outside the copied part are dropped. With `hide_unreachable`,
    /// branches that fearless bans rule out are left out, and without `show_conflicts`
    /// conflicting branches are (a scope inside a left-out branch falls back to its nearest
    /// ancestor that is kept).
    pub fn build(
        tree: &DraftTree,
        scope: Option<NodeId>,
        colors: &HashMap<u8, usize>,
        highlights: &[NodeId],
        hide_unreachable: bool,
        show_conflicts: bool,
    ) -> Snapshot {
        let marks = tree.marks();
        let stars = tree.starred();
        let pruned = |id: NodeId| marks.unreachable.get(id).map(|&t| Tone::from_fearless(t));
        let conflict = |id: NodeId| {
            marks
                .conflicts
                .get(id)
                .map(|c| matches!(c, crate::tree::Conflict::Root(_)))
        };
        let skip = |id: NodeId| {
            (hide_unreachable && marks.unreachable.contains_key(id))
                || (!show_conflicts && marks.conflicts.contains_key(id))
        };
        let scope = scope.filter(|&s| tree.contains(s)).unwrap_or(tree.root());
        let scope = tree
            .ancestors(scope)
            .find(|&n| !skip(n))
            .unwrap_or(tree.root());
        let path = tree.path(scope);
        let mut nodes: Vec<XNode> = Vec::new();
        let mut index_of: HashMap<NodeId, usize> = HashMap::new();

        // The line from the root down to the scope node: one child each.
        for (depth, &id) in path.iter().enumerate() {
            let node = tree.get(id).expect("path nodes exist");
            let index = nodes.len();
            let parent = index.checked_sub(1);
            if let Some(p) = parent {
                nodes[p].children.push(index);
            }
            nodes.push(XNode {
                pos: node.pos(),
                value: node.value(),
                parent,
                children: Vec::new(),
                depth,
                pruned: pruned(id),
                conflict: conflict(id),
                roles: node.roles(),
                starred: stars.contains_key(id),
            });
            index_of.insert(id, index);
        }
        let scope_root = nodes.len() - 1;

        // The scope node's subtree, depth first so siblings stay in order.
        let mut stack: Vec<(NodeId, usize)> = tree
            .get(scope)
            .map(|n| {
                n.children()
                    .iter()
                    .rev()
                    .map(|&c| (c, scope_root))
                    .collect()
            })
            .unwrap_or_default();
        while let Some((id, parent)) = stack.pop() {
            if skip(id) {
                continue;
            }
            let node = tree.get(id).expect("child ids are valid");
            let index = nodes.len();
            nodes[parent].children.push(index);
            nodes.push(XNode {
                pos: node.pos(),
                value: node.value(),
                parent: Some(parent),
                children: Vec::new(),
                depth: nodes[parent].depth + 1,
                pruned: pruned(id),
                conflict: conflict(id),
                roles: node.roles(),
                starred: stars.contains_key(id),
            });
            index_of.insert(id, index);
            stack.extend(node.children().iter().rev().map(|&c| (c, index)));
        }

        let highlights = highlights
            .iter()
            .enumerate()
            .filter_map(|(i, id)| Some((*index_of.get(id)?, highlight_color(i))))
            .collect();
        Snapshot {
            nodes,
            groups: tree.groups().clone(),
            colors: colors.clone(),
            scope_root,
            highlights,
            fearless: *tree.fearless(),
            group_colors: crate::theme::DEFAULT_GROUP_COLORS
                .iter()
                .map(|c| c.to_string())
                .collect(),
        }
    }

    /// Values from the first action down to `index`, one per draft step.
    pub fn line_values(&self, index: usize) -> Vec<u8> {
        let mut values = Vec::with_capacity(self.nodes[index].depth);
        let mut at = Some(index);
        while let Some(i) = at {
            if i != 0 {
                values.push(self.nodes[i].value);
            }
            at = self.nodes[i].parent;
        }
        values.reverse();
        values
    }

    /// For the draft state at `index`, per team (blue first) and role (top ... support),
    /// the values its role slot shows, as in the app's side panels.
    pub fn role_slots(&self, index: usize) -> [Vec<Vec<u8>>; 2] {
        let mut teams: [Vec<(u8, u8)>; 2] = [Vec::new(), Vec::new()];
        for i in self.path(index).into_iter().skip(1) {
            let n = &self.nodes[i];
            if n.pos.action() == crate::draft::Action::Pick {
                let team = usize::from(n.pos.team() == crate::draft::Team::Red);
                teams[team].push((n.value, n.roles));
            }
        }
        teams.map(|picks| {
            let masks: Vec<u8> = picks.iter().map(|&(_, m)| m).collect();
            let candidates = crate::roles::candidates(&masks);
            crate::roles::Role::ALL
                .iter()
                .map(|role| {
                    picks
                        .iter()
                        .zip(&candidates)
                        .filter(|&(&(v, m), &c)| {
                            c & role.bit() != 0 && (v != crate::tree::EMPTY || m != 0)
                        })
                        .map(|(&(v, _), _)| v)
                        .collect()
                })
                .collect()
        })
    }

    /// Nodes `index` and its ancestors.
    pub fn path(&self, index: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut at = Some(index);
        while let Some(i) = at {
            path.push(i);
            at = self.nodes[i].parent;
        }
        path.reverse();
        path
    }

    /// Nodes in scope at `depth` (1-20), top to bottom. Nodes on the line leading to the
    /// scope node only count from the scope node down.
    pub fn at_depth(&self, depth: usize) -> Vec<usize> {
        let min = self.nodes[self.scope_root].depth;
        if depth < min.max(1) {
            return Vec::new();
        }
        if depth == min {
            return vec![self.scope_root];
        }
        (self.scope_root..self.nodes.len())
            .filter(|&i| self.nodes[i].depth == depth)
            .collect()
    }

    /// Number of draft lines (leaves).
    pub fn lines(&self) -> usize {
        self.nodes.iter().filter(|n| n.children.is_empty()).count()
    }

    /// Deepest depth in the snapshot.
    pub fn max_depth(&self) -> usize {
        self.nodes.iter().map(|n| n.depth).max().unwrap_or(0)
    }

    /// Groups used by `nodes`, in catalog order (presets first, then by value).
    pub fn groups_in(&self, nodes: impl IntoIterator<Item = usize>) -> Vec<&Group> {
        let mut used = [false; 256];
        for i in nodes {
            used[usize::from(self.nodes[i].value)] = true;
        }
        self.groups
            .all()
            .iter()
            .filter(|g| used[usize::from(g.value)])
            .collect()
    }

    /// Every group in the snapshot.
    pub fn used_groups(&self) -> Vec<&Group> {
        self.groups_in(0..self.nodes.len())
    }

    /// The group's highlight color, as in the app.
    pub fn group_color(&self, value: u8) -> Color {
        let i = self.colors.get(&value).copied().unwrap_or_else(|| {
            // Not shown in the app yet: continue the palette in value order.
            self.colors.len() + usize::from(value)
        });
        let hex = match self.group_colors.get(i) {
            Some(c) => c.clone(),
            None => crate::theme::extra_group_color(i - self.group_colors.len()),
        };
        Color::hex(&hex)
    }

    pub fn is_group(&self, value: u8) -> bool {
        groups::is_group(value)
    }
}

/// `A06`: the draft action number, so names sort in draft order.
pub fn action_code(depth: usize) -> String {
    format!("A{depth:02}")
}

/// `RB3` for red ban 3.
pub fn pos_code(pos: DraftPos) -> String {
    let team = match pos.team() {
        Team::Blue => 'B',
        Team::Red => 'R',
    };
    let action = match pos.action() {
        Action::Ban => 'B',
        Action::Pick => 'P',
    };
    format!("{team}{action}{}", pos.number())
}

/// `A06-RB3` for the sixth action.
pub fn depth_code(depth: usize) -> String {
    let pos = DRAFT_ORDER[depth - 1].pos();
    format!("{}-{}", action_code(depth), pos_code(pos))
}

/// `red ban 3`.
pub fn pos_long(pos: DraftPos) -> String {
    format!(
        "{} {} {}",
        pos.team().label().to_lowercase(),
        pos.action().label().to_lowercase(),
        pos.number()
    )
}

/// `After red ban 3`.
pub fn depth_title(depth: usize) -> String {
    format!("After {}", pos_long(DRAFT_ORDER[depth - 1].pos()))
}

/// Zero-padded sequence number, at least four digits.
pub fn seq_text(seq: usize, count: usize) -> String {
    let digits = count.to_string().len().max(4);
    format!("{seq:0digits$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// root ─ a(16) ─ a1(20)
    ///      │       └ a2(21) ─ a21(30)
    ///      └ b(17)
    fn sample() -> (DraftTree, [NodeId; 5]) {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        let b = t.add_child(root, 17).unwrap();
        let a1 = t.add_child(a, 20).unwrap();
        let a2 = t.add_child(a, 21).unwrap();
        let a21 = t.add_child(a2, 30).unwrap();
        let _ = b;
        (t, [a, b, a1, a2, a21])
    }

    #[test]
    fn whole_tree_in_pre_order() {
        let (t, [_, _, _, _, a21]) = sample();
        let s = Snapshot::build(&t, None, &HashMap::new(), &[a21], false, true);
        let values: Vec<u8> = s.nodes.iter().map(|n| n.value).collect();
        assert_eq!(values, [0, 16, 20, 21, 30, 17]);
        assert_eq!(s.lines(), 3);
        assert_eq!(s.at_depth(1), [1, 5]);
        assert_eq!(s.at_depth(2), [2, 3]);
        assert_eq!(s.line_values(4), [16, 21, 30]);
        assert_eq!(s.highlights.len(), 1);
        assert_eq!(s.highlights[0].0, 4);
        assert_eq!(s.max_depth(), 3);
    }

    #[test]
    fn subtree_keeps_the_line_leading_to_it() {
        let (t, [a, b, _, a2, _]) = sample();
        let s = Snapshot::build(&t, Some(a2), &HashMap::new(), &[b, a], false, true);
        let values: Vec<u8> = s.nodes.iter().map(|n| n.value).collect();
        assert_eq!(values, [0, 16, 21, 30]);
        assert_eq!(s.scope_root, 2);
        // Only the scope node and below count as states.
        assert!(s.at_depth(1).is_empty());
        assert_eq!(s.at_depth(2), [2]);
        assert_eq!(s.at_depth(3), [3]);
        // `b` is outside the scope; `a` is on the leading line and keeps its color index.
        assert_eq!(s.highlights, [(1, highlight_color(1))]);
    }

    #[test]
    fn codes_and_titles() {
        assert_eq!(depth_code(1), "A01-BB1");
        assert_eq!(depth_code(6), "A06-RB3");
        assert_eq!(depth_code(20), "A20-RP5");
        assert_eq!(depth_title(9), "After red pick 2");
        assert_eq!(seq_text(14, 300), "0014");
        assert_eq!(seq_text(14, 12_000), "00014");
    }
}
