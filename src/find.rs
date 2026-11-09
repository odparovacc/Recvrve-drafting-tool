//! Find in tree (Ctrl+F): which nodes match a search, and stepping through them.
//!
//! A node matches when its champion's name, or its group's name or label, contains the
//! search text (ignoring case). With "Include groups", a group node also matches when one
//! of its champions does. Matches are in the graph's order (depth first, children in
//! order). Branches left out of the graph (unreachable ones while they are hidden) never
//! match; hidden branches (Ctrl+H) only with "Search in hidden branches".

use rayon::prelude::*;
use slotmap::SecondaryMap;

use crate::champions;
use crate::fearless::Tone;
use crate::tree::{DraftTree, NodeId};
use crate::value_view::value_name;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Group nodes match when one of their champions does.
    pub include_groups: bool,
    /// Look inside hidden branches (Ctrl+H) too.
    pub search_hidden: bool,
}

/// Every node in the graph's order, root first, leaving out `skip` and everything below
/// it, and (unless `into_hidden`) everything below a node whose branches are hidden.
fn walk(tree: &DraftTree, skip: &SecondaryMap<NodeId, Tone>, into_hidden: bool) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        let Some(node) = tree.get(id) else { continue };
        out.push(id);
        if node.is_collapsed() && !into_hidden {
            continue;
        }
        stack.extend(
            node.children()
                .iter()
                .rev()
                .copied()
                .filter(|&c| !skip.contains_key(c)),
        );
    }
    out
}

/// The nodes matching `query`, in the graph's order. Nothing for an empty query.
pub fn matches(
    tree: &DraftTree,
    query: &str,
    options: FindOptions,
    skip: &SecondaryMap<NodeId, Tone>,
) -> Vec<NodeId> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let hit = |text: &str| text.to_lowercase().contains(&query);
    let groups = tree.groups();
    // Whether each value matches, checked once per value rather than once per node.
    let matches_value = |value: u8| {
        if let Some(g) = groups.get(value) {
            hit(&g.name)
                || hit(&g.label)
                || (options.include_groups
                    && g.members
                        .iter()
                        .any(|&m| champions::by_value(m).is_some_and(|c| hit(c.name))))
        } else {
            hit(&value_name(groups, value))
        }
    };
    let mut present = [false; 256];
    for value in tree.values() {
        present[usize::from(value)] = true;
    }
    let mut hits = [false; 256];
    for value in 0..=u8::MAX {
        hits[usize::from(value)] = present[usize::from(value)] && matches_value(value);
    }
    let order = walk(tree, skip, options.search_hidden);
    let root = tree.root();
    let keep =
        |&id: &NodeId| id != root && hits[usize::from(tree.get(id).expect("walked node").value())];
    // Large trees are filtered in parallel; the order stays the graph's.
    if order.len() >= PARALLEL_FIND_MIN {
        order.par_iter().copied().filter(keep).collect()
    } else {
        order.into_iter().filter(keep).collect()
    }
}

/// From this many nodes on, matching runs on several threads.
pub const PARALLEL_FIND_MIN: usize = 20_000;

/// The match after (`forward`) or before `from` in the graph's order, wrapping around.
/// `from` need not be a match itself.
pub fn step(tree: &DraftTree, matches: &[NodeId], from: NodeId, forward: bool) -> Option<NodeId> {
    if matches.is_empty() {
        return None;
    }
    // Positions in the full order, hidden branches included, so any node can be a start.
    let order = walk(tree, &SecondaryMap::new(), true);
    // Each node's position, looked up once per match rather than searched for.
    let mut position: SecondaryMap<NodeId, usize> = SecondaryMap::with_capacity(order.len());
    for (i, &id) in order.iter().enumerate() {
        position.insert(id, i);
    }
    let at = |id: NodeId| position.get(id).copied();
    let here = at(from).unwrap_or(0);
    let positions: Vec<(usize, NodeId)> = matches
        .iter()
        .filter_map(|&m| at(m).map(|p| (p, m)))
        .collect();
    let found = if forward {
        positions
            .iter()
            .find(|(p, _)| *p > here)
            .or(positions.first())
    } else {
        positions
            .iter()
            .rev()
            .find(|(p, _)| *p < here)
            .or(positions.last())
    };
    found.map(|&(_, m)| m)
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
            .map(champions::value_of)
            .unwrap()
    }

    #[test]
    fn finds_and_steps_in_graph_order() {
        let (ahri, annie, ashe) = (champ("Ahri"), champ("Annie"), champ("Ashe"));
        let group = Group::new_user(190, "Mages".into(), String::new(), vec![ahri, annie]);
        let mut t =
            DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(vec![group], None)));
        let root = t.root();
        let a = t.add_child(root, ahri).unwrap();
        let a1 = t.add_child(a, ashe).unwrap();
        let a11 = t.add_child(a1, annie).unwrap();
        let b = t.add_child(root, 190).unwrap();
        let c = t.add_child(root, annie).unwrap();
        let none = SecondaryMap::new();
        let opts = FindOptions::default();
        // Case doesn't matter; "an" is in Annie (twice) and Mages has no "an".
        assert_eq!(matches(&t, "AN", opts, &none), vec![a11, c]);
        assert!(matches(&t, "  ", opts, &none).is_empty());
        // Groups by name and label; with Include groups, by member too.
        assert_eq!(matches(&t, "mage", opts, &none), vec![b]);
        assert_eq!(matches(&t, "g1", opts, &none), vec![b]);
        let groups = FindOptions {
            include_groups: true,
            ..opts
        };
        assert_eq!(matches(&t, "ahri", groups, &none), vec![a, b]);
        // Hidden branches only on request.
        t.set_collapsed(a, true);
        assert_eq!(matches(&t, "annie", opts, &none), vec![c]);
        let hidden = FindOptions {
            search_hidden: true,
            ..opts
        };
        assert_eq!(matches(&t, "annie", hidden, &none), vec![a11, c]);
        // Stepping wraps, and starts from any node.
        let found = matches(&t, "annie", hidden, &none);
        assert_eq!(step(&t, &found, root, true), Some(a11));
        assert_eq!(step(&t, &found, a11, true), Some(c));
        assert_eq!(step(&t, &found, c, true), Some(a11));
        assert_eq!(step(&t, &found, a11, false), Some(c));
        assert_eq!(step(&t, &found, b, false), Some(a11));
        assert_eq!(step(&t, &[], root, true), None);
        // Skipped branches (hidden unreachable ones) never match.
        let mut skip = SecondaryMap::new();
        skip.insert(c, Tone::Both);
        assert_eq!(matches(&t, "annie", hidden, &skip), vec![a11]);
    }

    /// The matching rule node by node, as it was before the per-value table: the
    /// reference the fast paths must agree with.
    fn reference(tree: &DraftTree, query: &str, options: FindOptions) -> Vec<NodeId> {
        let query = query.trim().to_lowercase();
        let hit = |text: &str| text.to_lowercase().contains(&query);
        let groups = tree.groups();
        walk(tree, &SecondaryMap::new(), options.search_hidden)
            .into_iter()
            .filter(|&id| id != tree.root())
            .filter(|&id| {
                let value = tree.get(id).unwrap().value();
                match groups.get(value) {
                    Some(g) => {
                        hit(&g.name)
                            || hit(&g.label)
                            || (options.include_groups
                                && g.members
                                    .iter()
                                    .any(|&m| champions::by_value(m).is_some_and(|c| hit(c.name))))
                    }
                    None => hit(&value_name(groups, value)),
                }
            })
            .collect()
    }

    #[test]
    fn large_trees_match_in_parallel_like_small_ones() {
        let mages = Group::new_user(190, "Mages".into(), String::new(), vec![17, 18]);
        let mut t =
            DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(vec![mages], None)));
        // A wide tree well past the parallel cutoff, with champions, a group and empties.
        let mut level = vec![t.root()];
        let mut depth = 0u8;
        while t.len() <= PARALLEL_FIND_MIN + 5_000 {
            let mut next = Vec::new();
            for &n in &level {
                for v in [
                    16 + 3 * depth,
                    17 + 3 * depth,
                    if depth == 2 { 190 } else { 0 },
                ] {
                    if let Ok(c) = t.add_child(n, v) {
                        next.push(c);
                    }
                }
            }
            level = next;
            depth += 1;
        }
        assert!(t.len() > PARALLEL_FIND_MIN);
        for query in ["a", "an", "mag", "empty", "zzz"] {
            for include_groups in [false, true] {
                let options = FindOptions {
                    include_groups,
                    search_hidden: false,
                };
                assert_eq!(
                    matches(&t, query, options, &SecondaryMap::new()),
                    reference(&t, query, options),
                    "{query} {include_groups}"
                );
            }
        }
    }
}
