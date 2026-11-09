//! The representational groups panel.

use super::*;

#[component]
pub fn GroupsPanel() -> Element {
    let ctx = use_context::<Ctx>();
    let Ctx {
        ui_settings,
        tree,
        dialog_open,
        mut rgroup_editor_open,
        mut groups_info_open,
        act,
        ..
    } = ctx;
    let settings = ui_settings.read().clone();
    if settings.hidden.groups {
        return rsx! {
            div {
                class: "collapsed-bar groups-bar",
                title: "Show the representational groups panel",
                onclick: move |_| act.set_hidden.call((Panel::Groups, false)),
                "Representational groups ▸"
            }
        };
    }
    use_shortcut_hints(&ctx);
    let branch_hint = keys::branch_click_hint();
    let line = ctx.line.read();
    let following = line.following;
    let branches = &line.branches;
    let group_tiles = group_tiles(&ctx, &line, &settings);
    let (user_group_count, groups_source) = {
        let t = tree.read();
        let source: Option<String> = t
            .groups()
            .source()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        (t.groups().user_groups().count(), source)
    };

    rsx! {
        section { class: "groups-panel",
            div { class: "groups-title",
                h3 { class: "panel-title", "Representational groups" }
                span { class: "groups-source",
                    match &groups_source {
                        Some(file) => format!("{file} · {user_group_count} custom RGroup(s)"),
                        None => "No user groups loaded".to_string(),
                    }
                }
            }
            div { class: "groups-actions",
                if show(&settings, buttons::Button::GroupsEdit) {
                    button {
                        class: "btn small",
                        title: "Add, edit or remove user groups",
                        disabled: dialog_open() || following,
                        onclick: move |_| rgroup_editor_open.set(true),
                        "Edit"
                    }
                }
                if show(&settings, buttons::Button::GroupsDetails) {
                    button {
                        class: "btn small",
                        title: "List every group and its champions",
                        onclick: move |_| groups_info_open.set(true),
                        "Details"
                    }
                }
                if show(&settings, buttons::Button::GroupsPresets) {
                    button {
                        class: if settings.hide_rgroup_presets { "btn small active" } else { "btn small" },
                        title: "Hide the preset groups Top to P15 (ANY stays)",
                        onclick: move |_| act.toggle_rgroup_presets.call(()),
                        "Hide presets"
                    }
                }
            }
            button {
                class: "panel-hide",
                title: "Hide the representational groups panel (View menu or the bar brings it back)",
                onclick: move |_| act.set_hidden.call((Panel::Groups, true)),
                "▾"
            }
            div { class: "group-grid",
                for GroupTile { value, label, tooltip, selectable, color } in group_tiles {
                    {
                        let class = if !selectable {
                            "group-tile taken"
                        } else if branches.contains(&value) {
                            "group-tile branch"
                        } else {
                            "group-tile"
                        };
                        let color = color.unwrap_or_else(|| "var(--muted)".to_string());
                        rsx! {
                            button {
                                key: "{value}",
                                "data-branch-tile": "1",
                                "data-drag-value": "{value}",
                                class,
                                disabled: !selectable,
                                title: "{tooltip}\n{branch_hint}+click adds it as a branch; releasing {branch_hint} selects the last one.",
                                onclick: move |e: MouseEvent| {
                                    if keys::is_branch_click(e.modifiers()) {
                                        act.add_branch.call(value)
                                    } else {
                                        act.choose.call(value)
                                    }
                                },
                                div { class: "group-icon", style: "--group-color: {color};", "{label}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The group tiles the panel shows: those matching the search (and, with the RGroup filter
/// on, the active selection filters), in catalog order.
fn group_tiles(ctx: &Ctx, line: &LineInfo, settings: &configio::UiSettings) -> Vec<GroupTile> {
    let query = ctx.search.read().to_lowercase();
    let theme_now = ctx.active_theme.read();
    let t = ctx.tree.read();
    let colors = ctx.group_colors.read();
    let state = t.next_line_state(line.grid_base);
    let open = line.grid_open;
    let editing = line.editing;
    let cursor = (ctx.cursor)();
    // In set mode, a sibling's value is offered too: choosing it moves there.
    let sibling_holds = |value: u8| editing && line.branches.contains(&value);
    // Selection filters apply here only with the RGroup filter on, so only then are they read.
    let rgroup_filter = (ctx.rgroup_filter)();
    let filter = if rgroup_filter {
        selection::combined(
            &ctx.sel_groups.read(),
            &ctx.filters_on.read(),
            (ctx.filter_intersect)(),
        )
    } else {
        None
    };
    let passes = |value: u8| filter.is_none_or(|f| f.contains(value));
    let mut tiles: Vec<GroupTile> = t
        .groups()
        .all()
        .iter()
        .filter(|g| {
            (g.name.to_lowercase().contains(&query) || g.label.to_lowercase().contains(&query))
                && (!rgroup_filter || passes(g.value))
                && !(settings.hide_rgroup_presets && (3..=groups::PRESET_LAST).contains(&g.value))
        })
        .map(|g| GroupTile {
            value: g.value,
            label: g.label.clone(),
            tooltip: group_tooltip(g),
            selectable: open
                && (state.check(g.value).is_ok() || sibling_holds(g.value))
                && (editing || g.roles == 0 || t.child_roles_fit(cursor, g.roles)),
            color: group_color(&colors, &theme_now, g.value),
        })
        .collect();

    // Follow mode: tiles follow the entered draft.
    if let Some(f) = ctx.follow.read().as_ref() {
        for g in tiles.iter_mut() {
            g.selectable = open && f.can_enter(g.value);
        }
    }
    tiles
}
