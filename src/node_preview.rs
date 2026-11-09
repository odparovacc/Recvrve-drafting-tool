//! The hover preview of a graph node: the draft state at that node, as the two team panels
//! (small, side by side), with a row of portraits for each group on the line above them.
//! It only shows; the real team panels keep showing the selected node.

use dioxus::prelude::*;

use crate::champions;
use crate::draft::{DRAFT_ORDER, Team};
use crate::icon_url;
use crate::tree::NodeId;
use crate::value_view::{ViewContext, group_color, value_name};

/// Portraits per group row before "+N".
const GROUP_ROW: usize = 10;

#[component]
pub fn NodePreview(node: NodeId, x: f64, y: f64) -> Element {
    let ctx = use_context::<ViewContext>();
    let t = ctx.tree.read();
    let Some(n) = t.get(node) else {
        return rsx! {};
    };
    if node == t.root() {
        return rsx! {};
    }
    let line = crate::line_data(&t, node);
    let t_names = t.team_names().clone();
    let at = line.values.len() - 1;
    let value = n.value();
    // A header only when the node's value isn't a champion (the portrait says enough).
    let header = (champions::by_value(value).is_none()).then(|| {
        format!(
            "{} · {}",
            DRAFT_ORDER[at].code(),
            value_name(t.groups(), value)
        )
    });
    // Groups on the line, once each, in draft order.
    let mut groups = Vec::new();
    for &v in &line.values {
        if let Some(g) = t.groups().get(v)
            && !groups
                .iter()
                .any(|g2: &&crate::groups::Group| g2.value == v)
        {
            groups.push(g);
        }
    }
    let group_rows: Vec<(
        String,
        String,
        String,
        Vec<&'static champions::ChampionInfo>,
        usize,
    )> = groups
        .iter()
        .map(|g| {
            let color = group_color(&ctx.colors.read(), &ctx.theme.read(), g.value)
                .unwrap_or_else(|| "var(--muted)".to_string());
            let members: Vec<_> = g
                .members
                .iter()
                .filter_map(|&m| champions::by_value(m))
                .collect();
            let more = members.len().saturating_sub(GROUP_ROW);
            (
                g.label.clone(),
                g.name.clone(),
                color,
                members.into_iter().take(GROUP_ROW).collect(),
                more,
            )
        })
        .collect();
    drop(t);

    // Keep it on screen: right of and below the pointer, pulled back near the edges.
    let style = format!(
        "left: clamp(8px, {}px, calc(100vw - var(--preview-w) - 8px)); top: clamp(8px, {}px, calc(100vh - var(--preview-h) - 8px));",
        x + 18.0,
        y + 18.0
    );
    rsx! {
        div { class: "node-preview", style: "{style}",
            if let Some(h) = header {
                div { class: "preview-header", "{h}" }
            }
            for (label, name, color, members, more) in group_rows {
                div { class: "preview-group", key: "{label}",
                    span { class: "preview-group-label", style: "--group-color: {color};", "{label}" }
                    span { class: "preview-group-name", "{name}" }
                    div { class: "preview-group-members",
                        for c in members {
                            img { key: "{c.name}", src: "{icon_url(c)}", alt: "{c.name}", draggable: "false" }
                        }
                        if more > 0 {
                            span { class: "preview-more", "+{more}" }
                        }
                    }
                }
            }
            div { class: "preview-teams",
                for (i, team) in [Team::Blue, Team::Red].into_iter().enumerate() {
                    crate::TeamPanel {
                        key: "{team.class()}",
                        team,
                        values: line.values.clone(),
                        step: at,
                        ids: line.ids.clone(),
                        tags: line.tags.clone(),
                        role_slots: line.role_slots[i].clone(),
                        on_roles: move |_| {},
                        name: t_names[i].clone(),
                    }
                }
            }
        }
    }
}
