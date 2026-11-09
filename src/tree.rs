//! Draft tree stored in an arena.
//!
//! All nodes live in one [`SlotMap`]; nodes refer to each other by [`NodeId`] instead of by
//! pointer. Every lookup goes through the slotmap, so a bad id can never touch freed memory:
//! at worst it returns `None`.

use std::fmt;
use std::sync::Arc;

use slotmap::{SecondaryMap, SlotMap, new_key_type};

use crate::champions;
use crate::draft::{Action, DraftPos};
use crate::fearless::{Fearless, LockSet, Mode, Tone};
use crate::groups::{self, Group, GroupCatalog};

// `new_key_type!` declares `NodeId` as a slotmap key: a 32-bit slot index plus a 32-bit
// version. When a slot is freed its version is bumped, so an old `NodeId` that still points
// at the slot no longer matches and every lookup with it returns `None` instead of whatever
// node reuses the slot later. The version is a `NonZeroU32`, so `Option<NodeId>` is the
// same size as `NodeId`.
new_key_type! {
    pub struct NodeId;
}

/// Node value meaning "no champion" / wildcard.
pub const EMPTY: u8 = 0;
/// Node value meaning "this ban was skipped". Only valid at ban positions.
pub const NO_BAN: u8 = 1;

/// Most children a single node may have.
pub const MAX_CHILDREN: usize = 255;

#[derive(Clone, Debug)]
pub struct Node {
    pos: DraftPos,
    value: u8,
    /// `None` only for the root.
    parent: Option<NodeId>,
    /// In insertion order.
    children: Vec<NodeId>,
    /// Role tags (see `roles.rs`); `0` = any role. Only picks have tags.
    roles: u8,
    /// The user hid this node's branches (see [`DraftTree::set_collapsed`]).
    collapsed: bool,
    /// The user marked the line ending here (see [`DraftTree::set_marked`]).
    marked: bool,
}

impl Node {
    /// Whether this node's branches are hidden. A node without children never is.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed && !self.children.is_empty()
    }

    /// Whether the user marked the line ending at this node.
    pub fn is_marked(&self) -> bool {
        self.marked
    }

    pub fn roles(&self) -> u8 {
        self.roles
    }

    pub fn pos(&self) -> DraftPos {
        self.pos
    }

    pub fn value(&self) -> u8 {
        self.value
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeError {
    /// The id doesn't refer to a live node (never existed, or its subtree was removed).
    NodeNotFound,
    /// The node is already at red pick 5; nothing comes after it.
    DraftComplete,
    TooManyChildren,
    /// A sibling already has this value.
    DuplicateSibling,
    /// The value is already used by an ancestor or descendant on the same draft line.
    ValueUsedOnPath,
    /// `NO_BAN` placed at a pick position.
    InvalidForPosition,
    /// The root can't be removed or given a value.
    RootIsFixed,
    /// The value is neither a champion, a known group nor an empty value.
    UnknownValue,
    /// Every champion of a group on this line is already accounted for.
    GroupExhausted,
    /// The champion was picked in an earlier game of a fearless series.
    FearlessLocked,
    /// The role tags don't fit with the other picks of the team on some line.
    RoleConflict,
    /// Role tags on a node that can't have them (a ban), or outside a group's role lock.
    RolesNotAllowed,
    /// The node is in a conflicting branch (see [`DraftTree::conflicts`]); nothing new can
    /// be added below it until the conflict is fixed.
    BelowConflict,
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TreeError::NodeNotFound => "node does not exist",
            TreeError::DraftComplete => "draft is already complete",
            TreeError::TooManyChildren => "node already has the maximum number of children",
            TreeError::DuplicateSibling => "a sibling already has this value",
            TreeError::ValueUsedOnPath => "value is already used earlier or later in this draft",
            TreeError::InvalidForPosition => "value is not allowed at this draft position",
            TreeError::RootIsFixed => "the root node cannot be changed or removed",
            TreeError::UnknownValue => "value is not a champion or a known group",
            TreeError::GroupExhausted => {
                "every champion of a group on this line is already accounted for"
            }
            TreeError::FearlessLocked => "the champion was picked in an earlier game of the series",
            TreeError::RoleConflict => "the role tags don't fit with the team's other picks",
            TreeError::RolesNotAllowed => "these role tags are not allowed here",
            TreeError::BelowConflict => "this branch conflicts with the line above it",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TreeError {}

/// 0 for blue, 1 for red.
fn team_index(pos: DraftPos) -> usize {
    match pos.team() {
        crate::draft::Team::Blue => 0,
        crate::draft::Team::Red => 1,
    }
}

/// Values that may repeat along one draft line (several empty picks, several skipped bans).
fn is_wildcard(value: u8) -> bool {
    value == EMPTY || value == NO_BAN
}

/// Whether two sibling values clash. Champions and groups appear at most once among
/// siblings; `EMPTY` and `NO_BAN` may repeat, so one state can have several open branches.
fn clashes(a: u8, b: u8) -> bool {
    a == b && !is_wildcard(a)
}

/// Whether choosing `value` follows the existing child holding `child`: the same value, or
/// "nothing chosen" for both (`EMPTY` and `NO_BAN` mean the same).
fn follows(child: u8, value: u8) -> bool {
    child == value || (is_wildcard(child) && is_wildcard(value))
}

/// The "nothing chosen" value for a position: `NO_BAN` at bans, `EMPTY` at picks.
pub fn empty_value_for(pos: DraftPos) -> u8 {
    match pos.action() {
        Action::Ban => NO_BAN,
        Action::Pick => EMPTY,
    }
}

fn check_value_for_pos(value: u8, pos: DraftPos) -> Result<(), TreeError> {
    if value == NO_BAN && pos.action() != Action::Ban {
        return Err(TreeError::InvalidForPosition);
    }
    Ok(())
}

/// Longest team name, in characters.
pub const MAX_NAME_CHARS: usize = 24;
/// Longest patch text, in characters.
pub const MAX_PATCH_CHARS: usize = 16;

#[derive(Clone, Debug)]
pub struct DraftTree {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
    /// Groups the node values may refer to.
    groups: Arc<GroupCatalog>,
    /// Champions unavailable from earlier games of a fearless series.
    fearless: Fearless,
    /// The teams' names (blue, red); empty for "Blue Team" / "Red Team".
    team_names: [String; 2],
    /// The game patch the prep is for, free text; may be empty.
    patch: String,
}

/// Branches removed by [`DraftTree::remove_unreachable`] or
/// [`DraftTree::remove_conflicts`], kept so they can be put back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// One entry per removed subtree, in the order they were removed (pre-order).
    branches: Vec<PrunedBranch>,
}

/// One branch in [`Pruned`]: the way down to its parent as (child index, value) steps from
/// the root, its index among the parent's children, and the subtree as (depth below the
/// parent, value, role tags) in pre-order. Indices tell repeated empty siblings apart;
/// values catch a tree that changed since.
type PrunedBranch = (Vec<(usize, u8)>, usize, Vec<(u8, u8, u8)>);

impl Pruned {
    pub fn is_empty(&self) -> bool {
        self.branches.is_empty()
    }

    /// Number of nodes stored.
    pub fn nodes(&self) -> usize {
        self.branches.iter().map(|(_, _, n)| n.len()).sum()
    }
}

/// How a node breaks the rules of its line (see [`DraftTree::conflicts`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// The first node on its line that breaks a rule, and why.
    Root(Cause),
    /// Below a conflicting node.
    Below,
}

/// Why a node conflicts with the line above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The champion is already on the line, at this node.
    Repeats(NodeId),
    /// Every champion of this group is already accounted for on the line. For a group
    /// node it is the node's own group; for a champion, a group on the line it belongs to.
    GroupFull(u8),
    /// The value is a group the active config doesn't define.
    Unknown,
}

/// Marks for the graph, from one walk over the tree (shared out across threads for large
/// trees): conflicting branches and branches fearless makes unreachable. A node has at
/// most one mark: the first problem on its line (from the root down) decides, and
/// everything below it inherits that mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub conflicts: SecondaryMap<NodeId, Conflict>,
    pub unreachable: SecondaryMap<NodeId, Tone>,
}

/// Result of [`DraftTree::replace_groups`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupSwap {
    /// User-group nodes set to the empty value.
    pub emptied: usize,
}

/// What one draft line (root to some node) has used so far. Decides whether a value may
/// come next:
///
/// - a champion may appear once per line;
/// - a group may appear as often as it still has champions left: a group of `n` champions
///   used `k` times, with `e` of its champions also on the line explicitly, needs
///   `k + e < n` to be used again;
/// - the same count limits explicit champions: once `k + e == n`, the group's remaining
///   champions are all spoken for and can't be picked or banned further down the line.
///
/// Overlapping groups are checked independently of each other.
///
/// Fearless bans for the position being checked (see [`set_locked`](Self::set_locked))
/// count like champions already on the line: they can't be chosen, and they use up a
/// slot of every group they belong to.
#[derive(Clone)]
pub struct LineState<'a> {
    /// Champions locked by fearless at the position being checked.
    locked: LockSet,
    groups: &'a GroupCatalog,
    /// Champions on the line.
    taken: [bool; 256],
    /// How often each group appears on the line.
    group_uses: [u8; 256],
    /// For groups on the line: how many of their champions are on the line explicitly.
    group_taken: [u8; 256],
    /// Groups on the line, in order of first appearance.
    line_groups: Vec<u8>,
}

impl<'a> LineState<'a> {
    pub fn new(groups: &'a GroupCatalog) -> Self {
        LineState {
            locked: LockSet::default(),
            groups,
            taken: [false; 256],
            group_uses: [0; 256],
            group_taken: [0; 256],
            line_groups: Vec::new(),
        }
    }

    fn slots_left(&self, group: &Group) -> usize {
        let v = usize::from(group.value);
        let taken = if self.group_uses[v] > 0 {
            usize::from(self.group_taken[v])
        } else {
            group
                .members
                .iter()
                .filter(|&&m| self.taken[usize::from(m)])
                .count()
        };
        // Fearless-locked members not already on the line.
        let locked = if self.locked.is_empty() {
            0
        } else {
            group
                .members
                .iter()
                .filter(|&&m| self.locked.contains(m) && !self.taken[usize::from(m)])
                .count()
        };
        group
            .len()
            .saturating_sub(usize::from(self.group_uses[v]) + taken + locked)
    }

    /// Sets the fearless bans for the position the next [`check`](Self::check) is for.
    pub fn set_locked(&mut self, locked: LockSet) {
        self.locked = locked;
    }

    /// Whether `value` may come next on this line (draft position rules aside).
    pub fn check(&self, value: u8) -> Result<(), TreeError> {
        if is_wildcard(value) {
            return Ok(());
        }
        if groups::is_group(value) {
            let group = self.groups.get(value).ok_or(TreeError::UnknownValue)?;
            if self.slots_left(group) == 0 {
                return Err(TreeError::GroupExhausted);
            }
            return Ok(());
        }
        if champions::by_value(value).is_none() {
            return Err(TreeError::UnknownValue);
        }
        if self.taken[usize::from(value)] {
            return Err(TreeError::ValueUsedOnPath);
        }
        if self.locked.contains(value) {
            return Err(TreeError::FearlessLocked);
        }
        for group in self.groups_on_line() {
            if group.contains(value) && self.slots_left(group) == 0 {
                return Err(TreeError::GroupExhausted);
            }
        }
        Ok(())
    }

    /// Adds `value` to the line. Call [`check`](Self::check) first.
    pub fn push(&mut self, value: u8) {
        if is_wildcard(value) {
            return;
        }
        let v = usize::from(value);
        if groups::is_group(value) {
            if self.group_uses[v] == 0 {
                if let Some(group) = self.groups.get(value) {
                    let taken = group
                        .members
                        .iter()
                        .filter(|&&m| self.taken[usize::from(m)]);
                    self.group_taken[v] = taken.count() as u8;
                }
                self.line_groups.push(value);
            }
            self.group_uses[v] = self.group_uses[v].saturating_add(1);
        } else {
            self.taken[v] = true;
            for &g in &self.line_groups {
                if self
                    .groups
                    .get(g)
                    .is_some_and(|group| group.contains(value))
                {
                    let slot = &mut self.group_taken[usize::from(g)];
                    *slot = slot.saturating_add(1);
                }
            }
        }
    }

    /// The group that stops `value` coming next: `value` itself for a group with no
    /// champions left, or for a champion a group on the line whose champions are all
    /// spoken for. Fearless bans count as set by [`set_locked`](Self::set_locked).
    pub fn full_group_for(&self, value: u8) -> Option<u8> {
        if groups::is_group(value) {
            let group = self.groups.get(value)?;
            return (self.slots_left(group) == 0).then_some(value);
        }
        self.groups_on_line()
            .find(|g| g.contains(value) && self.slots_left(g) == 0)
            .map(|g| g.value)
    }

    /// Whether `champion` is on the line explicitly.
    pub fn is_taken(&self, champion: u8) -> bool {
        self.taken[usize::from(champion)]
    }

    /// Groups on the line, in order of first appearance.
    pub fn groups_on_line(&self) -> impl Iterator<Item = &'a Group> + '_ {
        self.line_groups.iter().filter_map(|&g| self.groups.get(g))
    }
}

/// Most nodes a tree may have, root excluded. Larger files don't load and larger merges
/// are refused: the app stays responsive up to here.
pub const MAX_NODES: usize = 250_000;

/// From this many nodes on, [`DraftTree::marks`] walks subtrees in parallel. Below it,
/// sharing out the work costs more than it saves.
pub const PARALLEL_MARKS_MIN: usize = 20_000;

/// A mark a node passes down to everything below it (see [`Marks`]).
#[derive(Clone, Copy)]
enum Inherit {
    None,
    Conflict,
    Fearless(Tone),
}

/// A node whose children [`DraftTree::marks`] still has to mark: the line above its
/// children, and the mark it passes down.
struct MarkTask<'a> {
    id: NodeId,
    line: LineState<'a>,
    inherited: Inherit,
}

impl<'a> MarkTask<'a> {
    fn root(tree: &'a DraftTree) -> Self {
        MarkTask {
            id: tree.root,
            line: LineState::new(&tree.groups),
            inherited: Inherit::None,
        }
    }
}

/// Where marks go: straight into [`Marks`], or into lists a parallel walk merges later.
trait MarkSink {
    fn conflict(&mut self, id: NodeId, c: Conflict);
    fn unreachable(&mut self, id: NodeId, t: Tone);
}

impl MarkSink for Marks {
    fn conflict(&mut self, id: NodeId, c: Conflict) {
        self.conflicts.insert(id, c);
    }
    fn unreachable(&mut self, id: NodeId, t: Tone) {
        self.unreachable.insert(id, t);
    }
}

#[derive(Default)]
struct MarkList {
    conflicts: Vec<(NodeId, Conflict)>,
    unreachable: Vec<(NodeId, Tone)>,
}

impl MarkSink for MarkList {
    fn conflict(&mut self, id: NodeId, c: Conflict) {
        self.conflicts.push((id, c));
    }
    fn unreachable(&mut self, id: NodeId, t: Tone) {
        self.unreachable.push((id, t));
    }
}

impl Default for DraftTree {
    fn default() -> Self {
        Self::new()
    }
}

impl DraftTree {
    /// A tree holding only the root (the empty draft).
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        // `insert` stores the node and hands back the key that now refers to it.
        let root = nodes.insert(Node {
            pos: DraftPos::ROOT,
            value: EMPTY,
            parent: None,
            children: Vec::new(),
            roles: 0,
            collapsed: false,
            marked: false,
        });
        DraftTree {
            nodes,
            root,
            groups: Arc::new(GroupCatalog::presets_only()),
            fearless: Fearless::default(),
            team_names: Default::default(),
            patch: String::new(),
        }
    }

    /// An empty tree whose values may refer to the groups in `groups`.
    pub fn with_groups(groups: Arc<GroupCatalog>) -> Self {
        DraftTree {
            groups,
            ..Self::new()
        }
    }

    pub fn groups(&self) -> &Arc<GroupCatalog> {
        &self.groups
    }

    pub fn fearless(&self) -> &Fearless {
        &self.fearless
    }

    /// Replaces the fearless bans. Existing nodes are kept even if they are now locked;
    /// see [`unreachable`](Self::unreachable).
    pub fn set_fearless(&mut self, fearless: Fearless) {
        self.fearless = fearless;
    }

    /// The line from the root down to `id` (root excluded), replayed into a [`LineState`].
    pub fn line_state(&self, id: NodeId) -> LineState<'_> {
        let mut line = LineState::new(&self.groups);
        for n in self.path(id).into_iter().skip(1) {
            line.push(self.nodes[n].value);
        }
        line
    }

    /// Whether `value` could be added as a child of `parent`, ignoring whether that child
    /// already exists. Used to grey out tiles.
    pub fn line_allows(&self, parent: NodeId, value: u8) -> bool {
        let Some(pos) = self.nodes.get(parent).and_then(|n| n.pos.next()) else {
            return false;
        };
        check_value_for_pos(value, pos).is_ok() && self.next_line_state(parent).check(value).is_ok()
    }

    /// [`line_state`](Self::line_state) of `parent`, with the fearless bans of the position
    /// after it applied: the state that decides what may be chosen next.
    pub fn next_line_state(&self, parent: NodeId) -> LineState<'_> {
        let mut line = self.line_state(parent);
        if let Some(pos) = self.nodes.get(parent).and_then(|n| n.pos.next()) {
            line.set_locked(self.fearless.locked_at(pos));
        }
        line
    }

    /// The fearless color for a node at `pos` that fearless makes unreachable: hard
    /// fearless for both teams, soft fearless for the team choosing at `pos`.
    fn tone_at(&self, pos: DraftPos) -> Tone {
        match (self.fearless.mode, pos.team()) {
            (Mode::Hard, _) => Tone::Both,
            (Mode::Soft, crate::draft::Team::Blue) => Tone::Blue,
            (Mode::Soft, crate::draft::Team::Red) => Tone::Red,
        }
    }

    /// Conflicting branches and fearless-unreachable branches (see [`Marks`]).
    ///
    /// A node *conflicts* when its value breaks a line rule given the nodes above it: a
    /// champion already on the line, a group with no champions left, or a group the config
    /// no longer has. The GUI never builds such a line directly, but editing a value higher
    /// up (see [`set_value`](Self::set_value)) can create one below, and the branch is kept
    /// rather than deleted so changing the value back restores it. A node is
    /// *unreachable* when only fearless bans stop its value.
    pub fn marks(&self) -> Marks {
        if self.nodes.len() >= PARALLEL_MARKS_MIN {
            self.marks_parallel()
        } else {
            self.marks_sequential()
        }
    }

    /// [`marks`](Self::marks) in one walk on this thread.
    pub(crate) fn marks_sequential(&self) -> Marks {
        let mut marks = Marks::default();
        let mut stack = vec![MarkTask::root(self)];
        while let Some(task) = stack.pop() {
            self.mark_children(task, &mut marks, &mut stack);
        }
        marks
    }

    /// [`marks`](Self::marks) with independent subtrees walked in parallel. The top of the
    /// tree is walked here until there are enough subtrees to share out; each subtree's
    /// marks depend only on the line above it, so the result is the same.
    pub(crate) fn marks_parallel(&self) -> Marks {
        use rayon::prelude::*;
        // Enough subtrees that uneven ones still keep every core busy.
        const MIN_TASKS: usize = 256;
        let mut marks = Marks::default();
        let mut frontier = vec![MarkTask::root(self)];
        while !frontier.is_empty() && frontier.len() < MIN_TASKS {
            let mut next = Vec::new();
            for task in frontier {
                self.mark_children(task, &mut marks, &mut next);
            }
            frontier = next;
        }
        let parts: Vec<MarkList> = frontier
            .into_par_iter()
            .map(|task| {
                let mut out = MarkList::default();
                let mut stack = vec![task];
                while let Some(task) = stack.pop() {
                    self.mark_children(task, &mut out, &mut stack);
                }
                out
            })
            .collect();
        // Sized for every slot up front: merging is the part that can't be shared out.
        let (conflicts, unreachable) = parts.iter().fold((0, 0), |(c, u), p| {
            (c + p.conflicts.len(), u + p.unreachable.len())
        });
        if conflicts > 0 {
            marks.conflicts.set_capacity(self.nodes.capacity());
        }
        if unreachable > 0 {
            marks.unreachable.set_capacity(self.nodes.capacity());
        }
        for part in parts {
            for (id, c) in part.conflicts {
                marks.conflicts.insert(id, c);
            }
            for (id, t) in part.unreachable {
                marks.unreachable.insert(id, t);
            }
        }
        marks
    }

    /// Marks the children of `task`'s node and queues those with children of their own.
    fn mark_children<'a>(
        &'a self,
        task: MarkTask<'a>,
        out: &mut impl MarkSink,
        queue: &mut Vec<MarkTask<'a>>,
    ) {
        let MarkTask {
            id,
            line,
            inherited,
        } = task;
        let fearless = self.fearless.is_active();
        for &c in &self.nodes[id].children {
            let child = &self.nodes[c];
            let mark = match inherited {
                Inherit::Conflict => {
                    out.conflict(c, Conflict::Below);
                    Inherit::Conflict
                }
                Inherit::Fearless(t) => {
                    out.unreachable(c, t);
                    Inherit::Fearless(t)
                }
                Inherit::None => {
                    if line.check(child.value).is_err() {
                        let cause = self.cause_with(c, &line);
                        out.conflict(c, Conflict::Root(cause));
                        Inherit::Conflict
                    } else if fearless && {
                        let mut probe = line.clone();
                        probe.set_locked(self.fearless.locked_at(child.pos));
                        probe.check(child.value).is_err()
                    } {
                        let t = self.tone_at(child.pos);
                        out.unreachable(c, t);
                        Inherit::Fearless(t)
                    } else {
                        Inherit::None
                    }
                }
            };
            if child.children.is_empty() {
                continue;
            }
            let next = match mark {
                // Below a mark nothing is checked any more; skip the line upkeep.
                Inherit::None => {
                    let mut next = line.clone();
                    next.push(child.value);
                    next
                }
                _ => line.clone(),
            };
            queue.push(MarkTask {
                id: c,
                line: next,
                inherited: mark,
            });
        }
    }

    /// Nodes that fearless makes unreachable (see [`marks`](Self::marks)): nodes whose value
    /// is locked at their position (a fearless champion, or a group with no champions
    /// left), and everything below them. Each maps to the tone of the node that caused it.
    pub fn unreachable(&self) -> SecondaryMap<NodeId, Tone> {
        self.marks().unreachable
    }

    /// Whether fearless makes `id` unreachable (see [`unreachable`](Self::unreachable)),
    /// found from its line alone rather than the whole tree's marks.
    pub fn line_unreachable(&self, id: NodeId) -> bool {
        if !self.fearless.is_active() || !self.nodes.contains_key(id) {
            return false;
        }
        let mut line = LineState::new(&self.groups);
        for n in self.path(id).into_iter().skip(1) {
            let node = &self.nodes[n];
            // Below a conflict, nodes are marked conflicting, not unreachable.
            if line.check(node.value).is_err() {
                return false;
            }
            let mut probe = line.clone();
            probe.set_locked(self.fearless.locked_at(node.pos));
            if probe.check(node.value).is_err() {
                return true;
            }
            line.push(node.value);
        }
        false
    }

    /// Conflicting nodes (see [`marks`](Self::marks)).
    pub fn conflicts(&self) -> SecondaryMap<NodeId, Conflict> {
        self.marks().conflicts
    }

    /// Why `id` breaks its line, given `line`, the state of the line above it.
    fn cause_with(&self, id: NodeId, line: &LineState<'_>) -> Cause {
        let value = self.nodes[id].value;
        if groups::is_group(value) && self.groups.get(value).is_none() {
            return Cause::Unknown;
        }
        if line.is_taken(value)
            && let Some(at) = self
                .ancestors(self.nodes[id].parent.expect("not the root"))
                .find(|&n| self.nodes[n].value == value)
        {
            return Cause::Repeats(at);
        }
        line.full_group_for(value)
            .map_or(Cause::Unknown, Cause::GroupFull)
    }

    /// The topmost conflicting node on the line from the root down to `id`, if any. New
    /// branches can't start at `id` while there is one.
    pub fn line_conflict(&self, id: NodeId) -> Option<NodeId> {
        let mut line = LineState::new(&self.groups);
        for n in self.path(id).into_iter().skip(1) {
            let value = self.nodes[n].value;
            if line.check(value).is_err() {
                return Some(n);
            }
            line.push(value);
        }
        None
    }

    /// Removes every unreachable branch (see [`unreachable`](Self::unreachable)) and
    /// returns them, so [`restore_pruned`](Self::restore_pruned) can put them back.
    pub fn remove_unreachable(&mut self) -> Pruned {
        let marked = self.unreachable();
        self.remove_marked(|n| marked.contains_key(n))
    }

    /// Removes every conflicting branch (see [`conflicts`](Self::conflicts)) and returns
    /// them, so [`restore_pruned`](Self::restore_pruned) can put them back.
    pub fn remove_conflicts(&mut self) -> Pruned {
        let marked = self.conflicts();
        self.remove_marked(|n| marked.contains_key(n))
    }

    /// Removes the topmost nodes satisfying `marked` with their subtrees.
    fn remove_marked(&mut self, marked: impl Fn(NodeId) -> bool) -> Pruned {
        // Topmost marked nodes, in pre-order.
        let mut tops = Vec::new();
        let mut stack = vec![self.root];
        while let Some(n) = stack.pop() {
            if n != self.root && marked(n) {
                tops.push(n);
            } else {
                stack.extend(self.nodes[n].children.iter().rev());
            }
        }
        let mut pruned = Pruned::default();
        for top in tops {
            let parent = self.nodes[top].parent.expect("the root is never marked");
            let way = self.way_to(parent);
            let index = self.index_in_parent(top);
            let mut nodes = Vec::new();
            let mut walk = vec![(top, 0u8)];
            while let Some((n, depth)) = walk.pop() {
                nodes.push((depth, self.nodes[n].value, self.nodes[n].roles));
                walk.extend(self.nodes[n].children.iter().rev().map(|&c| (c, depth + 1)));
            }
            pruned.branches.push((way, index, nodes));
            self.remove_subtree(top).expect("live non-root node");
        }
        pruned
    }

    /// `id`'s index among its parent's children. The root counts as index 0.
    fn index_in_parent(&self, id: NodeId) -> usize {
        self.nodes[id].parent.map_or(0, |p| {
            self.nodes[p]
                .children
                .iter()
                .position(|&c| c == id)
                .expect("child of its parent")
        })
    }

    /// The way from the root down to `id` as (child index, value) steps.
    fn way_to(&self, id: NodeId) -> Vec<(usize, u8)> {
        self.path(id)
            .into_iter()
            .skip(1)
            .map(|n| (self.index_in_parent(n), self.nodes[n].value))
            .collect()
    }

    /// Follows a way from [`way_to`](Self::way_to): by index while the values still match,
    /// else by value (champions and groups are unique among siblings). `None` if the node
    /// is gone.
    fn follow_way(&self, way: &[(usize, u8)]) -> Option<NodeId> {
        let mut at = self.root;
        for &(index, value) in way {
            let children = &self.nodes[at].children;
            at = match children.get(index) {
                Some(&c) if self.nodes[c].value == value => c,
                _ if is_wildcard(value) => return None,
                _ => *children.iter().find(|&&c| self.nodes[c].value == value)?,
            };
        }
        Some(at)
    }

    /// Puts branches from [`remove_unreachable`](Self::remove_unreachable) or
    /// [`remove_conflicts`](Self::remove_conflicts) back, exactly as they were if the tree
    /// hasn't changed since. Branches whose parent no longer exists are skipped; a champion
    /// or group that exists again among the siblings is merged with it. Line rules are not
    /// checked: the branches come back with the same marks they had. Returns the number of
    /// nodes added.
    pub fn restore_pruned(&mut self, pruned: &Pruned) -> usize {
        let before = self.len();
        // Last removed first, so every index refers to the siblings it was taken from.
        for (way, index, nodes) in pruned.branches.iter().rev() {
            let Some(parent) = self.follow_way(way) else {
                continue;
            };
            // `ids[d]` is the node at depth `d` below `parent` on the current branch.
            let mut ids = vec![parent];
            let mut skip_below: Option<u8> = None;
            for &(depth, value, roles) in nodes {
                if let Some(s) = skip_below {
                    if depth > s {
                        continue;
                    }
                    skip_below = None;
                }
                ids.truncate(usize::from(depth) + 1);
                let at = ids[usize::from(depth)];
                let existing = self.nodes[at]
                    .children
                    .iter()
                    .copied()
                    .find(|&c| clashes(self.nodes[c].value, value));
                let id = match existing {
                    Some(id) => id,
                    None => {
                        let place = (depth == 0).then_some(*index);
                        match self.insert_raw(at, value, 0, place) {
                            Ok(id) => id,
                            Err(_) => {
                                skip_below = Some(depth);
                                continue;
                            }
                        }
                    }
                };
                // Tags only where they still fit: the tree may have changed since.
                if roles != 0
                    && self.nodes[id].roles == 0
                    && self.roles_fit(id, roles, false).is_ok()
                {
                    self.nodes[id].roles = roles;
                }
                ids.push(id);
            }
        }
        self.len() - before
    }

    /// Adds a child without checking line rules, at `index` among the children (clamped)
    /// or last. Still refuses what no tree may hold: a child past red pick 5, `NO_BAN` at
    /// a pick, more than [`MAX_CHILDREN`] children, or tags on a ban.
    pub(crate) fn insert_raw(
        &mut self,
        parent: NodeId,
        value: u8,
        roles: u8,
        index: Option<usize>,
    ) -> Result<NodeId, TreeError> {
        let parent_node = self.nodes.get(parent).ok_or(TreeError::NodeNotFound)?;
        let pos = parent_node.pos.next().ok_or(TreeError::DraftComplete)?;
        check_value_for_pos(value, pos)?;
        if parent_node.children.len() >= MAX_CHILDREN {
            return Err(TreeError::TooManyChildren);
        }
        let roles = if pos.action() == Action::Pick {
            roles & crate::roles::ALL
        } else {
            0
        };
        let id = self.nodes.insert(Node {
            pos,
            value,
            parent: Some(parent),
            children: Vec::new(),
            roles,
            collapsed: false,
            marked: false,
        });
        let children = &mut self.nodes[parent].children;
        match index {
            Some(i) => children.insert(i.min(children.len()), id),
            None => children.push(id),
        }
        Ok(id)
    }

    /// Sets role tags without any check, for merging (see `merge.rs`), which only ever
    /// clears them.
    pub(crate) fn set_roles_raw(&mut self, id: NodeId, roles: u8) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.roles = roles;
        }
    }

    /// Adds a child while loading a saved tree. Like [`add_child`](Self::add_child), but
    /// line rules are not checked, since a saved tree may hold conflicting branches (see
    /// [`conflicts`](Self::conflicts)). Values must still be champions, empty values or
    /// groups the active config defines, and champions and groups can't repeat among
    /// siblings.
    pub fn load_child(&mut self, parent: NodeId, value: u8) -> Result<NodeId, TreeError> {
        let known = is_wildcard(value)
            || champions::by_value(value).is_some()
            || self.groups.get(value).is_some();
        if !known {
            return Err(TreeError::UnknownValue);
        }
        let parent_node = self.nodes.get(parent).ok_or(TreeError::NodeNotFound)?;
        if parent_node
            .children
            .iter()
            .any(|&c| clashes(self.nodes[c].value, value))
        {
            return Err(TreeError::DuplicateSibling);
        }
        self.insert_raw(parent, value, 0, None)
    }

    /// Whether `id` can carry role tags: a pick (champion, group or empty).
    pub fn can_tag(&self, id: NodeId) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|n| n.parent.is_some() && n.pos.action() == Action::Pick)
    }

    /// Roles a group node may be tagged with: its role lock, or every role.
    pub fn role_limit(&self, value: u8) -> u8 {
        match self.groups.get(value) {
            Some(g) if g.roles != 0 => g.roles,
            _ => crate::roles::ALL,
        }
    }

    /// Tags of the picks on the line from the root to `id` (`id` included), per team
    /// (blue first), as (node, tags) in draft order.
    pub fn team_picks(&self, id: NodeId) -> [Vec<(NodeId, u8)>; 2] {
        let mut out: [Vec<(NodeId, u8)>; 2] = [Vec::new(), Vec::new()];
        for n in self.path(id).into_iter().skip(1) {
            let node = &self.nodes[n];
            if node.pos.action() == Action::Pick {
                out[team_index(node.pos)].push((n, node.roles));
            }
        }
        out
    }

    /// Whether `id` could have the tags `mask` without breaking any line through it. With
    /// `respect_lock`, a group's role lock limits the tags too.
    pub fn roles_fit(&self, id: NodeId, mask: u8, respect_lock: bool) -> Result<(), TreeError> {
        let node = self.nodes.get(id).ok_or(TreeError::NodeNotFound)?;
        if mask & !crate::roles::ALL != 0 || (mask != 0 && !self.can_tag(id)) {
            return Err(TreeError::RolesNotAllowed);
        }
        if respect_lock && mask & !self.role_limit(node.value) != 0 {
            return Err(TreeError::RolesNotAllowed);
        }
        let parent = node.parent.ok_or(TreeError::RootIsFixed)?;
        // The line above `id`, then `id` with its new tags.
        let mut prefix = self.team_picks(parent).map(|picks| {
            picks
                .into_iter()
                .map(|(_, m)| m)
                .filter(|&m| m != 0)
                .collect::<Vec<u8>>()
        });
        if mask != 0 {
            prefix[team_index(node.pos)].push(mask);
        }
        if !crate::roles::assignable(&prefix[team_index(node.pos)]) {
            return Err(TreeError::RoleConflict);
        }
        // Every line below `id`.
        let mut stack: Vec<(NodeId, [Vec<u8>; 2])> =
            node.children.iter().map(|&c| (c, prefix.clone())).collect();
        while let Some((n, mut lines)) = stack.pop() {
            let child = &self.nodes[n];
            if child.roles != 0 && child.pos.action() == Action::Pick {
                let team = team_index(child.pos);
                lines[team].push(child.roles);
                if !crate::roles::assignable(&lines[team]) {
                    return Err(TreeError::RoleConflict);
                }
            }
            for &c in &child.children {
                stack.push((c, lines.clone()));
            }
        }
        Ok(())
    }

    /// Whether a new child of `parent` tagged `mask` would fit with its team's picks.
    pub fn child_roles_fit(&self, parent: NodeId, mask: u8) -> bool {
        if mask == 0 {
            return true;
        }
        let Some(pos) = self.nodes.get(parent).and_then(|n| n.pos.next()) else {
            return false;
        };
        if pos.action() != Action::Pick {
            return false;
        }
        let mut masks: Vec<u8> = self.team_picks(parent)[team_index(pos)]
            .iter()
            .map(|&(_, m)| m)
            .collect();
        masks.push(mask);
        crate::roles::assignable(&masks)
    }

    /// Sets the role tags of `id` (`0` = any role), keeping every line valid.
    pub fn set_roles(&mut self, id: NodeId, mask: u8) -> Result<(), TreeError> {
        self.roles_fit(id, mask, true)?;
        self.nodes[id].roles = mask;
        Ok(())
    }

    /// Sets role tags without checking group locks, e.g. when loading a file whose group
    /// config has changed since. Lines must still fit.
    pub fn set_roles_unlocked(&mut self, id: NodeId, mask: u8) -> Result<(), TreeError> {
        self.roles_fit(id, mask, false)?;
        self.nodes[id].roles = mask;
        Ok(())
    }

    /// Adds a child like [`add_child`](Self::add_child), tagged with `mask`. If the tags
    /// don't fit, the child is added untagged.
    pub fn add_child_tagged(
        &mut self,
        parent: NodeId,
        value: u8,
        mask: u8,
    ) -> Result<NodeId, TreeError> {
        let id = self.add_child(parent, value)?;
        if mask != 0 && self.set_roles(id, mask).is_err() {
            // Keep the pick; just leave it untagged.
        }
        Ok(id)
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Looks up a node. `None` if the id is stale or from another tree's removed node.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        // `SlotMap::get` checks both the slot index and its version.
        self.nodes.get(id)
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Number of live nodes, root included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// The immediate child of `parent` that choosing `value` leads to, if any. `EMPTY` and
    /// `NO_BAN` match each other; with several empty children, the first one.
    pub fn child_with_value(&self, parent: NodeId, value: u8) -> Option<NodeId> {
        let node = self.nodes.get(parent)?;
        node.children
            .iter()
            .copied()
            .find(|&c| follows(self.nodes[c].value, value))
    }

    /// `id`'s sibling holding the champion or group `value`, if any. Never an empty
    /// sibling, since empty values may repeat.
    pub fn sibling_with_value(&self, id: NodeId, value: u8) -> Option<NodeId> {
        let parent = self.nodes.get(id)?.parent?;
        self.nodes[parent]
            .children
            .iter()
            .copied()
            .find(|&c| c != id && clashes(self.nodes[c].value, value))
    }

    /// For a node with empty siblings: its number among them (1, 2, ...) when there are
    /// at least two, to tell them apart in the graph. `None` otherwise.
    pub fn empty_number(&self, id: NodeId) -> Option<usize> {
        let node = self.nodes.get(id)?;
        if !is_wildcard(node.value) {
            return None;
        }
        let siblings = &self.nodes[node.parent?].children;
        let empties: Vec<NodeId> = siblings
            .iter()
            .copied()
            .filter(|&c| is_wildcard(self.nodes[c].value))
            .collect();
        (empties.len() > 1).then(|| empties.iter().position(|&c| c == id).map(|i| i + 1))?
    }

    /// Whether `parent` already has an immediate child holding `value` (see
    /// [`child_with_value`](Self::child_with_value)).
    pub fn has_child_with_value(&self, parent: NodeId, value: u8) -> bool {
        self.child_with_value(parent, value).is_some()
    }

    /// Iterates from `id` up to the root, both included. Empty if `id` is not live.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let start = self.contains(id).then_some(id);
        std::iter::successors(start, |&n| self.nodes[n].parent)
    }

    /// Node ids from the root down to `id`, both included. Empty if `id` is not live.
    pub fn path(&self, id: NodeId) -> Vec<NodeId> {
        let mut path: Vec<NodeId> = self.ancestors(id).collect();
        path.reverse();
        path
    }

    /// Whether `value` appears on `id` or any of its ancestors (root excluded).
    pub fn value_on_path(&self, id: NodeId, value: u8) -> bool {
        self.ancestors(id)
            .filter(|&n| n != self.root)
            .any(|n| self.nodes[n].value == value)
    }

    /// Adds a child holding `value` below `parent`, at the draft position after the
    /// parent's. Returns the new node's id.
    pub fn add_child(&mut self, parent: NodeId, value: u8) -> Result<NodeId, TreeError> {
        let parent_node = self.nodes.get(parent).ok_or(TreeError::NodeNotFound)?;
        let pos = parent_node.pos.next().ok_or(TreeError::DraftComplete)?;
        check_value_for_pos(value, pos)?;
        if parent_node.children.len() >= MAX_CHILDREN {
            return Err(TreeError::TooManyChildren);
        }
        if parent_node
            .children
            .iter()
            .any(|&c| clashes(self.nodes[c].value, value))
        {
            return Err(TreeError::DuplicateSibling);
        }
        if self.line_conflict(parent).is_some() {
            return Err(TreeError::BelowConflict);
        }
        self.next_line_state(parent).check(value)?;

        let id = self.nodes.insert(Node {
            pos,
            value,
            parent: Some(parent),
            children: Vec::new(),
            roles: 0,
            collapsed: false,
            marked: false,
        });
        // Indexing a SlotMap with a key panics if the key is stale; `parent` was checked above.
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// Returns `parent`'s child holding `value`, adding it first if there is none. This is
    /// what choosing a champion does: follow an existing branch or start a new one.
    pub fn get_or_add_child(&mut self, parent: NodeId, value: u8) -> Result<NodeId, TreeError> {
        match self.child_with_value(parent, value) {
            Some(child) => Ok(child),
            None => self.add_child(parent, value),
        }
    }

    /// Changes the value stored in `id`, keeping its subtree.
    ///
    /// The new value must fit the line *above* `id` (fearless bans at its position
    /// included) and must not repeat a sibling's champion or group. The lines *below* are
    /// not checked: nodes there that now break a rule become conflicting branches (see
    /// [`conflicts`](Self::conflicts)) and stay in the tree, so changing the value back
    /// makes them normal again.
    ///
    /// Role tags of a pick are adjusted to a group's role lock: narrowed to the lock, or
    /// set to it when they don't overlap, or cleared if that wouldn't fit the team.
    pub fn set_value(&mut self, id: NodeId, value: u8) -> Result<(), TreeError> {
        let node = self.nodes.get(id).ok_or(TreeError::NodeNotFound)?;
        let parent = node.parent.ok_or(TreeError::RootIsFixed)?;
        if node.value == value {
            return Ok(());
        }
        check_value_for_pos(value, node.pos)?;
        if self.sibling_with_value(id, value).is_some() {
            return Err(TreeError::DuplicateSibling);
        }
        self.next_line_state(parent).check(value)?;
        let old_roles = node.roles;
        self.nodes[id].value = value;
        if old_roles != 0 || self.role_limit(value) != crate::roles::ALL {
            let limit = self.role_limit(value);
            let wanted = match (old_roles & limit, limit) {
                (0, crate::roles::ALL) => old_roles,
                (0, lock) => lock,
                (narrowed, _) => narrowed,
            };
            let roles = if self.can_tag(id) && self.roles_fit(id, wanted, true).is_ok() {
                wanted
            } else {
                0
            };
            self.nodes[id].roles = roles;
        }
        Ok(())
    }

    /// Copies the subtree of `id` (values and role tags) as a new sibling right after `id`.
    /// The copy's top node gets the empty value for its position, since it can't repeat
    /// the original's champion. Returns the copy of `id`.
    pub fn duplicate_branch(&mut self, id: NodeId) -> Result<NodeId, TreeError> {
        let node = self.nodes.get(id).ok_or(TreeError::NodeNotFound)?;
        let parent = node.parent.ok_or(TreeError::RootIsFixed)?;
        let (pos, roles) = (node.pos, node.roles);
        let index = self.index_in_parent(id) + 1;
        let copy = self.insert_raw(parent, empty_value_for(pos), roles, Some(index))?;
        self.copy_children(id, copy);
        Ok(copy)
    }

    /// Copies the line from the first draft action down to `id` as a new branch of the
    /// root, right after the original's first node, with the copy's first node set to the
    /// empty value (`NO_BAN` for blue ban 1). With `subtree`, everything below `id` is
    /// copied too; otherwise the copy ends at `id`. Returns the copy of `id`.
    pub fn duplicate_line(&mut self, id: NodeId, subtree: bool) -> Result<NodeId, TreeError> {
        if !self.contains(id) {
            return Err(TreeError::NodeNotFound);
        }
        let path = self.path(id);
        let Some(&first) = path.get(1) else {
            return Err(TreeError::RootIsFixed);
        };
        let first_node = &self.nodes[first];
        let (pos, roles) = (first_node.pos, first_node.roles);
        let index = self.index_in_parent(first) + 1;
        let mut copy = self.insert_raw(self.root, empty_value_for(pos), roles, Some(index))?;
        for &n in &path[2..] {
            let (value, roles) = (self.nodes[n].value, self.nodes[n].roles);
            copy = self.insert_raw(copy, value, roles, None)?;
        }
        if subtree {
            self.copy_children(id, copy);
        }
        Ok(copy)
    }

    /// Copies every node below `from` (values and role tags, in order) below `to`.
    fn copy_children(&mut self, from: NodeId, to: NodeId) {
        let mut stack: Vec<(NodeId, NodeId)> = self.nodes[from]
            .children
            .iter()
            .rev()
            .map(|&c| (c, to))
            .collect();
        while let Some((src, dst_parent)) = stack.pop() {
            let (value, roles) = (self.nodes[src].value, self.nodes[src].roles);
            let dst = self
                .insert_raw(dst_parent, value, roles, None)
                .expect("a copy has the same shape as its original");
            stack.extend(self.nodes[src].children.iter().rev().map(|&c| (c, dst)));
        }
    }

    /// Sets `id` to the "nothing chosen" value for its position (see [`empty_value_for`]),
    /// keeping its subtree.
    pub fn set_empty(&mut self, id: NodeId) -> Result<(), TreeError> {
        let pos = self.nodes.get(id).ok_or(TreeError::NodeNotFound)?.pos;
        self.set_value(id, empty_value_for(pos))
    }

    /// The next (or, with `forward == false`, previous) node at the same depth as `id`, in
    /// the graph's top-to-bottom order, wrapping around. Branches that end above this depth
    /// have no node there and are skipped. `None` for the root or when `id` is the only
    /// node at its depth.
    pub fn cycle_same_depth(&self, id: NodeId, forward: bool) -> Option<NodeId> {
        let target = self.nodes.get(id)?.pos;
        if target == DraftPos::ROOT {
            return None;
        }
        // Every node at one depth has the same position byte, so the walk collects nodes
        // with `target` and never descends below them.
        let mut level = Vec::new();
        let mut stack = vec![self.root];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            if node.pos == target {
                level.push(n);
            } else {
                stack.extend(node.children.iter().rev());
            }
        }
        if level.len() < 2 {
            return None;
        }
        let i = level.iter().position(|&n| n == id)?;
        let next = if forward {
            (i + 1) % level.len()
        } else {
            (i + level.len() - 1) % level.len()
        };
        Some(level[next])
    }

    /// Removes `id` and everything below it. Returns how many nodes were removed.
    ///
    /// Every removed id becomes stale: lookups with it return `None` from now on.
    pub fn remove_subtree(&mut self, id: NodeId) -> Result<usize, TreeError> {
        let node = self.nodes.get(id).ok_or(TreeError::NodeNotFound)?;
        let parent = node.parent.ok_or(TreeError::RootIsFixed)?;

        // Unlink from the parent first so no live node ever points at a removed one.
        // `Vec::remove` shifts later siblings left, keeping child order intact.
        let siblings = &mut self.nodes[parent].children;
        if let Some(i) = siblings.iter().position(|&c| c == id) {
            siblings.remove(i);
        }

        Ok(self.free_subtree(id))
    }

    /// Every node's value, root included, in no particular order.
    pub fn values(&self) -> impl Iterator<Item = u8> + '_ {
        self.nodes.values().map(|n| n.value)
    }

    /// Number of nodes whose value satisfies `pred`.
    pub fn count_nodes_where(&self, pred: impl Fn(u8) -> bool) -> usize {
        self.nodes.values().filter(|n| pred(n.value)).count()
    }

    /// Number of nodes holding a user group.
    pub fn user_group_nodes(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| groups::is_user_group(n.value))
            .count()
    }

    /// Switches to a new group catalog. Every node holding a user group is set to the empty
    /// value for its position, since the old definitions no longer apply. Subtrees are
    /// kept.
    pub fn replace_groups(&mut self, groups: Arc<GroupCatalog>) -> GroupSwap {
        self.replace_groups_where(groups, groups::is_user_group)
    }

    /// Like [`replace_groups`](Self::replace_groups), but only empties nodes whose value
    /// satisfies `should_empty`, e.g. the groups an edit changed or deleted.
    pub fn replace_groups_where(
        &mut self,
        groups: Arc<GroupCatalog>,
        should_empty: impl Fn(u8) -> bool,
    ) -> GroupSwap {
        let mut report = GroupSwap::default();
        // Pre-order, so a removed subtree's nodes are skipped below.
        let mut order = Vec::new();
        let mut stack = vec![self.root];
        while let Some(n) = stack.pop() {
            order.push(n);
            stack.extend(self.nodes[n].children.iter().rev());
        }
        for id in order {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            if !groups::is_user_group(node.value) || !should_empty(node.value) {
                continue;
            }
            let empty = empty_value_for(node.pos);
            self.nodes[id].value = empty;
            report.emptied += 1;
        }
        self.groups = groups;
        report
    }

    /// Renumbers group values with `map` (value -> new value) and switches to `groups`.
    /// Used when a loaded file's groups join the app's config (see `group_import.rs`).
    /// `map` must not send two values that are siblings somewhere to the same value, and
    /// `groups` must define every group value the tree holds afterwards.
    pub fn remap_groups(&mut self, map: &[u8; 256], groups: Arc<GroupCatalog>) {
        for node in self.nodes.values_mut() {
            node.value = map[usize::from(node.value)];
        }
        self.groups = groups;
        debug_assert!(
            self.values()
                .all(|v| !groups::is_group(v) || self.groups.get(v).is_some()),
            "every group value is defined"
        );
    }

    /// Hides (`true`) or shows `id`'s branches. The nodes stay in the tree; the graph shows
    /// `id` as a hidden-branches node instead. Hiding a node without children does
    /// nothing. Returns whether anything changed.
    pub fn set_collapsed(&mut self, id: NodeId, collapsed: bool) -> bool {
        match self.nodes.get_mut(id) {
            Some(node) if node.is_collapsed() != collapsed => {
                if collapsed && node.children.is_empty() {
                    return false;
                }
                node.collapsed = collapsed;
                true
            }
            _ => false,
        }
    }

    /// Whether `id` sits inside hidden branches (some node above it is hidden).
    pub fn concealed(&self, id: NodeId) -> bool {
        self.ancestors(id)
            .skip(1)
            .any(|a| self.nodes.get(a).is_some_and(Node::is_collapsed))
    }

    /// Shows the hidden branches `id` sits inside, so it is visible. Returns whether
    /// anything changed.
    pub fn reveal(&mut self, id: NodeId) -> bool {
        let hidden: Vec<NodeId> = self
            .ancestors(id)
            .skip(1)
            .filter(|&a| self.nodes[a].is_collapsed())
            .collect();
        for &a in &hidden {
            self.nodes[a].collapsed = false;
        }
        !hidden.is_empty()
    }

    /// Shows every hidden branch. Returns how many nodes had theirs hidden.
    pub fn expand_all(&mut self) -> usize {
        let mut n = 0;
        for node in self.nodes.values_mut() {
            if node.is_collapsed() {
                n += 1;
            }
            node.collapsed = false;
        }
        n
    }

    /// The topmost unreachable nodes that have branches: the nodes "Hide unreachable"
    /// hides the branches of. Each stays visible, as a hidden-branches node, and everything
    /// unreachable sits below one of them.
    pub fn unreachable_tops(&self) -> Vec<NodeId> {
        self.unreachable_tops_in(&self.unreachable())
    }

    /// [`unreachable_tops`](Self::unreachable_tops) from already computed unreachable
    /// marks (`Marks::unreachable`), so the marks aren't computed again.
    pub fn unreachable_tops_in(&self, unreachable: &SecondaryMap<NodeId, Tone>) -> Vec<NodeId> {
        unreachable
            .keys()
            .filter(|&id| {
                let node = &self.nodes[id];
                !node.children.is_empty()
                    && !node.parent.is_some_and(|p| unreachable.contains_key(p))
            })
            .collect()
    }

    /// Whether any node has its branches hidden.
    pub fn any_collapsed(&self) -> bool {
        self.nodes.values().any(Node::is_collapsed)
    }

    /// A copy without the hidden branches: the nodes whose branches are hidden keep no
    /// children. Ids stay the same for the nodes that remain.
    pub fn without_hidden(&self) -> DraftTree {
        let mut copy = self.clone();
        let hidden: Vec<NodeId> = copy
            .nodes
            .iter()
            .filter(|(_, n)| n.is_collapsed())
            .map(|(id, _)| id)
            .collect();
        for id in hidden {
            // An earlier one may already have taken this one away.
            if let Some(node) = copy.nodes.get_mut(id) {
                let children = std::mem::take(&mut node.children);
                for child in children {
                    copy.free_subtree(child);
                }
            }
        }
        copy
    }

    /// Number of nodes below `id` (not counting `id`).
    pub fn subtree_size(&self, id: NodeId) -> usize {
        let mut n = 0;
        let mut stack: Vec<NodeId> = self
            .nodes
            .get(id)
            .map_or_else(Vec::new, |node| node.children.clone());
        while let Some(c) = stack.pop() {
            n += 1;
            stack.extend(self.nodes[c].children.iter().copied());
        }
        n
    }

    /// Marks (`true`) or unmarks the line ending at `id` (the root can't be marked). Returns
    /// whether anything changed.
    pub fn set_marked(&mut self, id: NodeId, marked: bool) -> bool {
        if id == self.root {
            return false;
        }
        match self.nodes.get_mut(id) {
            Some(node) if node.marked != marked => {
                node.marked = marked;
                true
            }
            _ => false,
        }
    }

    /// Unmarks `id` and every marked node below it. Returns how many were marked.
    pub fn unmark_subtree(&mut self, id: NodeId) -> usize {
        let mut n = 0;
        let mut stack = vec![id];
        while let Some(c) = stack.pop() {
            let Some(node) = self.nodes.get_mut(c) else {
                continue;
            };
            if node.marked {
                node.marked = false;
                n += 1;
            }
            stack.extend(node.children.iter().copied());
        }
        n
    }

    /// Nodes on a marked line: marked nodes and everything above them (root included).
    pub fn starred(&self) -> SecondaryMap<NodeId, ()> {
        let mut out = SecondaryMap::new();
        for (id, node) in &self.nodes {
            if node.marked {
                for a in self.ancestors(id) {
                    if out.insert(a, ()).is_some() {
                        break;
                    }
                }
            }
        }
        out
    }

    /// Every node with its id, in no particular order.
    pub fn nodes_iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter()
    }

    /// Whether any line is marked.
    pub fn any_marked(&self) -> bool {
        self.nodes.values().any(|n| n.marked)
    }

    /// Moves `id` one place earlier (`-1`) or later (`1`) among its siblings. Returns whether
    /// it moved (not at the ends, nor for the root).
    pub fn move_among_siblings(&mut self, id: NodeId, delta: isize) -> bool {
        let Some(parent) = self.nodes.get(id).and_then(|n| n.parent) else {
            return false;
        };
        let i = self.index_in_parent(id);
        let children = &mut self.nodes[parent].children;
        let j = i as isize + delta;
        if j < 0 || j as usize >= children.len() {
            return false;
        }
        children.swap(i, j as usize);
        true
    }

    /// The teams' names (blue, red), empty when not set.
    pub fn team_names(&self) -> &[String; 2] {
        &self.team_names
    }

    /// Sets a team's name (`0` blue, `1` red), trimmed to 24 characters.
    pub fn set_team_name(&mut self, team: usize, name: &str) {
        self.team_names[team] = name.trim().chars().take(MAX_NAME_CHARS).collect();
    }

    pub fn patch(&self) -> &str {
        &self.patch
    }

    /// Sets the patch text, trimmed to 16 characters.
    pub fn set_patch(&mut self, patch: &str) {
        self.patch = patch.trim().chars().take(MAX_PATCH_CHARS).collect();
    }

    /// Removes everything except the root.
    pub fn clear(&mut self) {
        // `take` leaves an empty Vec behind, which unlinks all children in one go.
        let children = std::mem::take(&mut self.nodes[self.root].children);
        for child in children {
            self.free_subtree(child);
        }
    }

    /// Frees `id` and all its descendants from the arena. The caller must have unlinked `id`
    /// from its parent already.
    ///
    /// Iterative depth-first walk with an explicit stack instead of recursion. `SlotMap::remove`
    /// hands the node back by value, so its child list moves onto the stack without cloning.
    fn free_subtree(&mut self, id: NodeId) -> usize {
        let mut removed = 0;
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.remove(n) {
                stack.extend(node.children);
                removed += 1;
            }
        }
        removed
    }
}

/// Raw node edits for undo and redo (see `history.rs`), which turn the tree back into a
/// state it held before while keeping the ids of the nodes that stay. Nothing is checked:
/// the caller keeps the tree valid and every child list in step with the parent links.
impl DraftTree {
    /// A new node below `parent`, not yet in `parent`'s child list.
    pub(crate) fn raw_new(
        &mut self,
        parent: NodeId,
        value: u8,
        roles: u8,
        collapsed: bool,
        marked: bool,
    ) -> NodeId {
        let pos = self.nodes[parent]
            .pos
            .next()
            .expect("restored nodes fit the draft");
        self.nodes.insert(Node {
            pos,
            value,
            parent: Some(parent),
            children: Vec::new(),
            roles,
            collapsed,
            marked,
        })
    }

    /// Sets everything a node holds except its place in the tree.
    pub(crate) fn raw_set(
        &mut self,
        id: NodeId,
        value: u8,
        roles: u8,
        collapsed: bool,
        marked: bool,
    ) {
        let node = &mut self.nodes[id];
        node.value = value;
        node.roles = roles;
        node.collapsed = collapsed;
        node.marked = marked;
    }

    /// Sets the hidden flag as stored, even on a node without children.
    pub(crate) fn raw_set_collapsed(&mut self, id: NodeId, collapsed: bool) {
        self.nodes[id].collapsed = collapsed;
    }

    /// Links `id` to `parent` without touching any child list.
    pub(crate) fn raw_set_parent(&mut self, id: NodeId, parent: NodeId) {
        self.nodes[id].parent = Some(parent);
    }

    pub(crate) fn raw_children_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        &mut self.nodes[id].children
    }

    /// Frees `id` alone; its children are left to the caller.
    pub(crate) fn raw_free(&mut self, id: NodeId) {
        self.nodes.remove(id);
    }
}

#[cfg(test)]
#[path = "tree_conflict_tests.rs"]
mod conflict_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::{DRAFT_ORDER, Team};

    #[test]
    fn marks_and_reordering() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        let a1 = t.add_child(a, 17).unwrap();
        let b = t.add_child(root, 18).unwrap();
        let c = t.add_child(root, 19).unwrap();
        // Marks: the root can't be marked; a mark stars its whole line.
        assert!(!t.set_marked(root, true));
        assert!(t.set_marked(a1, true));
        assert!(t.any_marked());
        let starred = t.starred();
        assert!(starred.contains_key(root) && starred.contains_key(a) && starred.contains_key(a1));
        assert!(!starred.contains_key(b));
        // Unmarking a node and everything below it.
        t.set_marked(a, true);
        assert_eq!(t.unmark_subtree(a), 2);
        assert!(!t.any_marked());
        // Reordering stops at the ends.
        assert!(!t.move_among_siblings(a, -1));
        assert!(t.move_among_siblings(a, 1));
        assert_eq!(t.get(root).unwrap().children(), &[b, a, c]);
        assert!(t.move_among_siblings(a, 1));
        assert!(!t.move_among_siblings(a, 1));
        assert_eq!(t.get(root).unwrap().children(), &[b, c, a]);
        assert!(!t.move_among_siblings(root, 1));
        // Names and patch are trimmed and cut to length.
        t.set_team_name(0, "  A very long team name that goes on  ");
        assert_eq!(t.team_names()[0].chars().count(), MAX_NAME_CHARS);
        t.set_patch(" 14.20 ");
        assert_eq!(t.patch(), "14.20");
    }

    #[test]
    fn hiding_branches() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        let a1 = t.add_child(a, 17).unwrap();
        let a11 = t.add_child(a1, 18).unwrap();
        let b = t.add_child(root, 19).unwrap();
        // Nothing to hide below a leaf.
        assert!(!t.set_collapsed(b, true));
        assert!(!t.get(b).unwrap().is_collapsed());
        assert!(t.set_collapsed(a, true));
        assert!(!t.set_collapsed(a, true), "already hidden");
        assert_eq!(t.subtree_size(a), 2);
        assert!(t.concealed(a1) && t.concealed(a11));
        assert!(!t.concealed(a) && !t.concealed(b));
        // Nested: the inner one stays hidden when the outer one is shown.
        assert!(t.set_collapsed(a1, true));
        assert!(t.set_collapsed(a, false));
        assert!(t.get(a1).unwrap().is_collapsed() && t.concealed(a11));
        // Revealing a node shows the hidden nodes above it, not its own branches.
        t.set_collapsed(a, true);
        assert!(t.reveal(a11));
        assert!(!t.concealed(a11));
        assert!(!t.get(a).unwrap().is_collapsed() && !t.get(a1).unwrap().is_collapsed());
        assert!(!t.reveal(a11));
        t.set_collapsed(a, true);
        assert!(t.reveal(a1) && !t.get(a).unwrap().is_collapsed());
        // Show all.
        t.set_collapsed(a, true);
        t.set_collapsed(root, true);
        assert!(t.any_collapsed());
        assert_eq!(t.expand_all(), 2);
        assert!(!t.any_collapsed());
        // A copy can leave the hidden branches out.
        t.set_collapsed(a, true);
        let pruned = t.without_hidden();
        assert_eq!((t.len(), pruned.len()), (5, 3));
        assert!(pruned.get(a).unwrap().children().is_empty());
        t.expand_all();
        // A hidden node that loses its children no longer counts as hidden.
        t.set_collapsed(a1, true);
        t.remove_subtree(a11).unwrap();
        assert!(!t.get(a1).unwrap().is_collapsed());
        assert!(!t.any_collapsed());
    }
    use crate::fearless::{Fearless, Mode, Tone};

    const ALISTAR: u8 = 16;
    const ANNIE: u8 = 17;
    const ASHE: u8 = 18;

    #[test]
    fn option_node_id_has_no_overhead() {
        assert_eq!(
            std::mem::size_of::<Option<NodeId>>(),
            std::mem::size_of::<NodeId>()
        );
    }

    #[test]
    fn root_is_empty_draft() {
        let tree = DraftTree::new();
        let root = tree.get(tree.root()).unwrap();
        assert_eq!(root.pos(), DraftPos::ROOT);
        assert_eq!(root.value(), EMPTY);
        assert_eq!(root.parent(), None);
        assert!(root.children().is_empty());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn children_follow_draft_order() {
        let mut tree = DraftTree::new();
        let mut cursor = tree.root();
        for (i, step) in DRAFT_ORDER.iter().enumerate() {
            cursor = tree.add_child(cursor, ALISTAR + i as u8).unwrap();
            assert_eq!(tree.get(cursor).unwrap().pos(), step.pos());
        }
        assert_eq!(tree.add_child(cursor, 100), Err(TreeError::DraftComplete));
        assert_eq!(tree.path(cursor).len(), 21);
    }

    #[test]
    fn siblings_must_differ_and_keep_insertion_order() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let b = tree.add_child(root, ANNIE).unwrap();
        let a = tree.add_child(root, ALISTAR).unwrap();
        assert!(tree.has_child_with_value(root, ANNIE));
        assert!(!tree.has_child_with_value(root, ASHE));
        assert_eq!(
            tree.add_child(root, ANNIE),
            Err(TreeError::DuplicateSibling)
        );
        assert_eq!(tree.get(root).unwrap().children(), &[b, a]);
    }

    #[test]
    fn value_cannot_repeat_on_a_line_but_wildcards_can() {
        let mut tree = DraftTree::new();
        let b1 = tree.add_child(tree.root(), ALISTAR).unwrap();
        assert_eq!(tree.add_child(b1, ALISTAR), Err(TreeError::ValueUsedOnPath));
        // Two skipped bans in a row are fine.
        let r1 = tree.add_child(b1, NO_BAN).unwrap();
        let b2 = tree.add_child(r1, NO_BAN).unwrap();
        // Alistar is still allowed on a different line.
        let other = tree.add_child(tree.root(), ANNIE).unwrap();
        tree.add_child(other, ALISTAR).unwrap();
        assert!(tree.value_on_path(b2, ALISTAR));
        assert!(!tree.value_on_path(b2, ANNIE));
    }

    #[test]
    fn no_ban_only_at_bans() {
        let mut tree = DraftTree::new();
        let mut cursor = tree.root();
        for i in 0..6 {
            cursor = tree.add_child(cursor, ALISTAR + i).unwrap();
        }
        // Next is blue pick 1.
        assert_eq!(
            tree.get(cursor).unwrap().pos().next().unwrap().team(),
            Team::Blue
        );
        assert_eq!(
            tree.add_child(cursor, NO_BAN),
            Err(TreeError::InvalidForPosition)
        );
        tree.add_child(cursor, EMPTY).unwrap();
    }

    #[test]
    fn remove_subtree_frees_all_descendants() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.add_child(root, ALISTAR).unwrap();
        let b = tree.add_child(root, ANNIE).unwrap();
        let c = tree.add_child(root, ASHE).unwrap();
        let a1 = tree.add_child(a, ANNIE).unwrap();
        let a2 = tree.add_child(a1, ASHE).unwrap();

        assert_eq!(tree.remove_subtree(a), Ok(3));
        assert_eq!(tree.len(), 3);
        for gone in [a, a1, a2] {
            assert!(tree.get(gone).is_none());
        }
        // Remaining siblings keep their order.
        assert_eq!(tree.get(root).unwrap().children(), &[b, c]);

        // A stale id stays stale even after its slot is reused.
        let reused = tree.add_child(root, ALISTAR).unwrap();
        assert!(tree.get(a).is_none());
        assert_eq!(tree.remove_subtree(a), Err(TreeError::NodeNotFound));
        assert!(tree.get(reused).is_some());

        assert_eq!(tree.remove_subtree(root), Err(TreeError::RootIsFixed));
    }

    #[test]
    fn set_value_checks_siblings_and_the_line_above_only() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.add_child(root, ALISTAR).unwrap();
        let b = tree.add_child(root, ANNIE).unwrap();
        let a1 = tree.add_child(a, ASHE).unwrap();

        assert_eq!(tree.set_value(a, ANNIE), Err(TreeError::DuplicateSibling));
        assert_eq!(tree.sibling_with_value(a, ANNIE), Some(b));
        assert_eq!(tree.set_value(a1, ALISTAR), Err(TreeError::ValueUsedOnPath));
        assert_eq!(tree.set_value(root, ASHE), Err(TreeError::RootIsFixed));
        // The line below is not checked: a1 now repeats Ashe and becomes a conflict.
        tree.set_value(a, ASHE).unwrap();
        assert_eq!(
            tree.conflicts().get(a1),
            Some(&Conflict::Root(Cause::Repeats(a)))
        );
        // Changing it back makes a1 normal again.
        tree.set_value(a, ALISTAR).unwrap();
        assert!(tree.conflicts().is_empty());

        // Clearing a pick keeps the node and its subtree.
        tree.set_value(a, EMPTY).unwrap();
        assert_eq!(tree.get(a).unwrap().value(), EMPTY);
        assert_eq!(tree.get(a).unwrap().children(), &[a1]);
    }

    #[test]
    fn get_or_add_child_reuses_existing_branches() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.get_or_add_child(root, ALISTAR).unwrap();
        assert_eq!(tree.get_or_add_child(root, ALISTAR), Ok(a));
        let b = tree.get_or_add_child(root, ANNIE).unwrap();
        assert_ne!(a, b);
        assert_eq!(tree.get(root).unwrap().children(), &[a, b]);
        assert_eq!(tree.len(), 3);
        // Validation still applies when a new child is needed.
        assert_eq!(
            tree.get_or_add_child(a, ALISTAR),
            Err(TreeError::ValueUsedOnPath)
        );
    }

    #[test]
    fn empty_siblings_may_repeat() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let empty = tree.add_child(root, EMPTY).unwrap();
        let second = tree.add_child(root, NO_BAN).unwrap();
        let third = tree.add_child(root, NO_BAN).unwrap();
        // Choosing "nothing" follows the first empty child.
        assert_eq!(tree.child_with_value(root, NO_BAN), Some(empty));
        assert_eq!(tree.get_or_add_child(root, NO_BAN), Ok(empty));
        assert_eq!(tree.empty_number(empty), Some(1));
        assert_eq!(tree.empty_number(third), Some(3));
        tree.remove_subtree(third).unwrap();
        tree.remove_subtree(empty).unwrap();
        // A single empty child needs no number.
        assert_eq!(tree.empty_number(second), None);
        // Empty values never clash with a sibling's champion lookup.
        assert_eq!(tree.sibling_with_value(second, NO_BAN), None);
        let empty = second;
        // Empties in a row on one line are still fine.
        let next = tree.add_child(empty, NO_BAN).unwrap();
        tree.add_child(next, EMPTY).unwrap();
    }

    #[test]
    fn set_empty_uses_position_and_checks_siblings() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.add_child(root, ALISTAR).unwrap();
        let b = tree.add_child(root, ANNIE).unwrap();
        let a1 = tree.add_child(a, ASHE).unwrap();

        tree.set_empty(a).unwrap();
        // Blue ban 1 is a ban, so "empty" means NO_BAN; the subtree stays.
        assert_eq!(tree.get(a).unwrap().value(), NO_BAN);
        assert_eq!(tree.get(a).unwrap().children(), &[a1]);
        // Emptying it again is a no-op, not a clash with itself.
        tree.set_empty(a).unwrap();
        // A second empty sibling is fine.
        tree.set_empty(b).unwrap();
        assert_eq!(tree.empty_number(b), Some(2));
        assert_eq!(tree.set_empty(root), Err(TreeError::RootIsFixed));

        // Blue pick 1 (depth 7) gets EMPTY.
        let mut c = b;
        for v in [30, 31, 32, 33, 34, 35] {
            c = tree.add_child(c, v).unwrap();
        }
        tree.set_empty(c).unwrap();
        assert_eq!(tree.get(c).unwrap().value(), EMPTY);
    }

    #[test]
    fn cycle_same_depth_skips_short_branches_and_wraps() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        // Three lines from the root: a (depth 3), b (depth 1 only), c (depth 2).
        let a = tree.add_child(root, 16).unwrap();
        let a2 = tree.add_child(a, 17).unwrap();
        let a3 = tree.add_child(a2, 18).unwrap();
        let b = tree.add_child(root, 19).unwrap();
        let c = tree.add_child(root, 20).unwrap();
        let c2 = tree.add_child(c, 21).unwrap();
        // A second branch under a2 at depth 3.
        let a3b = tree.add_child(a2, 22).unwrap();

        assert_eq!(tree.cycle_same_depth(a, true), Some(b));
        assert_eq!(tree.cycle_same_depth(c, true), Some(a));
        assert_eq!(tree.cycle_same_depth(a, false), Some(c));
        // Depth 2: b has nothing there and is skipped.
        assert_eq!(tree.cycle_same_depth(a2, true), Some(c2));
        assert_eq!(tree.cycle_same_depth(c2, true), Some(a2));
        // Depth 3 has two nodes in the same family.
        assert_eq!(tree.cycle_same_depth(a3, true), Some(a3b));
        assert_eq!(tree.cycle_same_depth(a3b, false), Some(a3));
        // Alone at its depth, or the root.
        tree.remove_subtree(a3b).unwrap();
        assert_eq!(tree.cycle_same_depth(a3, true), None);
        assert_eq!(tree.cycle_same_depth(root, true), None);
    }

    /// Catalog with G1 = {Alistar, Annie} (190) and G2 = {Ashe, Fiddlesticks, Jax} (191).
    fn user_groups() -> Arc<GroupCatalog> {
        Arc::new(
            groups::parse_config(
                r#"{ "groups": [
                    { "value": 190, "name": "G1", "champions": [16, 17] },
                    { "value": 191, "name": "G2", "champions": [18, 19, 20] }
                ] }"#,
                None,
            )
            .unwrap(),
        )
    }

    const G1: u8 = 190;
    const G2: u8 = 191;

    #[test]
    fn groups_repeat_until_their_champions_run_out() {
        let mut tree = DraftTree::with_groups(user_groups());
        let mut c = tree.root();
        // G2 has three champions; one is taken explicitly, so G2 fits twice more.
        c = tree.add_child(c, 18).unwrap();
        c = tree.add_child(c, G2).unwrap();
        c = tree.add_child(c, G2).unwrap();
        assert_eq!(tree.add_child(c, G2), Err(TreeError::GroupExhausted));
        // G2's remaining champions are now spoken for, too.
        assert_eq!(tree.add_child(c, 19), Err(TreeError::GroupExhausted));
        assert_eq!(tree.add_child(c, 20), Err(TreeError::GroupExhausted));
        // Other champions and groups are unaffected.
        tree.add_child(c, 16).unwrap();
        tree.add_child(c, G1).unwrap();
    }

    #[test]
    fn explicit_champions_after_a_group_use_up_its_slots() {
        let mut tree = DraftTree::with_groups(user_groups());
        // G1 = {Alistar, Annie}: after G1 and Alistar, Annie can't come any more.
        let g = tree.add_child(tree.root(), G1).unwrap();
        let alistar = tree.add_child(g, 16).unwrap();
        assert_eq!(tree.add_child(alistar, 17), Err(TreeError::GroupExhausted));
        assert!(!tree.line_allows(alistar, 17));
        assert!(!tree.line_allows(alistar, G1));
        assert!(tree.line_allows(alistar, 18));
        // The same holds with the champion first.
        let annie = tree.add_child(tree.root(), 17).unwrap();
        let g_after = tree.add_child(annie, G1).unwrap();
        assert_eq!(tree.add_child(g_after, 16), Err(TreeError::GroupExhausted));
    }

    #[test]
    fn groups_and_champions_do_not_clash_as_siblings() {
        let mut tree = DraftTree::with_groups(user_groups());
        let root = tree.root();
        tree.add_child(root, 16).unwrap();
        tree.add_child(root, G1).unwrap();
        tree.add_child(root, 14).unwrap(); // preset {Alistar, Annie}
        assert_eq!(tree.add_child(root, G1), Err(TreeError::DuplicateSibling));
        assert_eq!(tree.add_child(root, 192), Err(TreeError::UnknownValue));
    }

    #[test]
    fn set_value_leaves_group_conflicts_below_the_node() {
        let mut tree = DraftTree::with_groups(user_groups());
        let a = tree.add_child(tree.root(), 30).unwrap();
        let b = tree.add_child(a, 16).unwrap();
        let c = tree.add_child(b, 17).unwrap();
        // G1 = {Alistar, Annie}: as `a`, Alistar is fine but then Annie has no slot left.
        tree.set_value(a, G1).unwrap();
        assert_eq!(
            tree.conflicts().get(c),
            Some(&Conflict::Root(Cause::GroupFull(G1)))
        );
        assert_eq!(tree.conflicts().len(), 1);
        tree.set_value(a, G2).unwrap();
        assert!(tree.conflicts().is_empty());
    }

    #[test]
    fn replacing_groups_empties_user_groups_and_drops_clashes() {
        let mut tree = DraftTree::with_groups(user_groups());
        let root = tree.root();
        let g1 = tree.add_child(root, G1).unwrap();
        let g2 = tree.add_child(root, G2).unwrap();
        let below = tree.add_child(g1, 30).unwrap();
        let deep = tree.add_child(below, G2).unwrap();
        let preset = tree.add_child(root, 3).unwrap();
        assert_eq!(tree.user_group_nodes(), 3);

        let report = tree.replace_groups(Arc::new(GroupCatalog::presets_only()));
        // Every user group becomes No ban; empty siblings may repeat, so nothing is removed.
        assert_eq!(report, GroupSwap { emptied: 3 });
        assert_eq!(tree.get(g1).unwrap().value(), NO_BAN);
        assert_eq!(tree.get(g2).unwrap().value(), NO_BAN);
        assert_eq!(tree.get(deep).unwrap().value(), NO_BAN);
        assert_eq!(tree.get(preset).unwrap().value(), 3);
        assert_eq!(tree.user_group_nodes(), 0);
        assert!(tree.groups().get(G1).is_none());
    }

    #[test]
    fn replacing_groups_can_spare_unchanged_ones() {
        let mut tree = DraftTree::with_groups(user_groups());
        let g1 = tree.add_child(tree.root(), G1).unwrap();
        let g2 = tree.add_child(g1, G2).unwrap();
        // Only G2 changed: G1 stays, G2 is emptied.
        let report = tree.replace_groups_where(user_groups(), |v| v == G2);
        assert_eq!(report, GroupSwap { emptied: 1 });
        assert_eq!(tree.get(g1).unwrap().value(), G1);
        assert_eq!(tree.get(g2).unwrap().value(), NO_BAN);
        assert_eq!(tree.count_nodes_where(|v| v == G1), 1);
    }

    #[test]
    fn clear_leaves_only_root() {
        let mut tree = DraftTree::new();
        let a = tree.add_child(tree.root(), ALISTAR).unwrap();
        tree.add_child(a, ANNIE).unwrap();
        tree.clear();
        assert_eq!(tree.len(), 1);
        assert!(tree.get(tree.root()).unwrap().children().is_empty());
        assert!(tree.get(a).is_none());
    }

    #[test]
    fn child_limit() {
        // Every distinct sibling value: one empty slot, 14 presets, 173 champions and 66 user
        // groups. That is 254, so the 255-child limit can't actually be reached.
        let config = (190..=255)
            .map(|v| format!(r#"{{ "value": {v}, "name": "U", "champions": [16] }}"#))
            .collect::<Vec<_>>()
            .join(",");
        let catalog =
            groups::parse_config(&format!(r#"{{ "groups": [{config}] }}"#), None).unwrap();
        let mut tree = DraftTree::with_groups(Arc::new(catalog));
        let root = tree.root();
        assert_eq!(tree.add_child(root, 189), Err(TreeError::UnknownValue));
        for v in (0..=u8::MAX).filter(|&v| v != NO_BAN && v != 189) {
            tree.add_child(root, v).unwrap();
        }
        assert_eq!(tree.get(root).unwrap().children().len(), 254);
        // Empty values may repeat, so the limit can be reached.
        tree.add_child(root, NO_BAN).unwrap();
        assert_eq!(
            tree.add_child(root, NO_BAN),
            Err(TreeError::TooManyChildren)
        );
        let full = tree.get(root).unwrap().children()[3];
        assert_eq!(tree.duplicate_branch(full), Err(TreeError::TooManyChildren));
        assert_eq!(
            tree.duplicate_line(full, true),
            Err(TreeError::TooManyChildren)
        );
    }

    fn hard_fearless(champions: &[u8]) -> Fearless {
        let mut f = Fearless {
            game: 2,
            ..Fearless::default()
        };
        f.rows[0][..champions.len()].copy_from_slice(champions);
        f
    }

    #[test]
    fn fearless_champions_cannot_be_chosen() {
        let mut tree = DraftTree::new();
        tree.set_fearless(hard_fearless(&[16]));
        let root = tree.root();
        assert_eq!(tree.add_child(root, 16), Err(TreeError::FearlessLocked));
        assert!(!tree.line_allows(root, 16));
        assert!(tree.add_child(root, 17).is_ok());
    }

    #[test]
    fn fearless_uses_up_group_slots() {
        // Preset 14 is {Alistar 16, Annie 17}.
        let mut tree = DraftTree::new();
        tree.set_fearless(hard_fearless(&[16]));
        let root = tree.root();
        let g = tree.add_child(root, 14).unwrap();
        // One member is fearless and one is now stood for: the group is used up.
        assert_eq!(tree.add_child(g, 14), Err(TreeError::GroupExhausted));
        tree.set_fearless(hard_fearless(&[16, 17]));
        assert_eq!(tree.add_child(root, 15).map(|_| ()), Ok(()));
        assert!(!tree.line_allows(root, 14), "no members left");
    }

    #[test]
    fn soft_fearless_only_stops_that_side_picking() {
        let mut tree = DraftTree::new();
        let mut f = Fearless {
            game: 2,
            mode: Mode::Soft,
            ..Fearless::default()
        };
        f.rows[0][0] = 16; // the blue-side team played Alistar
        tree.set_fearless(f);
        // Bans first: Alistar can still be banned.
        let mut at = tree.root();
        for _ in 0..6 {
            assert!(tree.line_allows(at, 16));
            at = tree.add_child(at, NO_BAN).unwrap();
        }
        // Blue pick 1: locked for blue.
        assert_eq!(tree.add_child(at, 16), Err(TreeError::FearlessLocked));
        let b1 = tree.add_child(at, 20).unwrap();
        // Red pick 1: red may take it.
        assert!(tree.add_child(b1, 16).is_ok());
    }

    #[test]
    fn unreachable_branches_can_be_removed_and_restored() {
        let mut tree = DraftTree::new();
        let root = tree.root();
        let a = tree.add_child(root, 16).unwrap();
        let a1 = tree.add_child(a, 18).unwrap();
        tree.add_child(a1, 19).unwrap();
        let b = tree.add_child(root, 17).unwrap();
        tree.add_child(b, 16).unwrap();
        tree.set_fearless(hard_fearless(&[16]));

        let unreachable = tree.unreachable();
        // a, a1, a1's child, and b's child.
        assert_eq!(unreachable.len(), 4);
        assert!(!unreachable.contains_key(b));
        assert!(unreachable.values().all(|&t| t == Tone::Both));

        let pruned = tree.remove_unreachable();
        assert_eq!(pruned.nodes(), 4);
        assert_eq!(tree.len(), 2);
        assert!(tree.unreachable().is_empty());

        assert_eq!(tree.restore_pruned(&pruned), 4);
        assert_eq!(tree.len(), 6);
        // `a` is back in front of `b`.
        let first = tree.get(root).unwrap().children()[0];
        assert_eq!(tree.get(first).unwrap().value(), 16);
        assert_eq!(tree.unreachable().len(), 4);
        // Restoring again merges with what is there.
        assert_eq!(tree.restore_pruned(&pruned), 0);
    }

    /// Six no-bans, then blue pick 1 (`a`), red picks 1-2, blue picks 2-3 (`b`, `c`).
    fn picks_tree() -> (DraftTree, [NodeId; 3]) {
        let mut t = DraftTree::new();
        let mut at = t.root();
        for _ in 0..6 {
            at = t.add_child(at, NO_BAN).unwrap();
        }
        let a = t.add_child(at, 16).unwrap();
        let r1 = t.add_child(a, 17).unwrap();
        let r2 = t.add_child(r1, 18).unwrap();
        let b = t.add_child(r2, 19).unwrap();
        let c = t.add_child(b, 20).unwrap();
        (t, [a, b, c])
    }

    #[test]
    fn role_tags_must_fit_per_team() {
        const TOP: u8 = 1;
        const JG: u8 = 2;
        const MID: u8 = 4;
        let (mut t, [a, b, c]) = picks_tree();
        t.set_roles(a, TOP).unwrap();
        // Another top-only blue pick below doesn't fit.
        assert_eq!(t.set_roles(c, TOP), Err(TreeError::RoleConflict));
        // A flex pick does, as long as some permutation works: with Top and Top/Jungle
        // above, the flex must go jungle, so a jungle-only pick no longer fits.
        t.set_roles(b, TOP | JG).unwrap();
        assert_eq!(t.set_roles(c, JG), Err(TreeError::RoleConflict));
        assert_eq!(t.set_roles(c, MID), Ok(()));
        // Tagging an earlier node is checked against the lines below it.
        assert_eq!(t.set_roles(a, MID), Err(TreeError::RoleConflict));
        assert_eq!(t.set_roles(a, JG), Ok(()));
        // Red's picks are separate.
        let red = t.get(b).unwrap().parent().unwrap();
        t.set_roles(red, JG).unwrap();
        // Bans can't be tagged.
        let ban = t.get(t.root()).unwrap().children()[0];
        assert_eq!(t.set_roles(ban, TOP), Err(TreeError::RolesNotAllowed));
        // Clearing always works.
        assert_eq!(t.set_roles(c, 0), Ok(()));
    }

    #[test]
    fn group_role_locks_limit_tags() {
        let groups = GroupCatalog::with_user_groups(
            vec![
                Group::new_user(190, "Tops".into(), String::new(), vec![16, 17])
                    .with_roles(0b00011),
            ],
            None,
        );
        let mut t = DraftTree::with_groups(std::sync::Arc::new(groups));
        let mut at = t.root();
        for _ in 0..6 {
            at = t.add_child(at, NO_BAN).unwrap();
        }
        let g = t.add_child_tagged(at, 190, t.role_limit(190)).unwrap();
        assert_eq!(t.get(g).unwrap().roles(), 0b00011);
        assert_eq!(t.set_roles(g, 0b00100), Err(TreeError::RolesNotAllowed));
        assert_eq!(t.set_roles(g, 0b00001), Ok(()));
    }
}
