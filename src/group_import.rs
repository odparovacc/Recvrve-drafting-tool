//! Bringing the user groups of a loaded save file into the app.
//!
//! A save carries the definitions of the user groups its tree uses (see `save.rs`). When
//! it loads, [`plan`] matches each of them against the active config:
//!
//! 1. the same value with the same champions: the active group is used as it is (its
//!    name, note and role lock win);
//! 2. another active group with the same champions and role lock: the file's nodes are
//!    renumbered to it;
//! 3. a value the active config doesn't use: the stored group is added at that value;
//! 4. a value the active config uses for something else: the stored group is added at the
//!    first free value, and its nodes are renumbered.
//!
//! If nothing is added, the active config stays as it is. Otherwise the session switches
//! to the active groups plus the added ones, written to `config/imported_configs/`. If
//! that would need more than the 66 user group values, the session switches to the
//! file's own groups instead, written to `config/temp/`.
//!
//! The planning is pure (it only looks at catalogs) and works on any incoming list of
//! groups, so merging two save files can use it too.

use std::path::{Path, PathBuf};

use crate::groups::{self, Group, GroupCatalog, USER_FIRST};

/// Most user groups a catalog can hold: values 190-255.
pub const MAX_USER_GROUPS: usize = 256 - USER_FIRST as usize;

/// Folder (inside the config folder) for configs imported from save files.
pub const IMPORTED_DIR_NAME: &str = "imported_configs";
/// Folder (inside the config folder) for configs imported from save files whose groups
/// didn't fit next to the active ones.
pub const TEMP_DIR_NAME: &str = "temp";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every stored group matched an active one: the active config stays.
    Unchanged,
    /// Stored groups were added to the active ones.
    Merged,
    /// Too many groups for both: the stored groups replace the active ones.
    FileOnly,
}

/// What loading a save does with its groups. See the module docs.
#[derive(Clone, Debug)]
pub struct Plan {
    pub outcome: Outcome,
    /// Value in the file -> value in the session. The identity for everything that isn't
    /// a stored user group. Different stored groups never map to the same value.
    pub map: [u8; 256],
    /// User groups of the session after loading.
    pub groups: Vec<Group>,
    /// Stored groups that were added, with the values they got.
    pub added: Vec<Group>,
    /// Stored groups that got a different value: (value in the file, value now).
    pub moved: Vec<(u8, u8)>,
}

impl Plan {
    /// The session catalog, with `source` as its config file.
    pub fn catalog(&self, source: Option<PathBuf>) -> GroupCatalog {
        GroupCatalog::with_user_groups(self.groups.clone(), source)
    }
}

/// Matches `incoming` user groups against the `active` catalog (see the module docs).
pub fn plan(active: &GroupCatalog, incoming: &[Group]) -> Plan {
    plan_with(active, incoming, false)
}

/// Like [`plan`], for merging save files (see `merge.rs`): groups with the same champions
/// match whatever their role locks, and a group matched with a different lock loses its
/// lock. [`Outcome::FileOnly`] means the groups don't fit together.
pub fn plan_merge(merged: &GroupCatalog, incoming: &[Group]) -> Plan {
    plan_with(merged, incoming, true)
}

fn plan_with(active: &GroupCatalog, incoming: &[Group], merging: bool) -> Plan {
    let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
    let active_user: Vec<&Group> = active.user_groups().collect();
    // Values that the session catalog already uses or has handed out.
    let mut taken = [false; 256];
    for g in &active_user {
        taken[usize::from(g.value)] = true;
    }
    // Active groups some stored group maps to, so no two map to the same one.
    let mut target = [false; 256];
    let mut resolved = vec![false; incoming.len()];
    // Active groups whose role lock is dropped (merging only).
    let mut unlocked = [false; 256];

    // 1. Same value, same champions.
    for (i, g) in incoming.iter().enumerate() {
        if let Some(a) = active
            .get(g.value)
            .filter(|a| groups::is_user_group(a.value) && a.members == g.members)
        {
            target[usize::from(g.value)] = true;
            resolved[i] = true;
            unlocked[usize::from(a.value)] |= merging && a.roles != g.roles;
        }
    }
    // 2. Another active group with the same champions and role lock.
    for (i, g) in incoming.iter().enumerate() {
        if resolved[i] {
            continue;
        }
        if let Some(a) = active_user.iter().find(|a| {
            !target[usize::from(a.value)]
                && a.members == g.members
                && (merging || a.roles == g.roles)
        }) {
            target[usize::from(a.value)] = true;
            map[usize::from(g.value)] = a.value;
            resolved[i] = true;
            unlocked[usize::from(a.value)] |= a.roles != g.roles;
        }
    }
    // 3. Free values keep their number; 4. the rest take the first free values.
    let mut added: Vec<Group> = Vec::new();
    for (i, g) in incoming.iter().enumerate() {
        if !resolved[i] && !taken[usize::from(g.value)] {
            taken[usize::from(g.value)] = true;
            added.push(g.clone());
            resolved[i] = true;
        }
    }
    let mut moved = Vec::new();
    let mut overflow = false;
    for (i, g) in incoming.iter().enumerate() {
        if resolved[i] {
            continue;
        }
        match (USER_FIRST..=u8::MAX).find(|&v| !taken[usize::from(v)]) {
            Some(v) => {
                taken[usize::from(v)] = true;
                map[usize::from(g.value)] = v;
                moved.push((g.value, v));
                added.push(renumbered(g, v));
            }
            None => overflow = true,
        }
    }

    if overflow {
        return Plan {
            outcome: Outcome::FileOnly,
            map: std::array::from_fn(|i| i as u8),
            groups: incoming.to_vec(),
            added: incoming.to_vec(),
            moved: Vec::new(),
        };
    }
    let outcome = if added.is_empty() {
        Outcome::Unchanged
    } else {
        Outcome::Merged
    };
    let mut session: Vec<Group> = active_user
        .into_iter()
        .map(|g| {
            if unlocked[usize::from(g.value)] {
                g.clone().with_roles(0)
            } else {
                g.clone()
            }
        })
        .collect();
    session.extend(added.iter().cloned());
    session.sort_by_key(|g| g.value);
    added.sort_by_key(|g| g.value);
    Plan {
        outcome,
        map,
        groups: session,
        added,
        moved,
    }
}

/// `group` at another value (its label follows the value).
fn renumbered(group: &Group, value: u8) -> Group {
    Group::new_user(
        value,
        group.name.clone(),
        group.note.clone(),
        group.members.clone(),
    )
    .with_roles(group.roles)
}

/// Writes `groups` as a config file named after `stem` into `dir`, and returns its path.
/// A file in `dir` with exactly this content is reused instead; otherwise a free name is
/// picked (`stem.json`, `stem-2.json`, ...).
pub fn write_config(dir: &Path, stem: &str, groups: &[Group]) -> std::io::Result<PathBuf> {
    let text = groups::to_config_json(groups);
    std::fs::create_dir_all(dir)?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut same: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter(|p| std::fs::read_to_string(p).is_ok_and(|t| t == text))
            .collect();
        same.sort();
        if let Some(existing) = same.into_iter().next() {
            return Ok(existing);
        }
    }
    let stem = safe_stem(stem);
    let path = (1..)
        .map(|n| {
            if n == 1 {
                dir.join(format!("{stem}.json"))
            } else {
                dir.join(format!("{stem}-{n}.json"))
            }
        })
        .find(|p| !p.exists())
        .expect("some name is free");
    crate::configio::write_with_backup(&path, &text)?;
    Ok(path)
}

/// A file name stem made of safe characters only.
fn safe_stem(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "imported".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(value: u8, name: &str, members: &[u8], roles: u8) -> Group {
        Group::new_user(value, name.into(), String::new(), members.to_vec()).with_roles(roles)
    }

    fn catalog(groups: &[Group]) -> GroupCatalog {
        GroupCatalog::with_user_groups(groups.to_vec(), None)
    }

    #[test]
    fn identical_groups_change_nothing() {
        let active = catalog(&[g(190, "Mine", &[16, 17], 0), g(191, "Other", &[18], 0)]);
        // Same champions at the same value; the stored name doesn't matter.
        let p = plan(&active, &[g(190, "Theirs", &[16, 17], 0b1)]);
        assert_eq!(p.outcome, Outcome::Unchanged);
        assert_eq!(p.map[190], 190);
        assert!(p.added.is_empty());
        assert_eq!(p.groups.len(), 2);
        assert_eq!(p.groups[0].name, "Mine", "active definitions win");
    }

    #[test]
    fn same_champions_elsewhere_are_reused() {
        let active = catalog(&[g(195, "Mine", &[16, 17], 0b11)]);
        let p = plan(&active, &[g(190, "Theirs", &[16, 17], 0b11)]);
        assert_eq!(p.outcome, Outcome::Unchanged);
        assert_eq!(p.map[190], 195);
        // A different role lock is a different group.
        let p = plan(&active, &[g(190, "Theirs", &[16, 17], 0b1)]);
        assert_eq!(p.outcome, Outcome::Merged);
        assert_eq!(p.map[190], 190);
    }

    #[test]
    fn two_stored_groups_never_share_one_active_group() {
        // Stored 190 matches active 190 by value (rule 1); stored 191 has the same
        // champions but must not map to 190 as well.
        let active = catalog(&[g(190, "Mine", &[16, 17], 0)]);
        let p = plan(
            &active,
            &[g(190, "A", &[16, 17], 0), g(191, "B", &[16, 17], 0)],
        );
        assert_eq!(p.map[190], 190);
        assert_eq!(p.map[191], 191);
        assert_eq!(p.outcome, Outcome::Merged);
    }

    #[test]
    fn free_values_are_kept_and_taken_ones_move_to_the_first_free_value() {
        let active = catalog(&[g(190, "Mine", &[16], 0), g(191, "Mine2", &[17], 0)]);
        let p = plan(
            &active,
            &[
                g(190, "Clash", &[18], 0),
                g(193, "Free", &[19], 0),
                g(191, "Clash2", &[20], 0),
            ],
        );
        assert_eq!(p.outcome, Outcome::Merged);
        assert_eq!(p.map[193], 193);
        // 192 is the first free value, then 194 (193 is kept by "Free").
        assert_eq!(p.map[190], 192);
        assert_eq!(p.map[191], 194);
        assert_eq!(p.moved, vec![(190, 192), (191, 194)]);
        let values: Vec<u8> = p.groups.iter().map(|g| g.value).collect();
        assert_eq!(values, vec![190, 191, 192, 193, 194]);
        assert_eq!(p.groups[2].name, "Clash");
        assert_eq!(p.groups[2].label, "G3", "labels follow the new value");
        // Non-group values are never touched.
        assert!((0..190).all(|v| p.map[v] == v as u8));
    }

    #[test]
    fn too_many_groups_fall_back_to_the_file_groups() {
        let full: Vec<Group> = (190..=255).map(|v| g(v, "Full", &[v - 100], 0)).collect();
        let active = catalog(&full);
        let stored = [g(190, "New", &[16, 17, 18], 0), g(200, "Same", &[100], 0)];
        let p = plan(&active, &stored);
        assert_eq!(p.outcome, Outcome::FileOnly);
        assert_eq!(p.groups, stored.to_vec());
        assert!(p.map.iter().enumerate().all(|(i, &v)| v == i as u8));
        // One free value is enough again.
        let active = catalog(&full[1..]);
        let p = plan(&active, &stored);
        assert_eq!(p.outcome, Outcome::Merged);
        assert_eq!(p.map[190], 190);
        assert_eq!(p.map[200], 200, "rule 1");
    }

    #[test]
    fn config_files_are_written_once_and_named_safely() {
        let dir = std::env::temp_dir().join(format!("recvrve-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let groups = [g(190, "A", &[16], 0)];
        let first = write_config(&dir, "my save/../x", &groups).unwrap();
        assert_eq!(first.file_name().unwrap(), "my_save_.._x.json");
        // Same content: the same file.
        assert_eq!(write_config(&dir, "other", &groups).unwrap(), first);
        // Different content, same name: a numbered file.
        let second = write_config(&dir, "my save/../x", &[g(190, "B", &[17], 0)]).unwrap();
        assert_eq!(second.file_name().unwrap(), "my_save_.._x-2.json");
        let loaded = groups::load_config(&second).unwrap();
        assert_eq!(loaded.get(190).unwrap().name, "B");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saves a tree made under one config and loads it under another, checking that every
    /// group node still stands for the same champions and the same conflicts show.
    fn load_into(made_with: &[Group], active: &[Group]) -> (crate::tree::DraftTree, Plan) {
        use crate::tree::DraftTree;
        use std::sync::Arc;
        let mut t = DraftTree::with_groups(Arc::new(catalog(made_with)));
        let root = t.root();
        let mut at = root;
        for g in made_with {
            // A line through every group, plus each group as a branch of the root.
            at = t.add_child(at, g.value).unwrap();
            t.add_child(root, g.value).ok();
        }
        let bytes = crate::save::encode(&t, None);
        let loaded = crate::save::decode(&bytes).unwrap().tree;
        let stored: Vec<Group> = loaded.groups().user_groups().cloned().collect();
        let p = plan(&catalog(active), &stored);
        let mut back = loaded;
        back.remap_groups(&p.map, Arc::new(p.catalog(None)));
        // Same shape, and each group node has the same champions as before.
        let walk = |t: &DraftTree| {
            let mut out = Vec::new();
            let mut stack = vec![t.root()];
            while let Some(n) = stack.pop() {
                let v = t.get(n).unwrap().value();
                out.push(t.groups().get(v).map_or(vec![v], |g| g.members.clone()));
                stack.extend(t.get(n).unwrap().children().iter().rev());
            }
            out
        };
        assert_eq!(walk(&back), walk(&t));
        assert_eq!(back.conflicts().len(), t.conflicts().len());
        // And it saves again without trouble.
        let again = crate::save::encode(&back, None);
        assert!(crate::save::decode(&again).is_ok());
        (back, p)
    }

    #[test]
    fn loading_into_another_config_keeps_every_group_meaning() {
        let made = [
            g(190, "Enchanters", &[16, 17, 18], 0b10000),
            g(191, "Divers", &[20, 21], 0),
            g(200, "Same", &[30, 31], 0),
        ];
        // Mine: 190 is something else, my 195 equals their 191, 200 matches by value.
        let mine = [
            g(190, "Mine", &[40], 0),
            g(195, "Divers too", &[20, 21], 0),
            g(200, "Same here", &[30, 31], 0b1),
        ];
        let (back, p) = load_into(&made, &mine);
        assert_eq!(p.outcome, Outcome::Merged);
        assert_eq!(p.map[190], 191, "first free value");
        assert_eq!(p.map[191], 195, "reused");
        assert_eq!(p.map[200], 200, "same value");
        assert_eq!(back.groups().user_groups().count(), 4);
        assert_eq!(back.groups().get(200).unwrap().name, "Same here");
        // Empty config: everything keeps its value.
        let (_, p) = load_into(&made, &[]);
        assert_eq!(p.outcome, Outcome::Merged);
        assert!(p.moved.is_empty());
        // Same config: nothing to import.
        let (_, p) = load_into(&made, &made);
        assert_eq!(p.outcome, Outcome::Unchanged);
    }

    #[test]
    fn loading_with_conflicting_groups_keeps_the_conflicts() {
        use crate::tree::{DraftTree, NO_BAN};
        use std::sync::Arc;
        // G = {16, 17}: empty, G, G, then the empty node becomes 16: the second G conflicts.
        let made = [g(190, "G", &[16, 17], 0)];
        let mut t = DraftTree::with_groups(Arc::new(catalog(&made)));
        let e = t.add_child(t.root(), NO_BAN).unwrap();
        let a = t.add_child(e, 190).unwrap();
        t.add_child(a, 190).unwrap();
        t.set_value(e, 16).unwrap();
        assert_eq!(t.conflicts().len(), 1);
        let loaded = crate::save::decode(&crate::save::encode(&t, None))
            .unwrap()
            .tree;
        let p = plan(&catalog(&[g(190, "Taken", &[50], 0)]), &stored(&loaded));
        let mut back = loaded;
        back.remap_groups(&p.map, Arc::new(p.catalog(None)));
        assert_eq!(p.map[190], 191);
        assert_eq!(back.conflicts().len(), 1);
    }

    fn stored(t: &crate::tree::DraftTree) -> Vec<Group> {
        t.groups().user_groups().cloned().collect()
    }

    #[test]
    fn undo_across_an_import_brings_the_old_config_back() {
        use crate::history::{ChangeKind, History, snapshot};
        use crate::tree::DraftTree;
        use std::sync::Arc;
        let mine = Arc::new(catalog(&[g(190, "Mine", &[40], 0)]));
        let mut current = DraftTree::with_groups(mine.clone());
        let n = current.add_child(current.root(), 190).unwrap();
        let mut history = History::default();

        // Load a save whose 190 is a different group: it moves to 191.
        let theirs = {
            let mut t = DraftTree::with_groups(Arc::new(catalog(&[g(190, "Theirs", &[16], 0)])));
            t.add_child(t.root(), 190).unwrap();
            crate::save::decode(&crate::save::encode(&t, None))
                .unwrap()
                .tree
        };
        let p = plan(&mine, &stored(&theirs));
        let merged = Arc::new(p.catalog(None));
        let mut loaded = theirs;
        loaded.remap_groups(&p.map, merged.clone());
        history.record(snapshot(&current, n), ChangeKind::Load);
        let root = loaded.root();
        current = loaded;

        // Undo: my tree and my config.
        let mut t = current;
        let cursor = history.undo(&mut t, root).unwrap().cursor;
        assert!(Arc::ptr_eq(t.groups(), &mine));
        assert_eq!(t.get(cursor).unwrap().value(), 190);
        // Redo: the loaded tree and the merged config.
        history.redo(&mut t, cursor).unwrap();
        assert!(Arc::ptr_eq(t.groups(), &merged));
        assert_eq!(t.groups().get(191).unwrap().name, "Theirs");
    }

    /// Random active and incoming catalogs (few member sets and values, so matches and
    /// clashes are common), in both modes, checking everything a plan promises.
    #[test]
    fn random_plans_keep_their_promises() {
        let sets: [&[u8]; 6] = [&[16], &[16, 17], &[17, 18], &[20, 21, 22], &[30], &[40, 41]];
        let mut state = 0x1234_5678_9ABC_DEF1u64;
        let mut next = |n: usize| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) % n as u64) as usize
        };
        let random_groups = |count: usize, next: &mut dyn FnMut(usize) -> usize| {
            let mut out: Vec<Group> = Vec::new();
            for _ in 0..count {
                // Mostly low values, sometimes anywhere up to 255.
                let value = if next(4) == 0 {
                    190 + next(66) as u8
                } else {
                    190 + next(8) as u8
                };
                if out.iter().any(|g| g.value == value) {
                    continue;
                }
                out.push(g(value, "R", sets[next(sets.len())], [0, 1, 3][next(3)]));
            }
            out
        };
        for round in 0..3000 {
            let active_count = if round % 10 == 0 { 66 } else { next(10) };
            let mut active = random_groups(active_count, &mut next);
            if round % 10 == 0 {
                // A full config.
                active = (190..=255)
                    .map(|v| g(v, "F", sets[next(sets.len())], 0))
                    .collect();
            }
            let incoming = random_groups(1 + next(8), &mut next);
            let merging = round % 2 == 1;
            let active_catalog = catalog(&active);
            let p = if merging {
                plan_merge(&active_catalog, &incoming)
            } else {
                plan(&active_catalog, &incoming)
            };
            let session = p.catalog(None);
            if p.outcome == Outcome::FileOnly {
                assert_eq!(p.groups, incoming);
                continue;
            }
            assert!(p.groups.len() <= MAX_USER_GROUPS);
            // Every active group is still there, with the same champions.
            for a in &active {
                let s = session.get(a.value).expect("active group kept");
                assert_eq!(s.members, a.members);
                assert_eq!(s.name, a.name);
                if !merging {
                    assert_eq!(s.roles, a.roles, "loading never changes locks");
                }
            }
            // Every incoming group maps to a group with the same champions, and no two
            // incoming groups map to the same value.
            let mut targets = std::collections::HashSet::new();
            for inc in &incoming {
                let to = p.map[usize::from(inc.value)];
                let s = session.get(to).expect("mapped group exists");
                assert_eq!(s.members, inc.members, "round {round}");
                assert!(targets.insert(to), "round {round}: two groups map to G{to}");
                if !merging && to != inc.value && active.iter().any(|a| a.value == to) {
                    assert_eq!(
                        s.roles, inc.roles,
                        "rule 2 needs the same lock when loading"
                    );
                }
                if merging && s.roles != 0 {
                    assert_eq!(
                        s.roles, inc.roles,
                        "differing locks are dropped when merging"
                    );
                }
            }
            // Labels follow values, and values are unique.
            let values: Vec<u8> = p.groups.iter().map(|g| g.value).collect();
            let mut sorted = values.clone();
            sorted.dedup();
            assert_eq!(values, sorted);
            for g in &p.groups {
                assert_eq!(g.label, format!("G{}", g.value - USER_FIRST + 1));
            }
            assert_eq!(p.outcome == Outcome::Unchanged, p.added.is_empty());
        }
    }
}
