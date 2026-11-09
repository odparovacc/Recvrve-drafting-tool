//! Mouse gestures on the graph: box selection and drag and drop.
//!
//! A press only becomes a drag once the pointer has moved a few pixels, so clicks,
//! Shift+clicks, Ctrl+clicks, middle and right clicks stay what they were.
//!
//! - From empty graph space: a selection box, reported in graph pixels so the nodes it
//!   touches are found from the layout ([`Gesture::Box`]); Ctrl adds them to the selection.
//! - From a graph node: dropping it on another node copies its branch there, or moves it
//!   with Shift held at the drop ([`Gesture::DropNodes`]).
//! - From a champion, group or Empty tile: dropping it on a node gives that node the value
//!   ([`Gesture::DropValue`]). Not with Shift held: that is a Shift+click batch.
//!
//! - From a tab: dropping it on another tab moves it there ([`Gesture::MoveTab`]).
//!
//! While dragging, the graph scrolls near its edges, the node under the pointer shows
//! whether it takes the drop (by depth; the app checks the rest), and Esc cancels. In
//! follow mode the graph and the tiles can't be dragged. The mouse wheel over the tabs (or
//! the draft tree's header buttons) scrolls them sideways.

use dioxus::document::{self, Eval};
use slotmap::{Key, KeyData};

use crate::tree::NodeId;

/// What a gesture asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// A selection box, in graph pixels (x0, y0, x1, y1); `add` keeps the current
    /// selection. The app finds the nodes inside it.
    Box {
        rect: (f64, f64, f64, f64),
        add: bool,
    },
    /// A node dragged onto `target`: copy its branch there, or move it.
    DropNodes {
        source: NodeId,
        target: NodeId,
        move_them: bool,
    },
    /// A tile's value dropped on `target`.
    DropValue { value: u8, target: NodeId },
    /// The tab `from` dropped on the tab `to`: it goes before it, or after it.
    MoveTab { from: u64, to: u64, after: bool },
}

/// The `data-node` attribute value for a node.
pub fn node_attr(id: NodeId) -> String {
    id.data().as_ffi().to_string()
}

fn node_from(text: &str) -> Option<NodeId> {
    text.parse::<u64>()
        .ok()
        .map(|n| NodeId::from(KeyData::from_ffi(n)))
}

/// Reads one message from the script.
pub fn parse(message: &str) -> Option<Gesture> {
    let (kind, rest) = message.split_once(':')?;
    match kind {
        "box" => {
            let (add, rect) = rest.split_once(':')?;
            let v: Vec<f64> = rect
                .split(',')
                .map(|s| s.parse::<f64>().ok().filter(|v| v.is_finite()))
                .collect::<Option<_>>()?;
            let [x0, y0, x1, y1] = v[..] else {
                return None;
            };
            Some(Gesture::Box {
                rect: (x0, y0, x1, y1),
                add: add == "1",
            })
        }
        "nodes" => {
            let mut parts = rest.split(':');
            let source = node_from(parts.next()?)?;
            let target = node_from(parts.next()?)?;
            let move_them = parts.next()? == "1";
            Some(Gesture::DropNodes {
                source,
                target,
                move_them,
            })
        }
        "tab" => {
            let mut parts = rest.split(':');
            let from = parts.next()?.parse().ok()?;
            let to = parts.next()?.parse().ok()?;
            let after = parts.next()? == "1";
            Some(Gesture::MoveTab { from, to, after })
        }
        "value" => {
            let (value, target) = rest.split_once(':')?;
            Some(Gesture::DropValue {
                value: value.parse().ok()?,
                target: node_from(target)?,
            })
        }
        _ => None,
    }
}

const SCRIPT: &str = r#"
(() => {
const THRESHOLD = 5;
let drag = null;
let ghost = null, box = null, hover = null, lastX = 0, lastY = 0;
const graph = () => document.querySelector('.center .tree-scroll');
const host = () => document.getElementById('app-root') || document.body;
const tabAt = (x, y) => {
    const el = document.elementFromPoint(x, y);
    return el && el.closest ? el.closest('[data-tab]') : null;
};
const tabAfter = (el, x) => {
    const r = el.getBoundingClientRect();
    return x > r.left + r.width / 2;
};
const nodeAt = (x, y) => {
    const el = document.elementFromPoint(x, y);
    return el && el.closest ? el.closest('.center .tree-node') : null;
};
const setHover = (el, ok) => {
    if (hover && hover !== el) hover.classList.remove('drop-ok', 'drop-bad', 'drop-before', 'drop-after');
    hover = el;
    if (el) {
        el.classList.toggle('drop-ok', ok);
        el.classList.toggle('drop-bad', !ok);
    }
};
const cleanup = () => {
    if (ghost) ghost.remove();
    if (box) box.remove();
    setHover(null);
    ghost = box = null;
    drag = null;
    document.body.classList.remove('dragging-graph', 'dragging-tab');
};
// After a drag, the click that ends it must not also act.
const swallowClick = () => {
    const stop = (e) => { e.stopPropagation(); e.preventDefault(); };
    window.addEventListener('click', stop, true);
    setTimeout(() => window.removeEventListener('click', stop, true), 0);
};
document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || drag) return;
    const t = e.target;
    if (!t || !t.closest) return;
    const tab = t.closest('[data-tab]');
    if (tab) {
        if (t.closest('.tab-close')) return;
        drag = { kind: 'tab', id: tab.dataset.tab, el: tab };
        drag.x0 = e.clientX;
        drag.y0 = e.clientY;
        drag.started = false;
        return;
    }
    // Follow mode: the graph and the tiles stay put.
    if (document.querySelector('.app.following')) return;
    const node = t.closest('.center .tree-node');
    const tile = t.closest('[data-drag-value]');
    const sc = t.closest('.center .tree-scroll');
    if (node) {
        drag = { kind: 'node', id: node.dataset.node, depth: +node.dataset.depth, el: node };
    } else if (tile) {
        // Shift+click on a tile is a batch of branches; dragging with Shift does nothing.
        if (e.shiftKey || tile.disabled) return;
        drag = { kind: 'value', value: tile.dataset.dragValue, el: tile };
    } else if (sc && !t.closest('.tree-steps')) {
        drag = { kind: 'box', add: e.ctrlKey || e.metaKey };
        e.preventDefault();
    } else {
        return;
    }
    drag.x0 = e.clientX;
    drag.y0 = e.clientY;
    drag.started = false;
}, true);
const start = () => {
    drag.started = true;
    if (drag.kind === 'tab') {
        document.body.classList.add('dragging-tab');
        return;
    }
    document.body.classList.add('dragging-graph');
    if (drag.kind === 'box') {
        box = document.createElement('div');
        box.className = 'select-box';
        // Inside the app root, where the theme's colors are defined.
        host().appendChild(box);
    } else {
        ghost = document.createElement('div');
        ghost.className = 'drag-ghost';
        const icon = drag.el.querySelector('img, .group-icon, .wildcard-icon');
        if (icon) ghost.appendChild(icon.cloneNode(true));
        const label = document.createElement('span');
        ghost.appendChild(label);
        host().appendChild(ghost);
    }
    requestAnimationFrame(autoScroll);
};
const update = (e) => {
    lastX = e.clientX;
    lastY = e.clientY;
    if (drag.kind === 'tab') {
        const target = tabAt(lastX, lastY);
        if (target && target !== drag.el) {
            setHover(target, true);
            target.classList.remove('drop-ok');
            const after = tabAfter(target, lastX);
            target.classList.toggle('drop-after', after);
            target.classList.toggle('drop-before', !after);
        } else {
            setHover(null);
        }
        return;
    }
    if (drag.kind === 'box') {
        const x = Math.min(drag.x0, lastX), y = Math.min(drag.y0, lastY);
        Object.assign(box.style, { left: x + 'px', top: y + 'px',
            width: Math.abs(lastX - drag.x0) + 'px', height: Math.abs(lastY - drag.y0) + 'px' });
        return;
    }
    ghost.style.left = lastX + 14 + 'px';
    ghost.style.top = lastY + 14 + 'px';
    const target = nodeAt(lastX, lastY);
    const label = ghost.querySelector('span');
    if (drag.kind === 'node') {
        label.textContent = e.shiftKey ? 'Move here' : 'Copy here';
        const ok = target && target !== drag.el && +target.dataset.depth === drag.depth - 1;
        setHover(target, !!ok);
    } else {
        label.textContent = 'Set value';
        setHover(target, !!target && +target.dataset.depth > 0);
    }
};
// Scrolls the graph while the pointer is near its edges.
const autoScroll = () => {
    if (!drag || !drag.started) return;
    const sc = graph();
    if (sc) {
        const r = sc.getBoundingClientRect(), edge = 20, step = 12;
        if (lastX < r.left + edge && lastX > r.left - edge) sc.scrollLeft -= step;
        if (lastX > r.right - edge && lastX < r.right + edge) sc.scrollLeft += step;
        if (lastY < r.top + edge && lastY > r.top - edge) sc.scrollTop -= step;
        if (lastY > r.bottom - edge && lastY < r.bottom + edge) sc.scrollTop += step;
    }
    requestAnimationFrame(autoScroll);
};
document.addEventListener('mousemove', (e) => {
    if (!drag) return;
    if (!drag.started) {
        if (Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) < THRESHOLD) return;
        start();
    }
    update(e);
}, true);
document.addEventListener('mouseup', (e) => {
    if (!drag) return;
    if (!drag.started) { drag = null; return; }
    swallowClick();
    if (drag.kind === 'tab') {
        const target = tabAt(e.clientX, e.clientY);
        if (target && target !== drag.el) {
            dioxus.send('tab:' + drag.id + ':' + target.dataset.tab + ':' + (tabAfter(target, e.clientX) ? '1' : '0'));
        }
        cleanup();
        return;
    }
    if (drag.kind === 'box') {
        // The box in graph pixels: the app finds the nodes in it from the layout, since
        // only part of a large graph is on the page.
        const r = box.getBoundingClientRect();
        const canvas = document.querySelector('.center .tree-canvas');
        if (canvas) {
            const c = canvas.getBoundingClientRect();
            const k = canvas.offsetWidth > 0 ? c.width / canvas.offsetWidth : 1;
            const rect = [r.left - c.left, r.top - c.top, r.right - c.left, r.bottom - c.top]
                .map((v) => v / k);
            dioxus.send('box:' + (drag.add ? '1' : '0') + ':' + rect.join(','));
        }
    } else {
        const target = nodeAt(e.clientX, e.clientY);
        if (target && drag.kind === 'node' && target !== drag.el) {
            dioxus.send('nodes:' + drag.id + ':' + target.dataset.node + ':' + (e.shiftKey ? '1' : '0'));
        } else if (target && drag.kind === 'value') {
            dioxus.send('value:' + drag.value + ':' + target.dataset.node);
        }
    }
    cleanup();
}, true);
// The mouse wheel over the tabs (or the draft tree header buttons) scrolls them sideways.
document.addEventListener('wheel', (e) => {
    const strip = e.target && e.target.closest ? e.target.closest('.tab-scroll, .tree-panel > .groups-header > .groups-actions') : null;
    if (!strip || e.ctrlKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
    strip.scrollLeft += e.deltaY;
    e.preventDefault();
}, { passive: false });
document.addEventListener('keydown', (e) => {
    if (drag && drag.started && e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        cleanup();
    }
}, true);
})();
"#;

/// Installs the script. Call once and keep the returned handle.
pub fn listen() -> Eval {
    document::eval(SCRIPT)
}

/// Waits for the next gesture. `None` once the script is gone.
pub async fn next(listener: &mut Eval) -> Option<Gesture> {
    loop {
        let message = listener.recv::<String>().await.ok()?;
        if let Some(g) = parse(&message) {
            return Some(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::DraftTree;

    #[test]
    fn messages_round_trip_node_ids() {
        let mut t = DraftTree::new();
        let a = t.add_child(t.root(), 16).unwrap();
        let b = t.add_child(t.root(), 17).unwrap();
        let (sa, sb) = (node_attr(a), node_attr(b));
        assert_eq!(
            parse("box:1:10,20.5,-3,40"),
            Some(Gesture::Box {
                rect: (10.0, 20.5, -3.0, 40.0),
                add: true
            })
        );
        // A box needs all four corners, as numbers.
        assert_eq!(parse("box:0:"), None);
        assert_eq!(parse("box:0:1,2,3"), None);
        assert_eq!(parse("box:0:1,2,3,NaN"), None);
        assert_eq!(
            parse(&format!("nodes:{sa}:{sb}:1")),
            Some(Gesture::DropNodes {
                source: a,
                target: b,
                move_them: true
            })
        );
        assert_eq!(
            parse(&format!("value:42:{sb}")),
            Some(Gesture::DropValue {
                value: 42,
                target: b
            })
        );
        assert_eq!(
            parse("tab:3:7:1"),
            Some(Gesture::MoveTab {
                from: 3,
                to: 7,
                after: true
            })
        );
        for bad in ["", "box", "nodes:1:2", "value:x:1", "other:1", "tab:1:x:0"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
