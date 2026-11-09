//! Copying a representational group into the selection groups and back.
//!
//! The original always stays. The copy keeps the name (made unique, see [`unique_name`])
//! and the note; role locks are dropped either way.
//!
//! - Representational → selection: the members are its champions.
//! - Selection → representational: champions are kept; group tiles (presets and user
//!   groups) are replaced by their champions, or left out when asked to. The copy gets the
//!   first free user group value, so its label (G1 ... G66) follows from that.

use crate::champions;
use crate::groups::{self, Group, GroupCatalog, MIN_RGROUP_CHAMPIONS, USER_FIRST};
use crate::selection::{self, SelectionGroup, ValueSet};

/// Longest group name, in characters, for both kinds of group.
const MAX_NAME_CHARS: usize = selection::MAX_NAME_CHARS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyError {
    /// All 66 user group values are in use.
    NoFreeValue,
    /// Fewer than [`MIN_RGROUP_CHAMPIONS`] champions are left.
    TooFewChampions(usize),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::NoFreeValue => write!(
                f,
                "All {} representational group values are in use. Delete one first.",
                crate::group_import::MAX_USER_GROUPS
            ),
            CopyError::TooFewChampions(n) => write!(
                f,
                "A representational group needs at least {MIN_RGROUP_CHAMPIONS} champions, and this one would have {n}."
            ),
        }
    }
}

/// `name` if `taken` doesn't hold it (ignoring case), otherwise `name (1)`, `name (2)`, ...
/// shortening `name` so the result fits in 16 characters.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = name.trim();
    let is_taken = |candidate: &str| taken(&candidate.to_lowercase());
    if !is_taken(base) {
        return base.to_string();
    }
    (1..)
        .map(|n| {
            let suffix = format!(" ({n})");
            let room = MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
            let short: String = base.chars().take(room).collect();
            format!("{}{suffix}", short.trim_end())
        })
        .find(|candidate| !is_taken(candidate))
        .expect("some number is free")
}

/// A selection group copied from `group`, named so it doesn't clash with `existing` or the
/// built-in role filters.
pub fn to_selection(group: &Group, existing: &[SelectionGroup]) -> SelectionGroup {
    let taken: Vec<String> = existing
        .iter()
        .map(|g| g.name.trim().to_lowercase())
        .chain(selection::PROTECTED_NAMES.iter().map(|n| n.to_lowercase()))
        .collect();
    let mut members = ValueSet::default();
    for &m in &group.members {
        members.insert(m);
    }
    SelectionGroup {
        name: unique_name(&group.name, |n| taken.iter().any(|t| t == n)),
        note: group.note.clone(),
        members,
    }
}

/// A user representational group copied from `group`, at the first value `catalog`
/// doesn't use, named so it doesn't clash with any group in `catalog`. With
/// `omit_groups`, group tiles in `group` are left out instead of replaced by their
/// champions.
pub fn to_representational(
    group: &SelectionGroup,
    catalog: &GroupCatalog,
    omit_groups: bool,
) -> Result<Group, CopyError> {
    let mut members = ValueSet::default();
    for v in group.members.values() {
        if champions::by_value(v).is_some() {
            members.insert(v);
        } else if !omit_groups && let Some(g) = catalog.get(v) {
            for &m in &g.members {
                members.insert(m);
            }
        }
    }
    let champions: Vec<u8> = members.values().collect();
    if champions.len() < MIN_RGROUP_CHAMPIONS {
        return Err(CopyError::TooFewChampions(champions.len()));
    }
    let value = (USER_FIRST..=u8::MAX)
        .find(|&v| catalog.get(v).is_none())
        .ok_or(CopyError::NoFreeValue)?;
    let taken: Vec<String> = catalog
        .all()
        .iter()
        .map(|g| g.name.trim().to_lowercase())
        .collect();
    let name = unique_name(&group.name, |n| taken.iter().any(|t| t == n));
    debug_assert!(groups::is_user_group(value));
    Ok(Group::new_user(value, name, group.note.clone(), champions))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(value: u8, name: &str, members: &[u8], roles: u8) -> Group {
        Group::new_user(value, name.into(), "a note".into(), members.to_vec()).with_roles(roles)
    }

    fn sel(name: &str, members: &[u8]) -> SelectionGroup {
        let mut set = ValueSet::default();
        for &m in members {
            set.insert(m);
        }
        SelectionGroup {
            name: name.into(),
            note: "sel note".into(),
            members: set,
        }
    }

    #[test]
    fn names_get_a_number_when_taken() {
        let taken = ["enchanters", "enchanters (1)", "top"];
        let is = |n: &str| taken.contains(&n);
        assert_eq!(unique_name("Divers", is), "Divers");
        assert_eq!(unique_name("  Divers ", is), "Divers");
        assert_eq!(unique_name("Enchanters", is), "Enchanters (2)");
        assert_eq!(
            unique_name("ENCHANTERS", is),
            "ENCHANTERS (2)",
            "case is ignored"
        );
        assert_eq!(unique_name("Top", is), "Top (1)");
        // Long names are shortened to fit 16 characters with the suffix.
        let long = "Engage supports";
        assert_eq!(
            unique_name(long, |n| n == "engage supports"),
            "Engage suppo (1)"
        );
        let mut many = vec!["abcdefghijklmnop".to_string()];
        for i in 1..=12 {
            many.push(unique_name("abcdefghijklmnop", |n| {
                many.iter().any(|m| m.to_lowercase() == n)
            }));
            assert!(many[i].chars().count() <= 16);
        }
        assert_eq!(many[10], "abcdefghijk (10)");
        // A space before the suffix is not doubled.
        assert_eq!(
            unique_name("Engage supp ort", |n| n == "engage supp ort"),
            "Engage supp (1)"
        );
    }

    #[test]
    fn representational_to_selection() {
        let g = rep(190, "Enchanters", &[28, 49, 110], 0b10000);
        let existing = vec![sel("Enchanters", &[1]), sel("Divers", &[2])];
        let s = to_selection(&g, &existing);
        assert_eq!(s.name, "Enchanters (1)");
        assert_eq!(s.note, "a note");
        assert_eq!(s.members.values().collect::<Vec<_>>(), vec![28, 49, 110]);
        // The built-in role filter names are always taken, even if missing from the list.
        let s = to_selection(&rep(190, "Support", &[16, 17], 0), &[]);
        assert_eq!(s.name, "Support (1)");
        let s = to_selection(&rep(190, "Free", &[16, 17], 0), &existing);
        assert_eq!(s.name, "Free");
    }

    #[test]
    fn selection_to_representational_expands_group_tiles() {
        let catalog = GroupCatalog::with_user_groups(
            vec![
                rep(190, "Enchanters", &[28, 49], 0b10000),
                rep(192, "Divers", &[30, 31], 0),
            ],
            None,
        );
        // Champions 16, 17, user group 190 and preset 7 (Support).
        let s = sel("Support", &[16, 17, 190, 7]);
        let g = to_representational(&s, &catalog, false).unwrap();
        assert_eq!(g.value, 191, "first free value");
        assert_eq!(g.label, "G2");
        assert_eq!(g.roles, 0, "no lock");
        assert_eq!(g.note, "sel note");
        assert_eq!(g.name, "Support (1)", "the preset Support is taken");
        let preset = catalog.get(7).unwrap();
        for m in [16, 17, 28, 49] {
            assert!(g.contains(m));
        }
        for &m in &preset.members {
            assert!(g.contains(m));
        }
        assert!(g.members.iter().all(|&m| champions::by_value(m).is_some()));

        // Leaving group tiles out keeps only the champions.
        let g = to_representational(&s, &catalog, true).unwrap();
        assert_eq!(g.members, vec![16, 17]);
    }

    #[test]
    fn selection_to_representational_refusals() {
        let catalog = GroupCatalog::with_user_groups(vec![rep(190, "A", &[16, 17], 0)], None);
        // Only group tiles, left out: nothing remains.
        assert_eq!(
            to_representational(&sel("Groups", &[190, 3]), &catalog, true),
            Err(CopyError::TooFewChampions(0))
        );
        assert_eq!(
            to_representational(&sel("One", &[16]), &catalog, false),
            Err(CopyError::TooFewChampions(1))
        );
        // Unknown group values are skipped.
        assert_eq!(
            to_representational(&sel("Unknown", &[200, 16]), &catalog, false),
            Err(CopyError::TooFewChampions(1))
        );
        // No free value.
        let full = GroupCatalog::with_user_groups(
            (190..=255).map(|v| rep(v, "F", &[16], 0)).collect(),
            None,
        );
        assert_eq!(
            to_representational(&sel("X", &[16, 17]), &full, false),
            Err(CopyError::NoFreeValue)
        );
    }

    #[test]
    fn copies_round_trip_through_the_config_files() {
        // A copy is valid for both config formats.
        let catalog =
            GroupCatalog::with_user_groups(vec![rep(190, "Enchanters", &[28, 49], 0)], None);
        let s = to_selection(catalog.get(190).unwrap(), &[]);
        let text = selection::to_config_json(std::slice::from_ref(&s));
        assert_eq!(selection::parse_config(&text).unwrap(), vec![s.clone()]);
        let g = to_representational(&s, &catalog, false).unwrap();
        let mut user: Vec<Group> = catalog.user_groups().cloned().collect();
        user.push(g.clone());
        let back = groups::parse_config(&groups::to_config_json(&user), None).unwrap();
        assert_eq!(back.get(g.value), Some(&g));
        assert_eq!(g.name, "Enchanters (1)");
    }
}
