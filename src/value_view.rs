//! Shared rendering of node values: champions, groups and the empty values.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::champions;
use crate::groups::{self, GroupCatalog};
use crate::icon_url;
use crate::role_icon;
use crate::roles::{self, Role};
use crate::theme::Theme;
use crate::tree::{DraftTree, NO_BAN};

/// Provided by `App` so value renderers can find group definitions and colors.
#[derive(Clone, Copy)]
pub struct ViewContext {
    pub tree: Signal<DraftTree>,
    /// Group value -> color index, assigned in order as groups are first used.
    pub colors: Signal<HashMap<u8, usize>>,
    /// The active theme, which supplies the group colors.
    pub theme: Memo<Theme>,
}

/// The color of a group that has been used, from the theme's group colors (or generated
/// once those run out).
pub fn group_color(colors: &HashMap<u8, usize>, theme: &Theme, value: u8) -> Option<String> {
    colors.get(&value).map(|&i| theme.group_color(i))
}

/// Human-readable name of a value.
pub fn value_name(groups: &GroupCatalog, value: u8) -> String {
    if let Some(c) = champions::by_value(value) {
        return c.name.to_string();
    }
    if let Some(g) = groups.get(value) {
        return format!("{} ({})", g.name, g.label);
    }
    match value {
        NO_BAN => "No ban".into(),
        0 => "Empty".into(),
        v => format!("Unknown value {v}"),
    }
}

/// Tooltip text for a group: name, label, value, note and members.
pub fn group_tooltip(group: &groups::Group) -> String {
    let mut text = format!("{} ({}, value {})", group.name, group.label, group.value);
    if !group.note.is_empty() {
        text.push('\n');
        text.push_str(&group.note);
    }
    let names = group.member_names();
    let anygroup = if group.label == "ANY" {
        "Just pick me anything \n - TL Dardoch \n"
    } else {
        ""
    };
    text.push_str(&format!(
        "\n{}\n{} champions: {}",
        anygroup,
        names.len(),
        names.join(", ")
    ));
    text
}

/// A square icon for any value: champion portrait, group tile or the empty sign.
#[component]
pub fn ValueIcon(value: u8) -> Element {
    let ctx = use_context::<ViewContext>();
    if let Some(c) = champions::by_value(value) {
        return rsx! {
            img { src: "{icon_url(c)}", alt: "{c.name}", draggable: "false" }
        };
    }
    let tree = ctx.tree.read();
    if let Some(g) = tree.groups().get(value) {
        let color = group_color(&ctx.colors.read(), &ctx.theme.read(), value)
            .unwrap_or_else(|| "var(--muted)".to_string());
        return rsx! {
            div { class: "group-icon", style: "--group-color: {color};", "{g.label}" }
        };
    }
    rsx! {
        div { class: "wildcard-icon", if groups::is_group(value) { "?" } else { "∅" } }
    }
}

/// One Riot position icon, drawn in the current text color with the faint parts of the
/// map dimmed.
#[component]
pub fn RoleGlyph(role: Role) -> Element {
    let icon = role_icon::icon(role);
    rsx! {
        svg {
            class: "role-glyph",
            view_box: "{icon.view_box()}",
            for layer in icon.layers.iter() {
                path {
                    d: "{layer.d}",
                    fill_rule: if layer.even_odd { "evenodd" } else { "nonzero" },
                    opacity: if layer.dim { role_icon::DIM_OPACITY } else { 1.0 },
                }
            }
        }
    }
}

/// The row of role tags along the bottom edge of a tile. Nothing for untagged nodes.
#[component]
pub fn RoleIcons(mask: u8) -> Element {
    if mask == 0 {
        return rsx! {};
    }
    rsx! {
        div { class: "role-row", title: "Roles: {roles::describe(mask)}",
            for role in roles::roles_in(mask) {
                RoleGlyph { key: "{role.key()}", role }
            }
        }
    }
}
