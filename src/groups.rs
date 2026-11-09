//! Representational groups: one node value standing for a set of champions.
//!
//! Value ranges:
//! - `2..=15`: preset groups, defined in code below.
//! - `190..=255`: user groups, loaded from `config/reprgroups.json` (or another file picked
//!   in the GUI).
//!
//! # Config format
//!
//! ```json
//! {
//!   "groups": [
//!     { "value": 190, "name": "Enchanters", "champions": [17, 60, 138], "note": "..." }
//!   ]
//! }
//! ```
//!
//! `champions` holds champion node values (see `champions.rs`). Any problem with the file
//! rejects the whole file, so either every user group loads or none does.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::champions::{self, CHAMPIONS};

pub const PRESET_FIRST: u8 = 2;
pub const PRESET_LAST: u8 = 15;
pub const USER_FIRST: u8 = 190;
/// Fewest champions a user group may have in the editor.
pub const MIN_RGROUP_CHAMPIONS: usize = 2;

pub const CONFIG_DIR_NAME: &str = "config";
pub const CONFIG_FILE_NAME: &str = "reprgroups.json";

const MAX_NAME_CHARS: usize = 16;
const MAX_NOTE_CHARS: usize = 200;

pub fn is_preset(value: u8) -> bool {
    (PRESET_FIRST..=PRESET_LAST).contains(&value)
}

pub fn is_user_group(value: u8) -> bool {
    value >= USER_FIRST
}

pub fn is_group(value: u8) -> bool {
    is_preset(value) || is_user_group(value)
}

/// The group values among `values` that have no colour in `colors` yet, in value order.
/// Runs on every tree edit, so it only ticks off the values seen in a table of all 256
/// rather than collecting and sorting them, and looks nothing up per node.
pub fn uncolored(
    values: impl Iterator<Item = u8>,
    colors: &std::collections::HashMap<u8, usize>,
) -> Vec<u8> {
    let mut seen = [false; 256];
    for v in values {
        seen[usize::from(v)] = true;
    }
    (0..=255)
        .filter(|&v| seen[usize::from(v)] && is_group(v) && !colors.contains_key(&v))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub value: u8,
    /// Short text shown on the tile, e.g. `TOP` or `G3`.
    pub label: String,
    pub name: String,
    pub note: String,
    /// Champion values, sorted.
    pub members: Vec<u8>,
    /// `members` as a 256-bit set for constant-time lookups.
    member_bits: [u64; 4],
    /// Role lock (see `roles.rs`): group nodes can only be tagged with these roles, and new
    /// group nodes start with them. `0` = no lock.
    pub roles: u8,
}

impl Group {
    fn new(value: u8, label: String, name: String, note: String, mut members: Vec<u8>) -> Self {
        members.sort_unstable();
        members.dedup();
        let mut member_bits = [0u64; 4];
        for &m in &members {
            member_bits[usize::from(m / 64)] |= 1 << (m % 64);
        }
        Group {
            value,
            label,
            name,
            note,
            members,
            member_bits,
            roles: 0,
        }
    }

    /// The same group with the role lock `roles` (`0` = none).
    pub fn with_roles(mut self, roles: u8) -> Self {
        self.roles = roles & crate::roles::ALL;
        self
    }

    /// A user group (value 190-255), labelled `G1` ... `G66`.
    pub fn new_user(value: u8, name: String, note: String, members: Vec<u8>) -> Self {
        let label = format!("G{}", value - USER_FIRST + 1);
        Group::new(value, label, name, note, members)
    }

    /// A user group from outside data (a config file or a save file), checked: value
    /// 190-255, a name of 1-16 characters, a note of at most 200, at least one champion,
    /// each a real champion and listed once. Errors say what is wrong, without context.
    pub fn checked_user(
        value: i64,
        name: String,
        note: String,
        champions: &[i64],
        roles: u8,
    ) -> Result<Group, String> {
        let value = u8::try_from(value)
            .ok()
            .filter(|&v| is_user_group(v))
            .ok_or_else(|| format!("value {value} is outside {USER_FIRST}-255"))?;
        let name_chars = name.chars().count();
        if name_chars == 0 || name_chars > MAX_NAME_CHARS {
            return Err(format!("name must be 1-{MAX_NAME_CHARS} characters"));
        }
        if note.chars().count() > MAX_NOTE_CHARS {
            return Err(format!("note is longer than {MAX_NOTE_CHARS} characters"));
        }
        if champions.is_empty() {
            return Err("has no champions".into());
        }
        let mut members = Vec::with_capacity(champions.len());
        let mut seen = [false; 256];
        for &c in champions {
            let champion = u8::try_from(c)
                .ok()
                .filter(|&v| champions::by_value(v).is_some())
                .ok_or_else(|| format!("{c} is not a champion value"))?;
            if std::mem::replace(&mut seen[usize::from(champion)], true) {
                return Err(format!("champion {c} is listed twice"));
            }
            members.push(champion);
        }
        Ok(Group::new_user(value, name, note, members).with_roles(roles))
    }

    pub fn contains(&self, champion: u8) -> bool {
        self.member_bits[usize::from(champion / 64)] & (1 << (champion % 64)) != 0
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Member names, for tooltips.
    pub fn member_names(&self) -> Vec<&'static str> {
        self.members
            .iter()
            .filter_map(|&m| champions::by_value(m).map(|c| c.name))
            .collect()
    }
}

/// Preset definitions: value, tile label, name, member icon keys (`None` = every champion).
const PRESETS: [(u8, &str, &str, Option<&[&str]>); 14] = [
    (2, "ANY", "Any champion", None),
    (
        3,
        "TOP",
        "Top",
        Some(&[
            "Jax",
            "Kayle",
            "Ryze",
            "Sion",
            "Teemo",
            "Singed",
            "Tryndamere",
            "Chogath",
            "Gangplank",
            "DrMundo",
            "Malphite",
            "Nasus",
            "Heimerdinger",
            "Udyr",
            "Poppy",
            "Gragas",
            "Pantheon",
            "Mordekaiser",
            "Shen",
            "Kennen",
            "Garen",
            "Akali",
            "Olaf",
            "Vladimir",
            "Urgot",
            "Irelia",
            "Trundle",
            "Renekton",
            "Rumble",
            "Vayne",
            "Yorick",
            "MonkeyKing",
            "Skarner",
            "Riven",
            "Volibear",
            "Fiora",
            "Darius",
            "Jayce",
            "Quinn",
            "Zac",
            "Aatrox",
            "Yasuo",
            "Gnar",
            "TahmKench",
            "Illaoi",
            "Kled",
            "Camille",
            "Ornn",
            "Sylas",
            "Sett",
            "Yone",
            "Gwen",
            "KSante",
            "Aurora",
            "Ambessa",
            "Zaahen",
        ]),
    ),
    (
        4,
        "JGL",
        "Jungle",
        Some(&[
            "Fiddlesticks",
            "Jax",
            "MasterYi",
            "Nunu",
            "Warwick",
            "Evelynn",
            "Karthus",
            "Amumu",
            "Rammus",
            "Shaco",
            "Udyr",
            "Nidalee",
            "Poppy",
            "Gragas",
            "Pantheon",
            "Olaf",
            "XinZhao",
            "Trundle",
            "Maokai",
            "JarvanIV",
            "Nocturne",
            "LeeSin",
            "Brand",
            "MonkeyKing",
            "Skarner",
            "Talon",
            "Graves",
            "Shyvana",
            "Volibear",
            "Sejuani",
            "Hecarim",
            "Zyra",
            "Diana",
            "Rengar",
            "Khazix",
            "Elise",
            "Vi",
            "Zac",
            "RekSai",
            "Ekko",
            "Kindred",
            "Taliyah",
            "Ivern",
            "Kayn",
            "Sylas",
            "Qiyana",
            "Lillia",
            "Viego",
            "Belveth",
            "Naafiri",
            "Briar",
            "Zaahen",
        ]),
    ),
    (
        5,
        "MID",
        "Mid",
        Some(&[
            "Annie",
            "Ryze",
            "Tristana",
            "TwistedFate",
            "Anivia",
            "Veigar",
            "Kassadin",
            "Corki",
            "Katarina",
            "Gragas",
            "Pantheon",
            "Akali",
            "Malzahar",
            "Vladimir",
            "Galio",
            "Swain",
            "Lux",
            "Leblanc",
            "Irelia",
            "Cassiopeia",
            "Orianna",
            "Talon",
            "Xerath",
            "Fizz",
            "Ahri",
            "Viktor",
            "Ziggs",
            "Diana",
            "Syndra",
            "Zed",
            "Lissandra",
            "Lucian",
            "Yasuo",
            "Velkoz",
            "Azir",
            "Ekko",
            "AurelionSol",
            "Taliyah",
            "Zoe",
            "Neeko",
            "Sylas",
            "Qiyana",
            "Yone",
            "Akshan",
            "Vex",
            "Naafiri",
            "Hwei",
            "Smolder",
            "Aurora",
            "Mel",
            "Locke",
        ]),
    ),
    (
        6,
        "ADC",
        "ADC",
        Some(&[
            "Ashe",
            "Sivir",
            "Tristana",
            "Twitch",
            "Corki",
            "Ezreal",
            "KogMaw",
            "MissFortune",
            "Swain",
            "Caitlyn",
            "Vayne",
            "Ziggs",
            "Varus",
            "Draven",
            "Lucian",
            "Jinx",
            "Yasuo",
            "Kalista",
            "Jhin",
            "Xayah",
            "Kaisa",
            "Senna",
            "Aphelios",
            "Samira",
            "Seraphine",
            "Zeri",
            "Nilah",
            "Smolder",
            "Yunara",
        ]),
    ),
    (
        7,
        "SUP",
        "Support",
        Some(&[
            "Alistar",
            "Morgana",
            "Soraka",
            "Zilean",
            "Amumu",
            "Taric",
            "Blitzcrank",
            "Janna",
            "Poppy",
            "Pantheon",
            "Sona",
            "Swain",
            "Lux",
            "Karma",
            "Maokai",
            "Brand",
            "Leona",
            "Xerath",
            "Nautilus",
            "Lulu",
            "Zyra",
            "Nami",
            "Thresh",
            "Velkoz",
            "Braum",
            "Bard",
            "TahmKench",
            "Camille",
            "Rakan",
            "Pyke",
            "Neeko",
            "Yuumi",
            "Senna",
            "Seraphine",
            "Rell",
            "Renata",
            "Milio",
            "Hwei",
            "Mel",
        ]),
    ),
    (
        8,
        "ASN",
        "Assassin",
        Some(&["Zed", "Talon", "Khazix", "Akali", "Qiyana", "Leblanc"]),
    ),
    (
        9,
        "MAG",
        "Mage",
        Some(&["Syndra", "Orianna", "Viktor", "Xerath", "Lux", "Hwei"]),
    ),
    (
        10,
        "BRU",
        "Bruiser",
        Some(&["Darius", "Aatrox", "Sett", "Renekton", "Jax", "Irelia"]),
    ),
    (
        11,
        "TNK",
        "Tank",
        Some(&["Ornn", "Sejuani", "Maokai", "Malphite", "Zac", "KSante"]),
    ),
    (
        12,
        "MRK",
        "Marksman",
        Some(&["Jinx", "Caitlyn", "KogMaw", "Varus", "Ashe", "Zeri"]),
    ),
    (
        13,
        "ENC",
        "Enchanter",
        Some(&["Lulu", "Janna", "Nami", "Yuumi", "Soraka", "Milio"]),
    ),
    (14, "P14", "Placeholder 14", Some(&["Alistar", "Annie"])),
    (15, "P15", "Placeholder 15", Some(&["Jinx", "Lulu"])),
];

fn champion_value_by_key(key: &str) -> u8 {
    let index = CHAMPIONS
        .iter()
        .position(|c| c.key == key)
        .unwrap_or_else(|| panic!("preset uses unknown champion key {key}"));
    champions::value_of(index)
}

fn presets() -> &'static [Group] {
    static PRESET_GROUPS: OnceLock<Vec<Group>> = OnceLock::new();
    PRESET_GROUPS.get_or_init(|| {
        PRESETS
            .iter()
            .map(|&(value, label, name, keys)| {
                let members = match keys {
                    Some(keys) => keys.iter().map(|k| champion_value_by_key(k)).collect(),
                    None => (0..CHAMPIONS.len()).map(champions::value_of).collect(),
                };
                Group::new(value, label.into(), name.into(), String::new(), members)
            })
            .collect()
    })
}

/// All groups the app currently knows: the presets plus the user groups of the loaded
/// config.
#[derive(Clone, Debug)]
pub struct GroupCatalog {
    /// Presets first, then user groups, each in value order.
    groups: Vec<Group>,
    /// Node value -> index into `groups`.
    index: Vec<Option<usize>>,
    /// File the user groups came from.
    source: Option<PathBuf>,
}

impl Default for GroupCatalog {
    fn default() -> Self {
        Self::presets_only()
    }
}

impl GroupCatalog {
    pub fn presets_only() -> Self {
        Self::build(Vec::new(), None)
    }

    /// The presets plus `user` groups, which came from (or will be saved to) `source`.
    pub fn with_user_groups(user: Vec<Group>, source: Option<PathBuf>) -> Self {
        Self::build(user, source)
    }

    fn build(mut user: Vec<Group>, source: Option<PathBuf>) -> Self {
        user.sort_by_key(|g| g.value);
        let groups: Vec<Group> = presets().iter().cloned().chain(user).collect();
        let mut index = vec![None; 256];
        for (i, g) in groups.iter().enumerate() {
            index[usize::from(g.value)] = Some(i);
        }
        GroupCatalog {
            groups,
            index,
            source,
        }
    }

    pub fn get(&self, value: u8) -> Option<&Group> {
        self.index[usize::from(value)].map(|i| &self.groups[i])
    }

    #[cfg(test)]
    pub fn presets(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| is_preset(g.value))
    }

    pub fn user_groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| is_user_group(g.value))
    }

    pub fn all(&self) -> &[Group] {
        &self.groups
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
}

/// Hash of group definitions: values and members, in value order. Names, notes, labels and
/// role locks don't count. Save files store it next to the definitions as a check.
pub fn hash_groups<'a>(groups: impl IntoIterator<Item = &'a Group>) -> u64 {
    let mut sorted: Vec<&Group> = groups.into_iter().collect();
    sorted.sort_by_key(|g| g.value);
    let mut hash = Fnv1a::new();
    for group in sorted {
        hash.write(&[group.value, group.members.len() as u8]);
        hash.write(&group.members);
    }
    hash.finish()
}

/// 64-bit FNV-1a. Written out here so the hash can never change with a library or Rust
/// version, which would break saved files.
struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Fnv1a(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    groups: Vec<GroupDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupDef {
    // Wide integer types so out-of-range numbers get our own clear message.
    value: i64,
    name: String,
    champions: Vec<i64>,
    #[serde(default)]
    note: String,
    /// Role lock, e.g. `["top", "jungle"]`.
    #[serde(default)]
    roles: Vec<String>,
}

#[derive(Serialize)]
struct GroupOut<'a> {
    value: u8,
    name: &'a str,
    champions: &'a [u8],
    note: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    roles: Vec<&'static str>,
}

#[derive(Serialize)]
struct ConfigOut<'a> {
    groups: Vec<GroupOut<'a>>,
}

/// The config file contents for `user` groups, in the format [`parse_config`] reads.
pub fn to_config_json(user: &[Group]) -> String {
    let out = ConfigOut {
        groups: user
            .iter()
            .map(|g| GroupOut {
                value: g.value,
                name: &g.name,
                champions: &g.members,
                note: &g.note,
                roles: crate::roles::roles_in(g.roles).map(|r| r.key()).collect(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&out).expect("groups serialize") + "\n"
}

/// Parses and validates a config file's contents.
pub fn parse_config(text: &str, source: Option<PathBuf>) -> Result<GroupCatalog, ConfigError> {
    let file: ConfigFile =
        serde_json::from_str(text).map_err(|e| ConfigError(format!("invalid JSON: {e}")))?;

    let mut seen_values = [false; 256];
    let mut user = Vec::with_capacity(file.groups.len());
    for (i, def) in file.groups.into_iter().enumerate() {
        let which = format!("group {} (\"{}\")", i + 1, def.name);
        let mut roles = 0u8;
        for key in &def.roles {
            let role = crate::roles::Role::from_key(key).ok_or_else(|| {
                ConfigError(format!(
                    "{which}: unknown role \"{key}\" (use top, jungle, mid, bot or support)"
                ))
            })?;
            roles |= role.bit();
        }
        let group = Group::checked_user(def.value, def.name, def.note, &def.champions, roles)
            .map_err(|e| ConfigError(format!("{which}: {e}")))?;
        if std::mem::replace(&mut seen_values[usize::from(group.value)], true) {
            return Err(ConfigError(format!(
                "{which}: value {} is used twice",
                group.value
            )));
        }
        user.push(group);
    }
    Ok(GroupCatalog::build(user, source))
}

pub fn load_config(path: &Path) -> Result<GroupCatalog, ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ConfigError(format!("could not read {}: {e}", path.display())))?;
    parse_config(&text, Some(path.to_path_buf()))
}

/// Finds `config/<file_name>`: next to the working directory, then next to the executable,
/// then in the crate root (for an installed copy, in the per-user config folder).
pub fn find_config(file_name: &str) -> Option<PathBuf> {
    crate::configio::candidate_dirs()
        .into_iter()
        .map(|dir| dir.join(file_name))
        .find(|p| p.is_file())
}

/// Finds `config/reprgroups.json` (see [`find_config`]).
pub fn default_config_path() -> Option<PathBuf> {
    find_config(CONFIG_FILE_NAME)
}

/// The catalog to start with, plus a message if something went wrong. Falls back to the
/// `.bak` backup and then to `config/defaults/` if the config is broken. The catalog's
/// source is always the regular config path, so saving goes back there.
pub fn load_startup() -> (GroupCatalog, Option<String>) {
    let main = default_config_path();
    let target = main
        .clone()
        .unwrap_or_else(|| crate::configio::config_dir().join(CONFIG_FILE_NAME));
    match crate::configio::load_with_fallback(main, CONFIG_FILE_NAME, load_config) {
        Ok((catalog, _, notice)) => {
            let user = catalog.user_groups().cloned().collect();
            (
                GroupCatalog::with_user_groups(user, Some(target)),
                notice.map(|n| format!("Representational groups: {n}")),
            )
        }
        Err(e) if e.is_empty() => (
            GroupCatalog::with_user_groups(Vec::new(), Some(target)),
            None,
        ),
        Err(e) => (
            GroupCatalog::with_user_groups(Vec::new(), Some(target)),
            Some(format!(
                "Could not load representational groups: {e}. User groups are disabled."
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncolored_lists_new_groups_once_in_order() {
        let colors = std::collections::HashMap::from([(3u8, 0usize), (200, 1)]);
        let values = [200, 17, 191, 3, 5, 191, 0, 2, 255, 40, 5];
        assert_eq!(uncolored(values.into_iter(), &colors), [2, 5, 191, 255]);
        assert!(uncolored([17, 3, 200].into_iter(), &colors).is_empty());
    }

    #[test]
    fn presets_resolve() {
        let catalog = GroupCatalog::presets_only();
        assert_eq!(catalog.presets().count(), 14);
        assert_eq!(catalog.get(2).unwrap().len(), CHAMPIONS.len());
        let p14 = catalog.get(14).unwrap();
        assert_eq!(p14.member_names(), ["Alistar", "Annie"]);
        assert!(p14.contains(16) && p14.contains(17) && !p14.contains(18));
        assert_eq!(catalog.get(15).unwrap().member_names().len(), 2);
        assert!(catalog.get(1).is_none() && catalog.get(16).is_none());
    }

    #[test]
    fn parses_a_valid_config() {
        let catalog = parse_config(
            r#"{ "groups": [
                { "value": 191, "name": "B", "champions": [18, 16] },
                { "value": 190, "name": "A", "champions": [20], "note": "hi" }
            ] }"#,
            None,
        )
        .unwrap();
        let groups: Vec<_> = catalog.user_groups().collect();
        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].value, groups[0].label.as_str()), (190, "G1"));
        assert_eq!(groups[1].members, [16, 18]);
        assert_eq!(groups[0].note, "hi");
    }

    #[test]
    fn rejects_bad_configs() {
        let bad = [
            r#"{ "groups": [ { "value": 189, "name": "A", "champions": [16] } ] }"#,
            r#"{ "groups": [ { "value": 300, "name": "A", "champions": [16] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [16] },
                             { "value": 190, "name": "B", "champions": [17] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [189] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [16, 16] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "", "champions": [16] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "Much too long a name", "champions": [16] } ] }"#,
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [16], "extra": 1 } ] }"#,
            r#"{ "groups": [ { "value": 190, "champions": [16] } ] }"#,
            "not json",
        ];
        for text in bad {
            assert!(parse_config(text, None).is_err(), "should reject: {text}");
        }
    }

    #[test]
    fn hash_covers_values_and_members_only() {
        let a = parse_config(
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [16, 17] },
                             { "value": 191, "name": "B", "champions": [18] } ] }"#,
            None,
        )
        .unwrap();
        // Renamed, reordered, extra group, different note: same hash for 190.
        let b = parse_config(
            r#"{ "groups": [ { "value": 192, "name": "C", "champions": [19] },
                             { "value": 190, "name": "Renamed", "champions": [17, 16], "note": "x" } ] }"#,
            None,
        )
        .unwrap();
        let hash = |c: &GroupCatalog, values: &[u8]| {
            hash_groups(values.iter().map(|&v| c.get(v).unwrap()))
        };
        assert_eq!(hash(&a, &[190]), hash(&b, &[190]));
        assert_ne!(hash(&a, &[190]), hash(&a, &[190, 191]));
        // Order doesn't matter.
        assert_eq!(hash(&a, &[190, 191]), hash(&a, &[191, 190]));
        // Different members, different hash.
        let c = parse_config(
            r#"{ "groups": [ { "value": 190, "name": "A", "champions": [16, 18] } ] }"#,
            None,
        )
        .unwrap();
        assert_ne!(hash(&a, &[190]), hash(&c, &[190]));
        assert_eq!(hash_groups([]), Fnv1a::new().finish());
    }

    #[test]
    fn config_json_round_trips() {
        let user = vec![
            Group::new_user(190, "A".into(), "note".into(), vec![17, 16]),
            Group::new_user(195, "B".into(), String::new(), vec![40, 41]),
        ];
        let catalog = parse_config(&to_config_json(&user), None).unwrap();
        let parsed: Vec<Group> = catalog.user_groups().cloned().collect();
        assert_eq!(parsed, user);
        assert_eq!(parsed[1].label, "G6");
    }

    #[test]
    fn example_config_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME);
        let catalog = load_config(&path).unwrap();
        assert!(catalog.user_groups().count() > 0);
    }
}
