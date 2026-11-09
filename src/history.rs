//! Undo/redo history of tree changes.
//!
//! A state is the tree as one fixed-size record per node in pre-order (depth, value, role
//! tags, flags), plus a small header (names, patch, fearless, group config) and the
//! selected node's pre-order index. Because records carry their depth instead of a shape,
//! adding or removing a branch only inserts or removes records, and any other node edit
//! changes one record.
//!
//! Each stack keeps only its newest state whole; every older one is stored as its
//! difference from the next newer one: the length of the records both share at the start
//! and at the end, the older state's records in between, and its header if that differs.
//! A small edit therefore costs a few dozen bytes however large the tree is.
//!
//! Undo and redo change the live tree in place: the records the current and the restored
//! state share at the start and at the end belong to nodes that stay as they are, ids
//! included, and only the nodes in between are changed, added or freed (see
//! [`restore_in_place`]). So the graph redraws only what changed, and whatever the app
//! keeps per node id (the last child visited, the selection) carries over. Only a step
//! that switches the group config builds a fresh tree.
//!
//! Loads (from a file or a pasted string) are marked, and only the trees from the last
//! few loads are kept: loading a new tree more than [`MAX_LOADED_TREES`] - 1 times drops
//! the oldest tree and all of its steps.
//!
//! Each step also keeps the group config that was active (shared, not copied), so undoing
//! or redoing across a load that imported groups switches the config back as well.
//! Loading a group config file or saving the group editor still clears the history.

use std::collections::VecDeque;

use std::sync::Arc;

use slotmap::Key;

use crate::fearless::Fearless;
use crate::groups::GroupCatalog;

use crate::tree::{DraftTree, NodeId};

/// Most undo steps kept in total.
pub const MAX_STEPS: usize = 200;
/// Trees reachable through undo: the current one plus the ones replaced by the last loads.
pub const MAX_LOADED_TREES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// An edit to the current tree.
    Edit,
    /// The tree was replaced by loading a file or pasting a string.
    Load,
}

/// One node: depth (the root is 0), value, role tags, [`HIDDEN`] | [`MARKED`].
type Record = [u8; 4];
/// The node's branches are hidden.
const HIDDEN: u8 = 1;
/// The line ending at the node is marked.
const MARKED: u8 = 2;

/// What a tree holds besides its nodes.
#[derive(Clone)]
struct Header {
    team_names: [String; 2],
    patch: String,
    fearless: Fearless,
    /// The group config the tree used, compared by pointer.
    groups: Arc<GroupCatalog>,
}

impl PartialEq for Header {
    fn eq(&self, other: &Self) -> bool {
        self.team_names == other.team_names
            && self.patch == other.patch
            && self.fearless == other.fearless
            && Arc::ptr_eq(&self.groups, &other.groups)
    }
}

/// A tree, its selection and its group config, as stored in the history.
#[derive(Clone, PartialEq)]
pub struct State {
    /// Every node in pre-order, the root first.
    records: Vec<Record>,
    /// Pre-order index of the selected node.
    cursor: u32,
    header: Arc<Header>,
}

/// An older state as its difference from the next newer one.
struct Delta {
    /// Records shared with the newer state at the start and at the end.
    prefix: u32,
    suffix: u32,
    /// The older state's records between those.
    middle: Box<[Record]>,
    cursor: u32,
    /// `None` when the header is the newer state's.
    header: Option<Arc<Header>>,
}

impl Delta {
    /// The difference that turns `newer` back into `older`.
    fn between(older: &State, newer: &State) -> Delta {
        let (a, b) = (&older.records, &newer.records);
        let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        let room = a.len().min(b.len()) - prefix;
        let suffix = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(room)
            .take_while(|(x, y)| x == y)
            .count();
        let header = (older.header != newer.header).then(|| older.header.clone());
        Delta {
            prefix: prefix as u32,
            suffix: suffix as u32,
            middle: a[prefix..a.len() - suffix].into(),
            cursor: older.cursor,
            header,
        }
    }

    /// The older state, rebuilt from `newer`.
    fn apply(&self, newer: &State) -> State {
        let b = &newer.records;
        let (prefix, suffix) = (self.prefix as usize, self.suffix as usize);
        let mut records = Vec::with_capacity(prefix + self.middle.len() + suffix);
        records.extend_from_slice(&b[..prefix]);
        records.extend_from_slice(&self.middle);
        records.extend_from_slice(&b[b.len() - suffix..]);
        State {
            records,
            cursor: self.cursor,
            header: self.header.clone().unwrap_or_else(|| newer.header.clone()),
        }
    }
}

/// A stack of states, newest on top: the top whole, the others as [`Delta`]s.
#[derive(Default)]
struct Stack {
    /// Oldest first; each with the change that led away from it (undo) or back to it (redo).
    older: VecDeque<(Delta, ChangeKind)>,
    top: Option<(State, ChangeKind)>,
}

impl Stack {
    fn len(&self) -> usize {
        self.older.len() + usize::from(self.top.is_some())
    }

    fn is_empty(&self) -> bool {
        self.top.is_none()
    }

    fn push(&mut self, state: State, kind: ChangeKind) {
        if let Some((top, top_kind)) = self.top.take() {
            self.older
                .push_back((Delta::between(&top, &state), top_kind));
        }
        self.top = Some((state, kind));
    }

    fn pop(&mut self) -> Option<(State, ChangeKind)> {
        let (state, kind) = self.top.take()?;
        if let Some((delta, below)) = self.older.pop_back() {
            self.top = Some((delta.apply(&state), below));
        }
        Some((state, kind))
    }

    /// Drops the oldest state, returning its change.
    fn pop_front(&mut self) -> Option<ChangeKind> {
        match self.older.pop_front() {
            Some((_, kind)) => Some(kind),
            None => self.top.take().map(|(_, kind)| kind),
        }
    }

    fn kinds(&self) -> impl Iterator<Item = ChangeKind> + '_ {
        let older = self.older.iter().map(|(_, kind)| *kind);
        older.chain(self.top.iter().map(|(_, kind)| *kind))
    }

    fn clear(&mut self) {
        self.older.clear();
        self.top = None;
    }
}

#[derive(Default)]
pub struct History {
    undo: Stack,
    redo: Stack,
}

/// Visits every node of `tree` in pre-order with its record, the root first, until `visit`
/// returns `false`.
fn walk(tree: &DraftTree, mut visit: impl FnMut(NodeId, Record) -> bool) {
    let record = |id: NodeId, depth: usize| -> Record {
        let node = tree.get(id).expect("live node");
        let mut flags = 0;
        if node.is_collapsed() {
            flags |= HIDDEN;
        }
        if node.is_marked() {
            flags |= MARKED;
        }
        debug_assert!(depth <= usize::from(u8::MAX), "drafts are shallow");
        [depth as u8, node.value(), node.roles(), flags]
    };
    // Iterative pre-order walk. Each stack entry is a node and the index of the next child
    // to visit; the stack's length is the depth of the next child.
    let root = tree.root();
    if !visit(root, record(root, 0)) {
        return;
    }
    let mut stack: Vec<(NodeId, usize)> = vec![(root, 0)];
    while let Some((id, next)) = stack.last_mut() {
        let children = tree.get(*id).expect("live node").children();
        if let Some(&child) = children.get(*next) {
            *next += 1;
            if !visit(child, record(child, stack.len())) {
                return;
            }
            stack.push((child, 0));
        } else {
            stack.pop();
        }
    }
}

/// What `tree` holds besides its nodes.
fn header_of(tree: &DraftTree) -> Header {
    Header {
        team_names: tree.team_names().clone(),
        patch: tree.patch().to_string(),
        fearless: *tree.fearless(),
        groups: tree.groups().clone(),
    }
}

/// The state of `tree` and `cursor`, and with `ids` every node's id in pre-order.
fn capture(tree: &DraftTree, cursor: NodeId, mut ids: Option<&mut Vec<NodeId>>) -> State {
    let mut records: Vec<Record> = Vec::with_capacity(tree.len());
    if let Some(ids) = ids.as_deref_mut() {
        ids.reserve(tree.len());
    }
    let mut selected = 0;
    walk(tree, |id, record| {
        if id == cursor {
            selected = records.len();
        }
        records.push(record);
        if let Some(ids) = ids.as_deref_mut() {
            ids.push(id);
        }
        true
    });
    State {
        records,
        cursor: selected as u32,
        header: Arc::new(header_of(tree)),
    }
}

/// The state of `tree` and `cursor` for [`History::record`].
pub fn snapshot(tree: &DraftTree, cursor: NodeId) -> State {
    capture(tree, cursor, None)
}

/// Whether `tree` and `cursor` are still in `state`, e.g. the [`snapshot`] taken before an
/// action that may have changed nothing. Cheaper than a second snapshot: nothing is
/// copied, and the walk stops at the first difference.
pub fn unchanged(state: &State, tree: &DraftTree, cursor: NodeId) -> bool {
    if *state.header != header_of(tree) || state.records.len() != tree.len() {
        return false;
    }
    // A snapshot selects the root for a cursor that isn't in the tree.
    let cursor = if tree.contains(cursor) {
        cursor
    } else {
        tree.root()
    };
    let mut same = true;
    let mut i = 0;
    walk(tree, |id, record| {
        same =
            state.records.get(i) == Some(&record) && (id == cursor) == (i == state.cursor as usize);
        i += 1;
        same
    });
    same && i == state.records.len()
}

/// Builds a tree from a state, with the state's own group config.
fn restore(state: &State) -> (DraftTree, NodeId) {
    let header = &state.header;
    let mut tree = DraftTree::with_groups(header.groups.clone());
    // Every node in pre-order, so indexes can be turned back into ids.
    let mut order = Vec::with_capacity(state.records.len());
    order.push(tree.root());
    let mut path = vec![tree.root()];
    for &[depth, value, _, _] in &state.records[1..] {
        path.truncate(usize::from(depth));
        let parent = *path.last().expect("records keep the root");
        let child = tree
            .load_child(parent, value)
            .expect("history states are always valid");
        order.push(child);
        path.push(child);
    }
    // Like a file load: tags in pre-order once every node is in, so lines fill up in order.
    for (&id, &[_, _, roles, flags]) in order.iter().zip(&state.records) {
        if roles != 0 && id != tree.root() {
            tree.set_roles_unlocked(id, roles)
                .expect("history states are always valid");
        }
        if flags & HIDDEN != 0 {
            tree.set_collapsed(id, true);
        }
        if flags & MARKED != 0 {
            tree.set_marked(id, true);
        }
    }
    tree.set_team_name(0, &header.team_names[0]);
    tree.set_team_name(1, &header.team_names[1]);
    tree.set_patch(&header.patch);
    tree.set_fearless(header.fearless);
    let cursor = order[state.cursor as usize];
    (tree, cursor)
}

/// Marks for "none" in the index arrays of [`restore_in_place`].
const NONE: u32 = u32::MAX;

/// The arena slot of `id`: unique among the live nodes of one tree.
fn slot_index(id: NodeId) -> usize {
    (id.data().as_ffi() & 0xFFFF_FFFF) as usize
}

/// Turns `tree` into `target` in place and returns the selection. `now` is `tree`'s own
/// state and `ids` its node ids in pre-order; both trees must use the same group config.
///
/// The records both states share at the start and at the end (the root always counts as
/// shared) belong to nodes that stay, with their ids. Only the target's records in
/// between ("changed") are worked on: each is matched to a node the tree has between
/// those, below the same parent and preferably with the same value (so siblings that
/// changed places keep their ids), or else gets a new node; the tree's nodes left over
/// are freed. A matched node whose whole branch is the same in both is taken over as it
/// is. Then the child lists that changed are rebuilt: those of the changed nodes, and
/// those of the nodes on the path down to the last shared record at the start, which keep
/// their children up to that path and get the rest from the target. The shared nodes at
/// the end whose parent lies outside them are linked to their new parent.
fn restore_in_place(tree: &mut DraftTree, now: &State, ids: &[NodeId], target: &State) -> NodeId {
    let (a, b) = (&target.records, &now.records);
    debug_assert_eq!(ids.len(), b.len());
    let prefix = 1 + a[1..]
        .iter()
        .zip(&b[1..])
        .take_while(|(x, y)| x == y)
        .count();
    let room = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    let (a_end, b_end) = (a.len() - suffix, b.len() - suffix);
    let changed = &a[prefix..a_end];
    let kept_end = &ids[b_end..];

    // The path to the last shared node at the start; its hidden flags as the target has
    // them (the same as now, except perhaps the root's).
    let path = tree.path(ids[prefix - 1]);
    let path_hidden: Vec<bool> = path
        .iter()
        .enumerate()
        .map(|(depth, &id)| match depth {
            0 => a[0][3] & HIDDEN != 0,
            _ => tree.get(id).expect("live node").is_collapsed(),
        })
        .collect();

    // The target's shape. Every node whose child list may change has a slot: the path's
    // nodes by depth, then the changed records. Each changed record's parent slot:
    let slot_of_changed = |j: usize| path.len() + j;
    let mut parent_of: Vec<u32> = Vec::with_capacity(changed.len());
    let mut stack: Vec<u32> = (0..path.len() as u32).collect();
    for (j, record) in changed.iter().enumerate() {
        stack.truncate(usize::from(record[0]));
        parent_of.push(*stack.last().expect("records keep the root"));
        stack.push(slot_of_changed(j) as u32);
    }
    // A shared node at the end has its parent among them unless it is no deeper than
    // every one before it. These come after all changed children, by parent slot.
    let mut relinked: Vec<(u32, NodeId)> = Vec::new();
    let mut shallowest = usize::MAX;
    for (&id, record) in kept_end.iter().zip(&a[a_end..]) {
        let depth = usize::from(record[0]);
        if depth <= shallowest {
            shallowest = depth;
            stack.truncate(depth);
            relinked.push((stack[depth - 1], id));
        }
    }
    relinked.sort_by_key(|&(slot, _)| slot);
    // Each slot's changed children in order: `kids[first[s]..first[s + 1]]`.
    let slots = path.len() + changed.len();
    let mut first = vec![0u32; slots + 1];
    for &p in &parent_of {
        first[p as usize + 1] += 1;
    }
    for s in 0..slots {
        first[s + 1] += first[s];
    }
    let mut kids = vec![0u32; changed.len()];
    let mut fill = first[..slots].to_vec();
    for (j, &p) in parent_of.iter().enumerate() {
        kids[fill[p as usize] as usize] = j as u32;
        fill[p as usize] += 1;
    }
    drop(fill);
    let changed_kids = |slot: usize| &kids[first[slot] as usize..first[slot + 1] as usize];
    let kept_kids = |slot: usize| {
        let from = relinked.partition_point(|&(s, _)| (s as usize) < slot);
        let to = relinked.partition_point(|&(s, _)| (s as usize) <= slot);
        &relinked[from..to]
    };

    // The tree's nodes between the shared ones that no changed record has taken yet: their
    // index in `now`, by arena slot.
    let between = &ids[prefix..b_end];
    let size = between
        .iter()
        .map(|&id| slot_index(id) + 1)
        .max()
        .unwrap_or(0);
    let mut spare = vec![NONE; size];
    for (i, &id) in between.iter().enumerate() {
        spare[slot_index(id)] = (prefix + i) as u32;
    }
    let take = |spare: &mut [u32], id: NodeId| -> Option<u32> {
        let at = spare.get_mut(slot_index(id))?;
        let i = std::mem::replace(at, NONE);
        (i != NONE).then_some(i)
    };
    // The node each changed record gets, and for a node taken over its index in `now`.
    let mut assigned: Vec<Option<NodeId>> = vec![None; changed.len()];
    let mut from: Vec<u32> = vec![NONE; changed.len()];
    // Gives `parent`'s changed children in the target the spare nodes now below `parent`:
    // one with the same value first, then the rest in order.
    let match_children = |tree: &DraftTree,
                          spare: &mut [u32],
                          assigned: &mut [Option<NodeId>],
                          from: &mut [u32],
                          parent: NodeId,
                          kids: &[u32]| {
        if kids.is_empty() {
            return;
        }
        let mut free: Vec<NodeId> = tree.get(parent).expect("live node").children().to_vec();
        free.retain(|&c| spare.get(slot_index(c)).is_some_and(|&i| i != NONE));
        if free.is_empty() {
            return;
        }
        let mut unmatched = Vec::new();
        for &j in kids {
            let j = j as usize;
            let value = changed[j][1];
            let found = free
                .iter()
                .position(|&c| tree.get(c).expect("live node").value() == value);
            match found {
                Some(i) => {
                    let id = free.remove(i);
                    assigned[j] = Some(id);
                    from[j] = take(spare, id).expect("spare");
                }
                None => unmatched.push(j),
            }
        }
        for (j, id) in unmatched.into_iter().zip(free) {
            assigned[j] = Some(id);
            from[j] = take(spare, id).expect("spare");
        }
    };
    for (d, &id) in path.iter().enumerate() {
        let kids = changed_kids(d);
        match_children(tree, &mut spare, &mut assigned, &mut from, id, kids);
    }
    // Changed records whose node's child list is already right: inside a branch taken
    // over whole.
    let mut settled = vec![false; changed.len()];
    let mut j = 0;
    while j < changed.len() {
        let [depth, value, roles, flags] = changed[j];
        let (hidden, marked) = (flags & HIDDEN != 0, flags & MARKED != 0);
        let Some(id) = assigned[j] else {
            let parent = match parent_of[j] as usize {
                p if p < path.len() => path[p],
                p => assigned[p - path.len()].expect("parents come first"),
            };
            assigned[j] = Some(tree.raw_new(parent, value, roles, hidden, marked));
            j += 1;
            continue;
        };
        // The branch below, in the target and now; the same records, wholly between the
        // shared ones, means the same nodes in the same shape.
        let (start, i) = (prefix + j, from[j] as usize);
        let below = |records: &[Record], at: usize| {
            records[at + 1..]
                .iter()
                .take_while(|r| r[0] > depth)
                .count()
        };
        let len = 1 + below(a, start);
        if start + len <= a_end
            && i + len <= b_end
            && a[start..start + len] == b[i..i + len]
            && (i + len == b.len() || b[i + len][0] <= depth)
        {
            for k in 1..len {
                let id = ids[i + k];
                take(&mut spare, id);
                assigned[j + k] = Some(id);
            }
            settled[j..j + len].fill(true);
            j += len;
            continue;
        }
        tree.raw_set(id, value, roles, hidden, marked);
        let kids = changed_kids(slot_of_changed(j));
        match_children(tree, &mut spare, &mut assigned, &mut from, id, kids);
        j += 1;
    }

    // Child lists and parent links.
    let node = |slot: usize| match slot {
        s if s < path.len() => path[s],
        s => assigned[s - path.len()].expect("every changed record has a node"),
    };
    let fill_children = |children: &mut Vec<NodeId>, slot: usize| {
        let changed = changed_kids(slot).iter();
        children.extend(changed.map(|&j| node(slot_of_changed(j as usize))));
        children.extend(kept_kids(slot).iter().map(|&(_, id)| id));
    };
    for (depth, &id) in path.iter().enumerate() {
        let keep = path.get(depth + 1).map_or(0, |next| {
            let children = tree.get(id).expect("live node").children();
            1 + children.iter().position(|c| c == next).expect("path child")
        });
        let children = tree.raw_children_mut(id);
        children.truncate(keep);
        fill_children(children, depth);
        tree.raw_set_collapsed(id, path_hidden[depth]);
    }
    for j in (0..changed.len()).filter(|&j| !settled[j]) {
        let slot = slot_of_changed(j);
        let children = tree.raw_children_mut(node(slot));
        children.clear();
        fill_children(children, slot);
    }
    for &(parent, id) in &relinked {
        tree.raw_set_parent(id, node(parent as usize));
    }
    for &id in between {
        if take(&mut spare, id).is_some() {
            tree.raw_free(id);
        }
    }

    let header = &target.header;
    tree.set_team_name(0, &header.team_names[0]);
    tree.set_team_name(1, &header.team_names[1]);
    tree.set_patch(&header.patch);
    tree.set_fearless(header.fearless);

    let at = target.cursor as usize;
    if at < prefix {
        ids[at]
    } else if at < a_end {
        node(slot_of_changed(at - prefix))
    } else {
        kept_end[at - a_end]
    }
}

/// What undo or redo did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// The node to select.
    pub cursor: NodeId,
    /// Whether the tree was built anew, so no earlier node id is valid any more. Otherwise
    /// it was changed in place, and every node that stayed kept its id.
    pub rebuilt: bool,
}

/// Turns `tree` (whose state is `now`, with its ids in pre-order) into `target`: in place
/// when both use the same group config, otherwise by building a new tree.
fn apply(tree: &mut DraftTree, now: &State, ids: &[NodeId], target: &State) -> Step {
    if Arc::ptr_eq(&target.header.groups, tree.groups()) {
        let cursor = restore_in_place(tree, now, ids, target);
        Step {
            cursor,
            rebuilt: false,
        }
    } else {
        let (new, cursor) = restore(target);
        *tree = new;
        Step {
            cursor,
            rebuilt: true,
        }
    }
}

impl History {
    /// Records `state`, a [`snapshot`] taken *before* a change of `kind`. Call it only once
    /// the change has succeeded; it clears the redo steps.
    pub fn record(&mut self, state: State, kind: ChangeKind) {
        self.redo.clear();
        self.push_undo(state, kind);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Steps back: turns `tree` into the state before the last change, in place where it
    /// can (see [`Step`]).
    pub fn undo(&mut self, tree: &mut DraftTree, cursor: NodeId) -> Option<Step> {
        let (state, kind) = self.undo.pop()?;
        let mut ids = Vec::new();
        let now = capture(tree, cursor, Some(&mut ids));
        let step = apply(tree, &now, &ids, &state);
        self.redo.push(now, kind);
        Some(step)
    }

    /// Re-applies the last undone change to `tree`, in place where it can (see [`Step`]).
    pub fn redo(&mut self, tree: &mut DraftTree, cursor: NodeId) -> Option<Step> {
        let (state, kind) = self.redo.pop()?;
        let mut ids = Vec::new();
        let now = capture(tree, cursor, Some(&mut ids));
        let step = apply(tree, &now, &ids, &state);
        self.push_undo(now, kind);
        Some(step)
    }

    /// Forgets every step, e.g. after the group config changed.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn push_undo(&mut self, state: State, kind: ChangeKind) {
        self.undo.push(state, kind);
        // Each Load step marks a tree that was replaced. Keep at most MAX_LOADED_TREES - 1 of
        // them; dropping one also drops every older step, which belongs to that tree.
        while self.loads() >= MAX_LOADED_TREES {
            while let Some(dropped) = self.undo.pop_front() {
                if dropped == ChangeKind::Load {
                    break;
                }
            }
        }
        while self.undo.len() > MAX_STEPS {
            self.undo.pop_front();
        }
    }

    fn loads(&self) -> usize {
        self.undo.kinds().filter(|&k| k == ChangeKind::Load).count()
    }

    /// Bytes each undo step keeps, oldest first (the newest is whole).
    #[cfg(test)]
    fn step_bytes(&self) -> Vec<usize> {
        let header = |h: &Header| {
            size_of::<Header>()
                + h.team_names.iter().map(String::len).sum::<usize>()
                + h.patch.len()
        };
        let older = self.undo.older.iter().map(|(d, _)| {
            size_of::<(Delta, ChangeKind)>()
                + d.middle.len() * size_of::<Record>()
                + d.header.as_deref().map_or(0, header)
        });
        let top = self.undo.top.iter().map(|(s, _)| {
            size_of::<(State, ChangeKind)>()
                + s.records.len() * size_of::<Record>()
                + header(&s.header)
        });
        older.chain(top).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::champions::{self, CHAMPIONS};
    use crate::groups::{self, Group};
    use crate::{clip, save};

    fn tree_with(values: &[u8]) -> (DraftTree, NodeId) {
        let mut tree = DraftTree::new();
        let mut cursor = tree.root();
        for &v in values {
            cursor = tree.add_child(cursor, v).unwrap();
        }
        (tree, cursor)
    }

    fn line(tree: &DraftTree, cursor: NodeId) -> Vec<u8> {
        tree.path(cursor)
            .iter()
            .skip(1)
            .map(|&n| tree.get(n).unwrap().value())
            .collect()
    }

    /// Every node's id in pre-order.
    fn preorder(tree: &DraftTree) -> Vec<NodeId> {
        let mut ids = Vec::new();
        capture(tree, tree.root(), Some(&mut ids));
        ids
    }

    /// Child lists and parent links agree, every node is reachable from the root, and
    /// every node sits at the draft position after its parent's.
    fn assert_links(tree: &DraftTree) {
        for (id, node) in tree.nodes_iter() {
            for &c in node.children() {
                let child = tree.get(c).expect("children are live");
                assert_eq!(child.parent(), Some(id), "parent link");
                assert_eq!(Some(child.pos()), node.pos().next(), "draft position");
            }
        }
        assert_eq!(preorder(tree).len(), tree.len(), "every node is reachable");
    }

    /// Undo (`redo == false`) or redo on a copy of `tree`, checking the result against a
    /// tree built from scratch from the same state, and that the nodes in the records
    /// both states share at the start and at the end kept their ids.
    fn step(
        h: &mut History,
        tree: &DraftTree,
        cursor: NodeId,
        redo: bool,
    ) -> Option<(DraftTree, NodeId)> {
        let stack = if redo { &h.redo } else { &h.undo };
        let target = stack.top.as_ref().map(|(s, _)| s.clone())?;
        let mut t = tree.clone();
        let before = preorder(&t);
        let now = snapshot(&t, cursor);
        let step = if redo {
            h.redo(&mut t, cursor)
        } else {
            h.undo(&mut t, cursor)
        }
        .unwrap();
        let (rebuilt, rebuilt_cursor) = restore(&target);
        assert!(snapshot(&t, step.cursor) == target, "the state is restored");
        assert!(
            snapshot(&t, step.cursor) == snapshot(&rebuilt, rebuilt_cursor),
            "the same as a rebuilt tree"
        );
        assert_links(&t);
        let same_groups = Arc::ptr_eq(&target.header.groups, &now.header.groups);
        assert_eq!(
            step.rebuilt, !same_groups,
            "in place unless the groups change"
        );
        if !step.rebuilt {
            let after = preorder(&t);
            let (a, b) = (&target.records, &now.records);
            let prefix = 1 + a[1..]
                .iter()
                .zip(&b[1..])
                .take_while(|(x, y)| x == y)
                .count();
            let room = a.len().min(b.len()) - prefix;
            let suffix = (0..room)
                .take_while(|i| a[a.len() - 1 - i] == b[b.len() - 1 - i])
                .count();
            assert_eq!(after[..prefix], before[..prefix], "shared start keeps ids");
            assert_eq!(
                after[after.len() - suffix..],
                before[before.len() - suffix..],
                "shared end keeps ids"
            );
            // Nodes that are gone are really gone: the count of survivors adds up.
            let survivors = before.iter().filter(|&&id| t.contains(id)).count();
            assert!(survivors >= prefix + suffix);
            assert!(survivors <= after.len());
        }
        Some((t, step.cursor))
    }

    fn undo(h: &mut History, tree: &DraftTree, cursor: NodeId) -> Option<(DraftTree, NodeId)> {
        step(h, tree, cursor, false)
    }

    fn redo(h: &mut History, tree: &DraftTree, cursor: NodeId) -> Option<(DraftTree, NodeId)> {
        step(h, tree, cursor, true)
    }

    #[test]
    fn undo_and_redo_restore_tree_and_selection() {
        let mut h = History::default();
        let (mut tree, mut cursor) = tree_with(&[16]);
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        cursor = tree.add_child(cursor, 17).unwrap();
        assert!(h.can_undo() && !h.can_redo());

        let (t, c) = undo(&mut h, &tree, cursor).unwrap();
        assert_eq!((t.len(), line(&t, c)), (2, vec![16]));
        let (t, c) = redo(&mut h, &t, c).unwrap();
        assert_eq!((t.len(), line(&t, c)), (3, vec![16, 17]));
        assert!(undo(&mut h, &t, c).is_some());
        assert!(undo(&mut h, &tree, cursor).is_none());
    }

    #[test]
    fn a_new_change_clears_redo() {
        let mut h = History::default();
        let (tree, cursor) = tree_with(&[16]);
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        let (t, c) = undo(&mut h, &tree, cursor).unwrap();
        assert!(h.can_redo());
        h.record(snapshot(&t, c), ChangeKind::Edit);
        assert!(!h.can_redo());
    }

    #[test]
    fn keeps_only_the_last_loaded_trees() {
        let mut h = History::default();
        // Tree A with two edits, then three loads (B, C, D) with an edit on each.
        let (a, ac) = tree_with(&[16]);
        h.record(snapshot(&a, ac), ChangeKind::Edit);
        h.record(snapshot(&a, ac), ChangeKind::Edit);
        h.record(snapshot(&a, ac), ChangeKind::Load); // A -> B
        let (b, bc) = tree_with(&[17]);
        h.record(snapshot(&b, bc), ChangeKind::Edit);
        h.record(snapshot(&b, bc), ChangeKind::Load); // B -> C: A is still reachable
        assert_eq!(h.undo.len(), 5);
        let (c, cc) = tree_with(&[18]);
        h.record(snapshot(&c, cc), ChangeKind::Load); // C -> D: A and its edits are dropped
        assert_eq!(h.undo.len(), 3);
        assert_eq!(h.loads(), 2);

        // Undo walks back to B and no further.
        let (d, dc) = tree_with(&[19]);
        let (t, c1) = undo(&mut h, &d, dc).unwrap();
        assert_eq!(line(&t, c1), vec![18]);
        let (t, c2) = undo(&mut h, &t, c1).unwrap();
        assert_eq!(line(&t, c2), vec![17]);
        let (t, c3) = undo(&mut h, &t, c2).unwrap();
        assert_eq!(line(&t, c3), vec![17]);
        assert!(undo(&mut h, &t, c3).is_none());
    }

    #[test]
    fn caps_the_number_of_steps() {
        let mut h = History::default();
        let (tree, cursor) = tree_with(&[]);
        for _ in 0..MAX_STEPS + 50 {
            h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        }
        assert_eq!(h.undo.len(), MAX_STEPS);
    }

    /// Small deterministic xorshift generator, so failures repeat.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }

        fn champion(&mut self) -> u8 {
            champions::value_of(self.below(CHAMPIONS.len()))
        }
    }

    /// Grows a random valid tree of about `size` nodes; the ids of its nodes are in `ids`.
    fn random_tree(rng: &mut Rng, size: usize) -> (DraftTree, Vec<NodeId>) {
        let mut tree = DraftTree::new();
        let mut ids = vec![tree.root()];
        for _ in 0..size * 4 {
            if tree.len() >= size {
                break;
            }
            let parent = ids[rng.below(ids.len())];
            let value = rng.champion();
            if let Ok(id) = tree.add_child(parent, value) {
                ids.push(id);
            }
        }
        (tree, ids)
    }

    fn any_node(rng: &mut Rng, tree: &DraftTree) -> NodeId {
        let ids: Vec<NodeId> = tree.nodes_iter().map(|(id, _)| id).collect();
        ids[rng.below(ids.len())]
    }

    fn catalog(n: usize) -> Arc<GroupCatalog> {
        let members = (0..5).map(|i| champions::value_of(i + n % 50)).collect();
        let group = Group::new_user(groups::USER_FIRST, format!("U{n}"), String::new(), members);
        Arc::new(GroupCatalog::with_user_groups(vec![group], None))
    }

    /// One random change; returns its kind, or `None` if it changed nothing.
    fn random_change(
        rng: &mut Rng,
        tree: &mut DraftTree,
        cursor: &mut NodeId,
        loads: &mut usize,
    ) -> Option<ChangeKind> {
        let before = snapshot(tree, *cursor);
        let node = any_node(rng, tree);
        let mut kind = ChangeKind::Edit;
        match rng.below(20) {
            0 | 1 => {
                // Sometimes a user group, when the config has one.
                let value = if rng.below(4) == 0 {
                    groups::USER_FIRST
                } else {
                    rng.champion()
                };
                if let Ok(id) = tree.add_child(node, value) {
                    *cursor = id;
                }
            }
            2 => {
                let _ = tree.remove_subtree(node);
            }
            3 => {
                let _ = tree.set_value(node, rng.champion());
            }
            4 => {
                let _ = tree.duplicate_branch(node);
            }
            5 => {
                let _ = tree.duplicate_line(node, rng.below(2) == 0);
            }
            6 => {
                tree.move_among_siblings(node, if rng.below(2) == 0 { -1 } else { 1 });
            }
            7 => {
                let hidden = tree.get(node).unwrap().is_collapsed();
                tree.set_collapsed(node, !hidden);
            }
            8 => {
                let marked = tree.get(node).unwrap().is_marked();
                tree.set_marked(node, !marked);
            }
            9 => {
                let _ = tree.set_roles(node, rng.below(32) as u8);
            }
            10 => {
                let n = rng.below(100);
                tree.set_team_name(rng.below(2), &format!("Team {n}"));
                if rng.below(2) == 0 {
                    tree.set_patch(&format!("25.{n}"));
                }
            }
            11 => {
                let mut f = *tree.fearless();
                f.game = 1 + rng.below(3) as u8;
                f.rows[0][rng.below(f.rows[0].len())] = rng.champion();
                tree.set_fearless(f);
            }
            12 => {
                tree.replace_groups(catalog(rng.below(1000)));
            }
            13 => {
                // Two loads at most, so no tree is dropped.
                if *loads == 2 {
                    return None;
                }
                *loads += 1;
                let (new, ids) = random_tree(rng, 300);
                *tree = new;
                *cursor = ids[rng.below(ids.len())];
                kind = ChangeKind::Load;
            }
            14 => {
                let _ = tree.remove_unreachable();
            }
            15 => {
                let _ = tree.remove_conflicts();
            }
            16 => {
                // Now and then a reset, else a few scattered branches removed at once.
                if rng.below(6) == 0 {
                    tree.clear();
                } else {
                    for _ in 0..3 {
                        let n = any_node(rng, tree);
                        let _ = tree.remove_subtree(n);
                    }
                }
            }
            17 | 18 => {
                // Copy or move a branch below a node at its parent's depth.
                let other = tree.cycle_same_depth(node, true).unwrap_or(node);
                if let Some(target) = tree.get(other).and_then(|n| n.parent()) {
                    let _ = clip::copy_or_move(tree, &[node], target, rng.below(2) == 0);
                }
            }
            _ => {
                if rng.below(2) == 0 {
                    tree.unmark_subtree(node);
                } else {
                    let _ = tree.set_empty(node);
                }
            }
        }
        if !tree.contains(*cursor) {
            *cursor = tree.root();
        }
        let changed = snapshot(tree, *cursor) != before;
        assert_eq!(unchanged(&before, tree, *cursor), !changed);
        changed.then_some(kind)
    }

    /// The tree and selection in the file encoding, and the group config.
    fn expected(tree: &DraftTree, cursor: NodeId) -> (Vec<u8>, Arc<GroupCatalog>) {
        (save::encode(tree, Some(cursor)), tree.groups().clone())
    }

    fn assert_state(tree: &DraftTree, cursor: NodeId, want: &(Vec<u8>, Arc<GroupCatalog>)) {
        assert!(
            save::encode(tree, Some(cursor)) == want.0,
            "tree or selection differs"
        );
        assert!(Arc::ptr_eq(tree.groups(), &want.1), "group config differs");
    }

    #[test]
    fn random_edits_undo_and_redo_exactly() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut tree, ids) = random_tree(&mut rng, 1500);
        let mut cursor = ids[ids.len() / 2];
        let mut h = History::default();
        let mut loads = 0;
        // The state before each recorded change, then the current one.
        let mut states = Vec::new();
        let mut kinds = [0usize; 2];
        while states.len() < MAX_STEPS + 60 {
            // The selection also moves between edits without a step of its own.
            if rng.below(3) == 0 {
                cursor = any_node(&mut rng, &tree);
            }
            let before = snapshot(&tree, cursor);
            let want = expected(&tree, cursor);
            if let Some(kind) = random_change(&mut rng, &mut tree, &mut cursor, &mut loads) {
                h.record(before, kind);
                states.push(want);
                kinds[usize::from(kind == ChangeKind::Load)] += 1;
            }
        }
        assert_eq!(kinds[1], 2, "both loads happened");
        states.push(expected(&tree, cursor));
        let last = states.len() - 1;
        let first = last - MAX_STEPS;

        let (mut t, mut c) = (tree, cursor);
        for i in (first..last).rev() {
            (t, c) = undo(&mut h, &t, c).unwrap();
            assert_state(&t, c, &states[i]);
        }
        assert!(undo(&mut h, &t, c).is_none());
        for want in &states[first + 1..] {
            (t, c) = redo(&mut h, &t, c).unwrap();
            assert_state(&t, c, want);
        }
        assert!(redo(&mut h, &t, c).is_none());
        // And back down once more, now from the states redo recorded.
        for i in (first..last).rev() {
            (t, c) = undo(&mut h, &t, c).unwrap();
            assert_state(&t, c, &states[i]);
        }
    }

    /// Edits, undos and redos mixed at random on small trees, where changes often touch
    /// the root, the first or the last node. Every undo and redo is checked against a
    /// tree built anew (see `step`).
    #[test]
    fn random_mixed_undo_redo_on_small_trees() {
        for seed in 1..=40u64 {
            let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
            let size = 2 + rng.below(120);
            let (mut tree, ids) = random_tree(&mut rng, size);
            let mut cursor = ids[rng.below(ids.len())];
            let mut h = History::default();
            let mut loads = 0;
            for _ in 0..300 {
                match rng.below(5) {
                    0 => {
                        if let Some((t, c)) = undo(&mut h, &tree, cursor) {
                            (tree, cursor) = (t, c);
                        }
                    }
                    1 => {
                        if let Some((t, c)) = redo(&mut h, &tree, cursor) {
                            (tree, cursor) = (t, c);
                        }
                    }
                    _ => {
                        if rng.below(3) == 0 {
                            cursor = any_node(&mut rng, &tree);
                        }
                        let before = snapshot(&tree, cursor);
                        let changed = random_change(&mut rng, &mut tree, &mut cursor, &mut loads);
                        if let Some(kind) = changed {
                            h.record(before, kind);
                        }
                    }
                }
            }
            // Everything still undoes and redoes.
            while let Some((t, c)) = undo(&mut h, &tree, cursor) {
                (tree, cursor) = (t, c);
            }
            while let Some((t, c)) = redo(&mut h, &tree, cursor) {
                (tree, cursor) = (t, c);
            }
        }
    }

    #[test]
    fn a_small_edit_on_a_large_tree_stores_a_few_bytes() {
        let mut rng = Rng(42);
        let (mut tree, ids) = random_tree(&mut rng, 30_000);
        assert!(tree.len() >= 30_000);
        let leaf = *ids
            .iter()
            .rev()
            .find(|&&id| tree.get(id).unwrap().children().is_empty())
            .unwrap();
        let mut h = History::default();
        let mut cursor = leaf;

        // Mark a line, rename a team, add a pick, remove it again.
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        tree.set_marked(leaf, true);
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        tree.set_team_name(0, "Blue");
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        let parent = tree.get(leaf).unwrap().parent().unwrap();
        cursor = (0..)
            .find_map(|_| tree.add_child(parent, rng.champion()).ok())
            .unwrap();
        h.record(snapshot(&tree, cursor), ChangeKind::Edit);
        tree.remove_subtree(cursor).unwrap();
        cursor = leaf;

        // Today about 50 bytes per node edit, plus the header (about 150) when that changed.
        let bytes = h.step_bytes();
        let [mark, rename, add, newest] = bytes[..] else {
            panic!("four steps");
        };
        assert!(
            mark <= 64 && add <= 64,
            "a node edit stores {mark} / {add} bytes"
        );
        assert!(rename <= 256, "a rename stores {rename} bytes");
        assert!(
            newest >= tree.len() * size_of::<Record>(),
            "the newest is whole"
        );
        // Everything still undoes.
        let want = expected(&tree, cursor);
        let (mut t, mut c) = (tree, cursor);
        for _ in 0..4 {
            (t, c) = undo(&mut h, &t, c).unwrap();
        }
        for _ in 0..4 {
            (t, c) = redo(&mut h, &t, c).unwrap();
        }
        assert_state(&t, c, &want);
    }

    fn id_set(tree: &DraftTree) -> std::collections::HashSet<NodeId> {
        tree.nodes_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn undo_and_redo_keep_the_ids_of_nodes_that_stay() {
        let mut rng = Rng(0x5EED);
        // Champions only, so no two siblings share a value.
        let (mut tree, _) = random_tree(&mut rng, 2000);
        let mut h = History::default();
        let mut cursor = tree.root();
        let mut steps = 0;
        while steps < 150 {
            let before_ids = id_set(&tree);
            let before = snapshot(&tree, cursor);
            let mut t = tree.clone();
            let node = any_node(&mut rng, &t);
            match rng.below(9) {
                0 => {
                    let _ = t.add_child(node, rng.champion());
                }
                1 => {
                    let _ = t.remove_subtree(node);
                }
                2 => {
                    let _ = t.set_value(node, rng.champion());
                }
                3 => {
                    t.move_among_siblings(node, if rng.below(2) == 0 { -1 } else { 1 });
                }
                4 => {
                    t.set_marked(node, !t.get(node).unwrap().is_marked());
                    let other = any_node(&mut rng, &t);
                    t.set_collapsed(other, !t.get(other).unwrap().is_collapsed());
                }
                5 => {
                    let _ = t.set_roles(node, rng.below(32) as u8);
                }
                6 => {
                    for _ in 0..4 {
                        let n = any_node(&mut rng, &t);
                        let _ = t.remove_subtree(n);
                    }
                }
                7 => {
                    let other = t.cycle_same_depth(node, true).unwrap_or(node);
                    if let Some(target) = t.get(other).and_then(|n| n.parent()) {
                        let _ = clip::copy_or_move(&mut t, &[node], target, rng.below(2) == 0);
                    }
                }
                _ => {
                    let _ = t.remove_conflicts();
                }
            }
            if unchanged(&before, &t, cursor) {
                continue;
            }
            steps += 1;
            h.record(before, ChangeKind::Edit);
            let after_ids = id_set(&t);
            let stayed: Vec<NodeId> = before_ids.intersection(&after_ids).copied().collect();
            if !t.contains(cursor) {
                cursor = t.root();
            }
            // Undo, check, redo, check, and go on from the redone tree.
            let (undone, c) = undo(&mut h, &t, cursor).unwrap();
            assert_eq!(undone.len(), before_ids.len());
            assert!(
                stayed.iter().all(|&id| undone.contains(id)),
                "undo keeps ids"
            );
            let (redone, c) = redo(&mut h, &undone, c).unwrap();
            assert_eq!(redone.len(), after_ids.len());
            assert!(
                stayed.iter().all(|&id| redone.contains(id)),
                "redo keeps ids"
            );
            // A second round keeps them too.
            let (again, c) = undo(&mut h, &redone, c).unwrap();
            assert!(
                stayed.iter().all(|&id| again.contains(id)),
                "undo keeps ids"
            );
            (tree, cursor) = redo(&mut h, &again, c).unwrap();
            assert!(stayed.iter().all(|&id| tree.contains(id)), "redo keeps ids");
        }
    }

    #[test]
    fn a_value_change_keeps_every_id() {
        let mut rng = Rng(3);
        let (mut tree, ids) = random_tree(&mut rng, 500);
        let node = ids[ids.len() / 3];
        let all = id_set(&tree);
        let mut h = History::default();
        h.record(snapshot(&tree, node), ChangeKind::Edit);
        let old = tree.get(node).unwrap().value();
        let value = (0..)
            .map(|_| rng.champion())
            .find(|&v| v != old && tree.set_value(node, v).is_ok())
            .unwrap();
        let (t, c) = undo(&mut h, &tree, node).unwrap();
        assert_eq!((id_set(&t), c), (all.clone(), node));
        let (t, _) = redo(&mut h, &t, c).unwrap();
        assert_eq!(id_set(&t), all);
        assert_eq!(t.get(node).unwrap().value(), value);
    }

    /// A full binary tree `depth` levels below the root (2^(depth+1) - 1 nodes).
    fn binary_tree(depth: usize) -> DraftTree {
        let mut tree = DraftTree::new();
        let mut level = vec![tree.root()];
        for d in 0..depth {
            let mut next = Vec::with_capacity(level.len() * 2);
            for &n in &level {
                for k in 0..2 {
                    let value = champions::value_of(2 * d + k);
                    next.push(tree.load_child(n, value).unwrap());
                }
            }
            level = next;
        }
        tree
    }

    /// Times undo and redo in place against building the tree anew, on a 131k-node tree.
    /// `cargo test --release history::tests::timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn timing() {
        use std::time::Instant;
        let base = binary_tree(16);
        assert_eq!(base.len(), 131_071);
        let ids = preorder(&base);
        let deep = ids[ids.len() / 2 + 5];
        // The root's second child starts the second half; 6 more steps down is a node with
        // 1023 nodes below.
        let mid = ids[ids.len() / 2 + 7];
        let near_root = base.get(base.root()).unwrap().children()[0];
        type Edit = fn(&mut DraftTree, NodeId);
        let edits: [(&str, NodeId, Edit); 6] = [
            ("delete 40 scattered leaves", near_root, |t, _| {
                let ids = preorder(t);
                for k in 0..40 {
                    let n = ids[1000 + k * 3200];
                    let leaf = (0..).try_fold(n, |n, _| {
                        t.get(n).unwrap().children().first().copied().ok_or(n)
                    });
                    let leaf = leaf.unwrap_err();
                    t.remove_subtree(leaf).unwrap();
                }
            }),
            ("mark a node", deep, |t, n| {
                t.set_marked(n, true);
            }),
            ("change a value", deep, |t, n| {
                let v = (100..).find(|&i| t.set_value(n, champions::value_of(i)).is_ok());
                assert!(v.is_some());
            }),
            ("add a leaf", deep, |t, n| {
                let leaf = *t.get(n).unwrap().children().first().unwrap_or(&n);
                let _ = t.load_child(leaf, champions::value_of(150));
            }),
            ("delete a subtree", mid, |t, n| {
                t.remove_subtree(n).unwrap();
            }),
            ("swap the root's branches", near_root, |t, n| {
                assert!(t.move_among_siblings(n, 1));
            }),
        ];
        for (name, node, edit) in edits {
            let mut tree = base.clone();
            let mut h = History::default();
            h.record(snapshot(&tree, node), ChangeKind::Edit);
            edit(&mut tree, node);
            let cursor = tree.root();
            let target = h.undo.top.as_ref().unwrap().0.clone();
            let changed = target.records.len().abs_diff(tree.len());

            let start = Instant::now();
            let (rebuilt, _) = restore(&target);
            let rebuild = start.elapsed();
            let old = tree.clone();
            let start = Instant::now();
            drop(old);
            let drop_old = start.elapsed();

            let start = Instant::now();
            let step = h.undo(&mut tree, cursor).unwrap();
            let undo = start.elapsed();
            assert!(!step.rebuilt);
            assert!(snapshot(&tree, cursor) == snapshot(&rebuilt, cursor));
            let start = Instant::now();
            h.redo(&mut tree, step.cursor).unwrap();
            let redo = start.elapsed();
            let start = Instant::now();
            let _ = snapshot(&tree, cursor);
            let snap = start.elapsed();
            println!(
                "{name:>26}: undo {undo:>9.2?}  redo {redo:>9.2?}  | rebuild {rebuild:>9.2?} + drop {drop_old:>9.2?}  (snapshot alone {snap:.2?}, {changed} nodes added/removed)"
            );
        }
    }
}
