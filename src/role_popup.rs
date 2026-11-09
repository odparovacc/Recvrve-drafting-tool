//! The role tag popup, opened by middle-clicking a pick in the graph or the side panels.

use dioxus::prelude::*;

use crate::roles::{self, Role};
use crate::tree::NodeId;
use crate::value_view::{RoleGlyph, ViewContext, value_name};

/// Where the popup is and which node it edits.
#[derive(Clone, PartialEq)]
pub struct RolePopupState {
    pub node: NodeId,
    pub x: f64,
    pub y: f64,
    /// Undo snapshot from when the popup opened: everything changed while it is open is
    /// one undo step.
    pub before: crate::history::State,
}

#[component]
pub fn RolePopup(
    node: NodeId,
    x: f64,
    y: f64,
    on_set: EventHandler<u8>,
    on_close: EventHandler<()>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let tree = ctx.tree.read();
    let Some(n) = tree.get(node) else {
        return rsx! {};
    };
    let mask = n.roles();
    let limit = tree.role_limit(n.value());
    let locked = limit != roles::ALL;
    let title = value_name(tree.groups(), n.value());
    let pos = n.pos();
    let where_ = format!(
        "{} {} {}",
        pos.team().label(),
        pos.action().label().to_lowercase(),
        pos.number()
    );
    // Keep the popup on screen near the click.
    let left = (x + 8.0).max(8.0);
    let top = (y + 8.0).max(8.0);

    let options: Vec<(Role, bool, bool, String)> = Role::ALL
        .into_iter()
        .map(|role| {
            let on = mask & role.bit() != 0;
            let next = mask ^ role.bit();
            let allowed = tree.roles_fit(node, next, true);
            let reason = match allowed {
                Ok(()) => String::new(),
                Err(crate::tree::TreeError::RolesNotAllowed) => {
                    "Outside this group's role lock".to_string()
                }
                Err(_) => "Would leave the team's tagged picks without distinct roles".to_string(),
            };
            (role, on, allowed.is_ok(), reason)
        })
        .collect();
    let clear_value = if locked { limit } else { 0 };
    let clear_ok = tree.roles_fit(node, clear_value, true).is_ok();

    rsx! {
        div {
            class: "role-popup-backdrop",
            onmousedown: move |_| on_close.call(()),
            oncontextmenu: move |e| {
                e.prevent_default();
                on_close.call(());
            },
        }
        div {
            class: "role-popup",
            style: "left: min({left}px, calc(100vw - 230px)); top: min({top}px, calc(100vh - 300px));",
            div { class: "role-popup-head",
                span { class: "role-popup-title", "{title}" }
                span { class: "muted", "{where_}" }
                button { class: "modal-close", onclick: move |_| on_close.call(()), "×" }
            }
            div { class: "role-popup-list",
                for (i, (role, on, allowed, reason)) in options.into_iter().enumerate() {
                    button {
                        key: "{role.key()}",
                        class: if on { "role-toggle active" } else { "role-toggle" },
                        disabled: !allowed,
                        title: if allowed { crate::keys::hint(crate::keys::Action::ToggleRole(i)).unwrap_or_default() } else { reason },
                        onclick: move |_| on_set.call(mask ^ role.bit()),
                        RoleGlyph { role }
                        span { "{role.label()}" }
                        span { class: "role-check", if on { "✓" } }
                    }
                }
            }
            button {
                class: "btn small role-clear",
                disabled: mask == clear_value || !clear_ok,
                onclick: move |_| on_set.call(clear_value),
                if locked { "Reset to group lock" } else { "Any role (clear)" }
            }
            p { class: "export-hint",
                if mask == 0 { "Any role." } else { "Tagged: {roles::describe(mask)}." }
            }
        }
    }
}
