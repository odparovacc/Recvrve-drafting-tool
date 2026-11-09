//! The blue and red team columns (or their collapsed bars).

use super::*;

/// One team's column at its side of the board, with the splitter towards the centre; a
/// collapsed bar while the team panels are hidden, nothing in fullscreen.
#[component]
pub fn TeamColumn(team: Team) -> Element {
    let ctx = use_context::<Ctx>();
    let Ctx {
        mut tree,
        follow,
        ui_settings,
        fullscreen,
        line,
        act,
        entering,
        counter_data,
        ..
    } = ctx;
    if fullscreen() {
        return rsx! {};
    }
    let (i, blue) = match team {
        Team::Blue => (0usize, true),
        Team::Red => (1usize, false),
    };
    if ui_settings.read().hidden.teams {
        let bar = rsx! {
            div {
                class: if blue { "collapsed-bar vertical blue" } else { "collapsed-bar vertical red" },
                title: "Show the team panels",
                onclick: move |_| act.set_hidden.call((Panel::Teams, false)),
                if blue { "Blue team ▸" } else { "Red team ▸" }
            }
        };
        // The gap sits on the centre side of the bar.
        return if blue {
            rsx! {
                {bar}
                div { class: "splitter-gap" }
            }
        } else {
            rsx! {
                div { class: "splitter-gap" }
                {bar}
            }
        };
    }
    let l = line.read();
    let panel = rsx! {
        TeamPanel {
            team,
            name: tree.read().team_names()[i].clone(),
            on_rename: move |name: String| {
                if follow.peek().is_none() && tree.peek().team_names()[i] != name.trim() {
                    tree.write().set_team_name(i, &name);
                }
            },
            values: l.values.clone(),
            step: l.step,
            ids: l.path_ids.clone(),
            added: {
                let e = entering.read();
                l.path_ids.iter().map(|&id| e.just_added(id)).collect::<Vec<bool>>()
            },
            tags: l.path_roles.clone(),
            role_slots: l.role_slots[i].clone(),
            counters: Some(slot_counters(&counter_data.read(), &tree.read(), team, &l.values, &l.path_roles)),
            on_roles: act.open_roles,
        }
    };
    if blue {
        rsx! {
            {panel}
            div { class: "splitter col", "data-split": "left", title: SPLITTER_TIP }
        }
    } else {
        rsx! {
            div { class: "splitter col", "data-split": "right", title: SPLITTER_TIP }
            {panel}
        }
    }
}

/// The counter picks for each of `team`'s champion picks on a line with `values` and role
/// `tags` (see `SlotCounters`): champions the other team could still pick, so not banned,
/// not picked by `team` and not locked by fearless for the other team. The other team's
/// own picks stay in.
fn slot_counters(
    data: &counters::Counters,
    tree: &DraftTree,
    team: Team,
    values: &[u8],
    tags: &[u8],
) -> Vec<Option<SlotCounters>> {
    let source = data
        .snapshot
        .as_ref()
        .map(|s| format!("Data: {}", s.describe()))
        .unwrap_or_default();
    let locked = DRAFT_ORDER
        .iter()
        .find(|s| s.team != team && s.action == Action::Pick)
        .map(|s| tree.fearless().locked_at(s.pos()))
        .unwrap_or_default();
    let unavailable = |v: u8| {
        locked.contains(v)
            || values
                .iter()
                .zip(DRAFT_ORDER.iter())
                .any(|(&x, s)| x == v && (s.action == Action::Ban || s.team == team))
    };
    values
        .iter()
        .zip(DRAFT_ORDER.iter())
        .enumerate()
        .map(|(i, (&v, s))| {
            if s.team != team || s.action != Action::Pick {
                return None;
            }
            champions::by_value(v)?;
            let lane = data.lane(v, tags.get(i).copied().unwrap_or(0));
            Some(SlotCounters {
                lane,
                shown: lane
                    .map(|r| data.counters(v, r, unavailable))
                    .unwrap_or_default(),
                source: source.clone(),
            })
        })
        .collect()
}
