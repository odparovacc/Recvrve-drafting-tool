//! Selection groups: named sets of values used only to filter which tiles the grid shows.
//!
//! They never touch the tree or the draft. They load once at startup from
//! `config/selectiongroups.json`; editing the file takes effect after a restart.
//!
//! ```json
//! {
//!   "groups": [
//!     { "name": "Support", "champions": [16, 26, 7, 190], "note": "..." }
//!   ]
//! }
//! ```
//!
//! `champions` may list champion values and group values (presets 2-15, user groups
//! 190-255), so a filter can also show representational group tiles. Groups are told apart
//! by their position in the file, so names may repeat.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::champions;
use crate::groups::{self, ConfigError};

pub const CONFIG_FILE_NAME: &str = "selectiongroups.json";

pub const MAX_NAME_CHARS: usize = 16;
pub const MAX_NOTE_CHARS: usize = 200;

/// Built-in role filters. They always exist: the editor can't delete or rename them or give
/// another group one of these names, and startup re-adds any that are missing.
pub const PROTECTED_NAMES: [&str; 5] = ["Top", "Jungle", "Mid", "ADC", "Support"];

/// Whether `name` is one of [`PROTECTED_NAMES`]. Case matters; surrounding spaces don't.
pub fn is_protected_name(name: &str) -> bool {
    PROTECTED_NAMES.contains(&name.trim())
}

/// A set of node values as a 256-bit mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValueSet([u64; 4]);

impl ValueSet {
    pub fn insert(&mut self, value: u8) {
        self.0[usize::from(value / 64)] |= 1 << (value % 64);
    }

    pub fn contains(&self, value: u8) -> bool {
        self.0[usize::from(value / 64)] & (1 << (value % 64)) != 0
    }

    pub fn union(self, other: ValueSet) -> ValueSet {
        ValueSet(std::array::from_fn(|i| self.0[i] | other.0[i]))
    }

    pub fn intersection(self, other: ValueSet) -> ValueSet {
        ValueSet(std::array::from_fn(|i| self.0[i] & other.0[i]))
    }

    pub fn toggle(&mut self, value: u8) {
        self.0[usize::from(value / 64)] ^= 1 << (value % 64);
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    /// The values in the set, ascending.
    pub fn values(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=u8::MAX).filter(|&v| self.contains(v))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionGroup {
    pub name: String,
    pub note: String,
    pub members: ValueSet,
}

/// The values to show for the active groups, combined by union or intersection. `None`
/// when no group is active, meaning "show everything".
pub fn combined(groups: &[SelectionGroup], active: &[bool], intersect: bool) -> Option<ValueSet> {
    let mut chosen = groups
        .iter()
        .zip(active)
        .filter(|(_, on)| **on)
        .map(|(g, _)| g.members);
    let first = chosen.next()?;
    Some(chosen.fold(first, |acc, set| {
        if intersect {
            acc.intersection(set)
        } else {
            acc.union(set)
        }
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    groups: Vec<GroupDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupDef {
    name: String,
    champions: Vec<i64>,
    #[serde(default)]
    note: String,
}

/// Whether a filter may list `value`: a champion or any group value.
fn is_listable(value: u8) -> bool {
    champions::by_value(value).is_some() || groups::is_group(value)
}

pub fn parse_config(text: &str) -> Result<Vec<SelectionGroup>, ConfigError> {
    let file: ConfigFile =
        serde_json::from_str(text).map_err(|e| ConfigError(format!("invalid JSON: {e}")))?;
    file.groups
        .into_iter()
        .enumerate()
        .map(|(i, def)| {
            let which = format!("group {} (\"{}\")", i + 1, def.name);
            let name_chars = def.name.chars().count();
            if name_chars == 0 || name_chars > MAX_NAME_CHARS {
                return Err(ConfigError(format!(
                    "{which}: name must be 1-{MAX_NAME_CHARS} characters"
                )));
            }
            if def.note.chars().count() > MAX_NOTE_CHARS {
                return Err(ConfigError(format!(
                    "{which}: note is longer than {MAX_NOTE_CHARS} characters"
                )));
            }
            let mut members = ValueSet::default();
            for c in def.champions {
                let value = u8::try_from(c)
                    .ok()
                    .filter(|&v| is_listable(v))
                    .ok_or_else(|| {
                        ConfigError(format!("{which}: {c} is not a champion or group value"))
                    })?;
                members.insert(value);
            }
            Ok(SelectionGroup {
                name: def.name,
                note: def.note,
                members,
            })
        })
        .collect()
}

#[derive(Serialize)]
struct GroupOut<'a> {
    name: &'a str,
    champions: Vec<u8>,
    note: &'a str,
}

#[derive(Serialize)]
struct ConfigOut<'a> {
    groups: Vec<GroupOut<'a>>,
}

/// The config file contents for `groups`, in the format [`parse_config`] reads.
pub fn to_config_json(groups: &[SelectionGroup]) -> String {
    let out = ConfigOut {
        groups: groups
            .iter()
            .map(|g| GroupOut {
                name: &g.name,
                champions: g.members.values().collect(),
                note: &g.note,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&out).expect("selection groups serialize") + "\n"
}

fn load_file(path: &Path) -> Result<Vec<SelectionGroup>, ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ConfigError(format!("could not read {}: {e}", path.display())))?;
    parse_config(&text)
}

/// Appends an empty group for every protected name that's missing.
fn ensure_protected(groups: &mut Vec<SelectionGroup>) {
    for name in PROTECTED_NAMES {
        if !groups.iter().any(|g| g.name.trim() == name) {
            groups.push(SelectionGroup {
                name: name.to_string(),
                note: String::new(),
                members: ValueSet::default(),
            });
        }
    }
}

/// The default path selection groups are loaded from and saved to.
pub fn config_path() -> PathBuf {
    groups::find_config(CONFIG_FILE_NAME)
        .unwrap_or_else(|| crate::configio::config_dir().join(CONFIG_FILE_NAME))
}

/// Selection groups to start with, plus a message if something went wrong. Falls back to
/// the `.bak` backup and then to `config/defaults/`, and always includes the protected
/// role filters.
pub fn load_startup() -> (Vec<SelectionGroup>, Option<String>) {
    let main = groups::find_config(CONFIG_FILE_NAME);
    let (mut groups, message) =
        match crate::configio::load_with_fallback(main, CONFIG_FILE_NAME, load_file) {
            Ok((groups, _, notice)) => (groups, notice.map(|n| format!("Selection groups: {n}"))),
            Err(e) if e.is_empty() => (Vec::new(), None),
            Err(e) => (
                Vec::new(),
                Some(format!("Could not load selection groups: {e}.")),
            ),
        };
    ensure_protected(&mut groups);
    (groups, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[u8]) -> ValueSet {
        let mut s = ValueSet::default();
        for &v in values {
            s.insert(v);
        }
        s
    }

    #[test]
    fn parses_champions_and_group_values() {
        let groups = parse_config(
            r#"{ "groups": [
                { "name": "A", "champions": [16, 17, 3, 190] },
                { "name": "A", "champions": [17, 17], "note": "same name is fine" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].members, set(&[16, 17, 3, 190]));
        assert_eq!(groups[1].members, set(&[17]));
    }

    #[test]
    fn rejects_bad_configs() {
        let bad = [
            r#"{ "groups": [ { "name": "A", "champions": [0] } ] }"#,
            r#"{ "groups": [ { "name": "A", "champions": [1] } ] }"#,
            r#"{ "groups": [ { "name": "A", "champions": [189] } ] }"#,
            r#"{ "groups": [ { "name": "A", "champions": [300] } ] }"#,
            r#"{ "groups": [ { "name": "", "champions": [16] } ] }"#,
            r#"{ "groups": [ { "name": "A", "value": 1, "champions": [16] } ] }"#,
            "{",
        ];
        for text in bad {
            assert!(parse_config(text).is_err(), "should reject: {text}");
        }
    }

    #[test]
    fn protected_names_are_exact_but_trimmed() {
        assert!(is_protected_name("Top"));
        assert!(is_protected_name(" Support "));
        assert!(!is_protected_name("TOP"));
        assert!(!is_protected_name("Tops"));
        let mut groups =
            parse_config(r#"{ "groups": [ { "name": "Mid", "champions": [16] } ] }"#).unwrap();
        ensure_protected(&mut groups);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Mid", "Top", "Jungle", "ADC", "Support"]);
        assert!(groups[1].members.is_empty());
    }

    #[test]
    fn config_json_round_trips() {
        let groups = parse_config(
            r#"{ "groups": [ { "name": "A", "champions": [16, 3, 250], "note": "n" },
                             { "name": "A", "champions": [] } ] }"#,
        )
        .unwrap();
        assert_eq!(parse_config(&to_config_json(&groups)).unwrap(), groups);
    }

    #[test]
    fn combines_by_union_or_intersection() {
        let groups = [
            SelectionGroup {
                name: "A".into(),
                note: String::new(),
                members: set(&[16, 17, 3]),
            },
            SelectionGroup {
                name: "B".into(),
                note: String::new(),
                members: set(&[17, 18]),
            },
            SelectionGroup {
                name: "C".into(),
                note: String::new(),
                members: set(&[40]),
            },
        ];
        assert_eq!(combined(&groups, &[false, false, false], false), None);
        assert_eq!(
            combined(&groups, &[true, false, false], true),
            Some(set(&[16, 17, 3]))
        );
        assert_eq!(
            combined(&groups, &[true, true, false], false),
            Some(set(&[16, 17, 18, 3]))
        );
        assert_eq!(
            combined(&groups, &[true, true, false], true),
            Some(set(&[17]))
        );
        assert_eq!(
            combined(&groups, &[true, false, true], true),
            Some(set(&[]))
        );
    }

    #[test]
    fn default_config_loads() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(groups::CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME);
        let groups = parse_config(&std::fs::read_to_string(path).unwrap()).unwrap();
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Top", "Jungle", "Mid", "ADC", "Support"]);
    }
}
