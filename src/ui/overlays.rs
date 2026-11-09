//! Popups and floating panels: export, theme settings, role tags, fearless, Help, group
//! editors and details, confirmations, merge, notices, previews, warnings and errors.

use super::*;

#[component]
pub fn Overlays() -> Element {
    let ctx = use_context::<Ctx>();
    let Ctx {
        tree,
        cursor,
        group_colors,
        export_busy,
        mut export_kind,
        mut ui_settings,
        mut themes,
        mut theme_preview,
        mut theme_menu_open,
        mut theme_editor_open,
        active_theme,
        role_popup,
        mut fearless_open,
        mut help_open,
        keybinds,
        mut recording,
        mut kb_message,
        mut view_menu_open,
        easter_eggs,
        mut groups_info_open,
        mut rgroup_editor_open,
        mut sel_editor_open,
        sel_groups,
        mut pending_groups,
        mut restore_offer,
        mut close_confirm,
        mut window_close_confirm,
        tab_list,
        mut merge_open,
        merge_files,
        merge_tabs,
        dialog_open,
        mut notice,
        notice_leaving,
        preview,
        mut warning,
        mut error,
        info,
        act,
        ..
    } = ctx;

    rsx! {
        if let Some(kind) = export_kind() {
            export_panel::ExportPanel {
                key: "{kind:?}",
                kind,
                tree,
                cursor,
                colors: group_colors,
                busy: export_busy,
                vertical: ui_settings.read().graph_vertical,
                on_close: move |_| export_kind.set(None),
            }
        }
        if theme_editor_open() {
            theme_editor::ThemeEditor {
                initial: {
                    let id = ui_settings.read().theme.clone();
                    themes
                        .read()
                        .iter()
                        .find(|t| t.id == id)
                        .cloned()
                        .unwrap_or_else(|| theme::Theme::builtin(theme::Base::Dark))
                },
                on_preview: move |t| theme_preview.set(Some(t)),
                on_save: act.save_theme,
                on_close: move |_| {
                    theme_editor_open.set(false);
                    theme_preview.set(None);
                },
            }
        }
        if let Some(p) = role_popup() {
            role_popup::RolePopup {
                node: p.node,
                x: p.x,
                y: p.y,
                on_set: act.set_roles,
                on_close: move |_| act.close_roles.call(()),
            }
        }
        if fearless_open() {
            fearless_editor::FearlessEditor {
                initial: info.read().fearless,
                on_apply: act.apply_fearless,
                on_close: move |_| fearless_open.set(false),
            }
        }
        if help_open() {
            shortcuts_panel::ShortcutsPanel {
                keybinds,
                recording,
                message: kb_message,
                ui_settings,
                on_settings: move |_| act.save_settings.call(()),
                menus: rsx! {
                    div { class: "menu-wrap",
                        button {
                            class: if view_menu_open() { "btn small active" } else { "btn small" },
                            title: "Panels and graph direction",
                            onclick: move |_| view_menu_open.toggle(),
                            "View ▾"
                        }
                        if view_menu_open() {
                            div {
                                class: "menu-backdrop",
                                onclick: move |_| view_menu_open.set(false),
                            }
                            div { class: "menu menu-right",
                                for panel in Panel::ALL {
                                    label { key: "{panel.label()}", class: "menu-item menu-check",
                                        input {
                                            r#type: "checkbox",
                                            checked: !panel.get(&ui_settings.read().hidden),
                                            onchange: move |e| act.set_hidden.call((panel, !e.checked())),
                                        }
                                        "{panel.label()}"
                                    }
                                }
                                div { class: "menu-separator" }
                                div { class: "menu-heading", "Graph" }
                                for (label, vertical) in [("Top to bottom", true), ("Left to right", false)] {
                                    label { key: "{label}", class: "menu-item menu-check",
                                        input {
                                            r#type: "radio",
                                            name: "graph-orientation",
                                            checked: ui_settings.read().graph_vertical == vertical,
                                            onchange: move |_| {
                                                ui_settings.write().graph_vertical = vertical;
                                                act.save_settings.call(());
                                            },
                                        }
                                        "{label}"
                                    }
                                }
                                div { class: "menu-separator" }
                                label { class: "menu-item menu-check",
                                    title: "Turn every easter egg on or off",
                                    input {
                                        r#type: "checkbox",
                                        checked: easter_eggs(),
                                        onchange: move |e| act.set_easter_eggs.call(e.checked()),
                                    }
                                    "Easter eggs"
                                }
                                div { class: "menu-separator" }
                                div { class: "menu-heading", "Animations" }
                                for (label, smooth, tip) in [
                                    ("Smooth", true, "Smooth animations in the graph, panels and menus"),
                                    ("Low spec", false, "Only the few quick effects the app always had: for slow computers"),
                                ] {
                                    label { key: "{label}", class: "menu-item menu-check", title: tip,
                                        input {
                                            r#type: "radio",
                                            name: "animations",
                                            checked: ui_settings.read().smooth_animations == smooth,
                                            onchange: move |_| {
                                                ui_settings.write().smooth_animations = smooth;
                                                act.save_settings.call(());
                                            },
                                        }
                                        "{label}"
                                    }
                                }
                                div { class: "menu-separator" }
                                div { class: "menu-heading", "Counter picks" }
                                div { class: "menu-note",
                                    {ctx.counter_data.read().snapshot.as_ref().map_or("No counter data".to_string(), |s| s.describe())}
                                }
                                button {
                                    class: "menu-item",
                                    title: "Download the champions that counter each pick from OP.GG (Master+ solo queue, current patch). Takes about a minute.",
                                    disabled: ctx.counter_update.read().is_some(),
                                    onclick: move |_| act.update_counters.call(()),
                                    match *ctx.counter_update.read() {
                                        Some((n, total)) => format!("Updating counter data… {n}/{total}"),
                                        None => "Update counter data".to_string(),
                                    }
                                }
                                div { class: "menu-separator" }
                                div { class: "menu-heading", "Autosave" }
                                for (label, minutes) in [("Off", 0u32), ("Every minute", 1), ("Every 2 minutes", 2), ("Every 5 minutes", 5), ("Every 10 minutes", 10)] {
                                    label { key: "{label}", class: "menu-item menu-check",
                                        title: "How often changed tabs are saved to saves/autosave, for restoring them after a crash",
                                        input {
                                            r#type: "radio",
                                            name: "autosave",
                                            checked: ui_settings.read().autosave_minutes == minutes,
                                            onchange: move |_| {
                                                ui_settings.write().autosave_minutes = minutes;
                                                act.save_settings.call(());
                                            },
                                        }
                                        "{label}"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "menu-wrap",
                        button {
                            class: if theme_menu_open() { "btn small theme-toggle active" } else { "btn small theme-toggle" },
                            title: "Choose a theme. Add your own as folders in config/themes.",
                            onclick: move |_| {
                                if !theme_menu_open() {
                                    themes.set(theme::scan().0);
                                }
                                theme_menu_open.toggle();
                            },
                            "◐ {active_theme.read().name} ▾"
                        }
                        if theme_menu_open() {
                            div {
                                class: "menu-backdrop",
                                onclick: move |_| theme_menu_open.set(false),
                            }
                            div { class: "menu menu-right",
                                for t in themes() {
                                    {
                                        let id = t.id.clone();
                                        let selected = id == ui_settings.read().theme;
                                        let tip = if t.warnings.is_empty() {
                                            t.dir().display().to_string()
                                        } else {
                                            t.warnings.join("\n")
                                        };
                                        rsx! {
                                            button {
                                                key: "{t.id}",
                                                class: if selected { "menu-item theme-item active" } else { "menu-item theme-item" },
                                                title: "{tip}",
                                                onclick: move |_| {
                                                    theme_menu_open.set(false);
                                                    ui_settings.write().theme = id.clone();
                                                    act.save_settings.call(());
                                                },
                                                span { class: "theme-check", if selected { "✓" } }
                                                span { "{t.name}" }
                                                if !t.warnings.is_empty() {
                                                    span { class: "theme-warn", "⚠" }
                                                }
                                            }
                                        }
                                    }
                                }
                                div { class: "menu-separator" }
                                button {
                                    class: "menu-item",
                                    title: "Edit the selected theme",
                                    onclick: move |_| {
                                        theme_menu_open.set(false);
                                        help_open.set(false);
                                        theme_editor_open.set(true);
                                    },
                                    "Theme settings…"
                                }
                            }
                        }
                    }
                },
                on_close: move |_| {
                    recording.set(None);
                    kb_message.set(None);
                    help_open.set(false);
                },
            }
        }
        if groups_info_open() {
            GroupsInfo {
                on_copy: move |value: u8| {
                    let group = tree.peek().groups().get(value).cloned();
                    if let Some(g) = group {
                        act.copy_to_selection.call(g);
                    }
                },
                on_close: move |_| groups_info_open.set(false),
            }
        }
        if rgroup_editor_open() {
            {
                let t = tree.read();
                let initial: Vec<groups::Group> = t.groups().user_groups().cloned().collect();
                let file_label = t
                    .groups()
                    .source()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| groups::CONFIG_FILE_NAME.to_string());
                rsx! {
                    editors::RepGroupEditor {
                        initial,
                        file_label,
                        on_save: act.save_rgroups,
                        on_copy: act.copy_to_selection,
                        on_load: move |_| {
                            rgroup_editor_open.set(false);
                            act.load_groups_file.call(());
                        },
                        on_close: move |_| rgroup_editor_open.set(false),
                    }
                }
            }
        }
        if sel_editor_open() {
            editors::SelectionEditor {
                initial: sel_groups.read().clone(),
                file_label: selection::config_path().display().to_string(),
                on_save: act.save_selection_groups,
                on_copy: act.copy_to_representational,
                on_close: move |_| sel_editor_open.set(false),
            }
        }
        if let Some(pending) = pending_groups() {
            div {
                class: "modal-backdrop",
                onclick: move |_| pending_groups.set(None),
                div {
                    class: "modal",
                    onclick: move |e| e.stop_propagation(),
                    p { class: "modal-text", "{pending.message}" }
                    div { class: "modal-buttons",
                        button { class: "btn", onclick: move |_| pending_groups.set(None), "Cancel" }
                        button {
                            class: "btn primary",
                            onclick: move |_| {
                                pending_groups.set(None);
                                act.commit_groups.call(pending.clone());
                            },
                            "Continue"
                        }
                    }
                }
            }
        }
        if let Some(crashed) = restore_offer() {
            {
                let names: Vec<String> = crashed.tabs.iter().map(|t| t.entry.name.clone()).collect();
                let count = names.len();
                let for_discard = crashed.clone();
                rsx! {
                    div { class: "modal-backdrop",
                        div {
                            class: "modal",
                            onclick: move |e| e.stop_propagation(),
                            h3 { class: "modal-title", "Restore your last session?" }
                            p { class: "modal-text",
                                if count == 1 {
                                    "Recvrve didn't close properly last time. Restore its tab from the autosave?"
                                } else {
                                    "Recvrve didn't close properly last time. Restore its {count} tabs from the autosave?"
                                }
                            }
                            ul { class: "restore-list",
                                for (i, name) in names.iter().enumerate() {
                                    li { key: "{i}", "{name}" }
                                }
                            }
                            div { class: "modal-buttons",
                                button {
                                    class: "btn",
                                    title: "Delete the autosaved tabs",
                                    onclick: move |_| {
                                        autosave::discard(&for_discard);
                                        restore_offer.set(None);
                                    },
                                    "Discard"
                                }
                                button {
                                    class: "btn primary",
                                    onclick: move |_| {
                                        let Some(crashed) = restore_offer.take() else {
                                            return;
                                        };
                                        let mut failed = 0;
                                        for tab in &crashed.tabs {
                                            match save::decode(&tab.bytes) {
                                                Ok(loaded) => {
                                                    let stem: String = tab.entry.name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
                                                    act.load_tree.call((loaded, stem, None, tab.entry.source.clone(), !tab.entry.dirty));
                                                }
                                                Err(_) => failed += 1,
                                            }
                                        }
                                        if failed > 0 {
                                            act.show_notice.call(format!("{failed} autosaved tab(s) could not be read."));
                                        }
                                        autosave::discard(&crashed);
                                    },
                                    "Restore"
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(id) = close_confirm() {
            {
                let name = tab_list.read().iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
                rsx! {
                    div {
                        class: "modal-backdrop",
                        onclick: move |_| close_confirm.set(None),
                        div {
                            class: "modal",
                            onclick: move |e| e.stop_propagation(),
                            p { class: "modal-text", "\u{201c}{name}\u{201d} has unsaved changes. Close it anyway?" }
                            div { class: "modal-buttons",
                                button { class: "btn", onclick: move |_| close_confirm.set(None), "Cancel" }
                                button {
                                    class: "btn primary",
                                    onclick: move |_| {
                                        close_confirm.set(None);
                                        act.close_tab.call(id);
                                    },
                                    "Close tab"
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(unsaved) = window_close_confirm() {
            {
                let text = if unsaved == 1 {
                    "A tab in this window has unsaved changes. Close the window anyway?".to_string()
                } else {
                    format!("{unsaved} tabs in this window have unsaved changes. Close the window anyway?")
                };
                rsx! {
                    div {
                        class: "modal-backdrop",
                        onclick: move |_| window_close_confirm.set(None),
                        div {
                            class: "modal",
                            onclick: move |e| e.stop_propagation(),
                            p { class: "modal-text", "{text}" }
                            div { class: "modal-buttons",
                                button { class: "btn", onclick: move |_| window_close_confirm.set(None), "Cancel" }
                                button {
                                    class: "btn primary",
                                    onclick: move |_| {
                                        window_close_confirm.set(None);
                                        act.close_window.call(());
                                    },
                                    "Close window"
                                }
                            }
                        }
                    }
                }
            }
        }
        if merge_open() {
            merge_panel::MergePanel {
                files: merge_files,
                tabs: tab_list.read().iter().map(|t| (t.id, t.name.clone())).collect::<Vec<_>>(),
                chosen_tabs: merge_tabs,
                busy: dialog_open(),
                on_merge: move |_| act.run_merge.call(()),
                on_close: move |_| merge_open.set(false),
            }
        }
        if let Some((_, message)) = notice() {
            div { class: if notice_leaving() { "notice leaving" } else { "notice" }, onclick: move |_| notice.set(None), "{message}" }
        }
        if let Some((node, x, y)) = preview() {
            node_preview::NodePreview { node, x, y }
        }
        if let Some(message) = warning() {
            div {
                class: "modal-backdrop",
                onclick: move |_| warning.set(None),
                div {
                    class: "modal",
                    onclick: move |e| e.stop_propagation(),
                    button {
                        class: "modal-close",
                        title: "Close",
                        onclick: move |_| warning.set(None),
                        "×"
                    }
                    h3 { class: "modal-title", "Warning" }
                    p { class: "modal-text", "{message}" }
                    button { class: "btn", onclick: move |_| warning.set(None), "OK" }
                }
            }
        }
        if let Some(message) = error() {
            div {
                class: "modal-backdrop",
                onclick: move |_| error.set(None),
                div {
                    class: "modal",
                    // Clicks inside the popup shouldn't reach the backdrop and close it.
                    onclick: move |e| e.stop_propagation(),
                    button {
                        class: "modal-close",
                        title: "Close",
                        onclick: move |_| error.set(None),
                        "×"
                    }
                    p { class: "modal-text", "{message}" }
                    button { class: "btn", onclick: move |_| error.set(None), "OK" }
                }
            }
        }
    }
}
