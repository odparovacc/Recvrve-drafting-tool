//! Tests for repeated empty siblings, value edits, conflicting branches and duplication.
//! Kept apart from `tree.rs` because there are many of them, including a randomized run
//! that checks every tree invariant after each edit.

use super::*;
use crate::groups::Group;
use crate::save;

const ALISTAR: u8 = 16;
const ANNIE: u8 = 17;
const ASHE: u8 = 18;
const FIDDLE: u8 = 19;
const JAX: u8 = 20;

/// Adds a line of `values` below `at`, returning the nodes in order.
fn line(t: &mut DraftTree, at: NodeId, values: &[u8]) -> Vec<NodeId> {
    let mut ids = Vec::new();
    let mut at = at;
    for &v in values {
        at = t.add_child(at, v).unwrap();
        ids.push(at);
    }
    ids
}

fn root_cause(t: &DraftTree, id: NodeId) -> Option<Cause> {
    match t.conflicts().get(id) {
        Some(Conflict::Root(c)) => Some(*c),
        _ => None,
    }
}

fn catalog(groups: &[(u8, &[u8], u8)]) -> Arc<GroupCatalog> {
    Arc::new(GroupCatalog::with_user_groups(
        groups
            .iter()
            .map(|&(v, members, roles)| {
                Group::new_user(v, format!("G{v}"), String::new(), members.to_vec())
                    .with_roles(roles)
            })
            .collect(),
        None,
    ))
}

/// The saved form of the tree, which is also what undo snapshots store.
fn bytes(t: &DraftTree) -> Vec<u8> {
    save::encode(t, None)
}

/// Checks everything that must hold for any tree, whatever was done to it.
fn check_invariants(t: &DraftTree) {
    let root = t.root();
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        count += 1;
        let node = t.get(n).unwrap();
        assert!(node.children().len() <= MAX_CHILDREN);
        for &c in node.children() {
            let child = t.get(c).unwrap();
            assert_eq!(child.parent(), Some(n), "parent link");
            assert_eq!(
                Some(child.pos()),
                node.pos().next(),
                "position follows depth"
            );
            assert!(child.value() != NO_BAN || child.pos().action() == Action::Ban);
            assert!(child.roles() == 0 || child.pos().action() == Action::Pick);
            let same = node
                .children()
                .iter()
                .filter(|&&s| clashes(t.get(s).unwrap().value(), child.value()))
                .count();
            assert!(same <= 1, "champion or group repeated among siblings");
            stack.push(c);
        }
    }
    assert_eq!(count, t.len(), "every node reachable from the root");

    // Marks: disjoint, inherited, and agreeing with the per-line check.
    let marks = t.marks();
    for (id, conflict) in &marks.conflicts {
        assert!(!marks.unreachable.contains_key(id), "one mark per node");
        let parent = t.get(id).unwrap().parent().unwrap();
        let parent_conflicts = marks.conflicts.contains_key(parent);
        match conflict {
            Conflict::Root(_) => assert!(!parent_conflicts, "a root has no conflict above"),
            Conflict::Below => assert!(parent_conflicts, "below means below a conflict"),
        }
        for &c in t.get(id).unwrap().children() {
            assert_eq!(marks.conflicts.get(c), Some(&Conflict::Below));
        }
    }
    for (id, _) in &marks.unreachable {
        for &c in t.get(id).unwrap().children() {
            assert!(
                marks.unreachable.contains_key(c),
                "unreachable is inherited"
            );
        }
    }
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let on_line = t.line_conflict(n);
        // `line_conflict` ignores fearless, so below an unreachable node it may still see
        // a conflict that `marks` reports as unreachable (the first problem wins there).
        if !marks.unreachable.contains_key(n) {
            assert_eq!(
                on_line.is_some(),
                marks.conflicts.contains_key(n),
                "line_conflict agrees with marks"
            );
        }
        if let Some(top) = on_line {
            assert!(t.ancestors(n).any(|a| a == top));
        }
        stack.extend(t.get(n).unwrap().children());
    }

    // Role tags fit on every line, conflicting branches included.
    let mut stack: Vec<(NodeId, [Vec<u8>; 2])> = vec![(root, [Vec::new(), Vec::new()])];
    while let Some((n, lines)) = stack.pop() {
        for &c in t.get(n).unwrap().children() {
            let child = t.get(c).unwrap();
            let mut lines = lines.clone();
            if child.roles() != 0 {
                let team = team_index(child.pos());
                lines[team].push(child.roles());
                assert!(crate::roles::assignable(&lines[team]), "role tags fit");
            }
            stack.push((c, lines));
        }
    }

    // Saving and loading keeps the tree and its marks exactly.
    let saved = bytes(t);
    let back = save::decode(&saved).unwrap().tree;
    assert_eq!(bytes(&back), saved, "save round trip");
    assert_eq!(back.conflicts().len(), marks.conflicts.len());
    assert_eq!(back.unreachable().len(), marks.unreachable.len());
}

// ---------------------------------------------------------------------------------------
// Repeated empty siblings

#[test]
fn several_empty_children_and_choosing_follows_the_first() {
    let mut t = DraftTree::new();
    let r = t.root();
    let a = t.add_child(r, NO_BAN).unwrap();
    let b = t.add_child(r, NO_BAN).unwrap();
    let c = t.add_child(r, ALISTAR).unwrap();
    assert_eq!(t.get_or_add_child(r, NO_BAN), Ok(a));
    assert_eq!(
        t.get_or_add_child(r, EMPTY),
        Ok(a),
        "EMPTY and NO_BAN match"
    );
    assert_eq!(t.get_or_add_child(r, ALISTAR), Ok(c));
    assert_eq!(
        (t.empty_number(a), t.empty_number(b), t.empty_number(c)),
        (Some(1), Some(2), None)
    );
    // Champions still can't repeat among siblings.
    assert_eq!(t.add_child(r, ALISTAR), Err(TreeError::DuplicateSibling));
    // Each empty branch is its own line.
    let a1 = t.add_child(a, ANNIE).unwrap();
    let b1 = t.add_child(b, ANNIE).unwrap();
    assert_ne!(a1, b1);
    check_invariants(&t);
}

#[test]
fn emptying_a_node_next_to_an_empty_sibling_keeps_both_subtrees() {
    let mut t = DraftTree::new();
    let r = t.root();
    let e = t.add_child(r, NO_BAN).unwrap();
    let a = t.add_child(r, ALISTAR).unwrap();
    let below = line(&mut t, a, &[ANNIE, ASHE]);
    t.set_empty(a).unwrap();
    assert_eq!(t.get(a).unwrap().value(), NO_BAN);
    assert_eq!(t.get(a).unwrap().children(), &[below[0]]);
    assert_eq!((t.empty_number(e), t.empty_number(a)), (Some(1), Some(2)));
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Value edits and repeated champions

#[test]
fn editing_an_empty_node_marks_the_later_mention_and_everything_below() {
    let mut t = DraftTree::new();
    let r = t.root();
    let e = t.add_child(r, NO_BAN).unwrap();
    // e -> Annie -> Ashe -> {Alistar -> Fiddlesticks, Jax}
    let l = line(&mut t, e, &[ANNIE, ASHE]);
    let al = t.add_child(l[1], ALISTAR).unwrap();
    let bard = t.add_child(al, FIDDLE).unwrap();
    let cait = t.add_child(l[1], JAX).unwrap();
    let before = bytes(&t);

    t.set_value(e, ALISTAR).unwrap();
    let c = t.conflicts();
    assert_eq!(c.len(), 2, "Alistar and the node below it");
    assert_eq!(c.get(al), Some(&Conflict::Root(Cause::Repeats(e))));
    assert_eq!(c.get(bard), Some(&Conflict::Below));
    // The rest of the tree is untouched and unmarked.
    for n in [e, l[0], l[1], cait] {
        assert!(!c.contains_key(n));
    }
    check_invariants(&t);

    // Back to empty: the tree is exactly what it was.
    t.set_value(e, NO_BAN).unwrap();
    assert!(t.conflicts().is_empty());
    assert_eq!(bytes(&t), before);
}

#[test]
fn one_edit_can_mark_several_branches_and_only_the_first_repeat_is_the_root() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[ANNIE, ALISTAR, ASHE]);
    let b = line(&mut t, e, &[ASHE, ALISTAR]);
    let x = line(&mut t, e, &[FIDDLE]);
    // Ashe on two branches at different depths.
    t.set_value(e, ASHE).unwrap();
    assert_eq!(root_cause(&t, a[2]), Some(Cause::Repeats(e)));
    assert_eq!(root_cause(&t, b[0]), Some(Cause::Repeats(e)));
    assert_eq!(t.conflicts().get(b[1]), Some(&Conflict::Below));
    assert!(!t.conflicts().contains_key(x[0]));
    assert_eq!(t.conflicts().len(), 3);
    check_invariants(&t);
}

#[test]
fn switching_between_values_moves_the_conflict() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[ANNIE, ASHE]);
    t.set_value(e, ANNIE).unwrap();
    assert_eq!(root_cause(&t, a[0]), Some(Cause::Repeats(e)));
    assert_eq!(t.conflicts().get(a[1]), Some(&Conflict::Below));
    t.set_value(e, ASHE).unwrap();
    assert_eq!(root_cause(&t, a[1]), Some(Cause::Repeats(e)));
    assert!(!t.conflicts().contains_key(a[0]));
    t.set_value(e, FIDDLE).unwrap();
    assert!(t.conflicts().is_empty());
    check_invariants(&t);
}

#[test]
fn a_conflict_is_fixed_by_editing_the_later_node_but_not_by_editing_below_it() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[ANNIE, ASHE, FIDDLE]);
    t.set_value(e, ASHE).unwrap();
    assert_eq!(root_cause(&t, a[1]), Some(Cause::Repeats(e)));
    // Editing a node below the conflict changes nothing about it.
    t.set_value(a[2], JAX).unwrap();
    assert_eq!(root_cause(&t, a[1]), Some(Cause::Repeats(e)));
    // Editing the conflicting node itself fixes it.
    t.set_value(a[1], FIDDLE).unwrap();
    assert!(t.conflicts().is_empty());
    check_invariants(&t);
}

#[test]
fn nothing_new_can_start_below_a_conflict_but_existing_branches_can_be_followed() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[ANNIE, ASHE]);
    t.set_value(e, ANNIE).unwrap();
    assert_eq!(t.add_child(a[0], FIDDLE), Err(TreeError::BelowConflict));
    assert_eq!(t.add_child(a[1], FIDDLE), Err(TreeError::BelowConflict));
    assert_eq!(t.get_or_add_child(a[0], ASHE), Ok(a[1]));
    assert_eq!(t.line_conflict(a[1]), Some(a[0]));
    // Next to the conflict, on a line of its own, adding works.
    let ok = t.add_child(e, FIDDLE).unwrap();
    t.add_child(ok, JAX).unwrap();
    // Duplicating out of a conflict is allowed: the copy starts empty.
    let copy = t.duplicate_branch(a[0]).unwrap();
    assert!(!t.conflicts().contains_key(copy));
    t.add_child(copy, JAX).unwrap();
    check_invariants(&t);
}

#[test]
fn refused_edits_leave_the_tree_unchanged() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ASHE, FIDDLE], 0)]));
    t.set_fearless(Fearless {
        game: 2,
        rows: {
            let mut rows = [[0; crate::fearless::SLOTS]; crate::fearless::ROWS];
            rows[0][0] = JAX;
            rows
        },
        ..Fearless::default()
    });
    let r = t.root();
    let a = t.add_child(r, ALISTAR).unwrap();
    let sib = t.add_child(r, ANNIE).unwrap();
    let l = line(&mut t, a, &[190, 190, NO_BAN]);
    let before = bytes(&t);
    let attempts = [
        (sib, ALISTAR, TreeError::DuplicateSibling),
        (l[0], ALISTAR, TreeError::ValueUsedOnPath),
        (l[2], ASHE, TreeError::GroupExhausted),
        (l[2], JAX, TreeError::FearlessLocked),
        (r, ASHE, TreeError::RootIsFixed),
        (a, 189, TreeError::UnknownValue),
    ];
    for (id, value, err) in attempts {
        assert_eq!(t.set_value(id, value), Err(err), "{value}");
        assert_eq!(bytes(&t), before);
    }
    // NO_BAN at a pick.
    let mut picks = DraftTree::new();
    let pr = picks.root();
    let p = line(&mut picks, pr, &[1, 1, 1, 1, 1, 1, EMPTY]);
    assert_eq!(
        picks.set_value(p[6], NO_BAN),
        Err(TreeError::InvalidForPosition)
    );
}

// ---------------------------------------------------------------------------------------
// Groups

/// G = {Alistar, Annie}. Line: empty, G, G. Making the empty node Alistar leaves only
/// Annie for two uses of G, so the second G conflicts.
#[test]
fn group_used_too_often_after_the_edit() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ALISTAR, ANNIE], 0)]));
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let l = line(&mut t, e, &[190, 190, ASHE]);
    t.set_value(e, ALISTAR).unwrap();
    assert!(!t.conflicts().contains_key(l[0]), "first G still has Annie");
    assert_eq!(root_cause(&t, l[1]), Some(Cause::GroupFull(190)));
    assert_eq!(t.conflicts().get(l[2]), Some(&Conflict::Below));
    // A champion outside the group doesn't matter.
    t.set_value(e, FIDDLE).unwrap();
    assert!(t.conflicts().is_empty());
    check_invariants(&t);
}

/// Line: empty, G, Annie. Making the empty node G as well: G twice already claims both
/// of its champions, so Annie conflicts.
#[test]
fn editing_into_a_group_claims_champions_further_down() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ALISTAR, ANNIE], 0)]));
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let l = line(&mut t, e, &[190, ANNIE]);
    t.set_value(e, 190).unwrap();
    assert_eq!(root_cause(&t, l[1]), Some(Cause::GroupFull(190)));
    check_invariants(&t);
}

/// Overlapping groups A = {Alistar, Annie}, B = {Annie, Ashe}. Line: empty, A, B, Alistar.
/// Annie at the empty node uses a slot of both groups; then A's last champion is spoken
/// for, so Alistar conflicts. B is unaffected.
#[test]
fn overlapping_groups_are_counted_separately() {
    let mut t = DraftTree::with_groups(catalog(&[
        (190, &[ALISTAR, ANNIE], 0),
        (191, &[ANNIE, ASHE], 0),
    ]));
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let l = line(&mut t, e, &[190, 191, ALISTAR]);
    t.set_value(e, ANNIE).unwrap();
    assert!(!t.conflicts().contains_key(l[0]));
    assert!(!t.conflicts().contains_key(l[1]));
    assert_eq!(root_cause(&t, l[2]), Some(Cause::GroupFull(190)));
    check_invariants(&t);
}

/// The edited node is below the group: the group comes first on the line, so it is the
/// champion that can't be chosen, and the edit is refused rather than creating a conflict
/// above the edited node.
#[test]
fn a_group_above_the_edited_node_refuses_the_edit() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ALISTAR, ANNIE], 0)]));
    let tr = t.root();
    let l = line(&mut t, tr, &[190, 190, NO_BAN]);
    assert_eq!(t.set_value(l[2], ALISTAR), Err(TreeError::GroupExhausted));
    assert!(t.conflicts().is_empty());
}

/// Changing what a group contains (here: sparing it from being emptied) can break lines
/// that were fine; they show as conflicts instead of failing.
#[test]
fn a_changed_group_definition_shows_up_as_conflicts() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ALISTAR, ANNIE, ASHE], 0)]));
    let tr = t.root();
    let l = line(&mut t, tr, &[190, 190, ANNIE]);
    assert!(t.conflicts().is_empty());
    t.replace_groups_where(catalog(&[(190, &[ALISTAR, ANNIE], 0)]), |_| false);
    assert_eq!(root_cause(&t, l[2]), Some(Cause::GroupFull(190)));
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Fearless together with conflicts

fn fearless_with(champions: &[u8]) -> Fearless {
    let mut f = Fearless {
        game: 2,
        ..Fearless::default()
    };
    f.rows[0][..champions.len()].copy_from_slice(champions);
    f
}

#[test]
fn the_first_problem_on_a_line_decides_the_mark() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    // e -> Annie -> Fiddlesticks -> Ashe;  e -> Fiddlesticks -> Annie
    let a = line(&mut t, e, &[ANNIE, FIDDLE, ASHE]);
    let b = line(&mut t, e, &[FIDDLE, ANNIE]);
    t.set_value(e, ANNIE).unwrap();
    t.set_fearless(fearless_with(&[FIDDLE]));
    let m = t.marks();
    // Line a: Annie repeats first, so it's a conflict all the way down.
    assert!(matches!(m.conflicts.get(a[0]), Some(Conflict::Root(_))));
    assert_eq!(m.conflicts.get(a[1]), Some(&Conflict::Below));
    assert!(!m.unreachable.contains_key(a[1]));
    // Line b: Fiddlesticks is fearless-locked first, so it's unreachable, Annie below included.
    assert!(m.unreachable.contains_key(b[0]));
    assert!(m.unreachable.contains_key(b[1]));
    assert!(!m.conflicts.contains_key(b[1]));
    check_invariants(&t);
    // Without fearless, the repeat on line b shows as a conflict.
    t.set_fearless(Fearless::default());
    assert!(matches!(
        t.conflicts().get(b[1]),
        Some(Conflict::Root(Cause::Repeats(_)))
    ));
}

/// Soft fearless doesn't lock bans, so Annie can be banned first; the later pick of Annie
/// is then both a repeat and fearless-locked. The repeat is checked first: conflict.
#[test]
fn a_node_both_repeated_and_fearless_locked_counts_as_a_conflict() {
    let mut t = DraftTree::new();
    let p = picks(&mut t, &[ANNIE, ASHE]);
    let first_ban = t.path(p[0])[1];
    t.set_fearless(Fearless {
        mode: crate::fearless::Mode::Soft,
        ..fearless_with(&[ANNIE])
    });
    t.set_value(first_ban, ANNIE).unwrap();
    let m = t.marks();
    assert!(
        !m.unreachable.contains_key(first_ban),
        "soft fearless allows bans"
    );
    assert_eq!(
        m.conflicts.get(p[0]),
        Some(&Conflict::Root(Cause::Repeats(first_ban)))
    );
    assert_eq!(m.conflicts.get(p[1]), Some(&Conflict::Below));
    assert!(m.unreachable.is_empty());
    check_invariants(&t);
}

#[test]
fn removing_unreachable_and_removing_conflicts_are_independent() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[ANNIE, ASHE]);
    let b = line(&mut t, e, &[FIDDLE, JAX]);
    t.set_value(e, ANNIE).unwrap();
    t.set_fearless(fearless_with(&[FIDDLE]));
    let full = bytes(&t);

    let gone = t.remove_unreachable();
    assert!(t.contains(a[0]), "conflicts stay");
    assert!(!t.contains(b[0]));
    t.restore_pruned(&gone);
    assert_eq!(bytes(&t), full);
    // Restored nodes are new nodes with new ids.
    let bard = t.child_with_value(e, FIDDLE).unwrap();

    let gone = t.remove_conflicts();
    assert_eq!(gone.nodes(), 2);
    assert!(!t.contains(a[0]));
    assert!(t.contains(bard), "unreachable stays");
    assert!(t.conflicts().is_empty());
    check_invariants(&t);
    t.restore_pruned(&gone);
    assert_eq!(bytes(&t), full);
    check_invariants(&t);
}

#[test]
fn edits_into_fearless_bans_are_refused() {
    let mut t = DraftTree::new();
    t.set_fearless(fearless_with(&[ANNIE]));
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    assert_eq!(t.set_value(e, ANNIE), Err(TreeError::FearlessLocked));
    t.set_value(e, ASHE).unwrap();
}

// ---------------------------------------------------------------------------------------
// Role tags

/// Six bans, then a line of picks starting at blue pick 1; returns the picks.
fn picks(t: &mut DraftTree, values: &[u8]) -> Vec<NodeId> {
    let bans = line(t, t.root(), &[NO_BAN; 6]);
    line(t, bans[5], values)
}

#[test]
fn editing_into_a_role_locked_group_adjusts_tags() {
    // Tops = {Ashe, Fiddlesticks}, locked to top/jungle.
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ASHE, FIDDLE], 0b00011)]));
    let p = picks(&mut t, &[EMPTY, ANNIE, JAX, ALISTAR]);
    // Overlapping tags are narrowed.
    t.set_roles(p[0], 0b00110).unwrap();
    t.set_value(p[0], 190).unwrap();
    assert_eq!(t.get(p[0]).unwrap().roles(), 0b00010);
    // Untagged becomes the lock.
    t.set_value(p[0], EMPTY).unwrap();
    t.set_roles(p[0], 0).unwrap();
    t.set_value(p[0], 190).unwrap();
    assert_eq!(t.get(p[0]).unwrap().roles(), 0b00011);
    // Back to a champion keeps the tags.
    t.set_value(p[0], EMPTY).unwrap();
    assert_eq!(t.get(p[0]).unwrap().roles(), 0b00011);
    check_invariants(&t);
}

#[test]
fn a_lock_that_would_break_the_team_clears_the_tags() {
    // Blue picks are p[0], p[3], p[4]. p[3] and p[4] take top and jungle, so the lock
    // (top/jungle) can't fit p[0] as well.
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ASHE, FIDDLE], 0b00011)]));
    let p = picks(&mut t, &[EMPTY, ANNIE, JAX, ALISTAR, 21]);
    t.set_roles(p[3], 0b00001).unwrap();
    t.set_roles(p[4], 0b00010).unwrap();
    t.set_roles(p[0], 0b00100).unwrap();
    t.set_value(p[0], 190).unwrap();
    assert_eq!(t.get(p[0]).unwrap().roles(), 0);
    check_invariants(&t);
}

#[test]
fn role_tags_are_still_checked_on_conflicting_lines() {
    let mut t = DraftTree::new();
    let p = picks(&mut t, &[EMPTY, ANNIE, JAX, ALISTAR, ASHE]);
    t.set_roles(p[3], 0b00001).unwrap();
    t.set_value(p[0], ALISTAR).unwrap();
    assert!(matches!(t.conflicts().get(p[3]), Some(Conflict::Root(_))));
    // p[4] is below the conflict, but a tag that can't fit is still refused.
    assert_eq!(t.set_roles(p[4], 0b00001), Err(TreeError::RoleConflict));
    t.set_roles(p[4], 0b00010).unwrap();
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Duplication

/// Values and tags of the subtree of `id` in pre-order, with depths.
fn shape(t: &DraftTree, id: NodeId) -> Vec<(usize, u8, u8)> {
    let mut out = Vec::new();
    let mut stack = vec![(id, 0)];
    while let Some((n, d)) = stack.pop() {
        let node = t.get(n).unwrap();
        out.push((d, node.value(), node.roles()));
        stack.extend(node.children().iter().rev().map(|&c| (c, d + 1)));
    }
    out
}

#[test]
fn duplicate_branch_copies_below_and_empties_the_top() {
    let mut t = DraftTree::new();
    let r = t.root();
    let first = t.add_child(r, ALISTAR).unwrap();
    let a = t.add_child(r, ANNIE).unwrap();
    let last = t.add_child(r, FIDDLE).unwrap();
    line(&mut t, a, &[ASHE, JAX]);
    t.add_child(a, 21).unwrap();
    let copy = t.duplicate_branch(a).unwrap();
    // Right after the original, among the root's children.
    assert_eq!(t.get(r).unwrap().children(), &[first, a, copy, last]);
    let mut expected = shape(&t, a);
    expected[0].1 = NO_BAN;
    assert_eq!(shape(&t, copy), expected);
    assert_eq!(t.get(a).unwrap().value(), ANNIE, "original untouched");
    // Again: a second empty copy.
    let copy2 = t.duplicate_branch(a).unwrap();
    assert_eq!(t.get(r).unwrap().children(), &[first, a, copy2, copy, last]);
    assert_eq!(t.empty_number(copy2), Some(1));
    assert_eq!(t.empty_number(copy), Some(2));
    // Duplicating an empty node works too.
    t.duplicate_branch(copy).unwrap();
    assert_eq!(t.duplicate_branch(r), Err(TreeError::RootIsFixed));
    check_invariants(&t);
}

#[test]
fn duplicate_branch_at_a_pick_keeps_tags_and_uses_empty() {
    let mut t = DraftTree::new();
    let p = picks(&mut t, &[ANNIE, ASHE]);
    t.set_roles(p[0], 0b00100).unwrap();
    t.set_roles(p[1], 0b01000).unwrap();
    let copy = t.duplicate_branch(p[0]).unwrap();
    assert_eq!(
        shape(&t, copy),
        vec![(0, EMPTY, 0b00100), (1, ASHE, 0b01000)]
    );
    check_invariants(&t);
}

#[test]
fn duplicate_line_copies_from_the_first_action_down() {
    let mut t = DraftTree::new();
    let r = t.root();
    let l = line(&mut t, r, &[ALISTAR, ANNIE, ASHE]);
    let side = t.add_child(l[0], FIDDLE).unwrap();
    line(&mut t, l[2], &[JAX, 21]);
    let other = t.add_child(r, 22).unwrap();

    // Only the line, ending at the selected node.
    let copy = t.duplicate_line(l[1], false).unwrap();
    assert_eq!(t.get(copy).unwrap().value(), ANNIE);
    assert!(t.get(copy).unwrap().children().is_empty());
    let path: Vec<u8> = t
        .path(copy)
        .iter()
        .skip(1)
        .map(|&n| t.get(n).unwrap().value())
        .collect();
    assert_eq!(path, vec![NO_BAN, ANNIE]);
    let top = t.path(copy)[1];
    assert_eq!(t.get(r).unwrap().children(), &[l[0], top, other]);
    // Side branches off the line are not copied.
    assert_eq!(t.get(top).unwrap().children().len(), 1);
    assert!(t.contains(side));

    // The line and everything below the selected node.
    let copy = t.duplicate_line(l[2], true).unwrap();
    let mut expected = shape(&t, l[2]);
    expected[0].1 = ASHE;
    assert_eq!(shape(&t, copy), expected);
    assert_eq!(t.get(t.path(copy)[1]).unwrap().value(), NO_BAN);

    // At the first action itself, both are the same as duplicating the branch.
    let a = t.duplicate_line(l[0], true).unwrap();
    assert_eq!(t.get(a).unwrap().value(), NO_BAN);
    assert_eq!(t.get(a).unwrap().children().len(), 2);
    let b = t.duplicate_line(l[0], false).unwrap();
    assert!(t.get(b).unwrap().children().is_empty());
    assert_eq!(t.duplicate_line(r, true), Err(TreeError::RootIsFixed));
    check_invariants(&t);
}

#[test]
fn copies_never_add_conflicts_and_can_lose_the_one_their_top_caused() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let l = line(&mut t, e, &[ANNIE, ASHE]);
    t.set_value(e, ASHE).unwrap();
    assert_eq!(t.conflicts().len(), 1);
    // The copy of e is empty, so Ashe below it is fine.
    let copy = t.duplicate_branch(e).unwrap();
    assert_eq!(t.conflicts().len(), 1);
    assert!(t.conflicts().contains_key(l[1]));
    let copied_ashe = t
        .get(t.get(copy).unwrap().children()[0])
        .unwrap()
        .children()[0];
    assert!(!t.conflicts().contains_key(copied_ashe));
    check_invariants(&t);
}

#[test]
fn a_line_copy_keeps_conflicts_its_first_node_did_not_cause() {
    let mut t = DraftTree::new();
    // Fiddlesticks -> e -> Annie -> Ashe, with e = Ashe: the cause is e, not the first node.
    let l = {
        let tr = t.root();
        line(&mut t, tr, &[FIDDLE, NO_BAN, ANNIE, ASHE])
    };
    t.set_value(l[1], ASHE).unwrap();
    let copy = t.duplicate_line(l[3], true).unwrap();
    let copied_e = t.path(copy)[2];
    assert_eq!(
        t.conflicts().get(copy),
        Some(&Conflict::Root(Cause::Repeats(copied_e)))
    );
    // Copying only the line to e's child doesn't reach the conflict.
    let short = t.duplicate_line(l[2], false).unwrap();
    assert!(!t.conflicts().contains_key(short));
    assert_eq!(t.conflicts().len(), 2);
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Removing and restoring

#[test]
fn restore_puts_branches_back_between_repeated_empty_siblings() {
    let mut t = DraftTree::new();
    let r = t.root();
    let e = t.add_child(r, NO_BAN).unwrap();
    for v in [ANNIE, ASHE, FIDDLE] {
        // Three empty children of e, each with a different champion below.
        let n = t.add_child(e, NO_BAN).unwrap();
        line(&mut t, n, &[v, ALISTAR]);
    }
    t.add_child(e, ALISTAR).unwrap();
    t.set_value(e, ALISTAR).unwrap();
    let full = bytes(&t);
    let marked = t.conflicts().len();
    assert_eq!(marked, 4, "three repeats and the sibling");
    let gone = t.remove_conflicts();
    assert_eq!(gone.nodes(), 4);
    check_invariants(&t);
    t.restore_pruned(&gone);
    assert_eq!(bytes(&t), full, "same places, same order");
}

#[test]
fn restore_skips_branches_whose_parent_is_gone_and_merges_existing_values() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    let a = line(&mut t, e, &[FIDDLE, JAX, ASHE]);
    let b = line(&mut t, e, &[ANNIE, FIDDLE]);
    t.set_fearless(fearless_with(&[FIDDLE]));
    let gone = t.remove_unreachable();
    assert_eq!(gone.nodes(), 4);
    assert!(!t.contains(a[0]) && !t.contains(b[1]));
    // Meanwhile: Annie (the parent of one removed branch) is deleted, fearless is lifted
    // and Fiddlesticks is added again below e.
    t.remove_subtree(b[0]).unwrap();
    t.set_fearless(Fearless::default());
    let again = t.add_child(e, FIDDLE).unwrap();
    t.restore_pruned(&gone);
    // Fiddlesticks's branch merged into the new Fiddlesticks node; Annie's branch is gone for good.
    assert_eq!(t.get(e).unwrap().children(), &[again]);
    assert_eq!(shape(&t, again).len(), 3);
    check_invariants(&t);
}

#[test]
fn restore_skips_branches_when_a_value_above_them_changed() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    line(&mut t, e, &[ANNIE, ASHE]);
    t.set_value(e, ANNIE).unwrap();
    let gone = t.remove_conflicts();
    // The way to the branch went through e holding Annie; now e holds Fiddlesticks.
    t.set_value(e, FIDDLE).unwrap();
    assert_eq!(t.restore_pruned(&gone), 0);
    // Changing it back first lets it return.
    t.set_value(e, ANNIE).unwrap();
    assert_eq!(t.restore_pruned(&gone), 2);
    assert_eq!(t.conflicts().len(), 2);
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Saving

#[test]
fn conflicts_and_repeated_empties_survive_saving() {
    let mut t = DraftTree::with_groups(catalog(&[(190, &[ALISTAR, ANNIE], 0b00001)]));
    let p = picks(&mut t, &[EMPTY, 190, 190]);
    let bans_end = t.get(p[0]).unwrap().parent().unwrap();
    t.add_child(bans_end, EMPTY).unwrap();
    t.add_child(bans_end, EMPTY).unwrap();
    t.set_value(p[0], ALISTAR).unwrap();
    assert_eq!(t.conflicts().len(), 1);
    let loaded = save::decode(&bytes(&t)).unwrap().tree;
    assert_eq!(bytes(&loaded), bytes(&t));
    assert_eq!(loaded.conflicts().len(), 1);
    // Text form too.
    let text = save::to_text(&t, None);
    let loaded = save::from_text(&text).unwrap().tree;
    assert_eq!(bytes(&loaded), bytes(&t));
    check_invariants(&t);
}

// ---------------------------------------------------------------------------------------
// Randomized edits

/// xorshift64*: small, deterministic, no extra dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn all_nodes(t: &DraftTree) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![t.root()];
    while let Some(n) = stack.pop() {
        out.push(n);
        stack.extend(t.get(n).unwrap().children());
    }
    out
}

/// Thousands of random edits of every kind on small value pools (so repeats, group
/// exhaustion and fearless bans happen often), checking every invariant after each one,
/// and that refused edits change nothing.
#[test]
fn random_edits_never_break_the_tree() {
    let groups = catalog(&[
        (190, &[ALISTAR, ANNIE], 0b00011),
        (191, &[ANNIE, ASHE, FIDDLE], 0),
        (192, &[JAX, 21], 0b10000),
    ]);
    // Champions 16-23, the groups and both empty values.
    let pool: Vec<u8> = (16..24).chain([190, 191, 192, EMPTY, NO_BAN]).collect();
    for seed in 1..=12u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut t = DraftTree::with_groups(groups.clone());
        if seed % 3 == 0 {
            t.set_fearless(Fearless {
                mode: if seed % 2 == 0 {
                    crate::fearless::Mode::Soft
                } else {
                    crate::fearless::Mode::Hard
                },
                ..fearless_with(&[ASHE, 22])
            });
        }
        let mut pruned: Vec<Pruned> = Vec::new();
        for step in 0..400 {
            let nodes = all_nodes(&t);
            let n = nodes[rng.below(nodes.len())];
            let v = pool[rng.below(pool.len())];
            let before = bytes(&t);
            let result: Result<(), TreeError> = match rng.below(12) {
                0..=3 => t.add_child(n, v).map(|_| ()),
                4..=5 => t.set_value(n, v),
                6 => t.duplicate_branch(n).map(|_| ()),
                7 => t.duplicate_line(n, rng.below(2) == 0).map(|_| ()),
                8 => t.set_roles(n, 1 << rng.below(5)),
                9 if t.len() > 40 => t.remove_subtree(n).map(|_| ()),
                10 => {
                    pruned.push(if rng.below(2) == 0 {
                        t.remove_conflicts()
                    } else {
                        t.remove_unreachable()
                    });
                    Ok(())
                }
                11 => {
                    if let Some(p) = pruned.pop() {
                        t.restore_pruned(&p);
                    }
                    Ok(())
                }
                _ => t.set_empty(n),
            };
            if result.is_err() {
                assert_eq!(
                    bytes(&t),
                    before,
                    "seed {seed} step {step}: refused edit changed the tree"
                );
            }
            if t.len() > 300 {
                // Keep the tree small enough to stay fast.
                let first = t.get(t.root()).unwrap().children()[0];
                t.remove_subtree(first).unwrap();
            }
            check_invariants(&t);
        }
    }
}

#[test]
fn unreachable_tops_are_where_hiding_starts() {
    let mut t = DraftTree::new();
    let e = t.add_child(t.root(), NO_BAN).unwrap();
    // e -> Fiddlesticks -> Annie -> Ashe;  e -> Jax;  e -> Alistar -> Fiddlesticks
    let a = line(&mut t, e, &[FIDDLE, ANNIE, ASHE]);
    let j = line(&mut t, e, &[JAX]);
    let b = line(&mut t, e, &[ALISTAR, FIDDLE]);
    assert!(t.unreachable_tops().is_empty());
    t.set_fearless(fearless_with(&[FIDDLE, JAX]));
    // Jax is unreachable but has no branches to hide; the Fiddlesticks leaf neither.
    assert!(t.unreachable().contains_key(j[0]) && t.unreachable().contains_key(b[1]));
    assert_eq!(t.unreachable_tops(), [a[0]]);
    // Hiding them conceals everything unreachable that has a node above it to hide it.
    for id in t.unreachable_tops() {
        t.set_collapsed(id, true);
    }
    assert!(t.concealed(a[1]) && t.concealed(a[2]));
    assert!(!t.concealed(a[0]) && !t.concealed(b[0]));
}

// ---------------------------------------------------------------------------------------
// Parallel marks

/// A tree of about `size` nodes grown at random from a pool of champions, groups and empty
/// values, then given fearless bans and value edits so it holds conflicts and unreachable
/// branches of every kind.
fn big_tree(seed: u64, size: usize, mode: Option<crate::fearless::Mode>) -> DraftTree {
    let groups = catalog(&[
        (190, &[ALISTAR, ANNIE, 30, 31], 0b00011),
        (191, &[ANNIE, ASHE, FIDDLE], 0),
        (192, &[JAX, 21, 40], 0b10000),
    ]);
    let pool: Vec<u8> = (16..48).chain([190, 191, 192, EMPTY, NO_BAN]).collect();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut t = DraftTree::with_groups(groups);
    let mut nodes = vec![t.root()];
    while t.len() < size {
        let at = nodes[rng.below(nodes.len())];
        if let Ok(id) = t.add_child(at, pool[rng.below(pool.len())]) {
            nodes.push(id);
        }
    }
    // Edits higher up leave conflicting branches below.
    for _ in 0..size / 50 {
        let n = nodes[rng.below(nodes.len())];
        let _ = t.set_value(n, pool[rng.below(pool.len())]);
    }
    if let Some(mode) = mode {
        t.set_fearless(Fearless {
            mode,
            ..fearless_with(&[ASHE, 22, 33, 41])
        });
    }
    t
}

#[test]
fn parallel_marks_match_sequential_marks() {
    use crate::fearless::Mode;
    for (seed, mode) in [(1, Mode::Hard), (2, Mode::Soft)] {
        let t = big_tree(seed, PARALLEL_MARKS_MIN + 5_000, Some(mode));
        let marks = t.marks_sequential();
        assert!(!marks.conflicts.is_empty() && !marks.unreachable.is_empty());
        assert!(t.marks_parallel() == marks, "seed {seed}");
        // Large enough that `marks` takes the parallel path.
        assert!(t.len() >= PARALLEL_MARKS_MIN);
    }
    // A small tree gives the same marks either way too.
    let t = big_tree(3, 300, Some(Mode::Hard));
    assert!(t.marks_parallel() == t.marks_sequential());
}

#[test]
fn line_unreachable_matches_the_marks() {
    use crate::fearless::Mode;
    for (seed, mode) in [(4, Some(Mode::Hard)), (5, Some(Mode::Soft)), (6, None)] {
        let t = big_tree(seed, 3_000, mode);
        let marks = t.marks();
        assert_eq!(marks.unreachable.is_empty(), mode.is_none(), "seed {seed}");
        for (id, _) in t.nodes_iter() {
            let want = marks.unreachable.contains_key(id);
            assert_eq!(t.line_unreachable(id), want, "seed {seed}");
        }
        assert_eq!(
            t.unreachable_tops_in(&marks.unreachable),
            t.unreachable_tops()
        );
    }
}
