//! The app's panels, each its own component. They share the app state through [`Ctx`]:
//! signals, memos and the actions as stable callbacks, provided once by `App`. A panel only
//! re-renders when something it reads changes, not whenever the app does.

pub mod groups_panel;
pub mod overlays;
pub mod pool;
pub mod share_bar;
pub mod teams;
pub mod top_bar;
pub mod tree_panel;

pub use crate::*;
pub use dioxus::prelude::*;
// The draft order's actions, not Dioxus's.
pub use crate::draft::Action;

/// The app state and actions the panels use. Every field is a signal, memo or callback, so
/// the struct is `Copy` and the same on every render.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub tree: Signal<DraftTree>,
    pub cursor: Signal<NodeId>,
    pub extra: Signal<Vec<NodeId>>,
    pub follow: Signal<Option<follow::Follow>>,
    pub tab_list: Signal<Vec<tabs::TabInfo>>,
    pub active_tab: Signal<u64>,
    pub parked: Signal<HashMap<u64, tabs::Parked>>,
    pub close_confirm: Signal<Option<u64>>,
    /// This window's number (see windows.rs).
    pub window_no: u64,
    /// A tab's right-click menu: the tab, and where it opened.
    pub tab_menu: Signal<Option<(u64, f64, f64)>>,
    /// The window's close button with tabs unsaved: how many, waiting for confirmation.
    pub window_close_confirm: Signal<Option<usize>>,
    pub search: Signal<String>,
    pub save_menu_open: Signal<bool>,
    pub save_selection: Signal<bool>,
    pub dialog_open: Signal<bool>,
    pub notice: Signal<Option<(u64, String)>>,
    /// The notice is sliding out (smooth animations).
    pub notice_leaving: Signal<bool>,
    pub warning: Signal<Option<String>>,
    pub merge_open: Signal<bool>,
    pub merge_files: Signal<Vec<PathBuf>>,
    pub merge_tabs: Signal<Vec<u64>>,
    pub error: Signal<Option<String>>,
    pub sel_groups: Signal<Vec<selection::SelectionGroup>>,
    pub filters_on: Signal<Vec<bool>>,
    pub filter_intersect: Signal<bool>,
    pub rgroup_filter: Signal<bool>,
    pub ui_settings: Signal<configio::UiSettings>,
    pub tree_size: Signal<(f64, f64)>,
    pub easter_eggs: Signal<bool>,
    pub fullscreen: Signal<bool>,
    pub view_menu_open: Signal<bool>,
    pub rgroup_editor_open: Signal<bool>,
    pub sel_editor_open: Signal<bool>,
    pub themes: Signal<Vec<theme::Theme>>,
    pub theme_preview: Signal<Option<theme::Theme>>,
    pub theme_menu_open: Signal<bool>,
    pub theme_editor_open: Signal<bool>,
    pub active_theme: Memo<theme::Theme>,
    pub share_open: Signal<bool>,
    pub share_draft: Signal<Option<(String, String, String)>>,
    pub help_open: Signal<bool>,
    pub keybinds: Signal<keys::Keybinds>,
    pub recording: Signal<Option<shortcuts_panel::RecordTarget>>,
    pub kb_message: Signal<Option<String>>,
    pub export_menu_open: Signal<bool>,
    pub export_kind: Signal<Option<export_panel::ExportKind>>,
    pub export_busy: Signal<bool>,
    pub fearless_open: Signal<bool>,
    pub role_popup: Signal<Option<role_popup::RolePopupState>>,
    pub edit_unreachable: Signal<bool>,
    pub pruned: Signal<Option<tree::Pruned>>,
    pub pruned_conflicts: Signal<Option<tree::Pruned>>,
    pub set_mode: Signal<bool>,
    pub group_colors: Signal<HashMap<u8, usize>>,
    pub pending_groups: Signal<Option<PendingGroups>>,
    pub groups_info_open: Signal<bool>,
    pub find_open: Signal<bool>,
    pub find_query: Signal<String>,
    pub find_options: Signal<find::FindOptions>,
    pub found: Memo<Vec<NodeId>>,
    pub preview: Signal<Option<(NodeId, f64, f64)>>,
    pub marked_only: Signal<bool>,
    pub restore_offer: Signal<Option<autosave::Crashed>>,
    /// The current tree as a `.dtt` string (with the selection when "Include selected
    /// node" is on), for the share bar. Empty while the bar is hidden: it is only built
    /// while shown.
    pub share_text: Memo<String>,
    /// Conflict and fearless marks, and what depends only on the tree.
    pub info: Memo<TreeInfo>,
    /// The graph's layout, per tree and "Marked only".
    pub graph: Memo<tree_view::CachedLayout>,
    /// The nodes the last edit added (smooth profile), for the graph's and the team
    /// panels' entrance animations.
    pub entering: Memo<tree_view::Entering>,
    /// Counter picks for the team panels (see `counters`).
    pub counter_data: Signal<std::rc::Rc<crate::counters::Counters>>,
    /// A counter data update in progress: requests done, of all.
    pub counter_update: Signal<Option<(usize, usize)>>,
    /// The row the graph's drawn window is centred on (see `tree_view::GraphWindow`).
    pub graph_center: Signal<Option<f64>>,
    /// The selected line and what the grid offers from it.
    pub line: Memo<LineInfo>,
    pub act: Act,
}

/// The actions the panels trigger, as callbacks that stay the same between renders (see
/// `use_callback`), so passing them around never makes a panel re-render.
#[derive(Clone, Copy)]
pub struct Act {
    pub follow_enter: Callback<u8>,
    pub show_notice: Callback<String>,
    /// Downloads the current patch's counter data (see `counters::download`).
    pub update_counters: Callback<()>,
    pub save_settings: Callback<()>,
    pub set_easter_eggs: Callback<bool>,
    pub set_hidden: Callback<(Panel, bool)>,
    pub set_fullscreen: Callback<bool>,
    pub choose: Callback<u8>,
    pub add_branch: Callback<u8>,
    pub find_step: Callback<bool>,
    pub hover_node: Callback<Option<(NodeId, f64, f64)>>,
    pub toggle_hidden: Callback<()>,
    pub show_all_hidden: Callback<()>,
    pub hide_unreachable: Callback<()>,
    pub switch_tab: Callback<u64>,
    pub new_tab: Callback<()>,
    pub close_tab: Callback<u64>,
    pub request_close: Callback<u64>,
    /// Moves a tab to another window (by number), or to a new one.
    pub move_tab: Callback<(u64, Option<u64>)>,
    /// Closes this window (its tabs and their autosaves).
    pub close_window: Callback<()>,
    pub start_follow: Callback<()>,
    pub leave_follow: Callback<()>,
    pub follow_back: Callback<()>,
    pub toggle_select: Callback<NodeId>,
    pub delete_node: Callback<NodeId>,
    pub reset: Callback<()>,
    pub open_roles: Callback<(NodeId, f64, f64)>,
    pub close_roles: Callback<()>,
    pub set_roles: Callback<u8>,
    pub apply_fearless: Callback<fearless::Fearless>,
    pub remove_unreachable: Callback<()>,
    pub restore_removed: Callback<()>,
    pub remove_conflicts: Callback<()>,
    pub restore_conflicts: Callback<()>,
    pub save_theme: Callback<(theme::Theme, bool)>,
    /// `load_tree(loaded, stem, prefix, source, saved)`.
    #[allow(clippy::type_complexity)]
    pub load_tree: Callback<(save::Loaded, String, Option<String>, Option<PathBuf>, bool)>,
    pub save_as: Callback<SaveFormat>,
    pub load_file: Callback<()>,
    pub run_merge: Callback<()>,
    pub commit_groups: Callback<PendingGroups>,
    pub save_rgroups: Callback<(Vec<groups::Group>, bool)>,
    #[allow(clippy::type_complexity)]
    pub save_selection_groups: Callback<(Vec<selection::SelectionGroup>, Vec<Option<usize>>, bool)>,
    pub copy_to_selection: Callback<groups::Group>,
    /// A selection group (and whether to leave out group tiles) into the representational
    /// groups.
    pub copy_to_representational: Callback<(selection::SelectionGroup, bool)>,
    pub toggle_selection_presets: Callback<()>,
    pub toggle_rgroup_presets: Callback<()>,
    pub load_groups_file: Callback<()>,
    pub on_share_input: Callback<String>,
}

/// What depends only on the tree: its conflict and fearless marks and their counts.
#[derive(Clone, PartialEq)]
pub struct TreeInfo {
    pub marks: tree::Marks,
    pub unreachable_count: usize,
    pub conflict_count: usize,
    /// How many topmost unreachable nodes have branches to hide.
    pub unreachable_tops: usize,
    /// "Hide unreachable" is on: the branches below every one of them are hidden.
    pub unreachable_hidden: bool,
    pub fearless: fearless::Fearless,
}

impl TreeInfo {
    pub fn compute(t: &DraftTree) -> TreeInfo {
        let marks = t.marks();
        let tops = t.unreachable_tops_in(&marks.unreachable);
        let unreachable_hidden = !tops.is_empty()
            && tops
                .iter()
                .all(|&n| t.get(n).is_some_and(|n| n.is_collapsed()));
        TreeInfo {
            unreachable_count: marks.unreachable.len(),
            conflict_count: marks.conflicts.len(),
            marks,
            unreachable_tops: tops.len(),
            unreachable_hidden,
            fearless: *t.fearless(),
        }
    }
}

/// The selected line (or, in follow mode, the entered draft) and what the grid offers from
/// it.
#[derive(Clone, PartialEq)]
pub struct LineInfo {
    /// Values chosen so far, in draft order (root excluded).
    pub values: Vec<u8>,
    /// Nodes on the line (root excluded) and their role tags.
    pub path_ids: Vec<NodeId>,
    pub path_roles: Vec<u8>,
    /// Per team, per role, the values its role slot shows.
    pub role_slots: [Vec<Vec<u8>>; 2],
    /// Filled steps.
    pub step: usize,
    pub following: bool,
    /// Follow mode: the values prepared for the next step.
    pub follow_options: Vec<u8>,
    /// Set mode on a node other than the root: the grid offers values for the selected
    /// node itself, from its parent.
    pub editing: bool,
    pub grid_base: NodeId,
    pub grid_step: usize,
    /// Values that already have a branch from the grid's state (in follow mode: the values
    /// prepared for the next step).
    pub branches: Vec<u8>,
    /// The selected node is in a branch that fearless rules out.
    pub cursor_unreachable: bool,
    /// The node where the selected line conflicts, if it does.
    pub cursor_conflict: Option<NodeId>,
    /// New branches may be added below the selected node.
    pub can_extend: bool,
    /// The grid's tiles do something (add, set, or enter in follow mode).
    pub grid_open: bool,
    /// Banner names: the node being set, and where the selected line conflicts.
    pub set_target: String,
    pub cursor_conflict_name: Option<String>,
    /// The "nothing" tile: EMPTY at picks, NO_BAN at bans.
    pub wildcard: u8,
}

impl LineInfo {
    pub fn compute(
        t: &DraftTree,
        cursor: NodeId,
        follow: Option<&follow::Follow>,
        set_mode: bool,
        edit_unreachable: bool,
    ) -> LineInfo {
        let LineData {
            values,
            ids: path_ids,
            tags: path_roles,
            role_slots,
        } = match follow {
            Some(f) => follow_line(t, f),
            None => line_data(t, cursor),
        };
        let step = values.len();
        let following = follow.is_some();
        let follow_options: Vec<u8> = follow.map(|f| f.options(t)).unwrap_or_default();
        let editing = set_mode && cursor != t.root();
        let grid_base = if editing {
            t.get(cursor).and_then(|n| n.parent()).unwrap_or(cursor)
        } else {
            cursor
        };
        let grid_step = if editing { step - 1 } else { step };
        let branches: Vec<u8> = if following {
            follow_options.clone()
        } else {
            t.get(grid_base)
                .map(|n| {
                    n.children()
                        .iter()
                        .map(|&c| t.get(c).unwrap().value())
                        .collect()
                })
                .unwrap_or_default()
        };
        let current = DRAFT_ORDER.get(step);
        // From the line alone: reading the marks (`TreeInfo`) here would compute them a second
        // time per edit (see `App`).
        let cursor_unreachable = t.line_unreachable(cursor);
        let cursor_conflict = t.line_conflict(cursor);
        let can_extend = current.is_some()
            && cursor_conflict.is_none()
            && (!cursor_unreachable || edit_unreachable);
        let grid_open = if following {
            current.is_some()
        } else if editing {
            true
        } else {
            can_extend
        };
        let node_name = |id: NodeId| {
            let Some(node) = t.get(id) else {
                return String::new();
            };
            let step = DRAFT_ORDER[t.path(id).len() - 2];
            format!(
                "{} ({} {} {})",
                value_name(t.groups(), node.value()),
                step.team.label(),
                step.action.label(),
                step.number
            )
        };
        let set_target = if editing {
            node_name(cursor)
        } else {
            String::new()
        };
        let cursor_conflict_name = cursor_conflict.map(node_name);
        let wildcard = match DRAFT_ORDER.get(grid_step) {
            Some(s) if s.action == Action::Ban => NO_BAN,
            _ => EMPTY,
        };
        LineInfo {
            values,
            path_ids,
            path_roles,
            role_slots,
            step,
            following,
            follow_options,
            editing,
            grid_base,
            grid_step,
            branches,
            cursor_unreachable,
            cursor_conflict,
            can_extend,
            grid_open,
            set_target,
            cursor_conflict_name,
            wildcard,
        }
    }

    /// The values the grid works from: the line, without the selected node in set mode.
    pub fn grid_values(&self) -> &[u8] {
        if self.editing {
            &self.values[..self.step - 1]
        } else {
            &self.values
        }
    }

    pub fn wildcard_label(&self) -> &'static str {
        if self.wildcard == NO_BAN {
            "No ban"
        } else {
            "Empty"
        }
    }
}

/// Whether a panel button is shown (Help → Button settings).
pub fn show(settings: &configio::UiSettings, b: buttons::Button) -> bool {
    buttons::shown(&settings.buttons, b)
}

/// Makes tooltips name the current shortcuts, and re-renders the calling panel when they
/// change (panels don't re-render with `App`).
pub fn use_shortcut_hints(ctx: &Ctx) {
    keys::set_current(&ctx.keybinds.read());
}
