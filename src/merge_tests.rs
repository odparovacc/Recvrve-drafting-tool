//! Tests for `merge.rs`: hand-made cases for each rule, and a randomized run checking the
//! defining property on many inputs: the merged tree has exactly the union of the inputs'
//! lines, with tags kept where the inputs agree and dropped where they don't.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::fearless::Mode;
use crate::tree::{EMPTY, NO_BAN};

const ALISTAR: u8 = 16;
const ANNIE: u8 = 17;
const ASHE: u8 = 18;
const FIDDLE: u8 = 19;
const JAX: u8 = 20;

fn g(value: u8, name: &str, members: &[u8], roles: u8) -> Group {
    Group::new_user(value, name.into(), String::new(), members.to_vec()).with_roles(roles)
}

fn tree_with(groups: &[Group]) -> DraftTree {
    DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(
        groups.to_vec(),
        None,
    )))
}

fn input(name: &str, tree: DraftTree) -> Input {
    Input {
        name: name.into(),
        tree,
    }
}

fn line(t: &mut DraftTree, at: NodeId, values: &[u8]) -> Vec<NodeId> {
    let mut ids = Vec::new();
    let mut at = at;
    for &v in values {
        at = t.get_or_add_child(at, v).unwrap();
        ids.push(at);
    }
    ids
}

/// Six bans (No ban), returning the last one; picks start below it.
fn bans(t: &mut DraftTree) -> NodeId {
    let root = t.root();
    *line(t, root, &[NO_BAN; 6]).last().unwrap()
}

/// What a node stands for, independent of group numbering.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Value(u8),
    Group(Vec<u8>),
    /// An empty value (Empty or No ban, which mean the same) with its tags and its number
    /// among same-tagged empty siblings.
    Empty(u8, usize),
}

/// Every node's path of keys from the root, with its tags and whether it conflicts.
fn paths(t: &DraftTree) -> BTreeMap<Vec<Key>, (u8, bool)> {
    let conflicts = t.conflicts();
    let mut out = BTreeMap::new();
    let mut stack = vec![(t.root(), Vec::new())];
    while let Some((n, path)) = stack.pop() {
        let mut seen: BTreeMap<u8, usize> = BTreeMap::new();
        for &c in t.get(n).unwrap().children() {
            let node = t.get(c).unwrap();
            let v = node.value();
            let key = if v == EMPTY || v == NO_BAN {
                let k = seen.entry(node.roles()).or_insert(0);
                *k += 1;
                Key::Empty(node.roles(), *k - 1)
            } else if crate::groups::is_user_group(v) {
                Key::Group(t.groups().get(v).unwrap().members.clone())
            } else {
                Key::Value(v)
            };
            let mut p = path.clone();
            p.push(key);
            out.insert(p.clone(), (node.roles(), conflicts.contains_key(c)));
            stack.push((c, p));
        }
    }
    out
}

/// The tags every line of `t` puts on each team fit (see `roles.rs`).
fn roles_fit_everywhere(t: &DraftTree) -> bool {
    let mut stack = vec![(t.root(), [Vec::new(), Vec::new()])];
    while let Some((n, lines)) = stack.pop() {
        for &c in t.get(n).unwrap().children() {
            let node = t.get(c).unwrap();
            let mut lines: [Vec<u8>; 2] = lines.clone();
            if node.roles() != 0 {
                let team = match node.pos().team() {
                    crate::draft::Team::Blue => 0,
                    crate::draft::Team::Red => 1,
                };
                lines[team].push(node.roles());
                if !crate::roles::assignable(&lines[team]) {
                    return false;
                }
            }
            stack.push((c, lines));
        }
    }
    true
}

/// The defining property of a merge, plus validity of the result.
fn check_merge(inputs: &[Input], merged: &Merged) {
    let out = paths(&merged.tree);
    let mut expected: BTreeMap<Vec<Key>, (BTreeSet<u8>, bool)> = BTreeMap::new();
    for i in inputs {
        for (p, (tags, conflict)) in paths(&i.tree) {
            let e = expected.entry(p).or_insert((BTreeSet::new(), false));
            e.0.insert(tags);
            e.1 |= conflict;
        }
    }
    let out_keys: BTreeSet<&Vec<Key>> = out.keys().collect();
    let expected_keys: BTreeSet<&Vec<Key>> = expected.keys().collect();
    assert_eq!(
        out_keys, expected_keys,
        "exactly the union of the inputs' lines"
    );
    for (p, (tags, conflict)) in &out {
        let (input_tags, input_conflict) = &expected[p];
        let want = if input_tags.len() == 1 {
            *input_tags.iter().next().unwrap()
        } else {
            0
        };
        assert_eq!(*tags, want, "tags kept where the inputs agree: {p:?}");
        assert_eq!(conflict, input_conflict, "same conflicts: {p:?}");
    }
    assert_eq!(merged.tree.len() - 1, out.len());
    assert_eq!(merged.input_nodes - merged.shared_nodes, out.len());
    assert!(roles_fit_everywhere(&merged.tree));
    assert!(groups_resolved(&merged.tree));
    // Saves and loads unchanged.
    let bytes = crate::save::encode(&merged.tree, None);
    let back = crate::save::decode(&bytes).unwrap().tree;
    assert_eq!(crate::save::encode(&back, None), bytes);
    assert_eq!(*back.fearless(), *merged.tree.fearless());
}

fn merged(inputs: &[Input]) -> Merged {
    let m = merge(inputs).unwrap();
    check_merge(inputs, &m);
    m
}

// ---------------------------------------------------------------------------------------
// Lines and branches

#[test]
fn the_same_line_in_several_files_appears_once() {
    let mut a = DraftTree::new();
    let r = a.root();
    line(&mut a, r, &[ALISTAR, ANNIE, ASHE]);
    let b = {
        let mut b = DraftTree::new();
        let r = b.root();
        line(&mut b, r, &[ALISTAR, ANNIE, ASHE]);
        b
    };
    let c = {
        let mut c = DraftTree::new();
        let r = c.root();
        line(&mut c, r, &[ALISTAR, ANNIE, ASHE]);
        c
    };
    let m = merged(&[input("a", a), input("b", b), input("c", c)]);
    assert_eq!(m.tree.len(), 4);
    assert_eq!((m.input_nodes, m.shared_nodes, m.tags_dropped), (9, 6, 0));
}

#[test]
fn branches_join_where_the_lines_agree() {
    // a: Alistar -> Annie -> Ashe;  b: Alistar -> Annie -> Fiddle, and Jax.
    let mut a = DraftTree::new();
    let r = a.root();
    line(&mut a, r, &[ALISTAR, ANNIE, ASHE]);
    let mut b = DraftTree::new();
    let r = b.root();
    line(&mut b, r, &[ALISTAR, ANNIE, FIDDLE]);
    line(&mut b, r, &[JAX]);
    let m = merged(&[input("a", a), input("b", b)]);
    let t = &m.tree;
    let root = t.get(t.root()).unwrap();
    assert_eq!(root.children().len(), 2, "Alistar and Jax");
    let alistar = root.children()[0];
    let annie = t.get(alistar).unwrap().children()[0];
    let tails: Vec<u8> = t
        .get(annie)
        .unwrap()
        .children()
        .iter()
        .map(|&c| t.get(c).unwrap().value())
        .collect();
    assert_eq!(tails, vec![ASHE, FIDDLE], "first file's branches first");
    assert_eq!(t.get(root.children()[1]).unwrap().value(), JAX);
}

#[test]
fn merging_a_tree_with_itself_changes_nothing() {
    let mut t = DraftTree::new();
    let last_ban = bans(&mut t);
    // Repeated empty picks, some tagged the same, and champions with tags.
    let e1 = t.add_child(last_ban, EMPTY).unwrap();
    let e2 = t.add_child(last_ban, EMPTY).unwrap();
    let e3 = t.add_child(last_ban, EMPTY).unwrap();
    t.set_roles(e1, 0b00001).unwrap();
    t.set_roles(e2, 0b00001).unwrap();
    let a = t.add_child(e1, ANNIE).unwrap();
    t.set_roles(a, 0b00010).unwrap();
    t.add_child(e2, ASHE).unwrap();
    t.add_child(e3, JAX).unwrap();
    let copy = crate::save::decode(&crate::save::encode(&t, None))
        .unwrap()
        .tree;
    let m = merged(&[input("a", t.clone()), input("b", copy)]);
    assert_eq!(
        crate::save::encode(&m.tree, None),
        crate::save::encode(&t, None)
    );
    assert_eq!(m.tags_dropped, 0);
}

#[test]
fn the_order_of_inputs_only_changes_the_order_of_branches() {
    let mut a = DraftTree::new();
    let r = a.root();
    line(&mut a, r, &[ALISTAR, ANNIE]);
    line(&mut a, r, &[ASHE]);
    let mut b = DraftTree::new();
    let r = b.root();
    line(&mut b, r, &[ASHE, JAX]);
    line(&mut b, r, &[FIDDLE]);
    let ab = merged(&[input("a", a.clone()), input("b", b.clone())]);
    let ba = merged(&[input("b", b), input("a", a)]);
    assert_eq!(paths(&ab.tree), paths(&ba.tree));
}

// ---------------------------------------------------------------------------------------
// Role tags

#[test]
fn the_same_champion_with_different_tags_merges_and_drops_the_tags() {
    // Three files with Annie at blue pick 1: top, mid, and top again.
    let make = |tags: u8, below: u8| {
        let mut t = DraftTree::new();
        let at = bans(&mut t);
        let annie = t.add_child(at, ANNIE).unwrap();
        if tags != 0 {
            t.set_roles(annie, tags).unwrap();
        }
        let next = t.add_child(annie, below).unwrap();
        // Tags below the merged node stay.
        t.set_roles(next, 0b10000).unwrap();
        t
    };
    let m = merged(&[
        input("top", make(0b00001, ASHE)),
        input("mid", make(0b00100, JAX)),
    ]);
    let t = &m.tree;
    let at = t.path(t.root()).len(); // just the root
    assert_eq!(at, 1);
    let annie = {
        let mut n = t.root();
        for _ in 0..7 {
            n = t.get(n).unwrap().children()[0];
        }
        n
    };
    assert_eq!(t.get(annie).unwrap().value(), ANNIE);
    assert_eq!(t.get(annie).unwrap().roles(), 0, "different tags: dropped");
    assert_eq!(t.get(annie).unwrap().children().len(), 2);
    assert_eq!(m.tags_dropped, 1);
    for &c in t.get(annie).unwrap().children() {
        assert_eq!(t.get(c).unwrap().roles(), 0b10000);
    }
    // Tagged against untagged also counts as different.
    let m = merged(&[
        input("top", make(0b00001, ASHE)),
        input("any", make(0, ASHE)),
    ]);
    assert_eq!(m.tags_dropped, 1);
    // The same tags stay.
    let m = merged(&[
        input("a", make(0b00001, ASHE)),
        input("b", make(0b00001, JAX)),
    ]);
    assert_eq!(m.tags_dropped, 0);
}

#[test]
fn empty_picks_merge_only_with_the_same_tags_and_in_order() {
    // a: two empties tagged top, one untagged. b: one tagged top, one tagged mid.
    let mut a = DraftTree::new();
    let at = bans(&mut a);
    for (tags, below) in [(0b1, ALISTAR), (0b1, ANNIE), (0, ASHE)] {
        let e = a.add_child(at, EMPTY).unwrap();
        if tags != 0 {
            a.set_roles(e, tags).unwrap();
        }
        a.add_child(e, below).unwrap();
    }
    let mut b = DraftTree::new();
    let at = bans(&mut b);
    for (tags, below) in [(0b1, FIDDLE), (0b100, JAX)] {
        let e = b.add_child(at, EMPTY).unwrap();
        b.set_roles(e, tags).unwrap();
        b.add_child(e, below).unwrap();
    }
    let m = merged(&[input("a", a), input("b", b)]);
    let t = &m.tree;
    let mut at = t.root();
    for _ in 0..6 {
        at = t.get(at).unwrap().children()[0];
    }
    let empties: Vec<(u8, Vec<u8>)> = t
        .get(at)
        .unwrap()
        .children()
        .iter()
        .map(|&e| {
            let n = t.get(e).unwrap();
            let below = n
                .children()
                .iter()
                .map(|&c| t.get(c).unwrap().value())
                .collect();
            (n.roles(), below)
        })
        .collect();
    assert_eq!(
        empties,
        vec![
            // b's first top-tagged empty joined a's first.
            (0b1, vec![ALISTAR, FIDDLE]),
            (0b1, vec![ANNIE]),
            (0, vec![ASHE]),
            // Different tags: a branch of its own.
            (0b100, vec![JAX]),
        ]
    );
    assert_eq!(m.tags_dropped, 0);
}

#[test]
fn skipped_bans_merge_like_empty_picks() {
    let mut a = DraftTree::new();
    let r = a.root();
    let n1 = a.add_child(r, NO_BAN).unwrap();
    a.add_child(n1, ALISTAR).unwrap();
    let n2 = a.add_child(r, NO_BAN).unwrap();
    a.add_child(n2, ANNIE).unwrap();
    let mut b = DraftTree::new();
    let r = b.root();
    let n1 = b.add_child(r, NO_BAN).unwrap();
    b.add_child(n1, ASHE).unwrap();
    let m = merged(&[input("a", a), input("b", b)]);
    let root = m.tree.get(m.tree.root()).unwrap();
    assert_eq!(root.children().len(), 2);
    assert_eq!(m.tree.get(root.children()[0]).unwrap().children().len(), 2);
}

// ---------------------------------------------------------------------------------------
// Groups

#[test]
fn groups_with_the_same_champions_become_one() {
    // a's 190 and b's 195 are the same group: their nodes merge, under a's number and name.
    let mut a = tree_with(&[g(190, "Divers", &[20, 21], 0)]);
    let r = a.root();
    line(&mut a, r, &[190, ALISTAR]);
    let mut b = tree_with(&[g(195, "Other name", &[20, 21], 0)]);
    let r = b.root();
    line(&mut b, r, &[195, ANNIE]);
    let m = merged(&[input("a", a), input("b", b)]);
    let t = &m.tree;
    assert_eq!(t.get(t.root()).unwrap().children().len(), 1);
    let user: Vec<&Group> = t.groups().user_groups().collect();
    assert_eq!(user.len(), 1);
    assert_eq!((user[0].value, user[0].name.as_str()), (190, "Divers"));
}

#[test]
fn groups_that_differ_only_in_role_lock_merge_and_lose_the_lock() {
    let mut a = tree_with(&[g(190, "Tops", &[20, 21], 0b1)]);
    let r = a.root();
    line(&mut a, r, &[190]);
    let mut b = tree_with(&[g(190, "Tops", &[20, 21], 0b11)]);
    let r = b.root();
    line(&mut b, r, &[190]);
    let mut c = tree_with(&[g(200, "Tops", &[20, 21], 0b1)]);
    let r = c.root();
    line(&mut c, r, &[200, ASHE]);
    let m = merged(&[input("a", a), input("b", b), input("c", c)]);
    let groups: Vec<&Group> = m.tree.groups().user_groups().collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].roles, 0, "lock dropped");
    assert_eq!(m.tree.len(), 3);
}

#[test]
fn different_groups_with_the_same_value_are_kept_apart() {
    let mut a = tree_with(&[g(190, "A", &[20, 21], 0)]);
    let r = a.root();
    line(&mut a, r, &[190, ALISTAR]);
    let mut b = tree_with(&[g(190, "B", &[30, 31], 0), g(191, "B2", &[40], 0)]);
    let r = b.root();
    line(&mut b, r, &[190, 191]);
    let m = merged(&[input("a", a), input("b", b)]);
    let t = &m.tree;
    let values: Vec<(u8, &str)> = t
        .groups()
        .user_groups()
        .map(|g| (g.value, g.name.as_str()))
        .collect();
    assert_eq!(values, vec![(190, "A"), (191, "B2"), (192, "B")]);
    assert_eq!(t.get(t.root()).unwrap().children().len(), 2);
}

#[test]
fn too_many_groups_together_are_refused() {
    let full: Vec<Group> = (190..=255).map(|v| g(v, "A", &[v - 150], 0)).collect();
    let mut a = tree_with(&full);
    let r = a.root();
    for v in 190..=255 {
        a.add_child(r, v).unwrap();
    }
    let mut b = tree_with(&[g(190, "New", &[16, 17, 18], 0)]);
    let r = b.root();
    b.add_child(r, 190).unwrap();
    assert_eq!(
        merge(&[input("a", a), input("b", b)]).err(),
        Some(MergeError::TooManyGroups)
    );
}

// ---------------------------------------------------------------------------------------
// Fearless, conflicts and errors

#[test]
fn different_fearless_settings_are_refused() {
    let mut f = Fearless {
        game: 2,
        ..Fearless::default()
    };
    f.rows[0][0] = ALISTAR;
    let mut a = DraftTree::new();
    a.set_fearless(f);
    let b = a.clone();
    let mut c = DraftTree::new();
    let mut soft = f;
    soft.mode = Mode::Soft;
    c.set_fearless(soft);
    let d = DraftTree::new();
    let err = merge(&[
        input("a.dtt", a.clone()),
        input("b.dtt", b),
        input("c.dtt", c),
        input("d.dtt", d),
    ])
    .err()
    .unwrap();
    assert_eq!(
        err,
        MergeError::FearlessDiffers {
            first: "a.dtt".into(),
            differing: vec!["c.dtt".into(), "d.dtt".into()],
        }
    );
    assert!(err.to_string().contains("c.dtt, d.dtt differ from a.dtt"));
    // The same settings are kept, unreachable branches included.
    let mut x = DraftTree::new();
    let r = x.root();
    x.add_child(r, ALISTAR).unwrap();
    x.set_fearless(f);
    let m = merged(&[input("a", a), input("x", x)]);
    assert_eq!(*m.tree.fearless(), f);
    assert_eq!(m.tree.unreachable().len(), 1);
}

#[test]
fn conflicting_branches_merge_like_any_other() {
    // a: empty -> Annie, with the empty set to Annie: a conflict. b: Annie -> Annie... is not
    // buildable, so b is a plain tree sharing the prefix.
    let mut a = DraftTree::new();
    let r = a.root();
    let e = a.add_child(r, NO_BAN).unwrap();
    a.add_child(e, ANNIE).unwrap();
    a.set_value(e, ANNIE).unwrap();
    assert_eq!(a.conflicts().len(), 1);
    let mut b = DraftTree::new();
    let r = b.root();
    line(&mut b, r, &[ANNIE, ASHE]);
    let m = merged(&[input("a", a), input("b", b)]);
    assert_eq!(m.tree.conflicts().len(), 1);
}

#[test]
fn fewer_than_two_inputs_are_refused() {
    assert_eq!(merge(&[]).err(), Some(MergeError::NotEnoughInputs));
    assert_eq!(
        merge(&[input("a", DraftTree::new())]).err(),
        Some(MergeError::NotEnoughInputs)
    );
}

// ---------------------------------------------------------------------------------------
// Randomized

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

/// A random tree on small value pools, with its own random group catalog drawn from a
/// few shared definitions (so the same groups appear under different numbers, with
/// different locks, and different groups under the same number).
fn random_tree(rng: &mut Rng) -> DraftTree {
    let definitions: [&[u8]; 4] = [&[16, 17], &[17, 18, 19], &[20, 21], &[22]];
    let mut catalog = Vec::new();
    for members in definitions {
        if rng.below(3) == 0 {
            continue;
        }
        let value = 190 + rng.below(4) as u8;
        if catalog.iter().any(|g: &Group| g.value == value) {
            continue;
        }
        let roles = [0, 0b1, 0b11][rng.below(3)];
        catalog.push(g(value, "G", members, roles));
    }
    let mut t = tree_with(&catalog);
    // Half the trees reach the picks through six skipped bans, so tags come into play.
    if rng.below(2) == 0 {
        bans(&mut t);
    }
    edit(&mut t, rng, 60);
    t
}

/// Up to `edits` + 5 random edits of `t`, then random tags on picks.
fn edit(t: &mut DraftTree, rng: &mut Rng, edits: usize) {
    let mut pool: Vec<u8> = (16..24).chain([EMPTY, NO_BAN]).collect();
    pool.extend(t.groups().user_groups().map(|g| g.value));
    for _ in 0..rng.below(edits) + 5 {
        let nodes = {
            let mut out = Vec::new();
            let mut stack = vec![t.root()];
            while let Some(n) = stack.pop() {
                out.push(n);
                stack.extend(t.get(n).unwrap().children());
            }
            out
        };
        let n = nodes[rng.below(nodes.len())];
        let v = pool[rng.below(pool.len())];
        match rng.below(8) {
            0..=4 => {
                // Mostly shallow, so lines overlap between trees.
                if t.path(n).len() < 14 {
                    let _ = t.add_child(n, v);
                }
            }
            5 => {
                let _ = t.set_roles(n, [1 << rng.below(5), 0b11, 0][rng.below(3)]);
            }
            6 => {
                let _ = t.set_value(n, v);
            }
            _ => {
                let _ = t.duplicate_branch(n);
            }
        }
    }
    // Tag some picks.
    let mut stack = vec![t.root()];
    let mut picks = Vec::new();
    while let Some(n) = stack.pop() {
        if t.can_tag(n) {
            picks.push(n);
        }
        stack.extend(t.get(n).unwrap().children());
    }
    for n in picks {
        if rng.below(3) == 0 {
            let _ = t.set_roles(n, [0b1, 0b10, 0b100, 0b11, 0][rng.below(5)]);
        }
    }
}

#[test]
fn random_merges_have_exactly_the_union_of_lines() {
    let mut total_shared = 0;
    let mut total_dropped = 0;
    for seed in 1..=150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let count = 2 + rng.below(3);
        // Half the runs merge unrelated trees; the other half edit copies of one tree, so
        // they share most lines and disagree on some tags.
        let base = random_tree(&mut rng);
        let inputs: Vec<Input> = (0..count)
            .map(|i| {
                let mut r = Rng((seed + i as u64 * 7919).wrapping_mul(0x2545_F491));
                let tree = if seed % 2 == 0 {
                    let mut t = base.clone();
                    edit(&mut t, &mut r, 20);
                    t
                } else {
                    random_tree(&mut r)
                };
                input(&format!("t{i}"), tree)
            })
            .collect();
        let m = match merge(&inputs) {
            Ok(m) => m,
            Err(e) => panic!("seed {seed}: {e}"),
        };
        check_merge(&inputs, &m);
        total_shared += m.shared_nodes;
        total_dropped += m.tags_dropped;
        // Merging the result with any input changes nothing.
        let again = merge(&[
            input("m", m.tree.clone()),
            input("x", inputs[0].tree.clone()),
        ])
        .unwrap();
        assert_eq!(paths(&again.tree), paths(&m.tree), "seed {seed}");
    }
    // The run actually exercised sharing and tag dropping.
    assert!(total_shared > 100, "{total_shared}");
    assert!(total_dropped > 5, "{total_dropped}");
}

#[test]
fn names_carry_over_only_when_they_agree() {
    let mut a = DraftTree::new();
    a.set_team_name(0, "G2");
    a.set_team_name(1, "FNC");
    a.set_patch("14.20");
    let mut b = a.clone();
    let inputs = |a: &DraftTree, b: &DraftTree| {
        vec![
            Input {
                name: "a".into(),
                tree: a.clone(),
            },
            Input {
                name: "b".into(),
                tree: b.clone(),
            },
        ]
    };
    let same = merge(&inputs(&a, &b)).unwrap().tree;
    assert_eq!(same.team_names(), &["G2".to_string(), "FNC".to_string()]);
    assert_eq!(same.patch(), "14.20");
    b.set_team_name(1, "MAD");
    b.set_patch("14.21");
    let differ = merge(&inputs(&a, &b)).unwrap().tree;
    assert_eq!(differ.team_names(), &[String::new(), String::new()]);
    assert_eq!(differ.patch(), "");
}

#[test]
fn merges_over_the_node_limit_are_refused() {
    // Two lines of three nodes that share their first node: five nodes merged.
    let mut a = DraftTree::new();
    let root = a.root();
    line(&mut a, root, &[ALISTAR, ANNIE, ASHE]);
    let mut b = DraftTree::new();
    let root = b.root();
    line(&mut b, root, &[ALISTAR, FIDDLE, JAX]);
    let inputs = [input("a", a), input("b", b)];
    assert_eq!(merge_capped(&inputs, 5).unwrap().tree.len() - 1, 5);
    assert_eq!(
        merge_capped(&inputs, 4).err(),
        Some(MergeError::TooManyNodes)
    );
    assert_eq!(
        MergeError::TooManyNodes.to_string(),
        "The merged tree would have more than 250,000 nodes, so it can't be merged."
    );
}
