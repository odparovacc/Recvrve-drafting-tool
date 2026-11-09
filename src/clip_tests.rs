use std::sync::Arc;

use super::*;
use crate::champions;
use crate::groups::GroupCatalog;

fn champ(i: usize) -> u8 {
    champions::value_of(i)
}

/// Root -> a (BB1) -> {a1, a2 -> a21} and root -> b (BB1) -> b1.
fn sample() -> (DraftTree, [NodeId; 6]) {
    let mut t = DraftTree::new();
    let root = t.root();
    let a = t.add_child(root, champ(0)).unwrap();
    let a1 = t.add_child(a, champ(1)).unwrap();
    let a2 = t.add_child(a, champ(2)).unwrap();
    let a21 = t.add_child(a2, champ(3)).unwrap();
    let b = t.add_child(root, champ(4)).unwrap();
    let b1 = t.add_child(b, champ(5)).unwrap();
    (t, [a, a1, a2, a21, b, b1])
}

fn values_below(t: &DraftTree, id: NodeId) -> Vec<u8> {
    t.get(id)
        .unwrap()
        .children()
        .iter()
        .map(|&c| t.get(c).unwrap().value())
        .collect()
}

#[test]
fn copies_need_one_depth() {
    let (t, [a, a1, ..]) = sample();
    assert_eq!(Clip::copy(&t, &[]).err(), Some(ClipError::Nothing));
    assert_eq!(Clip::copy(&t, &[t.root()]).err(), Some(ClipError::Nothing));
    assert_eq!(Clip::copy(&t, &[a, a1]).err(), Some(ClipError::MixedDepths));
    let clip = Clip::copy(&t, &[a1]).unwrap();
    assert_eq!((clip.depth(), clip.len(), clip.nodes()), (2, 1, 1));
}

#[test]
fn paste_goes_one_level_up_and_merges() {
    let (mut t, [a, a1, a2, a21, b, b1]) = sample();
    // a's branches (a1, a2 -> a21) below b: b gets them next to b1.
    let clip = Clip::copy(&t, &[a1, a2]).unwrap();
    assert_eq!(
        paste(&mut t, &clip, a1).err(),
        Some(ClipError::WrongDepth(1)),
        "a1 is one level too deep"
    );
    let before = t.len();
    let pasted = paste(&mut t, &clip, b).unwrap();
    assert_eq!((pasted.added, pasted.merged), (3, 0));
    assert_eq!(t.len(), before + 3);
    assert_eq!(values_below(&t, b), vec![champ(5), champ(1), champ(2)]);
    // Pasting again merges with what's there: nothing new.
    let again = paste(&mut t, &clip, b).unwrap();
    assert_eq!((again.added, again.merged), (0, 3));
    assert_eq!(again.tops, pasted.tops);
    // A paste into the branch it came from is a no-op too.
    let same = paste(&mut t, &clip, a).unwrap();
    assert_eq!(same.added, 0);
    assert_eq!(t.get(a21).unwrap().value(), champ(3));
    let _ = b1;
}

#[test]
fn pasted_repeats_are_conflicts() {
    let (mut t, [a, _, _, _, b, b1]) = sample();
    // b1's branch (champion 5) below a: fine, a's line has champions 0 only.
    let clip = Clip::copy(&t, &[b1]).unwrap();
    paste(&mut t, &clip, a).unwrap();
    assert!(t.conflicts().is_empty());
    // A branch holding champion 4 (b's own) pasted below b repeats it on that line.
    let mut src = DraftTree::new();
    let x = src.add_child(src.root(), champ(9)).unwrap();
    let x1 = src.add_child(x, champ(4)).unwrap();
    let clip = Clip::copy(&src, &[x1]).unwrap();
    let pasted = paste(&mut t, &clip, b).unwrap();
    assert!(t.conflicts().contains_key(pasted.tops[0]));
}

#[test]
fn move_removes_the_originals() {
    let (mut t, [a, a1, a2, a21, b, _]) = sample();
    let moved = copy_or_move(&mut t, &[a2], b, true).unwrap();
    assert!(!t.contains(a2) && !t.contains(a21));
    assert_eq!(values_below(&t, a), vec![champ(1)]);
    assert_eq!(values_below(&t, moved.tops[0]), vec![champ(3)]);
    // Moving onto the parent it already has changes nothing.
    let len = t.len();
    copy_or_move(&mut t, &[a1], a, true).unwrap();
    assert_eq!(t.len(), len);
    assert!(t.contains(a1));
    // A failed move leaves the tree as it was.
    let len = t.len();
    assert!(copy_or_move(&mut t, &[a1], a1, true).is_err());
    assert_eq!(t.len(), len);
}

#[test]
fn marks_hidden_branches_and_tags_come_along() {
    let mut t = DraftTree::new();
    let root = t.root();
    // Six bans, then blue pick 1 tagged top, below it red pick 1.
    let mut at = root;
    for i in 0..6 {
        at = t.add_child(at, champ(10 + i)).unwrap();
    }
    let p = t.add_child_tagged(at, champ(20), 0b00001).unwrap();
    let p1 = t.add_child(p, champ(21)).unwrap();
    t.set_marked(p1, true);
    t.set_collapsed(p, true);
    let clip = Clip::copy(&t, &[p]).unwrap();
    // Another line with the same six bans but a different last ban.
    let other = t
        .add_child(t.get(at).unwrap().parent().unwrap(), champ(30))
        .unwrap();
    let pasted = paste(&mut t, &clip, other).unwrap();
    let top = pasted.tops[0];
    assert_eq!(t.get(top).unwrap().roles(), 0b00001);
    assert!(t.get(top).unwrap().is_collapsed());
    let copy_of_p1 = t.get(top).unwrap().children()[0];
    assert!(t.get(copy_of_p1).unwrap().is_marked());
}

#[test]
fn groups_are_matched_by_champions() {
    let g = crate::groups::Group::new_user(
        190,
        "Mages".into(),
        String::new(),
        vec![champ(0), champ(1)],
    );
    let mut t = DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(
        vec![g.clone()],
        None,
    )));
    let x = t.add_child(t.root(), 190).unwrap();
    let x1 = t.add_child(x, champ(5)).unwrap();
    let clip = Clip::copy(&t, &[x]).unwrap();
    // Another config where the same champions are group 200: the copy maps to it.
    let moved = crate::groups::Group::new_user(
        200,
        "Casters".into(),
        String::new(),
        vec![champ(0), champ(1)],
    );
    let mut other =
        DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(vec![moved], None)));
    let root = other.root();
    let pasted = paste(&mut other, &clip, root).unwrap();
    assert_eq!(other.get(pasted.tops[0]).unwrap().value(), 200);
    // A config without that group refuses.
    let mut bare = DraftTree::new();
    let root = bare.root();
    assert_eq!(
        paste(&mut bare, &clip, root).err(),
        Some(ClipError::UnknownGroup("Mages".into()))
    );
    let _ = x1;
}
