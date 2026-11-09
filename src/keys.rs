//! Keyboard shortcuts and their configuration.
//!
//! A document-level listener in the webview turns every key press into a combo string
//! such as `Ctrl+Shift+W`, looks it up in the bindings (sent to the webview by
//! [`send_to_webview`]), stops the webview's own handling of shortcuts (e.g. Ctrl+R reloading the
//! page) and sends the action to Rust. In text fields, the text editing combos (Ctrl+C,
//! Ctrl+V, Ctrl+X, Ctrl+Z, arrows, ...) keep their usual meaning.
//!
//! Bindings live in `config/keybinds.json`. Every action can have any number of combos; a
//! combo the user records needs at least two keys (a modifier and a key). Esc is fixed: it
//! closes overlays and cancels recording.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

use dioxus::document::{self, Eval};
use serde::{Deserialize, Serialize};

pub const KEYBINDS_FILE_NAME: &str = "keybinds.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Set the selected node to No ban / Empty.
    EmptyNode,
    /// The next champion or group clicked becomes the selected node's value.
    SetValue,
    Save,
    Undo,
    Redo,
    /// Copy the selected nodes' branches.
    Copy,
    /// Copy the selected nodes' branches and delete them.
    Cut,
    /// Put the copied branches below the active node.
    Paste,
    /// Move the active node one place earlier / later among its siblings.
    MoveEarlier,
    MoveLater,
    /// Mark or unmark the line ending at the selected nodes.
    Mark,
    /// Go to the parent node.
    Parent,
    /// Go down (the only child, or the one you came from).
    Down,
    NextBranch,
    PrevBranch,
    /// Delete the selected node (outside text fields).
    DeleteNode,
    /// Toggle the selection group filter at this index (0-based, 0..9).
    ToggleFilter(usize),
    ClearFilters,
    /// Switch between union and intersection of active selection groups.
    ToggleSetMode,
    /// Whether selection filters also apply to representational groups.
    ToggleRGroupFilter,
    /// Toggle role tag (top, jungle, mid, bot, support) on the selected pick (0..5).
    ToggleRole(usize),
    /// Open or close the fearless draft editor.
    Fearless,
    /// Copy the selected node's branch (it and everything below) next to it.
    DuplicateBranch,
    /// Copy the line from the first draft action down to the selected node.
    DuplicateLine,
    /// Copy that line and everything below the selected node.
    DuplicateLineAndBranch,
    ToggleFullscreen,
    /// Open or close the Help panel.
    Help,
    /// Hide or show the selected node's branches.
    ToggleHidden,
    /// Show every hidden branch.
    ShowAllHidden,
    /// Hide (or show again) the branches below every unreachable (fearless) node.
    HideUnreachable,
    /// Delete every unreachable branch / put the last deleted ones back.
    RemoveUnreachable,
    RestoreUnreachable,
    /// Delete every conflicting branch / put the last deleted ones back.
    RemoveConflicting,
    RestoreConflicting,
    /// Delete the whole tree.
    Reset,
    /// Open the find bar (or focus it).
    Find,
    /// Open a new tab with an empty tree.
    NewTab,
    /// Show the next tab.
    NextTab,
    TabToNewWindow,
    /// Open a save file / the merge panel / the Export menu.
    Load,
    Merge,
    Export,
    /// Graph zoom in / out / back to 100%.
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Move the graph's view, like scrolling, without changing the selection. Handled in
    /// the webview, for as long as the key is held (see `layout.rs`).
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    /// Start or leave follow mode.
    Follow,
    /// In follow mode: take back the last entered action.
    FollowBack,
    /// Esc: not configurable.
    CloseOverlays,
}

const ROLE_NAMES: [&str; 5] = ["Top", "Jungle", "Mid", "Bot", "Support"];

impl Action {
    /// Every configurable action, in the order the shortcut editor lists them.
    pub fn all() -> Vec<Action> {
        use Action::*;
        let mut all = vec![
            Parent,
            Down,
            NextBranch,
            PrevBranch,
            EmptyNode,
            SetValue,
            DuplicateBranch,
            DuplicateLine,
            DuplicateLineAndBranch,
            DeleteNode,
            ToggleHidden,
            ShowAllHidden,
            HideUnreachable,
            RemoveUnreachable,
            RestoreUnreachable,
            RemoveConflicting,
            RestoreConflicting,
            Reset,
            Find,
            Undo,
            Redo,
            Save,
            Copy,
            Cut,
            Paste,
            MoveEarlier,
            MoveLater,
            Mark,
            Follow,
            FollowBack,
            NewTab,
            NextTab,
            TabToNewWindow,
            Load,
            Merge,
            Export,
            ZoomIn,
            ZoomOut,
            ZoomReset,
            PanUp,
            PanDown,
            PanLeft,
            PanRight,
        ];
        all.extend((0..9).map(ToggleFilter));
        all.extend([ClearFilters, ToggleSetMode, ToggleRGroupFilter]);
        all.extend((0..5).map(ToggleRole));
        all.extend([Fearless, ToggleFullscreen, Help]);
        all
    }

    /// Name in the keybinds file and in messages from the webview.
    pub fn id(self) -> String {
        use Action::*;
        match self {
            EmptyNode => "empty".into(),
            SetValue => "set-value".into(),
            Save => "save".into(),
            Undo => "undo".into(),
            Redo => "redo".into(),
            Copy => "copy".into(),
            Cut => "cut".into(),
            Paste => "paste".into(),
            MoveEarlier => "move-earlier".into(),
            MoveLater => "move-later".into(),
            Mark => "mark".into(),
            Parent => "parent".into(),
            Down => "down".into(),
            NextBranch => "next".into(),
            PrevBranch => "prev".into(),
            DeleteNode => "delete".into(),
            ToggleFilter(i) => format!("filter{}", i + 1),
            ClearFilters => "clear-filters".into(),
            ToggleSetMode => "set-mode".into(),
            ToggleRGroupFilter => "rgroup-filter".into(),
            ToggleRole(i) => format!("role{}", i + 1),
            Fearless => "fearless".into(),
            DuplicateBranch => "dup-branch".into(),
            DuplicateLine => "dup-line".into(),
            DuplicateLineAndBranch => "dup-all".into(),
            ToggleFullscreen => "fullscreen".into(),
            Help => "help".into(),
            ToggleHidden => "hide-branches".into(),
            ShowAllHidden => "show-all-branches".into(),
            HideUnreachable => "hide-unreachable".into(),
            RemoveUnreachable => "remove-unreachable".into(),
            RestoreUnreachable => "restore-unreachable".into(),
            RemoveConflicting => "remove-conflicting".into(),
            RestoreConflicting => "restore-conflicting".into(),
            Reset => "reset".into(),
            Find => "find".into(),
            NewTab => "new-tab".into(),
            NextTab => "next-tab".into(),
            TabToNewWindow => "tab-new-window".into(),
            Load => "load".into(),
            Merge => "merge".into(),
            Export => "export".into(),
            ZoomIn => "zoom-in".into(),
            ZoomOut => "zoom-out".into(),
            ZoomReset => "zoom-reset".into(),
            PanUp => "pan-up".into(),
            PanDown => "pan-down".into(),
            PanLeft => "pan-left".into(),
            PanRight => "pan-right".into(),
            Follow => "follow".into(),
            FollowBack => "follow-back".into(),
            CloseOverlays => "escape".into(),
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        if id == "escape" {
            return Some(Action::CloseOverlays);
        }
        Action::all().into_iter().find(|a| a.id() == id)
    }

    /// What the action does, for the Help panel.
    pub fn description(self) -> String {
        use Action::*;
        match self {
            EmptyNode => "Empty the selected node (No ban / Empty)".into(),
            SetValue => {
                "Set the selected node's champion: the next grid click becomes its value".into()
            }
            Save => "Save as compact (.dts)".into(),
            Undo => "Undo".into(),
            Redo => "Redo".into(),
            Copy => "Copy the selected branches".into(),
            Cut => "Cut the selected branches (copy, then delete)".into(),
            Paste => "Paste the copied branches below the selected node".into(),
            MoveEarlier => "Move the selected node one place earlier among its siblings".into(),
            MoveLater => "Move the selected node one place later among its siblings".into(),
            Mark => "Mark or unmark the line ending at the selected node".into(),
            Parent => "Go to the parent node".into(),
            Down => "Go down (the only child, or the one you came from)".into(),
            NextBranch => "Next node at the same depth".into(),
            PrevBranch => "Previous node at the same depth".into(),
            DeleteNode => "Delete the selected node and its subtree".into(),
            ToggleFilter(i) => format!("Toggle selection group filter {}", i + 1),
            ClearFilters => "Turn all selection group filters off".into(),
            ToggleSetMode => "Switch filters between union and intersection".into(),
            ToggleRGroupFilter => "Apply selection filters to representational groups too".into(),
            ToggleRole(i) => format!("Toggle the {} tag of the selected pick", ROLE_NAMES[i]),
            Fearless => "Open the fearless draft editor".into(),
            DuplicateBranch => {
                "Duplicate the selected node's branch (the copy starts empty)".into()
            }
            DuplicateLine => {
                "Duplicate the line from the first ban down to the selected node".into()
            }
            DuplicateLineAndBranch => {
                "Duplicate that line and everything below the selected node".into()
            }
            ToggleFullscreen => {
                "Fullscreen: only champions, groups and the graph (Esc leaves it)".into()
            }
            Help => "Open or close Help (shortcuts, button settings, view and theme)".into(),
            ToggleHidden => "Hide or show the selected node's branches".into(),
            ShowAllHidden => "Show all hidden branches".into(),
            HideUnreachable => {
                "Hide or show the branches below unreachable (fearless) nodes".into()
            }
            RemoveUnreachable => "Delete every unreachable (fearless) branch".into(),
            RestoreUnreachable => {
                "Put back the branches the last Remove unreachable deleted".into()
            }
            RemoveConflicting => "Delete every conflicting branch".into(),
            RestoreConflicting => {
                "Put back the branches the last Remove conflicting deleted".into()
            }
            Reset => "Delete the whole tree (Undo brings it back)".into(),
            Find => "Find in the tree (Enter / Shift + Enter: next / previous)".into(),
            NewTab => "Open a new tab".into(),
            NextTab => "Show the next tab".into(),
            TabToNewWindow => "Move the active tab to a new window".into(),
            Load => "Load a save file".into(),
            Merge => "Merge save files".into(),
            Export => "Open the Export menu".into(),
            ZoomIn => "Zoom the graph in".into(),
            ZoomOut => "Zoom the graph out".into(),
            ZoomReset => "Graph zoom back to 100%".into(),
            PanUp => "Move the graph view up (the selection stays)".into(),
            PanDown => "Move the graph view down (the selection stays)".into(),
            PanLeft => "Move the graph view left (the selection stays)".into(),
            PanRight => "Move the graph view right (the selection stays)".into(),
            Follow => "Start or leave follow mode (enter a real draft as it happens)".into(),
            FollowBack => "Follow mode: take back the last entered action".into(),
            CloseOverlays => "Close popups and editors, then leave fullscreen".into(),
        }
    }

    /// The shipped bindings.
    pub fn defaults(self) -> Vec<Combo> {
        use Action::*;
        let combos: Vec<String> = match self {
            EmptyNode => vec!["Ctrl+Shift+W".into()],
            SetValue => vec!["Ctrl+W".into()],
            Save => vec!["Ctrl+S".into()],
            Undo => vec!["Ctrl+Z".into()],
            Redo => vec!["Ctrl+Y".into(), "Ctrl+Shift+Z".into()],
            Copy => vec!["Ctrl+C".into()],
            Cut => vec!["Ctrl+X".into()],
            Paste => vec!["Ctrl+V".into()],
            MoveEarlier => vec!["Ctrl+ArrowLeft".into()],
            MoveLater => vec!["Ctrl+ArrowRight".into()],
            Mark => vec!["Ctrl+M".into()],
            Parent => vec!["Shift+Z".into()],
            Down => vec!["Shift+X".into()],
            NextBranch => vec!["Ctrl+B".into()],
            PrevBranch => vec!["Shift+B".into()],
            DeleteNode => vec!["Delete".into()],
            ToggleFilter(i) => vec![format!("Ctrl+{}", i + 1)],
            ClearFilters => vec!["Ctrl+0".into()],
            ToggleSetMode => vec!["Ctrl+Q".into()],
            ToggleRGroupFilter => vec!["Ctrl+E".into()],
            ToggleRole(i) => vec![format!("Alt+{}", i + 1)],
            Fearless => vec!["Ctrl+G".into()],
            DuplicateBranch => vec!["Ctrl+D".into()],
            DuplicateLine => vec!["Ctrl+Alt+D".into()],
            DuplicateLineAndBranch => vec!["Ctrl+Shift+D".into()],
            ToggleFullscreen => vec!["F11".into()],
            Help => vec!["F1".into()],
            ToggleHidden => vec!["Ctrl+H".into()],
            ShowAllHidden => vec!["Ctrl+Shift+H".into()],
            HideUnreachable => vec!["Ctrl+Alt+H".into()],
            RemoveUnreachable => vec!["Ctrl+Alt+U".into()],
            RestoreUnreachable => vec!["Ctrl+Shift+U".into()],
            RemoveConflicting => vec!["Ctrl+Alt+C".into()],
            RestoreConflicting => vec!["Ctrl+Shift+C".into()],
            Reset => vec!["Ctrl+R".into()],
            Find => vec!["Ctrl+F".into()],
            NewTab => vec!["Ctrl+T".into()],
            NextTab => vec!["Alt+Backquote".into()],
            TabToNewWindow => vec!["Ctrl+Shift+N".into()],
            Load => vec!["Ctrl+L".into()],
            Merge => vec!["Ctrl+Shift+M".into()],
            Export => vec!["Ctrl+Shift+E".into()],
            // "+" takes Shift on most keyboards, so Ctrl + = works too, and the numpad.
            ZoomIn => vec![
                "Ctrl+Equal".into(),
                "Ctrl+Shift+Equal".into(),
                "Ctrl+NumpadAdd".into(),
            ],
            ZoomOut => vec![
                "Ctrl+Minus".into(),
                "Ctrl+Shift+Minus".into(),
                "Ctrl+NumpadSubtract".into(),
            ],
            // Ctrl + 0 turns the selection filters off.
            ZoomReset => vec!["Ctrl+Shift+0".into()],
            PanUp => vec!["ArrowUp".into()],
            PanDown => vec!["ArrowDown".into()],
            PanLeft => vec!["ArrowLeft".into()],
            PanRight => vec!["ArrowRight".into()],
            Follow => vec!["Ctrl+Shift+L".into()],
            FollowBack => vec!["Backspace".into()],
            CloseOverlays => vec!["Escape".into()],
        };
        combos
            .iter()
            .map(|c| Combo::parse(c).expect("valid default"))
            .collect()
    }
}

/// A key combination: modifiers plus one key. Cmd counts as Ctrl.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// `W`, `1`, `F11`, `Delete`, `Minus`, `ArrowUp`, ... (letters upper case, digits
    /// plain, other keys by their physical key name).
    pub key: String,
}

/// Key names besides letters, digits and F1 ... F24 (the browser's physical key names).
const KEY_NAMES: [&str; 44] = [
    "Delete",
    "Backspace",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Space",
    "Enter",
    "Tab",
    "Escape",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Backquote",
    "Comma",
    "Period",
    "Slash",
    "IntlBackslash",
    "NumpadAdd",
    "NumpadSubtract",
    "NumpadMultiply",
    "NumpadDivide",
    "NumpadDecimal",
    "NumpadEnter",
    "Numpad0",
    "Numpad1",
    "Numpad2",
    "Numpad3",
    "Numpad4",
    "Numpad5",
    "Numpad6",
    "Numpad7",
    "Numpad8",
    "Numpad9",
    "ContextMenu",
];

/// The canonical name of a key (any case), or `None` for an unknown key.
fn key_name(key: &str) -> Option<String> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return c
            .is_ascii_alphanumeric()
            .then(|| c.to_ascii_uppercase().to_string());
    }
    let lower = key.to_lowercase();
    if let Some(digits) = lower.strip_prefix('f')
        && let Ok(n) = digits.parse::<u8>()
        && (1..=24).contains(&n)
        && digits == n.to_string()
    {
        return Some(format!("F{n}"));
    }
    if lower == "esc" {
        return Some("Escape".into());
    }
    KEY_NAMES
        .iter()
        .find(|k| k.to_lowercase() == lower)
        .map(|k| k.to_string())
}

impl Combo {
    /// Reads `Ctrl+Shift+W` (any case, spaces allowed). `None` without exactly one key.
    pub fn parse(text: &str) -> Option<Combo> {
        let mut combo = Combo {
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match m.to_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "meta" => combo.ctrl = true,
                "alt" | "option" => combo.alt = true,
                "shift" => combo.shift = true,
                _ => return None,
            }
        }
        combo.key = key_name(key)?;
        Some(combo)
    }

    /// Number of keys pressed together.
    pub fn key_count(&self) -> usize {
        1 + usize::from(self.ctrl) + usize::from(self.alt) + usize::from(self.shift)
    }

    fn parts(&self) -> Vec<&str> {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(match self.key.as_str() {
            "Escape" => "Esc",
            other => other,
        });
        parts
    }

    /// For people: `Ctrl + Shift + W`.
    pub fn display(&self) -> String {
        let mut parts = self.parts();
        if self.key == "Backquote" {
            *parts.last_mut().unwrap() = "`";
        }
        parts.join(" + ")
    }
}

/// For files and the webview: `Ctrl+Shift+W`.
impl std::fmt::Display for Combo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = self.parts();
        if self.key == "Escape" {
            *parts.last_mut().unwrap() = "Escape";
        }
        f.write_str(&parts.join("+"))
    }
}

/// Why a combo can't be bound.
pub fn check(action: Action, combo: &Combo) -> Result<(), String> {
    if action == Action::CloseOverlays {
        return Err("Esc can't be changed.".into());
    }
    if combo.key == "Escape" && combo.key_count() == 1 {
        return Err("Esc is reserved for closing popups.".into());
    }
    if combo.key_count() < 2 && !action.defaults().contains(combo) {
        return Err(format!(
            "{} is a single key. A shortcut needs at least two keys pressed together, such as Ctrl + {}.",
            combo.display(),
            combo.key
        ));
    }
    Ok(())
}

/// All bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keybinds {
    map: HashMap<Action, Vec<Combo>>,
    /// Held while clicking a tile to add it as a branch without moving.
    pub branch_click: ClickMods,
}

/// Modifier keys held during a click. Cmd counts as Ctrl.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClickMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Default for ClickMods {
    fn default() -> Self {
        ClickMods::SHIFT
    }
}

impl ClickMods {
    pub const SHIFT: ClickMods = ClickMods {
        ctrl: false,
        alt: false,
        shift: true,
    };

    /// The choices for the branch click, in the order the editor lists them.
    pub fn options() -> Vec<ClickMods> {
        let m = |ctrl, alt, shift| ClickMods { ctrl, alt, shift };
        vec![
            m(false, false, true),
            m(true, false, false),
            m(false, true, false),
            m(true, false, true),
            m(false, true, true),
            m(true, true, false),
        ]
    }

    fn names(self) -> Vec<&'static str> {
        [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| n)
        .collect()
    }

    /// For people: `Ctrl + Shift`.
    pub fn display(self) -> String {
        self.names().join(" + ")
    }

    /// For the file: `Ctrl+Shift`.
    pub fn id(self) -> String {
        self.names().join("+")
    }

    /// Reads `Ctrl+Shift` (any order or case); `None` without a modifier or for other text.
    pub fn parse(text: &str) -> Option<ClickMods> {
        let combo = Combo::parse(&format!("{text}+K"))?;
        let mods = ClickMods {
            ctrl: combo.ctrl,
            alt: combo.alt,
            shift: combo.shift,
        };
        Self::options().contains(&mods).then_some(mods)
    }

    /// Whether a click with these modifiers held counts (extra modifiers are fine).
    pub fn held_in(self, ctrl: bool, alt: bool, shift: bool) -> bool {
        (!self.ctrl || ctrl) && (!self.alt || alt) && (!self.shift || shift)
    }
}

impl Default for Keybinds {
    fn default() -> Self {
        Keybinds {
            map: Action::all()
                .into_iter()
                .map(|a| (a, a.defaults()))
                .collect(),
            branch_click: ClickMods::SHIFT,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct KeybindsFile {
    /// Modifiers for adding a branch without moving, such as `Shift`.
    #[serde(
        default,
        rename = "branch-click",
        skip_serializing_if = "Option::is_none"
    )]
    branch_click: Option<String>,
    #[serde(default)]
    bindings: serde_json::Map<String, serde_json::Value>,
}

impl Keybinds {
    pub fn get(&self, action: Action) -> &[Combo] {
        if action == Action::CloseOverlays {
            static ESC: LazyLock<Vec<Combo>> = LazyLock::new(|| Action::CloseOverlays.defaults());
            return &ESC;
        }
        self.map.get(&action).map_or(&[], Vec::as_slice)
    }

    pub fn is_default(&self, action: Action) -> bool {
        self.get(action) == action.defaults().as_slice()
    }

    /// The action `combo` is bound to.
    pub fn action_for(&self, combo: &Combo) -> Option<Action> {
        Action::all()
            .into_iter()
            .chain([Action::CloseOverlays])
            .find(|&a| self.get(a).contains(combo))
    }

    /// Binds `combo` to `action`, replacing the combo at `slot` or adding it when `slot` is
    /// `None`. A combo can only do one thing, so it is taken from any other action: that
    /// action is returned.
    pub fn assign(
        &mut self,
        action: Action,
        slot: Option<usize>,
        combo: Combo,
    ) -> Result<Option<Action>, String> {
        check(action, &combo)?;
        if let Some(other) = self.action_for(&combo) {
            if other == Action::CloseOverlays {
                return Err("That combo is reserved.".into());
            }
            if other == action {
                // Already bound here: only drop the slot being replaced.
                let list = self.map.entry(action).or_default();
                if let Some(i) = slot
                    && list.get(i) != Some(&combo)
                    && i < list.len()
                {
                    list.remove(i);
                }
                return Ok(None);
            }
        }
        let stolen = self.action_for(&combo);
        if let Some(other) = stolen {
            self.map.entry(other).or_default().retain(|c| c != &combo);
        }
        let list = self.map.entry(action).or_default();
        match slot {
            Some(i) if i < list.len() => list[i] = combo,
            _ => list.push(combo),
        }
        Ok(stolen)
    }

    pub fn remove(&mut self, action: Action, slot: usize) {
        if let Some(list) = self.map.get_mut(&action)
            && slot < list.len()
        {
            list.remove(slot);
        }
    }

    /// Restores `action`'s shipped combos, taking them from other actions if needed. Returns
    /// those actions.
    pub fn reset(&mut self, action: Action) -> Vec<Action> {
        let defaults = action.defaults();
        let mut stolen = Vec::new();
        for combo in &defaults {
            if let Some(other) = self.action_for(combo).filter(|&o| o != action) {
                self.map.entry(other).or_default().retain(|c| c != combo);
                stolen.push(other);
            }
        }
        self.map.insert(action, defaults);
        stolen
    }

    /// Reads a keybinds file. Unknown actions, unreadable combos, combos that break the
    /// rules and combos already used by an earlier action are left out and listed in the
    /// returned problems. Actions missing from the file get their defaults (unless another
    /// action uses them).
    pub fn parse(text: &str) -> Result<(Keybinds, Vec<String>), String> {
        let file: KeybindsFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut problems = Vec::new();
        let mut map: HashMap<Action, Vec<Combo>> = HashMap::new();
        let mut used: HashSet<Combo> = HashSet::new();
        // File order doesn't matter: the editor order decides who keeps a duplicate.
        let mut entries: Vec<(Action, &serde_json::Value)> = Vec::new();
        for (id, value) in &file.bindings {
            match Action::from_id(id).filter(|&a| a != Action::CloseOverlays) {
                Some(a) => entries.push((a, value)),
                None => problems.push(format!("unknown action \"{id}\"")),
            }
        }
        let order = Action::all();
        entries.sort_by_key(|(a, _)| order.iter().position(|o| o == a));
        for (action, value) in entries {
            let mut list = Vec::new();
            let items: Vec<&serde_json::Value> = match value {
                serde_json::Value::Array(items) => items.iter().collect(),
                other => vec![other],
            };
            for item in items {
                let Some(text) = item.as_str() else {
                    problems.push(format!("{}: not a text value", action.id()));
                    continue;
                };
                let Some(combo) = Combo::parse(text) else {
                    problems.push(format!("{}: \"{text}\" is not a key combo", action.id()));
                    continue;
                };
                if let Err(e) = check(action, &combo) {
                    problems.push(format!("{}: {e}", action.id()));
                    continue;
                }
                if !used.insert(combo.clone()) {
                    problems.push(format!(
                        "{}: {} is already used by another action",
                        action.id(),
                        combo.display()
                    ));
                    continue;
                }
                list.push(combo);
            }
            map.insert(action, list);
        }
        for action in Action::all() {
            map.entry(action).or_insert_with(|| {
                action
                    .defaults()
                    .into_iter()
                    .filter(|c| used.insert(c.clone()))
                    .collect()
            });
        }
        let branch_click = match file.branch_click.as_deref() {
            None => ClickMods::SHIFT,
            Some(text) => ClickMods::parse(text).unwrap_or_else(|| {
                problems.push(format!(
                    "branch-click: \"{text}\" is not one of {}",
                    ClickMods::options()
                        .iter()
                        .map(|m| m.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                ClickMods::SHIFT
            }),
        };
        Ok((Keybinds { map, branch_click }, problems))
    }

    /// The file contents: every action with its combos.
    pub fn to_json(&self) -> String {
        let mut bindings = serde_json::Map::new();
        for action in Action::all() {
            let combos = self
                .get(action)
                .iter()
                .map(|c| c.to_string().into())
                .collect();
            bindings.insert(action.id(), serde_json::Value::Array(combos));
        }
        let file = KeybindsFile {
            branch_click: Some(self.branch_click.id()),
            bindings,
        };
        serde_json::to_string_pretty(&file).expect("keybinds serialize") + "\n"
    }

    /// `config/keybinds.json` with defaults for anything missing, and a message if the file
    /// had problems. A missing file gives the defaults silently.
    pub fn load() -> (Keybinds, Option<String>) {
        let path = path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Keybinds::default(), None);
        };
        match Keybinds::parse(&text) {
            Ok((binds, problems)) if problems.is_empty() => (binds, None),
            Ok((binds, problems)) => (
                binds,
                Some(format!(
                    "Some shortcuts in {KEYBINDS_FILE_NAME} were skipped: {}.",
                    problems.join("; ")
                )),
            ),
            Err(e) => (
                Keybinds::default(),
                Some(format!(
                    "{KEYBINDS_FILE_NAME} could not be read ({e}), so the default shortcuts are used. It will be replaced when you change a shortcut."
                )),
            ),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::configio::write_with_backup(&path(), &self.to_json())
    }

    /// JavaScript object literal: combo -> action id.
    fn js_table(&self) -> String {
        let mut table = serde_json::Map::new();
        for action in Action::all() {
            for combo in self.get(action) {
                table.insert(combo.to_string(), action.id().into());
            }
        }
        serde_json::Value::Object(table).to_string()
    }

    /// Rows for the Help panel: combos (for people) and what they do. The selection filter
    /// and role tag rows are shown as one row each while they follow one pattern, such as
    /// Ctrl + 1 ... 9.
    pub fn help_rows(&self) -> Vec<(Vec<String>, String)> {
        let mut rows = Vec::new();
        let mut done: HashSet<Action> = HashSet::new();
        for action in Action::all() {
            if done.contains(&action) {
                continue;
            }
            let range: Option<(Vec<Action>, &str)> = match action {
                Action::ToggleFilter(0) => Some((
                    (0..9).map(Action::ToggleFilter).collect(),
                    "Toggle the first nine selection group filters",
                )),
                Action::ToggleRole(0) => Some((
                    (0..5).map(Action::ToggleRole).collect(),
                    "Toggle the Top / Jungle / Mid / Bot / Support tag of the selected pick",
                )),
                _ => None,
            };
            if let Some((actions, what)) = range
                && let Some(prefix) = self.numbered_prefix(&actions)
            {
                let last = actions.len();
                rows.push((vec![format!("{prefix}1 … {last}")], what.to_string()));
                done.extend(actions);
                continue;
            }
            let combos = self.get(action).iter().map(Combo::display).collect();
            rows.push((combos, action.description()));
        }
        rows
    }

    /// `Ctrl + ` if `actions[i]` is bound to exactly Ctrl + (i + 1) for every i.
    fn numbered_prefix(&self, actions: &[Action]) -> Option<String> {
        let first = self.get(actions[0]).first()?;
        let ok = actions.iter().enumerate().all(|(i, &a)| {
            let combos = self.get(a);
            combos.len() == 1
                && combos[0].key == (i + 1).to_string()
                && (combos[0].ctrl, combos[0].alt, combos[0].shift)
                    == (first.ctrl, first.alt, first.shift)
        });
        let display = first.display();
        ok.then(|| display[..display.len() - first.key.len()].to_string())
    }
}

pub fn path() -> PathBuf {
    crate::configio::config_dir().join(KEYBINDS_FILE_NAME)
}

/// The bindings in use, for tooltips anywhere in the app.
static CURRENT: LazyLock<RwLock<Keybinds>> = LazyLock::new(Default::default);

/// The first combo of `action` for people, such as `Ctrl + E`, or `None` if it has none.
pub fn hint(action: Action) -> Option<String> {
    CURRENT.read().ok()?.get(action).first().map(Combo::display)
}

/// Whether a tile click with `mods` adds a branch without moving.
pub fn is_branch_click(mods: dioxus::prelude::Modifiers) -> bool {
    use dioxus::prelude::Modifiers;
    let branch = CURRENT.read().map_or(ClickMods::SHIFT, |c| c.branch_click);
    branch.held_in(
        mods.intersects(Modifiers::CONTROL | Modifiers::META),
        mods.contains(Modifiers::ALT),
        mods.contains(Modifiers::SHIFT),
    )
}

/// The branch click modifiers for people, such as `Shift`.
pub fn branch_click_hint() -> String {
    CURRENT
        .read()
        .map_or(ClickMods::SHIFT, |c| c.branch_click)
        .display()
}

/// ` (Ctrl + E)` for tooltips, or nothing.
pub fn hint_suffix(action: Action) -> String {
    hint(action).map_or_else(String::new, |h| format!(" ({h})"))
}

/// Makes `binds` the bindings tooltips name.
pub fn set_current(binds: &Keybinds) {
    if let Ok(mut current) = CURRENT.write()
        && *current != *binds
    {
        *current = binds.clone();
    }
}

/// Makes `binds` the bindings the webview acts on.
pub fn send_to_webview(binds: &Keybinds) {
    set_current(binds);
    let b = binds.branch_click;
    document::eval(&format!(
        "window.__recvrveKeys = {}; window.__recvrveBranch = {{ctrl: {}, alt: {}, shift: {}}};",
        binds.js_table(),
        b.ctrl,
        b.alt,
        b.shift
    ));
}

/// Whether the find bar is open, so Enter / Shift+Enter step through its matches.
pub fn set_find_open(on: bool) {
    document::eval(&format!("window.__recvrveFind = {on};"));
}

/// While on, the next combo pressed is sent as [`Event::Recorded`] instead of acting.
pub fn set_recording(on: bool) {
    document::eval(&format!("window.__recvrveRecording = {on};"));
}

const LISTENER: &str = r#"
const MODIFIERS = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph', 'CapsLock', 'OS', 'Hyper', 'Super'];
// Combos a text field needs for itself.
const TEXT_COMBOS = ['Ctrl+A', 'Ctrl+C', 'Ctrl+V', 'Ctrl+X', 'Ctrl+Z', 'Ctrl+Y', 'Ctrl+Shift+Z'];
const TEXT_ACTIONS = ['undo', 'redo', 'copy', 'cut', 'paste', 'delete'];
function keyName(e) {
    const code = e.code || '';
    if (/^Key[A-Z]$/.test(code)) {
        // The letter on the key, whatever the layout; the physical key if Alt changes it.
        const k = e.key || '';
        return /^[a-zA-Z]$/.test(k) ? k.toUpperCase() : code.slice(3);
    }
    // Physical digit keys, so these work whatever the keyboard layout.
    if (/^Digit[0-9]$/.test(code)) return code.slice(5);
    return code || e.key || '';
}
// Branch clicks: after a tile is clicked with the branch modifiers held (Shift by default),
// every shortcut is off until one of them is released (or the window loses focus); then
// the app selects the last clicked child.
let branchBatch = false;
function branchHeld(e) {
    const b = window.__recvrveBranch || { shift: true };
    return (!b.ctrl || e.ctrlKey || e.metaKey) && (!b.alt || e.altKey) && (!b.shift || e.shiftKey);
}
function endBranchBatch() {
    if (!branchBatch) return;
    branchBatch = false;
    dioxus.send('branch-release');
}
document.addEventListener('click', (e) => {
    const tile = e.target && e.target.closest && e.target.closest('[data-branch-tile]');
    if (tile && !tile.disabled && branchHeld(e)) branchBatch = true;
}, true);
// Releasing one of the modifiers ends it. On X11 the release of Shift itself can still
// report Shift as held, so the released key counts too.
function releasesBranch(e) {
    const b = window.__recvrveBranch || { shift: true };
    return (b.shift && e.key === 'Shift') || (b.alt && (e.key === 'Alt' || e.key === 'AltGraph'))
        || (b.ctrl && (e.key === 'Control' || e.key === 'Meta')) || !branchHeld(e);
}
document.addEventListener('keyup', (e) => {
    if (branchBatch && releasesBranch(e)) endBranchBatch();
    if (window.__recvrvePan) window.__recvrvePan(null, e.code, false);
}, true);
window.addEventListener('blur', endBranchBatch);
window.addEventListener('blur', () => { if (window.__recvrvePan) window.__recvrvePan(null, null, false); });
// Capture phase, so nothing else in the page sees the key either.
document.addEventListener('keydown', (e) => {
    // Pressing the modifiers again is harmless; anything else is ignored.
    if (branchBatch && !MODIFIERS.includes(e.key)) {
        e.preventDefault();
        e.stopImmediatePropagation();
    }
}, true);
document.addEventListener('keydown', (e) => {
    if (branchBatch || MODIFIERS.includes(e.key)) return;
    const key = keyName(e);
    if (!key) return;
    const parts = [];
    if (e.ctrlKey || e.metaKey) parts.push('Ctrl');
    if (e.altKey) parts.push('Alt');
    if (e.shiftKey) parts.push('Shift');
    parts.push(key);
    const combo = parts.join('+');
    if (window.__recvrveRecording) {
        e.preventDefault();
        e.stopPropagation();
        dioxus.send(combo === 'Escape' ? 'record-cancel' : 'record:' + combo);
        return;
    }
    const target = e.target;
    // The share box is a plain text editor: no shortcuts inside it.
    if (target && target.id === 'share-input') return;
    const inText = !!target && (target.tagName === 'TEXTAREA'
        || (target.tagName === 'INPUT' && target.type !== 'checkbox'));
    // While the find bar is open, Enter / Shift+Enter step through the matches (outside
    // other text fields), and so do Down / Up in the find box.
    if (window.__recvrveFind && (combo === 'Enter' || combo === 'Shift+Enter')
        && (!inText || target.id === 'find-input')) {
        e.preventDefault();
        dioxus.send(combo === 'Enter' ? 'find-next' : 'find-prev');
        return;
    }
    if (window.__recvrveFind && target.id === 'find-input'
        && (combo === 'ArrowDown' || combo === 'ArrowUp')) {
        e.preventDefault();
        dioxus.send(combo === 'ArrowDown' ? 'find-next' : 'find-prev');
        return;
    }
    let action = combo === 'Escape' ? 'escape' : (window.__recvrveKeys || {})[combo] || null;
    if (action && inText && (TEXT_ACTIONS.includes(action) || TEXT_COMBOS.includes(combo)
        || (!e.altKey && /^(Arrow|Home$|End$|Page|Backspace$|Delete$)/.test(key)))) {
        return;
    }
    // Moving the graph's view is the page's own business (see the layout script); popups
    // and menus keep their keys.
    if (action && action.startsWith('pan-')) {
        if (document.querySelector('.modal-backdrop, .menu-backdrop, .role-popup-backdrop')) return;
        e.preventDefault();
        if (window.__recvrvePan) window.__recvrvePan(action.slice(4), e.code, true);
        return;
    }
    // Reloading the page would lose the tree.
    if (!action && (combo === 'F5' || combo === 'Ctrl+R' || combo === 'Ctrl+Shift+R')) action = 'block';
    if (!action) return;
    e.preventDefault();
    if (action === 'block') return;
    dioxus.send(action);
});
"#;

/// What the listener reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Action(Action),
    /// A combo pressed while recording (see [`set_recording`]).
    Recorded(Combo),
    /// Esc pressed while recording.
    RecordCancelled,
    /// The branch click modifiers were released after branch clicks.
    BranchRelease,
    /// Enter / Shift+Enter while the find bar is open: the next / previous match.
    FindStep(bool),
    /// A key pressed while recording that can't be part of a shortcut.
    RecordFailed(String),
}

/// Installs the listener. Call once and keep the returned handle.
pub fn listen() -> Eval {
    document::eval(LISTENER)
}

/// Waits for the next shortcut. `None` once the listener is gone.
pub async fn next(listener: &mut Eval) -> Option<Event> {
    loop {
        let name = listener.recv::<String>().await.ok()?;
        if name == "branch-release" {
            return Some(Event::BranchRelease);
        }
        if name == "find-next" || name == "find-prev" {
            return Some(Event::FindStep(name == "find-next"));
        }
        if name == "record-cancel" {
            return Some(Event::RecordCancelled);
        }
        if let Some(text) = name.strip_prefix("record:") {
            return Some(match Combo::parse(text) {
                Some(combo) => Event::Recorded(combo),
                None => Event::RecordFailed(format!(
                    "{} can't be used in a shortcut.",
                    text.replace('+', " + ")
                )),
            });
        }
        if let Some(action) = Action::from_id(&name) {
            return Some(Event::Action(action));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(text: &str) -> Combo {
        Combo::parse(text).unwrap()
    }

    #[test]
    fn combos_parse_and_print() {
        let combo = c("ctrl + shift + w");
        assert_eq!(combo.to_string(), "Ctrl+Shift+W");
        assert_eq!(combo.display(), "Ctrl + Shift + W");
        assert_eq!(combo.key_count(), 3);
        // Modifiers are always printed in one order.
        assert_eq!(c("Shift+Alt+Ctrl+1").to_string(), "Ctrl+Alt+Shift+1");
        assert_eq!(c("Cmd+K"), c("Ctrl+K"));
        assert_eq!(c("F11").key_count(), 1);
        assert_eq!(c("Escape").display(), "Esc");
        assert_eq!(c("Escape").to_string(), "Escape");
        assert_eq!(c("ctrl+pageup").to_string(), "Ctrl+PageUp");
        assert_eq!(c("Alt+f12").to_string(), "Alt+F12");
        assert_eq!(c("Esc").to_string(), "Escape");
        for bad in [
            "",
            "Ctrl+",
            "Ctrl+Shift",
            "Hyper+K",
            "Ctrl+Page Up",
            "+",
            "Ctrl+Hyper",
            "F0",
            "F25",
            "F011",
            "Ctrl+É",
            "Ctrl+Foo",
        ] {
            assert_eq!(Combo::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn defaults_are_valid_and_distinct() {
        let binds = Keybinds::default();
        let mut seen = HashSet::new();
        for action in Action::all() {
            assert_eq!(Action::from_id(&action.id()), Some(action));
            for combo in binds.get(action) {
                assert!(check(action, combo).is_ok(), "{combo}");
                assert!(seen.insert(combo.clone()), "{combo} twice");
            }
        }
        assert_eq!(Action::all().len(), 63);
        assert_eq!(binds.action_for(&c("Ctrl+L")), Some(Action::Load));
        assert_eq!(binds.action_for(&c("Ctrl+Shift+L")), Some(Action::Follow));
        assert_eq!(binds.action_for(&c("Ctrl+Shift+M")), Some(Action::Merge));
        assert_eq!(binds.action_for(&c("Ctrl+Shift+E")), Some(Action::Export));
        for (combo, action) in [
            ("Ctrl+Equal", Action::ZoomIn),
            ("Ctrl+Shift+Equal", Action::ZoomIn),
            ("Ctrl+NumpadAdd", Action::ZoomIn),
            ("Ctrl+Minus", Action::ZoomOut),
            ("Ctrl+NumpadSubtract", Action::ZoomOut),
            ("Ctrl+Shift+0", Action::ZoomReset),
            ("ArrowUp", Action::PanUp),
            ("ArrowLeft", Action::PanLeft),
        ] {
            assert_eq!(binds.action_for(&c(combo)), Some(action), "{combo}");
        }
        assert_eq!(
            binds.action_for(&c("Ctrl+Shift+N")),
            Some(Action::TabToNewWindow)
        );
        assert_eq!(binds.action_for(&c("F1")), Some(Action::Help));
        // Ctrl+W sets the value, Ctrl+Shift+W empties.
        assert_eq!(binds.action_for(&c("Ctrl+W")), Some(Action::SetValue));
        assert_eq!(
            binds.action_for(&c("Ctrl+Shift+W")),
            Some(Action::EmptyNode)
        );
        assert_eq!(binds.action_for(&c("Escape")), Some(Action::CloseOverlays));
    }

    #[test]
    fn single_keys_are_refused() {
        let mut binds = Keybinds::default();
        assert!(binds.assign(Action::Save, None, c("K")).is_err());
        assert!(binds.assign(Action::Save, None, c("F2")).is_err());
        assert!(binds.assign(Action::Save, None, c("Escape")).is_err());
        assert!(
            binds
                .assign(Action::CloseOverlays, None, c("Ctrl+K"))
                .is_err()
        );
        // A single-key default can come back, but not move to another action.
        binds.remove(Action::ToggleFullscreen, 0);
        assert!(
            binds
                .assign(Action::ToggleFullscreen, None, c("F11"))
                .is_ok()
        );
        assert!(binds.assign(Action::Save, None, c("F11")).is_err());
        assert_eq!(binds, Keybinds::default());
        // Two keys are enough, even Shift + a letter.
        assert_eq!(binds.assign(Action::Save, None, c("Shift+K")), Ok(None));
        assert_eq!(binds.get(Action::Save), [c("Ctrl+S"), c("Shift+K")]);
    }

    #[test]
    fn assigning_takes_a_combo_from_its_old_action() {
        let mut binds = Keybinds::default();
        // Replace Save's Ctrl+S with Ctrl+B, which Next node had.
        assert_eq!(
            binds.assign(Action::Save, Some(0), c("Ctrl+B")),
            Ok(Some(Action::NextBranch))
        );
        assert_eq!(binds.get(Action::Save), [c("Ctrl+B")]);
        assert!(binds.get(Action::NextBranch).is_empty());
        assert_eq!(binds.action_for(&c("Ctrl+S")), None);
        // Assigning a combo the action already has in another slot just drops the slot.
        binds.assign(Action::Redo, Some(1), c("Ctrl+Y")).unwrap();
        assert_eq!(binds.get(Action::Redo), [c("Ctrl+Y")]);
        // Reset gives the combos back, taking them from where they went.
        assert_eq!(binds.reset(Action::NextBranch), vec![Action::Save]);
        assert_eq!(binds.get(Action::NextBranch), [c("Ctrl+B")]);
        assert!(binds.get(Action::Save).is_empty());
        assert!(!binds.is_default(Action::Save));
        binds.reset(Action::Save);
        binds.reset(Action::Redo);
        assert_eq!(binds, Keybinds::default());
    }

    #[test]
    fn file_round_trip() {
        let mut binds = Keybinds::default();
        binds
            .assign(Action::Fearless, Some(0), c("Ctrl+Alt+F"))
            .unwrap();
        binds.remove(Action::Parent, 0);
        binds
            .assign(Action::ToggleRole(2), None, c("Ctrl+Shift+3"))
            .unwrap();
        let text = binds.to_json();
        assert!(text.contains("\"fearless\": [\n      \"Ctrl+Alt+F\"\n    ]"));
        assert!(text.contains("\"parent\": []"));
        assert_eq!(Keybinds::parse(&text), Ok((binds, vec![])));
    }

    #[test]
    fn file_problems_are_skipped() {
        let text = r#"{"bindings": {
            "save": "Ctrl+Alt+S",
            "next": ["Ctrl+Alt+S", "K", "Ctrl+Hyper"],
            "nope": ["Ctrl+N"],
            "escape": ["Ctrl+X"],
            "undo": [5],
            "set-value": ["Ctrl+Z"]
        }}"#;
        let (binds, problems) = Keybinds::parse(text).unwrap();
        // Next node comes before Save in the editor order, so it keeps the duplicate.
        assert_eq!(binds.get(Action::NextBranch), [c("Ctrl+Alt+S")]);
        assert!(binds.get(Action::Save).is_empty());
        assert!(binds.get(Action::Undo).is_empty());
        assert_eq!(binds.get(Action::SetValue), [c("Ctrl+Z")]);
        // Missing actions get their defaults.
        assert_eq!(binds.get(Action::Fearless), [c("Ctrl+G")]);
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert!(Keybinds::parse("not json").is_err());
        assert_eq!(
            Keybinds::parse("{}").unwrap(),
            (Keybinds::default(), vec![])
        );
    }

    #[test]
    fn missing_defaults_never_clash() {
        // Next node takes Ctrl+W, so Set value (missing from the file) can't have it.
        let (binds, problems) = Keybinds::parse(r#"{"bindings": {"next": ["Ctrl+W"]}}"#).unwrap();
        assert!(problems.is_empty());
        assert_eq!(binds.get(Action::NextBranch), [c("Ctrl+W")]);
        assert!(binds.get(Action::SetValue).is_empty());
    }

    #[test]
    fn help_rows_condense_numbered_shortcuts() {
        let mut binds = Keybinds::default();
        let rows = binds.help_rows();
        let find = |rows: &[(Vec<String>, String)], what: &str| {
            rows.iter().find(|(_, w)| w.contains(what)).cloned()
        };
        assert_eq!(
            find(&rows, "first nine").unwrap().0,
            vec!["Ctrl + 1 … 9".to_string()]
        );
        assert_eq!(find(&rows, "Support tag").unwrap().0, vec!["Alt + 1 … 5"]);
        assert_eq!(
            find(&rows, "Redo").unwrap().0,
            vec!["Ctrl + Y", "Ctrl + Shift + Z"]
        );
        assert_eq!(rows.len(), Action::all().len() - 8 - 4);
        // Breaking the pattern lists them one by one.
        binds
            .assign(Action::ToggleRole(4), Some(0), c("Alt+S"))
            .unwrap();
        let rows = binds.help_rows();
        assert!(find(&rows, "Support tag").unwrap().0 == vec!["Alt + S"]);
        assert!(find(&rows, "Top tag").is_some());
    }

    #[test]
    fn branch_click_modifiers() {
        assert_eq!(ClickMods::parse("shift"), Some(ClickMods::SHIFT));
        let ctrl_shift = ClickMods::parse("Shift + Ctrl").unwrap();
        assert_eq!(ctrl_shift.id(), "Ctrl+Shift");
        assert_eq!(ctrl_shift.display(), "Ctrl + Shift");
        for bad in ["", "K", "Ctrl+K", "Ctrl+Alt+Shift", "Hyper"] {
            assert_eq!(ClickMods::parse(bad), None, "{bad}");
        }
        for m in ClickMods::options() {
            assert_eq!(ClickMods::parse(&m.id()), Some(m));
        }
        // Extra modifiers still count; missing ones don't.
        assert!(ClickMods::SHIFT.held_in(false, false, true));
        assert!(ClickMods::SHIFT.held_in(true, false, true));
        assert!(!ClickMods::SHIFT.held_in(true, false, false));
        assert!(ctrl_shift.held_in(true, true, true));
        assert!(!ctrl_shift.held_in(false, false, true));
    }

    #[test]
    fn branch_click_in_the_file() {
        let mut binds = Keybinds::default();
        assert!(binds.to_json().contains("\"branch-click\": \"Shift\""));
        binds.branch_click = ClickMods::parse("Ctrl").unwrap();
        let text = binds.to_json();
        assert_eq!(Keybinds::parse(&text), Ok((binds.clone(), vec![])));
        assert_ne!(binds, Keybinds::default());
        // Missing means Shift; nonsense is reported and gives Shift.
        assert_eq!(
            Keybinds::parse("{}").unwrap().0.branch_click,
            ClickMods::SHIFT
        );
        let (b, problems) = Keybinds::parse(r#"{"branch-click": "Ctrl+K"}"#).unwrap();
        assert_eq!(b.branch_click, ClickMods::SHIFT);
        assert_eq!(problems.len(), 1);
    }
}
