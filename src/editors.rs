//! Simple GUI editors for representational groups and selection groups.
//!
//! Each editor works on a draft copy and hands the result to `App` through `on_save`;
//! writing the file and applying the change happen there, where the tree and filters live.

use dioxus::prelude::*;

use crate::champions::{self, CHAMPIONS};
use crate::groups::{self, Group, MIN_RGROUP_CHAMPIONS, USER_FIRST};
use crate::roles::Role;
use crate::selection::{self, SelectionGroup, ValueSet};
use crate::value_view::{RoleGlyph, ValueIcon, ViewContext};

const MAX_NAME_CHARS: usize = 16;
const MAX_NOTE_CHARS: usize = 200;
fn champion_count(members: &ValueSet) -> usize {
    members
        .values()
        .filter(|&v| champions::by_value(v).is_some())
        .count()
}

fn name_problem(name: &str) -> Option<String> {
    let chars = name.trim().chars().count();
    (chars == 0 || chars > MAX_NAME_CHARS)
        .then(|| format!("Name must be 1-{MAX_NAME_CHARS} characters."))
}

fn note_problem(note: &str) -> Option<String> {
    (note.chars().count() > MAX_NOTE_CHARS)
        .then(|| format!("Note must be at most {MAX_NOTE_CHARS} characters."))
}

/// Click-to-toggle grid of champions (and optionally group tiles) with its own search box.
#[component]
pub fn MemberPicker(
    members: ValueSet,
    include_groups: bool,
    on_toggle: EventHandler<u8>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let mut search = use_signal(String::new);
    let query = search.read().to_lowercase();

    let mut tiles: Vec<(u8, String)> = Vec::new();
    if include_groups {
        let tree = ctx.tree.read();
        for g in tree.groups().all() {
            if g.name.to_lowercase().contains(&query) || g.label.to_lowercase().contains(&query) {
                tiles.push((g.value, g.name.clone()));
            }
        }
    }
    let mut champs: Vec<(u8, String)> = CHAMPIONS
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name.to_lowercase().contains(&query))
        .map(|(i, c)| (champions::value_of(i), c.name.to_string()))
        .collect();
    champs.sort_by(|a, b| a.1.cmp(&b.1));
    tiles.extend(champs);

    let placeholder = if include_groups {
        "Search champions and groups..."
    } else {
        "Search champions..."
    };

    rsx! {
        div { class: "picker",
            input {
                class: "search picker-search",
                r#type: "text",
                placeholder,
                value: "{search}",
                oninput: move |e| search.set(e.value()),
            }
            div { class: "picker-grid",
                for (value, name) in tiles {
                    button {
                        key: "{value}",
                        class: if members.contains(value) { "pick-tile selected" } else { "pick-tile" },
                        title: "{name}",
                        onclick: move |_| on_toggle.call(value),
                        div { class: "pick-icon", ValueIcon { value } }
                    }
                }
            }
        }
    }
}

/// Footer shared by both editors: problems, Save, Save as, Cancel with a discard check.
#[component]
fn EditorFooter(
    problems: Vec<String>,
    dirty: bool,
    on_save: EventHandler<bool>,
    on_close: EventHandler<()>,
) -> Element {
    let mut confirm_discard = use_signal(|| false);
    let can_save = problems.is_empty();
    rsx! {
        div { class: "editor-footer",
            div { class: "editor-problems",
                if let Some(first) = problems.first() {
                    span { class: "share-status", "{first}" }
                    if problems.len() > 1 {
                        span { class: "muted", " (+{problems.len() - 1} more)" }
                    }
                }
            }
            if confirm_discard() {
                span { class: "muted", "Discard unsaved changes?" }
                button { class: "btn", onclick: move |_| confirm_discard.set(false), "Keep editing" }
                button { class: "btn primary", onclick: move |_| on_close.call(()), "Discard" }
            } else {
                button {
                    class: "btn",
                    onclick: move |_| {
                        if dirty {
                            confirm_discard.set(true);
                        } else {
                            on_close.call(());
                        }
                    },
                    "Cancel"
                }
                button {
                    class: "btn",
                    disabled: !can_save,
                    title: "Save to another file",
                    onclick: move |_| on_save.call(true),
                    "Save as…"
                }
                button {
                    class: "btn primary",
                    disabled: !can_save,
                    onclick: move |_| on_save.call(false),
                    "Save"
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct RepItem {
    value: u8,
    name: String,
    note: String,
    members: ValueSet,
    /// Role lock (`0` = none).
    roles: u8,
}

fn rep_problem(item: &RepItem) -> Option<String> {
    name_problem(&item.name)
        .or_else(|| note_problem(&item.note))
        .or_else(|| {
            (champion_count(&item.members) < MIN_RGROUP_CHAMPIONS)
                .then(|| format!("Needs at least {MIN_RGROUP_CHAMPIONS} champions."))
        })
}

/// Editor for user representational groups (values 190-255).
#[component]
pub fn RepGroupEditor(
    initial: Vec<Group>,
    file_label: String,
    on_save: EventHandler<(Vec<Group>, bool)>,
    /// Copy this group (as edited) into the selection groups.
    on_copy: EventHandler<Group>,
    /// Load the groups from another config file instead (closes the editor).
    on_load: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let start: Vec<RepItem> = initial
        .iter()
        .map(|g| {
            let mut members = ValueSet::default();
            for &m in &g.members {
                members.insert(m);
            }
            RepItem {
                value: g.value,
                name: g.name.clone(),
                note: g.note.clone(),
                members,
                roles: g.roles,
            }
        })
        .collect();
    let original = use_hook(|| start.clone());
    let mut items = use_signal(|| start);
    let mut selected = use_signal(|| (!original.is_empty()).then_some(0usize));

    let list = items.read().clone();
    let problems: Vec<String> = list
        .iter()
        .filter_map(|it| {
            rep_problem(it)
                .map(|p| format!("{} (G{}): {p}", it.name.trim(), it.value - USER_FIRST + 1))
        })
        .collect();
    let dirty = list != original;
    let free_value = (USER_FIRST..=u8::MAX).find(|v| !list.iter().any(|it| it.value == *v));
    let current = selected().filter(|&i| i < list.len());

    let save = move |save_as: bool| {
        let groups = items
            .read()
            .iter()
            .map(|it| {
                Group::new_user(
                    it.value,
                    it.name.trim().to_string(),
                    it.note.clone(),
                    it.members.values().collect(),
                )
                .with_roles(it.roles)
            })
            .collect();
        on_save.call((groups, save_as));
    };

    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal editor", onclick: move |e| e.stop_propagation(),
                h2 { class: "help-title", "Edit representational groups" }
                div { class: "editor-head-row",
                    p { class: "help-note", "Saving writes {file_label} (the previous version is kept as .bak). Groups need at least {MIN_RGROUP_CHAMPIONS} champions." }
                    button {
                        class: "btn small",
                        disabled: dirty,
                        title: if dirty { "Save or cancel your changes first" } else { "Load user groups from another config file" },
                        onclick: move |_| on_load.call(()),
                        "Load RGroup config…"
                    }
                }
                div { class: "editor-body",
                    div { class: "editor-list",
                        div { class: "editor-list-items",
                            for (i, it) in list.iter().enumerate() {
                                button {
                                    key: "{it.value}",
                                    class: if current == Some(i) { "editor-item active" } else { "editor-item" },
                                    onclick: move |_| selected.set(Some(i)),
                                    span { class: "editor-item-label", "G{it.value - USER_FIRST + 1}" }
                                    span { class: "editor-item-name", "{it.name}" }
                                    if rep_problem(it).is_some() {
                                        span { class: "editor-item-warn", title: "Needs fixing before saving", "!" }
                                    }
                                }
                            }
                        }
                        div { class: "editor-list-actions",
                            button {
                                class: "btn small",
                                disabled: free_value.is_none(),
                                title: if free_value.is_none() { "All 66 user group values are in use" } else { "Add a new group" },
                                onclick: move |_| {
                                    if let Some(value) = free_value {
                                        let mut list = items.write();
                                        list.push(RepItem {
                                            value,
                                            name: "New group".into(),
                                            note: String::new(),
                                            members: ValueSet::default(),
                                            roles: 0,
                                        });
                                        selected.set(Some(list.len() - 1));
                                    }
                                },
                                "+ New"
                            }
                            button {
                                class: "btn small",
                                disabled: current.is_none(),
                                onclick: move |_| {
                                    if let Some(i) = current {
                                        items.write().remove(i);
                                        let len = items.read().len();
                                        selected.set((len > 0).then(|| i.min(len - 1)));
                                    }
                                },
                                "Delete"
                            }
                        }
                    }
                    div { class: "editor-form",
                        match current {
                            Some(i) => {
                                let it = list[i].clone();
                                rsx! {
                                    div { class: "editor-fields",
                                        label { class: "editor-field",
                                            span { "Name" }
                                            input {
                                                class: "search",
                                                value: "{it.name}",
                                                maxlength: "{MAX_NAME_CHARS}",
                                                oninput: move |e| items.write()[i].name = e.value(),
                                            }
                                        }
                                        label { class: "editor-field",
                                            span { "Note" }
                                            input {
                                                class: "search",
                                                value: "{it.note}",
                                                maxlength: "{MAX_NOTE_CHARS}",
                                                oninput: move |e| items.write()[i].note = e.value(),
                                            }
                                        }
                                        div { class: "editor-field",
                                            span { "Roles" }
                                            div { class: "role-lock",
                                                for role in Role::ALL {
                                                    button {
                                                        key: "{role.key()}",
                                                        class: if it.roles & role.bit() != 0 { "role-chip active" } else { "role-chip" },
                                                        title: "Lock this group's nodes to {role.label()} (with any other locked roles)",
                                                        onclick: move |_| items.write()[i].roles ^= role.bit(),
                                                        RoleGlyph { role }
                                                        "{role.label()}"
                                                    }
                                                }
                                                span { class: "muted role-lock-note",
                                                    if it.roles == 0 { "No lock: any role" } else { "Group nodes start with these tags and can't get others" }
                                                }
                                            }
                                        }
                                        div { class: "muted", "G{it.value - USER_FIRST + 1} (value {it.value}) · {champion_count(&it.members)} champion(s) selected" }
                                        {
                                            let problem = rep_problem(&it);
                                            let copy = it.clone();
                                            rsx! {
                                                button {
                                                    class: "btn small editor-copy",
                                                    disabled: problem.is_some(),
                                                    title: match &problem {
                                                        Some(p) => format!("Fix this group first: {p}"),
                                                        None => "Add a copy of this group (as shown, saved or not) to the selection groups. The role lock is not copied.".to_string(),
                                                    },
                                                    onclick: move |_| {
                                                        on_copy.call(
                                                            Group::new_user(
                                                                copy.value,
                                                                copy.name.trim().to_string(),
                                                                copy.note.clone(),
                                                                copy.members.values().collect(),
                                                            ),
                                                        )
                                                    },
                                                    "Copy to selection groups"
                                                }
                                            }
                                        }
                                    }
                                    MemberPicker {
                                        members: it.members,
                                        include_groups: false,
                                        on_toggle: move |v| items.write()[i].members.toggle(v),
                                    }
                                }
                            }
                            None => rsx! {
                                div { class: "muted editor-empty", "No group selected. Use + New to add one." }
                            },
                        }
                    }
                }
                EditorFooter { problems, dirty, on_save: save, on_close }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SelItem {
    name: String,
    note: String,
    members: ValueSet,
    /// Index in the saved list, for keeping filter toggles; `None` for new groups.
    origin: Option<usize>,
    /// A built-in role filter: can't be renamed or deleted.
    protected: bool,
}

fn sel_problem(item: &SelItem) -> Option<String> {
    name_problem(&item.name)
        .or_else(|| note_problem(&item.note))
        .or_else(|| {
            (!item.protected && selection::is_protected_name(&item.name)).then(|| {
                format!(
                    "\"{}\" is reserved for the built-in filter.",
                    item.name.trim()
                )
            })
        })
        .or_else(|| {
            (!item.protected && item.members.is_empty())
                .then(|| "Select at least one member.".into())
        })
}

/// Editor for selection groups. The built-in role filters can be edited and moved but not
/// renamed or deleted.
#[component]
pub fn SelectionEditor(
    initial: Vec<SelectionGroup>,
    file_label: String,
    on_save: EventHandler<(Vec<SelectionGroup>, Vec<Option<usize>>, bool)>,
    /// Copy this group (as edited) into the representational groups; the flag leaves
    /// group tiles out instead of replacing them with their champions.
    on_copy: EventHandler<(SelectionGroup, bool)>,
    on_close: EventHandler<()>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let mut omit_groups = use_signal(|| false);
    let start: Vec<SelItem> = initial
        .iter()
        .enumerate()
        .map(|(i, g)| SelItem {
            name: g.name.clone(),
            note: g.note.clone(),
            members: g.members,
            origin: Some(i),
            protected: selection::is_protected_name(&g.name),
        })
        .collect();
    let original = use_hook(|| start.clone());
    let mut items = use_signal(|| start);
    let mut selected = use_signal(|| (!original.is_empty()).then_some(0usize));

    let list = items.read().clone();
    let problems: Vec<String> = list
        .iter()
        .filter_map(|it| sel_problem(it).map(|p| format!("{}: {p}", it.name.trim())))
        .collect();
    let dirty = list != original;
    let current = selected().filter(|&i| i < list.len());
    let len = list.len();

    let save = move |save_as: bool| {
        let list = items.read();
        let groups = list
            .iter()
            .map(|it| SelectionGroup {
                name: it.name.trim().to_string(),
                note: it.note.clone(),
                members: it.members,
            })
            .collect();
        let origins = list.iter().map(|it| it.origin).collect();
        on_save.call((groups, origins, save_as));
    };

    // Members referring to groups the loaded config doesn't define are kept but not shown.
    let hidden_members = current.map_or(0, |i| {
        let tree = ctx.tree.read();
        list[i]
            .members
            .values()
            .filter(|&v| groups::is_group(v) && tree.groups().get(v).is_none())
            .count()
    });

    let mut swap = move |a: usize, b: usize| {
        items.write().swap(a, b);
        selected.set(Some(b));
    };

    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal editor", onclick: move |e| e.stop_propagation(),
                h2 { class: "help-title", "Edit selection groups" }
                p { class: "help-note", "Saving writes {file_label} (the previous version is kept as .bak) and applies straight away. Top, Jungle, Mid, ADC and Support can be edited and moved but not renamed or deleted." }
                div { class: "editor-body",
                    div { class: "editor-list",
                        div { class: "editor-list-items",
                            for (i, it) in list.iter().enumerate() {
                                button {
                                    key: "{i}",
                                    class: if current == Some(i) { "editor-item active" } else { "editor-item" },
                                    onclick: move |_| selected.set(Some(i)),
                                    span { class: "editor-item-label", {if i < 9 { crate::keys::hint(crate::keys::Action::ToggleFilter(i)).unwrap_or_default() } else { String::new() }} }
                                    span { class: "editor-item-name", "{it.name}" }
                                    if it.protected {
                                        span { class: "editor-item-lock", title: "Built-in filter", "🔒" }
                                    }
                                    if sel_problem(it).is_some() {
                                        span { class: "editor-item-warn", title: "Needs fixing before saving", "!" }
                                    }
                                }
                            }
                        }
                        div { class: "editor-list-actions",
                            button {
                                class: "btn small",
                                onclick: move |_| {
                                    let mut list = items.write();
                                    list.push(SelItem {
                                        name: "New filter".into(),
                                        note: String::new(),
                                        members: ValueSet::default(),
                                        origin: None,
                                        protected: false,
                                    });
                                    selected.set(Some(list.len() - 1));
                                },
                                "+ New"
                            }
                            button {
                                class: "btn small",
                                disabled: current.is_none_or(|i| list[i].protected),
                                title: "Built-in filters can't be deleted",
                                onclick: move |_| {
                                    if let Some(i) = current {
                                        items.write().remove(i);
                                        let len = items.read().len();
                                        selected.set((len > 0).then(|| i.min(len - 1)));
                                    }
                                },
                                "Delete"
                            }
                            button {
                                class: "btn small",
                                disabled: current.is_none_or(|i| i == 0),
                                title: "Move up",
                                onclick: move |_| {
                                    if let Some(i) = current.filter(|&i| i > 0) {
                                        swap(i, i - 1);
                                    }
                                },
                                "▲"
                            }
                            button {
                                class: "btn small",
                                disabled: current.is_none_or(|i| i + 1 >= len),
                                title: "Move down",
                                onclick: move |_| {
                                    if let Some(i) = current.filter(|&i| i + 1 < len) {
                                        swap(i, i + 1);
                                    }
                                },
                                "▼"
                            }
                        }
                    }
                    div { class: "editor-form",
                        match current {
                            Some(i) => {
                                let it = list[i].clone();
                                rsx! {
                                    div { class: "editor-fields",
                                        label { class: "editor-field",
                                            span { "Name" }
                                            input {
                                                class: "search",
                                                value: "{it.name}",
                                                maxlength: "{MAX_NAME_CHARS}",
                                                disabled: it.protected,
                                                title: if it.protected { "Built-in filters can't be renamed" } else { "" },
                                                oninput: move |e| items.write()[i].name = e.value(),
                                            }
                                        }
                                        label { class: "editor-field",
                                            span { "Note" }
                                            input {
                                                class: "search",
                                                value: "{it.note}",
                                                maxlength: "{MAX_NOTE_CHARS}",
                                                oninput: move |e| items.write()[i].note = e.value(),
                                            }
                                        }
                                        div { class: "muted",
                                            "{it.members.values().count() - hidden_members} member(s) selected"
                                            if hidden_members > 0 {
                                                " · {hidden_members} more refer to groups not in the loaded config (kept)"
                                            }
                                        }
                                        {
                                            let problem = name_problem(&it.name).or_else(|| note_problem(&it.note));
                                            let copy = SelectionGroup {
                                                name: it.name.trim().to_string(),
                                                note: it.note.clone(),
                                                members: it.members,
                                            };
                                            rsx! {
                                                div { class: "editor-copy-row",
                                                    button {
                                                        class: "btn small editor-copy",
                                                        disabled: problem.is_some(),
                                                        title: match &problem {
                                                            Some(p) => format!("Fix this filter first: {p}"),
                                                            None => "Add a copy of this filter (as shown, saved or not) to the representational groups, at the first free G number.".to_string(),
                                                        },
                                                        onclick: move |_| on_copy.call((copy.clone(), omit_groups())),
                                                        "Copy to representational groups"
                                                    }
                                                    label { class: "export-check",
                                                        title: "Representational groups can't contain groups. Normally each group tile is replaced by its champions; tick this to leave group tiles out instead.",
                                                        input {
                                                            r#type: "checkbox",
                                                            checked: omit_groups(),
                                                            onchange: move |e| omit_groups.set(e.checked()),
                                                        }
                                                        "Leave group tiles out"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    MemberPicker {
                                        members: it.members,
                                        include_groups: true,
                                        on_toggle: move |v| items.write()[i].members.toggle(v),
                                    }
                                }
                            }
                            None => rsx! {
                                div { class: "muted editor-empty", "No filter selected." }
                            },
                        }
                    }
                }
                EditorFooter { problems, dirty, on_save: save, on_close }
            }
        }
    }
}
