//! Button settings: which panel buttons are shown.
//!
//! Every button listed here can be hidden from its panel under Help → Button settings.
//! Hidden buttons leave the panel; shown again, they come back in their usual place. The
//! user's choices are kept in `config/settings.json` as differences from the defaults, so
//! a later change of defaults reaches everyone who never touched that button.
//!
//! Not listed (always shown): the fullscreen button, the tabs, Help, the panels' ▾ hide
//! buttons, and buttons in popups and in the follow and find bars.

use std::collections::BTreeMap;

use crate::keys::Action;

/// The panel a button belongs to; each is a section of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    TopBar,
    Champions,
    Groups,
    Tree,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::TopBar,
        Section::Champions,
        Section::Groups,
        Section::Tree,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::TopBar => "Top bar",
            Section::Champions => "Champions",
            Section::Groups => "Representational groups",
            Section::Tree => "Draft tree",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Save,
    Load,
    Merge,
    Export,
    Share,
    Fearless,
    Follow,
    Previews,
    Tooltips,
    SelectionFilters,
    RGroupFilter,
    SetMode,
    AutoTag,
    SelectionEdit,
    ClearFilters,
    SelectionPresets,
    GroupsEdit,
    GroupsDetails,
    GroupsPresets,
    Zoom,
    Delete,
    Reset,
    HideBranches,
    ShowAll,
    MarkedOnly,
    HideUnreachable,
    EditUnreachable,
    RemoveUnreachable,
    RemoveConflicting,
}

impl Button {
    /// Every button, in panel order (the order the settings list them).
    pub const ALL: [Button; 29] = {
        use Button::*;
        [
            Save,
            Load,
            Merge,
            Export,
            Share,
            Fearless,
            Follow,
            Previews,
            Tooltips,
            SelectionFilters,
            RGroupFilter,
            SetMode,
            AutoTag,
            SelectionEdit,
            ClearFilters,
            SelectionPresets,
            GroupsEdit,
            GroupsDetails,
            GroupsPresets,
            Zoom,
            Delete,
            Reset,
            HideBranches,
            ShowAll,
            MarkedOnly,
            HideUnreachable,
            EditUnreachable,
            RemoveUnreachable,
            RemoveConflicting,
        ]
    };

    /// Name in the settings file.
    pub fn id(self) -> &'static str {
        use Button::*;
        match self {
            Save => "save",
            Load => "load",
            Merge => "merge",
            Export => "export",
            Share => "share",
            Fearless => "fearless",
            Follow => "follow",
            Previews => "previews",
            Tooltips => "tooltips",
            SelectionFilters => "selection-filters",
            RGroupFilter => "rgroup-filter",
            SetMode => "set-mode",
            AutoTag => "auto-tag",
            SelectionEdit => "selection-edit",
            ClearFilters => "clear-filters",
            SelectionPresets => "selection-presets",
            GroupsEdit => "groups-edit",
            GroupsDetails => "groups-details",
            GroupsPresets => "groups-presets",
            Zoom => "zoom",
            Delete => "delete",
            Reset => "reset",
            HideBranches => "hide-branches",
            ShowAll => "show-all",
            MarkedOnly => "marked-only",
            HideUnreachable => "hide-unreachable",
            EditUnreachable => "edit-unreachable",
            RemoveUnreachable => "remove-unreachable",
            RemoveConflicting => "remove-conflicting",
        }
    }

    /// Name in the settings list.
    pub fn label(self) -> &'static str {
        use Button::*;
        match self {
            Save => "Save",
            Load => "Load",
            Merge => "Merge",
            Export => "Export",
            Share => "Share",
            Fearless => "Fearless",
            Follow => "Follow",
            Previews => "Previews",
            Tooltips => "Tooltips",
            SelectionFilters => "Selection group filters",
            RGroupFilter => "RGroup filter",
            SetMode => "Union / Intersection",
            AutoTag => "Auto-tag",
            SelectionEdit => "Edit",
            ClearFilters => "Clear",
            SelectionPresets => "Hide presets",
            GroupsEdit => "Edit",
            GroupsDetails => "Details",
            GroupsPresets => "Hide presets",
            Zoom => "Zoom (click resets to 100%)",
            Delete => "Delete",
            Reset => "Reset",
            HideBranches => "Hide branches / Show branches",
            ShowAll => "Show all",
            MarkedOnly => "Marked only",
            HideUnreachable => "Hide unreachable",
            EditUnreachable => "Edit unreachable",
            RemoveUnreachable => "Remove unreachable (and Restore)",
            RemoveConflicting => "Remove conflicting (and Restore)",
        }
    }

    pub fn section(self) -> Section {
        use Button::*;
        match self {
            Save | Load | Merge | Export | Share | Fearless | Follow | Previews | Tooltips => {
                Section::TopBar
            }
            SelectionFilters | RGroupFilter | SetMode | AutoTag | SelectionEdit | ClearFilters
            | SelectionPresets => Section::Champions,
            GroupsEdit | GroupsDetails | GroupsPresets => Section::Groups,
            Zoom | Delete | Reset | HideBranches | ShowAll | MarkedOnly | HideUnreachable
            | EditUnreachable | RemoveUnreachable | RemoveConflicting => Section::Tree,
        }
    }

    /// Shown until the user changes it.
    pub fn default_shown(self) -> bool {
        use Button::*;
        matches!(
            self,
            Save | Load
                | Export
                | Share
                | Fearless
                | SelectionFilters
                | SelectionEdit
                | ClearFilters
                | GroupsEdit
        )
    }

    /// The shortcut that does the same, for the settings list.
    pub fn action(self) -> Option<Action> {
        use Button::*;
        match self {
            Save => Some(Action::Save),
            Fearless => Some(Action::Fearless),
            Follow => Some(Action::Follow),
            SelectionFilters => Some(Action::ToggleFilter(0)),
            RGroupFilter => Some(Action::ToggleRGroupFilter),
            SetMode => Some(Action::ToggleSetMode),
            ClearFilters => Some(Action::ClearFilters),
            Delete => Some(Action::DeleteNode),
            HideBranches => Some(Action::ToggleHidden),
            ShowAll => Some(Action::ShowAllHidden),
            HideUnreachable => Some(Action::HideUnreachable),
            RemoveUnreachable => Some(Action::RemoveUnreachable),
            RemoveConflicting => Some(Action::RemoveConflicting),
            Reset => Some(Action::Reset),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn by_id(id: &str) -> Option<Button> {
        Button::ALL.into_iter().find(|b| b.id() == id)
    }
}

/// The user's changes from the defaults: button id → shown.
pub type Overrides = BTreeMap<String, bool>;

pub fn shown(overrides: &Overrides, button: Button) -> bool {
    overrides
        .get(button.id())
        .copied()
        .unwrap_or(button.default_shown())
}

/// Shows or hides `button`, keeping only differences from the defaults.
pub fn set_shown(overrides: &mut Overrides, button: Button, on: bool) {
    if on == button.default_shown() {
        overrides.remove(button.id());
    } else {
        overrides.insert(button.id().to_string(), on);
    }
}

/// Shows every button.
pub fn show_all(overrides: &mut Overrides) {
    overrides.clear();
    for b in Button::ALL {
        set_shown(overrides, b, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_and_round_trip() {
        for b in Button::ALL {
            assert_eq!(Button::by_id(b.id()), Some(b));
        }
        let mut ids: Vec<_> = Button::ALL.iter().map(|b| b.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Button::ALL.len());
    }

    #[test]
    fn sections_keep_panel_order() {
        // Each section's buttons are listed together.
        let sections: Vec<Section> = Button::ALL.iter().map(|b| b.section()).collect();
        let mut seen: Vec<Section> = Vec::new();
        for s in sections {
            if seen.last() != Some(&s) {
                assert!(!seen.contains(&s), "{s:?} listed twice");
                seen.push(s);
            }
        }
        assert_eq!(seen, Section::ALL);
    }

    #[test]
    fn overrides_store_only_changes() {
        let mut o = Overrides::new();
        assert!(shown(&o, Button::Save) && !shown(&o, Button::Merge));
        set_shown(&mut o, Button::Merge, true);
        set_shown(&mut o, Button::Save, false);
        assert!(shown(&o, Button::Merge) && !shown(&o, Button::Save));
        set_shown(&mut o, Button::Save, true);
        assert_eq!(o.len(), 1);
        show_all(&mut o);
        assert!(Button::ALL.iter().all(|&b| shown(&o, b)));
        o.clear();
        assert!(!shown(&o, Button::Reset));
    }
}
