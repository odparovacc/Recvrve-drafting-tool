//! The fearless draft editor: which game of the series this is, hard or soft fearless, and
//! the champions picked in each earlier game.

use dioxus::prelude::*;

use crate::champions::{self, CHAMPIONS};
use crate::fearless::{Fearless, HALF, MAX_GAME, Mode, SLOTS};
use crate::value_view::{ValueIcon, ViewContext};

/// The first empty slot at or after `from` (game, slot) among the active rows, wrapping
/// around once.
fn next_empty(f: &Fearless, from: (usize, usize)) -> Option<(usize, usize)> {
    let games = usize::from(f.game) - 1;
    let total = games * SLOTS;
    if total == 0 {
        return None;
    }
    let start = (from.0 - 1) * SLOTS + from.1;
    (0..total)
        .map(|k| (start + k) % total)
        .map(|i| (i / SLOTS + 1, i % SLOTS))
        .find(|&(g, slot)| f.rows[g - 1][slot] == 0)
}

#[component]
pub fn FearlessEditor(
    initial: Fearless,
    on_apply: EventHandler<Fearless>,
    on_close: EventHandler<()>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let mut draft = use_signal(|| initial);
    // The slot the next chosen champion goes into: (game 1-6, slot 0-9).
    let mut selected = use_signal(|| next_empty(&initial, (1, 0)));
    let mut search = use_signal(String::new);

    let f = draft();
    let earlier = usize::from(f.game) - 1;
    let dirty = f != initial;

    // What applying would do to the tree.
    let (nodes, branches) = {
        let mut t = ctx.tree.read().clone();
        t.set_fearless(f);
        let unreachable = t.unreachable();
        let tops = unreachable
            .keys()
            .filter(|&id| {
                t.get(id)
                    .and_then(|n| n.parent())
                    .is_none_or(|p| !unreachable.contains_key(p))
            })
            .count();
        (unreachable.len(), tops)
    };

    let query = search.read().to_lowercase();
    let mut champs: Vec<(u8, &'static str)> = CHAMPIONS
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name.to_lowercase().contains(&query))
        .map(|(i, c)| (champions::value_of(i), c.name))
        .collect();
    champs.sort_by_key(|&(_, name)| name);

    let mut place = move |value: u8| {
        let mut f = draft();
        let Some(target) = selected().or_else(|| next_empty(&f, (1, 0))) else {
            return;
        };
        if !f.can_place(target.0, target.1, value) {
            return;
        }
        f.rows[target.0 - 1][target.1] = value;
        draft.set(f);
        selected.set(next_empty(&f, target));
    };

    let summary = if earlier == 0 {
        "Game 1 has no fearless bans.".to_string()
    } else {
        format!(
            "{} of {} champions entered for games 1–{}.",
            f.count(),
            earlier * SLOTS,
            earlier
        )
    };
    let preview = if nodes == 0 {
        "No branches of the tree are affected.".to_string()
    } else {
        format!(
            "Applying makes {nodes} node{} in {branches} branch{} unreachable.",
            if nodes == 1 { "" } else { "s" },
            if branches == 1 { "" } else { "es" }
        )
    };

    rsx! {
        div { class: "modal-backdrop",
            div {
                class: "modal editor fearless-editor",
                onclick: move |e| e.stop_propagation(),
                h2 { class: "help-title", "Fearless draft" }
                div { class: "fl-options",
                    span { class: "export-label", "Game" }
                    div { class: "seg",
                        for g in 1..=MAX_GAME {
                            button {
                                key: "{g}",
                                class: if f.game == g { "seg-btn active" } else { "seg-btn" },
                                title: if g == 1 { "First game: no fearless bans".to_string() } else { format!("Game {g}: champions from games 1–{} are locked", g - 1) },
                                onclick: move |_| {
                                    let mut f = draft();
                                    f.game = g;
                                    draft.set(f);
                                    selected.set(next_empty(&f, (1, 0)));
                                },
                                "G{g}"
                            }
                        }
                    }
                    span { class: "export-label", "Mode" }
                    div { class: "seg fl-mode",
                        for mode in [Mode::Hard, Mode::Soft] {
                            button {
                                key: "{mode.label()}",
                                class: if f.mode == mode { "seg-btn active" } else { "seg-btn" },
                                onclick: move |_| {
                                    let mut f = draft();
                                    f.mode = mode;
                                    draft.set(f);
                                },
                                "{mode.label()}"
                            }
                        }
                    }
                }
                p { class: "export-hint",
                    match f.mode {
                        Mode::Hard => "Hard fearless: champions picked in earlier games can't be picked or banned by either team.",
                        Mode::Soft => "Soft fearless: a team can't pick its own champions from earlier games again. Bans are not affected. Rows are split by the side each team plays on in this game.",
                    }
                }
                div { class: "fl-body",
                    div { class: "fl-rows",
                        if earlier == 0 {
                            p { class: "editor-empty muted", "Choose game 2 or later to enter the champions picked in earlier games." }
                        }
                        for g in 1..=earlier {
                            div { key: "{g}", class: "fl-row",
                                div { class: "fl-row-head",
                                    span { class: "fl-row-title", "Game {g}" }
                                    span { class: "muted", "{f.rows[g - 1].iter().filter(|&&v| v != 0).count()} / {SLOTS}" }
                                    button {
                                        class: "export-remove",
                                        title: "Clear this game",
                                        onclick: move |_| {
                                            let mut f = draft();
                                            f.rows[g - 1] = [0; SLOTS];
                                            draft.set(f);
                                            selected.set(Some((g, 0)));
                                        },
                                        "Clear"
                                    }
                                }
                                div { class: "fl-halves",
                                    for (side, range) in [("Blue side", 0..HALF), ("Red side", HALF..SLOTS)] {
                                        div { key: "{side}", class: if side == "Blue side" { "fl-half blue" } else { "fl-half red" },
                                            span { class: "fl-half-label", "{side}" }
                                            div { class: "fl-slots",
                                                for slot in range {
                                                    {
                                                        let value = f.rows[g - 1][slot];
                                                        let is_selected = selected() == Some((g, slot));
                                                        let class = match (value != 0, is_selected) {
                                                            (_, true) => "fl-slot selected",
                                                            (true, false) => "fl-slot filled",
                                                            (false, false) => "fl-slot",
                                                        };
                                                        let title = champions::by_value(value)
                                                            .map_or("Empty: click to fill this slot next".to_string(), |c| format!("{}: click to remove", c.name));
                                                        rsx! {
                                                            button {
                                                                key: "{slot}",
                                                                class,
                                                                title,
                                                                onclick: move |_| {
                                                                    let mut f = draft();
                                                                    f.rows[g - 1][slot] = 0;
                                                                    draft.set(f);
                                                                    selected.set(Some((g, slot)));
                                                                },
                                                                if value != 0 {
                                                                    ValueIcon { value }
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
                        }
                    }
                    div { class: "picker",
                        input {
                            class: "search picker-search",
                            r#type: "text",
                            placeholder: "Search champions...",
                            value: "{search}",
                            oninput: move |e| search.set(e.value()),
                        }
                        div { class: "fl-picker-grid",
                            for (value, name) in champs {
                                {
                                    let target = selected().or_else(|| next_empty(&f, (1, 0)));
                                    let ok = target.is_some_and(|(g, slot)| f.can_place(g, slot, value));
                                    let used = f.rows.iter().take(earlier).flatten().any(|&v| v == value);
                                    let class = match (ok, used) {
                                        (true, _) => "fl-pick",
                                        (false, true) => "fl-pick used",
                                        (false, false) => "fl-pick off",
                                    };
                                    rsx! {
                                        button {
                                            key: "{value}",
                                            class,
                                            disabled: !ok,
                                            title: "{name}",
                                            onclick: move |_| place(value),
                                            ValueIcon { value }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "editor-footer",
                    div { class: "editor-problems",
                        span { class: "muted", "{summary} " }
                        span { class: if nodes > 0 { "fl-preview" } else { "muted" }, "{preview}" }
                    }
                    button {
                        class: "btn",
                        disabled: f.rows == [[0; SLOTS]; crate::fearless::ROWS],
                        title: "Empty every game, including games after the current one",
                        onclick: move |_| {
                            let mut f = draft();
                            f.rows = [[0; SLOTS]; crate::fearless::ROWS];
                            draft.set(f);
                            selected.set(next_empty(&f, (1, 0)));
                        },
                        "Clear all"
                    }
                    button { class: "btn", onclick: move |_| on_close.call(()), "Cancel" }
                    button {
                        class: "btn primary",
                        disabled: !dirty,
                        onclick: move |_| on_apply.call(draft()),
                        "Apply"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_empty_wraps_over_active_rows() {
        let mut f = Fearless {
            game: 3,
            ..Fearless::default()
        };
        assert_eq!(next_empty(&f, (1, 0)), Some((1, 0)));
        f.rows[0] = [16; SLOTS];
        assert_eq!(next_empty(&f, (1, 4)), Some((2, 0)));
        f.rows[1] = [17; SLOTS];
        f.rows[1][3] = 0;
        assert_eq!(next_empty(&f, (2, 5)), Some((2, 3)));
        f.rows[1][3] = 18;
        assert_eq!(next_empty(&f, (1, 0)), None);
        f.game = 1;
        assert_eq!(next_empty(&f, (1, 0)), None);
    }
}
