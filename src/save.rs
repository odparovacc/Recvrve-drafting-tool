//! Saving and loading draft trees.
//!
//! Two file types share one encoding:
//! - `.dts` (compact) stores the bytes below as-is.
//! - `.dtt` (text) stores the same bytes as URL-safe base64, so a tree can be pasted into
//!   chat and saved back into a file.
//!
//! A save is self-contained: it carries the definitions of the user groups its tree uses,
//! so it loads without the config file it was made with (see `group_import.rs` for how
//! they join the groups of the app).
//!
//! # Byte layout (format version 7)
//!
//! ```text
//! "DTRE"          4 bytes  file signature
//! version         1 byte   bits 0-5: format version, currently 7 (5 and 6 also load)
//!                          bit 6: set if the tree uses user groups (values 190-255)
//!                          bit 7: set if a selected node is stored at the end
//! groups          only if bit 6 is set:
//!   g             varint   number of group definitions (at least 1)
//!   hash          8 bytes  little-endian 64-bit FNV-1a hash of the definitions' values and
//!                          members (see `groups::hash_groups`), checked on load
//!   g times, in ascending value order:
//!     value       1 byte   190-255
//!     flags       1 byte   bits 0-1: member encoding (0 list, 1 bitmap, 2 complement),
//!                          bits 2-6: role lock (top, jungle, mid, bot, support)
//!     name        1 byte length, then that many bytes of UTF-8 (1-16 characters)
//!     members     list: count byte (1+), then the champion values ascending;
//!                 bitmap: 22 bytes, bit i (most significant first) = champion value 16 + i;
//!                 complement: count byte, then the champions *not* in the group ascending.
//!                 The encoder picks whichever is shortest.
//! fearless        1 byte   bits 0-2: game of the series (1-7), bit 3: soft fearless,
//!                          bits 4-6: number of stored earlier-game rows r (0-6)
//!                 10r bytes  each row: the champions picked in that game, five by the
//!                          team on blue side in the current game, then five by the
//!                          red-side team; 0 = empty slot (see `fearless.rs`)
//! n               varint   number of nodes, root excluded (LEB128: 7 bits per byte)
//! values          n bytes  node values in pre-order (depth-first, children in order)
//! shape           2n bits  1 = step down into the next child, 0 = step back up,
//!                          packed most significant bit first, zero-padded to a byte
//! tags            varint   number of nodes with role tags, then for each: its pre-order
//!                          index (varint, root = 0) and its tag byte (bits 0-4: top,
//!                          jungle, mid, bot, support)
//! hidden          varint   (version 6+) number of nodes whose branches are hidden,
//!                          then their pre-order indexes (varint, ascending, root = 0);
//!                          each has children
//! marked          varint   (version 7) number of marked nodes (ends of marked lines),
//!                          then their pre-order indexes (varint, ascending, not the root)
//! names           (version 7) blue team name, red team name and patch: each a varint
//!                          byte length then UTF-8 (empty = not set; at most 24, 24 and 16
//!                          characters)
//! selected        varint   only if bit 7 is set: the selected node's pre-order index,
//!                          counting the root as 0
//! ```
//!
//! Only the groups the tree uses are stored, without their notes. Node values refer to
//! them as they do in the app. Positions follow from depth, and parent/child ids are
//! rebuilt on load.
//!
//! Loading rebuilds the tree through `DraftTree::load_child`, which checks what every tree
//! must satisfy (draft length, `NO_BAN` only at bans, known values, champions and groups
//! unique among siblings) but not the line rules: a tree may hold empty siblings and
//! conflicting branches (see `DraftTree::conflicts`), and they load as they were saved.
//! Role tags are still checked against every line. The encoding is canonical: decoding
//! and encoding again gives the same bytes. Older format versions don't load.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use std::sync::Arc;

use crate::champions;
use crate::fearless::Fearless;
use crate::groups::{self, Group, GroupCatalog};
use crate::tree::{DraftTree, MAX_NAME_CHARS, MAX_NODES, MAX_PATCH_CHARS, NodeId, TreeError};

const MAGIC: &[u8; 4] = b"DTRE";
const VERSION: u8 = 7;
/// Version 6: without the marks and names.
const VERSION_6: u8 = 6;
/// Version 5: without the hidden branches either.
const VERSION_5: u8 = 5;
const VERSION_MASK: u8 = 0x3F;
/// Member encodings of a stored group.
const MEMBERS_LIST: u8 = 0;
const MEMBERS_BITMAP: u8 = 1;
const MEMBERS_COMPLEMENT: u8 = 2;
const CHAMPION_COUNT: usize = champions::CHAMPIONS.len();
const BITMAP_BYTES: usize = CHAMPION_COUNT.div_ceil(8);
const HAS_GROUPS: u8 = 0x40;
const HAS_SELECTION: u8 = 0x80;

pub const SAVES_DIR_NAME: &str = "saves";

/// The default folder for save files (see [`crate::configio::app_folder`]), created if
/// needed so file dialogs can open in it.
pub fn saves_dir() -> PathBuf {
    let dir = crate::configio::app_folder(SAVES_DIR_NAME);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A decoded save file.
pub struct Loaded {
    /// The tree. Its group catalog holds the presets plus the user groups stored in the
    /// file (without notes and without a source file), not the app's config.
    pub tree: DraftTree,
    /// The node that was selected when saving, if the file stores one.
    pub selected: Option<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// `.dts`: raw bytes.
    Compact,
    /// `.dtt`: base64 text.
    Text,
}

impl SaveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Compact => "dts",
            SaveFormat::Text => "dtt",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SaveFormat::Compact => "Compact (.dts)",
            SaveFormat::Text => "Text (.dtt)",
        }
    }

    /// Format for a file, judged by its extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<SaveFormat> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "dts" => Some(SaveFormat::Compact),
            "dtt" => Some(SaveFormat::Text),
            _ => None,
        }
    }

    /// `path` with this format's extension, appended if it has a different one.
    pub fn apply_extension(self, path: PathBuf) -> PathBuf {
        if SaveFormat::from_path(&path) == Some(self) {
            return path;
        }
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(self.extension());
        path.with_file_name(name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    InvalidFileType,
    Io(String),
    NotASaveFile,
    UnsupportedVersion(u8),
    /// The file ends early, has extra bytes, or its shape bits don't describe a tree.
    Corrupt(&'static str),
    /// The data describes a tree that breaks a draft rule.
    InvalidTree(TreeError),
    /// A stored group definition is invalid.
    InvalidGroup(String),
    /// The tree has more nodes than [`MAX_NODES`] (root excluded): how many.
    TooManyNodes(usize),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::InvalidFileType => f.write_str("Invalid save file type."),
            LoadError::Io(e) => write!(f, "Could not read the file: {e}"),
            LoadError::NotASaveFile => f.write_str("This is not a Recvrve save file."),
            LoadError::UnsupportedVersion(v) => {
                write!(f, "Save file version {v} is not supported.")
            }
            LoadError::Corrupt(why) => write!(f, "The save file is damaged: {why}."),
            LoadError::InvalidTree(e) => write!(f, "The save file contains an invalid draft: {e}."),
            LoadError::InvalidGroup(why) => {
                write!(f, "The save file contains an invalid group: {why}.")
            }
            LoadError::TooManyNodes(n) => write!(
                f,
                "This tree has {} nodes; Recvrve opens trees with up to {}.",
                thousands(*n),
                thousands(MAX_NODES)
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// `n` with commas between groups of three digits: 250,000.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Where the encoder writes: a byte vector for files, or a hasher for
/// [`content_hash`]. Both see exactly the same bytes in the same order.
trait Sink {
    fn put(&mut self, bytes: &[u8]);

    fn byte(&mut self, b: u8) {
        self.put(&[b]);
    }
}

impl Sink for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }

    fn byte(&mut self, b: u8) {
        self.push(b);
    }
}

/// Feeds the bytes to a hasher. `Hasher::write` is a byte stream for `DefaultHasher`
/// (SipHash): how the bytes are split into calls doesn't change the hash.
struct HashSink(std::collections::hash_map::DefaultHasher);

impl Sink for HashSink {
    fn put(&mut self, bytes: &[u8]) {
        std::hash::Hasher::write(&mut self.0, bytes);
    }
}

/// Encodes the whole tree in the compact byte format, plus `selected` if given.
pub fn encode(tree: &DraftTree, selected: Option<NodeId>) -> Vec<u8> {
    let node_count = tree.len() - 1;
    let mut out = Vec::with_capacity(32 + node_count + (node_count * 2).div_ceil(8));
    encode_into(tree, selected, &mut out);
    out
}

/// A hash of [`encode`]`(tree, None)`, without building the bytes: equal encodings
/// always hash the same, and different ones differ unless 64-bit hashes collide. The tabs
/// compare it to tell unsaved changes. Stable within a run of the app, not across runs.
pub fn content_hash(tree: &DraftTree) -> u64 {
    let mut sink = HashSink(std::collections::hash_map::DefaultHasher::new());
    encode_into(tree, None, &mut sink);
    std::hash::Hasher::finish(&sink.0)
}

/// [`encode`] into any [`Sink`]. Values go straight to the sink during the walk; only the
/// shape bits and the (usually short) tag, hidden and marked lists are held until their
/// place in the layout comes.
fn encode_into(tree: &DraftTree, selected: Option<NodeId>, out: &mut impl Sink) {
    let node_count = tree.len() - 1;

    // The user groups the tree uses, in value order. The root holds EMPTY, never a group.
    let mut used = [false; 256];
    for v in tree.values() {
        used[usize::from(v)] = true;
    }
    let used_groups: Vec<&Group> = tree
        .groups()
        .user_groups()
        .filter(|g| used[usize::from(g.value)])
        .collect();

    // Pre-order index of `selected`, found before the header that flags it.
    let selected_index = match selected {
        Some(s) if tree.contains(s) => preorder_index(tree, s),
        _ => None,
    };

    out.put(MAGIC);
    let mut version_byte = VERSION;
    if selected_index.is_some() {
        version_byte |= HAS_SELECTION;
    }
    if !used_groups.is_empty() {
        version_byte |= HAS_GROUPS;
    }
    out.byte(version_byte);
    let mut head = Vec::new();
    if !used_groups.is_empty() {
        write_varint(&mut head, used_groups.len() as u32);
        head.extend_from_slice(&groups::hash_groups(used_groups.iter().copied()).to_le_bytes());
        for group in &used_groups {
            write_group(&mut head, group);
        }
    }
    tree.fearless().encode(&mut head);
    write_varint(&mut head, node_count as u32);
    out.put(&head);

    let mut shape = BitWriter::with_capacity(node_count * 2);
    // (pre-order index, tags) of tagged nodes.
    let mut tags: Vec<(usize, u8)> = Vec::new();
    // Pre-order indexes of nodes with hidden branches, and of marked nodes.
    let mut hidden: Vec<usize> = Vec::new();
    let mut marked: Vec<usize> = Vec::new();
    if tree.get(tree.root()).is_some_and(|n| n.is_collapsed()) {
        hidden.push(0);
    }
    // Iterative pre-order walk. Each stack entry is a node and the index of the next child
    // to visit. `index` counts the nodes visited, the root being 0.
    let mut index = 0;
    let mut stack: Vec<(NodeId, usize)> = vec![(tree.root(), 0)];
    while let Some((id, next)) = stack.last_mut() {
        let children = tree.get(*id).expect("live node").children();
        if let Some(&child) = children.get(*next) {
            *next += 1;
            shape.push(true);
            let node = tree.get(child).expect("live node");
            index += 1;
            out.byte(node.value());
            if node.roles() != 0 {
                tags.push((index, node.roles()));
            }
            if node.is_collapsed() {
                hidden.push(index);
            }
            if node.is_marked() {
                marked.push(index);
            }
            stack.push((child, 0));
        } else {
            stack.pop();
            // The root has no matching "down" bit, so it gets no "up" bit either.
            if !stack.is_empty() {
                shape.push(false);
            }
        }
    }
    out.put(&shape.bytes);

    let mut tail = Vec::new();
    write_varint(&mut tail, tags.len() as u32);
    for (index, mask) in tags {
        write_varint(&mut tail, index as u32);
        tail.push(mask);
    }
    for list in [hidden, marked] {
        write_varint(&mut tail, list.len() as u32);
        for index in list {
            write_varint(&mut tail, index as u32);
        }
    }
    let [blue, red] = tree.team_names();
    for text in [blue.as_str(), red.as_str(), tree.patch()] {
        write_varint(&mut tail, text.len() as u32);
        tail.extend_from_slice(text.as_bytes());
    }
    if let Some(index) = selected_index {
        write_varint(&mut tail, index as u32);
    }
    out.put(&tail);
}

/// `id`'s pre-order index (root = 0): the nodes before it are its ancestors, and every
/// earlier sibling's subtree on the way down.
fn preorder_index(tree: &DraftTree, id: NodeId) -> Option<usize> {
    let mut index = 0;
    let mut at = id;
    while let Some(parent) = tree.get(at)?.parent() {
        let siblings = tree.get(parent)?.children();
        let pos = siblings.iter().position(|&c| c == at)?;
        index += 1 + siblings[..pos]
            .iter()
            .map(|&c| 1 + tree.subtree_size(c))
            .sum::<usize>();
        at = parent;
    }
    Some(index)
}

/// Writes one group definition (see the layout at the top of this file).
fn write_group(out: &mut Vec<u8>, group: &Group) {
    out.push(group.value);
    let k = group.members.len();
    let list = 1 + k;
    let complement = 1 + (CHAMPION_COUNT - k);
    let mode = if list <= BITMAP_BYTES && list <= complement {
        MEMBERS_LIST
    } else if BITMAP_BYTES <= complement {
        MEMBERS_BITMAP
    } else {
        MEMBERS_COMPLEMENT
    };
    out.push(mode | (group.roles << 2));
    let name = group.name.as_bytes();
    out.push(name.len() as u8);
    out.extend_from_slice(name);
    match mode {
        MEMBERS_LIST => {
            out.push(k as u8);
            out.extend_from_slice(&group.members);
        }
        MEMBERS_BITMAP => {
            let mut bits = [0u8; BITMAP_BYTES];
            for &m in &group.members {
                let i = usize::from(m - champions::FIRST_CHAMPION_VALUE);
                bits[i / 8] |= 0x80 >> (i % 8);
            }
            out.extend_from_slice(&bits);
        }
        _ => {
            let missing: Vec<u8> = (0..CHAMPION_COUNT)
                .map(champions::value_of)
                .filter(|&v| !group.contains(v))
                .collect();
            out.push(missing.len() as u8);
            out.extend_from_slice(&missing);
        }
    }
}

fn take(bytes: &[u8], n: usize) -> Result<(&[u8], &[u8]), LoadError> {
    if bytes.len() < n {
        return Err(LoadError::Corrupt("unexpected file length"));
    }
    Ok(bytes.split_at(n))
}

/// Reads `count` champion values that must be strictly ascending.
fn read_champions(bytes: &[u8], count: usize) -> Result<(Vec<u8>, &[u8]), LoadError> {
    let (list, rest) = take(bytes, count)?;
    if list.windows(2).any(|w| w[0] >= w[1]) {
        return Err(LoadError::Corrupt("group members out of order"));
    }
    if let Some(&bad) = list.iter().find(|&&v| champions::by_value(v).is_none()) {
        return Err(LoadError::InvalidGroup(format!(
            "{bad} is not a champion value"
        )));
    }
    Ok((list.to_vec(), rest))
}

/// Reads one group definition written by [`write_group`].
fn read_group(bytes: &[u8]) -> Result<(Group, &[u8]), LoadError> {
    let (head, rest) = take(bytes, 3)?;
    let (value, flags, name_len) = (head[0], head[1], usize::from(head[2]));
    let mode = flags & 0b11;
    let roles = flags >> 2;
    if roles & !crate::roles::ALL != 0 {
        return Err(LoadError::Corrupt("unknown role bits"));
    }
    let (name, rest) = take(rest, name_len)?;
    let name = std::str::from_utf8(name)
        .map_err(|_| LoadError::Corrupt("group name is not UTF-8"))?
        .to_string();
    let (members, rest) = match mode {
        MEMBERS_LIST => {
            let (&count, rest) = rest
                .split_first()
                .ok_or(LoadError::Corrupt("unexpected file length"))?;
            read_champions(rest, usize::from(count))?
        }
        MEMBERS_BITMAP => {
            let (bits, rest) = take(rest, BITMAP_BYTES)?;
            let members: Vec<u8> = (0..BITMAP_BYTES * 8)
                .filter(|&i| bits[i / 8] & (0x80 >> (i % 8)) != 0)
                .map(|i| i as u8 + champions::FIRST_CHAMPION_VALUE)
                .collect();
            if members
                .last()
                .is_some_and(|&v| champions::by_value(v).is_none())
            {
                return Err(LoadError::Corrupt("unexpected padding"));
            }
            (members, rest)
        }
        MEMBERS_COMPLEMENT => {
            let (&count, rest) = rest
                .split_first()
                .ok_or(LoadError::Corrupt("unexpected file length"))?;
            let (missing, rest) = read_champions(rest, usize::from(count))?;
            let members = (0..CHAMPION_COUNT)
                .map(champions::value_of)
                .filter(|v| !missing.contains(v))
                .collect();
            (members, rest)
        }
        _ => return Err(LoadError::Corrupt("unknown member encoding")),
    };
    let wide: Vec<i64> = members.iter().map(|&m| i64::from(m)).collect();
    let group = Group::checked_user(i64::from(value), name, String::new(), &wide, roles)
        .map_err(LoadError::InvalidGroup)?;
    Ok((group, rest))
}

/// Decodes the compact byte format. The tree's group catalog is built from the group
/// definitions in the file (see [`Loaded::tree`]).
pub fn decode(bytes: &[u8]) -> Result<Loaded, LoadError> {
    decode_capped(bytes, MAX_NODES)
}

/// [`decode`] with `max_nodes` in place of [`MAX_NODES`], so tests can check the limit
/// without building a huge tree.
fn decode_capped(bytes: &[u8], max_nodes: usize) -> Result<Loaded, LoadError> {
    let rest = bytes.strip_prefix(MAGIC).ok_or(LoadError::NotASaveFile)?;
    let (&version_byte, rest) = rest.split_first().ok_or(LoadError::NotASaveFile)?;
    let version = version_byte & VERSION_MASK;
    if !(VERSION_5..=VERSION).contains(&version) {
        return Err(LoadError::UnsupportedVersion(version));
    }
    let has_selection = version_byte & HAS_SELECTION != 0;
    let mut stored_groups: Vec<Group> = Vec::new();
    let rest = if version_byte & HAS_GROUPS != 0 {
        let (count, rest) = read_varint(rest)?;
        if count == 0 || count > 66 {
            return Err(LoadError::Corrupt("bad group count"));
        }
        let (hash, mut rest) = rest
            .split_first_chunk::<8>()
            .ok_or(LoadError::Corrupt("unexpected file length"))?;
        for _ in 0..count {
            let (group, tail) = read_group(rest)?;
            if stored_groups.last().is_some_and(|g| g.value >= group.value) {
                return Err(LoadError::Corrupt("groups out of order"));
            }
            stored_groups.push(group);
            rest = tail;
        }
        if groups::hash_groups(&stored_groups) != u64::from_le_bytes(*hash) {
            return Err(LoadError::Corrupt(
                "the group definitions don't match their checksum",
            ));
        }
        rest
    } else {
        rest
    };
    let (fearless, rest) = Fearless::decode(rest).map_err(LoadError::Corrupt)?;
    let (node_count, rest) = read_varint(rest)?;
    let node_count = node_count as usize;
    // Before anything is sized by it.
    if node_count > max_nodes {
        return Err(LoadError::TooManyNodes(node_count));
    }

    let shape_len = (node_count * 2).div_ceil(8);
    let body_len = node_count + shape_len;
    if rest.len() < body_len {
        return Err(LoadError::Corrupt("unexpected file length"));
    }
    let (values, rest) = rest.split_at(node_count);
    let (shape, trailer) = rest.split_at(shape_len);

    // Every stored group is used, and every user group used is stored.
    for group in &stored_groups {
        if !values.contains(&group.value) {
            return Err(LoadError::Corrupt("a stored group is not used"));
        }
    }
    let catalog = Arc::new(GroupCatalog::with_user_groups(stored_groups, None));
    if values
        .iter()
        .any(|&v| groups::is_user_group(v) && catalog.get(v).is_none())
    {
        return Err(LoadError::Corrupt("a group is used but not stored"));
    }

    let mut tree = DraftTree::with_groups(catalog);
    let mut stack = vec![tree.root()];
    // Every node in pre-order, so a stored selection index can be turned back into an id.
    let mut order = Vec::with_capacity(node_count + 1);
    order.push(tree.root());
    let mut values = values.iter().copied();
    for i in 0..node_count * 2 {
        let down = shape[i / 8] & (0x80 >> (i % 8)) != 0;
        if down {
            let value = values.next().ok_or(LoadError::Corrupt("too many nodes"))?;
            let parent = *stack.last().expect("stack keeps the root");
            let child = tree
                .load_child(parent, value)
                .map_err(LoadError::InvalidTree)?;
            order.push(child);
            stack.push(child);
        } else {
            if stack.len() == 1 {
                return Err(LoadError::Corrupt("shape leaves the root"));
            }
            stack.pop();
        }
    }
    if stack.len() != 1 || values.next().is_some() {
        return Err(LoadError::Corrupt("shape does not match node count"));
    }
    // Padding bits after the shape must be zero.
    let bits_in_last_byte = (node_count * 2) % 8;
    if bits_in_last_byte != 0 && shape[shape_len - 1] & (0xFF >> bits_in_last_byte) != 0 {
        return Err(LoadError::Corrupt("unexpected padding"));
    }

    // Role tags, in pre-order so every line is checked as it fills up.
    let (count, mut rest) = read_varint(trailer)?;
    let mut last = 0u32;
    for _ in 0..count {
        let (index, tail) = read_varint(rest)?;
        let (&mask, tail) = tail
            .split_first()
            .ok_or(LoadError::Corrupt("unexpected file length"))?;
        if index <= last || mask == 0 {
            return Err(LoadError::Corrupt("role tags out of order"));
        }
        last = index;
        let id = *order
            .get(index as usize)
            .ok_or(LoadError::Corrupt("tagged node out of range"))?;
        tree.set_roles_unlocked(id, mask)
            .map_err(LoadError::InvalidTree)?;
        rest = tail;
    }
    // Hidden branches (version 6+), ascending, each on a node with children.
    if version >= VERSION_6 {
        let (count, mut tail) = read_varint(rest)?;
        let mut last: Option<u32> = None;
        for _ in 0..count {
            let (index, after) = read_varint(tail)?;
            if last.is_some_and(|l| index <= l) {
                return Err(LoadError::Corrupt("hidden branches out of order"));
            }
            last = Some(index);
            let id = *order
                .get(index as usize)
                .ok_or(LoadError::Corrupt("hidden node out of range"))?;
            if !tree.set_collapsed(id, true) {
                return Err(LoadError::Corrupt("hidden node without branches"));
            }
            tail = after;
        }
        rest = tail;
    }
    // Marked lines and names (version 7).
    if version >= VERSION {
        let (count, mut tail) = read_varint(rest)?;
        let mut last = 0u32;
        for _ in 0..count {
            let (index, after) = read_varint(tail)?;
            if index <= last {
                return Err(LoadError::Corrupt("marked nodes out of order"));
            }
            last = index;
            let id = *order
                .get(index as usize)
                .ok_or(LoadError::Corrupt("marked node out of range"))?;
            tree.set_marked(id, true);
            tail = after;
        }
        let mut texts = Vec::with_capacity(3);
        for limit in [MAX_NAME_CHARS, MAX_NAME_CHARS, MAX_PATCH_CHARS] {
            let (len, after) = read_varint(tail)?;
            let (bytes, after) = take(after, len as usize)?;
            let text = std::str::from_utf8(bytes)
                .map_err(|_| LoadError::Corrupt("a name is not UTF-8"))?;
            if text.chars().count() > limit || text.trim() != text {
                return Err(LoadError::Corrupt("a name is too long"));
            }
            texts.push(text.to_string());
            tail = after;
        }
        tree.set_team_name(0, &texts[0]);
        tree.set_team_name(1, &texts[1]);
        tree.set_patch(&texts[2]);
        rest = tail;
    }
    let trailer = rest;
    if !has_selection && !trailer.is_empty() {
        return Err(LoadError::Corrupt("unexpected file length"));
    }

    let selected = if has_selection {
        let (index, rest) = read_varint(trailer)?;
        if !rest.is_empty() {
            return Err(LoadError::Corrupt("unexpected file length"));
        }
        let id = order
            .get(index as usize)
            .ok_or(LoadError::Corrupt("selected node out of range"))?;
        Some(*id)
    } else {
        None
    };
    tree.set_fearless(fearless);
    Ok(Loaded { tree, selected })
}

/// `Blue-vs-Red` for file names, from the tree's team names ("blue" / "red" for one not
/// set), or `None` when neither is set. Characters file systems don't like become `_`,
/// spaces `-`.
pub fn match_stem(tree: &DraftTree) -> Option<String> {
    let [blue, red] = tree.team_names();
    if blue.is_empty() && red.is_empty() {
        return None;
    }
    let clean = |name: &str, fallback: &str| -> String {
        let name = if name.is_empty() { fallback } else { name };
        name.chars()
            .map(|c| match c {
                ' ' => '-',
                c if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') => c,
                _ => '_',
            })
            .collect()
    };
    Some(format!("{}-vs-{}", clean(blue, "blue"), clean(red, "red")))
}

/// The default file name (without extension) for saving `tree`: `Blue-vs-Red-<date>`
/// when the teams have names, `draft` otherwise.
pub fn default_stem(tree: &DraftTree) -> String {
    match match_stem(tree) {
        Some(stem) => format!("{stem}-{}", chrono::Local::now().format("%Y-%m-%d")),
        None => "draft".to_string(),
    }
}

/// Encodes the tree as a single line of URL-safe base64.
pub fn to_text(tree: &DraftTree, selected: Option<NodeId>) -> String {
    URL_SAFE_NO_PAD.encode(encode(tree, selected))
}

/// Decodes text from [`to_text`]. Whitespace anywhere is ignored, so wrapped or indented
/// pastes still load.
pub fn from_text(text: &str) -> Result<Loaded, LoadError> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = URL_SAFE_NO_PAD
        .decode(compact)
        .map_err(|_| LoadError::NotASaveFile)?;
    decode(&bytes)
}

/// Serialises `tree` in `format`, including `selected` if given.
pub fn to_file_contents(tree: &DraftTree, selected: Option<NodeId>, format: SaveFormat) -> Vec<u8> {
    match format {
        SaveFormat::Compact => encode(tree, selected),
        SaveFormat::Text => {
            let mut text = to_text(tree, selected);
            text.push('\n');
            text.into_bytes()
        }
    }
}

/// Loads a save file, choosing the format from its extension.
pub fn load(path: &Path) -> Result<Loaded, LoadError> {
    let format = SaveFormat::from_path(path).ok_or(LoadError::InvalidFileType)?;
    let bytes = std::fs::read(path).map_err(|e| LoadError::Io(e.to_string()))?;
    match format {
        SaveFormat::Compact => decode(&bytes),
        SaveFormat::Text => {
            let text = std::str::from_utf8(&bytes).map_err(|_| LoadError::NotASaveFile)?;
            from_text(text)
        }
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    len: usize,
}

impl BitWriter {
    fn with_capacity(bits: usize) -> Self {
        BitWriter {
            bytes: Vec::with_capacity(bits.div_ceil(8)),
            len: 0,
        }
    }

    fn push(&mut self, bit: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit {
            *self.bytes.last_mut().unwrap() |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }
}

/// LEB128: 7 bits per byte, low bits first; the high bit says another byte follows.
fn write_varint(out: &mut Vec<u8>, mut n: u32) {
    loop {
        let byte = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(bytes: &[u8]) -> Result<(u32, &[u8]), LoadError> {
    let mut n: u32 = 0;
    for (i, &byte) in bytes.iter().enumerate().take(5) {
        n |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((n, &bytes[i + 1..]));
        }
    }
    Err(LoadError::Corrupt("bad node count"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{EMPTY, NO_BAN};

    fn dec(bytes: &[u8]) -> Result<Loaded, LoadError> {
        decode(bytes)
    }

    /// A file without groups, fearless bans, tags or hidden branches: `n` values and their
    /// shape bytes.
    fn raw(values: &[u8], shape: &[u8]) -> Vec<u8> {
        let mut out = raw_head(values, shape);
        // Nothing hidden or marked, no names.
        out.extend_from_slice(&[0, 0, 0, 0, 0]);
        out
    }

    /// [`raw`] up to and including the (empty) tag list.
    fn raw_head(values: &[u8], shape: &[u8]) -> Vec<u8> {
        let mut out = vec![b'D', b'T', b'R', b'E', VERSION, 1, values.len() as u8];
        out.extend_from_slice(values);
        out.extend_from_slice(shape);
        out.push(0);
        out
    }

    /// A file with the given hidden and marked lists and no names.
    fn raw_lists(values: &[u8], shape: &[u8], hidden: &[u8], marked: &[u8]) -> Vec<u8> {
        let mut out = raw_head(values, shape);
        for list in [hidden, marked] {
            out.push(list.len() as u8);
            out.extend_from_slice(list);
        }
        out.extend_from_slice(&[0, 0, 0]);
        out
    }

    /// Values of every node in pre-order with its depth, to compare trees structurally.
    fn dump(tree: &DraftTree) -> Vec<(usize, u8, u8)> {
        let mut out = Vec::new();
        let mut stack = vec![(tree.root(), 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let node = tree.get(id).unwrap();
            out.push((depth, node.value(), node.pos().bits()));
            for &c in node.children().iter().rev() {
                stack.push((c, depth + 1));
            }
        }
        out
    }

    fn sample_tree() -> DraftTree {
        let mut t = DraftTree::new();
        let r = t.root();
        let a = t.add_child(r, 16).unwrap();
        let b = t.add_child(r, 40).unwrap();
        t.add_child(r, NO_BAN).unwrap();
        let a1 = t.add_child(a, 60).unwrap();
        t.add_child(a, 61).unwrap();
        let mut c = a1;
        for v in 80..90 {
            c = t.add_child(c, v).unwrap();
        }
        t.add_child(c, EMPTY).unwrap();
        t.add_child(b, NO_BAN).unwrap();
        t.add_child(b, 17).unwrap();
        t
    }

    fn group(value: u8, name: &str, members: &[u8], roles: u8) -> Group {
        Group::new_user(value, name.into(), "a note".into(), members.to_vec()).with_roles(roles)
    }

    /// A tree using `groups` (one node each, below the root), plus an unused extra group in
    /// its catalog.
    fn tree_using(groups: &[Group]) -> DraftTree {
        let mut all = groups.to_vec();
        all.push(group(250, "Unused", &[40], 0));
        let mut t = DraftTree::with_groups(Arc::new(GroupCatalog::with_user_groups(all, None)));
        for g in groups {
            t.add_child(t.root(), g.value).unwrap();
        }
        t
    }

    #[test]
    fn round_trips_compact_and_text() {
        let tree = sample_tree();
        let bytes = encode(&tree, None);
        assert_eq!(dump(&dec(&bytes).unwrap().tree), dump(&tree));
        let text = to_text(&tree, None);
        assert!(
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        let back = from_text(&format!("  {text}\n")).unwrap().tree;
        assert_eq!(dump(&back), dump(&tree));
        // Canonical: encoding the decoded tree gives the same bytes.
        assert_eq!(encode(&back, None), bytes);
    }

    #[test]
    fn empty_tree_is_thirteen_bytes() {
        let tree = DraftTree::new();
        // Version 7, no fearless bans (game 1), no nodes, no tags, nothing hidden or
        // marked, three empty names.
        assert_eq!(
            encode(&tree, None),
            b"DTRE\x07\x01\x00\x00\x00\x00\x00\x00\x00"
        );
        assert_eq!(dec(&encode(&tree, None)).unwrap().tree.len(), 1);
    }

    #[test]
    fn size_is_about_ten_bits_per_node() {
        let tree = sample_tree();
        let n = tree.len() - 1;
        assert_eq!(
            encode(&tree, None).len(),
            // Header, fearless byte, values, shape, empty tag, hidden and marked lists,
            // three empty names.
            5 + 1 + 1 + n + (2 * n).div_ceil(8) + 3 + 3
        );
    }

    #[test]
    fn hidden_branches_round_trip() {
        // Root -> 16 -> 17, plus root -> 18 -> 19; hide 16's and the root's branches.
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        t.add_child(a, 17).unwrap();
        let b = t.add_child(t.root(), 18).unwrap();
        t.add_child(b, 19).unwrap();
        assert!(t.set_collapsed(a, true));
        assert!(t.set_collapsed(t.root(), true));
        let bytes = encode(&t, None);
        // ... hidden: 2 nodes, pre-order 0 (root) and 1 (16); nothing marked, no names.
        assert_eq!(&bytes[bytes.len() - 7..], &[2, 0, 1, 0, 0, 0, 0]);
        let back = dec(&bytes).unwrap().tree;
        let hidden: Vec<u8> = dump_hidden(&back);
        assert_eq!(hidden, vec![1, 16]);
        assert_eq!(encode(&back, None), bytes);
        // A node without children can't be stored as hidden.
        assert_eq!(
            dec(&raw_lists(&[16], &[0b1000_0000], &[1], &[])).err(),
            Some(LoadError::Corrupt("hidden node without branches"))
        );
        // Out of order or out of range.
        assert_eq!(
            dec(&raw_lists(&[16, 17], &[0b1100_0000], &[1, 0], &[])).err(),
            Some(LoadError::Corrupt("hidden branches out of order"))
        );
        assert_eq!(
            dec(&raw_lists(&[16], &[0b1000_0000], &[9], &[])).err(),
            Some(LoadError::Corrupt("hidden node out of range"))
        );
    }

    #[test]
    fn file_names_come_from_the_team_names() {
        let mut t = DraftTree::new();
        assert_eq!(match_stem(&t), None);
        assert_eq!(default_stem(&t), "draft");
        t.set_team_name(0, "G2 Esports");
        assert_eq!(match_stem(&t).unwrap(), "G2-Esports-vs-red");
        t.set_team_name(1, "T1/Academy?");
        assert_eq!(match_stem(&t).unwrap(), "G2-Esports-vs-T1_Academy_");
        assert!(default_stem(&t).starts_with("G2-Esports-vs-T1_Academy_-20"));
    }

    #[test]
    fn marks_and_names_round_trip() {
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        let a1 = t.add_child(a, 17).unwrap();
        let b = t.add_child(t.root(), 18).unwrap();
        t.set_marked(a1, true);
        t.set_marked(b, true);
        t.set_team_name(0, " G2 Esports ");
        t.set_team_name(1, "Fnatic");
        t.set_patch("14.20");
        let bytes = encode(&t, None);
        let back = dec(&bytes).unwrap().tree;
        assert_eq!(
            back.team_names(),
            &["G2 Esports".to_string(), "Fnatic".to_string()]
        );
        assert_eq!(back.patch(), "14.20");
        let starred = back.starred();
        assert_eq!(starred.len(), 4, "root, 16, 17 and 18");
        assert_eq!(encode(&back, None), bytes);
        // Marks must be ascending and not the root.
        assert_eq!(
            dec(&raw_lists(&[16, 17], &[0b1100_0000], &[], &[2, 1])).err(),
            Some(LoadError::Corrupt("marked nodes out of order"))
        );
        assert_eq!(
            dec(&raw_lists(&[16], &[0b1000_0000], &[], &[0])).err(),
            Some(LoadError::Corrupt("marked nodes out of order"))
        );
        // Names longer than allowed don't load.
        let mut long = raw_head(&[], &[]);
        long.extend_from_slice(&[0, 0, 25]);
        long.extend_from_slice(&[b'x'; 25]);
        long.extend_from_slice(&[0, 0]);
        assert_eq!(
            dec(&long).err(),
            Some(LoadError::Corrupt("a name is too long"))
        );
    }

    #[test]
    fn version_5_and_6_files_still_load() {
        // Version 6: no marks or names; version 5: no hidden list either.
        let mut v6 = raw_head(&[16, 17], &[0b1100_0000]);
        v6.push(0);
        v6[4] = 6;
        let mut v5 = raw_head(&[16, 17], &[0b1100_0000]);
        v5[4] = 5;
        for old in [v6, v5] {
            let back = dec(&old).unwrap().tree;
            assert_eq!(back.len(), 3);
            assert!(!back.any_collapsed() && !back.any_marked());
            // Saved again, it becomes version 7.
            assert_eq!(encode(&back, None), raw(&[16, 17], &[0b1100_0000]));
        }
    }

    /// Values of the nodes whose branches are hidden (the root as 1), in pre-order.
    fn dump_hidden(tree: &DraftTree) -> Vec<u8> {
        let mut out = Vec::new();
        let mut stack = vec![tree.root()];
        while let Some(id) = stack.pop() {
            let node = tree.get(id).unwrap();
            if node.is_collapsed() {
                out.push(if id == tree.root() { 1 } else { node.value() });
            }
            stack.extend(node.children().iter().rev());
        }
        out
    }

    #[test]
    fn known_layout() {
        // Root -> 16 -> 17, plus root -> 18.
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        t.add_child(a, 17).unwrap();
        t.add_child(t.root(), 18).unwrap();
        // Shape: down down up up down up = 110010, padded to 1100_1000.
        assert_eq!(encode(&t, None), raw(&[16, 17, 18], &[0b1100_1000]));
    }

    #[test]
    fn group_definitions_known_layout() {
        let g = group(190, "Tops", &[16, 17], 0b00011);
        let t = tree_using(std::slice::from_ref(&g));
        let bytes = encode(&t, None);
        let mut expected = vec![b'D', b'T', b'R', b'E', VERSION | HAS_GROUPS, 1];
        expected.extend_from_slice(&groups::hash_groups([&g]).to_le_bytes());
        // value, flags (list, roles top+jungle), name, 2 members.
        expected.extend_from_slice(&[190, 0b0000_1100, 4, b'T', b'o', b'p', b's', 2, 16, 17]);
        expected.extend_from_slice(&[1, 1, 190, 0b1000_0000, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes, expected);
        // Only used groups are stored, without notes; the loaded catalog has them alone.
        let back = dec(&bytes).unwrap().tree;
        let stored: Vec<&Group> = back.groups().user_groups().collect();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            (
                stored[0].value,
                stored[0].name.as_str(),
                stored[0].note.as_str()
            ),
            (190, "Tops", "")
        );
        assert_eq!(stored[0].members, vec![16, 17]);
        assert_eq!(stored[0].roles, 0b00011);
        assert_eq!(back.groups().source(), None);
    }

    #[test]
    fn member_encodings_pick_the_shortest() {
        let all: Vec<u8> = (0..CHAMPION_COUNT).map(champions::value_of).collect();
        let cases: [(Vec<u8>, u8, usize); 6] = [
            (vec![16], MEMBERS_LIST, 2),
            (all[..21].to_vec(), MEMBERS_LIST, 22),
            (all[..22].to_vec(), MEMBERS_BITMAP, 22),
            (all.iter().copied().step_by(2).collect(), MEMBERS_BITMAP, 22),
            (all[..CHAMPION_COUNT - 3].to_vec(), MEMBERS_COMPLEMENT, 4),
            (all.clone(), MEMBERS_COMPLEMENT, 1),
        ];
        for (members, mode, size) in cases {
            let g = group(200, "Ünïcødé ✓", &members, 0b10101);
            let mut out = Vec::new();
            write_group(&mut out, &g);
            let name_len = g.name.len();
            assert_eq!(out[1] & 0b11, mode, "{} members", members.len());
            assert_eq!(out.len(), 3 + name_len + size, "{} members", members.len());
            let (back, rest) = read_group(&out).unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                (back.members, back.roles, back.name),
                (g.members.clone(), 0b10101, g.name.clone())
            );
            // And through a whole file.
            let t = tree_using(&[g]);
            let bytes = encode(&t, None);
            assert_eq!(encode(&dec(&bytes).unwrap().tree, None), bytes);
        }
    }

    #[test]
    fn several_groups_are_stored_in_value_order_with_their_nodes() {
        let gs = [
            group(201, "B", &[20, 21, 22], 0),
            group(190, "A", &[16, 17], 0b1),
            group(255, "C", &[100], 0),
        ];
        let mut t = tree_using(&gs);
        // The same group on several nodes is stored once.
        let first = t.get(t.root()).unwrap().children()[0];
        t.add_child(first, 190).unwrap();
        let bytes = encode(&t, None);
        let back = dec(&bytes).unwrap().tree;
        let values: Vec<u8> = back.groups().user_groups().map(|g| g.value).collect();
        assert_eq!(values, vec![190, 201, 255]);
        assert_eq!(dump(&back), dump(&t));
        assert_eq!(encode(&back, None), bytes);
    }

    #[test]
    fn rejects_bad_group_blocks() {
        let g = group(190, "Tops", &[16, 17], 0);
        let good = encode(&tree_using(std::slice::from_ref(&g)), None);
        // Header: magic, version, count, 8 hash bytes; the definition starts at 14.
        let def = 14;
        let corrupt = |bytes: &[u8]| matches!(dec(bytes), Err(LoadError::Corrupt(_)));
        // Damaged hash, damaged member, damaged role bits, unknown encoding.
        let mut b = good.clone();
        b[6] ^= 1;
        assert!(corrupt(&b), "hash");
        let mut b = good.clone();
        b[def + 8] = 18;
        assert!(corrupt(&b), "member changed: hash no longer matches");
        let mut b = good.clone();
        b[def + 1] = 0b1000_0000;
        assert!(corrupt(&b), "role bits");
        let mut b = good.clone();
        b[def + 1] = 0b11;
        assert!(corrupt(&b), "encoding 3");
        // Members out of order.
        let mut b = good.clone();
        b.swap(def + 8, def + 9);
        assert!(corrupt(&b), "order");
        // Not a champion.
        let mut b = good.clone();
        b[def + 9] = 189;
        assert!(matches!(dec(&b), Err(LoadError::InvalidGroup(_))));
        // Empty name, value outside the user range.
        let bad = |value: u8, name: &str| {
            let mut out = Vec::new();
            write_group(
                &mut out,
                &Group::new_user(190, name.into(), String::new(), vec![16]),
            );
            out[0] = value;
            read_group(&out).err()
        };
        assert!(matches!(bad(190, ""), Some(LoadError::InvalidGroup(_))));
        assert!(matches!(bad(100, "X"), Some(LoadError::InvalidGroup(_))));
        assert!(matches!(
            bad(190, "Seventeen chars!!"),
            Some(LoadError::InvalidGroup(_))
        ));
        // A group value in the tree without a definition, and a definition nobody uses.
        assert!(corrupt(&raw(&[190], &[0b1000_0000])), "undefined group");
        let mut unused = vec![b'D', b'T', b'R', b'E', VERSION | HAS_GROUPS, 1];
        unused.extend_from_slice(&groups::hash_groups([&g]).to_le_bytes());
        unused.extend_from_slice(&[190, 0, 4, b'T', b'o', b'p', b's', 2, 16, 17]);
        unused.extend_from_slice(&[1, 1, 16, 0b1000_0000, 0]);
        assert!(corrupt(&unused), "unused group");
        // Groups out of order.
        let gs = [group(190, "A", &[16], 0), group(191, "B", &[17], 0)];
        let mut b = encode(&tree_using(&gs), None);
        let first = def;
        let second = def + 3 + 1 + 2;
        let (x, y) = (b[first], b[second]);
        b[first] = y;
        b[second] = x;
        assert!(corrupt(&b), "group order");
    }

    #[test]
    fn role_tags_round_trip() {
        let mut t = DraftTree::new();
        let mut at = t.root();
        // Six bans, then blue pick 1 and red pick 1.
        for v in [16, 17, 18, 19, 20, 21, 22, 23] {
            at = t.add_child(at, v).unwrap();
        }
        let red_p1 = at;
        let blue_p1 = t.get(red_p1).unwrap().parent().unwrap();
        t.set_roles(blue_p1, 0b00011).unwrap();
        t.set_roles(red_p1, 0b10000).unwrap();
        let bytes = encode(&t, Some(red_p1));
        let back = dec(&bytes).unwrap();
        let picks = back.tree.team_picks(back.selected.unwrap());
        assert_eq!(picks[0][0].1, 0b00011);
        assert_eq!(picks[1][0].1, 0b10000);
    }

    #[test]
    fn fearless_round_trips_and_keeps_locked_branches() {
        use crate::fearless::{Fearless, Mode};
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        t.add_child(a, 17).unwrap();
        let mut f = Fearless {
            game: 2,
            mode: Mode::Soft,
            ..Fearless::default()
        };
        f.rows[0][0] = 16;
        f.rows[4][9] = 40;
        t.set_fearless(f);
        let back = dec(&encode(&t, None)).unwrap().tree;
        assert_eq!(*back.fearless(), f);
        // Soft fearless doesn't lock bans, so both nodes are still there and reachable.
        assert_eq!(back.len(), 3);
        assert!(back.unreachable().is_empty());
        let mut hard = f;
        hard.mode = Mode::Hard;
        t.set_fearless(hard);
        let back = dec(&encode(&t, None)).unwrap().tree;
        assert_eq!(back.len(), 3, "locked branches are kept");
        assert_eq!(back.unreachable().len(), 2);
    }

    #[test]
    fn rejects_bad_input() {
        let good = encode(&sample_tree(), None);
        assert_eq!(dec(b"nope").err(), Some(LoadError::NotASaveFile));
        assert_eq!(
            dec(b"DTRE\x04\x01\x00\x00").err(),
            Some(LoadError::UnsupportedVersion(4)),
            "older versions don't load"
        );
        assert!(matches!(
            dec(&good[..good.len() - 1]),
            Err(LoadError::Corrupt(_))
        ));
        let mut extra = good.clone();
        extra.push(0);
        assert!(matches!(dec(&extra), Err(LoadError::Corrupt(_))));
        // Shape "up" first would climb above the root.
        assert!(matches!(
            dec(&raw(&[16], &[0b0100_0000])),
            Err(LoadError::Corrupt(_))
        ));
        // Non-zero padding.
        assert!(matches!(
            dec(&raw(&[16], &[0b1000_0001])),
            Err(LoadError::Corrupt(_))
        ));
        // The same champion twice among siblings is never valid.
        assert_eq!(
            dec(&raw(&[16, 16], &[0b1010_0000])).err(),
            Some(LoadError::InvalidTree(TreeError::DuplicateSibling))
        );
        // NO_BAN at a pick, and values that are neither champions nor groups.
        assert!(champions::by_value(189).is_none() && !groups::is_group(189));
        assert_eq!(
            dec(&raw(&[1; 7], &[0xFE, 0x00])).err(),
            Some(LoadError::InvalidTree(TreeError::InvalidForPosition))
        );
        assert_eq!(
            dec(&raw(&[189], &[0b1000_0000])).err(),
            Some(LoadError::InvalidTree(TreeError::UnknownValue))
        );
        assert_eq!(
            from_text("not base64!").err(),
            Some(LoadError::NotASaveFile)
        );
    }

    #[test]
    fn selection_round_trips() {
        let mut tree = sample_tree();
        let root = tree.root();
        let first = tree.get(root).unwrap().children()[0];
        let mut deep = first;
        while let Some(&c) = tree.get(deep).unwrap().children().first() {
            deep = c;
        }
        let deep = tree.get(deep).unwrap().parent().unwrap();

        for selected in [root, first, deep] {
            let bytes = encode(&tree, Some(selected));
            assert_eq!(bytes[4], VERSION | HAS_SELECTION);
            let loaded = dec(&bytes).unwrap();
            assert_eq!(dump(&loaded.tree), dump(&tree));
            let path = |t: &DraftTree, id| -> Vec<u8> {
                t.path(id)
                    .iter()
                    .map(|&n| t.get(n).unwrap().value())
                    .collect()
            };
            assert_eq!(
                path(&loaded.tree, loaded.selected.unwrap()),
                path(&tree, selected)
            );
        }

        // Only a varint is added.
        assert_eq!(
            encode(&tree, Some(first)).len(),
            encode(&tree, None).len() + 1
        );
        // A removed (stale) id is not stored.
        let gone = tree.add_child(first, 100).unwrap();
        tree.remove_subtree(gone).unwrap();
        assert_eq!(encode(&tree, Some(gone)), encode(&tree, None));
    }

    #[test]
    fn rejects_bad_selection() {
        let tree = sample_tree();
        let good = encode(&tree, Some(tree.root()));
        // Flag set but index missing.
        let missing = &good[..good.len() - 1];
        assert!(matches!(dec(missing), Err(LoadError::Corrupt(_))));
        // Index past the last node.
        let mut out_of_range = good.clone();
        *out_of_range.last_mut().unwrap() = tree.len() as u8;
        assert!(matches!(dec(&out_of_range), Err(LoadError::Corrupt(_))));
        // Bytes after the index.
        let mut extra = good.clone();
        extra.push(0);
        assert!(matches!(dec(&extra), Err(LoadError::Corrupt(_))));
    }

    #[test]
    fn trees_over_the_node_limit_do_not_load() {
        // Just the header and the node count: refused before the missing body is noticed.
        let mut over = vec![b'D', b'T', b'R', b'E', VERSION, 1];
        write_varint(&mut over, MAX_NODES as u32 + 1);
        assert_eq!(
            dec(&over).err(),
            Some(LoadError::TooManyNodes(MAX_NODES + 1))
        );
        assert_eq!(
            LoadError::TooManyNodes(MAX_NODES + 1).to_string(),
            "This tree has 250,001 nodes; Recvrve opens trees with up to 250,000."
        );
        // A huge count can't make the loader allocate for it either.
        let mut huge = vec![b'D', b'T', b'R', b'E', VERSION, 1];
        write_varint(&mut huge, u32::MAX);
        assert_eq!(
            dec(&huge).err(),
            Some(LoadError::TooManyNodes(u32::MAX as usize))
        );
        // The limit itself loads: checked with a small one, as a full-size tree is slow.
        let bytes = encode(&sample_tree(), None);
        let n = sample_tree().len() - 1;
        assert!(decode_capped(&bytes, n).is_ok());
        assert_eq!(
            decode_capped(&bytes, n - 1).err(),
            Some(LoadError::TooManyNodes(n))
        );
    }

    #[test]
    fn content_hash_is_the_hash_of_the_encoding() {
        use std::hash::Hasher;
        let mut trees = vec![
            DraftTree::new(),
            sample_tree(),
            tree_using(&[group(190, "Tops", &[16, 17], 0b00011)]),
        ];
        let mut named = sample_tree();
        named.set_team_name(0, "G2");
        named.set_marked(named.get(named.root()).unwrap().children()[0], true);
        trees.push(named);
        for t in &trees {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            h.write(&encode(t, None));
            assert_eq!(content_hash(t), h.finish());
        }
        // Any edit that changes the encoding changes the hash.
        let hashes: Vec<u64> = trees.iter().map(content_hash).collect();
        for (i, a) in hashes.iter().enumerate() {
            assert!(hashes[i + 1..].iter().all(|b| b != a));
        }
    }

    #[test]
    fn thousands_separators() {
        for (n, s) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1,000"),
            (250_000, "250,000"),
        ] {
            assert_eq!(thousands(n), s);
        }
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn varint_round_trip() {
        for n in [0, 1, 127, 128, 300, 16_383, 16_384, u32::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, n);
            assert_eq!(read_varint(&buf).unwrap(), (n, &[][..]));
        }
    }

    #[test]
    fn extensions() {
        use SaveFormat::*;
        assert_eq!(SaveFormat::from_path(Path::new("a.dts")), Some(Compact));
        assert_eq!(SaveFormat::from_path(Path::new("a.DTT")), Some(Text));
        assert_eq!(SaveFormat::from_path(Path::new("a.txt")), None);
        assert_eq!(SaveFormat::from_path(Path::new("dts")), None);
        assert_eq!(
            Compact.apply_extension("x/draft".into()),
            PathBuf::from("x/draft.dts")
        );
        assert_eq!(
            Text.apply_extension("draft.dtt".into()),
            PathBuf::from("draft.dtt")
        );
        assert_eq!(
            Text.apply_extension("draft.dts".into()),
            PathBuf::from("draft.dts.dtt")
        );
        assert_eq!(
            load(Path::new("whatever.json")).err(),
            Some(LoadError::InvalidFileType)
        );
    }
}
