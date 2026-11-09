//! The panel for merging save files: a list of files to merge (added in any number of
//! rounds, from any folders), the open tabs to merge too, and the Merge button.
//! The merging itself happens in `main.rs` (see `merge.rs`).

use std::path::PathBuf;

use dioxus::prelude::*;

#[component]
pub fn MergePanel(
    /// The files picked so far.
    files: Signal<Vec<PathBuf>>,
    /// The open tabs (id and name), in strip order.
    tabs: Vec<(u64, String)>,
    /// The tabs ticked to merge too.
    chosen_tabs: Signal<Vec<u64>>,
    /// A file dialog or the merge is running.
    busy: bool,
    on_merge: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    let count = files.read().len() + chosen_tabs.read().len();
    let add_files = move |_| {
        spawn(async move {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Add save files to merge")
                .set_directory(crate::save::saves_dir())
                .add_filter("Recvrve saves", &["dts", "dtt", "DTS", "DTT"])
                .add_filter("All files", &["*"])
                .pick_files()
                .await;
            for handle in picked.unwrap_or_default() {
                let path = handle.path().to_path_buf();
                if !files.peek().contains(&path) {
                    files.write().push(path);
                }
            }
        });
    };
    rsx! {
        div { class: "modal-backdrop",
            div { class: "modal merge-panel", onclick: move |e| e.stop_propagation(),
                button {
                    class: "modal-close",
                    title: "Close",
                    onclick: move |_| on_close.call(()),
                    "×"
                }
                h3 { class: "modal-title", "Merge save files" }
                p { class: "merge-hint",
                    "Lines that appear in several trees are kept once; everything else is added. The merged tree is saved as a .dtt file and loaded."
                }
                div { class: "merge-list",
                    if files.read().is_empty() {
                        p { class: "merge-empty", "No files yet." }
                    }
                    for (i, path) in files.read().iter().enumerate() {
                        div { key: "{path.display()}", class: "merge-row", title: "{path.display()}",
                            span { class: "merge-name",
                                {path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()}
                            }
                            span { class: "merge-dir",
                                {path.parent().map(|d| d.display().to_string()).unwrap_or_default()}
                            }
                            button {
                                class: "merge-remove",
                                title: "Remove from the list",
                                onclick: move |_| {
                                    files.write().remove(i);
                                },
                                "×"
                            }
                        }
                    }
                }
                div { class: "merge-actions",
                    button { class: "btn", disabled: busy, onclick: add_files, "Add files…" }
                }
                h4 { class: "merge-subtitle", "Open tabs" }
                div { class: "merge-tabs",
                    for (id, name) in tabs {
                        label { key: "{id}", class: "export-check",
                            input {
                                r#type: "checkbox",
                                checked: chosen_tabs.read().contains(&id),
                                onchange: move |e| {
                                    let mut chosen = chosen_tabs.write();
                                    chosen.retain(|&c| c != id);
                                    if e.checked() {
                                        chosen.push(id);
                                    }
                                },
                            }
                            "{name}"
                        }
                    }
                }
                div { class: "merge-actions",
                    button { class: "btn", onclick: move |_| on_close.call(()), "Cancel" }
                    button {
                        class: "btn primary",
                        disabled: busy || count < 2,
                        title: if count < 2 { "Pick at least two trees" } else { "Merge and save" },
                        onclick: move |_| on_merge.call(()),
                        "Merge {count} trees…"
                    }
                }
            }
        }
    }
}
