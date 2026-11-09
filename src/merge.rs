//! Merging any number of draft trees (save files, and optionally the current tree) into one.
//!
//! The merged tree holds every line of every input exactly once:
//!
//! - Starting at the root, a node of one input and a node of the merged tree are the same
//!   node when their parents are the same node and they hold the same champion or group.
//!   If their role tags differ, the merged node keeps no tags (any role): that never
//!   makes a team's roles stop fitting, since lines below were valid with either.
//! - Empty values (Empty / No ban) only match with the same role tags, and in order: the
//!   first such empty child of an input matches the first such child of the merged node,
//!   the second the second, and so on. Merging a tree with itself gives the same tree.
//! - Branches without a match are added after the existing ones, in input order.
//!
//! So every line of the result is a line of some input, apart from tags dropped where the
//! inputs disagreed. No new conflicts, unreachable branches or role problems can appear.
//!
//! User groups are combined first with [`group_import::plan_merge`]: groups with the same
//! champions become one (the first input's name wins; differing role locks are dropped),
//! others keep their value or move to a free one. Inputs must share their fearless
//! settings, since the result can hold only one. Team names and the patch carry over
//! when all inputs have the same ones, and reset otherwise.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::fearless::Fearless;
use crate::group_import::{self, Outcome};
use crate::groups::{Group, GroupCatalog};
use crate::tree::{DraftTree, MAX_NODES, NodeId, TreeError};

/// One tree to merge, with a name for messages (usually the file name).
pub struct Input {
    pub name: String,
    pub tree: DraftTree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// Fewer than two inputs.
    NotEnoughInputs,
    /// The inputs have different fearless settings: the first input's name, and the names
    /// of the inputs that differ from it.
    FearlessDiffers {
        first: String,
        differing: Vec<String>,
    },
    /// The inputs use more than 66 different user groups together.
    TooManyGroups,
    /// A node would get more than 255 children.
    TooManyChildren,
    /// The merged tree would have more than [`MAX_NODES`] nodes, though each input has
    /// fewer.
    TooManyNodes,
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::NotEnoughInputs => f.write_str("Pick at least two trees to merge."),
            MergeError::FearlessDiffers { first, differing } => write!(
                f,
                "These trees have different fearless draft settings, so they can't be merged: {} {} from {first}. Make the fearless settings match first{}.",
                differing.join(", "),
                if differing.len() == 1 { "differs" } else { "differ" },
                crate::keys::hint_suffix(crate::keys::Action::Fearless)
            ),
            MergeError::TooManyGroups => f.write_str(
                "Together these trees use more than 66 different representational groups, so they can't be merged.",
            ),
            MergeError::TooManyChildren => f.write_str(
                "The merged tree would have a node with more than 255 branches, so it can't be merged.",
            ),
            MergeError::TooManyNodes => write!(
                f,
                "The merged tree would have more than {} nodes, so it can't be merged.",
                crate::save::thousands(MAX_NODES)
            ),
        }
    }
}

/// The merged tree and some numbers for the notice.
pub struct Merged {
    pub tree: DraftTree,
    /// Nodes in all inputs together (roots excluded).
    pub input_nodes: usize,
    /// Nodes that were the same as one already merged (`input_nodes` - merged nodes).
    pub shared_nodes: usize,
    /// Merged nodes whose role tags were dropped because the inputs disagreed.
    pub tags_dropped: usize,
}

/// User groups `tree` actually uses.
fn used_groups(tree: &DraftTree) -> Vec<Group> {
    let mut used = [false; 256];
    for v in tree.values() {
        used[usize::from(v)] = true;
    }
    tree.groups()
        .user_groups()
        .filter(|g| used[usize::from(g.value)])
        .cloned()
        .collect()
}

/// Merges `inputs` (see the module docs).
pub fn merge(inputs: &[Input]) -> Result<Merged, MergeError> {
    merge_capped(inputs, MAX_NODES)
}

/// [`merge`] with `max_nodes` in place of [`MAX_NODES`], so tests can check the limit
/// without huge trees.
fn merge_capped(inputs: &[Input], max_nodes: usize) -> Result<Merged, MergeError> {
    let (first, rest) = inputs.split_first().ok_or(MergeError::NotEnoughInputs)?;
    if rest.is_empty() {
        return Err(MergeError::NotEnoughInputs);
    }
    let fearless: Fearless = *first.tree.fearless();
    let differing: Vec<String> = rest
        .iter()
        .filter(|i| *i.tree.fearless() != fearless)
        .map(|i| i.name.clone())
        .collect();
    if !differing.is_empty() {
        return Err(MergeError::FearlessDiffers {
            first: first.name.clone(),
            differing,
        });
    }

    // Groups: each input's groups join the ones merged so far.
    let mut catalog = GroupCatalog::with_user_groups(Vec::new(), None);
    let mut maps: Vec<[u8; 256]> = Vec::with_capacity(inputs.len());
    for input in inputs {
        let plan = group_import::plan_merge(&catalog, &used_groups(&input.tree));
        if plan.outcome == Outcome::FileOnly {
            return Err(MergeError::TooManyGroups);
        }
        catalog = plan.catalog(None);
        maps.push(plan.map);
    }

    let mut out = DraftTree::with_groups(Arc::new(catalog));
    out.set_fearless(fearless);
    // Team names and patch carry over only when every input agrees; otherwise they reset
    // to "Blue Team" / "Red Team" and no patch.
    let agree =
        |f: &dyn Fn(&DraftTree) -> String| rest.iter().all(|i| f(&i.tree) == f(&first.tree));
    if agree(&|t: &DraftTree| t.team_names().join("\n")) {
        for team in 0..2 {
            out.set_team_name(team, &first.tree.team_names()[team]);
        }
    }
    if agree(&|t: &DraftTree| t.patch().to_string()) {
        out.set_patch(first.tree.patch());
    }
    let mut input_nodes = 0;
    // Merged nodes that came from more than one input with different tags.
    let mut dropped: HashSet<NodeId> = HashSet::new();
    for (input, map) in inputs.iter().zip(&maps) {
        input_nodes += input.tree.len() - 1;
        merge_one(&mut out, &input.tree, map, &mut dropped, max_nodes)?;
    }
    let shared_nodes = input_nodes - (out.len() - 1);
    Ok(Merged {
        tree: out,
        input_nodes,
        shared_nodes,
        tags_dropped: dropped.len(),
    })
}

fn is_empty_value(value: u8) -> bool {
    value == crate::tree::EMPTY || value == crate::tree::NO_BAN
}

/// Merges `tree` (values renumbered by `map`) into `out`.
fn merge_one(
    out: &mut DraftTree,
    tree: &DraftTree,
    map: &[u8; 256],
    dropped: &mut HashSet<NodeId>,
    max_nodes: usize,
) -> Result<(), MergeError> {
    let mut stack: Vec<(NodeId, NodeId)> = vec![(tree.root(), out.root())];
    while let Some((src, dst)) = stack.pop() {
        let src_children = tree.get(src).expect("live node").children().to_vec();
        // How many empty children with each tag set this input has had so far here.
        let mut empties_seen: HashMap<u8, usize> = HashMap::new();
        let mut pairs = Vec::with_capacity(src_children.len());
        for c in src_children {
            let node = tree.get(c).expect("live node");
            let value = map[usize::from(node.value())];
            let roles = node.roles();
            let existing = if is_empty_value(value) {
                let k = empties_seen.entry(roles).or_insert(0);
                let found = out
                    .get(dst)
                    .expect("live node")
                    .children()
                    .iter()
                    .copied()
                    .filter(|&d| {
                        let n = out.get(d).expect("live node");
                        is_empty_value(n.value()) && n.roles() == roles
                    })
                    .nth(*k);
                *k += 1;
                found
            } else {
                out.get(dst)
                    .expect("live node")
                    .children()
                    .iter()
                    .copied()
                    .find(|&d| out.get(d).expect("live node").value() == value)
            };
            let target = match existing {
                Some(d) => {
                    if out.get(d).expect("live node").roles() != roles {
                        out.set_roles_raw(d, 0);
                        dropped.insert(d);
                    }
                    d
                }
                // Stops as soon as the limit is passed, before the tree grows further.
                None if out.len() > max_nodes => return Err(MergeError::TooManyNodes),
                None => out
                    .insert_raw(dst, value, roles, None)
                    .map_err(|e| match e {
                        TreeError::TooManyChildren => MergeError::TooManyChildren,
                        e => panic!("a merged node is always valid: {e}"),
                    })?,
            };
            pairs.push((c, target));
        }
        // Depth first, children in order (the order doesn't change the result).
        stack.extend(pairs.into_iter().rev());
    }
    Ok(())
}

/// Every group value `tree` uses is defined, and no two inputs' different groups got the
/// same value: for tests and debug checks.
#[cfg(test)]
fn groups_resolved(tree: &DraftTree) -> bool {
    tree.values()
        .all(|v| !crate::groups::is_group(v) || tree.groups().get(v).is_some())
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
