//! Resizable panels, and fitting the graph to its panel.
//!
//! Splitters are elements with a `data-split` attribute (`left`, `right`, `pool`,
//! `groups`). The two side splitters move both side panels together, as a mirrored pair.
//! In fullscreen the `pool` splitter sits between the champion pool and the groups column
//! (it sets the column's width) and the `groups` splitter between that row and the graph
//! (it sets the row's height). A hidden panel's splitters aren't shown; with the groups
//! hidden, the `pool` splitter trades height between the pool and the graph.
//!
//! With the graph top to bottom (the `vertical` class on the app root) the middle column is
//! arranged like fullscreen, and the splitters set their own sizes (`v_side`, `v_top`,
//! `v_groups`), so each orientation keeps its layout.
//!
//! Until the user drags them (`v_auto`, `h_auto`), the panels above the graph size
//! themselves to whole rows of tiles: up to four rows of champions (fewer when the window
//! is too low to leave the graph area [`GRAPH_MIN`], or [`GRAPH_SHARE`] of the middle column
//! on taller windows, below the draft tree's header), and left to right one row of groups, or
//! two when there is room after the champions. Outside fullscreen both tile grids always show whole rows only.
//!
//! The same script scales the left-to-right graph down (CSS `zoom`) whenever it is wider
//! than its panel, so all draft steps are visible without scrolling sideways. It also
//! reports the graph area's size, which the graph centres its root in.
//! While one is dragged, a script in the webview updates the CSS size variables
//! on the app root directly, so resizing stays smooth. On release it sends the final
//! [`Layout`] to Rust, which stores it in the UI settings and saves it. Double-clicking a
//! splitter resets the sizes it controls.

use dioxus::document::{self, Eval};

use crate::configio::Layout;

/// Smallest heights, in pixels, the middle column's panels can be dragged to.
const POOL_MIN: f32 = 140.0;
const GROUPS_MIN: f32 = 70.0;
const TREE_MIN: f32 = 110.0;
/// Height the automatic panel sizes leave the graph area at least (below the draft tree
/// panel's header, which is measured), and the share of the middle column's height it gets
/// on taller windows.
const GRAPH_MIN: f32 = 200.0;
const GRAPH_SHARE: f32 = 0.3;

fn script() -> String {
    format!(
        r#"
(() => {{
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
document.addEventListener('mousedown', (e) => {{
    const sp = e.target.closest && e.target.closest('[data-split]');
    if (!sp || e.button !== 0) return;
    e.preventDefault();
    const kind = sp.dataset.split;
    const root = document.getElementById('app-root');
    const center = document.querySelector('.center');
    const pool = document.querySelector('.center > .pool');
    const groups = document.querySelector('.center > .groups-panel');
    const tree = document.querySelector('.center > .tree-panel');
    if (!root || !center) return;
    const full = root.classList.contains('fullscreen');
    const vert = !full && root.classList.contains('vertical');
    const cs = getComputedStyle(root);
    const num = (n) => parseFloat(cs.getPropertyValue(n));
    const side = num('--side-w');
    const layout = {{ side: side, v_side: side,
        pool: num('--pool-frac'), groups: num('--groups-frac'),
        fs_top: num('--fs-top'), fs_groups: num('--fs-groups-w'),
        v_top: num('--v-top'), v_groups: num('--v-groups-w'),
        v_auto: root.classList.contains('v-auto'),
        h_auto: root.classList.contains('h-auto') }};
    // Left to right: from the first drag on, the dragged sizes count.
    const stopAuto = () => {{
        if (!layout.h_auto) return;
        layout.h_auto = false;
        root.classList.remove('h-auto');
    }};
    // Only the active orientation's side width and automatic-size flag are sent back.
    // (Fullscreen has no side panels; its flag is the graph direction's.)
    if (root.classList.contains('vertical')) {{ delete layout.side; delete layout.h_auto; }}
    else {{ delete layout.v_side; delete layout.v_auto; }}
    const sideKey = root.classList.contains('vertical') ? 'v_side' : 'side';
    const start = Object.assign({{}}, layout);
    const height = (el) => el ? el.getBoundingClientRect().height : 0;
    const h = center.getBoundingClientRect().height;
    const poolH = height(pool), groupsH = height(groups), treeH = height(tree);
    const x0 = e.clientX, y0 = e.clientY;
    const grid = full || vert;
    const topKey = full ? 'fs_top' : 'v_top', colKey = full ? 'fs_groups' : 'v_groups';
    const axis = (kind === 'left' || kind === 'right' || (grid && kind === 'pool')) ? 'x' : 'y';
    document.body.classList.add('resizing-' + axis);
    sp.classList.add('dragging');
    const apply = () => {{
        root.style.setProperty('--side-w', layout[sideKey] + 'px');
        root.style.setProperty('--pool-frac', layout.pool);
        root.style.setProperty('--groups-frac', layout.groups);
        root.style.setProperty('--fs-top', layout.fs_top);
        root.style.setProperty('--fs-groups-w', layout.fs_groups + 'px');
        root.style.setProperty('--v-top', layout.v_top);
        root.style.setProperty('--v-groups-w', layout.v_groups + 'px');
    }};
    const move = (ev) => {{
        const dx = ev.clientX - x0, dy = ev.clientY - y0;
        if (kind === 'left' || kind === 'right') {{
            const d = kind === 'left' ? dx : -dx;
            layout[sideKey] = Math.round(clamp(start[sideKey] + d, {side_min}, {side_max}));
        }} else if (grid && kind === 'pool') {{
            // The groups column's width.
            layout[colKey] = Math.round(clamp(start[colKey] - dx, {side_min}, {side_max}));
        }} else if (grid && kind === 'groups') {{
            // The height of the pool and groups row.
            const topH = Math.max(poolH, groupsH);
            layout[topKey] = clamp((topH + dy) / h, 0.15, 0.8);
            if (vert && layout.v_auto) {{
                // From now on the dragged size counts.
                layout.v_auto = false;
                root.classList.remove('v-auto');
            }}
        }} else if (kind === 'pool' && !groups && tree) {{
            stopAuto();
            // Groups hidden: the pool and the graph trade height.
            const d = clamp(dy, {pool_min} - poolH, treeH - {tree_min});
            layout.pool = (poolH + d) / h;
        }} else if (!pool || !groups || !tree) {{
            return;
        }} else if (kind === 'pool') {{
            stopAuto();
            // Champion pool and groups area trade height.
            const d = clamp(dy, {pool_min} - poolH, groupsH - {groups_min});
            layout.pool = (poolH + d) / h;
            layout.groups = (groupsH - d) / h;
        }} else if (kind === 'groups') {{
            stopAuto();
            // Groups area and graph trade height.
            const d = clamp(dy, {groups_min} - groupsH, treeH - {tree_min});
            layout.groups = (groupsH + d) / h;
        }}
        apply();
    }};
    const up = () => {{
        document.removeEventListener('mousemove', move);
        document.removeEventListener('mouseup', up);
        document.body.classList.remove('resizing-' + axis);
        sp.classList.remove('dragging');
        dioxus.send('set:' + JSON.stringify(layout));
    }};
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
}});
document.addEventListener('dblclick', (e) => {{
    const sp = e.target.closest && e.target.closest('[data-split]');
    if (!sp) return;
    const root = document.getElementById('app-root');
    const full = root && root.classList.contains('fullscreen');
    const vert = root && root.classList.contains('vertical');
    dioxus.send('reset:' + (full ? 'fs-' : vert ? 'v-' : '') + sp.dataset.split);
}});

// Top to bottom: until the user drags it, the champions and groups row is as tall as
// four whole rows of champion tiles, or fewer whole rows when the window is too low to
// leave the graph area {graph_min}px (or {graph_share} of the column). Both tile grids are cut to whole rows, so no tile shows
// half.
const setStyle = (el, prop, value) => {{
    if (el && el.style.getPropertyValue(prop) !== value) {{
        if (value) el.style.setProperty(prop, value); else el.style.removeProperty(prop);
    }}
}};
const tilePitch = (grid) => {{
    const first = grid && grid.firstElementChild;
    if (!first) return 0;
    const gap = parseFloat(getComputedStyle(grid).rowGap) || 0;
    return first.getBoundingClientRect().height + gap;
}};
const padY = (el) => {{
    const cs = getComputedStyle(el);
    return (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0);
}};
// Height of everything in `panel` but `grid`'s rows: its padding, the other children in
// the flow and the gaps between them (rows and columns of a grid panel alike).
const chromeOf = (panel, grid) => {{
    const flowing = [...panel.children].filter((el) => {{
        const pos = getComputedStyle(el).position;
        return pos !== 'absolute' && pos !== 'fixed' && el.getClientRects().length > 0;
    }});
    if (getComputedStyle(panel).display === 'grid') {{
        // A grid panel: the grid's own track is what's left.
        return panel.getBoundingClientRect().height - grid.getBoundingClientRect().height + padY(grid);
    }}
    const others = flowing.filter((el) => el !== grid)
        .reduce((sum, el) => sum + el.getBoundingClientRect().height, 0);
    const gap = parseFloat(getComputedStyle(panel).rowGap) || 0;
    return padY(panel) + others + gap * (flowing.length - 1) + padY(grid);
}};
const rowGap = (grid) => parseFloat(getComputedStyle(grid).rowGap) || 0;
// Height for `rows` whole rows of `grid` in `panel`.
const heightFor = (panel, grid, rows) =>
    Math.ceil(chromeOf(panel, grid) + rows * tilePitch(grid) - rowGap(grid));
// Rows of tiles `grid` holds (all of them).
const rowsHeld = (grid) => {{
    const cols = getComputedStyle(grid).gridTemplateColumns.split(' ').filter((t) => t).length || 1;
    return Math.ceil(grid.children.length / cols);
}};
// The size variables the automatic layout sets on the app root (empty: removed). Only
// reads the page; `fit` writes the result.
const autoSizes = (root, center, pool, grid, groups, groupGrid, tree) => {{
    const sizes = {{ '--v-top-px': '', '--h-pool-px': '', '--h-groups-px': '',
        '--h-pool-min': '', '--h-groups-min': '' }};
    const full = !root || root.classList.contains('fullscreen');
    if (full || !center) return sizes;
    const vert = root.classList.contains('vertical');
    // Height the panels above leave the draft tree panel: the graph area it should get, plus
    // the panel's header and padding (the graph area is what the header leaves). Without
    // the measured header the graph got about 150 of the 200 px meant for it.
    const centerH = center.getBoundingClientRect().height;
    const scroll = tree && tree.querySelector('.tree-scroll');
    const treeChrome = scroll
        ? Math.max(0, tree.getBoundingClientRect().height - scroll.getBoundingClientRect().height)
        : 0;
    const treeWant = Math.max({graph_min}, {graph_share} * centerH) + treeChrome;
    const champRows = (room) => Math.max(1, Math.min(4,
        Math.floor((room - chromeOf(pool, grid) + rowGap(grid)) / tilePitch(grid))));
    if (vert && root.classList.contains('v-auto') && pool && grid && tree && tilePitch(grid)) {{
        // Top to bottom: the champions and groups row, with the graph below.
        const room = centerH - 16 - treeWant;
        sizes['--v-top-px'] = heightFor(pool, grid, champRows(room)) + 'px';
    }}
    if (!vert && root.classList.contains('h-auto') && tree && (pool || groups)) {{
        // Left to right: champions, groups and graph stacked. The groups get one whole
        // row, the champions up to four, then the groups a second row if they fill one
        // and there is room left; the graph gets the rest.
        const fixed = [...center.children]
            .filter((el) => el !== pool && el !== groups && el !== tree)
            .reduce((sum, el) => sum + el.getBoundingClientRect().height, 0);
        let room = centerH - fixed - treeWant;
        const hasGroups = groups && groupGrid && tilePitch(groupGrid);
        const groupsOne = hasGroups ? heightFor(groups, groupGrid, 1) : 0;
        room -= groupsOne;
        if (pool && grid && tilePitch(grid)) {{
            const h = heightFor(pool, grid, champRows(room));
            sizes['--h-pool-px'] = h + 'px';
            // On a short window the column scrolls rather than cutting tiles in slivers.
            sizes['--h-pool-min'] = heightFor(pool, grid, 1) + 'px';
            room -= h;
        }}
        if (hasGroups) {{
            const groupsTwo = heightFor(groups, groupGrid, 2);
            const two = rowsHeld(groupGrid) >= 2 && room >= groupsTwo - groupsOne;
            sizes['--h-groups-px'] = (two ? groupsTwo : groupsOne) + 'px';
            sizes['--h-groups-min'] = groupsOne + 'px';
        }}
    }}
    return sizes;
}};
// The max-height that cuts `grid` to the whole rows fitting the space it has (its row in
// the panel), or null to leave it.
const rowsCut = (grid) => {{
    const pitch = tilePitch(grid);
    if (!pitch) return null;
    const space = grid.getBoundingClientRect().height;
    const gap = rowGap(grid);
    const rows = Math.max(1, Math.floor((space - padY(grid) + gap) / pitch));
    return Math.floor(rows * pitch - gap + padY(grid)) + 'px';
}};

// Lays the panels out (see `autoSizes`), scales each left-to-right graph down to fit its
// scroll area's width (never up), and reports the size the graph centres its root in.
// Reads and writes come in rounds, since each read after a write makes the webview lay the
// page out again.
let lastSize = '';
const fit = () => {{
    const root = document.getElementById('app-root');
    const center = document.querySelector('.center');
    const pool = document.querySelector('.center > .pool');
    const grid = pool && pool.querySelector('.grid');
    const groups = document.querySelector('.center > .groups-panel');
    const groupGrid = groups && groups.querySelector('.group-grid');
    const tree = document.querySelector('.center > .tree-panel');
    // Measure without the whole-row cut from last time.
    setStyle(grid, 'max-height', '');
    setStyle(groupGrid, 'max-height', '');
    const sizes = autoSizes(root, center, pool, grid, groups, groupGrid, tree);
    if (root) for (const [v, value] of Object.entries(sizes)) setStyle(root, v, value);
    // Whole rows only, in both grids (in fullscreen too). The cut doesn't change the
    // panels' sizes, so the graph is measured in the same round.
    const cuts = [grid, groupGrid].filter((g) => g).map((g) => [g, rowsCut(g)]);
    // Where the top bar ends (it wraps to two rows on narrow windows; hidden in
    // fullscreen), so panels floating below it don't cover it.
    const bar = document.querySelector('.topbar');
    const bottom = bar && bar.getClientRects().length ? Math.ceil(bar.getBoundingClientRect().bottom) : 0;
    const main = document.querySelector('.center .tree-scroll');
    const size = main && main.clientWidth > 0 ? main.clientWidth + ',' + main.clientHeight : null;
    const fits = [...document.querySelectorAll('.tree-scroll')].map((sc) => {{
        const canvas = sc.querySelector('.tree-canvas');
        const w = canvas ? parseFloat(canvas.dataset.w) || 0 : 0;
        const f = w > 0 ? Math.min(1, Math.max(0.2, (sc.clientWidth - 2) / w)) : 1;
        return [sc, f.toFixed(4)];
    }});
    cuts.forEach(([g, cut]) => {{ if (cut !== null) setStyle(g, 'max-height', cut); }});
    if (root) setStyle(root, '--topbar-bottom', bottom + 'px');
    fits.forEach(([sc, value]) => setStyle(sc, '--tree-fit', value));
    if (size !== null && size !== lastSize) {{
        lastSize = size;
        dioxus.send('tree-size:' + size);
    }}
    // The app root's style changes above were this function's own.
    rootObserver.takeRecords();
}};
// Every trigger in a frame comes down to one `fit` before the next paint. Resize
// observations come after the webview's layout, so they fit right away instead of
// showing the old layout for a frame (the webview sends them at most once per layout).
const WATCHED = '.tree-scroll, .center, .center > .pool, .center > .groups-panel, .topbar';
const watched = new WeakSet();
let frame = 0, rescan = false;
const run = () => {{
    if (frame) {{ cancelAnimationFrame(frame); frame = 0; }}
    // The graph's view may have grown past its drawn window, and the step bar may have
    // moved its steps.
    askWindow();
    if (window.__recvrvePlaceStep) window.__recvrvePlaceStep();
    if (rescan) {{
        rescan = false;
        document.querySelectorAll(WATCHED).forEach((el) => {{
            if (!watched.has(el)) {{
                watched.add(el);
                observer.observe(el);
            }}
        }});
    }}
    fit();
}};
const schedule = () => {{
    if (!frame) frame = requestAnimationFrame(run);
}};
const observer = new ResizeObserver(run);
// Only changes the layout depends on: panels coming and going, what's in the middle
// column besides the graph (rows of tiles, bars above the graph) and the graph's width
// (`data-w`). Changes inside the graph itself, the busiest part of the page, don't count,
// nor do the top bar's (a tab's unsaved mark changes with every edit): it is watched for
// size changes instead.
new MutationObserver((records) => {{
    let any = false;
    for (const r of records) {{
        if (r.type === 'attributes') {{ any = true; continue; }}
        const t = r.target;
        if (!t.closest || t.closest('.tree-canvas')) continue;
        if (!rescan) for (const n of r.addedNodes) {{
            if (n.nodeType === 1 && (n.matches(WATCHED) || n.querySelector(WATCHED))) {{
                rescan = true;
                break;
            }}
        }}
        if (rescan || t.closest('.center, .tree-scroll')) any = true;
    }}
    if (any) schedule();
}}).observe(document.body, {{
    childList: true, subtree: true, attributes: true, attributeFilter: ['data-w'],
}});
// The graph only draws a window of its rows (`data-win-lo` to `data-win-hi` on the
// canvas, built around `data-win-center`; none when it draws them all). Once the view
// gets near the window's edge (half of the room it had on that side when the window was
// built), it asks for a window around where the view is. Checked once per frame while
// scrolling and after every redraw or resize of the graph, never polled; one request at
// a time.
const graphParts = () => {{
    const sc = document.querySelector('.center .tree-scroll');
    const canvas = sc && sc.querySelector('.tree-canvas');
    if (!canvas || !canvas.dataset.geom) return null;
    const [a, b, sa, sb] = canvas.dataset.geom.split(' ').map(parseFloat);
    if (![a, b, sa, sb].every(isFinite) || !(b > 0) || !(sb > 0)) return null;
    return {{ sc, canvas, a, b, sa, sb, x: canvas.dataset.winAxis === 'x' }};
}};
// Layout units (row, step) at a point of the view, and the scroll position that puts
// them there again.
const unitsAt = (g, left, top, ox, oy) => {{
    const [pb, ps] = g.x ? [left + ox, top + oy] : [top + oy, left + ox];
    return {{ row: (pb - g.a) / g.b, step: (ps - g.sa) / g.sb, ox, oy }};
}};
// `axes`: which of the branching and the step axis to move (both by default).
const scrollTo = (g, u, axes = [true, true]) => {{
    const pb = g.a + u.row * g.b, ps = g.sa + u.step * g.sb;
    const [left, top] = g.x ? [pb - u.ox, ps - u.oy] : [ps - u.ox, pb - u.oy];
    const [moveLeft, moveTop] = g.x ? axes : [axes[1], axes[0]];
    if (moveLeft) g.sc.scrollLeft = left;
    if (moveTop) g.sc.scrollTop = top;
}};
let winFrame = 0, winPending = null;
const checkWindow = () => {{
    winFrame = 0;
    const g = graphParts();
    if (!g) return;
    const d = g.canvas.dataset;
    if (!d.winLo) return;
    const lo = parseFloat(d.winLo), hi = parseFloat(d.winHi);
    const center = parseFloat(d.winCenter), rows = parseFloat(d.rows);
    if (![lo, hi, center, rows].every(isFinite)) return;
    if (winPending !== null) {{
        if (winPending === d.winCenter) return;
        winPending = null;
    }}
    const size = g.x ? g.sc.clientWidth : g.sc.clientHeight;
    const pos = g.x ? g.sc.scrollLeft : g.sc.scrollTop;
    const vlo = (pos - g.a) / g.b, vhi = (pos + size - g.a) / g.b;
    const half = size / 2 / g.b;
    const room = [d.winStart ? Infinity : center - lo - half, d.winEnd ? Infinity : hi - center - half];
    const margin = Math.max(0, Math.min(...room)) / 2;
    if ((!d.winStart && vlo < lo + margin) || (!d.winEnd && vhi > hi - margin)) {{
        const row = Math.min(Math.max((vlo + vhi) / 2, 0), rows);
        if (Math.abs(row - center) < 0.5) return;
        winPending = d.winCenter;
        // Should the app keep this window after all, ask again later.
        setTimeout(() => {{
            if (winPending === d.winCenter) {{ winPending = null; askWindow(); }}
        }}, 1000);
        dioxus.send('win:' + row);
    }}
}};
const askWindow = () => {{
    if (!winFrame) winFrame = requestAnimationFrame(checkWindow);
}};
// Where the view was last seen, for keeping it in place when the geometry changes (by
// then the browser may already have clamped the scroll position to the new size).
let lastView = null;
const viewOf = (sc) => ({{ sc, left: sc.scrollLeft, top: sc.scrollTop, w: sc.clientWidth, h: sc.clientHeight }});
document.addEventListener('scroll', (e) => {{
    if (!e.target.closest || !e.target.closest('.center .tree-scroll')) return;
    lastView = viewOf(e.target);
    askWindow();
}}, {{ capture: true, passive: true }});
// The arrow keys (the pan actions, see keys.rs) move the graph's view like scrolling, and
// leave the selection alone: smoothly, for as long as a key is held, and by one wheel step
// for a tap. Two keys held move it diagonally. The drawn window follows the view as it
// does for any scroll.
const PAN_SPEED = 900, PAN_STEP = 100;
const PAN_DIRS = {{ up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] }};
// Keys held, by key code: direction, pixels moved, and whether it was let go.
const panKeys = new Map();
let panFrame = 0, panLast = 0;
const panTick = (now) => {{
    panFrame = 0;
    const dt = panLast ? Math.min(0.05, (now - panLast) / 1000) : 1 / 60;
    panLast = now;
    let dx = 0, dy = 0;
    for (const [code, p] of panKeys) {{
        // A tap goes on until it moved a whole step.
        const left = p.released ? PAN_STEP - p.moved : Infinity;
        if (left <= 0) {{
            panKeys.delete(code);
            continue;
        }}
        const step = Math.min(PAN_SPEED * dt, left);
        p.moved += step;
        dx += PAN_DIRS[p.dir][0] * step;
        dy += PAN_DIRS[p.dir][1] * step;
    }}
    const sc = document.querySelector('.center .tree-scroll');
    if (sc && (dx || dy)) sc.scrollBy(dx, dy);
    if (panKeys.size) panFrame = requestAnimationFrame(panTick);
    else panLast = 0;
}};
window.__recvrvePan = (dir, code, down) => {{
    if (down) {{
        const held = panKeys.get(code);
        // Held keys repeat; they already move.
        if (!PAN_DIRS[dir] || (held && !held.released)) return;
        panKeys.set(code, {{ dir, moved: 0, released: false }});
    }} else if (code === null) {{
        panKeys.clear();
    }} else {{
        const held = panKeys.get(code);
        if (held) held.released = true;
    }}
    if (!panFrame && panKeys.size) panFrame = requestAnimationFrame(panTick);
}};
// Ctrl+wheel over the graph zooms it, keeping the point under the mouse in place: the
// point, in layout units, stays the anchor until the wheel rests.
let zoomAnchor = null, zoomTimer = 0;
document.addEventListener('wheel', (e) => {{
    if (!e.ctrlKey) return;
    const sc = e.target.closest && e.target.closest('.center .tree-scroll');
    if (!sc) return;
    e.preventDefault();
    const g = graphParts();
    if (g && g.sc === sc) {{
        const rect = sc.getBoundingClientRect();
        zoomAnchor = unitsAt(g, sc.scrollLeft, sc.scrollTop, e.clientX - rect.left, e.clientY - rect.top);
    }}
    clearTimeout(zoomTimer);
    zoomTimer = setTimeout(() => {{ zoomAnchor = null; }}, 1000);
    dioxus.send('zoom:' + Math.exp(-e.deltaY * 0.0015));
}}, {{ passive: false }});
// When the graph's geometry changes (zoom, a resize, the zoom chip), the view stays on
// the same spot: the zoom anchor, or else the point that was in the middle of the view.
// A new graph (another direction, shown again) starts where the app puts it.
let lastGeom = null;
// The graph, already at its new size, is drawn scaled back to the old one around the
// zoom's anchor point and eases into place: only the compositor works meanwhile.
const easeZoom = (g, u, sx, sy) => {{
    if (Math.abs(sx - 1) < 0.001 && Math.abs(sy - 1) < 0.001) return;
    const c = g.canvas;
    c.style.transition = 'none';
    c.style.transform = '';
    const sr = g.sc.getBoundingClientRect(), cr = c.getBoundingClientRect();
    c.style.transformOrigin = (u.ox + sr.left - cr.left) + 'px ' + (u.oy + sr.top - cr.top) + 'px';
    c.style.transform = 'scale(' + sx + ', ' + sy + ')';
    c.getBoundingClientRect();
    c.style.transition = 'transform 0.16s ease-out';
    c.style.transform = '';
    clearTimeout(c.zoomEase);
    c.zoomEase = setTimeout(() => {{
        c.style.transition = '';
        c.style.transformOrigin = '';
    }}, 220);
}};
// Smooth animations: the highlight that slides along the bottom step bar to the step to
// choose next (see `OrderStrip`). The first placement doesn't slide.
window.__recvrvePlaceStep = () => {{
    const cursor = document.querySelector('.order-cursor');
    if (!cursor) return;
    const step = cursor.parentElement.querySelector('.order-step.current');
    if (!step) {{
        cursor.style.opacity = '0';
        return;
    }}
    const first = !cursor.dataset.placed;
    if (first) cursor.style.transition = 'none';
    cursor.style.opacity = '';
    cursor.style.width = step.offsetWidth + 'px';
    cursor.style.height = step.offsetHeight + 'px';
    cursor.style.translate = step.offsetLeft + 'px ' + step.offsetTop + 'px';
    if (first) {{
        cursor.getBoundingClientRect();
        cursor.style.transition = '';
        cursor.dataset.placed = '1';
    }}
}};
const graphChanged = () => {{
    const g = graphParts();
    if (g) {{
        const same = lastGeom && lastGeom.canvas === g.canvas && lastGeom.x === g.x;
        // Only an axis whose mapping changed moves; the other keeps its scroll position.
        const axes = same ? [lastGeom.a !== g.a || lastGeom.b !== g.b,
            lastGeom.sa !== g.sa || lastGeom.sb !== g.sb] : [false, false];
        if (axes[0] || axes[1]) {{
            let u = zoomAnchor;
            if (!u) {{
                const v = lastView && lastView.sc === g.sc ? lastView : viewOf(g.sc);
                u = unitsAt(lastGeom, v.left, v.top, v.w / 2, v.h / 2);
                u.ox = g.sc.clientWidth / 2;
                u.oy = g.sc.clientHeight / 2;
            }}
            scrollTo(g, u, zoomAnchor ? [true, true] : axes);
            // Smooth animations: a zoom (the wheel or the zoom keys, not a resize or another
            // tab) eases from the old size to the new one.
            const zoomed = zoomAnchor || performance.now() - (window.__recvrveZoomKey || -1e9) < 1000;
            if (window.__recvrveSmooth && zoomed) {{
                const ratio = (was, now) => (now > 0 ? was / now : 1);
                const sx = ratio(lastGeom.x ? lastGeom.b : lastGeom.sb, g.x ? g.b : g.sb);
                const sy = ratio(lastGeom.x ? lastGeom.sb : lastGeom.b, g.x ? g.sb : g.b);
                easeZoom(g, u, sx, sy);
            }}
        }}
        lastView = viewOf(g.sc);
    }}
    lastGeom = g;
    askWindow();
}};
new MutationObserver(graphChanged).observe(document.body, {{
    subtree: true, attributes: true,
    attributeFilter: ['data-geom', 'data-win-lo', 'data-win-hi', 'data-win-center'],
}});
// Tile size, orientation and fullscreen changes show on the app root.
const rootObserver = new MutationObserver(schedule);
const appRoot = document.getElementById('app-root');
if (appRoot) rootObserver.observe(appRoot, {{
    attributes: true, attributeFilter: ['class', 'style'],
}});
rescan = true;
schedule();
}})();
"#,
        side_min = Layout::SIDE_MIN,
        side_max = Layout::SIDE_MAX,
        pool_min = POOL_MIN,
        groups_min = GROUPS_MIN,
        tree_min = TREE_MIN,
        graph_min = GRAPH_MIN,
        graph_share = GRAPH_SHARE,
    )
}

/// Installs the splitter script. Call once and keep the returned handle.
pub fn listen() -> Eval {
    document::eval(&script())
}

/// A message from the script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    /// New panel sizes.
    Layout(Layout),
    /// The size (width, height), in pixels, of the graph's scroll area.
    TreeSize(f64, f64),
    /// Ctrl+wheel over the graph: multiply its zoom by this.
    Zoom(f64),
    /// The graph was scrolled too far from the middle of its drawn window: centre the
    /// window on this row (see `tree_view::GraphWindow`).
    Window(f64),
}

/// What a message from the script asks for, applied to `current`.
pub fn apply_message(current: Layout, message: &str) -> Option<Layout> {
    if let Some(json) = message.strip_prefix("set:") {
        // Fields the message leaves out (the other orientation's side width) stay.
        let mut value = serde_json::to_value(current).ok()?;
        let changes: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(json).ok()?;
        let fields = value.as_object_mut()?;
        for (k, v) in changes {
            fields.insert(k, v);
        }
        let layout: Layout = serde_json::from_value(value).ok()?;
        return Some(layout.clamped());
    }
    let defaults = Layout::default();
    let reset = match message.strip_prefix("reset:")? {
        "left" | "right" => Layout {
            side: defaults.side,
            ..current
        },
        "pool" => Layout {
            pool: defaults.pool,
            groups: defaults.groups,
            h_auto: true,
            ..current
        },
        "groups" => Layout {
            groups: defaults.groups,
            h_auto: true,
            ..current
        },
        "fs-pool" => Layout {
            fs_groups: defaults.fs_groups,
            ..current
        },
        "fs-groups" => Layout {
            fs_top: defaults.fs_top,
            ..current
        },
        "v-left" | "v-right" => Layout {
            v_side: defaults.v_side,
            ..current
        },
        "v-pool" => Layout {
            v_groups: defaults.v_groups,
            ..current
        },
        "v-groups" => Layout {
            v_top: defaults.v_top,
            v_auto: true,
            ..current
        },
        _ => return None,
    };
    Some(reset.clamped())
}

/// Waits for the next message from the script. `None` once the script is gone.
pub async fn next(listener: &mut Eval, current: impl Fn() -> Layout) -> Option<Message> {
    loop {
        let message = listener.recv::<String>().await.ok()?;
        if let Some((w, h)) = message
            .strip_prefix("tree-size:")
            .and_then(|size| size.split_once(','))
            && let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>())
        {
            return Some(Message::TreeSize(w, h));
        }
        if let Some(row) = message.strip_prefix("win:")
            && let Ok(row) = row.parse::<f64>()
            && row.is_finite()
        {
            return Some(Message::Window(row));
        }
        if let Some(f) = message.strip_prefix("zoom:")
            && let Ok(f) = f.parse::<f64>()
            && f.is_finite()
            && f > 0.0
        {
            return Some(Message::Zoom(f));
        }
        if let Some(layout) = apply_message(current(), &message) {
            return Some(Message::Layout(layout));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_messages() {
        let current = Layout {
            side: 300.0,
            pool: 0.6,
            groups: 0.1,
            ..Layout::default()
        };
        let set = apply_message(current, r#"set:{"side":180,"pool":0.4,"groups":0.25}"#).unwrap();
        assert_eq!((set.side, set.pool, set.groups), (180.0, 0.4, 0.25));

        // Either side splitter resets the shared width.
        for kind in ["left", "right"] {
            let reset = apply_message(current, &format!("reset:{kind}")).unwrap();
            assert_eq!((reset.side, reset.pool), (250.0, 0.6));
        }
        let reset = apply_message(current, "reset:pool").unwrap();
        assert_eq!((reset.pool, reset.groups), (0.5, 0.2));

        assert!(apply_message(current, "set:{broken").is_none());
        assert!(apply_message(current, "reset:nope").is_none());
        assert!(apply_message(current, "other").is_none());
        // Out-of-range values are clamped.
        let wide = apply_message(current, r#"set:{"side":9000}"#).unwrap();
        assert_eq!(wide.side, Layout::SIDE_MAX);
        // Fullscreen sizes reset on their own.
        let full = apply_message(current, r#"set:{"fs_top":0.5,"fs_groups":400}"#).unwrap();
        assert_eq!((full.fs_top, full.fs_groups), (0.5, 400.0));
        let reset = apply_message(full, "reset:fs-pool").unwrap();
        assert_eq!((reset.fs_top, reset.fs_groups), (0.5, 280.0));
        let reset = apply_message(full, "reset:fs-groups").unwrap();
        assert_eq!((reset.fs_top, reset.fs_groups), (0.36, 400.0));
        // Top to bottom has its own sizes; a message without `side` keeps it.
        let v = apply_message(current, r#"set:{"v_side":200,"v_top":0.5,"v_groups":300}"#).unwrap();
        assert_eq!(
            (v.side, v.v_side, v.v_top, v.v_groups),
            (300.0, 200.0, 0.5, 300.0)
        );
        let reset = apply_message(v, "reset:v-left").unwrap();
        assert_eq!((reset.side, reset.v_side), (300.0, 250.0));
        let reset = apply_message(v, "reset:v-pool").unwrap();
        assert_eq!((reset.v_top, reset.v_groups), (0.5, 280.0));
        let reset = apply_message(v, "reset:v-groups").unwrap();
        assert_eq!((reset.v_top, reset.v_groups), (0.64, 300.0));
        // Dragging the row's splitter turns the automatic size off; resetting turns it on.
        let dragged = apply_message(current, r#"set:{"v_top":0.5,"v_auto":false}"#).unwrap();
        assert!(!dragged.v_auto);
        assert!(apply_message(dragged, "reset:v-groups").unwrap().v_auto);
        let pool = apply_message(dragged, "reset:v-pool").unwrap();
        assert!(!pool.v_auto);
        // The same for left to right.
        let dragged = apply_message(current, r#"set:{"pool":0.4,"h_auto":false}"#).unwrap();
        assert!(!dragged.h_auto && dragged.v_auto);
        assert!(apply_message(dragged, "reset:pool").unwrap().h_auto);
        assert!(apply_message(dragged, "reset:groups").unwrap().h_auto);
    }
}
