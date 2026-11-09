//! The champion pool: selection filters, search and the champion grid.

use super::*;

#[component]
pub fn ChampionPool() -> Element {
    let ctx = use_context::<Ctx>();
    let Ctx {
        mut filters_on,
        mut filter_intersect,
        mut rgroup_filter,
        mut ui_settings,
        sel_groups,
        dialog_open,
        mut sel_editor_open,
        mut search,
        mut set_mode,
        edit_unreachable,
        act,
        ..
    } = ctx;
    let settings = ui_settings.read().clone();
    if settings.hidden.pool {
        return rsx! {
            div {
                class: "collapsed-bar pool-bar",
                title: "Show the champions panel",
                onclick: move |_| act.set_hidden.call((Panel::Pool, false)),
                "Champions ▸"
            }
        };
    }
    use_shortcut_hints(&ctx);
    let set_hint = keys::hint_suffix(keys::Action::SetValue);
    let branch_hint = keys::branch_click_hint();
    let shown = |b: buttons::Button| show(&settings, b);
    let sel_list = sel_groups.read();
    let filter_action_cols = {
        use buttons::Button::*;
        [
            RGroupFilter,
            SetMode,
            AutoTag,
            SelectionEdit,
            ClearFilters,
            SelectionPresets,
        ]
        .into_iter()
        .filter(|&b| shown(b))
        .count()
        .clamp(1, 3)
    };
    // Room for "RGroup filter off" and "Intersection" only when those buttons are shown.
    let filter_action_w = if shown(buttons::Button::RGroupFilter) || shown(buttons::Button::SetMode)
    {
        124
    } else {
        100
    };
    // Indices of the selection groups whose buttons are shown (Ctrl+1..9 count these).
    let visible_filters: Vec<usize> = (0..sel_list.len())
        .filter(|&i| {
            !(settings.hide_selection_presets && selection::is_protected_name(&sel_list[i].name))
        })
        .collect();
    let line = ctx.line.read();
    let editing = line.editing;
    let grid_open = line.grid_open;
    let wildcard = line.wildcard;
    let wildcard_label = line.wildcard_label();
    let branches = &line.branches;
    let cursor_unreachable = line.cursor_unreachable;
    let current = DRAFT_ORDER.get(line.step);
    let visible = champ_tiles(&ctx, &line, &sel_list);

    rsx! {
        section { class: "pool",
            // Filters wrap on the left (two rows at most, then they scroll); the
            // buttons on the right keep their places however many filters exist.
            div { class: "filter-head",
                div { class: "filter-list",
                    if shown(buttons::Button::SelectionFilters) {
                        for (n, i) in visible_filters.iter().copied().enumerate() {
                            button {
                                key: "{i}",
                                class: if filters_on.read().get(i).copied().unwrap_or(false) { "filter-btn active" } else { "filter-btn" },
                                title: if n < 9 { keys::hint(keys::Action::ToggleFilter(n)).unwrap_or_default() } else { String::new() },
                                onclick: move |_| {
                                    if let Some(on) = filters_on.write().get_mut(i) {
                                        *on = !*on;
                                    }
                                },
                                "{sel_list[i].name}"
                            }
                        }
                    }
                }
                div {
                    class: "filter-actions",
                    // Three columns, or fewer when fewer buttons are shown.
                    style: "--fa-cols: {filter_action_cols}; --fa-w: {filter_action_w}px;",
                    if shown(buttons::Button::RGroupFilter) {
                        button {
                            class: if rgroup_filter() { "filter-btn set-mode active" } else { "filter-btn set-mode" },
                            title: format!("Whether selection filters also apply to the representational groups area{}", keys::hint_suffix(keys::Action::ToggleRGroupFilter)),
                            onclick: move |_| rgroup_filter.toggle(),
                            if rgroup_filter() { "RGroup filter on" } else { "RGroup filter off" }
                        }
                    }
                    if shown(buttons::Button::SetMode) {
                        button {
                            class: if filters_on.read().iter().filter(|on| **on).count() < 2 { "filter-btn set-mode dim" } else { "filter-btn set-mode" },
                            title: format!("How several active filters combine{}", keys::hint_suffix(keys::Action::ToggleSetMode)),
                            onclick: move |_| filter_intersect.toggle(),
                            if filter_intersect() { "Intersection" } else { "Union" }
                        }
                    }
                    if shown(buttons::Button::AutoTag) {
                        button {
                            class: if settings.auto_tag_roles { "filter-btn set-mode active" } else { "filter-btn set-mode" },
                            title: "Tag new picks with the roles of the active Top / Jungle / Mid / ADC / Support filters",
                            onclick: move |_| {
                                let on = !ui_settings.peek().auto_tag_roles;
                                ui_settings.write().auto_tag_roles = on;
                                act.save_settings.call(());
                            },
                            if settings.auto_tag_roles { "Auto-tag on" } else { "Auto-tag off" }
                        }
                    }
                    if shown(buttons::Button::SelectionEdit) {
                        button {
                            class: "filter-btn",
                            disabled: dialog_open(),
                            title: "Add, edit, reorder or remove selection groups",
                            onclick: move |_| sel_editor_open.set(true),
                            "Edit"
                        }
                    }
                    if shown(buttons::Button::ClearFilters) {
                        button {
                            class: "filter-btn",
                            disabled: !filters_on.read().iter().any(|on| *on),
                            title: format!("Turn every filter off{}", keys::hint_suffix(keys::Action::ClearFilters)),
                            onclick: move |_| filters_on.write().fill(false),
                            "Clear"
                        }
                    }
                    if shown(buttons::Button::SelectionPresets) {
                        button {
                            class: if settings.hide_selection_presets { "filter-btn active" } else { "filter-btn" },
                            title: "Hide the Top / Jungle / Mid / ADC / Support filters",
                            onclick: move |_| act.toggle_selection_presets.call(()),
                            "Hide presets"
                        }
                    }
                }
                button {
                    class: "panel-hide",
                    title: "Hide the champions panel (View menu or the bar brings it back)",
                    onclick: move |_| act.set_hidden.call((Panel::Pool, true)),
                    "▾"
                }
            }
            input {
                class: "search",
                r#type: "text",
                placeholder: "Search champions...",
                value: "{search}",
                oninput: move |e| search.set(e.value()),
            }
            if ICON_DIR.get().is_some_and(Option::is_none) {
                div { class: "empty",
                    "Champion icons not found. Run the app from the project folder, or set RECVRVE_ICONS."
                }
            }
            if editing {
                div { class: "set-banner",
                    span {
                        "Setting the value of "
                        b { "{line.set_target}" }
                        ": click a champion or group. Choosing a value a sibling already has selects that sibling. Esc cancels."
                    }
                    button { class: "btn small", onclick: move |_| set_mode.set(false), "Cancel" }
                }
            } else if let Some(at) = line.cursor_conflict_name.as_ref() {
                div { class: "set-banner conflict",
                    "This branch conflicts at {at}, so nothing new can be added below it. Change that node's value{set_hint} or remove it."
                }
            }
            div { class: "grid",
                button {
                    key: "wildcard",
                    "data-branch-tile": "1",
                    "data-drag-value": "{wildcard}",
                    class: if branches.contains(&wildcard) { "champ branch" } else { "champ" },
                    disabled: !grid_open,
                    title: if editing { format!("{wildcard_label} (value {wildcard}): empty the selected node") } else { format!("{wildcard_label} (value {wildcard}). {branch_hint}+click adds another empty branch even if one exists.") },
                    onclick: move |e: MouseEvent| {
                        if keys::is_branch_click(e.modifiers()) {
                            act.add_branch.call(wildcard)
                        } else {
                            act.choose.call(wildcard)
                        }
                    },
                    div { class: "wildcard-icon", "∅" }
                }
                for ChampTile { value, champion, selectable, taken, banned, rings, ring_names, fearless } in visible {
                    {
                        let class = if !selectable {
                            "champ taken"
                        } else if branches.contains(&value) {
                            "champ branch"
                        } else {
                            "champ"
                        };
                        let glow_class = if rings.is_empty() { "" } else { "ringed" };
                        let ban_class = if banned { "banned" } else { "" };
                        let fl_class = fearless.map_or(String::new(), |(_, t)| format!("fl {}", t.class()));
                        let glow_style = ring_style(&rings);
                        let title = if let Some((game, tone)) = fearless {
                            let who = match tone {
                                fearless::Tone::Both => "both teams",
                                fearless::Tone::Blue => "the blue-side team",
                                fearless::Tone::Red => "the red-side team",
                            };
                            format!("{} (value {value}) was picked in game {game}: fearless locks it for {who}", champion.name)
                        } else if cursor_unreachable && !edit_unreachable() {
                            format!("{} (value {value}): this branch is unreachable. Turn on \"Edit unreachable\" to extend it.", champion.name)
                        } else if banned {
                            format!("{} (value {value}) is banned on this line", champion.name)
                        } else if taken {
                            format!("{} (value {value}) is already on this line", champion.name)
                        } else if !selectable && current.is_some() {
                            format!(
                                "{} (value {value}): every champion of a group on this line is accounted for",
                                champion.name
                            )
                        } else if !ring_names.is_empty() {
                            format!(
                                "{} (value {value}) may already be taken by a group earlier on this line: {}",
                                champion.name,
                                ring_names.join(", ")
                            )
                        } else {
                            format!("{} (value {value})", champion.name)
                        };
                        rsx! {
                            button {
                                key: "{value}",
                                "data-branch-tile": "1",
                                "data-drag-value": "{value}",
                                class: "{class} {glow_class} {ban_class} {fl_class}",
                                style: "{glow_style}",
                                disabled: !selectable,
                                title: "{title}",
                                onclick: move |e: MouseEvent| {
                                    if keys::is_branch_click(e.modifiers()) {
                                        act.add_branch.call(value)
                                    } else {
                                        act.choose.call(value)
                                    }
                                },
                                img { src: "{icon_url(champion)}", alt: "{champion.name}", draggable: "false" }
                                if let Some((game, _)) = fearless {
                                    span { class: "fl-label", "F{game}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The champion tiles the grid shows: those matching the search and the active selection
/// filters, sorted by name.
fn champ_tiles(
    ctx: &Ctx,
    line: &LineInfo,
    sel_list: &[selection::SelectionGroup],
) -> Vec<ChampTile> {
    let query = ctx.search.read().to_lowercase();
    let theme_now = ctx.active_theme.read();
    let t = ctx.tree.read();
    let colors = ctx.group_colors.read();
    let state = t.next_line_state(line.grid_base);
    let open = line.grid_open;
    let editing = line.editing;
    let branches = &line.branches;
    let fearless = t.fearless();
    // In set mode, a sibling's value is offered too: choosing it moves there.
    let sibling_holds = |value: u8| editing && branches.contains(&value);
    // Values banned on this line, i.e. chosen at ban positions.
    let banned_on_line: Vec<u8> = line
        .grid_values()
        .iter()
        .zip(DRAFT_ORDER.iter())
        .filter(|(_, step)| step.action == Action::Ban)
        .map(|(&v, _)| v)
        .collect();
    // Values the active selection groups let through; `None` shows everything.
    let filter = selection::combined(sel_list, &ctx.filters_on.read(), (ctx.filter_intersect)());
    let passes = |value: u8| filter.is_none_or(|f| f.contains(value));
    let mut visible: Vec<ChampTile> = CHAMPIONS
        .iter()
        .enumerate()
        .filter(|(i, c)| c.name.to_lowercase().contains(&query) && passes(champions::value_of(*i)))
        .map(|(i, c)| {
            let value = champions::value_of(i);
            let taken = state.is_taken(value);
            // Groups on this line that could stand for this champion, earliest first.
            let line_groups: Vec<&groups::Group> = if taken {
                Vec::new()
            } else {
                state
                    .groups_on_line()
                    .filter(|g| g.contains(value))
                    .collect()
            };
            let rings: Vec<String> = line_groups
                .iter()
                .filter_map(|g| group_color(&colors, &theme_now, g.value))
                .take(4)
                .collect();
            let ring_names = line_groups.iter().map(|g| g.name.clone()).collect();
            ChampTile {
                value,
                champion: c,
                selectable: open && (state.check(value).is_ok() || sibling_holds(value)),
                taken,
                banned: banned_on_line.contains(&value),
                rings,
                ring_names,
                fearless: fearless.mark(value),
            }
        })
        .collect();
    visible.sort_by_key(|tile| tile.champion.name);

    // Follow mode: tiles follow the entered draft, and champions of the groups prepared for
    // the next step get those groups' rings.
    if let Some(f) = ctx.follow.read().as_ref() {
        let banned_line: Vec<u8> = f
            .entered
            .iter()
            .zip(DRAFT_ORDER.iter())
            .filter(|(_, s)| s.action == Action::Ban)
            .map(|(&v, _)| v)
            .collect();
        let prepared: Vec<&groups::Group> = line
            .follow_options
            .iter()
            .filter_map(|&v| t.groups().get(v))
            .collect();
        for tile in visible.iter_mut() {
            tile.taken = f.entered.contains(&tile.value);
            tile.banned = banned_line.contains(&tile.value);
            tile.selectable = open && f.can_enter(tile.value);
            let gs: Vec<&&groups::Group> =
                prepared.iter().filter(|g| g.contains(tile.value)).collect();
            tile.rings = gs
                .iter()
                .filter_map(|g| group_color(&colors, &theme_now, g.value))
                .take(4)
                .collect();
            tile.ring_names = gs.iter().map(|g| g.name.clone()).collect();
        }
    }
    visible
}
