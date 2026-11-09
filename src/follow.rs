//! Follow mode (Ctrl+Shift+L): during a real draft, each action is entered as it happens and the
//! app follows the prepared branches that fit it.
//!
//! Starting from the start node, every entered value is matched against the children of
//! the nodes that still fit: a child holding that champion matches best, then a group
//! holding it, then an empty pick or skipped ban. All matching children are followed at
//! once, best first. Branches that fearless rules out or that conflict never match. When
//! nothing matches, the draft has left the prepared tree; entering more actions still
//! records them, but they match nothing. Follow mode never changes the tree.

use crate::draft::DRAFT_ORDER;
use crate::groups;
use crate::tree::{DraftTree, EMPTY, NO_BAN, NodeId};

/// How a prepared node fits an entered value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// The same champion (or group, or empty value).
    Exact,
    /// A group holding the champion.
    Group,
    /// An empty pick or skipped ban, which stands for anything.
    Wildcard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    /// Values entered so far, one per draft step.
    pub entered: Vec<u8>,
    /// For the start and each entered step, the nodes that still fit, best first. Empty
    /// once the draft has left the prepared tree.
    pub sets: Vec<Vec<NodeId>>,
    /// The user chose to keep following after leaving the prepared tree, so the warning
    /// stays hidden.
    pub keep_going: bool,
}

fn is_wildcard(value: u8) -> bool {
    value == EMPTY || value == NO_BAN
}

/// How the node value `node` fits the entered value `entered`, if at all.
pub fn fit(tree: &DraftTree, node: u8, entered: u8) -> Option<Fit> {
    if node == entered || (is_wildcard(node) && is_wildcard(entered)) {
        return Some(Fit::Exact);
    }
    if is_wildcard(node) {
        return Some(Fit::Wildcard);
    }
    if groups::is_group(node)
        && !groups::is_group(entered)
        && !is_wildcard(entered)
        && tree.groups().get(node).is_some_and(|g| g.contains(entered))
    {
        return Some(Fit::Group);
    }
    None
}

impl Follow {
    pub fn start(tree: &DraftTree) -> Follow {
        Follow {
            entered: Vec::new(),
            sets: vec![vec![tree.root()]],
            keep_going: false,
        }
    }

    /// The draft step to enter next (0 = Blue Ban 1), or `None` after the last pick.
    pub fn step(&self) -> Option<usize> {
        (self.entered.len() < DRAFT_ORDER.len()).then_some(self.entered.len())
    }

    /// The nodes that fit the draft so far, best first (empty when off the tree).
    pub fn current(&self) -> &[NodeId] {
        self.sets.last().map_or(&[], Vec::as_slice)
    }

    /// The deepest node followed: the best fit at the last step anything fitted.
    pub fn last_followed(&self) -> Option<NodeId> {
        self.sets.iter().rev().find_map(|s| s.first().copied())
    }

    /// The first step (0 = Blue Ban 1) where nothing fitted, if the draft left the tree.
    pub fn off_tree_at(&self) -> Option<usize> {
        self.sets.iter().position(Vec::is_empty).map(|i| i - 1)
    }

    /// Whether `value` can be entered next: not after the last pick, and not a champion
    /// already on the line.
    pub fn can_enter(&self, value: u8) -> bool {
        self.step().is_some()
            && (is_wildcard(value) || groups::is_group(value) || !self.entered.contains(&value))
    }

    /// Enters the next action. Returns whether anything in the tree fits it.
    pub fn enter(&mut self, tree: &DraftTree, value: u8) -> bool {
        if !self.can_enter(value) {
            return !self.current().is_empty();
        }
        let next = matches(tree, self.current(), value);
        let found = !next.is_empty();
        self.entered.push(value);
        self.sets.push(next);
        found
    }

    /// Takes back the last entered action.
    pub fn back(&mut self) -> bool {
        if self.entered.pop().is_none() {
            return false;
        }
        self.sets.pop();
        if self.off_tree_at().is_none() {
            self.keep_going = false;
        }
        true
    }

    /// Every node on a line that still fits (from the start down to the nodes fitting the
    /// last step that anything fitted), plus the nodes prepared for the next step.
    pub fn followed(&self, tree: &DraftTree) -> Vec<NodeId> {
        let skip = skipped(tree);
        let last = self
            .sets
            .iter()
            .rev()
            .find(|s| !s.is_empty())
            .map_or(&[][..], Vec::as_slice);
        let mut out: Vec<NodeId> = Vec::new();
        for &n in last {
            for p in tree.path(n) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        for &n in self.current() {
            out.extend(
                tree.get(n)
                    .map(|n| n.children())
                    .unwrap_or_default()
                    .iter()
                    .copied()
                    .filter(|&c| !skip(c)),
            );
        }
        out
    }

    /// The values prepared for the next step, best first: each distinct value among the
    /// children of the fitting nodes, once.
    pub fn options(&self, tree: &DraftTree) -> Vec<u8> {
        let skip = skipped(tree);
        let mut out: Vec<u8> = Vec::new();
        for &n in self.current() {
            for &c in tree.get(n).map(|n| n.children()).unwrap_or_default() {
                if skip(c) {
                    continue;
                }
                let v = tree.get(c).expect("live child").value();
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
        out
    }
}

/// Nodes follow mode never enters: fearless rules them out, or their line conflicts.
fn skipped(tree: &DraftTree) -> impl Fn(NodeId) -> bool {
    let marks = tree.marks();
    move |id| marks.unreachable.contains_key(id) || marks.conflicts.contains_key(id)
}

/// The children of `nodes` that fit `value`, best fit first, in tree order within a fit.
pub fn matches(tree: &DraftTree, nodes: &[NodeId], value: u8) -> Vec<NodeId> {
    let skip = skipped(tree);
    let mut found: Vec<(Fit, NodeId)> = Vec::new();
    for &n in nodes {
        for &c in tree.get(n).map(|n| n.children()).unwrap_or_default() {
            if skip(c) {
                continue;
            }
            if let Some(f) = fit(tree, tree.get(c).expect("live child").value(), value) {
                found.push((f, c));
            }
        }
    }
    // Stable: tree order stays within each fit.
    found.sort_by_key(|&(f, _)| f);
    found.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::{Group, GroupCatalog};
    use std::sync::Arc;

    fn champ(name: &str) -> u8 {
        crate::champions::CHAMPIONS
            .iter()
            .position(|c| c.name == name)
            .map(crate::champions::value_of)
            .unwrap()
    }

    #[test]
    fn follows_exact_then_groups_then_wildcards() {
        let (ahri, annie, ashe, zed) = (champ("Ahri"), champ("Annie"), champ("Ashe"), champ("Zed"));
        let mages = Group::new_user(190, "Mages".into(), String::new(), vec![ahri, annie]);
        let mut t =
            DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(vec![mages], None)));
        let root = t.root();
        let skip = t.add_child(root, NO_BAN).unwrap();
        let group = t.add_child(root, 190).unwrap();
        let exact = t.add_child(root, ahri).unwrap();
        let _other = t.add_child(root, ashe).unwrap();
        let below = t.add_child(group, zed).unwrap();

        let mut f = Follow::start(&t);
        assert_eq!(f.options(&t), [NO_BAN, 190, ahri, ashe]);
        assert!(f.enter(&t, ahri));
        assert_eq!(f.current(), [exact, group, skip]);
        assert_eq!(f.last_followed(), Some(exact));
        // A champion already on the line can't be entered again.
        assert!(!f.can_enter(ahri));
        // Zed only follows below the group.
        assert!(f.enter(&t, zed));
        assert_eq!(f.current(), [below]);
        let followed = f.followed(&t);
        assert!(followed.contains(&group) && followed.contains(&root));
        assert!(!followed.contains(&exact));
        // Nothing prepared: off the tree, but the last followed node stays.
        assert!(!f.enter(&t, annie));
        assert_eq!(f.off_tree_at(), Some(2));
        assert_eq!(f.last_followed(), Some(below));
        assert!(!f.enter(&t, ashe));
        assert_eq!(f.off_tree_at(), Some(2));
        // Stepping back returns to the tree.
        f.keep_going = true;
        assert!(f.back() && f.back());
        assert_eq!(f.off_tree_at(), None);
        assert!(!f.keep_going);
        assert_eq!(f.current(), [below]);
        assert!(f.back() && f.back());
        assert!(!f.back());
        assert_eq!(f.current(), [root]);
    }

    #[test]
    fn unreachable_and_conflicting_branches_never_match() {
        let mut t = DraftTree::new();
        let e = t.add_child(t.root(), NO_BAN).unwrap();
        let a = t.add_child(e, 17).unwrap();
        t.add_child(a, 18).unwrap();
        t.add_child(e, 19).unwrap();
        // Changing `e` to 17 makes the 17 child below it conflict.
        t.set_value(e, 17).unwrap();
        let mut f = Follow::start(&t);
        assert!(f.enter(&t, 17));
        assert_eq!(f.options(&t), [19]);
        assert!(!f.enter(&t, 18));
    }

    #[test]
    fn entered_groups_and_wildcards() {
        let mut t = DraftTree::new();
        let root = t.root();
        let g = t.add_child(root, 3).unwrap();
        let w = t.add_child(root, NO_BAN).unwrap();
        let f0 = Follow::start(&t);
        let mut f = f0.clone();
        // A group entered fits that group, then wildcards; not other groups' members.
        assert!(f.enter(&t, 3));
        assert_eq!(f.current(), [g, w]);
        let mut f = f0.clone();
        assert!(f.enter(&t, EMPTY));
        assert_eq!(f.current(), [w]);
        // Groups and wildcards may repeat.
        assert!(f.can_enter(EMPTY) && f.can_enter(3));
    }
}
