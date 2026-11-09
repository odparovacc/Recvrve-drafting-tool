//! Theme settings panel: edits the selected theme's `theme.json`, previewing every change
//! live. Saving writes the file (keeping a `.bak`); "Save as new" copies it into a new
//! theme folder.

use dioxus::prelude::*;

use crate::theme::{self, Base, Colors, Fit, Theme};

/// A color with a picker and a hex text box. Invalid text is ignored.
#[component]
fn ColorField(label: String, value: String, on_change: EventHandler<String>) -> Element {
    rsx! {
        label { class: "theme-field",
            span { class: "theme-field-label", "{label}" }
            input {
                r#type: "color",
                class: "theme-color",
                value: "{value}",
                oninput: move |e| {
                    if let Some(c) = theme::normalize_color(&e.value()) {
                        on_change.call(c);
                    }
                },
            }
            input {
                r#type: "text",
                class: "search theme-hex",
                value: "{value}",
                onchange: move |e| {
                    if let Some(c) = theme::normalize_color(&e.value()) {
                        on_change.call(c);
                    }
                },
            }
        }
    }
}

/// A labelled range slider.
#[component]
fn Slider(
    label: String,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    unit: String,
    on_change: EventHandler<f32>,
) -> Element {
    rsx! {
        label { class: "theme-field",
            span { class: "theme-field-label", "{label}" }
            input {
                r#type: "range",
                class: "theme-range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f32>() {
                        on_change.call(v.clamp(min, max));
                    }
                },
            }
            span { class: "theme-value", "{value}{unit}" }
        }
    }
}

#[component]
pub fn ThemeEditor(
    initial: Theme,
    /// Called with every change, so the app can show it right away.
    on_preview: EventHandler<Theme>,
    /// Save the theme: `false` to its own folder, `true` as a new theme.
    on_save: EventHandler<(Theme, bool)>,
    on_close: EventHandler<()>,
) -> Element {
    let mut draft = use_signal(|| initial.clone());
    let mut images = use_signal(|| theme::images_in(&initial.id));
    let mut new_name = use_signal(|| None::<String>);
    let mut notice = use_signal(|| None::<String>);

    // Blur and picture can change on every slider move: the preview blurs live until saved,
    // instead of building a cache file for each step.
    let saved = initial.clone();
    use_effect(move || {
        let mut t = draft();
        t.live_blur = t.blur != saved.blur
            || t.background != saved.background
            || t.fit != saved.fit
            || t.background_version != saved.background_version;
        on_preview.call(t);
    });

    let t = draft();
    let dirty = t != initial;
    let folder = t.dir().display().to_string();
    let mut warnings = t.warnings.clone();
    if let Some(n) = notice() {
        warnings.insert(0, n);
    }

    rsx! {
        div { class: "modal-backdrop",
            div {
                class: "modal editor theme-editor",
                onclick: move |e| e.stop_propagation(),
                h2 { class: "help-title", "Theme settings" }
                div { class: "theme-body",
                    div { class: "theme-col",
                        div { class: "export-label", "General" }
                        label { class: "theme-field",
                            span { class: "theme-field-label", "Name" }
                            input {
                                class: "search theme-text",
                                r#type: "text",
                                value: "{t.name}",
                                oninput: move |e| draft.write().name = e.value(),
                            }
                        }
                        div { class: "theme-field",
                            span { class: "theme-field-label", "Base" }
                            div { class: "seg",
                                for base in [Base::Dark, Base::Light] {
                                    button {
                                        key: "{base.class()}",
                                        class: if t.base == base { "seg-btn active" } else { "seg-btn" },
                                        title: "Team colors and anything the theme leaves out follow the base",
                                        onclick: move |_| draft.write().base = base,
                                        if base == Base::Dark { "Dark" } else { "Light" }
                                    }
                                }
                            }
                        }

                        div { class: "export-label", "Background picture" }
                        div { class: "theme-field",
                            span { class: "theme-field-label", "Picture" }
                            select {
                                class: "export-select theme-select",
                                onchange: move |e| {
                                    let v = e.value();
                                    let mut d = draft.write();
                                    d.background = (!v.is_empty()).then_some(v);
                                    d.background_version += 1;
                                },
                                option { value: "", selected: t.background.is_none(), "None" }
                                for img in images() {
                                    option {
                                        key: "{img}",
                                        value: "{img}",
                                        selected: t.background.as_deref() == Some(img.as_str()),
                                        "{img}"
                                    }
                                }
                            }
                            button {
                                class: "btn small",
                                title: "Look for new pictures in the theme folder",
                                onclick: move |_| images.set(theme::images_in(&draft.peek().id)),
                                "Refresh"
                            }
                        }
                        p { class: "export-hint theme-path", "Pictures go into {folder}" }
                        div { class: "theme-field",
                            span { class: "theme-field-label", "Fit" }
                            div { class: "seg",
                                for fit in Fit::ALL {
                                    button {
                                        key: "{fit.label()}",
                                        class: if t.fit == fit { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| draft.write().fit = fit,
                                        "{fit.label()}"
                                    }
                                }
                            }
                        }
                        Slider {
                            label: "Dim picture",
                            value: f32::from(t.dim),
                            min: 0.0,
                            max: 90.0,
                            step: 5.0,
                            unit: "%",
                            on_change: move |v: f32| draft.write().dim = v as u8,
                        }
                        Slider {
                            label: "Panel opacity",
                            value: f32::from(t.panel_opacity),
                            min: 20.0,
                            max: 100.0,
                            step: 5.0,
                            unit: "%",
                            on_change: move |v: f32| draft.write().panel_opacity = v as u8,
                        }
                        Slider {
                            label: "Panel blur",
                            value: f32::from(t.blur),
                            min: 0.0,
                            max: 20.0,
                            step: 1.0,
                            unit: "px",
                            on_change: move |v: f32| draft.write().blur = v as u8,
                        }

                        div { class: "export-label", "Graph lines" }
                        ColorField {
                            label: "Branches",
                            value: t.graph.edge.clone(),
                            on_change: move |c| draft.write().graph.edge = c,
                        }
                        ColorField {
                            label: "Selected path",
                            value: t.graph.edge_active.clone(),
                            on_change: move |c| draft.write().graph.edge_active = c,
                        }
                        ColorField {
                            label: "Turn lines",
                            value: t.graph.turn_line.clone(),
                            on_change: move |c| draft.write().graph.turn_line = c,
                        }
                        Slider {
                            label: "Turn width",
                            value: t.graph.turn_width,
                            min: 0.0,
                            max: 6.0,
                            step: 0.5,
                            unit: "px",
                            on_change: move |v| draft.write().graph.turn_width = v,
                        }
                        ColorField {
                            label: "Phase lines",
                            value: t.graph.phase_line.clone(),
                            on_change: move |c| draft.write().graph.phase_line = c,
                        }
                        Slider {
                            label: "Phase width",
                            value: t.graph.phase_width,
                            min: 0.0,
                            max: 10.0,
                            step: 0.5,
                            unit: "px",
                            on_change: move |v| draft.write().graph.phase_width = v,
                        }
                    }

                    div { class: "theme-col",
                        div { class: "export-label", "Colors" }
                        for (key, label) in Colors::FIELDS {
                            ColorField {
                                key: "{key}",
                                label,
                                value: t.colors.get(key).to_string(),
                                on_change: move |c| draft.write().colors.set(key, c),
                            }
                        }
                        button {
                            class: "btn small theme-reset",
                            title: "Set every color and graph line back to the base theme's",
                            onclick: move |_| {
                                let base = Theme::builtin(draft.peek().base);
                                let mut d = draft.write();
                                d.colors = base.colors;
                                d.graph = base.graph;
                            },
                            "Reset colors to base"
                        }

                        div { class: "export-label", "Group colors" }
                        p { class: "export-hint",
                            "Handed out in order as groups are used; extra groups get generated colors. Shades of team blue, team red and the fearless purple are not allowed."
                        }
                        div { class: "theme-swatches",
                            for (i, color) in t.group_colors.iter().cloned().enumerate() {
                                div { key: "{i}", class: "theme-swatch",
                                    input {
                                        r#type: "color",
                                        class: "theme-color",
                                        value: "{color}",
                                        title: "{color}",
                                        oninput: move |e| {
                                            let Some(c) = theme::normalize_color(&e.value()) else { return };
                                            match theme::reserved_clash(&c) {
                                                Some(what) => notice.set(Some(format!("{c} is too close to {what}."))),
                                                None => {
                                                    notice.set(None);
                                                    draft.write().group_colors[i] = c;
                                                }
                                            }
                                        },
                                    }
                                    button {
                                        class: "export-remove",
                                        title: "Remove this color",
                                        onclick: move |_| {
                                            draft.write().group_colors.remove(i);
                                        },
                                        "×"
                                    }
                                }
                            }
                            button {
                                class: "btn small",
                                title: "Add a color",
                                onclick: move |_| {
                                    let n = draft.peek().group_colors.len();
                                    draft.write().group_colors.push(theme::extra_group_color(n));
                                },
                                "+ Add"
                            }
                        }
                    }
                }
                div { class: "editor-footer",
                    div { class: "editor-problems",
                        if let Some(first) = warnings.first() {
                            span { class: "share-status", title: "{warnings.join(\"\\n\")}", "{first}" }
                            if warnings.len() > 1 {
                                span { class: "muted", " (+{warnings.len() - 1} more)" }
                            }
                        } else {
                            span { class: "muted", "Saved to {folder}" }
                        }
                    }
                    if let Some(name) = new_name() {
                        input {
                            class: "search theme-text",
                            r#type: "text",
                            placeholder: "New theme name",
                            value: "{name}",
                            oninput: move |e| new_name.set(Some(e.value())),
                        }
                        button {
                            class: "btn",
                            onclick: move |_| new_name.set(None),
                            "Back"
                        }
                        button {
                            class: "btn primary",
                            disabled: name.trim().is_empty(),
                            onclick: move |_| {
                                let mut d = draft();
                                d.name = new_name().unwrap_or_default().trim().to_string();
                                on_save.call((d, true));
                            },
                            "Create"
                        }
                    } else {
                        button {
                            class: "btn",
                            title: "Copy these settings (and the picture) into a new theme folder",
                            onclick: move |_| new_name.set(Some(format!("{} copy", draft.peek().name))),
                            "Save as new…"
                        }
                        button { class: "btn", onclick: move |_| on_close.call(()), "Cancel" }
                        button {
                            class: "btn primary",
                            disabled: !dirty || t.name.trim().is_empty(),
                            onclick: move |_| on_save.call((draft(), false)),
                            "Save"
                        }
                    }
                }
            }
        }
    }
}
