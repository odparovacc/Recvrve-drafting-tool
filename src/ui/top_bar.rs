//! The top bar: fullscreen, tile size, tabs and the file / mode buttons.

use super::*;

#[component]
pub fn TopBar() -> Element {
    let ctx = use_context::<Ctx>();
    use_shortcut_hints(&ctx);
    let Ctx {
        tree,
        follow,
        tab_list,
        active_tab,
        parked,
        dialog_open,
        fullscreen,
        mut help_open,
        mut fearless_open,
        mut ui_settings,
        mut save_menu_open,
        mut save_selection,
        mut merge_open,
        mut merge_tabs,
        mut export_menu_open,
        mut export_kind,
        export_busy,
        mut share_open,
        mut share_draft,
        mut preview,
        mut tab_menu,
        act,
        ..
    } = ctx;

    let settings = ui_settings.read().clone();
    let full = fullscreen();
    let following = follow.read().is_some();
    let fearless_state = ctx.info.read().fearless;

    rsx! {
        header { class: if full { "topbar hidden" } else { "topbar" },
            div { class: "title-area",
                // Far left, away from the window's own buttons.
                button {
                    class: "btn fullscreen-btn",
                    title: format!("Fullscreen: only champions, groups and the graph{}. Esc leaves it.", keys::hint_suffix(keys::Action::ToggleFullscreen)),
                    "aria-label": "Fullscreen",
                    onclick: move |_| act.set_fullscreen.call(true),
                    "⛶"
                }
                label {
                    class: "tile-slider",
                    title: "Champion tile size (double-click to reset)",
                    ondoubleclick: move |_| {
                        ui_settings.write().tile_size = configio::TILE_DEFAULT;
                        act.save_settings.call(());
                    },
                    span { "Tile size" }
                    input {
                        r#type: "range",
                        min: "{configio::TILE_MIN}",
                        max: "{configio::TILE_MAX}",
                        step: "4",
                        value: "{settings.tile_size}",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<u32>() {
                                ui_settings.write().tile_size = v.clamp(configio::TILE_MIN, configio::TILE_MAX);
                            }
                        },
                        onchange: move |_| act.save_settings.call(()),
                    }
                    span { class: "tile-value", "{settings.tile_size}px" }
                }
                // Tabs: click to show, drag to reorder, × or a middle click to close, right-click
                // for a menu (move to another window, close).
                div { class: "tab-strip",
                    div { class: "tab-scroll",
                        for info in tab_list() {
                            {
                                let id = info.id;
                                let active = id == active_tab();
                                let following = if active {
                                    follow.read().is_some()
                                } else {
                                    parked.read().get(&id).is_some_and(|p| p.follow.is_some())
                                };
                                let class = match (active, following) {
                                    (true, true) => "tab active following",
                                    (true, false) => "tab active",
                                    (false, true) => "tab following",
                                    (false, false) => "tab",
                                };
                                let mut tip = match &info.source {
                                    Some(p) => format!("{}\n{}", info.name, p.display()),
                                    None => info.name.clone(),
                                };
                                if info.dirty {
                                    tip.push_str("\nUnsaved changes");
                                }
                                if following {
                                    tip.push_str("\nIn follow mode");
                                }
                                rsx! {
                                    div {
                                        key: "{id}",
                                        class,
                                        "data-tab": "{id}",
                                        title: "{tip}",
                                        onclick: move |_| act.switch_tab.call(id),
                                        onmousedown: move |e: MouseEvent| {
                                            // No middle-click scrolling: a middle click closes.
                                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                                e.prevent_default();
                                            }
                                        },
                                        onmouseup: move |e: MouseEvent| {
                                            if e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary) {
                                                act.request_close.call(id);
                                            }
                                        },
                                        oncontextmenu: move |e: MouseEvent| {
                                            e.prevent_default();
                                            let p = e.client_coordinates();
                                            tab_menu.set(Some((id, p.x, p.y)));
                                        },
                                        span { class: "tab-name", "{info.name}" }
                                        if info.dirty {
                                            span { class: "tab-dirty", "•" }
                                        }
                                        button {
                                            class: "tab-close",
                                            title: "Close the tab",
                                            onclick: move |e| {
                                                e.stop_propagation();
                                                act.request_close.call(id);
                                            },
                                            "×"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some((id, x, y)) = tab_menu() {
                        {
                            let only = tab_list.read().len() < 2;
                            let others = crate::windows::others(ctx.window_no);
                            let hint = keys::hint_suffix(keys::Action::TabToNewWindow);
                            rsx! {
                                div {
                                    class: "menu-backdrop",
                                    onclick: move |_| tab_menu.set(None),
                                    oncontextmenu: move |e: MouseEvent| {
                                        e.prevent_default();
                                        tab_menu.set(None);
                                    },
                                }
                                div {
                                    class: "menu tab-menu",
                                    style: "left: min({x}px, calc(100vw - 300px)); top: {y + 4.0}px;",
                                    button {
                                        class: "menu-item",
                                        disabled: only,
                                        title: if only { "This is the window's only tab".to_string() } else { format!("Moves the tab to a window of its own{hint}") },
                                        onclick: move |_| {
                                            tab_menu.set(None);
                                            act.move_tab.call((id, None));
                                        },
                                        "Open in new window"
                                    }
                                    for (number, label) in others {
                                        button {
                                            key: "{number}",
                                            class: "menu-item",
                                            onclick: move |_| {
                                                tab_menu.set(None);
                                                act.move_tab.call((id, Some(number)));
                                            },
                                            "Move to {label}"
                                        }
                                    }
                                    div { class: "menu-separator" }
                                    button {
                                        class: "menu-item",
                                        onclick: move |_| {
                                            tab_menu.set(None);
                                            act.request_close.call(id);
                                        },
                                        "Close tab"
                                    }
                                }
                            }
                        }
                    }
                    button {
                        class: "tab-new",
                        title: format!("New tab{}", keys::hint_suffix(keys::Action::NewTab)),
                        onclick: move |_| act.new_tab.call(()),
                        "+"
                    }
                }
            }
            div { class: "controls",
                if show(&settings, buttons::Button::Save) {
                    div { class: "menu-wrap",
                        button {
                            class: "btn",
                            title: match keys::hint(keys::Action::Save) { Some(h) => format!("Save the draft tree to a file ({h} saves compact)"), None => "Save the draft tree to a file".to_string() },
                            disabled: dialog_open(),
                            onclick: move |_| save_menu_open.toggle(),
                            "Save ▾"
                        }
                        if save_menu_open() {
                            // Invisible layer that closes the menu on any outside click.
                            div {
                                class: "menu-backdrop",
                                onclick: move |_| save_menu_open.set(false),
                            }
                            div { class: "menu",
                                for format in [SaveFormat::Compact, SaveFormat::Text] {
                                    button {
                                        key: "{format.extension()}",
                                        class: "menu-item",
                                        onclick: move |_| act.save_as.call(format),
                                        "{format.label()}"
                                    }
                                }
                                div { class: "menu-separator" }
                                label {
                                    class: "menu-check",
                                    title: "Store which node is selected, so loading the file opens that draft state",
                                    input {
                                        r#type: "checkbox",
                                        checked: save_selection(),
                                        onchange: move |e| save_selection.set(e.checked()),
                                    }
                                    "Include selected node"
                                }
                            }
                        }
                    }
                }
                if show(&settings, buttons::Button::Load) {
                    button {
                        class: "btn",
                        title: format!("Load a draft tree from a .dts or .dtt file{}", keys::hint_suffix(keys::Action::Load)),
                        disabled: dialog_open(),
                        onclick: move |_| act.load_file.call(()),
                        "Load"
                    }
                }
                if show(&settings, buttons::Button::Merge) {
                    button {
                        class: "btn",
                        title: format!("Merge several save files into one tree{}", keys::hint_suffix(keys::Action::Merge)),
                        disabled: dialog_open(),
                        onclick: move |_| {
                            // Closed tabs drop out of the merge.
                            let open: Vec<u64> = tab_list.peek().iter().map(|t| t.id).collect();
                            merge_tabs.write().retain(|id| open.contains(id));
                            merge_open.set(true);
                        },
                        "Merge…"
                    }
                }
                // The menu also opens from its shortcut while the button is hidden.
                if show(&settings, buttons::Button::Export) || export_menu_open() {
                    div { class: "menu-wrap",
                        if show(&settings, buttons::Button::Export) {
                            button {
                                class: if export_kind().is_some() { "btn active" } else { "btn" },
                                title: format!("Export the tree or draft states as images, PDF or XML{}", keys::hint_suffix(keys::Action::Export)),
                                disabled: tree.read().len() <= 1 || export_busy(),
                                onclick: move |_| export_menu_open.toggle(),
                                "Export ▾"
                            }
                        }
                        if export_menu_open() {
                            div {
                                class: "menu-backdrop",
                                onclick: move |_| export_menu_open.set(false),
                            }
                            div { class: "menu",
                                for kind in export_panel::ExportKind::ALL {
                                    button {
                                        key: "{kind:?}",
                                        class: "menu-item",
                                        onclick: move |_| {
                                            export_menu_open.set(false);
                                            export_kind.set(Some(kind));
                                        },
                                        "{kind.menu_label()}"
                                    }
                                }
                            }
                        }
                    }
                }
                if show(&settings, buttons::Button::Share) {
                    button {
                        class: if share_open() { "btn active" } else { "btn" },
                        title: "Show or hide the tree as a text string for copying and pasting",
                        onclick: move |_| {
                            share_draft.set(None);
                            share_open.toggle();
                        },
                        "Share"
                    }
                }
                if show(&settings, buttons::Button::Fearless) {
                    button {
                        class: if fearless_open() || fearless_state.is_active() { "btn active" } else { "btn" },
                        disabled: following,
                        title: format!("Fearless draft: champions picked in earlier games of the series{}", keys::hint_suffix(keys::Action::Fearless)),
                        onclick: move |_| fearless_open.toggle(),
                        "Fearless"
                        if fearless_state.game > 1 {
                            span { class: "fl-badge-top", "G{fearless_state.game} · {fearless_state.count()}" }
                        }
                    }
                }
                if show(&settings, buttons::Button::Follow) {
                    button {
                        class: if following { "btn active" } else { "btn" },
                        title: if following { format!("Leave follow mode and go back to editing{}", keys::hint_suffix(keys::Action::Follow)) } else { format!("Follow mode: enter a real draft action by action and follow the prepared branches{}", keys::hint_suffix(keys::Action::Follow)) },
                        onclick: move |_| {
                            if follow.peek().is_some() {
                                act.leave_follow.call(());
                            } else {
                                act.start_follow.call(());
                            }
                        },
                        "Follow"
                    }
                }
                if show(&settings, buttons::Button::Previews) {
                    button {
                        class: if settings.node_previews { "btn active" } else { "btn" },
                        title: "Show the draft state when hovering a node in the graph",
                        onclick: move |_| {
                            let on = !ui_settings.peek().node_previews;
                            ui_settings.write().node_previews = on;
                            preview.set(None);
                            act.save_settings.call(());
                        },
                        "Previews"
                    }
                }
                if show(&settings, buttons::Button::Tooltips) {
                    button {
                        class: if settings.node_tooltips { "btn active" } else { "btn" },
                        title: "Show the browser's tooltips on graph nodes, with debug details (id, position bits, children)",
                        onclick: move |_| {
                            let on = !ui_settings.peek().node_tooltips;
                            ui_settings.write().node_tooltips = on;
                            act.save_settings.call(());
                        },
                        "Tooltips"
                    }
                }
                button {
                    class: if help_open() { "btn active" } else if !settings.help_seen { "btn help-glow" } else { "btn" },
                    title: format!("Keyboard shortcuts, button settings, view and theme{}", keys::hint_suffix(keys::Action::Help)),
                    onclick: move |_| help_open.toggle(),
                    "Help"
                }
            }
        }
    }
}
