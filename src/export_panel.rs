//! The export options panel. It floats over the right side of the window without blocking
//! the rest of the app, so nodes can still be selected in the tree while it is open (for
//! the subtree scope and for highlighted lines).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;

use crate::draft::{DRAFT_ORDER, Team};
use crate::export::pdf::{self, PdfOptions};
use crate::export::raster::{self, ImageFormat};
use crate::export::states::{self, ImagesOptions};
use crate::export::{self, Job, Request, Snapshot, TreeFormat, TreeOptions, model};
use crate::tree::{DraftTree, NodeId};
use crate::value_view::value_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Tree,
    Images,
    Pdf,
}

impl ExportKind {
    pub const ALL: [ExportKind; 3] = [ExportKind::Tree, ExportKind::Images, ExportKind::Pdf];

    pub fn menu_label(self) -> &'static str {
        match self {
            ExportKind::Tree => "Tree (PNG, SVG, XML)…",
            ExportKind::Images => "Draft state images…",
            ExportKind::Pdf => "Draft states PDF…",
        }
    }

    fn title(self) -> &'static str {
        match self {
            ExportKind::Tree => "Export tree",
            ExportKind::Images => "Export draft state images",
            ExportKind::Pdf => "Export draft states as PDF",
        }
    }
}

/// Draft depths grouped by phase, for the depth picker.
const PHASES: [(&str, std::ops::RangeInclusive<usize>); 4] = [
    ("Ban phase 1", 1..=6),
    ("Pick phase 1", 7..=12),
    ("Ban phase 2", 13..=16),
    ("Pick phase 2", 17..=20),
];

/// Quick picks: the last action of each phase.
const PHASE_ENDS: [(&str, usize); 4] = [
    ("End of ban phase 1", 6),
    ("End of pick phase 1", 12),
    ("End of ban phase 2", 16),
    ("Complete drafts", 20),
];

/// Short label of a node for lists: `A09-RP2 Jinx`.
fn node_label(tree: &DraftTree, id: NodeId) -> String {
    let Some(node) = tree.get(id) else {
        return String::new();
    };
    let depth = node.pos().depth();
    if depth == 0 {
        return "Start (empty draft)".into();
    }
    format!(
        "{} {}",
        model::depth_code(depth),
        value_name(tree.groups(), node.value())
    )
}

/// A snapshot that always counts as changed, so the memo never compares whole trees.
#[derive(Clone)]
struct Fresh(Arc<Snapshot>);

impl PartialEq for Fresh {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

/// Where a running or finished export stands.
#[derive(Clone, Debug, PartialEq)]
enum Status {
    Idle,
    Running { done: usize, total: usize },
    Done(String),
    Failed(String),
}

#[component]
pub fn ExportPanel(
    kind: ExportKind,
    tree: Signal<DraftTree>,
    cursor: Signal<NodeId>,
    colors: Signal<HashMap<u8, usize>>,
    /// Set while an export runs, so the panel can't be closed under it.
    busy: Signal<bool>,
    /// The graph runs top to bottom: the tree image starts in portrait.
    vertical: bool,
    on_close: EventHandler<()>,
) -> Element {
    // Shared options.
    let mut subtree = use_signal(|| false);
    // Include conflicting branches (left out by default).
    let mut show_conflicts = use_signal(|| false);
    // Leave out the branches hidden in the graph (Ctrl+H).
    let mut leave_out_hidden = use_signal(|| false);
    let mut status = use_signal(|| Status::Idle);
    let mut job = use_signal(|| None::<Job>);
    let mut dialog_open = use_signal(|| false);
    // Tree export.
    let mut tree_format = use_signal(|| TreeFormat::Png);
    let mut png_scale = use_signal(|| 2.0f32);
    let mut legend = use_signal(|| true);
    // Portrait draws the tree top to bottom, landscape left to right.
    let mut tree_portrait = use_signal(|| vertical);
    let mut highlights = use_signal(Vec::<NodeId>::new);
    // State images and PDF: chosen depths, in section order.
    let mut depths = use_signal(Vec::<usize>::new);
    let mut image_format = use_signal(|| ImageFormat::WebP);
    let mut width_px = use_signal(|| 900u32);
    let mut zip = use_signal(|| false);
    let mut folder_parent = use_signal(export::exports_dir);
    // Export names start with the teams' names when they are set.
    let prefix = move || crate::save::match_stem(&tree.peek()).unwrap_or_else(|| "recvrve".into());
    let mut folder_name = use_signal(|| format!("{}-states-{}", prefix(), export::file_stamp()));
    let mut packed = use_signal(|| true);
    let mut landscape = use_signal(|| false);
    let mut grid = use_signal(|| pdf::default_grid(false));
    let mut page_numbers = use_signal(|| true);
    let mut bookmarks = use_signal(|| true);
    let mut title_on = use_signal(|| false);
    let mut title_text = use_signal(|| "Draft prep".to_string());

    // Drop highlighted nodes that were deleted from the tree.
    use_effect(move || {
        let t = tree.read();
        if highlights.peek().iter().any(|&id| !t.contains(id)) {
            highlights.write().retain(|&id| t.contains(id));
        }
    });

    let scope_node = move || if subtree() { Some(cursor()) } else { None };
    let view = use_context::<crate::value_view::ViewContext>();
    let snapshot = use_memo(move || {
        let t = tree.read();
        let pruned;
        let source: &crate::tree::DraftTree = if leave_out_hidden() && t.any_collapsed() {
            pruned = t.without_hidden();
            &pruned
        } else {
            &t
        };
        let mut snap = Snapshot::build(
            source,
            scope_node(),
            &colors.read(),
            &highlights.read(),
            false,
            show_conflicts(),
        );
        snap.group_colors = view.theme.read().group_colors.clone();
        Fresh(Arc::new(snap))
    });

    let running = matches!(status(), Status::Running { .. });
    let t = tree.read();
    let conflict_count = t.conflicts().len();
    let any_hidden = t.any_collapsed();
    let cursor_label = node_label(&t, cursor());
    let at_root = cursor() == t.root();
    let snap = snapshot.read().0.clone();
    let counts: Vec<usize> = (0..=DRAFT_ORDER.len())
        .map(|d| if d == 0 { 0 } else { snap.at_depth(d).len() })
        .collect();
    let chosen = depths();
    let state_total: usize = chosen.iter().map(|&d| counts[d]).sum();
    // "Select all phases": every position that has draft states, in draft order.
    let with_states: Vec<usize> = (1..=DRAFT_ORDER.len()).filter(|&d| counts[d] > 0).collect();

    let pdf_options = move || PdfOptions {
        sections: depths(),
        grid: if packed() { grid() } else { (1, 1) },
        landscape: landscape(),
        page_numbers: page_numbers(),
        bookmarks: bookmarks(),
        legend_page: legend(),
        title_page: title_on().then_some(title_text()),
    };

    // Summary line and warnings for the current options.
    let mut warning = None::<String>;
    let mut blocked = false;
    let info = match kind {
        ExportKind::Tree => {
            let lines = snap.lines();
            let nodes = snap.nodes.len() - 1;
            let mut info = format!(
                "{lines} line{} · {nodes} node{}",
                plural(lines),
                plural(nodes)
            );
            match tree_format() {
                TreeFormat::Xml => {}
                format => {
                    let (w, h) = export::tree_image_size(&snap, legend(), tree_portrait());
                    if format == TreeFormat::Png {
                        let (pw, ph) = raster::pixel_size(w, h, png_scale());
                        info.push_str(&format!(" · {pw} × {ph} px"));
                        if !raster::fits(pw, ph) {
                            blocked = true;
                            warning = Some(
                                "Too large for PNG at this scale. Use SVG, a smaller scale or a subtree."
                                    .into(),
                            );
                        } else if lines > export::SVG_SUGGESTED_LINES {
                            warning = Some(format!(
                                "Over {} lines: SVG is recommended. It stays sharp at any zoom and is much smaller.",
                                export::SVG_SUGGESTED_LINES
                            ));
                        }
                    } else {
                        info.push_str(&format!(" · {:.0} × {:.0} (vector)", w, h));
                    }
                }
            }
            if nodes == 0 {
                blocked = true;
            }
            info
        }
        ExportKind::Images => {
            let files = state_total + usize::from(legend() && state_total > 0);
            let bytes = states::estimate_bytes(state_total, image_format(), width_px());
            if state_total == 0 {
                blocked = true;
            }
            format!(
                "{files} image{} · about {}",
                plural(files),
                export::format_bytes(bytes)
            )
        }
        ExportKind::Pdf => {
            if state_total == 0 {
                blocked = true;
            }
            let pages = pdf::page_count(&snap, &pdf_options());
            format!(
                "{state_total} state{} · {pages} page{}",
                plural(state_total),
                plural(pages)
            )
        }
    };

    // Runs a job and follows its progress until it finishes.
    let mut run = move |request: Request| {
        let started = export::start(request);
        job.set(Some(started.clone()));
        busy.set(true);
        status.set(Status::Running { done: 0, total: 0 });
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Some(result) = started.take_result() {
                    status.set(match result {
                        Ok(outcome) => Status::Done(outcome.message),
                        Err(e) => Status::Failed(e),
                    });
                    break;
                }
                status.set(Status::Running {
                    done: started.progress.done(),
                    total: started.progress.total(),
                });
            }
            job.set(None);
            busy.set(false);
        });
    };

    let start_export = move |_| {
        // The snapshot is taken now, so edits made while the export runs don't affect it.
        let snap = (*snapshot.peek().0).clone();
        let stamp = export::file_stamp();
        match kind {
            ExportKind::Images => {
                let folder = states::unique_folder(&folder_parent().join(folder_name().trim()));
                run(Request::Images(
                    snap,
                    ImagesOptions {
                        depths: depths(),
                        format: image_format(),
                        width_px: width_px(),
                        legend: legend(),
                        zip: zip(),
                        folder,
                    },
                ));
                folder_name.set(format!("{}-states-{}", prefix(), export::file_stamp()));
            }
            ExportKind::Tree | ExportKind::Pdf => {
                let (ext, label, name) = match kind {
                    ExportKind::Tree => {
                        let f = tree_format();
                        (
                            f.extension(),
                            f.label(),
                            format!("{}-tree-{stamp}", prefix()),
                        )
                    }
                    _ => ("pdf", "PDF", format!("{}-states-{stamp}", prefix())),
                };
                dialog_open.set(true);
                let options = pdf_options();
                spawn(async move {
                    let dir = export::exports_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let picked = rfd::AsyncFileDialog::new()
                        .set_title("Export")
                        .set_directory(&dir)
                        .set_file_name(format!("{name}.{ext}"))
                        .add_filter(label, &[ext, &ext.to_uppercase()])
                        .save_file()
                        .await;
                    dialog_open.set(false);
                    let Some(handle) = picked else {
                        return;
                    };
                    let mut path = handle.path().to_path_buf();
                    if !path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
                    {
                        let mut file = path.file_name().unwrap_or_default().to_os_string();
                        file.push(format!(".{ext}"));
                        path.set_file_name(file);
                    }
                    let request = match kind {
                        ExportKind::Tree => Request::Tree(
                            snap,
                            TreeOptions {
                                format: tree_format(),
                                scale: png_scale(),
                                legend: legend(),
                                portrait: tree_portrait(),
                                path,
                            },
                        ),
                        _ => Request::Pdf(snap, options, path),
                    };
                    run(request);
                });
            }
        }
    };

    let mut toggle_depth = move |d: usize| {
        let mut list = depths.write();
        if let Some(i) = list.iter().position(|&x| x == d) {
            list.remove(i);
        } else {
            // New sections go in draft order after any reordering.
            let at = list.iter().position(|&x| x > d).unwrap_or(list.len());
            list.insert(at, d);
        }
    };

    rsx! {
        div { class: "export-panel",
            div { class: "export-head",
                h2 { class: "export-title", "{kind.title()}" }
                button {
                    class: "modal-close",
                    title: if running { "Cancel the export first" } else { "Close (Esc)" },
                    disabled: running,
                    onclick: move |_| on_close.call(()),
                    "×"
                }
            }

            // Scope
            div { class: "export-section",
                div { class: "export-label", "Scope" }
                div { class: "seg",
                    button {
                        class: if !subtree() { "seg-btn active" } else { "seg-btn" },
                        onclick: move |_| subtree.set(false),
                        "Whole tree"
                    }
                    button {
                        class: if subtree() { "seg-btn active" } else { "seg-btn" },
                        onclick: move |_| subtree.set(true),
                        "Selected subtree"
                    }
                }
                if any_hidden {
                    label { class: "export-check",
                        input {
                            r#type: "checkbox",
                            checked: leave_out_hidden(),
                            onchange: move |e| leave_out_hidden.set(e.checked()),
                        }
                        "Leave hidden branches out"
                    }
                    p { class: "export-hint",
                        if leave_out_hidden() {
                            "Nodes whose branches are hidden in the graph (Ctrl+H) are exported without them."
                        } else {
                            "Hidden branches (Ctrl+H) are exported like the rest."
                        }
                    }
                }
                if conflict_count > 0 {
                    label { class: "export-check",
                        input {
                            r#type: "checkbox",
                            checked: show_conflicts(),
                            onchange: move |e| show_conflicts.set(e.checked()),
                        }
                        "Show conflicting branches ({conflict_count} node(s))"
                    }
                    p { class: "export-hint",
                        if show_conflicts() {
                            "Conflicting branches are drawn faded with green lines, and marked conflict in cards and XML."
                        } else {
                            "Conflicting branches (after a value edit higher up) are left out."
                        }
                    }
                }
                if subtree() {
                    p { class: "export-hint",
                        if at_root {
                            "The start node is selected, so this is the whole tree. Click a node in the tree to export from there."
                        } else {
                            "From {cursor_label} (and the line leading to it). Click another node in the tree to change it."
                        }
                    }
                }
            }

            match kind {
                ExportKind::Tree => rsx! {
                    div { class: "export-section",
                        div { class: "export-label", "Format" }
                        div { class: "seg",
                            for f in TreeFormat::ALL {
                                button {
                                    key: "{f.label()}",
                                    class: if tree_format() == f { "seg-btn active" } else { "seg-btn" },
                                    onclick: move |_| tree_format.set(f),
                                    "{f.label()}"
                                }
                            }
                        }
                        p { class: "export-hint",
                            match tree_format() {
                                TreeFormat::Png => "A picture that opens anywhere.",
                                TreeFormat::Svg => "Vector image: sharp at any zoom and small. Opens in web browsers.",
                                TreeFormat::Xml => "The tree as data: nested elements named after draft positions (bb1 … rp5), with a legend of the groups used.",
                            }
                        }
                    }
                    if tree_format() == TreeFormat::Png {
                        div { class: "export-section",
                            div { class: "export-label", "Resolution" }
                            div { class: "seg",
                                for s in [1.0f32, 2.0, 3.0] {
                                    button {
                                        key: "{s}",
                                        class: if png_scale() == s { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| png_scale.set(s),
                                        "{s}×"
                                    }
                                }
                            }
                        }
                    }
                    if tree_format() != TreeFormat::Xml {
                        div { class: "export-section",
                            div { class: "export-label", "Orientation" }
                            div { class: "seg",
                                for (label, value) in [("Portrait", true), ("Landscape", false)] {
                                    button {
                                        key: "{label}",
                                        class: if tree_portrait() == value { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| tree_portrait.set(value),
                                        "{label}"
                                    }
                                }
                            }
                            p { class: "export-hint",
                                if tree_portrait() {
                                    "Top to bottom: draft steps as rows, branches spread sideways."
                                } else {
                                    "Left to right: draft steps as columns, branches spread downwards."
                                }
                            }
                        }
                        div { class: "export-section",
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: legend(),
                                    onchange: move |e| legend.set(e.checked()),
                                }
                                "Group legend above the tree"
                            }
                        }
                        div { class: "export-section",
                            div { class: "export-label", "Highlighted lines" }
                            p { class: "export-hint", "The lines leading to these nodes are drawn in color." }
                            for (i, id) in highlights().into_iter().enumerate() {
                                div { key: "{id:?}", class: "export-row",
                                    span {
                                        class: "export-swatch",
                                        style: "background: {model::HIGHLIGHT_COLORS[i % model::HIGHLIGHT_COLORS.len()]};",
                                    }
                                    span { class: "export-row-label", "{node_label(&t, id)}" }
                                    button {
                                        class: "export-remove",
                                        title: "Remove",
                                        onclick: move |_| highlights.write().retain(|&h| h != id),
                                        "×"
                                    }
                                }
                            }
                            div { class: "export-actions",
                                button {
                                    class: "btn small",
                                    disabled: highlights().contains(&cursor()),
                                    title: "Highlight the line leading to the node selected in the tree",
                                    onclick: move |_| {
                                        let id = cursor();
                                        if !highlights.peek().contains(&id) {
                                            highlights.write().push(id);
                                        }
                                    },
                                    "Add selected node"
                                }
                                if !highlights().is_empty() {
                                    button {
                                        class: "btn small",
                                        onclick: move |_| highlights.write().clear(),
                                        "Clear"
                                    }
                                }
                            }
                        }
                    }
                },
                ExportKind::Images | ExportKind::Pdf => rsx! {
                    div { class: "export-section",
                        div { class: "export-label", "Draft states after" }
                        div { class: "export-actions",
                            for (label, d) in PHASE_ENDS {
                                button {
                                    key: "{d}",
                                    class: if chosen.contains(&d) { "btn small active" } else { "btn small" },
                                    title: "{model::depth_title(d)}",
                                    onclick: move |_| toggle_depth(d),
                                    "{label}"
                                }
                            }
                        }
                        for (phase, range) in PHASES {
                            div { key: "{phase}", class: "depth-row",
                                span { class: "depth-phase", "{phase}" }
                                for d in range {
                                    {
                                        let step = DRAFT_ORDER[d - 1];
                                        let team = match step.team { Team::Blue => "blue", Team::Red => "red" };
                                        let on = if chosen.contains(&d) { "on" } else { "" };
                                        let empty = if counts[d] == 0 { "none" } else { "" };
                                        rsx! {
                                            button {
                                                key: "{d}",
                                                class: "depth-chip {team} {on} {empty}",
                                                title: "{model::depth_title(d)}: {counts[d]} state(s)",
                                                onclick: move |_| toggle_depth(d),
                                                "{step.code()}"
                                                span { class: "depth-count", "{counts[d]}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "export-actions",
                            {
                                let all = with_states.clone();
                                rsx! {
                                    button {
                                        class: "btn small",
                                        disabled: with_states.is_empty() || chosen == with_states,
                                        title: "Choose every position that has draft states, in draft order (BB1 … RP5)",
                                        onclick: move |_| depths.set(all.clone()),
                                        "Select all phases"
                                    }
                                }
                            }
                            button {
                                class: "btn small",
                                disabled: chosen.is_empty(),
                                title: "Choose no positions",
                                onclick: move |_| depths.write().clear(),
                                "Clear selection"
                            }
                        }
                    }
                    if kind == ExportKind::Images {
                        div { class: "export-section",
                            div { class: "export-label", "Format" }
                            div { class: "seg",
                                for f in ImageFormat::ALL {
                                    button {
                                        key: "{f.label()}",
                                        class: if image_format() == f { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| image_format.set(f),
                                        "{f.label()}"
                                    }
                                }
                            }
                            p { class: "export-hint",
                                match image_format() {
                                    ImageFormat::WebP => "Lossless and the smallest lossless option.",
                                    ImageFormat::Png => "Lossless and opens everywhere.",
                                    ImageFormat::Jpeg => "Lossy (quality 90): text edges are slightly soft.",
                                }
                            }
                            div { class: "export-label", "Width" }
                            div { class: "seg",
                                for w in states::WIDTHS {
                                    button {
                                        key: "{w}",
                                        class: if width_px() == w { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| width_px.set(w),
                                        "{w} px"
                                    }
                                }
                            }
                        }
                        div { class: "export-section",
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: legend(),
                                    onchange: move |e| legend.set(e.checked()),
                                }
                                "Group legend as a separate first image"
                            }
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: zip(),
                                    onchange: move |e| zip.set(e.checked()),
                                }
                                "Also pack everything into a .zip"
                            }
                        }
                        div { class: "export-section",
                            div { class: "export-label", "New folder" }
                            input {
                                class: "search export-input",
                                r#type: "text",
                                value: "{folder_name}",
                                oninput: move |e| folder_name.set(e.value()),
                            }
                            div { class: "export-row",
                                span { class: "export-row-label muted", title: "{folder_parent().display()}", "in {folder_parent().display()}" }
                                button {
                                    class: "btn small",
                                    disabled: dialog_open(),
                                    onclick: move |_| {
                                        dialog_open.set(true);
                                        spawn(async move {
                                            let picked = rfd::AsyncFileDialog::new()
                                                .set_title("Export into folder")
                                                .set_directory(folder_parent())
                                                .pick_folder()
                                                .await;
                                            dialog_open.set(false);
                                            if let Some(handle) = picked {
                                                folder_parent.set(handle.path().to_path_buf());
                                            }
                                        });
                                    },
                                    "Change…"
                                }
                            }
                        }
                    } else {
                        if chosen.len() > 1 {
                            div { class: "export-section",
                                div { class: "export-label", "Section order" }
                                for (i, d) in chosen.iter().copied().enumerate() {
                                    div { key: "{d}", class: "export-row",
                                        span { class: "export-row-label", "{model::depth_title(d)} ({counts[d]})" }
                                        button {
                                            class: "export-move",
                                            disabled: i == 0,
                                            title: "Move up",
                                            onclick: move |_| depths.write().swap(i, i - 1),
                                            "▲"
                                        }
                                        button {
                                            class: "export-move",
                                            disabled: i + 1 == chosen.len(),
                                            title: "Move down",
                                            onclick: move |_| depths.write().swap(i, i + 1),
                                            "▼"
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "export-section",
                            div { class: "export-label", "Layout" }
                            div { class: "seg",
                                button {
                                    class: if !packed() { "seg-btn active" } else { "seg-btn" },
                                    onclick: move |_| packed.set(false),
                                    "One per page"
                                }
                                button {
                                    class: if packed() { "seg-btn active" } else { "seg-btn" },
                                    onclick: move |_| packed.set(true),
                                    "Several per page"
                                }
                            }
                            if packed() {
                                div { class: "export-row",
                                    span { class: "grid-label", "Columns" }
                                    div { class: "seg grid-seg",
                                        for c in 1..=4usize {
                                            button {
                                                key: "{c}",
                                                class: if grid().0 == c { "seg-btn active" } else { "seg-btn" },
                                                onclick: move |_| grid.write().0 = c,
                                                "{c}"
                                            }
                                        }
                                    }
                                }
                                div { class: "export-row",
                                    span { class: "grid-label", "Rows" }
                                    div { class: "seg grid-seg",
                                        for r in 1..=5usize {
                                            button {
                                                key: "{r}",
                                                class: if grid().1 == r { "seg-btn active" } else { "seg-btn" },
                                                onclick: move |_| grid.write().1 = r,
                                                "{r}"
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "export-label", "Orientation" }
                            div { class: "seg",
                                for (label, value) in [("Portrait", false), ("Landscape", true)] {
                                    button {
                                        key: "{label}",
                                        class: if landscape() == value { "seg-btn active" } else { "seg-btn" },
                                        onclick: move |_| {
                                            if landscape() != value {
                                                landscape.set(value);
                                                grid.set(pdf::default_grid(value));
                                            }
                                        },
                                        "{label}"
                                    }
                                }
                            }
                            p { class: "export-hint", "A4, 12 mm margins. Each section starts on a new page." }
                        }
                        div { class: "export-section",
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: page_numbers(),
                                    onchange: move |e| page_numbers.set(e.checked()),
                                }
                                "Page numbers"
                            }
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: bookmarks(),
                                    onchange: move |e| bookmarks.set(e.checked()),
                                }
                                "Bookmarks for each section"
                            }
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: legend(),
                                    onchange: move |e| legend.set(e.checked()),
                                }
                                "Group legend page at the end"
                            }
                            label { class: "export-check",
                                input {
                                    r#type: "checkbox",
                                    checked: title_on(),
                                    onchange: move |e| title_on.set(e.checked()),
                                }
                                "Title page"
                            }
                            if title_on() {
                                input {
                                    class: "search export-input",
                                    r#type: "text",
                                    value: "{title_text}",
                                    oninput: move |e| title_text.set(e.value()),
                                }
                            }
                        }
                    }
                },
            }

            div { class: "export-footer",
                p { class: "export-info", "{info}" }
                if let Some(w) = warning {
                    p { class: "export-warning", "{w}" }
                }
                match status() {
                    Status::Running { done, total } => {
                        let pct = if total == 0 { 0.0 } else { done as f64 * 100.0 / total as f64 };
                        rsx! {
                            div { class: "export-progress",
                                div { class: "export-bar", style: "width: {pct:.1}%;" }
                            }
                            div { class: "export-row",
                                span { class: "export-row-label muted",
                                    if total > 2 { "{done} / {total}" } else { "Working…" }
                                }
                                button {
                                    class: "btn small",
                                    onclick: move |_| {
                                        if let Some(j) = job.peek().as_ref() {
                                            j.progress.cancel();
                                        }
                                    },
                                    "Cancel"
                                }
                            }
                        }
                    }
                    Status::Done(message) => rsx! {
                        p { class: "export-done", "{message}" }
                    },
                    Status::Failed(message) => rsx! {
                        p { class: "export-warning", "{message}" }
                    },
                    Status::Idle => rsx! {},
                }
                if !running {
                    button {
                        class: "btn primary export-go",
                        disabled: blocked || dialog_open() || (kind == ExportKind::Images && folder_name().trim().is_empty()),
                        onclick: start_export,
                        if kind == ExportKind::Images { "Export" } else { "Export…" }
                    }
                }
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
