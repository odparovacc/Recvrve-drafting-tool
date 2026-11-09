//! Draft state cards: the two team panels of one draft state side by side, as a portrait
//! image.

use crate::draft::{Action, DRAFT_ORDER, Team};
use crate::tree::{EMPTY, NO_BAN};

use super::draw::{value_label, value_tile};
use super::model::{self, Snapshot};
use super::scene::{Anchor, Color, Scene, fit_text, palette};

/// Card width in logical pixels; raster exports scale this to the chosen pixel width.
pub const CARD_W: f32 = 340.0;
const PAD: f32 = 12.0;
const COL_GAP: f32 = 10.0;
const COL_W: f32 = (CARD_W - 2.0 * PAD - COL_GAP) / 2.0;
const HEADER_H: f32 = 30.0;
const TEAM_BAR: f32 = 3.0;
const TEAM_NAME_H: f32 = 22.0;
const BAN_GAP: f32 = 4.0;
/// Extra space between the first three bans and the last two (the two ban phases).
const PHASE_GAP: f32 = 6.0;
const BAN_SIZE: f32 = (COL_W - 4.0 * BAN_GAP - PHASE_GAP) / 5.0;
const PICK_H: f32 = 54.0;
const PICK_GAP: f32 = 6.0;
const PICK_ICON: f32 = PICK_H - 6.0;
const BANS_Y: f32 = PAD + HEADER_H + TEAM_BAR + TEAM_NAME_H;
const PICKS_Y: f32 = BANS_Y + BAN_SIZE + 12.0;
const ROLES_Y: f32 = PICKS_Y + 5.0 * PICK_H + 4.0 * PICK_GAP + 12.0;
const ROLE_GLYPH: f32 = 11.0;
const ROLE_GAP: f32 = 4.0;
const ROLE_SLOT: f32 = (COL_W - 4.0 * ROLE_GAP) / 5.0;
pub const CARD_H: f32 = ROLES_Y + ROLE_GLYPH + 4.0 + ROLE_SLOT + PAD;

/// Step indices of a team's bans or picks, in slot order (ban 1 ... ban 5).
fn slots(team: Team, action: Action) -> Vec<usize> {
    let mut steps: Vec<(usize, u8)> = DRAFT_ORDER
        .iter()
        .enumerate()
        .filter(|(_, s)| s.team == team && s.action == action)
        .map(|(i, s)| (i, s.number))
        .collect();
    steps.sort_by_key(|&(_, n)| n);
    steps.into_iter().map(|(i, _)| i).collect()
}

fn team_color(team: Team) -> Color {
    match team {
        Team::Blue => palette::BLUE,
        Team::Red => palette::RED,
    }
}

/// The card for the draft state at snapshot node `node`. `seq` is its number among the
/// exported states at that depth, out of `count`.
pub fn card(snap: &Snapshot, node: usize, seq: usize, count: usize) -> Scene {
    let values = snap.line_values(node);
    let tags: Vec<u8> = snap
        .path(node)
        .into_iter()
        .skip(1)
        .map(|i| snap.nodes[i].roles)
        .collect();
    let role_slots = snap.role_slots(node);
    let depth = values.len();
    let mut s = Scene::new(CARD_W, CARD_H);

    // Header: which state this is, its number, and a note if fearless rules it out or the
    // line conflicts.
    let mut number = format!("#{}", model::seq_text(seq, count));
    if snap.nodes[node].conflict.is_some() {
        number = format!("conflict · {number}");
    } else if snap.nodes[node].pruned.is_some() {
        number = format!("unreachable · {number}");
    }
    if snap.nodes[node].starred && node != 0 {
        number = format!("★ {number}");
    }
    let number_color = if snap.nodes[node].conflict.is_some() {
        palette::CONFLICT
    } else {
        snap.nodes[node]
            .pruned
            .map_or(palette::MUTED, super::scene::Tone::accent)
    };
    let number_w = super::scene::text_width(&number, 11.0, true);
    let title = if depth == 0 {
        "Empty draft".to_string()
    } else {
        model::depth_title(depth)
    };
    s.text(
        PAD,
        PAD + 15.0,
        14.0,
        palette::TEXT,
        true,
        Anchor::Start,
        fit_text(&title, 14.0, true, CARD_W - 2.0 * PAD - number_w - 10.0),
    );
    s.text(
        CARD_W - PAD,
        PAD + 15.0,
        11.0,
        number_color,
        true,
        Anchor::End,
        number,
    );

    for (col, team) in [Team::Blue, Team::Red].into_iter().enumerate() {
        let x0 = PAD + col as f32 * (COL_W + COL_GAP);
        let color = team_color(team);
        let top = PAD + HEADER_H;
        s.rect(x0, top, COL_W, TEAM_BAR, 1.5, color);
        s.text(
            x0,
            top + TEAM_BAR + 15.0,
            13.0,
            color,
            true,
            Anchor::Start,
            format!("{} Team", team.label()),
        );

        for (n, step) in slots(team, Action::Ban).into_iter().enumerate() {
            let x = x0 + n as f32 * (BAN_SIZE + BAN_GAP) + if n >= 3 { PHASE_GAP } else { 0.0 };
            ban_slot(
                &mut s,
                snap,
                values.get(step).copied(),
                n,
                x,
                BANS_Y,
                step + 1 == depth,
            );
        }
        for (n, step) in slots(team, Action::Pick).into_iter().enumerate() {
            let y = PICKS_Y + n as f32 * (PICK_H + PICK_GAP);
            pick_slot(
                &mut s,
                snap,
                values.get(step).copied(),
                tags.get(step).copied().unwrap_or(0),
                n,
                x0,
                y,
                step + 1 == depth,
            );
        }
        // Role slots: who can play each role on this line.
        for (k, role) in crate::roles::Role::ALL.into_iter().enumerate() {
            let x = x0 + k as f32 * (ROLE_SLOT + ROLE_GAP);
            super::draw::role_glyph(
                &mut s,
                role,
                x + (ROLE_SLOT - ROLE_GLYPH) / 2.0,
                ROLES_Y,
                ROLE_GLYPH,
                palette::MUTED,
            );
            super::draw::role_slot(
                &mut s,
                snap,
                &role_slots[col][k],
                x,
                ROLES_Y + ROLE_GLYPH + 4.0,
                ROLE_SLOT,
            );
        }
    }
    s
}

/// Outline of a slot: dashed for slots the draft hasn't reached, gold for the action the
/// state is "after".
#[allow(clippy::too_many_arguments)]
fn slot_frame(s: &mut Scene, x: f32, y: f32, w: f32, h: f32, r: f32, filled: bool, last: bool) {
    if last {
        s.outline(x, y, w, h, r, palette::GOLD, 2.0, false);
    } else {
        s.outline(x, y, w, h, r, palette::BORDER, 1.0, !filled);
    }
}

fn ban_slot(
    s: &mut Scene,
    snap: &Snapshot,
    value: Option<u8>,
    n: usize,
    x: f32,
    y: f32,
    last: bool,
) {
    match value {
        Some(v) => {
            let ban = v != EMPTY && v != NO_BAN;
            value_tile(
                s,
                snap,
                v,
                x + 1.0,
                y + 1.0,
                BAN_SIZE - 2.0,
                4.0,
                ban,
                0.75,
                None,
            );
        }
        None => s.text(
            x + BAN_SIZE / 2.0,
            y + BAN_SIZE / 2.0 + 3.0,
            8.5,
            palette::MUTED,
            false,
            Anchor::Middle,
            format!("B{}", n + 1),
        ),
    }
    slot_frame(s, x, y, BAN_SIZE, BAN_SIZE, 5.0, value.is_some(), last);
}

#[allow(clippy::too_many_arguments)]
fn pick_slot(
    s: &mut Scene,
    snap: &Snapshot,
    value: Option<u8>,
    roles: u8,
    n: usize,
    x: f32,
    y: f32,
    last: bool,
) {
    match value {
        Some(v) => {
            value_tile(
                s,
                snap,
                v,
                x + 3.0,
                y + 3.0,
                PICK_ICON,
                5.0,
                false,
                1.0,
                None,
            );
            super::draw::role_row(s, roles, x + 3.0, y + 3.0, PICK_ICON);
            let muted = v == EMPTY || v == NO_BAN;
            let tx = x + 3.0 + PICK_ICON + 8.0;
            let name = fit_text(&value_label(snap, v), 12.5, !muted, COL_W - (tx - x) - 4.0);
            s.text(
                tx,
                y + PICK_H / 2.0 + 4.5,
                12.5,
                if muted { palette::MUTED } else { palette::TEXT },
                !muted,
                Anchor::Start,
                name,
            );
        }
        None => s.text(
            x + 10.0,
            y + PICK_H / 2.0 + 4.0,
            11.0,
            palette::MUTED,
            false,
            Anchor::Start,
            format!("Pick {}", n + 1),
        ),
    }
    slot_frame(s, x, y, COL_W, PICK_H, 6.0, value.is_some(), last);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::scene::Tone;
    use crate::export::{icons, raster};
    use crate::tree::DraftTree;
    use std::collections::HashMap;

    #[test]
    fn cards_are_portrait() {
        const { assert!(CARD_H > CARD_W) };
        let mut t = DraftTree::new();
        let mut at = t.root();
        for v in [16, 1, 17, 18, 3, 1, 40, 41, 42] {
            at = t.add_child(at, v).unwrap();
        }
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let node = snap.at_depth(9)[0];
        let scene = card(&snap, node, 1, 1);
        assert_eq!(
            scene.champions().filter(|&(_, t)| t == Tone::Gray).count(),
            3
        );
        // Full portraits: the three picks, plus blue's only pick in each of its five role
        // slots. With nothing tagged every pick can play every role, so red's two picks
        // share each of its slots.
        let full = scene
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    crate::export::scene::Item::Icon {
                        tone: Tone::Color,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(full, 3 + 5);
        let bands = scene
            .items
            .iter()
            .filter(|i| matches!(i, crate::export::scene::Item::IconBand { .. }))
            .count();
        assert_eq!(bands, 2 * 5);
        let pm = raster::render(&scene, 900.0 / CARD_W, icons::for_tests()).unwrap();
        assert_eq!(pm.width(), 900);
    }
}
