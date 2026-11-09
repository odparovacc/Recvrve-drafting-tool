//! Copying branches and putting them somewhere else: Ctrl+C / Ctrl+X / Ctrl+V, and dragging
//! nodes onto other nodes.
//!
//! A [`Clip`] holds copies of one or more branches whose top nodes are all at one depth, with
//! everything below them (values, role tags, marks and hidden branches). It keeps its own
//! group catalog, so it survives loading another tree or another group config.
//!
//! [`paste`] puts the branches below a node one level up from them (ban/pick slots depend on
//! depth, so nothing else fits). Like merging trees, a copied node and a child that holds the
//! same champion or group become one, and empty values match in order among those with the
//! same role tags. New nodes may repeat champions of their new line: they show as
//! conflicting, as after a value change higher up. Role tags that don't fit the new line are
//! dropped.

use std::collections::HashMap;

use crate::groups::{Group, GroupCatalog};
use crate::tree::{DraftTree, EMPTY, NO_BAN, NodeId, TreeError, empty_value_for};

/// Copied branches.
#[derive(Clone, Debug)]
pub struct Clip {
    /// A tree holding a line of empty placeholders down to `holder`, whose children are
    /// the copied branches' top nodes.
    tree: DraftTree,
    holder: NodeId,
    /// Depth of the copied top nodes (the root is 0).
    depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipError {
    /// Nothing to copy (no nodes, or the root).
    Nothing,
    /// The copied nodes are at different depths.
    MixedDepths,
    /// Pasting needs a node one level above the copied nodes: the draft step it must be at
    /// (0 = the start).
    WrongDepth(usize),
    /// A copied group isn't in the current group config (its name).
    UnknownGroup(String),
    /// A node would get more than 255 children.
    TooManyChildren,
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipError::Nothing => f.write_str("Nothing to copy: select nodes below the start."),
            ClipError::MixedDepths => {
                f.write_str("Only nodes at one draft step can be copied together.")
            }
            ClipError::WrongDepth(step) => write!(
                f,
                "These branches start at {}, so they can only go below a node at {}.",
                step_name(step + 1),
                step_name(*step)
            ),
            ClipError::UnknownGroup(name) => write!(
                f,
                "The copied branches use the group \"{name}\", which the current group config doesn't have."
            ),
            ClipError::TooManyChildren => {
                f.write_str("A node would get more than 255 branches, so this can't be done.")
            }
        }
    }
}

/// "the start" or a draft step's code (BB1 ... RP5) for a depth.
fn step_name(depth: usize) -> String {
    match depth
        .checked_sub(1)
        .and_then(|i| crate::draft::DRAFT_ORDER.get(i))
    {
        Some(s) => s.code(),
        None => "the start".into(),
    }
}

/// What a paste did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pasted {
    /// Where each copied top node ended up (new or merged), in order.
    pub tops: Vec<NodeId>,
    /// Nodes added.
    pub added: usize,
    /// Copied nodes that joined a node already there.
    pub merged: usize,
    /// Role tags dropped because they don't fit the new line.
    pub tags_dropped: usize,
}

fn depth_of(tree: &DraftTree, id: NodeId) -> usize {
    tree.path(id).len().saturating_sub(1)
}

impl Clip {
    /// Copies the branches of `tops` (all at one depth, in this order). A branch inside
    /// another one is copied once, as part of the outer one.
    pub fn copy(tree: &DraftTree, tops: &[NodeId]) -> Result<Clip, ClipError> {
        let tops: Vec<NodeId> = tops
            .iter()
            .copied()
            .filter(|&t| tree.contains(t) && t != tree.root())
            .collect();
        let depth = match tops.first() {
            Some(&t) => depth_of(tree, t),
            None => return Err(ClipError::Nothing),
        };
        if tops.iter().any(|&t| depth_of(tree, t) != depth) {
            return Err(ClipError::MixedDepths);
        }
        let mut clip = DraftTree::with_groups(tree.groups().clone());
        let mut holder = clip.root();
        for _ in 1..depth {
            let pos = clip.get(holder).expect("live").pos().next().expect("short");
            holder = clip
                .insert_raw(holder, empty_value_for(pos), 0, None)
                .expect("empty placeholders always fit");
        }
        let mut stack: Vec<(NodeId, NodeId)> = tops.iter().rev().map(|&t| (t, holder)).collect();
        while let Some((src, dst_parent)) = stack.pop() {
            let node = tree.get(src).expect("live node");
            let dst = clip
                .insert_raw(dst_parent, node.value(), node.roles(), None)
                .map_err(|_| ClipError::TooManyChildren)?;
            clip.set_marked(dst, node.is_marked());
            stack.extend(node.children().iter().rev().map(|&c| (c, dst)));
        }
        // Hidden branches: copy the flags now that every node has its children.
        let mut src_order = Vec::new();
        let mut walk: Vec<NodeId> = tops.iter().rev().copied().collect();
        while let Some(n) = walk.pop() {
            src_order.push(n);
            walk.extend(tree.get(n).expect("live").children().iter().rev().copied());
        }
        let mut dst_order = Vec::new();
        let mut walk: Vec<NodeId> = clip
            .get(holder)
            .expect("live")
            .children()
            .iter()
            .rev()
            .copied()
            .collect();
        while let Some(n) = walk.pop() {
            dst_order.push(n);
            walk.extend(clip.get(n).expect("live").children().iter().rev().copied());
        }
        for (&s, &d) in src_order.iter().zip(&dst_order) {
            if tree.get(s).is_some_and(|n| n.is_collapsed()) {
                clip.set_collapsed(d, true);
            }
        }
        Ok(Clip {
            tree: clip,
            holder,
            depth,
        })
    }

    /// Depth of the copied top nodes.
    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Number of copied branches.
    pub fn len(&self) -> usize {
        self.tree.get(self.holder).map_or(0, |n| n.children().len())
    }

    /// Number of copied nodes.
    pub fn nodes(&self) -> usize {
        self.tree.len() - self.depth
    }

    /// The copy's group values as the current config numbers them: a group with the same
    /// value and champions keeps its value, otherwise one with the same champions is used.
    fn group_map(&self, catalog: &GroupCatalog) -> Result<[u8; 256], ClipError> {
        let mut map = [0u8; 256];
        for (v, slot) in map.iter_mut().enumerate() {
            *slot = v as u8;
        }
        let mut used = [false; 256];
        for v in self.tree.values() {
            used[usize::from(v)] = true;
        }
        for g in self.tree.groups().user_groups() {
            if !used[usize::from(g.value)] {
                continue;
            }
            let same = |o: &Group| o.members == g.members;
            let found = catalog
                .get(g.value)
                .filter(|o| same(o))
                .or_else(|| catalog.user_groups().find(|o| same(o)));
            match found {
                Some(o) => map[usize::from(g.value)] = o.value,
                None => return Err(ClipError::UnknownGroup(g.name.clone())),
            }
        }
        Ok(map)
    }
}

fn is_empty_value(v: u8) -> bool {
    v == EMPTY || v == NO_BAN
}

/// Puts `clip`'s branches below `under` (see the module docs). The tree is left as it was
/// if this fails.
pub fn paste(tree: &mut DraftTree, clip: &Clip, under: NodeId) -> Result<Pasted, ClipError> {
    if !tree.contains(under) {
        return Err(ClipError::Nothing);
    }
    if depth_of(tree, under) + 1 != clip.depth {
        return Err(ClipError::WrongDepth(clip.depth - 1));
    }
    let map = clip.group_map(tree.groups())?;
    let mut work = tree.clone();
    let mut out = Pasted::default();
    let mut new_nodes = Vec::new();
    let mut hidden = Vec::new();
    let src = &clip.tree;
    // (copied node, its new parent)
    let mut stack: Vec<(NodeId, NodeId, bool)> = src
        .get(clip.holder)
        .expect("live")
        .children()
        .iter()
        .rev()
        .map(|&c| (c, under, true))
        .collect();
    // How many empty children with each tag set have been matched under each parent.
    let mut empties_seen: HashMap<(NodeId, u8), usize> = HashMap::new();
    while let Some((s, parent, top)) = stack.pop() {
        let node = src.get(s).expect("live node");
        let value = map[usize::from(node.value())];
        let roles = node.roles();
        let existing = if is_empty_value(value) {
            let k = empties_seen.entry((parent, roles)).or_insert(0);
            let found = work
                .get(parent)
                .expect("live")
                .children()
                .iter()
                .copied()
                .filter(|&d| {
                    let n = work.get(d).expect("live");
                    is_empty_value(n.value()) && n.roles() == roles && !new_nodes.contains(&d)
                })
                .nth(*k);
            *k += 1;
            found
        } else {
            work.child_with_value(parent, value)
        };
        let target = match existing {
            Some(d) => {
                out.merged += 1;
                d
            }
            None => {
                let d = work
                    .insert_raw(parent, value, roles, None)
                    .map_err(|e| match e {
                        TreeError::TooManyChildren => ClipError::TooManyChildren,
                        e => panic!("a pasted node is always valid at its depth: {e}"),
                    })?;
                out.added += 1;
                new_nodes.push(d);
                d
            }
        };
        if node.is_marked() {
            work.set_marked(target, true);
        }
        if node.is_collapsed() {
            hidden.push(target);
        }
        if top {
            out.tops.push(target);
        }
        stack.extend(node.children().iter().rev().map(|&c| (c, target, false)));
    }
    for id in hidden {
        work.set_collapsed(id, true);
    }
    // Role tags that don't fit the new line are dropped, top down.
    for &id in &new_nodes {
        let mask = work.get(id).expect("live").roles();
        if mask != 0 && work.roles_fit(id, mask, true).is_err() {
            work.set_roles_raw(id, 0);
            out.tags_dropped += 1;
        }
    }
    *tree = work;
    Ok(out)
}

/// Copies `sources` below `target` and, with `move_them`, removes the originals (except
/// those already below `target`, which stay where they are).
pub fn copy_or_move(
    tree: &mut DraftTree,
    sources: &[NodeId],
    target: NodeId,
    move_them: bool,
) -> Result<Pasted, ClipError> {
    let clip = Clip::copy(tree, sources)?;
    let mut work = tree.clone();
    let pasted = paste(&mut work, &clip, target)?;
    if move_them {
        for &s in sources {
            let stays = work.get(s).and_then(|n| n.parent()) == Some(target);
            if !stays && work.contains(s) {
                work.remove_subtree(s).map_err(|_| ClipError::Nothing)?;
            }
        }
    }
    *tree = work;
    Ok(pasted)
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
