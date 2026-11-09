//! XML export of the tree: a `<legend>` describing the groups used, then the tree as
//! nested elements named after their draft position (`<bb1>`, `<rb1>`, ... `<rp5>`).
//!
//! ```xml
//! <recvrve version="1" exported="..." scope="whole">
//!   <legend>
//!     <group value="190" label="G1" name="Enchanters" note="...">
//!       <champion value="57" name="Janna"/>
//!     </group>
//!   </legend>
//!   <tree nodes="3" lines="1">
//!     <bb1 value="129" name="Aatrox">
//!       <rb1 value="1" name="No ban"/>
//!     </bb1>
//!   </tree>
//! </recvrve>
//! ```
//!
//! With fearless bans, the legend also has
//! `<fearless game="3" mode="hard" champions="10"><game n="1"><champion .../></game></fearless>`;
//! soft fearless wraps each game's champions in `<blue>` and `<red>`. Nodes that the bans
//! make unreachable carry `unreachable="true"`. When conflicting branches are included,
//! the node where a line first breaks carries `conflict="true"` and the nodes below it
//! `conflict="below"`. Nodes on a marked line (Ctrl+M: the marked node and the nodes
//! leading to it) carry `marked="true"`.
//!
//! Values are the app's own node values: 0 empty, 1 no ban, 2-15 preset groups, 16-189
//! champions, 190-255 user groups.

use std::fmt::Write;

use crate::champions;
use crate::fearless::{Fearless, HALF, Mode};

use super::draw::value_label;
use super::model::{Snapshot, pos_code};
use super::svg::escape;

pub const FORMAT_VERSION: u32 = 1;

fn write_champion(out: &mut String, indent: &str, value: u8) {
    if let Some(c) = champions::by_value(value) {
        let _ = writeln!(
            out,
            "{indent}<champion value=\"{value}\" name=\"{}\"/>",
            escape(c.name)
        );
    }
}

/// `<fearless>`: one `<game>` per earlier game. Hard fearless lists its champions directly;
/// soft fearless splits them into `<blue>` and `<red>`, by the side the team that picked
/// them plays on in the current game.
fn write_fearless(out: &mut String, f: &Fearless) {
    let mode = f.mode.label().to_lowercase();
    let _ = writeln!(
        out,
        "    <fearless game=\"{}\" mode=\"{mode}\" champions=\"{}\">",
        f.game,
        f.count()
    );
    for (g, row) in f.active_rows().iter().enumerate() {
        let _ = writeln!(out, "      <game n=\"{}\">", g + 1);
        match f.mode {
            Mode::Hard => {
                for &v in row {
                    write_champion(out, "        ", v);
                }
            }
            Mode::Soft => {
                for (tag, half) in [("blue", &row[..HALF]), ("red", &row[HALF..])] {
                    let _ = writeln!(out, "        <{tag}>");
                    for &v in half {
                        write_champion(out, "          ", v);
                    }
                    let _ = writeln!(out, "        </{tag}>");
                }
            }
        }
        out.push_str("      </game>\n");
    }
    out.push_str("    </fearless>\n");
}

pub fn render(snap: &Snapshot, exported: &str) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let scope = if snap.scope_root == 0 {
        "whole"
    } else {
        "subtree"
    };
    let _ = writeln!(
        out,
        "<recvrve version=\"{FORMAT_VERSION}\" exported=\"{}\" scope=\"{scope}\">",
        escape(exported)
    );

    let groups = snap.used_groups();
    let fearless = &snap.fearless;
    if groups.is_empty() && !fearless.is_active() {
        out.push_str("  <legend/>\n");
    } else {
        out.push_str("  <legend>\n");
        for g in groups {
            let _ = writeln!(
                out,
                "    <group value=\"{}\" label=\"{}\" name=\"{}\" note=\"{}\">",
                g.value,
                escape(&g.label),
                escape(&g.name),
                escape(&g.note)
            );
            for &m in &g.members {
                if let Some(c) = champions::by_value(m) {
                    let _ = writeln!(
                        out,
                        "      <champion value=\"{m}\" name=\"{}\"/>",
                        escape(c.name)
                    );
                }
            }
            out.push_str("    </group>\n");
        }
        if fearless.is_active() {
            write_fearless(&mut out, fearless);
        }
        out.push_str("  </legend>\n");
    }

    let _ = write!(
        out,
        "  <tree nodes=\"{}\" lines=\"{}\"",
        snap.nodes.len() - 1,
        snap.lines()
    );
    let root = &snap.nodes[0];
    if root.children.is_empty() {
        out.push_str("/>\n");
    } else {
        out.push_str(">\n");
        // Iterative so deep trees can't overflow the stack: (node, closing?).
        let mut stack: Vec<(usize, bool)> =
            root.children.iter().rev().map(|&c| (c, false)).collect();
        while let Some((i, closing)) = stack.pop() {
            let node = &snap.nodes[i];
            let indent = "  ".repeat(node.depth + 1);
            let tag = pos_code(node.pos).to_lowercase();
            if closing {
                let _ = writeln!(out, "{indent}</{tag}>");
                continue;
            }
            let _ = write!(
                out,
                "{indent}<{tag} value=\"{}\" name=\"{}\"",
                node.value,
                escape(&value_label(snap, node.value))
            );
            if i == snap.scope_root && i != 0 {
                out.push_str(" scope-root=\"true\"");
            }
            if node.pruned.is_some() {
                out.push_str(" unreachable=\"true\"");
            }
            match node.conflict {
                Some(true) => out.push_str(" conflict=\"true\""),
                Some(false) => out.push_str(" conflict=\"below\""),
                None => {}
            }
            if node.starred {
                out.push_str(" marked=\"true\"");
            }
            if node.roles != 0 {
                let _ = write!(out, " roles=\"{}\"", crate::roles::keys(node.roles));
            }
            if node.children.is_empty() {
                out.push_str("/>\n");
            } else {
                out.push_str(">\n");
                stack.push((i, true));
                stack.extend(node.children.iter().rev().map(|&c| (c, false)));
            }
        }
        out.push_str("  </tree>\n");
    }
    out.push_str("</recvrve>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::DraftTree;
    use std::collections::HashMap;

    #[test]
    fn nests_nodes_by_position() {
        let mut t = DraftTree::new();
        let root = t.root();
        let a = t.add_child(root, 16).unwrap();
        t.add_child(a, 1).unwrap();
        t.add_child(a, 14).unwrap();
        t.add_child(root, 17).unwrap();
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let xml = render(&snap, "2026-10-01");
        let expected = r#"  <tree nodes="4" lines="3">
    <bb1 value="16" name="Alistar">
      <rb1 value="1" name="No ban"/>
      <rb1 value="14" name="Placeholder 14"/>
    </bb1>
    <bb1 value="17" name="Annie"/>
  </tree>"#;
        assert!(xml.contains(expected), "{xml}");
        assert!(xml.contains(r#"<group value="14""#));
        assert!(xml.contains(r#"<champion value="16" name="Alistar"/>"#));
        assert!(!xml.contains("<fearless"));
    }

    #[test]
    fn writes_fearless_bans() {
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        t.add_child(a, 17).unwrap();
        let mut f = Fearless {
            game: 2,
            ..Fearless::default()
        };
        f.rows[0][0] = 16;
        f.rows[0][7] = 18;
        t.set_fearless(f);
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let xml = render(&snap, "now");
        assert!(
            xml.contains(r#"<fearless game="2" mode="hard" champions="2">"#),
            "{xml}"
        );
        assert!(xml.contains(r#"<bb1 value="16" name="Alistar" unreachable="true">"#));
        assert!(!xml.contains("<blue>"));

        f.mode = Mode::Soft;
        t.set_fearless(f);
        let xml = render(
            &Snapshot::build(&t, None, &HashMap::new(), &[], false, true),
            "now",
        );
        assert!(
            xml.contains("<blue>\n          <champion value=\"16\""),
            "{xml}"
        );
        assert!(xml.contains("<red>\n          <champion value=\"18\""));
        assert!(
            !xml.contains("unreachable"),
            "soft fearless doesn't lock bans"
        );

        // Hidden unreachable branches are left out of the export.
        f.mode = Mode::Hard;
        t.set_fearless(f);
        let xml = render(
            &Snapshot::build(&t, None, &HashMap::new(), &[], true, true),
            "now",
        );
        assert!(xml.contains(r#"<tree nodes="0" lines="1"/>"#), "{xml}");
    }

    #[test]
    fn conflicting_branches_are_left_out_unless_shown() {
        let mut t = DraftTree::new();
        let e = t.add_child(t.root(), crate::tree::NO_BAN).unwrap();
        let a = t.add_child(e, 17).unwrap();
        t.add_child(a, 18).unwrap();
        t.add_child(e, 19).unwrap();
        t.set_value(e, 17).unwrap();
        let hidden = Snapshot::build(&t, None, &HashMap::new(), &[], false, false);
        assert_eq!(hidden.nodes.len(), 3, "root, e and 19");
        let xml = render(&hidden, "now");
        assert!(!xml.contains("conflict"), "{xml}");
        // A scope inside a left-out branch falls back to the nearest kept ancestor.
        let scoped = Snapshot::build(&t, Some(a), &HashMap::new(), &[], false, false);
        assert_eq!(scoped.nodes.len(), 3);

        let shown = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        assert_eq!(shown.nodes.len(), 5);
        let xml = render(&shown, "now");
        assert!(
            xml.contains(r#"<rb1 value="17" name="Annie" conflict="true">"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<bb2 value="18" name="Ashe" conflict="below"/>"#),
            "{xml}"
        );
        assert!(xml.contains(r#"<rb1 value="19" name=""#), "{xml}");
    }

    #[test]
    fn marked_lines_carry_an_attribute() {
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        let a1 = t.add_child(a, 17).unwrap();
        t.add_child(t.root(), 18).unwrap();
        t.set_marked(a1, true);
        let s = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let starred: Vec<bool> = s.nodes.iter().map(|n| n.starred).collect();
        assert_eq!(starred, [true, true, true, false]);
        let xml = render(&s, "now");
        assert_eq!(xml.matches(r#" marked="true""#).count(), 2, "{xml}");
        assert!(!xml.contains(r#"value="18" name="Ashe" marked"#), "{xml}");
    }
}
