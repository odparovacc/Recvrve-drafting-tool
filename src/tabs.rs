//! Tabs: several trees open at once.
//!
//! The active tab's state lives in the app's signals, as before tabs existed. Switching
//! tabs parks that state in a [`Parked`] and moves the other tab's parked state into the
//! signals. Each tab keeps its own tree (and so its own group config, which the tree
//! owns), undo history, selection, Find bar, zoom, filters and follow mode; the clipboard,
//! selection groups, layout, theme and settings are shared.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slotmap::SecondaryMap;

use crate::find::FindOptions;
use crate::follow::Follow;
use crate::history::History;
use crate::tree::{DraftTree, NodeId, Pruned};

/// What the tab strip shows for a tab, kept for every tab (the active one included).
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub id: u64,
    /// "Draft N": numbered in the order tabs were opened.
    pub number: usize,
    /// The file the tree was last loaded from or saved to.
    pub source: Option<PathBuf>,
    /// [`content_hash`](crate::save::content_hash) of the tree when it was last loaded or
    /// saved, or when the tab opened: the tab has unsaved changes while the tree hashes
    /// differently. `None` for a tree never saved as it is (a pasted one): always unsaved.
    pub baseline: Option<u64>,
    /// Name and unsaved state as last seen; kept up to date for the active tab while it is
    /// active, and frozen while it is parked.
    pub name: String,
    pub dirty: bool,
}

impl TabInfo {
    pub fn new(id: u64, number: usize, tree: &DraftTree, source: Option<PathBuf>) -> TabInfo {
        TabInfo {
            id,
            number,
            name: tab_name(tree, source.as_deref(), number),
            source,
            baseline: Some(crate::save::content_hash(tree)),
            dirty: false,
        }
    }

    /// A new tab nobody has touched: loading a file may use it instead of opening another.
    pub fn untouched(&self, tree: &DraftTree) -> bool {
        self.source.is_none() && tree.len() <= 1 && !self.dirty
    }
}

/// A tab's name: the team names when one is set ("G2 vs Fnatic"), otherwise the file it
/// came from, otherwise "Draft N".
pub fn tab_name(tree: &DraftTree, source: Option<&Path>, number: usize) -> String {
    let [blue, red] = tree.team_names();
    if !blue.is_empty() || !red.is_empty() {
        let or = |s: &String, default: &str| {
            if s.is_empty() {
                default.to_string()
            } else {
                s.clone()
            }
        };
        return format!("{} vs {}", or(blue, "Blue"), or(red, "Red"));
    }
    if let Some(stem) = source.and_then(|p| p.file_stem()) {
        return stem.to_string_lossy().into_owned();
    }
    format!("Draft {number}")
}

/// Everything a tab keeps while another tab is shown.
pub struct Parked {
    pub tree: DraftTree,
    pub cursor: NodeId,
    pub extra: Vec<NodeId>,
    pub history: History,
    pub last_child: SecondaryMap<NodeId, NodeId>,
    pub group_colors: HashMap<u8, usize>,
    pub find_open: bool,
    pub find_query: String,
    pub find_options: FindOptions,
    pub marked_only: bool,
    /// Graph zoom, left to right and top to bottom.
    pub zoom: (f64, f64),
    pub pruned: Option<Pruned>,
    pub pruned_conflicts: Option<Pruned>,
    pub save_selection: bool,
    pub edit_unreachable: bool,
    pub follow: Option<Follow>,
}

impl Parked {
    /// A fresh tab's state for `tree`, with the given zoom.
    pub fn fresh(tree: DraftTree, cursor: Option<NodeId>, zoom: (f64, f64)) -> Parked {
        let cursor = cursor.unwrap_or(tree.root());
        Parked {
            tree,
            cursor,
            extra: Vec::new(),
            history: History::default(),
            last_child: SecondaryMap::new(),
            group_colors: HashMap::new(),
            find_open: false,
            find_query: String::new(),
            find_options: FindOptions::default(),
            marked_only: false,
            zoom,
            pruned: None,
            pruned_conflicts: None,
            save_selection: false,
            edit_unreachable: false,
            follow: None,
        }
    }
}

/// Moves the tab `from` next to `to`: before it, or after it with `after`. Unknown ids or
/// a tab dropped on itself change nothing.
pub fn move_tab(tabs: &mut Vec<TabInfo>, from: u64, to: u64, after: bool) {
    if from == to {
        return;
    }
    let Some(i) = tabs.iter().position(|t| t.id == from) else {
        return;
    };
    if !tabs.iter().any(|t| t.id == to) {
        return;
    }
    let tab = tabs.remove(i);
    let j = tabs.iter().position(|t| t.id == to).expect("checked above");
    tabs.insert(if after { j + 1 } else { j }, tab);
}

/// The tab after `id` in the strip, wrapping around.
pub fn next_tab(tabs: &[TabInfo], id: u64) -> Option<u64> {
    let i = tabs.iter().position(|t| t.id == id)?;
    Some(tabs[(i + 1) % tabs.len()].id)
}

/// The tab to show after closing `id`: the one to its right, else the one to its left.
pub fn neighbour(tabs: &[TabInfo], id: u64) -> Option<u64> {
    let i = tabs.iter().position(|t| t.id == id)?;
    tabs.get(i + 1)
        .or_else(|| i.checked_sub(1).and_then(|j| tabs.get(j)))
        .map(|t| t.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(ids: &[u64]) -> Vec<TabInfo> {
        let t = DraftTree::new();
        ids.iter()
            .map(|&id| TabInfo::new(id, id as usize, &t, None))
            .collect()
    }

    fn ids(tabs: &[TabInfo]) -> Vec<u64> {
        tabs.iter().map(|t| t.id).collect()
    }

    #[test]
    fn names_come_from_teams_then_files() {
        let mut t = DraftTree::new();
        assert_eq!(tab_name(&t, None, 3), "Draft 3");
        let file = Path::new("/x/saves/scrim-week2.dts");
        assert_eq!(tab_name(&t, Some(file), 3), "scrim-week2");
        t.set_team_name(0, "G2");
        assert_eq!(tab_name(&t, Some(file), 3), "G2 vs Red");
        t.set_team_name(1, "Fnatic");
        assert_eq!(tab_name(&t, None, 3), "G2 vs Fnatic");
        t.set_team_name(0, "");
        assert_eq!(tab_name(&t, None, 3), "Blue vs Fnatic");
    }

    #[test]
    fn untouched_tabs_can_be_reused() {
        let mut t = DraftTree::new();
        let mut info = TabInfo::new(1, 1, &t, None);
        assert!(info.untouched(&t));
        t.add_child(t.root(), 16).unwrap();
        assert!(!info.untouched(&t));
        let t = DraftTree::new();
        info.source = Some("a.dts".into());
        assert!(!info.untouched(&t));
    }

    #[test]
    fn moving_cycling_and_closing() {
        let mut list = tabs(&[1, 2, 3, 4]);
        move_tab(&mut list, 1, 3, true);
        assert_eq!(ids(&list), [2, 3, 1, 4]);
        move_tab(&mut list, 4, 2, false);
        assert_eq!(ids(&list), [4, 2, 3, 1]);
        move_tab(&mut list, 4, 4, true);
        move_tab(&mut list, 9, 4, true);
        assert_eq!(ids(&list), [4, 2, 3, 1]);
        assert_eq!(next_tab(&list, 3), Some(1));
        assert_eq!(next_tab(&list, 1), Some(4));
        assert_eq!(neighbour(&list, 2), Some(3));
        assert_eq!(neighbour(&list, 1), Some(3));
        assert_eq!(neighbour(&tabs(&[7]), 7), None);
    }
}
