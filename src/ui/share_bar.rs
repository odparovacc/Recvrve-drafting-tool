//! The share bar: the tree as a text string to copy or paste.

use super::*;

#[component]
pub fn ShareBar() -> Element {
    let ctx = use_context::<Ctx>();
    let Ctx {
        share_open,
        fullscreen,
        share_draft,
        mut save_selection,
        share_text,
        act,
        ..
    } = ctx;

    if !share_open() || fullscreen() {
        return rsx! {};
    }

    // What the box shows: the typed text while it is still about the current tree (with
    // why it does not load), else the current tree's string.
    let share_text = share_text();
    let (share_value, share_error) = match &*share_draft.read() {
        Some((typed, shown, why)) if *shown == share_text => {
            let error = (!typed.trim().is_empty()).then(|| why.clone());
            (typed.clone(), error)
        }
        _ => (share_text.clone(), None),
    };

    rsx! {
        div { class: "share-bar",
            span { class: "share-label", "Tree string" }
            input {
                id: "share-input",
                class: if share_error.is_some() { "share-input invalid" } else { "share-input" },
                r#type: "text",
                spellcheck: "false",
                value: "{share_value}",
                placeholder: "Paste a tree string to load it",
                oninput: move |e: FormEvent| act.on_share_input.call(e.value()),
            }
            if let Some(why) = &share_error {
                span { class: "share-status", "{why}" }
            }
            label {
                class: "share-check",
                title: "Store which node is selected in the string",
                input {
                    r#type: "checkbox",
                    checked: save_selection(),
                    onchange: move |e| save_selection.set(e.checked()),
                }
                "Include selected node"
            }
            button {
                class: "btn",
                title: "Copy the string to the clipboard",
                onclick: move |_| {
                    document::eval(
                        "const el = document.getElementById('share-input'); el.select(); document.execCommand('copy');",
                    );
                },
                "Copy"
            }
        }
    }
}
