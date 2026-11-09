//! The Help panel: lists the shortcuts and lets the user change them, chooses which panel
//! buttons are shown (Button settings), and holds the View and theme menus.
//!
//! Recording works through the keyboard listener in `keys.rs`: while `recording` is set,
//! the next combo pressed is sent to the app instead of acting, and the app assigns it
//! (see `main.rs`).

use dioxus::prelude::*;

use crate::buttons::{self, Button, Section};
use crate::configio::UiSettings;
use crate::keys::{self, Action, Keybinds};

/// The action being recorded and the slot the new combo replaces (`None` adds one).
pub type RecordTarget = (Action, Option<usize>);

/// Mouse actions and the fixed Esc key, shown above the keyboard shortcuts.
/// What a branch click does, for the Help panel and the editor.
const BRANCH_HELP: &str =
    "Add a champion or group as a branch without moving; on release, select the last one clicked";

const FIXED_HELP: [(&str, &str); 4] = [
    ("Click", "Choose a champion (follows an existing branch)"),
    ("Middle-click pick", "Edit its role tags"),
    ("Right-click node", "Delete that node and its subtree"),
    ("Esc", "Close popups and editors, then leave fullscreen"),
];

/// Saves `binds`, reporting a failure in `message`.
pub fn persist(binds: &Keybinds, mut message: Signal<Option<String>>) {
    if let Err(e) = binds.save() {
        message.set(Some(format!(
            "Could not save {}: {e}",
            keys::path().display()
        )));
    }
}

/// The message after a combo moved from `from` to another action.
pub fn moved_message(binds: &Keybinds, combo: &keys::Combo, from: Action) -> String {
    let rest = if binds.get(from).is_empty() {
        " It has no shortcut now."
    } else {
        ""
    };
    format!(
        "{} was taken from \"{}\".{rest}",
        combo.display(),
        from.description()
    )
}

/// What the Help panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Shortcuts,
    Customize,
    Buttons,
}

#[component]
pub fn ShortcutsPanel(
    keybinds: Signal<Keybinds>,
    recording: Signal<Option<RecordTarget>>,
    message: Signal<Option<String>>,
    ui_settings: Signal<UiSettings>,
    /// Saves the UI settings after a button is shown or hidden.
    on_settings: EventHandler<()>,
    /// The View and theme menus, next to Customize and Button settings.
    menus: Element,
    on_close: EventHandler<()>,
) -> Element {
    let mut mode = use_signal(|| Mode::Shortcuts);
    let editing = move || mode() == Mode::Customize;
    let binds = keybinds.read();
    let rec = recording();
    let all_default = *binds == Keybinds::default();

    let mut start = move |target: RecordTarget| {
        message.set(None);
        recording.set(Some(target));
    };

    let body = if mode() == Mode::Buttons {
        let overrides = ui_settings.read().buttons.clone();
        rsx! {
            div { class: "btn-settings",
                for section in Section::ALL {
                    div { class: "btn-section", key: "{section.label()}",
                        h3 { class: "btn-section-title", "{section.label()}" }
                        for b in Button::ALL.into_iter().filter(|b| b.section() == section) {
                            {
                                let on = buttons::shown(&overrides, b);
                                let combos: Vec<String> = match b.action() {
                                    Some(Action::ToggleFilter(_)) => {
                                        let first = binds.get(Action::ToggleFilter(0)).first().map(|c| c.display());
                                        let last = binds.get(Action::ToggleFilter(8)).first().map(|c| c.display());
                                        match (first, last) {
                                            (Some(f), Some(l)) => vec![format!("{f} … {l}")],
                                            (Some(f), None) => vec![f],
                                            _ => Vec::new(),
                                        }
                                    }
                                    Some(a) => binds.get(a).iter().map(|c| c.display()).collect(),
                                    None => Vec::new(),
                                };
                                rsx! {
                                    label {
                                        key: "{b.id()}",
                                        class: if on { "btn-toggle on" } else { "btn-toggle" },
                                        title: if on { "Shown: click to hide it" } else { "Hidden: click to show it" },
                                        input {
                                            r#type: "checkbox",
                                            checked: on,
                                            onchange: move |e| {
                                                buttons::set_shown(&mut ui_settings.write().buttons, b, e.checked());
                                                on_settings.call(());
                                            },
                                        }
                                        span { class: "btn-toggle-name", "{b.label()}" }
                                        span { class: "btn-toggle-keys",
                                            for (i, c) in combos.iter().enumerate() {
                                                if i > 0 { " / " }
                                                kbd { "{c}" }
                                            }
                                            if b.default_shown() != on {
                                                span { class: "btn-toggle-changed", title: "Not the default", "•" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if editing() {
        rsx! {
            div { class: "kb-list",
                div { class: "kb-row", key: "branch-click",
                    span { class: "kb-what", "{BRANCH_HELP} (hold, then click)" }
                    div { class: "kb-combos",
                        for mods in keys::ClickMods::options() {
                            button {
                                key: "{mods.id()}",
                                class: if binds.branch_click == mods { "btn small active" } else { "btn small" },
                                title: "Hold {mods.display()} and click",
                                onclick: move |_| {
                                    recording.set(None);
                                    message.set(None);
                                    keybinds.write().branch_click = mods;
                                    persist(&keybinds.peek(), message);
                                },
                                "{mods.display()}"
                            }
                        }
                    }
                    button {
                        class: "btn small",
                        title: "Back to the default (Shift)",
                        disabled: binds.branch_click == keys::ClickMods::SHIFT,
                        onclick: move |_| {
                            keybinds.write().branch_click = keys::ClickMods::SHIFT;
                            persist(&keybinds.peek(), message);
                        },
                        "Reset"
                    }
                }
                for action in Action::all() {
                    {
                        let combos = binds.get(action).to_vec();
                        let adding = rec == Some((action, None));
                        let row_class = if rec.is_some_and(|(a, _)| a == action) { "kb-row recording" } else { "kb-row" };
                        rsx! {
                            div { class: row_class, key: "{action.id()}",
                                span { class: "kb-what", "{action.description()}" }
                                div { class: "kb-combos",
                                    for (i, combo) in combos.into_iter().enumerate() {
                                        {
                                            let now = rec == Some((action, Some(i)));
                                            rsx! {
                                                span { class: if now { "kb-chip recording" } else { "kb-chip" }, key: "{i}",
                                                    button {
                                                        class: "kb-key",
                                                        title: "Click, then press the new keys",
                                                        onclick: move |_| start((action, Some(i))),
                                                        if now { "Press keys…" } else { "{combo.display()}" }
                                                    }
                                                    button {
                                                        class: "kb-remove",
                                                        title: "Remove this shortcut",
                                                        onclick: move |_| {
                                                            recording.set(None);
                                                            keybinds.write().remove(action, i);
                                                            persist(&keybinds.peek(), message);
                                                        },
                                                        "×"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if adding {
                                        span { class: "kb-chip recording",
                                            span { class: "kb-key", "Press keys…" }
                                        }
                                    } else if binds.get(action).is_empty() {
                                        span { class: "kb-none", "none" }
                                    }
                                    button {
                                        class: "btn small kb-add",
                                        title: "Add a shortcut",
                                        onclick: move |_| start((action, None)),
                                        "+"
                                    }
                                }
                                button {
                                    class: "btn small",
                                    title: "Back to the default shortcut",
                                    disabled: binds.is_default(action),
                                    onclick: move |_| {
                                        recording.set(None);
                                        let moved = keybinds.write().reset(action);
                                        let b = keybinds.peek();
                                        persist(&b, message);
                                        if !moved.is_empty() {
                                            let names: Vec<String> = moved.iter().map(|a| format!("\"{}\"", a.description())).collect();
                                            message.set(Some(format!("Taken back from {}.", names.join(", "))));
                                        }
                                    },
                                    "Reset"
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        let rows = binds.help_rows();
        rsx! {
            div { class: "help-list",
                for (combo, what) in [
                    (format!("{} + Click", binds.branch_click.display()), BRANCH_HELP.to_string()),
                    (
                        format!("{} + Click Empty", binds.branch_click.display()),
                        "Add another empty branch, even if one exists".to_string(),
                    ),
                ]
                    .into_iter()
                    .chain(FIXED_HELP.iter().map(|(c, w)| (c.to_string(), w.to_string())))
                {
                    div { class: "help-row", key: "{combo}",
                        span { class: "help-keys", kbd { "{combo}" } }
                        span { "{what}" }
                    }
                }
                for (combos, what) in rows {
                    div { class: "help-row", key: "{what}",
                        span { class: "help-keys",
                            if combos.is_empty() {
                                span { class: "kb-none", "none" }
                            }
                            for (i, c) in combos.iter().enumerate() {
                                if i > 0 { " / " }
                                kbd { "{c}" }
                            }
                        }
                        span { "{what}" }
                    }
                }
            }
        }
    };

    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal help",
                onclick: move |e| e.stop_propagation(),
                button {
                    class: "modal-close",
                    title: "Close",
                    onclick: move |_| on_close.call(()),
                    "×"
                }
                div { class: "help-head",
                    h2 { class: "help-title",
                        if mode() == Mode::Buttons { "Button settings" } else { "Keyboard shortcuts" }
                    }
                    button {
                        class: if editing() { "btn small active" } else { "btn small" },
                        title: "Change the shortcuts",
                        onclick: move |_| {
                            recording.set(None);
                            message.set(None);
                            mode.set(if editing() { Mode::Shortcuts } else { Mode::Customize });
                        },
                        if editing() { "Done" } else { "Customize" }
                    }
                    button {
                        class: if mode() == Mode::Buttons { "btn small active" } else { "btn small" },
                        title: "Choose which buttons the panels show",
                        onclick: move |_| {
                            recording.set(None);
                            message.set(None);
                            mode.set(if mode() == Mode::Buttons { Mode::Shortcuts } else { Mode::Buttons });
                        },
                        if mode() == Mode::Buttons { "Done" } else { "Button settings" }
                    }
                    {menus}
                }
                div { class: "help-body", {body} }
                if editing() {
                    if let Some(m) = message() {
                        p { class: "kb-message", "{m}" }
                    }
                    div { class: "help-foot",
                        p { class: "help-note",
                            "Click a shortcut and press the new keys (Esc cancels). A shortcut needs at least two keys pressed together, such as Ctrl + K or Shift + K. A shortcut already in use moves here. Changes are saved to config/keybinds.json."
                        }
                        button {
                            class: "btn small",
                            disabled: all_default,
                            title: "Every shortcut back to its default",
                            onclick: move |_| {
                                recording.set(None);
                                keybinds.set(Keybinds::default());
                                persist(&keybinds.peek(), message);
                                message.set(Some("All shortcuts are back to their defaults.".into()));
                            },
                            "Reset all"
                        }
                    }
                } else if mode() == Mode::Buttons {
                    div { class: "help-foot",
                        p { class: "help-note",
                            "Hidden buttons leave their panel; shown again, they come back in their usual place. Their shortcuts work either way. Changes are saved to config/settings.json."
                        }
                        button {
                            class: "btn small",
                            disabled: Button::ALL.iter().all(|&b| buttons::shown(&ui_settings.read().buttons, b)),
                            title: "Show every button",
                            onclick: move |_| {
                                buttons::show_all(&mut ui_settings.write().buttons);
                                on_settings.call(());
                            },
                            "Show all"
                        }
                        button {
                            class: "btn small",
                            disabled: ui_settings.read().buttons.is_empty(),
                            title: "Every button back to its default",
                            onclick: move |_| {
                                ui_settings.write().buttons.clear();
                                on_settings.call(());
                            },
                            "Reset to defaults"
                        }
                    }
                } else {
                    p { class: "help-note",
                        "Plain letters are never shortcuts, so you can always type in the search box. Delete only removes nodes while no text field is focused. Ctrl + C, Ctrl + Z and Ctrl + Y act on text while a text field is focused, and nothing works as a shortcut inside the share box."
                    }
                }
            }
        }
    }
}
