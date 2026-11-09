<p align="center"><img src="assets/icon/recvrve.svg" alt="Recvrve logo" width="160"></p>

# Recvrve
Draft preparation tool with features that shouldve existed 5 years ago.

Current version: **0.3.1 Beta**. Copyright (C) 2026 [odparovacc](https://github.com/odparovacc). Licensed under the GNU General Public License, version 2 or (at your option) any later version (see `LICENSE`). The fonts and libraries it includes keep their own licenses, collected in `THIRD-PARTY-NOTICES.txt` (installed next to the app; regenerate it with `python tools/third_party_notices.py` after changing dependencies — the Windows installer workflow does so on every build).

## Running

Requires Rust and, on Linux, the WebKitGTK dev packages
(`libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev`).

```sh
cargo run --release
```

The repository, crate and binary are still called `draftree`; only the app itself is
named Recvrve (the Windows installer installs the binary as `Recvrve.exe`).

A save file given on the command line (`draftree path/to/tree.dts`, or double-clicking a
`.dts` / `.dtt` file once the installer associated them) opens in its own tab.

## Windows installer

`installer/recvrve.iss` is an [Inno Setup](https://jrsoftware.org/isinfo.php) 6.3+ script
that builds a single setup program, `Recvrve-0.2.1-beta-setup.exe`:

```bat
cargo build --release
iscc installer\recvrve.iss
```

The setup lands in `target\installer\`. The **Windows installer** GitHub Actions workflow
does the same on a Windows runner: run it from the Actions tab, or push a tag like `v0.2.1`
to attach the setup to a (pre-)release.

The wizard: a welcome page titled "Recvrve 0.3.1 Beta", the license (GNU GPL v2 or later, with the font and
Riot Games notices), the install folder (`Program Files\Recvrve`), the Start menu folder,
a desktop shortcut and the `.dts` / `.dtt` file associations (both optional), then install
and finish with "Launch Recvrve". Music plays while the wizard is open, quietly, with a
**Mute music** button at the bottom left; silent installs (`/SILENT`, `/VERYSILENT`) skip it.

The app's window runs on Microsoft's WebView2 Runtime. Windows 11 and up-to-date Windows 10
have it; otherwise the setup downloads Microsoft's bootstrapper and installs it. The
uninstaller (Settings → Apps) removes the program but leaves saves and settings.

An installed copy (marked by the `recvrve-installed` file next to `Recvrve.exe`) can't
write to Program Files, so it keeps:

- settings, keybinds, group configs and themes in `%APPDATA%\Recvrve\config`, copied
  from the shipped `config` on the first start (shipped defaults are refreshed on updates;
  your own files are never overwritten);
- saves, exports and autosaves in `Documents\Recvrve\`.

Running from source, or a copy without the marker, keeps everything next to the working
directory or the executable, as before. Release builds on Windows open without a console
window, and the executable carries the app icon and version details (`build.rs`).

Champion icons are read from `champion_icons/<key>.png`, where `key` comes from the
champion table in `src/champions.rs`. The folder is looked up in this order:
`$RECVRVE_ICONS` (or `$DRAFTREE_ICONS`), `./champion_icons`, next to the executable, then the crate root.

## Draft tree

`src/tree.rs` stores drafts as a tree in a `slotmap` arena. The root is the empty draft;
each level below it is the next action in the draft order. Every node stores:

- its draft position byte (bit 7 team, bit 6 ban/pick, bits 0-5 number; see `src/draft.rs`),
- its value byte: `0` empty/wildcard, `1` no ban, `16..` champions in release order
  (see `src/champions.rs`),
- its parent (`None` for the root) and its children, in insertion order.

Champions and groups appear at most once among siblings. Empty and No ban may repeat, so
one state can have several open branches; in the graph they get small numbers (1, 2, …)
when there is more than one. Clicking Empty / No ban follows the first empty branch;
**Shift + click** always adds another one.

### Adding several branches at once

Hold **Shift** and click champions, groups or Empty: each click adds a branch below the
selected node without moving. Clicking a value the node already has a branch for adds
nothing, but still counts as clicked. When Shift is released, the last child clicked
becomes the selected node. Pressing and releasing Shift without clicking does nothing.

From the first Shift + click until Shift is released, every other shortcut is off (Shift + Z
or Ctrl + Z do nothing then), so the tree can't change under the pending selection. If the
window loses focus while Shift is held, that counts as releasing it.

The modifier can be changed under **Help → Customize** (Shift, Ctrl, Alt, Ctrl + Shift,
Alt + Shift or Ctrl + Alt); it is stored as `"branch-click"` in `config/keybinds.json`.
Extra modifiers held with it are fine. Some Linux desktops use Alt + drag to move
windows, so Alt may not reach the app.

### Editing a node's value

**Ctrl + W** turns on set mode for the selected node (a banner above the grid says
so; Esc cancels). The next champion or group clicked becomes that node's value, and its
subtree is kept. The grid then shows what fits *at* the node rather than after it:

- values that the line above the node rules out (already on the line, a group with no
  champions left, fearless bans at that position) are greyed out, as usual;
- choosing a value a sibling already holds selects that sibling and changes nothing;
- choosing Empty / No ban empties the node.

Role tags stay, except that a group with a role lock narrows them to the lock (or sets
them to it when they don't overlap, or clears them if that wouldn't fit the team).

### Conflicting branches

Only the line *above* an edited node is checked. If the new value breaks something
further down, that part of the tree is kept but marked as a **conflicting branch**:

- the node where a line first breaks gets a green ring and a green line leading to it;
- that node and everything below it are faded, with green lines;
- its tooltip says why: "Annie is already on this line at Blue Ban 1", or "every champion
  of G1 is already accounted for on this line".

Nothing is copied or deleted, so changing the value back (or to anything that fits) makes
the branches normal again at once, and undo works as usual. The rules are the same as for
adding nodes:

- a champion may appear once per line;
- a group of *n* champions used *k* times, with *e* of its champions also on the line
  explicitly, has *n − k − e* champions left; it can be used again, and its champions can
  be chosen, only while that is above zero. Overlapping groups are counted separately.

So an edit can create a conflict in three ways: the new champion appears again below;
the new champion uses up the last free champion of a group that appears below (that
group node, or a later champion of the group, conflicts); or the new group claims
champions that appear below. The first problem on a line decides what is marked:
everything below it inherits the mark, and fearless-unreachable lines work the same way.
If a fearless ban is reached first, the line is unreachable rather than conflicting.

Conflicting branches can be selected, edited and deleted, but nothing new can be added
below them until the conflict is fixed: either set the conflicting node itself to
another value, or change the value above. **Remove conflicting** in the graph header
(Ctrl + Alt + C) deletes them all; **Restore conflicting** (Ctrl + Shift + C, or Ctrl + Z)
brings them back. Exports leave
them out unless **Show conflicting branches** is ticked; then they are drawn faded with
green lines, labelled "conflict" on cards and tagged `conflict="true"` (where the line
breaks) or `conflict="below"` in XML.

### Duplicating branches

| Keys | Copies | The copy starts |
|---|---|---|
| Ctrl + D | the selected node and everything below it | next to the selected node |
| Ctrl + Alt + D | the line from Blue Ban 1 down to the selected node | as a new Blue Ban 1 branch |
| Ctrl + Shift + D | that line and everything below the selected node | as a new Blue Ban 1 branch |

The copy's first node is Empty (No ban at bans), since it can't repeat its sibling's
champion; everything else keeps its values and role tags. The copy of the selected node is
selected afterwards, so Ctrl + W can give the copy its own value right away. Some
Linux desktops use Ctrl + Alt + D themselves.

### Selecting several nodes

Ctrl + click a node to add it to the selection or take it out again; the selected node
(the gold one, whose draft state the panels show) stays where it is. Dragging over empty
space in the graph draws a selection box: the nodes it touches become the selection, or
are added to it with Ctrl held. Only nodes at one draft step can be selected together, so
nodes at other steps are left out (the box keeps those at the selected node's step, or at
the first step it touches). Selected nodes get a dashed white ring.

With several nodes selected:

- **Grid clicks** and Shift + click batches apply to every selected node; afterwards the
  new children are the selection.
- **Delete**, **Ctrl + H** (hide or show), **Ctrl + Shift + W** (empty), **Alt + 1 … 5**
  (role tags, on the selected picks), **Ctrl + M** (mark) and **Ctrl + D** (duplicate,
  the copies become the selection) act on all of them.
- Ctrl + W, Ctrl + Alt + D, Ctrl + Shift + D, Ctrl + ← / → and dropping a tile only act on
  the gold node. Ctrl + B / Shift + B move the gold node and keep the selection.
- Shift + Z / Shift + X, clicking a node, loading, Reset, undo and redo clear the selection.
  Right-click deletes only the clicked branch and clears the selection.

Each of these is one undo step, however many nodes it changes.

### Moving and copying branches

- **Reorder:** Ctrl + ← / Ctrl + → move the selected node one place earlier / later among
  its siblings (in both graph directions), stopping at the ends.
- **Copy, cut and paste:** Ctrl + C copies the selected nodes, each with everything below
  it; Ctrl + X does the same and deletes them. Ctrl + V puts the copies below the gold
  node, which has to be one draft step above the copied nodes (bans and picks depend on
  the step). The copied branches stay available after loading another tree or group
  config.
- **Drag and drop:** drag a node onto another node to copy its branch there; hold Shift
  when you drop to move it instead. The target has to be one draft step above the dragged
  node: it turns green when it takes the drop, red when it doesn't. Dragging a selected
  node takes the whole selection along. Dragging a champion, group or Empty tile from the
  selection panel onto a graph node gives that node the tile's value, like Ctrl + W. The
  graph scrolls near its edges while you drag, and Esc cancels.

Pasting and dropping work like merging files: a copied node and a child that already holds
the same champion or group become one, and empties join in order among those with the same
role tags. The new nodes may repeat a champion of their new line; those branches show as
conflicting. Role tags that don't fit the new line are dropped, and a notice says so.

### Marks

**Ctrl + M** marks the selected node, and pressing it again removes the mark. Every node on
the line from the start down to a marked node gets a small gold star in its bottom-right
corner, so you can follow the lines that matter. Pressing Ctrl + M on a node that only has a
star (because of a mark below it) removes the marks below it. A node whose branches are
hidden shows a star when they contain a marked line.

**Marked only** in the Draft tree header (shown under Help → Button settings) shows just the starred lines; it turns itself off
when no marks are left. Marks are an undo step, are saved with the tree, and show up in
exports (see below).

### Team names and patch

Double-click **Blue Team** or **Red Team** above the side panels to rename them (Enter or
clicking away keeps the name, Esc cancels, an empty name goes back to the default; at most
24 characters). The **Patch** field at the bottom left, next to the draft order strip, holds
up to 16 characters. Both are saved with the tree. Save dialogs start with
`G2-vs-Fnatic-2026-10-04.dts` when names are set (`G2-vs-red` when only one is), and exports
start with `G2-vs-Fnatic` instead of `recvrve`. Merging files whose names (or patches)
differ resets them to the defaults.

### Working with large trees

- **Hiding branches (Ctrl + H, or Hide branches in the Draft tree header):** hides
  everything below the selected node. The nodes stay in the tree; the node turns round,
  with four small satellites and the number of hidden nodes in the color of the line
  into it (gold on the selected line, the fearless colors for unreachable branches, green
  for conflicts). The number replaces a ban's red sign. Ctrl + H again (or Show branches)
  shows them; **Show all** (Ctrl + Shift + H) shows every hidden branch. Hiding isn't an
  edit, so undo doesn't step through it, and it is saved with the tree. Whatever selects a
  node inside hidden branches (a grid click, Shift + X, Find, a Shift + click batch) shows
  the hidden nodes above it; Ctrl + B / Shift + B skip nodes inside hidden branches.
- **Find (Ctrl + F):** a bar above the graph. Type part of a champion's name, or a group's
  name or label (TOP, G2, …); matches get a cyan ring and the bar shows "3 / 12".
  **Enter / Shift + Enter** (or ↓ / ↑ in the Find box) select the next / previous match in the graph's order,
  wrapping around; they only do that while the bar is open. **Include groups** also matches
  group nodes holding a matching champion; **Search in hidden branches** looks inside hidden
  branches too (going to a match there shows it). Esc or × closes the bar. Typing selects
  the first match (unless the selected node still matches), so the graph jumps there.
- **Only part of a big graph is drawn:** the whole tree stays loaded, but a graph of more
  than 1,000 nodes draws a window around the view: about 5 screens across in the
  top-to-bottom view and 3 screens up and down left to right (every draft step is always
  drawn). Scrolling, resizing, zooming and selecting a node outside it move the window
  along before the view reaches its edge, so nothing seems missing. A window holds whole
  rows, at most 1,000 nodes, but always the visible screen and one more on each side.
  Lines to nodes outside it, and lines passing through it, are still drawn, and a
  selection box selects every node it touches, drawn or not.
- **Moving around:** the arrow keys move the graph's view like scrolling, smoothly while a
  key is held (two keys move diagonally); the selection stays where it is. They do nothing
  while a popup or menu is open, and in text fields they move the text cursor as usual.
- **Zoom (Ctrl + mouse wheel over the graph, or Ctrl + = / Ctrl + -):** 30–200 %, kept for
  each graph direction (Ctrl + = also works as Ctrl + + and with the numpad's + and -;
  Ctrl + Shift + 0 goes back to 100 %);
  the point under the mouse stays put. The step labels keep their size, and the turn and
  phase lines follow the zoom. Left to right, 100 % fits all draft steps into the width as
  before; zooming in scrolls. The chip next to "Draft tree" shows the zoom; click it to go
  back to 100 %.
- **Previews:** resting the pointer on a graph node for a moment shows the draft state at
  that node: both team panels, small and side by side, with a row of portraits for each
  group on the line above them, and a header for nodes that hold a group or an empty
  value. It only shows; the team panels keep showing the selected node. The **Previews**
  button in the top bar turns them on and off (on by default). **Tooltips** turns on the
  browser's tooltips on graph nodes, with debug details (off by default). Both buttons are
  hidden by default; show them under **Help → Button settings**.

## Follow mode

During a real draft (a scrim, a stage game on stream), **Follow** in the top bar (shown
under Help → Button settings) or
**Ctrl + Shift + L** lets you enter each action as it happens and see what you prepared for it.
It starts from the empty draft; while it is on, the tree can't be changed.

- **Entering actions:** click a champion, group or Empty / No ban tile (or a tile in the
  *Prepared for …* strip at the bottom of the graph) for each action in draft order. The
  team panels show what was entered.
- **Matching:** the entered value is matched against the prepared branches: a node with that
  champion fits best, then a group holding it, then an empty pick or skipped ban. Every
  branch that fits is followed at once; the best one is selected (gold) and the bar above
  the graph says how many lines still fit. Branches that fearless rules out or that
  conflict never fit. A champion only prepared through a group still shows its own portrait
  in the team panels.
- **What you see:** lines that no longer fit are dimmed and the ones that do are lit. The
  strip at the bottom lists the values prepared for the next step, the grid outlines those
  champions and groups, and champions of a prepared group get that group's ring. The graph
  scrolls along with the draft.
- **Backspace** (or Shift + Z, or **Back**) takes back the last action.
- **Leaving the tree:** when an action has no prepared branch, a warning says so and offers
  **Back to editor** (leaves follow mode with the last node that fitted selected, ready to
  prepare the missing branch) or **Keep following** (keeps entering actions; they show in
  the team panels but fit nothing). Nothing is ever added to the tree on its own.
- **Leave** (or Ctrl + Shift + L again) also goes back to the editor with the last node that fitted
  selected.

Follow mode belongs to its tab (see below), so another tab can be edited meanwhile.

## Tabs

Several trees can be open at once, each in a tab in the top bar to the right of the tile
size slider.

- **New tab:** **+** or **Ctrl + T** opens an empty tree with the active tab's groups.
- **Switching:** click a tab, or **Alt + `** for the next one (some Linux desktops use
  Alt + ` themselves; it can be changed in Help → Customize).
- **Names:** the team names when one is set ("G2 vs Fnatic"), otherwise the file the tree
  came from, otherwise "Draft 1", "Draft 2", …. A dot marks unsaved changes, a ▶ follow
  mode; hovering a tab shows its file.
- **Closing:** the tab's **×** or a middle click. A tab with unsaved changes asks first.
  Closing the last tab leaves a new empty one. (Ctrl + W still sets a node's value; it
  doesn't close tabs.)
- **Order:** drag a tab onto another to move it before or after it. When the tabs don't fit,
  they shrink and then scroll (with the mouse wheel over them).
- **Loading:** **Load**, a pasted tree string and a merge result open in a new tab, or in
  the active tab while it is a new tab nobody touched.
- **Windows:** right-click a tab for **Open in new window** (or **Ctrl + Shift + N** for the
  active tab): the tab moves to a window of its own, with everything it keeps. In another
  window, the same menu offers **Move to Window N: …** for each open window; a window whose
  last tab leaves closes. Each window has its own tabs and panel sizes; the settings,
  shortcuts, theme, selection groups and clipboard are shared, and a change in one window
  shows in all of them. Closing a window closes its tabs, asking first if any has unsaved
  changes; the app ends with its last window.

Each tab keeps its own tree, undo history, group config (a loaded file can import groups,
see "Groups in save files"), selection, Find bar, zoom, Marked only and other graph
options, and follow mode. The clipboard (Ctrl + C / Ctrl + V), selection groups, layout,
theme and settings are shared, so branches can be copied from one tab (or window) to
another.

## Save files

**Save ▾** writes the whole tree in one of two formats; **Load** picks the format from the
file extension and opens the tree in a new tab. Both open in the `saves/` folder by
default.

- `.dts` (compact): a `DTRE` signature and format version byte, the definitions of the user
  groups the tree uses, the node count, then one value byte per node plus 2 bits of shape
  per node (about 10 bits per node).
- `.dtt` (text): the same bytes as one line of URL-safe base64, easy to paste into chat.

**Share** in the top bar shows the current tree as a `.dtt` string in a bar under the toolbar, kept up to
date as the tree changes. Pasting a valid string into it opens that tree in a new tab
straight away.

Ticking **Include selected node** in the Save menu (off by default) also stores which node
is selected, so loading the file opens that draft state. It adds 1-3 bytes; without it the
loaded tree opens at the root.

Save files also store the fearless draft state, role tags, repeated empty siblings,
conflicting branches, hidden branches (Ctrl + H), marks (Ctrl + M), the team names and
the patch, which all load exactly as they were saved. The current format is version 7;
versions 5 and 6 still load (with no marks or names, and for version 5 nothing hidden),
older ones don't.

**Size limit:** trees open with up to 250,000 nodes (far beyond any real prep tree). A
larger file, pasted string, autosave or merge result is refused with a message giving its
size, before anything is built, so a broken or hostile file can't freeze the app.

### Autosave and crash recovery

Every two minutes, the tabs whose tree changed since the last autosave are written to
`saves/autosave/` (in the compact format, on a background thread, so it doesn't slow the
app down; unchanged tabs cost nothing, and new tabs nobody touched are skipped). **View →
Autosave** (in Help) sets the interval (every 1, 2, 5 or 10 minutes) or turns it off.

Each run of the app has its own folder there (with a list of tabs per window), deleted
when the app closes normally; a window closed on its own takes its tabs' files with it. If the
app didn't close normally (a crash, a kill, a power cut), the next start offers to
**Restore** that session's tabs, each in its own tab (with the file it came from); **Discard**
deletes them. Several copies of the app can run at once without mixing their autosaves up.

### Groups in save files

A save carries the definitions of the user groups its tree uses (value, name, role lock
and champions, but not notes), so it doesn't need the config file it was made with. Each
group is stored once, however many nodes use it; nodes refer to it by value as before.
Champions are stored as a list, a 22-byte bitmap or a list of the champions *not* in the
group, whichever is shortest, so a typical group costs about 20 bytes. A hash of the
definitions is stored next to them and checked on load, so a damaged file is caught.

When a save loads, each stored group is matched against the active config:

1. same value, same champions: the active group is used (its name and note win);
2. another active group with the same champions and role lock: that one is used and the
   save's nodes are renumbered to it;
3. a value the active config doesn't use: the group is added at that value;
4. a value the active config uses for something else: the group is added at the first free
   value and its nodes are renumbered (so its label changes, e.g. G1 → G4).

If nothing had to be added, the active config stays. Otherwise the active groups plus the
added ones are written to `config/imported_configs/` (named after the save file, or
`pasted-<date-time>.json` for a pasted string; an identical file there is reused) and that
file becomes the active config for the session, so editing groups doesn't touch
`config/reprgroups.json`. A notice under the toolbar says what was imported and hides after
four seconds. If the groups don't fit next to yours (66 user groups at most), a warning
explains it and the save's own groups are used alone, written to `config/temp/`.
**Load RGroup config…** (in the representational group editor, under **Edit**) switches back
to any config file at any time.

### Merging save files

**Merge…** next to Load (shown under Help → Button settings) opens a panel: add any number of save files (in several rounds, from
any folders), remove ones you don't want, tick any **open tabs** to merge their trees
too, and press **Merge**. The result is saved as a `.dtt` file (the dialog starts in
`saves/` with the name `merged-YYYYMMDD-HHMM.dtt`) and opened in a new tab like any save,
so its groups are imported as described above. A notice says how many nodes were shared.

How trees combine:

- A line that appears in several files appears once. Where lines agree up to some point
  and then split, the branches join at that point.
- Two nodes are the same when everything above them is the same and they hold the same
  champion or group. If their role tags differ (including tagged against untagged), the
  merged node keeps no tags (any role); tags further down stay.
- Empty picks and skipped bans only merge with the same role tags, and in order: the first
  top-tagged empty of one file merges with the first top-tagged empty of another, the
  second with the second. Empties with different tags stay separate branches.
- Groups with the same champions become one group (the first file's number and name win;
  if only their role locks differ, the lock is dropped). Different groups that share a
  number keep apart, the later one moving to a free number.
- The first file's branches come first; new branches follow in file order.

Every line of the merged tree is a line of one of the files (apart from dropped tags), so
merging never creates conflicts or role problems. Files with different fearless settings
can't be merged; the app says which ones differ.

The layout is documented in `src/save.rs`. Loading checks what every tree must satisfy
(draft length, No ban only at bans, known champions and groups, no repeated champion or
group among siblings, role tags that fit), so a damaged file shows an error instead of
loading a bad tree.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| Shift + Click | Add a champion or group as a new branch without moving; releasing Shift selects the last one clicked |
| Shift + Z / Shift + X | Go to the parent / go down (the only child, or the last one visited) |
| Ctrl + B / Shift + B | Next / previous node at the same depth, anywhere in the tree |
| Shift + Click Empty | Add another empty branch, even if one exists |
| Ctrl + W | Set the selected node's value: the next grid click replaces it |
| Ctrl + Shift + W | Empty the selected node (No ban at bans, Empty at picks) |
| Ctrl + D | Duplicate the selected node's branch (the copy starts empty) |
| Ctrl + Alt + D | Duplicate the line from Blue Ban 1 down to the selected node |
| Ctrl + Shift + D | Duplicate that line and everything below the selected node |
| Delete | Delete the selected nodes and their subtrees (not while typing in a text field) |
| Ctrl + Click / drag on empty graph space | Add or remove a node from the selection / box select (Ctrl adds) |
| Ctrl + C / Ctrl + X / Ctrl + V | Copy / cut the selected branches / paste them below the selected node |
| Ctrl + ← / Ctrl + → | Move the selected node earlier / later among its siblings |
| Ctrl + M | Mark or unmark the selected nodes |
| Drag a node / Shift + drag | Copy / move its branch below another node |
| Drag a selection tile onto a node | Give the node that value |
| Ctrl + H / Ctrl + Shift + H | Hide or show the selected node's branches / show all hidden branches |
| Ctrl + Alt + H | Hide or show the branches below unreachable (fearless) nodes |
| Ctrl + Alt + U / Ctrl + Shift + U | Remove unreachable branches / restore the last removed ones |
| Ctrl + Alt + C / Ctrl + Shift + C | Remove conflicting branches / restore the last removed ones |
| Ctrl + R | Reset: delete the whole tree (Ctrl + Z brings it back) |
| Ctrl + F | Find in the tree (Enter / Shift + Enter: next / previous match while the bar is open) |
| Ctrl + mouse wheel / Ctrl + = / Ctrl + - | Zoom the graph |
| Ctrl + Shift + 0 | Graph zoom back to 100% |
| Arrow keys | Move the graph's view (the selection stays) |
| Ctrl + T / Alt + ` | New tab / next tab |
| Ctrl + Shift + N | Move the active tab to a new window |
| Ctrl + Shift + L | Start or leave follow mode |
| Ctrl + L / Ctrl + Shift + M / Ctrl + Shift + E | Load / Merge / open the Export menu |
| Backspace | Follow mode: take back the last action |
| Ctrl + Z / Ctrl + Y (or Ctrl + Shift + Z) | Undo / redo tree changes |
| Ctrl + S | Save as compact `.dts` |
| Ctrl + 1 … 9 / Ctrl + 0 | Toggle the first nine selection filters / turn all off |
| Ctrl + Q | Switch selection filters between union and intersection |
| Ctrl + E | Apply selection filters to representational groups too |
| Ctrl + G | Open the fearless draft editor |
| F11 | Fullscreen (Esc leaves it) |
| F1 | Open or close Help |
| Alt + 1 … 5 | Toggle the Top / Jungle / Mid / Bot / Support tag of the selected pick |
| Middle-click a pick | Edit its role tags |
| Esc | Close popups and editors; with nothing open, leave fullscreen |

Undo covers every change to the tree, including Reset. Each tab has its own history of up
to 200 steps; loading a file opens it in a new tab, so closing that tab is the way back. Each step
only stores what changed (a few dozen bytes for a single edit, even in a large tree).

### Changing shortcuts

The **Help** button (or **F1**, which also closes it) lists the shortcuts in use. The panel
fits the window, and scrolls when the window is too small for it. Next to its title sit
**Customize**, **Button settings** (see below), and the **View ▾** and theme menus. Until
Help has been opened once, its button glows softly (remembered in `config/settings.json`).
**Customize** lists every keyboard action:

- Click a shortcut, then press the new keys. Esc cancels.
- **+** adds another shortcut for the action; **×** removes one. An action can have
  several shortcuts or none.
- A shortcut needs at least two keys pressed together, such as Ctrl + K, Alt + 3 or
  Shift + K. Single keys are refused, apart from the shipped F1, F11 and Delete, which can be
  removed and brought back with **Reset**.
- A combo does one thing: a combo already used by another action moves to the new one,
  and the panel says which action lost it.
- **Reset** restores one action's default, **Reset all** every default.
- Esc can't be changed, and Ctrl + C, Ctrl + V, Ctrl + X, Ctrl + A, Ctrl + Z, Ctrl + Y,
  arrows, Home, End and Delete keep their usual meaning while a text field is focused.

Changes are saved right away to `config/keybinds.json` (with a `.bak` of the previous
version) and load at the next start. The file lists every action with its combos, e.g.
`"set-value": ["Ctrl+W"]`, and can be edited by hand: actions missing from it get their
defaults, and combos that break the rules above are skipped with a notice at startup.
Tooltips name the current shortcuts. Letters follow the keyboard layout; digits and other
keys are the physical keys, so Ctrl + 1 … 9 work on any layout.

### Button settings

**Help → Button settings** has a checkbox for every panel button, in sections for the top
bar, champions, representational groups and draft tree, with each button's shortcut next to
it. A hidden button leaves its panel; shown again, it comes back in its usual place. Its
shortcut works either way. **Show all** shows every button, **Reset to defaults** goes back
to the defaults below. Choices are saved in `config/settings.json`.

Shown by default:

- **Top bar:** Save, Load, Export, Share, Fearless (and Help, which can't be hidden).
- **Champions:** the selection group filters, Edit and Clear.
- **Representational groups:** Edit.
- **Draft tree:** nothing; every header button is hidden.

Hidden by default: Merge, Follow, Previews and Tooltips in the top bar;
RGroup filter, Union / Intersection, Auto-tag and Hide presets in the champions panel;
Details and Hide presets in the groups panel; and the zoom chip, Delete, Reset, Hide
branches, Show all, Marked only, Hide unreachable, Edit unreachable, Remove unreachable and
Remove conflicting in the draft tree (each Restore button follows its Remove button). The
fullscreen button, the tabs, the panels' ▾ and buttons in popups or in the follow and find
bars are always shown.

## Representational groups

A group is one node value standing for several champions the user treats as
interchangeable, so branches don't have to be repeated for each of them.

- Values 2-15 are preset groups defined in `src/groups.rs` (Any champion, roles, classes,
  and two placeholders).
- Values 190-255 are user groups from `config/reprgroups.json`, found the same way as
  `champion_icons/`. **Load RGroup config** switches to another file for the session.

```json
{ "groups": [ { "value": 190, "name": "Enchanters", "champions": [110, 49], "note": "..." } ] }
```

`champions` lists champion values (release order from 16, see `src/champions.rs`). A file
with any problem is rejected as a whole; presets and champions keep working.

Rules along one draft line: a group of `n` champions used `k` times, with `e` of its
champions also picked or banned explicitly, can be used again only while `k + e < n`, and
once `k + e == n` its remaining champions can't be picked or banned further down. Champions a
group earlier on the line may stand for glow in that group's color.

Save files carry the definitions of the user groups they use (see
[Groups in save files](#groups-in-save-files)), so they load under any config. Switching
configs with **Load RGroup config** empties every user-group node and clears undo.

## Selection groups

Selection groups only filter which tiles the champion and group panels show; they never
affect the tree or the draft. They load at startup from `config/selectiongroups.json` and
appear as toggle buttons above the search bar. By default they filter only champions; the
**RGroup filter** toggle (Ctrl+E) makes them filter representational groups too.

```json
{ "groups": [ { "name": "Support", "champions": [16, 26, 7, 190], "note": "..." } ] }
```

`champions` may list champion values and group values (presets 2-15, user groups 190-255),
so a filter can show representational group tiles too. Groups are identified by their
position in the file, so names may repeat. With several filters on, the **Union** /
**Intersection** button in the champions panel (shown under Help → Button settings) or
Ctrl + Q decides how they combine (union after every start).
The search box narrows the result further. The default file has rough Top / Jungle / Mid /
ADC / Support lists meant to be reviewed and edited.

## Editing groups in the app

**Edit** next to *Details* (representational groups) and next to *Clear* (selection groups)
open simple editors: add, rename, change members, delete, and for selection groups reorder.
**Save** writes the file the groups came from and applies the change at once; **Save as…**
picks another file.

- Representational groups need at least 2 champions. New groups get the next free value
  (G1 = 190 ... G66 = 255). Changing or deleting a group the tree uses asks first, then empties
  only that group's nodes and clears the undo history.
- The Top / Jungle / Mid / ADC / Support selection groups can be edited and moved but not
  renamed or deleted, and no other group may use those names. Missing ones are re-added at
  startup. Members that refer to representational groups which don't exist are kept but not
  shown.
- Every save keeps the previous file as `<name>.bak`. At startup a broken config falls back to
  its `.bak`, then to the shipped copy in `config/defaults/`.
- **Hide presets** (in both areas) hides the built-in role filters and the preset groups Top to
  P15. These choices are stored in `config/settings.json`.

### Copying between the two kinds of group

A group can be copied into the other list; the original stays. The copy is written to the
other config straight away (a notice confirms it) and uses the group as currently shown in
the editor, saved or not.

- **Copy to selection groups**: in the representational group editor, and for any group
  (presets included) in the *Details* list. The selection group gets the group's champions.
- **Copy to representational groups**: in the selection group editor. Representational
  groups can't contain groups, so each group tile is replaced by its champions, or left out
  with **Leave group tiles out**. The copy gets the first free G number (its label follows),
  needs at least 2 champions, and is refused if all 66 numbers are in use.
- Names stay the same unless taken (ignoring case), then get " (1)", " (2)", ..., shortened
  to fit 16 characters. The built-in role filters and the preset group names count as taken.
  Notes are copied; role locks are not.

## Layout

- **Graph direction:** the graph runs **top to bottom** by default: each draft step is a row
  (labels BB1 … RP5 in a column on the left, the step chosen next underlined), branches
  spread to the right in the order they were added, and horizontal lines separate the turns
  (thicker between ban and pick phases). The root starts in the middle of the panel and the
  tree grows around it; the labels stay on the left while the graph scrolls sideways, and
  it scrolls up and down when all steps don't fit. **Help → View → Graph** switches to **Left to right** (the older view, described
  under "Fitting the graph" below) and back; the choice is saved.
- **Phase shading:** behind the graph (and its step labels), the ban phases are shaded
  red and the pick phases dark blue, the first ban and pick phase a little stronger than
  the second, so it is always clear which phase a part of the tree belongs to. The colors
  are the `--phase-ban-1`, `--phase-ban-2`, `--phase-pick-1` and `--phase-pick-2` variables
  in `src/style.css` (separately for the dark and light themes).
- **Zoom:** Ctrl + mouse wheel over the graph (see "Working with large trees").
- **Font:** the app and every export use **Lexend Deca**, a typeface designed to be easy to
  read, also with dyslexia. It is bundled (`assets/fonts/`, SIL Open Font License), so it
  works offline; SVG exports embed it. Symbols it lacks (★, ●, arrows) come from DejaVu
  Sans in exports.
- **Narrow windows:** below 1700 px the tile size slider loses its labels and the top bar
  buttons get smaller to leave room for the tabs; on very narrow windows the buttons wrap
  to a second row.
- **Top-to-bottom arrangement:** with the graph top to bottom, the middle column is arranged
  like fullscreen: champions top left, the representational groups as a column on the right
  (buttons at the bottom), and the graph across the whole width below. Until you drag the
  splitter above the graph, the top row sizes itself to the window: four whole rows of
  champion tiles, or fewer whole rows when the window is too low to leave the graph area
  (below the draft tree's header) at least 200 px, or 30% of the middle column on taller
  windows. It follows window and tile size changes; double-clicking that splitter goes back
  to this automatic size. Champion and group tiles are always shown in whole rows (the rest
  scroll), so no tile is cut in half. Its splitters, and the team panels' width, have their
  own sizes, so each direction keeps its own layout.
- **Resizing panels:** drag the gaps between panels to resize them. That works for the two
  team panels (they resize together as a mirrored pair, 160–560 px) and for the champion
  pool, groups area and graph in the middle. Double-click a gap to reset the sizes it
  controls. On a narrow window the team panels give way so the middle column keeps at least
  760 px.
- **Short windows:** the team panels never let the pick slots spill over the role slots:
  the pick slots get a little smaller on short windows, and past that the panel scrolls.
- **Small windows:** the layout is made for windows of at least 1024 × 600. The window can
  still be made smaller, but then the app keeps that size and the window scrolls instead of
  squashing panels together. On short windows the draft tree's header buttons stay on one
  row and scroll sideways (mouse wheel over them), the graph keeps at least about 120 px,
  and the middle column scrolls rather than cutting champion or group tiles in half.
  Popups and floating panels (export, notices) open below the top bar, even when it wraps
  to two rows.
- **Panel sizes (left to right):** until you drag the splitters between champions, groups
  and graph, the champions get up to four whole rows of tiles and the groups one whole row
  (two when there is room and enough groups), leaving the graph area (below the draft
  tree's header) at least 200 px, or 30% of the middle column on taller windows; the
  sizes follow the window and the tile size. Double-clicking either splitter goes back to
  this. Champion and group tiles always show in whole rows. The root starts halfway down
  the graph.
- **Fitting the graph (left to right):** when the graph's 20 columns don't fit the middle
  column, the graph scales itself down, so every step is visible without scrolling
  sideways. The step headers (BB1, RB1, … RP5: team, ban/pick, number) stay at the top
  while the graph scrolls, and the step chosen next is underlined.
- **Hiding panels:** the ▾ in a panel's top-right corner, or **View ▾** in the Help panel,
  hides a panel; it collapses to a bar that brings it back when clicked. The two team panels
  hide and show together.
- **Animations:** **Help → View → Animations** picks **Smooth** (the default) or **Low spec**
  (the app as it was before animations: only a few quick hover effects, for slow
  computers). Smooth adds, using only cheap effects (opacity, scaling and moving):
  - in the graph, a new node pops in, nodes glide to their new places when an edit moves
    them (lines follow where the webview can move them, and fade back in elsewhere), the
    selected node pops, the path to it draws itself from the start down, a Find jump
    flashes, follow mode fades lines in and out, zooming eases to the new size, and the
    view glides to a selected node off screen;
  - menus, popups, the Help panel and previews fade and grow in; buttons change color
    smoothly and press in; new tabs slide in; ruled-out tiles fade; a pick settles into its
    slot, and the slot to fill next glows softly; a highlight slides along the bottom step
    bar; notices slide out; a theme change fades between colors; a panel shown again fades
    in.
  The setting is saved and shared by every window.
- **Easter eggs:** **View → Easter eggs** turns all easter eggs on or off (on by default,
  remembered in `config/settings.json`). For developers: every egg is wrapped in
  `if easter::on() { … }`; CSS eggs go under `.eggs-on`, and webview scripts check
  `window.__recvrveEggs` (see `src/easter.rs`).
- **Fullscreen:** the **⛶** button (far left of the top bar) or **F11** fills the screen with
  the champions, the representational groups (as a column on the right, buttons at the
  bottom) and the graph across the whole width below. The top bar, team panels and order
  strip are hidden, and so are panels you hid yourself. Its splitters have their own sizes.
  The graph keeps the direction chosen under **View → Graph**; fullscreen is otherwise the
  same in both.
  **Esc** leaves it (after closing any open popup first).
- **Draft tree header:** **Delete** (the selected action and everything after it) and
  **Reset** (the whole tree) sit next to the "Draft tree" title, when shown under
  **Help → Button settings** (hidden by default).
- **Panel headers:** the buttons of the champions and groups panels keep their places: the
  selection filters wrap into at most two rows on the left (then scroll), and RGroup filter /
  Union / Auto-tag sit above Edit / Clear / Hide presets on the right (on a narrow
  champions panel they move below the filters, so the filters always have room). Buttons
  hidden under **Help → Button settings** leave the panel; the others keep their order. The
  draft tree's shown buttons are always there and greyed out when they don't apply.
- **Tile size:** the **Tile size** slider in the top bar sets the champion tile size
  (40–128 px), and group tiles scale with it. Double-click the slider to reset it. Tiles
  show no names; hover a tile for its name, and search finds groups by name or label
  (e.g. `G2`, `TOP`).
- **No accidental selection:** dragging over tiles, nodes or labels doesn't select them
  (no blue highlight), and images can't be dragged out. Text fields stay selectable.
- **Saving:** sizes, hidden panels, the tile size, the graph direction, the zoom, the
  Previews / Tooltips toggles and the button settings are
  saved in `config/settings.json`.

The window title shows the version from `version` in `Cargo.toml` ("Recvrve 0.2.1 Beta"),
and the window uses the app icon. The icon's source is `assets/icon/recvrve.svg`; the window
icon `recvrve-256.png` and the Windows icon `recvrve.ico` (16–256 px, used for the executable,
its shortcuts, saved files and the installer) are rendered from it.

## Role tags

Every pick can be tagged with the roles it can play: **Top, Jungle, Mid, Bot and Support**.
That includes champions, representational groups and empty picks. Untagged picks count as
any role.

**Tagging a pick:**

- **Middle-click** a pick in the graph or in a side panel to open the tag popup. Everything
  changed in one popup visit is a single undo step.
- **Alt + 1 … 5** toggles Top / Jungle / Mid / Bot / Support on the selected pick.
- **Auto-tag** in the filter row tags new picks with the roles of the active Top / Jungle /
  Mid / ADC / Support filters.
- **Role lock:** the representational group editor can lock a group to a set of roles, for
  example top only or top/jungle. Its nodes start with those tags and can't get others.

**Icons.** Roles are shown with Riot's position icons from `assets/roles/*.svg`, embedded
at build time. They are drawn in the text color, with the rest of the map dimmed. A
replacement file only needs a `viewBox` and `<path>` elements (absolute or relative
`M L H V C Z` commands); paths with `fill-opacity` below 1 are drawn dimmed.

**The rule.** On each line, a team's tagged picks must fit into different roles: there must
be at least one way to give each one a different role it is tagged with. So a top-only pick
blocks other top-only picks, while top/jungle and top/mid can both be on the team. Tags that
would break a line (including lines further down the tree) are greyed out.

**Role slots.** At the bottom of each team panel, one slot per role shows the picks that can
play it on the selected line. Untagged picks count as any role. A pick forced into one role
fills that slot alone. When several picks could play a role, the slot is shared between up
to five of them: two split along a diagonal, three to five as pie wedges from the centre.

**Exports:**

- The tree image and the state cards show the tags as a row of small icons along the bottom
  of each tile. Exports draw the icons as vector shapes, so they stay sharp in SVG and PDF.
- Cards (and the PDF) get the role slots below the team panels.
- The XML gets `roles="top,jungle"` on tagged nodes.

Save files store tags as format version 3; older saves still load.

## Counter picks

Each champion pick in the team panels shows, beside its icon, the three champions that
beat it most in its lane, as small tiles with the pick's win rate against each (hover a
tile for the games played and where the numbers come from). Hovering a pick's or ban's
icon shows the champion's name.

- The numbers are OP.GG's ranked solo queue statistics for Master tier and above, on the
  current patch. A matchup needs at least 100 games to show (OP.GG names none under that).
- The lane is the pick's role tag; with several tags (or none), the one of them the
  champion plays most.
- Counters the other team can't pick are left out: banned champions, the pick's own team's
  picks and champions fearless locks for the other team. The other team's picks stay in.
  So a pick can show fewer than three, or none.
- Groups and empty picks keep their name instead.

The data built into the app is from the patch it was released on. **View ▾ → Update
counter data** (in the Help panel) downloads the current patch from OP.GG, which takes
about a minute. Each download is kept as `config/counters/<patch>.json`, and the app uses
the newest one. The update only reads OP.GG's public statistics over HTTPS; the app sends
nothing about you or your drafts.

To refresh the built-in data before a release, run
`cargo test --release counters::tests::refresh_bundled -- --ignored`, which rewrites
`assets/counters.json`.

## Themes

The theme menu (◐, in the Help panel next to **View ▾**) lists **Dark**, **Light** and every theme found in
`config/themes/`. It re-reads the folder each time it opens, so new themes show up without a
restart. The selected theme is saved in `config/settings.json` as `"theme": "<folder name>"`,
and you can also set it there by hand.

**Adding a theme.** A theme is a folder holding a `theme.json` and, optionally, a background
picture (png, jpg, webp, gif or bmp) next to it, for example:

```json
{
  "name": "My Theme",
  "base": "dark",
  "background": "background.jpg",
  "fit": "cover",
  "dim": 30,
  "panel_opacity": 72,
  "blur": 10,
  "colors": { "bg": "#121735", "panel": "#161c40", "panel2": "#222a58", "border": "#36407a",
              "text": "#eef0ff", "muted": "#a3abd6", "accent": "#f6a26b" },
  "group_colors": ["#f6a26b", "#5fd3c2", "#f7d774"],
  "graph": { "edge": "#59639c", "edge_active": "#f6a26b", "turn_line": "#2c3466",
             "turn_width": 1, "phase_line": "#4b5596", "phase_width": 3 }
}
```

**The fields:**

| Field | Values | What it does |
|---|---|---|
| `name` | text | Shown in the menu. |
| `base` | `dark` or `light` | Sets the team colors, and every value the theme leaves out. |
| `background` | file name | A picture in the theme folder. |
| `fit` | `cover`, `contain`, `tile` or `center` | How the picture fills the window. |
| `dim` | 0–90 % | Lays the background color over the picture so text stays readable. |
| `panel_opacity` | 20–100 % | How opaque the panels are. |
| `blur` | 0–20 px | Blur behind the panels. A blurred copy of the picture is made once and kept in the theme's `.cache/` folder (rebuilt when the picture, fit or blur changes), so scrolling doesn't blur the screen again on every frame. Later starts find it through a small index there and show it from the first frame; while it is first made, the panels stay plain. |
| `colors` | `#rrggbb` per key | Interface colors. `accent` replaces the gold highlight. |
| `group_colors` | list of `#rrggbb` | Representational group colors, handed out in order. Groups beyond the list get generated colors. |
| `graph` | `edge`, `edge_active`, `turn_line`, `turn_width`, `phase_line`, `phase_width` | Branch lines, the selected path, the thin lines between turns and the thicker lines between ban and pick phases. |

Every field except `name` can be left out.

**Rules:**

- Team blue and red and the fearless colors can't be changed.
- Group colors in those shades are left out.
- Mistakes are listed as warnings (⚠ in the menu) instead of breaking the theme.
- If `dark` or `light` go missing they are written again from the built-in defaults.

**Theme settings…** in the menu edits the selected theme with a live preview. **Save** writes
its `theme.json`, keeping a `.bak` of the previous version. **Save as new…** copies it, with
its picture, into a new theme folder. Exports keep their own white print style whatever the
theme.

## Fearless draft

**Fearless** in the top bar or Ctrl+G opens the fearless
editor. While a series is active, an indicator in the bottom right corner (on the draft
order strip, opposite the patch field) shows the game and the number of locked champions,
e.g. `G3 · 14`; clicking it opens the editor too. The Fearless button, when shown, carries
the same indicator.

- **Game:** G1–G7 picks which game of the series is being prepared. Each earlier game gets a
  row of ten slots: five for the team on blue side in *this* game and five for the red-side
  team. Click a slot to clear it and fill it next, then click champions in the picker. Rows
  may stay incomplete. Rows for later games are kept when you switch to an earlier game.
- **Hard fearless** (default): champions from earlier games can't be picked or banned by
  either team.
- **Soft fearless:** a team can't pick its own earlier champions again. Bans are not
  affected.
- **Groups:** fearless champions use up group slots like champions already on the line, so a
  group with no members left can't be chosen.
- **Undo:** Apply is one undo step.

In the champion grid, locked champions are shown in purple (hard), or in blue or red (soft:
the side that can't pick them any more), with `F1`…`F6` for the game they were picked in.

**Existing trees.** Branches that the bans rule out stay in the tree but become
*unreachable*. Their lines glow purple (or blue/red for soft fearless) and their icons turn
purple. The tree panel header shows their count and four buttons:

- **Hide unreachable** (Ctrl + Alt + H) hides the branches below each topmost unreachable
  node, the way Ctrl + H hides a node's branches: that node stays, showing how many nodes
  it hides, and **Show all** or Ctrl + H on it brings them back. Pressing it again shows
  them all. Exports leave the hidden branches out while **Leave hidden branches out** is
  ticked.
- **Edit unreachable** allows adding new branches below unreachable nodes (off by default).
- **Remove unreachable** (Ctrl + Alt + U) deletes them. Ctrl+Z undoes it.
- **Restore unreachable** (Ctrl + Shift + U) puts the last removed branches back in their
  old places, even after other edits.

Exports put the fearless bans into the legend: the tree image's legend band, the separate
legend image, a PDF page and a line on the PDF title page, and
`<fearless>` (with `<blue>`/`<red>` for soft fearless) in the XML. They are not shown on every
card.

## Exports

**Export ▾** in the top bar opens an options panel on the right. The panel doesn't block the
window, so you can keep clicking nodes in the tree while it is open. Exports are written to
`exports/` by default. They are output only and can't be loaded back into the app. Every
export uses the light theme on a white background.

File names start with the team names when they are set (`G2-vs-Fnatic-tree-…`), otherwise
with `recvrve`; the names themselves don't appear inside exports.

Each export can cover the **whole tree** or the **selected subtree**. A subtree export also
includes the single line that leads to the selected node. With branches hidden in the
graph (Ctrl + H), **Leave hidden branches out** exports those nodes without what is below
them (off by default: hiding doesn't change exports).

- **Tree (PNG, SVG, XML)**
  - **PNG and SVG** draw the tree as it looks in the tree view, with a legend of the groups
    used above it. Nodes on a marked line get a gold star, as in the app.
  - **Orientation:** **Portrait** draws the tree top to bottom (steps as rows, labels on the
    left), **Landscape** left to right (steps as columns, labels on top). It starts at the
    direction the graph has in the app. The image is as wide as the tree needs either way.
  - **Highlighted lines:** select a node in the tree and press "Add selected node". The line
    leading to it is drawn in color, and lines shared by several highlights show every color.
  - **Size:** the panel shows the image size. It recommends SVG for trees with more than 400
    lines, because SVG stays sharp and small. Each champion portrait is embedded in the SVG
    only once.
  - **XML** writes the tree as nested elements named after draft positions (`bb1`, `rb1` …
    `rp5`). Each element has the node's `value` and a readable `name`, and a `<legend>`
    describes every group used. Nodes on a marked line have `marked="true"`. Values are the app's own values: 0 empty, 1 no ban, 2–15
    preset groups, 16–189 champions and 190–255 user groups.
- **Draft state images:** one portrait card per draft state at the chosen positions. A card
  shows both team panels side by side, and a ★ in its header when the state is on a marked
  line.
  - **Choosing positions:** pick any of the 20 positions, or use the "End of ban phase 1",
    "End of pick phase 1", "End of ban phase 2" and "Complete drafts" shortcuts. **Select
    all phases** chooses every position that has draft states, in draft order (BB1 … RP5),
    and **Clear selection** chooses none; both also work in the PDF panel. Lines that end
    before a chosen position are skipped.
  - **File names:** files go into a new folder with one subfolder per position, named like
    `A06-RB3/A06-RB3_0014.webp`. `A06` is the 6th action (red ban 3), and `0014` is the 14th
    state at that position, counted top to bottom in the tree view.
  - **Options:**
    - WebP lossless (default), PNG or JPEG;
    - card width of 600, 900 or 1200 px;
    - a separate `00_legend` image describing the groups;
    - also packing everything into a `.zip`.
  - **Size estimate:** the panel shows the number of files and their estimated total size
    before you export.
- **Draft states PDF:** A4 pages with 12 mm margins, portrait by default. Cards are drawn as
  vector graphics, and each portrait is stored in the file only once.
  - **Sections:** each chosen position becomes a section that starts on a new page. Sections
    can be reordered, and the states inside a section follow tree order.
  - **Layout:** one card per page, or several per page in an adjustable grid (2 × 3 in
    portrait, 3 × 2 in landscape).
  - **Optional parts:** page numbers, bookmarks and a group legend page are on by default.
    A title page is off by default.

Exports run in the background with a progress bar and a Cancel button. A cancelled image
export removes the files it had already written.
