//! The draft tree panel: header buttons, follow and find bars, and the graph.

use super::*;

#[component]
pub fn TreePanel() -> Element {
    let ctx = use_context::<Ctx>();
    use_shortcut_hints(&ctx);
    let Ctx {
        tree,
        mut cursor,
        mut extra,
        mut follow,
        mut ui_settings,
        tree_size,
        mut edit_unreachable,
        pruned,
        pruned_conflicts,
        mut find_open,
        mut find_query,
        mut find_options,
        found,
        mut marked_only,
        act,
        ..
    } = ctx;
    let settings = ui_settings.read().clone();
    let show = |b: buttons::Button| super::show(&settings, b);
    let hidden = settings.hidden;
    let (unreachable_count, conflict_count, unreachable_tops, unreachable_hidden) = {
        let i = ctx.info.read();
        (
            i.unreachable_count,
            i.conflict_count,
            i.unreachable_tops,
            i.unreachable_hidden,
        )
    };
    let (step, following, follow_options) = {
        let l = ctx.line.read();
        (l.step, l.following, l.follow_options.clone())
    };

    rsx! {
        if hidden.tree {
            div {
                class: "collapsed-bar tree-bar",
                title: "Show the draft tree panel",
                onclick: move |_| act.set_hidden.call((Panel::Tree, false)),
                "Draft tree ▸"
            }
        } else {
            section { class: "tree-panel",
                div { class: "groups-header",
                    h3 { class: "panel-title", "Draft tree" }
                    {
                        let zoom = if settings.graph_vertical { settings.graph_zoom_v } else { settings.graph_zoom_h };
                        let percent = (zoom * 100.0).round();
                        rsx! {
                            if show(buttons::Button::Zoom) {
                                button {
                                    class: "zoom-chip",
                                    disabled: percent == 100.0,
                                    title: format!("Graph zoom: Ctrl + mouse wheel over the graph{}. Click to reset to 100%{}.", match (keys::hint(keys::Action::ZoomIn), keys::hint(keys::Action::ZoomOut)) { (Some(i), Some(o)) => format!(", or {i} / {o}"), _ => String::new() }, keys::hint_suffix(keys::Action::ZoomReset)),
                                    onclick: move |_| {
                                        let mut s = ui_settings.write();
                                        if s.graph_vertical { s.graph_zoom_v = 1.0 } else { s.graph_zoom_h = 1.0 }
                                        drop(s);
                                        act.save_settings.call(());
                                    },
                                    "{percent}%"
                                }
                            }
                        }
                    }
                    // Delete (the selected action and what follows) and Reset (the whole tree).
                    div { class: "tree-edit-actions",
                        if show(buttons::Button::Delete) {
                            button {
                                class: "btn small",
                                disabled: step == 0 || following,
                                title: "Remove the current action and everything after it (Delete key, or right-click a node)",
                                onclick: move |_| act.delete_node.call(cursor()),
                                "Delete"
                            }
                        }
                        if show(buttons::Button::Reset) {
                            button {
                                class: "btn small",
                                disabled: tree.read().len() <= 1 || following,
                                title: format!("{}{}", match keys::hint(keys::Action::Undo) { Some(h) => format!("Delete the whole tree ({h} undoes it)"), None => "Delete the whole tree (Undo brings it back)".to_string() }, keys::hint_suffix(keys::Action::Reset)),
                                onclick: move |_| act.reset.call(()),
                                "Reset"
                            }
                        }
                    }
                    if unreachable_count > 0 {
                        span { class: "groups-source fl-count", "{unreachable_count} unreachable" }
                    }
                    if conflict_count > 0 {
                        span {
                            class: "groups-source conflict-count",
                            title: "Nodes whose champion or group no longer fits the line above them, after a value was changed higher up. They are kept, so changing the value back restores them.",
                            "{conflict_count} conflicting"
                        }
                    }
                    // Always the same buttons in the same places; the ones that don't
                    // apply are greyed out. Restore buttons follow only after a removal.
                    div { class: "groups-actions",
                        {
                            let t = tree.read();
                            let node = t.get(cursor());
                            let collapsed = node.is_some_and(|n| n.is_collapsed());
                            let has_children = node.is_some_and(|n| !n.children().is_empty());
                            let any = t.any_collapsed();
                            rsx! {
                                if show(buttons::Button::HideBranches) {
                                    button {
                                        class: if collapsed { "btn small active" } else { "btn small" },
                                        disabled: !has_children,
                                        title: if collapsed { format!("Show the selected node's branches{}", keys::hint_suffix(keys::Action::ToggleHidden)) } else { format!("Hide the selected node's branches (they stay in the tree){}", keys::hint_suffix(keys::Action::ToggleHidden)) },
                                        onclick: move |_| act.toggle_hidden.call(()),
                                        if collapsed { "Show branches" } else { "Hide branches" }
                                    }
                                }
                                if show(buttons::Button::ShowAll) {
                                    button {
                                        class: "btn small",
                                        disabled: !any,
                                        title: format!("Show every hidden branch{}", keys::hint_suffix(keys::Action::ShowAllHidden)),
                                        onclick: move |_| act.show_all_hidden.call(()),
                                        "Show all"
                                    }
                                }
                                if show(buttons::Button::MarkedOnly) {
                                    button {
                                        class: if marked_only() { "btn small active" } else { "btn small" },
                                        disabled: (!t.any_marked() && !marked_only()) || following,
                                        title: format!("Show only marked lines (★), without deleting the rest. Mark a line with {}.", keys::hint(keys::Action::Mark).unwrap_or_else(|| "Mark".into())),
                                        onclick: move |_| marked_only.toggle(),
                                        "Marked only"
                                    }
                                }
                            }
                        }
                        if show(buttons::Button::HideUnreachable) {
                            button {
                                class: if unreachable_hidden { "btn small active" } else { "btn small" },
                                disabled: unreachable_tops == 0 || following,
                                title: format!("Hide the branches below unreachable (fearless) nodes, as Hide branches does; click again to show them{}", keys::hint_suffix(keys::Action::HideUnreachable)),
                                onclick: move |_| act.hide_unreachable.call(()),
                                "Hide unreachable"
                            }
                        }
                        if show(buttons::Button::EditUnreachable) {
                            button {
                                class: if edit_unreachable() { "btn small active" } else { "btn small" },
                                disabled: unreachable_count == 0 && !edit_unreachable(),
                                title: "Allow adding new branches below unreachable nodes",
                                onclick: move |_| edit_unreachable.toggle(),
                                "Edit unreachable"
                            }
                        }
                        if show(buttons::Button::RemoveUnreachable) {
                            button {
                                class: "btn small",
                                disabled: unreachable_count == 0 || following,
                                title: format!("Delete every unreachable branch ({}Restore brings them back){}", keys::hint(keys::Action::Undo).map_or_else(String::new, |h| format!("{h} or ")), keys::hint_suffix(keys::Action::RemoveUnreachable)),
                                onclick: move |_| act.remove_unreachable.call(()),
                                "Remove unreachable"
                            }
                        }
                        if show(buttons::Button::RemoveConflicting) {
                            button {
                                class: "btn small",
                                disabled: conflict_count == 0 || following,
                                title: format!("Delete every conflicting branch ({}Restore brings them back){}", keys::hint(keys::Action::Undo).map_or_else(String::new, |h| format!("{h} or ")), keys::hint_suffix(keys::Action::RemoveConflicting)),
                                onclick: move |_| act.remove_conflicts.call(()),
                                "Remove conflicting"
                            }
                        }
                        if show(buttons::Button::RemoveUnreachable) {
                            if let Some(p) = pruned.read().as_ref() {
                                button {
                                    class: "btn small",
                                    disabled: following,
                                    title: format!("Put back the branches the last Remove unreachable deleted{}", keys::hint_suffix(keys::Action::RestoreUnreachable)),
                                    onclick: move |_| act.restore_removed.call(()),
                                    "Restore unreachable ({p.nodes()})"
                                }
                            }
                        }
                        if show(buttons::Button::RemoveConflicting) {
                            if let Some(p) = pruned_conflicts.read().as_ref() {
                                button {
                                    class: "btn small",
                                    disabled: following,
                                    title: format!("Put back the branches the last Remove conflicting deleted{}", keys::hint_suffix(keys::Action::RestoreConflicting)),
                                    onclick: move |_| act.restore_conflicts.call(()),
                                    "Restore conflicting ({p.nodes()})"
                                }
                            }
                        }
                    }
                    button {
                        class: "panel-hide",
                        title: "Hide the draft tree panel (View menu or the bar brings it back)",
                        onclick: move |_| act.set_hidden.call((Panel::Tree, true)),
                        "▾"
                    }
                }
                if let Some(f) = follow.read().clone() {
                    {
                        let entered = f.entered.len();
                        let next = DRAFT_ORDER.get(entered).map(|s| format!("{} {} {}", s.team.label(), s.action.label(), s.number));
                        let off = f.off_tree_at();
                        let fits = f.current().len();
                        let off_text = off.map(|at| {
                            let t = tree.read();
                            (value_name(t.groups(), f.entered[at]), DRAFT_ORDER[at].code())
                        });
                        rsx! {
                            div { class: "follow-bar",
                                span { class: "follow-title", "▶ Follow mode" }
                                span { class: "follow-state",
                                    match &next {
                                        Some(n) => format!("Next: {n}"),
                                        None => "Draft complete".to_string(),
                                    }
                                }
                                span { class: "follow-fit",
                                    if off.is_some() {
                                        "Off the prepared tree"
                                    } else if fits > 1 {
                                        "{fits} prepared lines fit"
                                    }
                                }
                                button {
                                    class: "btn small",
                                    disabled: entered == 0,
                                    title: format!("Take back the last action{}", keys::hint_suffix(keys::Action::FollowBack)),
                                    onclick: move |_| act.follow_back.call(()),
                                    "Back"
                                }
                                button {
                                    class: "btn small",
                                    title: format!("Back to editing, with the last followed node selected{}", keys::hint_suffix(keys::Action::Follow)),
                                    onclick: move |_| act.leave_follow.call(()),
                                    "Leave"
                                }
                            }
                            if let Some((name, code)) = off_text.filter(|_| !f.keep_going) {
                                div { class: "follow-warning",
                                    span {
                                        "No prepared branch for "
                                        b { "{name}" }
                                        " at {code}. "
                                    }
                                    button {
                                        class: "btn small primary",
                                        title: "Leave follow mode with the last node that fitted selected",
                                        onclick: move |_| act.leave_follow.call(()),
                                        "Back to editor"
                                    }
                                    button {
                                        class: "btn small",
                                        title: "Keep entering actions; they are shown in the team panels but match nothing",
                                        onclick: move |_| {
                                            if let Some(f) = follow.write().as_mut() {
                                                f.keep_going = true;
                                            }
                                        },
                                        "Keep following"
                                    }
                                }
                            }
                        }
                    }
                }
                if find_open() {
                    {
                        let matches = found();
                        let total = matches.len();
                        let here = matches.iter().position(|&m| m == cursor()).map_or("–".to_string(), |i| (i + 1).to_string());
                        let opts = find_options();
                        let query_empty = find_query().trim().is_empty();
                        rsx! {
                            div { class: "find-bar",
                                input {
                                    id: "find-input",
                                    class: "search find-input",
                                    autofocus: true,
                                    placeholder: "Find a champion or group in the tree…",
                                    // Not written back while typing: fast typing would lose keys.
                                    initial_value: "{find_query.peek()}",
                                    oninput: move |e| find_query.set(e.value()),
                                }
                                span { class: if total == 0 && !query_empty { "find-count none" } else { "find-count" },
                                    if query_empty { "" } else if total == 0 { "No matches" } else { "{here} / {total}" }
                                }
                                button {
                                    class: "btn small",
                                    disabled: total == 0,
                                    title: "Previous match (Shift + Enter)",
                                    onclick: move |_| act.find_step.call(false),
                                    "↑"
                                }
                                button {
                                    class: "btn small",
                                    disabled: total == 0,
                                    title: "Next match (Enter)",
                                    onclick: move |_| act.find_step.call(true),
                                    "↓"
                                }
                                label { class: "find-check", title: "Group nodes also match when one of their champions does",
                                    input {
                                        r#type: "checkbox",
                                        checked: opts.include_groups,
                                        onchange: move |e| find_options.write().include_groups = e.checked(),
                                    }
                                    "Include groups"
                                }
                                label { class: "find-check", title: "Also look inside branches hidden with Ctrl+H (going to a match there shows it)",
                                    input {
                                        r#type: "checkbox",
                                        checked: opts.search_hidden,
                                        onchange: move |e| find_options.write().search_hidden = e.checked(),
                                    }
                                    "Search in hidden branches"
                                }
                                button {
                                    class: "modal-close find-close",
                                    title: "Close (Esc)",
                                    onclick: move |_| find_open.set(false),
                                    "×"
                                }
                            }
                        }
                    }
                }
                div { class: if settings.graph_vertical { "tree-scroll vertical" } else { "tree-scroll" },
                    TreeView {
                        tree,
                        cursor: cursor(),
                        // In follow mode the graph only shows.
                        on_select: move |id| {
                            if follow.peek().is_none() {
                                extra.set(Vec::new());
                                cursor.set(id);
                            }
                        },
                        // Right-click deletes only the clicked node's branch.
                        on_delete: move |id| {
                            if follow.peek().is_none() {
                                extra.set(Vec::new());
                                act.delete_node.call(id);
                            }
                        },
                        on_roles: move |r| {
                            if follow.peek().is_none() {
                                act.open_roles.call(r);
                            }
                        },
                        followed: follow.read().as_ref().map(|f| (f.followed(&tree.read()), f.entered.len())),
                        vertical: settings.graph_vertical,
                        zoom: if settings.graph_vertical { settings.graph_zoom_v } else { settings.graph_zoom_h },
                        view_width: tree_size().0,
                        view_height: tree_size().1,
                        found: found(),
                        on_hover: move |h: Option<(NodeId, f64, f64)>| act.hover_node.call(h),
                        tooltips: settings.node_tooltips,
                        selected: extra(),
                        on_toggle: move |id| {
                            if follow.peek().is_none() {
                                act.toggle_select.call(id);
                            }
                        },
                        marked_only: marked_only(),
                        smooth: settings.smooth_animations,
                    }
                }
                if following {
                    // What the tree has prepared for the next step; clicking one
                    // enters it.
                    div { class: "follow-options",
                        span { class: "follow-options-label",
                            match DRAFT_ORDER.get(step) {
                                Some(s) => format!("Prepared for {}:", s.code()),
                                None => "Draft complete".to_string(),
                            }
                        }
                        if follow_options.is_empty() && step < DRAFT_ORDER.len() {
                            span { class: "follow-none", "nothing" }
                        }
                        for v in follow_options.clone() {
                            button {
                                key: "{v}",
                                class: "follow-option",
                                title: "{value_name(tree.read().groups(), v)}",
                                onclick: move |_| act.follow_enter.call(v),
                                ValueIcon { value: v }
                            }
                        }
                    }
                }
            }
        }
    }
}
