// No console window next to the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use dioxus::desktop::wry::http::Response;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;

// These modules expose API the GUI doesn't use yet (e.g. editing nodes in place).
mod autosave;
mod buttons;
#[allow(dead_code)]
mod champions;
mod clip;
mod configio;
mod counters;
mod draft;
#[allow(dead_code)]
mod drag;
mod easter;
mod editors;
mod export;
mod export_panel;
mod fearless;
mod fearless_editor;
mod find;
mod follow;
#[allow(dead_code)]
mod group_copy;
mod group_import;
mod groups;
mod history;
mod keys;
mod layout;
mod merge;
mod merge_panel;
mod node_preview;
mod role_icon;
mod role_popup;
mod roles;
mod save;
mod selection;
mod shortcuts_panel;
mod tabs;
mod theme;
mod theme_editor;
#[allow(dead_code)]
mod tree;
mod tree_view;
mod ui;
mod value_view;
mod windows;

use champions::{CHAMPIONS, ChampionInfo};
use draft::{Action, DRAFT_ORDER, DraftPos, Team};
use groups::GroupCatalog;
use history::{ChangeKind, History};
use save::SaveFormat;
use slotmap::SecondaryMap;
use tree::{DraftTree, EMPTY, NO_BAN, NodeId, TreeError};
use tree_view::TreeView;
use value_view::{
    RoleGlyph, RoleIcons, ValueIcon, ViewContext, group_color, group_tooltip, value_name,
};

const STYLE: &str = include_str!("style.css");
/// Window title; the version comes from `version` in Cargo.toml.
const WINDOW_TITLE: &str = concat!("Recvrve ", env!("CARGO_PKG_VERSION"), " Beta");
/// The app icon (the window's; on Windows the exe's comes from build.rs).
static ICON_PNG: &[u8] = include_bytes!("../assets/icon/recvrve-256.png");
/// Save files given on the command line (e.g. a double-clicked .dts), opened in tabs.
static STARTUP_FILES: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// The window icon, decoded from the bundled PNG.
fn window_icon() -> Option<dioxus::desktop::tao::window::Icon> {
    let img = image::load_from_memory(ICON_PNG).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    dioxus::desktop::tao::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}
const SPLITTER_TIP: &str = "Drag to resize, double-click to reset";
const ICON_DIR_NAME: &str = "champion_icons";
const ICON_PROTOCOL: &str = "champ";

static ICON_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
/// Icon bytes by champion key, read once: the grid asks for the same icons over and over.
/// Leaked on purpose, as the champion table bounds them and they live for the whole run.
static ICON_CACHE: std::sync::LazyLock<std::sync::Mutex<HashMap<&'static str, &'static [u8]>>> =
    std::sync::LazyLock::new(Default::default);
/// Served files never change under their URL (icons, fonts, versioned pictures).
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Group catalog from `config/reprgroups.json`, plus an error message if it failed to load.
static STARTUP_GROUPS: OnceLock<(Arc<GroupCatalog>, Option<String>)> = OnceLock::new();
/// Selection groups from `config/selectiongroups.json`, loaded once at startup.
static SELECTION_GROUPS: OnceLock<(Vec<selection::SelectionGroup>, Option<String>)> =
    OnceLock::new();

/// A group config change waiting for confirmation because it empties tree nodes.
#[derive(Clone)]
struct PendingGroups {
    catalog: Arc<GroupCatalog>,
    message: String,
    /// User group values to empty in the tree; `None` empties every user group.
    only: Option<Vec<u8>>,
    /// Config file to write before applying (editor saves); `None` for loaded files.
    write_to: Option<PathBuf>,
}

/// Finds the icon folder: `$RECVRVE_ICONS` (or the older `$DRAFTREE_ICONS`), then `./champion_icons`, then next to the
/// executable, then the crate root (handy for `cargo run` from elsewhere).
fn find_icon_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for var in ["RECVRVE_ICONS", "DRAFTREE_ICONS"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(PathBuf::from(dir));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(ICON_DIR_NAME));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join(ICON_DIR_NAME));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(ICON_DIR_NAME));
    candidates.into_iter().find(|p| p.is_dir())
}

/// URL of a file served by the icon protocol (see `serve_icon`).
fn served_url(path: &str) -> String {
    // Wry exposes custom protocols as `http://<name>.localhost` on Windows.
    if cfg!(target_os = "windows") {
        format!("http://{ICON_PROTOCOL}.localhost/{path}")
    } else {
        format!("{ICON_PROTOCOL}://localhost/{path}")
    }
}

pub(crate) fn icon_url(champion: &ChampionInfo) -> String {
    // Icon keys are plain ASCII letters, so they need no URL escaping.
    served_url(&format!("{}.png", champion.key))
}

/// The app's font (Lexend Deca, all weights), bundled so it works offline.
static APP_FONT: &[u8] = include_bytes!("../assets/fonts/LexendDeca-Variable.ttf");
const APP_FONT_PATH: &str = "fonts/LexendDeca.ttf";

/// `@font-face` for the app's font, served by the icon protocol.
fn font_face_css() -> String {
    format!(
        "@font-face {{ font-family: 'Lexend Deca'; font-weight: 100 900; font-style: normal; src: url('{}') format('truetype'); }}",
        served_url(APP_FONT_PATH)
    )
}

/// The icon of the champion with this key, from memory or read into it.
fn icon_bytes(key: &'static str) -> Option<&'static [u8]> {
    if let Some(bytes) = ICON_CACHE.lock().ok()?.get(key) {
        return Some(*bytes);
    }
    // Read outside the lock; a missing icon isn't cached, so it can still appear.
    let dir = ICON_DIR.get()?.as_ref()?;
    let bytes = std::fs::read(dir.join(format!("{key}.png"))).ok()?;
    let mut cache = ICON_CACHE.lock().ok()?;
    Some(
        *cache
            .entry(key)
            .or_insert_with(|| Box::leak(bytes.into_boxed_slice())),
    )
}

/// Serves icon files. Only keys from the champion table are served, so a request can never
/// reach outside the icon folder.
fn serve_icon(uri_path: &str) -> Response<Cow<'static, [u8]>> {
    let requested = uri_path.trim_start_matches('/');
    if requested == APP_FONT_PATH {
        return Response::builder()
            .header("Content-Type", "font/ttf")
            .header("Cache-Control", IMMUTABLE)
            .body(Cow::Borrowed(APP_FONT))
            .unwrap();
    }
    let found = CHAMPIONS
        .iter()
        .find(|c| requested.strip_suffix(".png") == Some(c.key))
        .and_then(|c| icon_bytes(c.key));

    match found {
        Some(bytes) => Response::builder()
            .header("Content-Type", "image/png")
            .header("Cache-Control", IMMUTABLE)
            .body(Cow::Borrowed(bytes))
            .unwrap(),
        None => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b"Not Found"[..]))
            .unwrap(),
    }
}

/// Serves theme background pictures and their blurred copies. Only picture files directly
/// inside a theme folder, and blur cache files, are served (see `theme::resolve_request`).
fn serve_background(uri_path: &str) -> Response<Cow<'static, [u8]>> {
    let found = theme::resolve_request(uri_path)
        .and_then(|(path, mime)| std::fs::read(path).ok().map(|bytes| (bytes, mime)));
    match found {
        // Pictures carry a `?v=` version and cache files a content hash, so a URL's file
        // never changes.
        Some((bytes, mime)) => Response::builder()
            .header("Content-Type", mime)
            .header("Cache-Control", IMMUTABLE)
            .body(Cow::Owned(bytes))
            .unwrap(),
        None => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b"Not Found"[..]))
            .unwrap(),
    }
}

fn main() {
    ICON_DIR.get_or_init(find_icon_dir);
    // Every icon into memory off the UI thread, which serves them: the grid asks for all of
    // them at once while the window opens, and the first nodes added soon after.
    std::thread::spawn(|| {
        for c in CHAMPIONS.iter() {
            icon_bytes(c.key);
        }
    });
    // Finds tabs left by a session that didn't close cleanly, and starts this one.
    autosave::start();
    SELECTION_GROUPS.get_or_init(selection::load_startup);
    STARTUP_GROUPS.get_or_init(|| {
        let (catalog, error) = groups::load_startup();
        (Arc::new(catalog), error)
    });

    STARTUP_FILES.get_or_init(|| {
        std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect()
    });
    // Every window hides when its close button is used, and its app decides whether it
    // closes (see `windows::Message::CloseRequested`). The app exits with the last window,
    // and a clean exit deletes the session's autosaves.
    let config = window_config().with_custom_event_handler(|event, _| {
        use dioxus::desktop::tao::event::{Event, WindowEvent};
        match event {
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                windows::route_close(*window_id);
            }
            Event::LoopDestroyed => autosave::finish(),
            _ => {}
        }
    });

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}

/// A champion tile in the grid, as seen from the selected node.
struct ChampTile {
    value: u8,
    champion: &'static ChampionInfo,
    /// Allowed as the next action on this line.
    selectable: bool,
    /// Already picked or banned on this line.
    taken: bool,
    /// Banned on this line (shown with the red ban sign).
    banned: bool,
    /// Colors of the groups on this line that could stand for this champion, earliest
    /// first, at most four.
    rings: Vec<String>,
    /// Names of all those groups, for the tooltip.
    ring_names: Vec<String>,
    /// Fearless: the first earlier game this champion was picked in, and who it is locked
    /// for.
    fearless: Option<(u8, fearless::Tone)>,
}

/// A panel that can be hidden (see `configio::Hidden`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    /// Both team panels together.
    Teams,
    Pool,
    Groups,
    Tree,
}

impl Panel {
    const ALL: [Panel; 4] = [Panel::Teams, Panel::Pool, Panel::Groups, Panel::Tree];

    fn label(self) -> &'static str {
        match self {
            Panel::Teams => "Team panels",
            Panel::Pool => "Champions",
            Panel::Groups => "Representational groups",
            Panel::Tree => "Draft tree",
        }
    }

    fn get(self, hidden: &configio::Hidden) -> bool {
        match self {
            Panel::Teams => hidden.teams,
            Panel::Pool => hidden.pool,
            Panel::Groups => hidden.groups,
            Panel::Tree => hidden.tree,
        }
    }

    fn set(self, hidden: &mut configio::Hidden, value: bool) {
        match self {
            Panel::Teams => hidden.teams = value,
            Panel::Pool => hidden.pool = value,
            Panel::Groups => hidden.groups = value,
            Panel::Tree => hidden.tree = value,
        }
    }
}

/// A group tile in the groups area.
struct GroupTile {
    value: u8,
    label: String,
    tooltip: String,
    selectable: bool,
    color: Option<String>,
}

/// Thickness of one group ring around a champion tile, in pixels.
const RING_WIDTH: usize = 2;

/// CSS variables for a champion tile's group rings. `rings` is earliest group first; the
/// earliest is drawn innermost. The soft glow uses the outermost ring's color.
fn ring_style(rings: &[String]) -> String {
    let n = rings.len();
    if n == 0 {
        return String::new();
    }
    // Inset shadows stack from the edge inwards; the first listed is drawn on top, so the
    // outermost (thinnest) ring comes first and the innermost (widest) last.
    let shadows: Vec<String> = (0..n)
        .rev()
        .map(|i| format!("inset 0 0 0 {}px {}", (n - i) * RING_WIDTH, rings[i]))
        .collect();
    format!("--rings: {}; --glow: {};", shadows.join(", "), rings[n - 1])
}

/// Asks where to save a group config, defaulting to `current`. Adds `.json` if missing.
async fn pick_json_save_path(title: &str, current: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter("Config", &["json", "JSON"]);
    if let Some(dir) = current.parent() {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = current.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    let mut path = dialog.save_file().await?.path().to_path_buf();
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".json");
        path.set_file_name(name);
    }
    Some(path)
}

/// Tooltip text describing a node's raw bytes, for checking what the tree stores.
fn node_debug(pos: DraftPos, value: u8) -> String {
    format!("value {value}, position {:08b}", pos.bits())
}

/// The smooth animation profile, for code that runs outside renders (see `UiSettings`).
static SMOOTH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// One zoom step of Ctrl + = / Ctrl + - (about one notch of the mouse wheel).
const ZOOM_KEY_STEP: f64 = 1.15;

/// A window's setup: size, icon, the protocols serving icons and theme pictures, and hiding
/// instead of closing (its app closes it, see `App`).
fn window_config() -> Config {
    let window = WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_window_icon(window_icon())
        .with_inner_size(LogicalSize::new(1400.0, 900.0));
    Config::new()
        .with_window(window)
        .with_menu(None)
        .with_background_color((18, 20, 27, 255))
        .with_custom_protocol(ICON_PROTOCOL, |_, request| serve_icon(request.uri().path()))
        .with_custom_protocol(theme::PROTOCOL, |_, request| {
            serve_background(request.uri().path())
        })
        .with_close_behaviour(dioxus::desktop::WindowCloseBehaviour::WindowHides)
}

/// A window opened for a tab moved out of another one (see windows.rs).
#[component]
fn WindowRoot(start: windows::Start) -> Element {
    use_context_provider(|| start);
    rsx! { App {} }
}

#[component]
fn App() -> Element {
    // This window (see windows.rs): its number (1 for the first) and its inbox, and the
    // tab it opened with when another window sent one.
    let start = try_consume_context::<windows::Start>();
    let (window_no, inbox) = use_hook(|| {
        let (number, inbox) = windows::register(dioxus::desktop::window().id());
        if number > 1 {
            dioxus::desktop::window()
                .set_title(&format!("{WINDOW_TITLE} \u{2014} Window {number}"));
        }
        (
            number,
            std::rc::Rc::new(std::cell::RefCell::new(Some(inbox))),
        )
    });
    use_drop(move || windows::unregister(window_no));
    // The tree owns the active group catalog; see `DraftTree::groups`.
    let mut tree = use_signal(|| {
        let groups = STARTUP_GROUPS
            .get()
            .map(|(c, _)| c.clone())
            .unwrap_or_default();
        DraftTree::with_groups(groups)
    });
    // The selected node: the draft state shown on the sides. The root is the empty draft.
    let mut cursor = use_signal(|| tree.peek().root());
    // Nodes selected together with the active node (Ctrl+click, a selection box), all at its
    // depth. Actions on several nodes use the active node and these (see `selection`).
    let mut extra = use_signal(Vec::<NodeId>::new);
    // Follow mode (Ctrl+Shift+L): the draft entered so far and the prepared nodes that fit it.
    let mut follow = use_signal(|| None::<follow::Follow>);
    // Tabs (see tabs.rs): what the strip shows for every tab, the active tab (whose state
    // is in the signals here) and the parked state of the others.
    // Tab ids are unique across windows. A window opened for a moved tab replaces its
    // first tab with that one, so the first takes no "Draft N" number.
    let first_tab = use_hook(|| {
        let number = if start.is_some() {
            0
        } else {
            windows::next_tab_number()
        };
        (windows::next_tab_id(), number)
    });
    let mut tab_list = use_signal(|| {
        vec![tabs::TabInfo::new(
            first_tab.0,
            first_tab.1,
            &tree.peek(),
            None,
        )]
    });
    let mut active_tab = use_signal(|| first_tab.0);
    let mut parked = use_signal(HashMap::<u64, tabs::Parked>::new);
    // A tab with unsaved changes waiting for confirmation before it closes.
    let mut close_confirm = use_signal(|| None::<u64>);
    // Follow mode: enters the draft's next action.
    let mut follow_enter = move |value: u8| {
        let Some(mut f) = follow.peek().clone() else {
            return;
        };
        if !f.can_enter(value) {
            return;
        }
        f.enter(&tree.peek(), value);
        if let Some(&best) = f.current().first() {
            cursor.set(best);
        }
        follow.set(Some(f));
    };
    // The selection: the active node first, then the others still at its depth.
    let selection = move || -> Vec<NodeId> {
        let t = tree.peek();
        let at = cursor();
        let depth = t.path(at).len();
        let mut out = vec![at];
        for &e in extra.peek().iter() {
            if e != at && t.contains(e) && t.path(e).len() == depth && !out.contains(&e) {
                out.push(e);
            }
        }
        out
    };
    // The selection only holds nodes at the active node's depth: moving it up or down
    // (Shift+Z / Shift+X, a click elsewhere) or deleting nodes drops the rest.
    use_effect(move || {
        let t = tree.read();
        let depth = t.path(cursor()).len();
        let stale = extra
            .peek()
            .iter()
            .any(|&e| !t.contains(e) || t.path(e).len() != depth || e == cursor());
        if stale {
            let at = cursor();
            extra
                .write()
                .retain(|&e| t.contains(e) && t.path(e).len() == depth && e != at);
        }
    });
    let search = use_signal(String::new);
    let mut save_menu_open = use_signal(|| false);
    // "Include selected node" checkbox in the Save menu. Unchecked when the app starts.
    let mut save_selection = use_signal(|| false);
    // True while a file dialog is open, so a second one can't be started.
    let mut dialog_open = use_signal(|| false);
    // Message shown in the error popup, if any.
    // A short message under the toolbar that hides itself (see `show_notice`), with an id
    // so an older timer doesn't hide a newer message.
    let mut notice = use_signal(|| None::<(u64, String)>);
    // Smooth animations: the notice slides out before it goes.
    let mut notice_leaving = use_signal(|| false);
    let mut show_notice = move |message: String| {
        let id = notice.peek().as_ref().map_or(0, |(id, _)| id + 1);
        notice_leaving.set(false);
        notice.set(Some((id, message)));
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            let shown = move || {
                notice
                    .peek()
                    .as_ref()
                    .is_some_and(|(shown, _)| *shown == id)
            };
            if shown() && SMOOTH.load(std::sync::atomic::Ordering::Relaxed) {
                notice_leaving.set(true);
                tokio::time::sleep(std::time::Duration::from_millis(160)).await;
            }
            if shown() {
                notice.set(None);
            }
        });
    };
    // A warning popup, like `error` but titled as a warning.
    let mut warning = use_signal(|| None::<String>);
    // The merge panel: open or not, the files picked, and whether the current tree joins.
    let mut merge_open = use_signal(|| false);
    let mut merge_files = use_signal(Vec::<PathBuf>::new);
    // Open tabs to merge too, by id.
    let mut merge_tabs = use_signal(Vec::<u64>::new);
    let mut error = use_signal(|| {
        let messages: Vec<String> = [
            STARTUP_GROUPS.get().and_then(|(_, e)| e.clone()),
            SELECTION_GROUPS.get().and_then(|(_, e)| e.clone()),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!messages.is_empty()).then(|| messages.join("\n\n"))
    });
    // Which selection groups filter the grid, and whether they combine by intersection.
    let mut sel_groups = use_signal(|| {
        SELECTION_GROUPS
            .get()
            .map(|(g, _)| g.clone())
            .unwrap_or_default()
    });
    let mut filters_on = use_signal(|| vec![false; sel_groups.peek().len()]);
    let mut filter_intersect = use_signal(|| false);
    // Whether selection filters also apply to the representational groups area.
    let mut rgroup_filter = use_signal(|| false);
    let mut ui_settings = use_signal(configio::UiSettings::load);
    // Size of the graph area, reported by the layout script: the graph centres its root
    // in it.
    let mut tree_size = use_signal(|| (900.0f64, 400.0f64));
    let save_settings = move || {
        if let Err(e) = ui_settings.peek().save() {
            eprintln!("could not save UI settings: {e}");
        }
        // The other windows share the settings.
        let settings = ui_settings.peek().clone();
        windows::broadcast(window_no, || {
            windows::Message::Settings(Box::new(settings.clone()))
        });
    };
    // Easter eggs: one switch for all of them (see easter.rs).
    let mut easter_eggs = use_signal(|| ui_settings.peek().easter_eggs);
    easter::provide(easter_eggs);
    let mut set_easter_eggs = move |on: bool| {
        easter_eggs.set(on);
        ui_settings.write().easter_eggs = on;
        save_settings();
    };
    // Hides or shows a panel, and remembers it.
    let mut set_hidden = move |panel: Panel, value: bool| {
        panel.set(&mut ui_settings.write().hidden, value);
        save_settings();
    };
    // Fullscreen: the window fills the screen and only the champions, groups and graph
    // are shown (not saved; Esc or F11 leaves it).
    let mut fullscreen = use_signal(|| false);
    let mut set_fullscreen = move |on: bool| {
        fullscreen.set(on);
        dioxus::desktop::window().set_fullscreen(on);
    };
    let mut view_menu_open = use_signal(|| false);
    // The row the graph's drawn window is centred on (see `tree_view::GraphWindow`);
    // `None` draws it around the selected node.
    let mut graph_center = use_signal(|| None::<f64>);
    // Graph zoom in the current direction: times `factor`, or back to 100% with `None`.
    let mut zoom_graph = move |factor: Option<f64>| {
        let mut s = ui_settings.write();
        let zoom = if s.graph_vertical {
            &mut s.graph_zoom_v
        } else {
            &mut s.graph_zoom_h
        };
        *zoom = match factor {
            Some(f) => (*zoom * f).clamp(configio::ZOOM_MIN, configio::ZOOM_MAX),
            None => 1.0,
        };
        drop(s);
        // Smooth animations ease the change (see the layout script).
        document::eval("window.__recvrveZoomKey = performance.now();");
        save_settings();
    };
    // Panel sizes from the splitters.
    use_future(move || async move {
        let mut listener = layout::listen();
        while let Some(message) =
            layout::next(&mut listener, move || ui_settings.peek().layout).await
        {
            match message {
                layout::Message::Layout(new) => {
                    ui_settings.write().layout = new;
                    save_settings();
                }
                layout::Message::TreeSize(w, h) => tree_size.set((w, h)),
                // Scrolling moved the view near the window's edge: the window follows it.
                layout::Message::Window(row) => graph_center.set(Some(row)),
                layout::Message::Zoom(factor) => zoom_graph(Some(factor)),
            }
        }
    });
    let mut rgroup_editor_open = use_signal(|| false);
    let mut sel_editor_open = use_signal(|| false);
    // Themes from config/themes/, re-read whenever the theme menu opens.
    let mut themes = use_signal(|| theme::scan().0);
    // The theme settings panel's unsaved version, shown live while it is open.
    let mut theme_preview = use_signal(|| None::<theme::Theme>);
    let mut theme_menu_open = use_signal(|| false);
    let mut theme_editor_open = use_signal(|| false);
    let active_theme = use_memo(move || {
        if let Some(t) = theme_preview() {
            return t;
        }
        let id = ui_settings.read().theme.clone();
        themes
            .read()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .unwrap_or_else(|| theme::Theme::builtin(theme::Base::Dark))
    });
    // Tell the user once if the saved theme is gone or some themes couldn't be read.
    use_hook(move || {
        let (_, errors) = theme::scan();
        let id = ui_settings.peek().theme.clone();
        let mut messages = errors;
        if !themes.peek().iter().any(|t| t.id == id) {
            messages.push(format!(
                "Theme \"{id}\" was not found in config/themes, so Dark is used."
            ));
        }
        if !messages.is_empty() {
            let mut text = error.peek().clone().unwrap_or_default();
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&messages.join("\n"));
            error.set(Some(text));
        }
    });
    let settings = ui_settings.read().clone();
    let share_open = use_signal(|| false);
    // Text typed into the share box that isn't a valid tree string, paired with the string
    // the box was showing when it was typed. It stays on screen only while the tree still
    // encodes to that string, so any change to the tree replaces it with the fresh string.
    // Also holds the message explaining why the text doesn't load.
    let mut share_draft = use_signal(|| None::<(String, String, String)>);
    let mut help_open = use_signal(|| false);
    // Keyboard shortcuts (see keys.rs) and the shortcut being recorded in the Help panel.
    let kb_loaded = use_hook(keys::Keybinds::load);
    let mut keybinds = use_signal(|| kb_loaded.0.clone());
    let mut recording = use_signal(|| None::<shortcuts_panel::RecordTarget>);
    let mut kb_message = use_signal(|| None::<String>);
    // The child the last Shift+click added or hit; selected when Shift is released.
    let mut branch_target = use_signal(|| None::<NodeId>);
    use_effect(move || keys::send_to_webview(&keybinds.read()));
    use_effect(move || keys::set_recording(recording().is_some()));
    use_effect(move || {
        if !help_open() {
            recording.set(None);
        } else if !ui_settings.peek().help_seen {
            // Opened once (click or shortcut): the Help button stops glowing for good.
            ui_settings.write().help_seen = true;
            if let Err(e) = ui_settings.peek().save() {
                eprintln!("could not save UI settings: {e}");
            }
        }
    });
    let mut export_menu_open = use_signal(|| false);
    let mut export_kind = use_signal(|| None::<export_panel::ExportKind>);
    // True while an export runs; the export panel stays open until it finishes.
    let export_busy = use_signal(|| false);
    let mut fearless_open = use_signal(|| false);
    // The role tag popup, while open.
    let mut role_popup = use_signal(|| None::<role_popup::RolePopupState>);
    // Allow adding new branches below unreachable nodes.
    let mut edit_unreachable = use_signal(|| false);
    // Branches taken out by "Remove unreachable", until they are restored.
    let mut pruned = use_signal(|| None::<tree::Pruned>);
    // Branches taken out by "Remove conflicting", until they are restored.
    let mut pruned_conflicts = use_signal(|| None::<tree::Pruned>);
    // Ctrl+Shift+W: the next champion or group clicked becomes the selected node's value.
    let mut set_mode = use_signal(|| false);
    let mut history = use_signal(History::default);
    // For each node, the child that was last on the selected path. Shift+X follows it when
    // a node has several children.
    let mut last_child = use_signal(SecondaryMap::<NodeId, NodeId>::new);
    // Group value -> palette index, handed out the first time a group appears in the tree.
    let mut group_colors = use_signal(HashMap::<u8, usize>::new);
    // A loaded group config waiting for confirmation, with the message to show.
    let mut pending_groups = use_signal(|| None::<PendingGroups>);
    let mut groups_info_open = use_signal(|| false);
    use_context_provider(|| ViewContext {
        tree,
        colors: group_colors,
        theme: active_theme,
    });

    use_effect(move || {
        let t = tree.read();
        // `peek` so this effect doesn't re-run on its own write.
        let new = groups::uncolored(t.values(), &group_colors.peek());
        if !new.is_empty() {
            let mut colors = group_colors.write();
            for v in new {
                let next = colors.len();
                colors.insert(v, next);
            }
        }
    });

    use_effect(move || {
        let t = tree.read();
        let path = t.path(cursor());
        let mut memory = last_child.write();
        for pair in path.windows(2) {
            memory.insert(pair[0], pair[1]);
        }
    });

    // Shared with the panels (see ui.rs): marks per tree, the selected line per selection.
    // `line_info` doesn't read `tree_info`: a memo read while it is stale recomputes on the
    // spot and then again in its own task, so the marks would be computed twice per edit.
    let tree_info = use_memo(move || ui::TreeInfo::compute(&tree.read()));
    let line_info = use_memo(move || {
        ui::LineInfo::compute(
            &tree.read(),
            cursor(),
            follow.read().as_ref(),
            set_mode(),
            edit_unreachable(),
        )
    });
    let (step, following) = {
        let line = line_info.read();
        (line.step, line.following)
    };
    let fearless_state = tree_info.read().fearless;
    // Tooltips name the current shortcuts, so this render follows binding changes.
    keys::set_current(&keybinds.read());
    let theme_now = active_theme();
    let theme = theme_now.base.class();
    let bg_class = if theme_now.background.is_some() {
        "has-bg"
    } else {
        ""
    };
    let theme_css = theme_now.css();

    // Whether new branches may start at the selected node: never in a conflicting branch,
    // and not below an unreachable node unless that is switched on.
    let extend_allowed = move || {
        let t = tree.read();
        t.line_conflict(cursor()).is_none() && (edit_unreachable() || !t.line_unreachable(cursor()))
    };

    // Snapshot of the current state, taken before an edit so it can be undone.
    let snapshot = move || history::snapshot(&tree.read(), cursor());
    let mut commit =
        move |before: history::State, kind: ChangeKind| history.write().record(before, kind);

    // Role tags for a new pick of `value`: a group's role lock, narrowed to the active role
    // filters when auto-tagging is on.
    let tags_for_new = move |value: u8| -> u8 {
        let t = tree.peek();
        let next = t.get(*cursor.peek()).and_then(|n| n.pos().next());
        if next.is_none_or(|p| p.action() != Action::Pick) {
            return 0;
        }
        let lock = t.groups().get(value).map_or(0, |g| g.roles);
        let auto = if ui_settings.peek().auto_tag_roles {
            sel_groups
                .peek()
                .iter()
                .zip(filters_on.peek().iter())
                .filter(|(_, on)| **on)
                .filter_map(|(g, _)| roles::Role::from_selection_name(&g.name))
                .fold(0, |m, r| m | r.bit())
        } else {
            0
        };
        match (lock, auto) {
            (0, a) => a,
            (l, a) if a & l != 0 => a & l,
            (l, _) => l,
        }
    };

    // Set mode: makes `value` the selected node's value. If a sibling already holds it,
    // selects that sibling instead and leaves the tree as it is.
    let mut set_value_to = move |value: u8| {
        set_mode.set(false);
        let id = cursor();
        let target = {
            let t = tree.read();
            t.get(id).filter(|n| n.parent().is_some()).map(|n| {
                let value = if value == EMPTY || value == NO_BAN {
                    tree::empty_value_for(n.pos())
                } else {
                    value
                };
                (value, t.sibling_with_value(id, value), n.value())
            })
        };
        let Some((value, sibling, old)) = target else {
            return;
        };
        if let Some(sibling) = sibling {
            cursor.set(sibling);
            return;
        }
        // The same value again changes nothing.
        if value == old {
            return;
        }
        let before = snapshot();
        let result = tree.write().set_value(id, value);
        match result {
            Ok(()) => commit(before, ChangeKind::Edit),
            Err(e) => error.set(Some(format!("Could not set this value: {e}."))),
        }
    };

    // Ctrl+D, Ctrl+Alt+D, Ctrl+Shift+D: copies a branch, then selects the copy of the
    // selected node.
    let mut duplicate = move |action: keys::Action| {
        // Ctrl+D copies every selected node's branch; the copies become the selection.
        if action == keys::Action::DuplicateBranch && !extra.peek().is_empty() {
            let nodes = selection();
            let before = snapshot();
            let copies: Vec<NodeId> = {
                let mut t = tree.write();
                nodes
                    .iter()
                    .filter_map(|&n| t.duplicate_branch(n).ok())
                    .collect()
            };
            if !copies.is_empty() {
                commit(before, ChangeKind::Edit);
                extra.set(copies[1..].to_vec());
                cursor.set(copies[0]);
            }
            return;
        }
        let id = cursor();
        let before = snapshot();
        let result = {
            let mut t = tree.write();
            match action {
                keys::Action::DuplicateLine => t.duplicate_line(id, false),
                keys::Action::DuplicateLineAndBranch => t.duplicate_line(id, true),
                _ => t.duplicate_branch(id),
            }
        };
        match result {
            Ok(copy) => {
                commit(before, ChangeKind::Edit);
                cursor.set(copy);
            }
            Err(TreeError::RootIsFixed) => {}
            Err(e) => error.set(Some(format!("Could not duplicate: {e}."))),
        }
    };

    // Follows the existing branch for `value`, or starts a new one.
    let mut choose = move |value: u8| {
        if follow.peek().is_some() {
            follow_enter(value);
            return;
        }
        if set_mode() && cursor() != tree.read().root() {
            set_value_to(value);
            return;
        }
        // The start node has no value to set.
        set_mode.set(false);
        // Several nodes selected: each follows or gets the value, and the selection moves to
        // those children.
        if !extra.peek().is_empty() {
            let nodes = selection();
            let tags = tags_for_new(value);
            let before = snapshot();
            let mut added = false;
            let mut children = Vec::new();
            {
                let mut t = tree.write();
                for &n in &nodes {
                    if let Some(c) = t.child_with_value(n, value) {
                        children.push(Some(c));
                    } else if t.line_conflict(n).is_none() {
                        match t.add_child_tagged(n, value, tags) {
                            Ok(c) => {
                                added = true;
                                children.push(Some(c));
                            }
                            Err(_) => children.push(None),
                        }
                    } else {
                        children.push(None);
                    }
                }
            }
            if added {
                commit(before, ChangeKind::Edit);
            }
            let skipped = children.iter().filter(|c| c.is_none()).count();
            if skipped > 0 {
                show_notice(format!(
                    "{skipped} of the {} selected nodes can't take this value.",
                    nodes.len()
                ));
            }
            if let Some(Some(active)) = children.first() {
                extra.set(children[1..].iter().flatten().copied().collect());
                cursor.set(*active);
            }
            return;
        }
        let existing = tree.read().child_with_value(cursor(), value);
        if let Some(child) = existing {
            cursor.set(child);
            return;
        }
        if !extend_allowed() {
            return;
        }
        let tags = tags_for_new(value);
        let before = snapshot();
        let result = tree.write().add_child_tagged(cursor(), value, tags);
        match result {
            Ok(id) => {
                commit(before, ChangeKind::Edit);
                cursor.set(id);
            }
            Err(e) => eprintln!("cannot add value {value}: {e}"),
        }
    };

    // Shift+click (the branch click modifier, see keys.rs): adds `value` as a new branch
    // without moving the selection. Empty values may repeat, so Shift+click on Empty always
    // adds another empty branch. The clicked child (new, or the existing one with that
    // value) is remembered, and releasing Shift selects the last one (`branch_release`).
    let mut add_branch = move |value: u8| {
        if follow.peek().is_some() {
            follow_enter(value);
            return;
        }
        if set_mode() && cursor() != tree.read().root() {
            set_value_to(value);
            return;
        }
        set_mode.set(false);
        let wildcard = value == EMPTY || value == NO_BAN;
        // Several nodes selected: each gets the branch too (releasing Shift moves the
        // selection to them, see `branch_release`).
        if !extra.peek().is_empty() {
            let tags = tags_for_new(value);
            let before = snapshot();
            let mut added = false;
            let nodes = selection();
            {
                let mut t = tree.write();
                for &n in &nodes[1..] {
                    let exists = !wildcard && t.child_with_value(n, value).is_some();
                    if !exists
                        && t.line_conflict(n).is_none()
                        && t.add_child_tagged(n, value, tags).is_ok()
                    {
                        added = true;
                    }
                }
            }
            if added {
                commit(before, ChangeKind::Edit);
            }
        }
        let existing = if wildcard {
            None
        } else {
            tree.read().child_with_value(cursor(), value)
        };
        if let Some(child) = existing {
            branch_target.set(Some(child));
            return;
        }
        if !extend_allowed() {
            return;
        }
        let tags = tags_for_new(value);
        let before = snapshot();
        let result = tree.write().add_child_tagged(cursor(), value, tags);
        match result {
            Ok(id) => {
                commit(before, ChangeKind::Edit);
                branch_target.set(Some(id));
            }
            Err(e) => eprintln!("cannot add value {value}: {e}"),
        }
    };

    // Same as selecting `id` and pressing Delete: removes `id` and everything below it, then
    // selects its parent. The root can't be deleted, so for the root this only selects it.
    // Find in tree (Ctrl+F): the bar's text and options, and the matches.
    let mut find_open = use_signal(|| false);
    let mut find_query = use_signal(String::new);
    let mut find_options = use_signal(find::FindOptions::default);
    use_effect(move || keys::set_find_open(find_open()));
    let found = use_memo(move || {
        if !find_open() {
            return Vec::new();
        }
        let t = tree.read();
        find::matches(&t, &find_query(), find_options(), &SecondaryMap::new())
    });
    // Typing in Find selects the first match (in the graph's order) unless the selected
    // node is still one.
    use_effect(move || {
        let matches = found();
        if !find_open() {
            return;
        }
        if let Some(&first) = matches.first()
            && !matches.contains(&cursor.peek())
        {
            cursor.set(first);
        }
    });
    // Enter / Shift+Enter: selects the next / previous match (and so shows it).
    let mut find_step = move |forward: bool| {
        let next = find::step(&tree.peek(), &found.peek(), cursor(), forward);
        if let Some(id) = next {
            cursor.set(id);
        }
    };
    let mut open_find = move || {
        find_open.set(true);
        // Focus the field once it exists (it appears with the next render).
        document::eval(
            "(function f(n) { const i = document.getElementById('find-input'); if (i) { i.focus(); i.select(); } else if (n > 0) setTimeout(() => f(n - 1), 30); })(20);",
        );
    };
    // Hover preview of graph nodes: shown after the pointer rests on a node for 300 ms.
    let mut preview = use_signal(|| None::<(NodeId, f64, f64)>);
    let mut hover_gen = use_signal(|| 0u64);
    let mut hover_node = move |h: Option<(NodeId, f64, f64)>| {
        let generation = *hover_gen.peek() + 1;
        hover_gen.set(generation);
        preview.set(None);
        let Some(h) = h else { return };
        if !ui_settings.peek().node_previews {
            return;
        }
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if *hover_gen.peek() == generation {
                preview.set(Some(h));
            }
        });
    };
    // Ctrl+H: hides or shows the selected node's branches. Not an edit: no undo step.
    let mut toggle_hidden = move || {
        let id = cursor();
        let collapsed = tree.peek().get(id).is_some_and(|n| n.is_collapsed());
        // Every selected node follows the active one.
        let nodes = selection();
        let mut t = tree.write();
        for n in nodes {
            t.set_collapsed(n, !collapsed);
        }
    };
    // Ctrl+Alt+H: shows every hidden branch.
    let mut show_all_hidden = move || {
        if tree.peek().any_collapsed() {
            tree.write().expand_all();
        }
    };
    // Ctrl+Alt+H: hides the branches below every topmost unreachable node, as Ctrl+H
    // does (the nodes themselves stay, showing how much is hidden), or shows them again
    // when they are all hidden already.
    let mut hide_unreachable = move || {
        let tops = tree.peek().unreachable_tops();
        if tops.is_empty() {
            return;
        }
        let hide = !tops
            .iter()
            .all(|&n| tree.peek().get(n).is_some_and(|n| n.is_collapsed()));
        let mut t = tree.write();
        for n in tops {
            t.set_collapsed(n, hide);
        }
        // Keep the selection on a visible node.
        let at = cursor();
        if t.concealed(at) {
            let visible = t.ancestors(at).find(|&a| !t.concealed(a));
            let to = visible.unwrap_or(t.root());
            drop(t);
            cursor.set(to);
        }
    };
    // Show only marked lines (the Draft tree header's "Marked only").
    let mut marked_only = use_signal(|| false);
    // Counter picks for the team panels (see `counters`): the newest snapshot, and an
    // update in progress (requests done, of all).
    let mut counter_data = use_signal(|| std::rc::Rc::new(counters::Counters::load()));
    let mut counter_update = use_signal(|| None::<(usize, usize)>);
    // The graph's layout, shared by the graph and the selection box.
    let graph_layout =
        use_memo(move || tree_view::CachedLayout::compute(&tree.read(), marked_only()));
    let smooth_on = use_memo(move || ui_settings.read().smooth_animations);
    let entering = tree_view::use_entering(graph_layout, active_tab, smooth_on);

    // Tabs: parks the active tab's state, and shows a parked one.
    let mut park_active = move || -> tabs::Parked {
        let (zoom_h, zoom_v) = {
            let s = ui_settings.peek();
            (s.graph_zoom_h, s.graph_zoom_v)
        };
        tabs::Parked {
            tree: tree.peek().clone(),
            cursor: *cursor.peek(),
            extra: extra.peek().clone(),
            history: std::mem::take(&mut *history.write()),
            last_child: last_child.peek().clone(),
            group_colors: group_colors.peek().clone(),
            find_open: *find_open.peek(),
            find_query: find_query.peek().clone(),
            find_options: *find_options.peek(),
            marked_only: *marked_only.peek(),
            zoom: (zoom_h, zoom_v),
            pruned: pruned.peek().clone(),
            pruned_conflicts: pruned_conflicts.peek().clone(),
            save_selection: *save_selection.peek(),
            edit_unreachable: *edit_unreachable.peek(),
            follow: follow.peek().clone(),
        }
    };
    let mut show_parked = move |p: tabs::Parked| {
        role_popup.set(None);
        set_mode.set(false);
        preview.set(None);
        share_draft.set(None);
        branch_target.set(None);
        tree.set(p.tree);
        cursor.set(p.cursor);
        // Another tree: its graph opens around its selected node.
        graph_center.set(None);
        extra.set(p.extra);
        history.set(p.history);
        last_child.set(p.last_child);
        group_colors.set(p.group_colors);
        find_open.set(p.find_open);
        find_query.set(p.find_query);
        find_options.set(p.find_options);
        marked_only.set(p.marked_only);
        pruned.set(p.pruned);
        pruned_conflicts.set(p.pruned_conflicts);
        save_selection.set(p.save_selection);
        edit_unreachable.set(p.edit_unreachable);
        follow.set(p.follow);
        {
            let mut s = ui_settings.write();
            s.graph_zoom_h = p.zoom.0;
            s.graph_zoom_v = p.zoom.1;
        }
        save_settings();
    };
    let mut switch_tab = move |id: u64| {
        let current = *active_tab.peek();
        if id == current || !parked.peek().contains_key(&id) {
            return;
        }
        let old = park_active();
        let next = {
            let mut map = parked.write();
            map.insert(current, old);
            map.remove(&id).expect("checked above")
        };
        show_parked(next);
        active_tab.set(id);
    };
    // Opens `p` in a new tab at the end of the strip and shows it. `baseline` is the
    // content hash it was saved with (`None`: the tree as it is now).
    let mut open_tab =
        move |p: tabs::Parked, source: Option<PathBuf>, baseline: Option<Option<u64>>| {
            let id = windows::next_tab_id();
            let number = windows::next_tab_number();
            let mut info = tabs::TabInfo::new(id, number, &p.tree, source);
            if let Some(b) = baseline {
                info.dirty = b != info.baseline;
                info.baseline = b;
            }
            tab_list.write().push(info);
            let current = *active_tab.peek();
            let old = park_active();
            parked.write().insert(current, old);
            show_parked(p);
            active_tab.set(id);
        };
    // Ctrl+T: a new tab with an empty tree, using the active tab's group config.
    let mut new_tab = move || {
        let empty = DraftTree::with_groups(tree.peek().groups().clone());
        open_tab(tabs::Parked::fresh(empty, None, (1.0, 1.0)), None, None);
    };
    let mut close_tab = move |id: u64| {
        if id == *active_tab.peek() {
            let next = tabs::neighbour(&tab_list.peek(), id);
            match next {
                Some(n) => switch_tab(n),
                // The last tab: a fresh one takes its place.
                None => new_tab(),
            }
        }
        parked.write().remove(&id);
        tab_list.write().retain(|t| t.id != id);
    };
    // × or a middle click: asks first when the tab has unsaved changes.
    let mut request_close = move |id: u64| {
        let dirty = tab_list.peek().iter().any(|t| t.id == id && t.dirty);
        if dirty {
            close_confirm.set(Some(id));
        } else {
            close_tab(id);
        }
    };
    // Keeps the active tab's name and unsaved mark up to date. Hashing the tree walks all of
    // it, so it isn't done on every edit: on a tab without unsaved changes it runs right
    // away (the mark shows with the edit); once the mark shows, it only runs after edits
    // settle (to notice the tree is back to how it was saved).
    //
    // Counts tree changes, so a hash can be matched to the tree it was taken of.
    let tree_gen = {
        // Not a local: the memo's closure is replaced on every render.
        let mut n = use_hook(|| CopyValue::new(0u64));
        use_memo(move || {
            tree.read();
            *n.write() += 1;
            *n.peek()
        })
    };
    // The tree's content hash and the `tree_gen` it was taken at.
    let mut tree_hash = use_signal(|| None::<(u64, u64)>);
    // The pending settled-edits hash, by the `tree_gen` it waits on.
    let mut hash_timer = use_signal(|| None::<(u64, dioxus_core::Task)>);
    use_effect(move || {
        let generation = tree_gen();
        let id = active_tab();
        let list = tab_list();
        let Some(info) = list.iter().find(|t| t.id == id) else {
            return;
        };
        let hash = match tree_hash() {
            Some((g, h)) if g == generation => Some(h),
            _ => None,
        };
        let dirty = match (info.baseline, hash) {
            (None, _) => true,
            (Some(b), Some(h)) => b != h,
            (Some(b), None) if !info.dirty => {
                let h = save::content_hash(&tree.read());
                tree_hash.set(Some((generation, h)));
                b != h
            }
            (Some(_), None) => {
                if hash_timer.peek().is_none_or(|(g, _)| g != generation) {
                    if let Some((_, task)) = hash_timer.take() {
                        task.cancel();
                    }
                    let task = spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        if *tree_gen.read() == generation {
                            let h = save::content_hash(&tree.peek());
                            tree_hash.set(Some((generation, h)));
                        }
                    });
                    hash_timer.set(Some((generation, task)));
                }
                true
            }
        };
        let name = tabs::tab_name(&tree.read(), info.source.as_deref(), info.number);
        if info.dirty != dirty || info.name != name {
            drop(list);
            if let Some(info) = tab_list.write().iter_mut().find(|t| t.id == id) {
                info.dirty = dirty;
                info.name = name;
            }
        }
    });

    // Autosave (see autosave.rs): every few minutes, the tabs whose tree changed since the
    // last round are written, with the list of tabs.
    let mut autosaved = use_signal(HashMap::<u64, Vec<u8>>::new);
    let mut autosaved_list = use_signal(autosave::Manifest::default);
    let mut autosave_round = move || {
        let active = *active_tab.peek();
        let list = tab_list.peek().clone();
        let mut written = autosaved.peek().clone();
        let mut changed = Vec::new();
        let mut entries = Vec::new();
        for info in &list {
            let encoded = if info.id == active {
                let t = tree.peek();
                Some((t.len(), save::encode(&t, None)))
            } else {
                parked
                    .peek()
                    .get(&info.id)
                    .map(|p| (p.tree.len(), save::encode(&p.tree, None)))
            };
            let Some((len, bytes)) = encoded else {
                continue;
            };
            // New tabs nobody touched aren't worth restoring.
            if len <= 1 && info.source.is_none() {
                continue;
            }
            entries.push(autosave::Entry {
                id: info.id,
                name: info.name.clone(),
                source: info.source.clone(),
                dirty: info.dirty,
            });
            if written.get(&info.id) != Some(&bytes) {
                changed.push((info.id, bytes.clone()));
                written.insert(info.id, bytes);
            }
        }
        let closed: Vec<u64> = written
            .keys()
            .copied()
            .filter(|id| !entries.iter().any(|e| e.id == *id))
            .collect();
        for id in &closed {
            written.remove(id);
        }
        let manifest = autosave::Manifest { tabs: entries };
        if changed.is_empty() && closed.is_empty() && manifest == *autosaved_list.peek() {
            return;
        }
        autosaved.set(written);
        autosaved_list.set(manifest.clone());
        autosave::write(window_no, changed, manifest, closed);
    };
    use_future(move || async move {
        loop {
            let minutes = u64::from(ui_settings.peek().autosave_minutes.max(1));
            tokio::time::sleep(std::time::Duration::from_secs(60 * minutes)).await;
            if ui_settings.peek().autosave_minutes > 0 {
                autosave_round();
            }
        }
    });
    // Tabs from a session that didn't close cleanly, offered once at startup.
    let restore_offer = use_signal(|| {
        if start.is_some() {
            return None;
        }
        let crashed = autosave::crashed();
        if crashed.tabs.is_empty() {
            // Nothing worth restoring: just tidy up.
            autosave::discard(&crashed);
            None
        } else {
            Some(crashed)
        }
    });
    // A tab's right-click menu: the tab, and where it opened.
    let mut tab_menu = use_signal(|| None::<(u64, f64, f64)>);
    // Windows (see windows.rs). What the other windows' menus call this one.
    use_effect(move || {
        let list = tab_list.read();
        let active = active_tab();
        let name = list
            .iter()
            .find(|t| t.id == active)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let label = match list.len() {
            0 | 1 => name,
            2 => format!("{name} (+1 tab)"),
            n => format!("{name} (+{} tabs)", n - 1),
        };
        windows::set_label(window_no, format!("Window {window_no}: {label}"));
    });
    // A tab arriving from another window: shown at the end of the strip, as it was there.
    let mut adopt_tab = move |moved: windows::MovedTab| {
        let windows::MovedTab {
            info,
            parked: state,
        } = moved;
        let id = info.id;
        let current = *active_tab.peek();
        let old = park_active();
        parked.write().insert(current, old);
        tab_list.write().push(info);
        show_parked(state);
        active_tab.set(id);
    };
    // Takes tab `id` out of this window with everything it keeps.
    let mut take_tab = move |id: u64| -> Option<windows::MovedTab> {
        let info = tab_list.peek().iter().find(|t| t.id == id).cloned()?;
        if id == *active_tab.peek() {
            let next = tabs::neighbour(&tab_list.peek(), id);
            match next {
                Some(next) => switch_tab(next),
                None => {
                    let state = park_active();
                    parked.write().insert(id, state);
                }
            }
        }
        let state = parked.write().remove(&id)?;
        tab_list.write().retain(|t| t.id != id);
        // Its autosave file stays: the window it goes to writes it from now on.
        autosaved.write().remove(&id);
        Some(windows::MovedTab {
            info,
            parked: state,
        })
    };
    // Closes this window for good, with its tabs' autosave files.
    let close_window_now = move || {
        let here: Vec<u64> = tab_list.peek().iter().map(|t| t.id).collect();
        autosave::close_window(window_no, here);
        windows::unregister(window_no);
        let window = dioxus::desktop::window();
        window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
        window.close();
    };
    // Moves tab `id` to window `to`, or to a new window. A window whose last tab leaves
    // closes.
    let mut move_tab = move |(id, to): (u64, Option<u64>)| {
        if to.is_none() && tab_list.peek().len() < 2 {
            show_notice("This is the window's only tab.".into());
            return;
        }
        let Some(moved) = take_tab(id) else {
            return;
        };
        match to {
            None => {
                let start = windows::stash_start(moved);
                let dom = VirtualDom::new_with_props(WindowRoot, WindowRootProps { start });
                dioxus::desktop::window().new_window(dom, window_config());
            }
            Some(window) => {
                let sent = windows::send(window, windows::Message::Adopt(Box::new(moved)));
                if let Err(windows::Message::Adopt(back)) = sent {
                    adopt_tab(*back);
                    show_notice("That window has closed.".into());
                    return;
                }
            }
        }
        if tab_list.peek().is_empty() {
            close_window_now();
        } else if ui_settings.peek().autosave_minutes > 0 {
            autosave_round();
        }
    };
    // The window's close button while tabs have unsaved changes: how many, until confirmed.
    let mut window_close_confirm = use_signal(|| None::<usize>);
    // A window opened for a moved tab shows it in place of its first, empty tab.
    use_hook(move || {
        if let Some(moved) = start.and_then(windows::take_start) {
            let first = *active_tab.peek();
            adopt_tab(moved);
            parked.write().remove(&first);
            tab_list.write().retain(|t| t.id != first);
            if ui_settings.peek().autosave_minutes > 0 {
                spawn(async move { autosave_round() });
            }
        }
    });
    // Messages from the other windows.
    use_future(move || {
        let inbox = inbox.borrow_mut().take();
        async move {
            let Some(mut inbox) = inbox else {
                return;
            };
            while let Some(message) = inbox.recv().await {
                match message {
                    windows::Message::Adopt(moved) => {
                        adopt_tab(*moved);
                        if ui_settings.peek().autosave_minutes > 0 {
                            autosave_round();
                        }
                        dioxus::desktop::window().set_focus();
                    }
                    windows::Message::CloseRequested => {
                        let unsaved = tab_list.peek().iter().filter(|t| t.dirty).count();
                        if unsaved == 0 {
                            close_window_now();
                        } else {
                            // The window hid itself (see `window_config`): back, and ask.
                            dioxus::desktop::window().set_visible(true);
                            window_close_confirm.set(Some(unsaved));
                        }
                    }
                    windows::Message::Settings(new) => {
                        let mut new = *new;
                        {
                            // Each window keeps its own panel sizes and graph zoom.
                            let mine = ui_settings.peek();
                            new.layout = mine.layout;
                            new.graph_zoom_h = mine.graph_zoom_h;
                            new.graph_zoom_v = mine.graph_zoom_v;
                        }
                        if *ui_settings.peek() != new {
                            if !themes.peek().iter().any(|t| t.id == new.theme) {
                                themes.set(theme::scan().0);
                            }
                            easter_eggs.set(new.easter_eggs);
                            ui_settings.set(new);
                        }
                    }
                    windows::Message::Keybinds(binds) => {
                        if *keybinds.peek() != *binds {
                            keybinds.set(*binds);
                        }
                    }
                    windows::Message::SelectionGroups(groups) => {
                        if *sel_groups.peek() != groups {
                            filters_on.set(vec![false; groups.len()]);
                            sel_groups.set(groups);
                        }
                    }
                    windows::Message::Themes => themes.set(theme::scan().0),
                    windows::Message::Counters => {
                        counter_data.set(std::rc::Rc::new(counters::Counters::load()));
                    }
                }
            }
        }
    });
    // The other windows share the shortcuts and the selection groups.
    use_effect(move || {
        let binds = keybinds.read().clone();
        windows::broadcast(window_no, || {
            windows::Message::Keybinds(Box::new(binds.clone()))
        });
    });
    use_effect(move || {
        let groups = sel_groups.read().clone();
        windows::broadcast(window_no, || {
            windows::Message::SelectionGroups(groups.clone())
        });
    });
    // Keeps the active tab in view in the strip.
    use_effect(move || {
        let _ = active_tab();
        document::eval(
            "setTimeout(() => document.querySelector('.tab.active')?.scrollIntoView({block: 'nearest', inline: 'nearest'}), 0);",
        );
    });

    // Follow mode (see follow.rs): starts at the start node; while it is on, grid clicks
    // enter the draft's actions instead of editing.
    let mut start_follow = move || {
        extra.set(Vec::new());
        set_mode.set(false);
        marked_only.set(false);
        find_open.set(false);
        role_popup.set(None);
        let f = follow::Follow::start(&tree.peek());
        cursor.set(tree.peek().root());
        follow.set(Some(f));
    };
    // Back to the editor, with the last node the draft followed selected.
    let mut leave_follow = move || {
        let last = follow.peek().as_ref().and_then(|f| f.last_followed());
        follow.set(None);
        if let Some(n) = last.filter(|&n| tree.peek().contains(n)) {
            cursor.set(n);
        }
    };
    // Backspace: takes back the last entered action.
    let mut follow_back = move || {
        let Some(mut f) = follow.peek().clone() else {
            return;
        };
        if f.back() {
            if let Some(n) = f.last_followed() {
                cursor.set(n);
            }
            follow.set(Some(f));
        }
    };

    // Ctrl+click: adds a node to the selection or takes it out. Only nodes at the active
    // node's depth can join it.
    let mut toggle_select = move |id: NodeId| {
        let t = tree.peek();
        if id == t.root() || id == cursor() || !t.contains(id) {
            return;
        }
        let depth = t.path(cursor()).len();
        if t.path(id).len() != depth {
            let step = DRAFT_ORDER
                .get(depth.saturating_sub(2))
                .map_or("the start".to_string(), |s| s.code());
            drop(t);
            show_notice(format!(
                "Only nodes at {step}, like the selected one, can be selected together."
            ));
            return;
        }
        drop(t);
        let mut e = extra.write();
        match e.iter().position(|&x| x == id) {
            Some(i) => {
                e.remove(i);
            }
            None => e.push(id),
        }
    };

    // A selection box: the nodes it touched at one depth. With Ctrl they join the selection;
    // at another depth than the active node, the first of them becomes active.
    let mut box_select = move |nodes: Vec<NodeId>, add: bool| {
        let (active, others) = {
            let t = tree.peek();
            let nodes: Vec<NodeId> = nodes
                .into_iter()
                .filter(|&n| t.contains(n) && n != t.root())
                .collect();
            if nodes.is_empty() {
                drop(t);
                if !add {
                    extra.set(Vec::new());
                }
                return;
            }
            let depth_of = |n: NodeId| t.path(n).len();
            let here = depth_of(cursor());
            let depth = if nodes.iter().any(|&n| depth_of(n) == here) {
                here
            } else {
                nodes.iter().map(|&n| depth_of(n)).min().unwrap()
            };
            let chosen: Vec<NodeId> = nodes
                .into_iter()
                .filter(|&n| depth_of(n) == depth)
                .collect();
            if depth == here {
                let mut others = if add {
                    extra.peek().clone()
                } else {
                    Vec::new()
                };
                for n in chosen {
                    if n != cursor() && !others.contains(&n) {
                        others.push(n);
                    }
                }
                (None, others)
            } else {
                (Some(chosen[0]), chosen[1..].to_vec())
            }
        };
        if let Some(a) = active {
            cursor.set(a);
        }
        extra.set(others);
    };

    // Delete: removes every selected node's branch and selects the active one's parent.
    let mut delete_selection = move || {
        let nodes = selection();
        let Some(parent) = tree.peek().get(nodes[0]).and_then(|n| n.parent()) else {
            return;
        };
        let before = snapshot();
        {
            let mut t = tree.write();
            for n in nodes {
                if t.contains(n) {
                    let _ = t.remove_subtree(n);
                }
            }
        }
        commit(before, ChangeKind::Edit);
        extra.set(Vec::new());
        cursor.set(parent);
    };

    // Ctrl+M: marks the lines ending at the selected nodes, or, when the active node is on
    // a marked line, unmarks them: a marked node loses its mark, a node starred only for a
    // marked line below it loses the marks below it.
    let mut toggle_mark = move || {
        let nodes = selection();
        let before = snapshot();
        let mut changed = false;
        {
            let mut t = tree.write();
            if nodes[0] == t.root() {
                return;
            }
            let starred = t.starred();
            let unmark = starred.contains_key(nodes[0]);
            for n in nodes {
                changed |= if !unmark {
                    t.set_marked(n, true)
                } else if t.get(n).is_some_and(|x| x.is_marked()) {
                    t.set_marked(n, false)
                } else {
                    t.unmark_subtree(n) > 0
                };
            }
        }
        if changed {
            commit(before, ChangeKind::Edit);
        }
    };

    // Ctrl+C / Ctrl+X: copies the selected nodes' branches (and deletes them).
    let mut copy_selection = move |cut: bool| {
        let nodes = selection();
        let copied = clip::Clip::copy(&tree.peek(), &nodes);
        match copied {
            Ok(c) => {
                let (branches, count) = (c.len(), c.nodes());
                windows::set_clipboard(c);
                show_notice(format!(
                    "{} {branches} branch{} ({count} node{}).",
                    if cut { "Cut" } else { "Copied" },
                    if branches == 1 { "" } else { "es" },
                    if count == 1 { "" } else { "s" }
                ));
                if cut {
                    delete_selection();
                }
            }
            Err(e) => show_notice(e.to_string()),
        }
    };

    // The message after a paste or a node drop.
    let pasted_notice = move |what: &str, p: &clip::Pasted| {
        let mut text = format!(
            "{what}: {} node{} added",
            p.added,
            if p.added == 1 { "" } else { "s" }
        );
        if p.merged > 0 {
            text.push_str(&format!(", {} joined matching branches", p.merged));
        }
        if p.tags_dropped > 0 {
            text.push_str(&format!(
                ", {} role tag{} dropped (they don't fit the new line)",
                p.tags_dropped,
                if p.tags_dropped == 1 { "" } else { "s" }
            ));
        }
        text.push('.');
        text
    };

    // Ctrl+V: puts the copied branches below the active node.
    let mut paste_clipboard = move || {
        let Some(c) = windows::clipboard() else {
            show_notice("Nothing copied yet: select nodes and press Ctrl+C.".into());
            return;
        };
        let before = snapshot();
        let at = cursor();
        let result = clip::paste(&mut tree.write(), &c, at);
        match result {
            Ok(p) => {
                // Pasting only onto nodes already there may still have changed their tags.
                if p.added > 0 || !history::unchanged(&before, &tree.peek(), cursor()) {
                    commit(before, ChangeKind::Edit);
                }
                show_notice(pasted_notice("Pasted", &p));
            }
            Err(e) => show_notice(e.to_string()),
        }
    };

    // A node dropped on another: its branch (or every selected one, when it is selected)
    // is copied there, or moved with Shift.
    let mut drop_nodes = move |source: NodeId, target: NodeId, move_them: bool| {
        let nodes = selection();
        let sources = if nodes.contains(&source) {
            nodes
        } else {
            vec![source]
        };
        let before = snapshot();
        let active = cursor();
        let result = clip::copy_or_move(&mut tree.write(), &sources, target, move_them);
        match result {
            Ok(p) => {
                if p.added > 0 || !history::unchanged(&before, &tree.peek(), cursor()) {
                    commit(before, ChangeKind::Edit);
                }
                // A moved active node: select its new place.
                if move_them && !tree.peek().contains(active) {
                    if let Some(i) = sources.iter().position(|&s| s == active)
                        && let Some(&new) = p.tops.get(i)
                    {
                        cursor.set(new);
                    } else {
                        cursor.set(target);
                    }
                }
                extra.set(Vec::new());
                show_notice(pasted_notice(
                    if move_them { "Moved" } else { "Copied" },
                    &p,
                ));
            }
            Err(e) => show_notice(e.to_string()),
        }
    };

    // With only marked lines shown, a selection off them moves up to the last node still
    // shown; with no marks left, the filter turns itself off.
    use_effect(move || {
        if !marked_only() {
            return;
        }
        let t = tree.read();
        if !t.any_marked() {
            drop(t);
            marked_only.set(false);
            return;
        }
        let starred = t.starred();
        let at = cursor();
        if !starred.contains_key(at) {
            let shown = t.ancestors(at).find(|&n| starred.contains_key(n));
            let to = shown.unwrap_or(t.root());
            drop(t);
            cursor.set(to);
        }
    });
    // Whatever moves the selection into hidden branches (a grid click, Shift+X, Find,
    // Shift+click batches) shows the hidden nodes above it, so it is never out of sight.
    use_effect(move || {
        let id = cursor();
        if tree.peek().concealed(id) {
            tree.write().reveal(id);
        }
    });
    let mut delete_node = move |id: NodeId| {
        let parent = tree.read().get(id).and_then(|n| n.parent());
        match parent {
            Some(parent) => {
                let before = snapshot();
                if tree.write().remove_subtree(id).is_ok() {
                    commit(before, ChangeKind::Edit);
                    cursor.set(parent);
                }
            }
            None => cursor.set(id),
        }
    };

    // Ctrl+W: sets the selected node to "No ban" / "Empty", keeping its subtree.
    let mut empty_current = move || {
        if !extra.peek().is_empty() {
            let nodes = selection();
            let before = snapshot();
            let mut changed = false;
            {
                let mut t = tree.write();
                for n in nodes {
                    let old = t.get(n).map(|x| x.value());
                    changed |= t.set_empty(n).is_ok() && t.get(n).map(|x| x.value()) != old;
                }
            }
            if changed {
                commit(before, ChangeKind::Edit);
            }
            return;
        }
        let id = cursor();
        let target = {
            let t = tree.read();
            t.get(id)
                .filter(|n| n.parent().is_some())
                .map(|n| (n.value(), tree::empty_value_for(n.pos())))
        };
        let Some((value, empty)) = target else {
            return;
        };
        if value == empty {
            return;
        }
        let before = snapshot();
        let result = tree.write().set_empty(id);
        match result {
            Ok(()) => commit(before, ChangeKind::Edit),
            Err(e) => error.set(Some(format!("Could not empty this node: {e}."))),
        }
    };

    let mut reset = move || {
        if tree.read().len() <= 1 {
            return;
        }
        let before = snapshot();
        tree.write().clear();
        commit(before, ChangeKind::Edit);
        cursor.set(tree.peek().root());
    };

    // Role tag popup: everything changed while it is open is one undo step.
    let mut open_roles = move |(node, x, y): (NodeId, f64, f64)| {
        if !tree.read().can_tag(node) {
            return;
        }
        role_popup.set(Some(role_popup::RolePopupState {
            node,
            x,
            y,
            before: snapshot(),
        }));
    };
    let mut close_roles = move || {
        let open = role_popup.take();
        if let Some(p) = open
            && !history::unchanged(&p.before, &tree.peek(), cursor())
        {
            commit(p.before, ChangeKind::Edit);
        }
    };
    let mut set_roles = move |mask: u8| {
        let Some(node) = role_popup.peek().as_ref().map(|p| p.node) else {
            return;
        };
        if let Err(e) = tree.write().set_roles(node, mask) {
            eprintln!("cannot set role tags: {e}");
        }
    };

    let mut apply_fearless = move |f: fearless::Fearless| {
        fearless_open.set(false);
        if *tree.read().fearless() == f {
            return;
        }
        let before = snapshot();
        tree.write().set_fearless(f);
        commit(before, ChangeKind::Edit);
    };

    // Deletes every unreachable branch, keeping them for "Restore removed".
    let mut remove_unreachable = move || {
        let path = tree.read().path(cursor());
        let before = snapshot();
        let removed = tree.write().remove_unreachable();
        if removed.is_empty() {
            return;
        }
        commit(before, ChangeKind::Edit);
        let t = tree.read();
        let keep = path.iter().rev().copied().find(|&n| t.contains(n));
        cursor.set(keep.unwrap_or(t.root()));
        pruned.set(Some(removed));
    };

    // Puts the branches of the last "Remove unreachable" back.
    let mut restore_removed = move || {
        let Some(removed) = pruned() else {
            return;
        };
        let before = snapshot();
        let added = tree.write().restore_pruned(&removed);
        if added > 0 {
            commit(before, ChangeKind::Edit);
        }
        pruned.set(None);
    };

    // Deletes every conflicting branch, keeping them for "Restore removed".
    let mut remove_conflicts = move || {
        let path = tree.read().path(cursor());
        let before = snapshot();
        let removed = tree.write().remove_conflicts();
        if removed.is_empty() {
            return;
        }
        commit(before, ChangeKind::Edit);
        let t = tree.read();
        let keep = path.iter().rev().copied().find(|&n| t.contains(n));
        cursor.set(keep.unwrap_or(t.root()));
        pruned_conflicts.set(Some(removed));
    };

    // Puts the branches of the last "Remove conflicting" back.
    let mut restore_conflicts = move || {
        let Some(removed) = pruned_conflicts() else {
            return;
        };
        let before = snapshot();
        let added = tree.write().restore_pruned(&removed);
        if added > 0 {
            commit(before, ChangeKind::Edit);
        }
        pruned_conflicts.set(None);
    };

    // Writes a theme from the settings panel, as itself or as a new theme folder (with a
    // copy of its picture), then selects it.
    let mut save_theme = move |(mut t, as_new): (theme::Theme, bool)| {
        if as_new {
            let old_dir = t.dir();
            t.id = theme::new_id(&t.name);
            let new_dir = t.dir();
            if let Err(e) = std::fs::create_dir_all(&new_dir) {
                error.set(Some(format!("Could not create {}: {e}", new_dir.display())));
                return;
            }
            if let Some(file) = &t.background
                && let Err(e) = std::fs::copy(old_dir.join(file), new_dir.join(file))
            {
                error.set(Some(format!("Could not copy the background picture: {e}")));
                return;
            }
        }
        if let Err(e) = theme::save(&t) {
            error.set(Some(format!("Could not save the theme: {e}")));
            return;
        }
        themes.set(theme::scan().0);
        windows::broadcast(window_no, || windows::Message::Themes);
        ui_settings.write().theme = t.id.clone();
        save_settings();
        theme_editor_open.set(false);
        theme_preview.set(None);
    };

    // Shows `message` under the toolbar for four seconds.

    // Loads a decoded save. Its stored groups join the active config (see
    // `group_import.rs`); when groups are added, the result is written to
    // config/imported_configs/ (or config/temp/ if they don't fit) and becomes the active
    // config. `stem` names that file. The tree opens in a new tab, or in the active one
    // while that is a new tab nobody touched. `source` is the file it came from, and
    // `saved` whether that file holds it as it is (otherwise the tab shows unsaved changes).
    // `prefix` goes in front of the notice (or is the notice when no groups were imported).
    let mut load_tree = move |loaded: save::Loaded,
                              stem: String,
                              prefix: Option<String>,
                              source: Option<PathBuf>,
                              saved: bool| {
        let save::Loaded {
            tree: mut new_tree,
            selected,
        } = loaded;
        let active = tree.peek().groups().clone();
        let stored: Vec<groups::Group> = new_tree.groups().user_groups().cloned().collect();
        let plan = group_import::plan(&active, &stored);
        let catalog = match plan.outcome {
            group_import::Outcome::Unchanged => active.clone(),
            outcome => {
                let folder = if outcome == group_import::Outcome::Merged {
                    group_import::IMPORTED_DIR_NAME
                } else {
                    group_import::TEMP_DIR_NAME
                };
                let dir = configio::config_dir().join(folder);
                match group_import::write_config(&dir, &stem, &plan.groups) {
                    Ok(path) => Arc::new(plan.catalog(Some(path))),
                    Err(e) => {
                        error.set(Some(format!(
                            "Could not save the save file's representational groups to {}: {e}. Nothing was loaded.",
                            dir.display()
                        )));
                        return;
                    }
                }
            }
        };
        new_tree.remap_groups(&plan.map, catalog.clone());
        let selected = selected.unwrap_or(new_tree.root());
        let baseline = saved.then(|| save::content_hash(&new_tree));
        let current = *active_tab.peek();
        let reuse = follow.peek().is_none()
            && tab_list
                .peek()
                .iter()
                .any(|t| t.id == current && t.untouched(&tree.peek()));
        let zoom = (1.0, 1.0);
        if reuse {
            let hash = save::content_hash(&new_tree);
            if let Some(info) = tab_list.write().iter_mut().find(|t| t.id == current) {
                info.name = tabs::tab_name(&new_tree, source.as_deref(), info.number);
                info.dirty = baseline != Some(hash);
                info.source = source;
                info.baseline = baseline;
            }
            show_parked(tabs::Parked::fresh(new_tree, Some(selected), zoom));
        } else {
            open_tab(
                tabs::Parked::fresh(new_tree, Some(selected), zoom),
                source,
                Some(baseline),
            );
        }

        let file = catalog
            .source()
            .map(|p| {
                let folder = p
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                format!("{folder}/{name}")
            })
            .unwrap_or_default();
        let names = |groups: &[groups::Group]| {
            groups
                .iter()
                .map(|g| format!("{} {}", g.label, g.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let lead = prefix.clone().map(|p| p + " ").unwrap_or_default();
        match plan.outcome {
            group_import::Outcome::Unchanged => {
                if let Some(p) = prefix {
                    show_notice(p);
                }
            }
            group_import::Outcome::Merged => {
                let moved = if plan.moved.is_empty() {
                    String::new()
                } else {
                    let list = plan
                        .moved
                        .iter()
                        .map(|&(from, to)| {
                            format!(
                                "G{} → G{}",
                                from - groups::USER_FIRST + 1,
                                to - groups::USER_FIRST + 1
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(" Renumbered: {list}.")
                };
                show_notice(format!(
                    "{lead}Imported {} representational group(s): {}.{moved} Active config: {file}",
                    plan.added.len(),
                    names(&plan.added)
                ));
            }
            group_import::Outcome::FileOnly => {
                if let Some(p) = prefix {
                    show_notice(p);
                }
                warning.set(Some(format!(
                    "This save uses {} representational group(s) that don't fit next to your {} (66 at most). For this session the save's own groups replace yours: {}. They were saved to {file}, which is now the active config. Load RGroup config switches back to your groups.",
                    plan.added.len(),
                    active.user_groups().count(),
                    names(&plan.added)
                )));
            }
        }
    };

    // Files given on the command line (a double-clicked save file) open in tabs.
    use_hook(move || {
        if start.is_some() {
            return;
        }
        for path in STARTUP_FILES.get().into_iter().flatten() {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match save::load(path) {
                Ok(loaded) => load_tree(loaded, stem, None, Some(path.clone()), true),
                Err(e) => error.set(Some(format!("Could not open {}: {e}", path.display()))),
            }
        }
    });

    // Undo and redo change the tree in place, so nodes that stay keep their ids and with
    // them the selection and the last child visited. A step that built a new tree resets
    // those like a load.
    let mut after_step = move |step: Option<history::Step>| {
        let Some(step) = step else {
            return;
        };
        role_popup.set(None);
        cursor.set(step.cursor);
        if step.rebuilt {
            extra.set(Vec::new());
            last_child.set(SecondaryMap::new());
        }
    };

    // Checked first, so an empty stack doesn't mark the tree as changed.
    let mut undo = move || {
        if !history.peek().can_undo() {
            return;
        }
        let at = cursor();
        let step = history.write().undo(&mut tree.write(), at);
        after_step(step);
    };

    let mut redo = move || {
        if !history.peek().can_redo() {
            return;
        }
        let at = cursor();
        let step = history.write().redo(&mut tree.write(), at);
        after_step(step);
    };

    // The tree is encoded right away, so later edits don't leak into a save that is
    // still waiting on its dialog.
    let mut save_as = move |format: SaveFormat| {
        save_menu_open.set(false);
        let selected = save_selection().then_some(cursor());
        let contents = save::to_file_contents(&tree.read(), selected, format);
        let stem = save::default_stem(&tree.read());
        // The tab saved, and its tree as saved: it has no unsaved changes until it differs.
        let tab = *active_tab.peek();
        let baseline = Some(save::content_hash(&tree.read()));
        dialog_open.set(true);
        spawn(async move {
            let ext = format.extension();
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Save draft tree")
                .set_directory(save::saves_dir())
                .add_filter(format.label(), &[ext, &ext.to_uppercase()])
                .set_file_name(format!("{stem}.{ext}"));
            let picked = dialog.save_file().await;
            dialog_open.set(false);
            let Some(handle) = picked else {
                return;
            };
            let path = format.apply_extension(handle.path().to_path_buf());
            if let Err(e) = std::fs::write(&path, contents) {
                error.set(Some(format!("Could not save the file: {e}")));
                return;
            }
            if let Some(info) = tab_list.write().iter_mut().find(|t| t.id == tab) {
                info.source = Some(path);
                info.baseline = baseline;
            }
        });
    };

    let mut load_file = move |_: ()| {
        dialog_open.set(true);
        spawn(async move {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Load draft tree")
                .set_directory(save::saves_dir())
                .add_filter("Recvrve saves", &["dts", "dtt", "DTS", "DTT"])
                .add_filter("All files", &["*"]);
            let picked = dialog.pick_file().await;
            dialog_open.set(false);
            let Some(handle) = picked else {
                return;
            };
            let stem = handle
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match save::load(handle.path()) {
                Ok(loaded) => {
                    load_tree(loaded, stem, None, Some(handle.path().to_path_buf()), true)
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };

    // Merges the picked files (and the current tree if asked), asks where to save the
    // result as .dtt (in saves/, named merged-<date-time>), saves it and loads it.
    let mut run_merge = move |_: ()| {
        let mut inputs = Vec::new();
        // Open tabs, in strip order.
        let chosen = merge_tabs.peek().clone();
        for info in tab_list.peek().iter().filter(|t| chosen.contains(&t.id)) {
            let tab_tree = if info.id == *active_tab.peek() {
                Some(tree.peek().clone())
            } else {
                parked.peek().get(&info.id).map(|p| p.tree.clone())
            };
            if let Some(t) = tab_tree {
                inputs.push(merge::Input {
                    name: format!("the tab \"{}\"", info.name),
                    tree: t,
                });
            }
        }
        for path in merge_files.peek().iter() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match save::load(path) {
                Ok(loaded) => inputs.push(merge::Input {
                    name,
                    tree: loaded.tree,
                }),
                Err(e) => {
                    error.set(Some(format!("Could not merge {name}: {e}")));
                    return;
                }
            }
        }
        let merged = match merge::merge(&inputs) {
            Ok(m) => m,
            Err(e) => {
                error.set(Some(e.to_string()));
                return;
            }
        };
        let text = save::to_text(&merged.tree, None);
        let count = inputs.len();
        dialog_open.set(true);
        spawn(async move {
            let name = format!("merged-{}.dtt", chrono::Local::now().format("%Y%m%d-%H%M"));
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Save merged draft tree")
                .set_directory(save::saves_dir())
                .add_filter(SaveFormat::Text.label(), &["dtt", "DTT"])
                .set_file_name(name)
                .save_file()
                .await;
            dialog_open.set(false);
            // Cancelling the dialog cancels the merge; the panel stays open.
            let Some(handle) = picked else {
                return;
            };
            let path = SaveFormat::Text.apply_extension(handle.path().to_path_buf());
            if let Err(e) = std::fs::write(&path, format!("{text}\n")) {
                error.set(Some(format!("Could not save the merged file: {e}")));
                return;
            }
            let loaded = save::from_text(&text).expect("a merged tree always decodes");
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut summary = format!(
                "Merged {count} trees: {} nodes, {} of {} shared.",
                merged.tree.len() - 1,
                merged.shared_nodes,
                merged.input_nodes
            );
            if merged.tags_dropped > 0 {
                summary.push_str(&format!(
                    " Role tags dropped on {} node(s) where the trees disagreed.",
                    merged.tags_dropped
                ));
            }
            summary.push_str(&format!(" Saved as {}.", path.display()));
            merge_open.set(false);
            merge_files.set(Vec::new());
            load_tree(loaded, stem, Some(summary), Some(path), true);
        });
    };

    // Switches to another group config. Nodes holding the user groups in `only` (or every
    // user group for `None`) are emptied (see `DraftTree::replace_groups_where`). The undo
    // history is cleared when any existing group changed, since old snapshots may use it.
    let mut apply_groups = move |new_groups: Arc<GroupCatalog>, only: Option<Vec<u8>>| {
        let path = tree.read().path(cursor());
        match &only {
            None => {
                tree.write().replace_groups(new_groups);
            }
            Some(values) => {
                tree.write()
                    .replace_groups_where(new_groups, |v| values.contains(&v));
            }
        }
        let selected = {
            let t = tree.read();
            path.iter()
                .rev()
                .copied()
                .find(|&n| t.contains(n))
                .unwrap_or(t.root())
        };
        cursor.set(selected);
        if only.as_ref().is_none_or(|values| !values.is_empty()) {
            history.write().clear();
        }
        last_child.set(SecondaryMap::new());
        // Changed user group values may mean something else now; they get fresh colors.
        group_colors.write().retain(|&v, _| {
            !groups::is_user_group(v) || only.as_ref().is_some_and(|values| !values.contains(&v))
        });
    };

    // Writes the config file (for editor saves) and applies the change.
    let mut commit_groups = move |pending: PendingGroups| {
        if let Some(path) = &pending.write_to {
            let user: Vec<groups::Group> = pending.catalog.user_groups().cloned().collect();
            if let Err(e) = configio::write_with_backup(path, &groups::to_config_json(&user)) {
                error.set(Some(format!("Could not save {}: {e}", path.display())));
                return;
            }
            rgroup_editor_open.set(false);
        }
        apply_groups(pending.catalog, pending.only);
    };

    // Applies a group change straight away, or asks first if tree nodes would be emptied.
    let mut request_groups =
        move |new_groups: Arc<GroupCatalog>, only: Option<Vec<u8>>, write_to: Option<PathBuf>| {
            let affected = match &only {
                None => tree.read().user_group_nodes(),
                Some(values) => tree.read().count_nodes_where(|v| values.contains(&v)),
            };
            let mut pending = PendingGroups {
                catalog: new_groups.clone(),
                message: String::new(),
                only: only.clone(),
                write_to,
            };
            if affected == 0 {
                commit_groups(pending);
                return;
            }
            let mut preview = tree.read().clone();
            let report = match &only {
                None => preview.replace_groups(new_groups),
                Some(values) => preview.replace_groups_where(new_groups, |v| values.contains(&v)),
            };
            let mut message = format!(
                "{affected} node(s) use representational groups that change. {} will be emptied",
                report.emptied
            );
            message.push_str(". The undo history will be cleared.");
            pending.message = message;
            pending_groups.set(Some(pending));
        };

    // Saves from the representational group editor: only groups whose champions changed,
    // or that were deleted, are emptied in the tree.
    let mut finish_rgroup_save = move |user: Vec<groups::Group>, path: PathBuf| {
        let old = tree.peek().groups().clone();
        let new_groups = Arc::new(GroupCatalog::with_user_groups(user, Some(path.clone())));
        let changed: Vec<u8> = old
            .user_groups()
            .filter(|g| {
                new_groups
                    .get(g.value)
                    .is_none_or(|n| n.members != g.members)
            })
            .map(|g| g.value)
            .collect();
        request_groups(new_groups, Some(changed), Some(path));
    };
    let mut save_rgroups = move |(user, save_as): (Vec<groups::Group>, bool)| {
        let default_path = tree
            .peek()
            .groups()
            .source()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| configio::config_dir().join(groups::CONFIG_FILE_NAME));
        if !save_as {
            finish_rgroup_save(user, default_path);
            return;
        }
        dialog_open.set(true);
        spawn(async move {
            let picked = pick_json_save_path("Save representational groups", &default_path).await;
            dialog_open.set(false);
            if let Some(path) = picked {
                finish_rgroup_save(user, path);
            }
        });
    };

    // Saves from the selection group editor: writes the file and applies it, keeping the
    // on/off state of filters that still exist.
    let mut finish_selection_save = move |new_groups: Vec<selection::SelectionGroup>,
                                          origins: Vec<Option<usize>>,
                                          path: PathBuf| {
        if let Err(e) = configio::write_with_backup(&path, &selection::to_config_json(&new_groups))
        {
            error.set(Some(format!("Could not save {}: {e}", path.display())));
            return;
        }
        let old = filters_on.peek().clone();
        filters_on.set(
            origins
                .iter()
                .map(|o| o.and_then(|i| old.get(i).copied()).unwrap_or(false))
                .collect(),
        );
        sel_groups.set(new_groups);
        sel_editor_open.set(false);
    };
    let mut save_selection_groups = move |(new_groups, origins, save_as): (
        Vec<selection::SelectionGroup>,
        Vec<Option<usize>>,
        bool,
    )| {
        let default_path = selection::config_path();
        if !save_as {
            finish_selection_save(new_groups, origins, default_path);
            return;
        }
        dialog_open.set(true);
        spawn(async move {
            let picked = pick_json_save_path("Save selection groups", &default_path).await;
            dialog_open.set(false);
            if let Some(path) = picked {
                finish_selection_save(new_groups, origins, path);
            }
        });
    };

    // Copies a representational group into the selection groups and saves them straight
    // away. The original stays; see `group_copy.rs`.
    let mut copy_to_selection = move |group: groups::Group| {
        let mut list = sel_groups.peek().clone();
        let copy = group_copy::to_selection(&group, &list);
        let name = copy.name.clone();
        list.push(copy);
        let path = selection::config_path();
        if let Err(e) = configio::write_with_backup(&path, &selection::to_config_json(&list)) {
            error.set(Some(format!("Could not save {}: {e}", path.display())));
            return;
        }
        filters_on.write().push(false);
        sel_groups.set(list);
        show_notice(format!(
            "Copied {} {} to the selection groups as \"{name}\".",
            group.label, group.name
        ));
    };

    // Copies a selection group into the representational groups of the active config and
    // saves it straight away. Nothing in the tree changes, so undo is kept.
    let mut copy_to_representational =
        move |(group, omit_groups): (selection::SelectionGroup, bool)| {
            let catalog = tree.peek().groups().clone();
            let copy = match group_copy::to_representational(&group, &catalog, omit_groups) {
                Ok(g) => g,
                Err(e) => {
                    error.set(Some(format!("Could not copy \"{}\": {e}", group.name)));
                    return;
                }
            };
            let path = catalog
                .source()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| configio::config_dir().join(groups::CONFIG_FILE_NAME));
            let mut user: Vec<groups::Group> = catalog.user_groups().cloned().collect();
            user.push(copy.clone());
            if let Err(e) = configio::write_with_backup(&path, &groups::to_config_json(&user)) {
                error.set(Some(format!("Could not save {}: {e}", path.display())));
                return;
            }
            apply_groups(
                Arc::new(GroupCatalog::with_user_groups(user, Some(path))),
                Some(Vec::new()),
            );
            show_notice(format!(
                "Copied \"{}\" to the representational groups as {} \"{}\" ({} champions).",
                group.name,
                copy.label,
                copy.name,
                copy.len()
            ));
        };

    let mut toggle_selection_presets = move || {
        let hide = !ui_settings.peek().hide_selection_presets;
        ui_settings.write().hide_selection_presets = hide;
        if hide {
            // Hidden filters shouldn't keep filtering out of sight.
            let groups = sel_groups.peek();
            let mut on = filters_on.write();
            for (i, g) in groups.iter().enumerate() {
                if selection::is_protected_name(&g.name)
                    && let Some(flag) = on.get_mut(i)
                {
                    *flag = false;
                }
            }
        }
        if let Err(e) = ui_settings.peek().save() {
            eprintln!("could not save settings: {e}");
        }
    };
    let mut toggle_rgroup_presets = move || {
        let hide = !ui_settings.peek().hide_rgroup_presets;
        ui_settings.write().hide_rgroup_presets = hide;
        if let Err(e) = ui_settings.peek().save() {
            eprintln!("could not save settings: {e}");
        }
    };

    let mut load_groups_file = move || {
        dialog_open.set(true);
        spawn(async move {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Load representational groups config")
                .add_filter("Group config", &["json", "JSON"])
                .add_filter("All files", &["*"]);
            let picked = dialog.pick_file().await;
            dialog_open.set(false);
            let Some(handle) = picked else {
                return;
            };
            match groups::load_config(handle.path()) {
                Ok(catalog) => request_groups(Arc::new(catalog), None, None),
                Err(e) => error.set(Some(format!(
                    "Could not load representational groups: {e}. The current groups stay active."
                ))),
            }
        });
    };

    // The tree's string, built only while the share bar shows it (and following the
    // selection only while it is included).
    let share_text_memo = use_memo(move || {
        if !share_open() || fullscreen() {
            return String::new();
        }
        let selected = save_selection().then(|| *cursor.read());
        save::to_text(&tree.read(), selected)
    });

    // Loads the tree as soon as the box holds a valid string.
    let mut on_share_input = move |text: String| {
        match save::from_text(&text) {
            Ok(loaded) => {
                share_draft.set(None);
                // Keep the pasted selection in the string shown back to the user.
                save_selection.set(loaded.selected.is_some());
                let stem = format!("pasted-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
                load_tree(loaded, stem, None, None, false);
            }
            Err(e) => {
                let why = match e {
                    save::LoadError::InvalidGroup(_) | save::LoadError::TooManyNodes(_) => {
                        e.to_string()
                    }
                    _ => "Not a valid tree string".to_string(),
                };
                // Paired with the string the box was showing.
                let shown = share_text_memo.read().clone();
                share_draft.set(Some((text, shown, why)));
            }
        }
    };

    // Shortcut actions sent by the keyboard listener in `keys.rs`.
    let mut on_key = move |action: keys::Action| {
        use keys::Action::*;
        // Follow mode never changes the tree: only actions that don't are let through.
        if follow.peek().is_some() {
            match action {
                Follow => {
                    leave_follow();
                    return;
                }
                FollowBack | Parent => {
                    follow_back();
                    return;
                }
                Save | Copy | ToggleFilter(_) | ClearFilters | ToggleSetMode
                | ToggleRGroupFilter | ToggleFullscreen | Help | CloseOverlays | NewTab
                | NextTab | TabToNewWindow | Load | Merge | Export | ZoomIn | ZoomOut
                | ZoomReset | PanUp | PanDown | PanLeft | PanRight => {}
                _ => {
                    show_notice(format!(
                        "Follow mode doesn't change the tree. Leave it{} to edit.",
                        keys::hint_suffix(Follow)
                    ));
                    return;
                }
            }
        }
        match action {
            NewTab => new_tab(),
            // As their buttons do.
            Load => {
                if !dialog_open() {
                    load_file(());
                }
            }
            Merge => {
                if !dialog_open() {
                    // Closed tabs drop out of the merge.
                    let open: Vec<u64> = tab_list.peek().iter().map(|t| t.id).collect();
                    merge_tabs.write().retain(|id| open.contains(id));
                    merge_open.set(true);
                }
            }
            Export => {
                if tree.peek().len() > 1 && !export_busy() {
                    export_menu_open.set(true);
                }
            }
            ZoomIn => zoom_graph(Some(ZOOM_KEY_STEP)),
            ZoomOut => zoom_graph(Some(1.0 / ZOOM_KEY_STEP)),
            ZoomReset => zoom_graph(None),
            // Moved in the webview (see layout.rs); never sent here.
            PanUp | PanDown | PanLeft | PanRight => {}
            TabToNewWindow => {
                let id = *active_tab.peek();
                move_tab((id, None));
            }
            NextTab => {
                let next = tabs::next_tab(&tab_list.peek(), *active_tab.peek());
                if let Some(n) = next {
                    switch_tab(n);
                }
            }
            Follow => start_follow(),
            // Only in follow mode.
            FollowBack => {}
            EmptyNode => empty_current(),
            Save => {
                if !dialog_open() {
                    save_as(SaveFormat::Compact);
                }
            }
            Undo => undo(),
            Redo => redo(),
            Parent => {
                let parent = tree.read().get(cursor()).and_then(|n| n.parent());
                if let Some(parent) = parent {
                    cursor.set(parent);
                }
            }
            Down => {
                let target = {
                    let t = tree.read();
                    let children = t.get(cursor()).map(|n| n.children()).unwrap_or_default();
                    match children {
                        [only] => Some(*only),
                        _ => last_child
                            .read()
                            .get(cursor())
                            .copied()
                            .filter(|c| children.contains(c)),
                    }
                };
                if let Some(child) = target {
                    cursor.set(child);
                }
            }
            NextBranch | PrevBranch => {
                // Skips hidden nodes; gives up after a full round.
                let t = tree.read();
                let mut at = cursor();
                for _ in 0..t.len() {
                    match t.cycle_same_depth(at, action == NextBranch) {
                        Some(node) if node == cursor() => break,
                        Some(node) if t.concealed(node) => at = node,
                        Some(node) => {
                            drop(t);
                            cursor.set(node);
                            break;
                        }
                        None => break,
                    }
                }
            }
            Fearless => fearless_open.toggle(),
            SetValue => {
                let at_root = cursor() == tree.read().root();
                set_mode.set(!set_mode() && !at_root);
            }
            DuplicateBranch | DuplicateLine | DuplicateLineAndBranch => duplicate(action),
            ToggleHidden => toggle_hidden(),
            Find => open_find(),
            ShowAllHidden => show_all_hidden(),
            HideUnreachable => {
                if tree.peek().unreachable_tops().is_empty() {
                    show_notice("No unreachable branches to hide.".into());
                } else {
                    hide_unreachable();
                }
            }
            RemoveUnreachable => {
                if tree.peek().unreachable().is_empty() {
                    show_notice("No unreachable branches to remove.".into());
                } else {
                    remove_unreachable();
                }
            }
            RestoreUnreachable => {
                if pruned.peek().is_none() {
                    show_notice("Nothing to restore: no unreachable branches were removed.".into());
                } else {
                    restore_removed();
                }
            }
            RemoveConflicting => {
                if tree.peek().conflicts().is_empty() {
                    show_notice("No conflicting branches to remove.".into());
                } else {
                    remove_conflicts();
                }
            }
            RestoreConflicting => {
                if pruned_conflicts.peek().is_none() {
                    show_notice("Nothing to restore: no conflicting branches were removed.".into());
                } else {
                    restore_conflicts();
                }
            }
            Reset => reset(),
            Copy => copy_selection(false),
            Cut => copy_selection(true),
            Paste => paste_clipboard(),
            Mark => toggle_mark(),
            MoveEarlier | MoveLater => {
                let before = snapshot();
                let moved = tree
                    .write()
                    .move_among_siblings(cursor(), if action == MoveLater { 1 } else { -1 });
                if moved {
                    commit(before, ChangeKind::Edit);
                }
            }
            ToggleRole(i) => {
                // Every selected pick follows the active node: the tag goes on, or off.
                let nodes = selection();
                let bit = roles::Role::ALL[i].bit();
                let on = {
                    let t = tree.read();
                    if !t.can_tag(nodes[0]) {
                        return;
                    }
                    t.get(nodes[0]).unwrap().roles() & bit == 0
                };
                let before = snapshot();
                let mut skipped = 0;
                let mut changed = false;
                {
                    let mut t = tree.write();
                    for &n in &nodes {
                        if !t.can_tag(n) {
                            continue;
                        }
                        let roles = t.get(n).unwrap().roles();
                        let mask = if on { roles | bit } else { roles & !bit };
                        if mask == roles {
                            continue;
                        }
                        match t.set_roles(n, mask) {
                            Ok(()) => changed = true,
                            Err(_) => skipped += 1,
                        }
                    }
                }
                if changed {
                    commit(before, ChangeKind::Edit);
                }
                if skipped > 0 {
                    show_notice(format!(
                        "The {} tag doesn't fit {skipped} of the selected picks.",
                        roles::Role::ALL[i].label()
                    ));
                }
            }
            DeleteNode => delete_selection(),
            ToggleFilter(n) => {
                // Counts only the filter buttons that are shown.
                let hide = ui_settings.peek().hide_selection_presets;
                let target = sel_groups
                    .peek()
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| !(hide && selection::is_protected_name(&g.name)))
                    .nth(n)
                    .map(|(i, _)| i);
                if let Some(i) = target
                    && let Some(on) = filters_on.write().get_mut(i)
                {
                    *on = !*on;
                }
            }
            ToggleRGroupFilter => rgroup_filter.toggle(),
            ClearFilters => filters_on.write().fill(false),
            ToggleSetMode => filter_intersect.toggle(),
            ToggleFullscreen => set_fullscreen(!fullscreen()),
            Help => help_open.toggle(),
            CloseOverlays => {
                // Esc first closes whatever is open; with nothing open it leaves fullscreen.
                let anything_open = help_open()
                    || find_open()
                    || set_mode()
                    || merge_open()
                    || fearless_open()
                    || export_menu_open()
                    || export_kind().is_some()
                    || groups_info_open()
                    || pending_groups.read().is_some()
                    || error.read().is_some()
                    || warning.read().is_some()
                    || save_menu_open()
                    || theme_menu_open()
                    || theme_editor_open()
                    || view_menu_open()
                    || role_popup.read().is_some()
                    || rgroup_editor_open()
                    || sel_editor_open()
                    || tab_menu.read().is_some()
                    || window_close_confirm.read().is_some();
                if !anything_open && fullscreen() {
                    set_fullscreen(false);
                    return;
                }
                view_menu_open.set(false);
                warning.set(None);
                help_open.set(false);
                find_open.set(false);
                set_mode.set(false);
                if !dialog_open() {
                    merge_open.set(false);
                }
                fearless_open.set(false);
                export_menu_open.set(false);
                if !export_busy() {
                    export_kind.set(None);
                }
                groups_info_open.set(false);
                pending_groups.set(None);
                error.set(None);
                save_menu_open.set(false);
                theme_menu_open.set(false);
                theme_editor_open.set(false);
                theme_preview.set(None);
                tab_menu.set(None);
                window_close_confirm.set(None);
                close_roles();
            }
        }
    };
    // Box selection and drag and drop on the graph (see drag.rs).
    use_future(move || async move {
        let mut listener = drag::listen();
        while let Some(gesture) = drag::next(&mut listener).await {
            let graph = !matches!(gesture, drag::Gesture::MoveTab { .. });
            if graph && follow.peek().is_some() {
                continue;
            }
            match gesture {
                drag::Gesture::Box { rect, add } => {
                    let nodes = {
                        let layout = graph_layout.peek();
                        let s = ui_settings.peek();
                        let (w, h) = *tree_size.peek();
                        let zoom = if s.graph_vertical {
                            s.graph_zoom_v
                        } else {
                            s.graph_zoom_h
                        };
                        let geom = tree_view::graph_geom(
                            s.graph_vertical,
                            zoom,
                            w,
                            h,
                            layout.placed[0].row,
                        );
                        tree_view::nodes_in_rect(&layout, &geom, rect)
                    };
                    box_select(nodes, add)
                }
                drag::Gesture::DropNodes {
                    source,
                    target,
                    move_them,
                } => drop_nodes(source, target, move_them),
                drag::Gesture::MoveTab { from, to, after } => {
                    tabs::move_tab(&mut tab_list.write(), from, to, after);
                }
                drag::Gesture::DropValue { value, target } => {
                    // The node dropped on becomes the active one and takes the value.
                    if tree.peek().contains(target) && target != tree.peek().root() {
                        extra.set(Vec::new());
                        cursor.set(target);
                        set_value_to(value);
                    }
                }
            }
        }
    });
    use_future(move || async move {
        let mut listener = keys::listen();
        while let Some(event) = keys::next(&mut listener).await {
            match event {
                keys::Event::Action(action) => on_key(action),
                keys::Event::BranchRelease => {
                    // Only a node that still exists: the tree can't change while Shift is
                    // held, since every other shortcut is off then, but stay safe.
                    if let Some(target) = branch_target.take()
                        && tree.peek().get(target).is_some()
                    {
                        // The other selected nodes move to their child with the same value
                        // (the newest one, for empty values).
                        let moved: Vec<NodeId> = {
                            let t = tree.peek();
                            let value = t.get(target).unwrap().value();
                            extra
                                .peek()
                                .iter()
                                .filter_map(|&e| {
                                    t.get(e)?
                                        .children()
                                        .iter()
                                        .rev()
                                        .copied()
                                        .find(|&c| t.get(c).is_some_and(|n| n.value() == value))
                                })
                                .collect()
                        };
                        extra.set(moved);
                        cursor.set(target);
                    }
                }
                keys::Event::RecordCancelled => recording.set(None),
                keys::Event::FindStep(forward) => find_step(forward),
                keys::Event::RecordFailed(m) => kb_message.set(Some(m)),
                keys::Event::Recorded(combo) => {
                    let Some((action, slot)) = recording() else {
                        continue;
                    };
                    let result = keybinds.write().assign(action, slot, combo.clone());
                    match result {
                        Ok(moved) => {
                            recording.set(None);
                            let binds = keybinds.peek();
                            kb_message.set(
                                moved.map(|from| {
                                    shortcuts_panel::moved_message(&binds, &combo, from)
                                }),
                            );
                            shortcuts_panel::persist(&binds, kb_message);
                        }
                        // Stays recording, so the user can try another combo.
                        Err(e) => kb_message.set(Some(e)),
                    }
                }
            }
        }
    });
    // Scripts in the webview check the animation profile too (smooth scrolling, zoom).
    use_effect(move || {
        let smooth = ui_settings.read().smooth_animations;
        SMOOTH.store(smooth, std::sync::atomic::Ordering::Relaxed);
        document::eval(&format!("window.__recvrveSmooth = {smooth};"));
    });
    // A keybinds file with problems: say so once at startup.
    use_hook(move || {
        if let Some(m) = kb_loaded.1.clone() {
            show_notice(m);
        }
    });

    // Classes on the app root for fullscreen and hidden panels (see style.css).
    let hidden = settings.hidden;
    let view_class = [
        (fullscreen(), "fullscreen"),
        (following, "following"),
        (settings.graph_vertical, "vertical"),
        (settings.graph_vertical && settings.layout.v_auto, "v-auto"),
        (!settings.graph_vertical && settings.layout.h_auto, "h-auto"),
        (hidden.teams, "hide-teams"),
        (hidden.pool, "hide-pool"),
        (hidden.groups, "hide-groups"),
        (hidden.tree, "hide-tree"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, c)| *c)
    .collect::<Vec<_>>()
    .join(" ");
    // The smooth animation profile; without it the app looks and moves as it always did.
    let anim_class = if settings.smooth_animations {
        "anim-smooth"
    } else {
        ""
    };
    let full = fullscreen();
    // Splitters only between two panels that are both shown. In fullscreen the pool
    // splitter sits between the pool and the groups column, the groups splitter between
    // that row and the graph.
    // Top to bottom, the middle column is arranged like fullscreen.
    let grid = full || settings.graph_vertical;
    let pool_split = if grid {
        !hidden.pool && !hidden.groups
    } else {
        !hidden.pool && (!hidden.groups || !hidden.tree)
    };
    let groups_split = if grid {
        (!hidden.pool || !hidden.groups) && !hidden.tree
    } else {
        !hidden.groups && !hidden.tree
    };

    // Shared with the panels (see ui.rs): the state, derived data and actions.
    let act = ui::Act {
        follow_enter: use_callback(move |v: u8| follow_enter(v)),
        show_notice: use_callback(move |m: String| show_notice(m)),
        update_counters: use_callback(move |()| {
            if counter_update.peek().is_some() {
                return;
            }
            counter_update.set(Some((0, 1)));
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            std::thread::spawn(move || {
                let steps = tx.clone();
                let result = counters::download(move |p| {
                    let _ = steps.send(p);
                });
                let _ = tx.send(match result {
                    Ok(s) => counters::Progress::Done(Box::new(s)),
                    Err(e) => counters::Progress::Failed(e),
                });
            });
            spawn(async move {
                while let Some(p) = rx.recv().await {
                    match p {
                        counters::Progress::Step(n, total) => counter_update.set(Some((n, total))),
                        counters::Progress::Done(s) => {
                            counter_update.set(None);
                            match counters::save(&s) {
                                Ok(_) => {
                                    counter_data.set(std::rc::Rc::new(counters::Counters::load()));
                                    windows::broadcast(window_no, || windows::Message::Counters);
                                    show_notice(format!("Counter data updated: {}", s.describe()));
                                }
                                Err(e) => show_notice(format!(
                                    "The counter data was downloaded but couldn't be saved: {e}"
                                )),
                            }
                            break;
                        }
                        counters::Progress::Failed(e) => {
                            counter_update.set(None);
                            show_notice(format!("Updating the counter data failed: {e}"));
                            break;
                        }
                    }
                }
            });
        }),
        save_settings: use_callback(move |()| save_settings()),
        set_easter_eggs: use_callback(move |on: bool| set_easter_eggs(on)),
        set_hidden: use_callback(move |(p, v): (Panel, bool)| set_hidden(p, v)),
        set_fullscreen: use_callback(move |on: bool| set_fullscreen(on)),
        choose: use_callback(move |v: u8| choose(v)),
        add_branch: use_callback(move |v: u8| add_branch(v)),
        find_step: use_callback(move |f: bool| find_step(f)),
        hover_node: use_callback(move |h: Option<(NodeId, f64, f64)>| hover_node(h)),
        toggle_hidden: use_callback(move |()| toggle_hidden()),
        show_all_hidden: use_callback(move |()| show_all_hidden()),
        hide_unreachable: use_callback(move |()| hide_unreachable()),
        switch_tab: use_callback(move |id: u64| switch_tab(id)),
        new_tab: use_callback(move |()| new_tab()),
        close_tab: use_callback(move |id: u64| close_tab(id)),
        request_close: use_callback(move |id: u64| request_close(id)),
        move_tab: use_callback(move |target: (u64, Option<u64>)| move_tab(target)),
        close_window: use_callback(move |_| close_window_now()),
        start_follow: use_callback(move |()| start_follow()),
        leave_follow: use_callback(move |()| leave_follow()),
        follow_back: use_callback(move |()| follow_back()),
        toggle_select: use_callback(move |id: NodeId| toggle_select(id)),
        delete_node: use_callback(move |id: NodeId| delete_node(id)),
        reset: use_callback(move |()| reset()),
        open_roles: use_callback(move |r: (NodeId, f64, f64)| open_roles(r)),
        close_roles: use_callback(move |()| close_roles()),
        set_roles: use_callback(move |m: u8| set_roles(m)),
        apply_fearless: use_callback(move |f: fearless::Fearless| apply_fearless(f)),
        remove_unreachable: use_callback(move |()| remove_unreachable()),
        restore_removed: use_callback(move |()| restore_removed()),
        remove_conflicts: use_callback(move |()| remove_conflicts()),
        restore_conflicts: use_callback(move |()| restore_conflicts()),
        save_theme: use_callback(move |t: (theme::Theme, bool)| save_theme(t)),
        load_tree: use_callback(
            move |(l, stem, prefix, source, saved): (
                save::Loaded,
                String,
                Option<String>,
                Option<PathBuf>,
                bool,
            )| load_tree(l, stem, prefix, source, saved),
        ),
        save_as: use_callback(move |f: SaveFormat| save_as(f)),
        load_file: use_callback(move |()| load_file(())),
        run_merge: use_callback(move |()| run_merge(())),
        commit_groups: use_callback(move |p: PendingGroups| commit_groups(p)),
        save_rgroups: use_callback(move |r: (Vec<groups::Group>, bool)| save_rgroups(r)),
        save_selection_groups: use_callback(
            move |r: (Vec<selection::SelectionGroup>, Vec<Option<usize>>, bool)| {
                save_selection_groups(r)
            },
        ),
        copy_to_selection: use_callback(move |g: groups::Group| copy_to_selection(g)),
        copy_to_representational: use_callback(move |c: (selection::SelectionGroup, bool)| {
            copy_to_representational(c)
        }),
        toggle_selection_presets: use_callback(move |()| toggle_selection_presets()),
        toggle_rgroup_presets: use_callback(move |()| toggle_rgroup_presets()),
        load_groups_file: use_callback(move |()| load_groups_file()),
        on_share_input: use_callback(move |t: String| on_share_input(t)),
    };
    use_context_provider(|| ui::Ctx {
        tree,
        cursor,
        extra,
        follow,
        tab_list,
        active_tab,
        parked,
        close_confirm,
        window_no,
        tab_menu,
        window_close_confirm,
        search,
        save_menu_open,
        save_selection,
        dialog_open,
        notice,
        notice_leaving,
        warning,
        merge_open,
        merge_files,
        merge_tabs,
        error,
        sel_groups,
        filters_on,
        filter_intersect,
        rgroup_filter,
        ui_settings,
        tree_size,
        easter_eggs,
        fullscreen,
        view_menu_open,
        rgroup_editor_open,
        sel_editor_open,
        themes,
        theme_preview,
        theme_menu_open,
        theme_editor_open,
        active_theme,
        share_open,
        share_draft,
        help_open,
        keybinds,
        recording,
        kb_message,
        export_menu_open,
        export_kind,
        export_busy,
        fearless_open,
        role_popup,
        edit_unreachable,
        pruned,
        pruned_conflicts,
        set_mode,
        group_colors,
        pending_groups,
        groups_info_open,
        find_open,
        find_query,
        find_options,
        found,
        preview,
        marked_only,
        restore_offer,
        share_text: share_text_memo,
        info: tree_info,
        graph: graph_layout,
        entering,
        counter_data,
        counter_update,
        graph_center,
        line: line_info,
        act,
    });

    rsx! {
        style { {font_face_css()} }
        style { {STYLE} }
        div {
            class: if easter_eggs() { "app {theme} {bg_class} {view_class} {anim_class} eggs-on" } else { "app {theme} {bg_class} {view_class} {anim_class}" },
            id: "app-root",
            style: "{theme_css} {settings.layout.css(settings.graph_vertical)} --tile: {settings.tile_size}px;",
            ui::top_bar::TopBar {}
            ui::share_bar::ShareBar {}
            main { class: "board",
                ui::teams::TeamColumn { team: Team::Blue }
                div { class: if hidden.tree { "center no-tree" } else { "center" },
                ui::pool::ChampionPool {}
                if pool_split {
                    div { class: "splitter row", "data-split": "pool", title: SPLITTER_TIP }
                }
                ui::groups_panel::GroupsPanel {}
                if groups_split {
                    div { class: "splitter row", "data-split": "groups", title: SPLITTER_TIP }
                }
                ui::tree_panel::TreePanel {}
                }
                ui::teams::TeamColumn { team: Team::Red }
            }
            if !full {
                OrderStrip {
                    step,
                    patch: tree.read().patch().to_string(),
                    on_patch: move |p: String| {
                        if follow.peek().is_none() && tree.peek().patch() != p.trim() {
                            tree.write().set_patch(&p);
                        }
                    },
                    // The fearless series indicator (game and locked champions), while active.
                    fearless: fearless_state.is_active().then(|| (fearless_state.game, fearless_state.count())),
                    on_fearless: move |_| {
                        if follow.peek().is_none() {
                            fearless_open.set(true);
                        }
                    },
                    smooth: settings.smooth_animations,
                }
            }
            ui::overlays::Overlays {}
        }
    }
}

/// The draft state at a node, as the team panels show it.
pub(crate) struct LineData {
    /// Values chosen so far, in draft order (root excluded).
    pub values: Vec<u8>,
    /// Nodes on the line (root excluded), one per filled step.
    pub ids: Vec<NodeId>,
    /// Their role tags.
    pub tags: Vec<u8>,
    /// Per team, per role, the values its role slot shows: picks that can play that role in
    /// some valid way of giving every pick a different role (untagged picks count as any
    /// role).
    pub role_slots: [Vec<Vec<u8>>; 2],
}

/// The draft state at `at`.
pub(crate) fn line_data(t: &DraftTree, at: NodeId) -> LineData {
    let ids: Vec<NodeId> = t.path(at).into_iter().skip(1).collect();
    let values = ids.iter().map(|&id| t.get(id).unwrap().value()).collect();
    let tags: Vec<u8> = ids.iter().map(|&id| t.get(id).unwrap().roles()).collect();
    let role_slots = t.team_picks(at).map(|team| {
        let picks: Vec<(u8, u8)> = team
            .iter()
            .map(|&(id, m)| (t.get(id).unwrap().value(), m))
            .collect();
        role_slots_of(&picks)
    });
    LineData {
        values,
        ids,
        tags,
        role_slots,
    }
}

/// Per role (top ... support), the values of one team's picks (value, role tags) that its
/// role slot shows.
fn role_slots_of(picks: &[(u8, u8)]) -> Vec<Vec<u8>> {
    let masks: Vec<u8> = picks.iter().map(|&(_, m)| m).collect();
    let candidates = roles::candidates(&masks);
    roles::Role::ALL
        .iter()
        .map(|role| {
            picks
                .iter()
                .zip(&candidates)
                // Untagged empty picks have no champion to show.
                .filter(|&(&(v, m), &c)| c & role.bit() != 0 && (v != EMPTY || m != 0))
                .map(|(&(v, _), _)| v)
                .collect()
        })
        .collect()
}

/// The draft entered in follow mode, as the team panels show it: the values entered, with
/// the role tags of the best fitting prepared node at each step.
pub(crate) fn follow_line(t: &DraftTree, f: &follow::Follow) -> LineData {
    let values = f.entered.clone();
    let best: Vec<Option<NodeId>> = (1..=values.len())
        .map(|k| f.sets[k].first().copied())
        .collect();
    let ids: Vec<NodeId> = best.iter().map(|b| b.unwrap_or(t.root())).collect();
    let tags: Vec<u8> = best
        .iter()
        .map(|b| b.and_then(|id| t.get(id)).map_or(0, |n| n.roles()))
        .collect();
    let role_slots = [Team::Blue, Team::Red].map(|team| {
        let picks: Vec<(u8, u8)> = values
            .iter()
            .zip(&tags)
            .zip(DRAFT_ORDER.iter())
            .filter(|(_, s)| s.team == team && s.action == Action::Pick)
            .map(|((&v, &m), _)| (v, m))
            .collect();
        role_slots_of(&picks)
    });
    LineData {
        values,
        ids,
        tags,
        role_slots,
    }
}

#[component]
pub(crate) fn TeamPanel(
    team: Team,
    values: Vec<u8>,
    step: usize,
    /// Nodes of the selected line, one per filled step.
    ids: Vec<NodeId>,
    /// Their role tags.
    tags: Vec<u8>,
    /// Per role (top ... support), the values its slot shows.
    role_slots: Vec<Vec<u8>>,
    on_roles: EventHandler<(NodeId, f64, f64)>,
    /// Per step, whether its node was just added (smooth profile): its icon slides in
    /// from the team's side instead of popping in.
    #[props(default)]
    added: Vec<bool>,
    /// Per step, the counter picks its champion pick shows instead of its name (see
    /// [`SlotCounters`]); none keeps the names (as in the node preview).
    #[props(default)]
    counters: Option<Vec<Option<SlotCounters>>>,
    /// The team's name; empty for "Blue Team" / "Red Team".
    #[props(default)]
    name: String,
    /// Double-clicking the title edits the name (when given).
    #[props(default)]
    on_rename: Option<EventHandler<String>>,
) -> Element {
    let mut editing = use_signal(|| false);
    let input_id = format!("team-name-{}", team.class());
    let title = if name.is_empty() {
        format!("{} Team", team.label())
    } else {
        name.clone()
    };
    // Draft step indices for this team's bans or picks, in slot order.
    let slots = |action: Action| -> Vec<usize> {
        let mut steps: Vec<(usize, u8)> = DRAFT_ORDER
            .iter()
            .enumerate()
            .filter(|(_, s)| s.team == team && s.action == action)
            .map(|(i, s)| (i, s.number))
            .collect();
        steps.sort_by_key(|&(_, n)| n);
        steps.into_iter().map(|(i, _)| i).collect()
    };
    let bans = slots(Action::Ban);
    let picks = slots(Action::Pick);

    rsx! {
        aside { class: "team {team.class()}",
            if editing() {
                input {
                    id: "{input_id}",
                    class: "team-name-input",
                    maxlength: "{tree::MAX_NAME_CHARS}",
                    placeholder: "{team.label()} Team",
                    initial_value: "{name}",
                    onkeydown: move |e: KeyboardEvent| {
                        match e.key() {
                            Key::Enter => {
                                document::eval(&format!("document.getElementById('team-name-{}')?.blur()", team.class()));
                            }
                            Key::Escape => editing.set(false),
                            _ => {}
                        }
                    },
                    onchange: move |e| {
                        if let Some(rename) = on_rename {
                            rename.call(e.value());
                        }
                    },
                    onblur: move |_| editing.set(false),
                }
            } else {
                h2 {
                    class: "team-name",
                    title: if on_rename.is_some() { "Double-click to name the team" } else { "" },
                    ondoubleclick: move |_| {
                        if on_rename.is_some() {
                            editing.set(true);
                            document::eval(&format!(
                                "(function f(n) {{ const i = document.getElementById('team-name-{}'); if (i) {{ i.focus(); i.select(); }} else if (n > 0) setTimeout(() => f(n - 1), 30); }})(20);",
                                team.class()
                            ));
                        }
                    },
                    "{title}"
                }
            }
            div { class: "bans",
                for (n, idx) in bans.into_iter().enumerate() {
                    Slot {
                        key: "ban{idx}",
                        step: idx,
                        value: values.get(idx).copied(),
                        active: idx == step,
                        label: format!("B{}", n + 1),
                        node: None,
                        tags: 0,
                        added: added.get(idx).copied().unwrap_or(false),
                        on_roles,
                    }
                }
            }
            div { class: "picks",
                for (n, idx) in picks.into_iter().enumerate() {
                    Slot {
                        key: "pick{idx}",
                        step: idx,
                        value: values.get(idx).copied(),
                        active: idx == step,
                        label: format!("Pick {}", n + 1),
                        node: ids.get(idx).copied(),
                        tags: tags.get(idx).copied().unwrap_or(0),
                        added: added.get(idx).copied().unwrap_or(false),
                        counters: counters.as_ref().and_then(|c| c.get(idx).cloned().flatten()),
                        on_roles,
                    }
                }
            }
            RoleSlots { slots: role_slots }
        }
    }
}

/// The counter picks a champion pick shows (see `counters`): the lane they are for (none
/// when the data has nothing for its role tags) and the counters left after the
/// unavailable champions.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SlotCounters {
    pub lane: Option<roles::Role>,
    pub shown: Vec<counters::Shown>,
    /// Where the data comes from, for the tooltips.
    pub source: String,
}

/// The counter picks beside a pick: champions that beat it, as small tiles with the pick's
/// win rate against each. Two columns are planned: counters from any role (waiting for a
/// data source, so hidden for now) and counters in the pick's lane.
#[component]
fn CounterTiles(pick: u8, data: SlotCounters) -> Element {
    let pick_name = champions::by_value(pick).map_or("", |c| c.name);
    let lane = data.lane.map_or("", |r| r.label());
    let empty = match data.lane {
        None => format!("No counter data for {pick_name} in its roles"),
        Some(_) => format!(
            "No counter for {pick_name} ({lane}) with {}+ games left",
            counters::MIN_GAMES
        ),
    };
    rsx! {
        div { class: "counters",
            div { class: "counter-col lane",
                if data.shown.is_empty() {
                    div { class: "counter-none", title: "{empty}\n{data.source}", "No counters" }
                }
                for c in data.shown.iter() {
                    if let Some(info) = champions::by_value(c.value) {
                        div {
                            key: "{c.value}",
                            class: "counter",
                            title: "{info.name} counters {pick_name} ({lane}): {pick_name} wins {c.pick_win_rate():.1}% of {c.games} games\n{data.source}",
                            img { src: "{icon_url(info)}", alt: "{info.name}", draggable: "false" }
                            span { class: "counter-wr", "{c.pick_win_rate():.0}%" }
                        }
                    }
                }
            }
        }
    }
}

/// A filled slot's icon. It enters as it mounts: popping in, or (smooth profile) sliding
/// in from its team's side when its node was just added. Which one is fixed at mount, so
/// a later render never restarts it.
#[component]
fn SlotIcon(value: u8, tags: u8, added: bool, name: String) -> Element {
    let added = use_hook(|| added);
    rsx! {
        div { class: if added { "slot-icon added" } else { "slot-icon" },
            // Hovering the icon names its champion.
            title: "{name}",
            ValueIcon { value }
            RoleIcons { mask: tags }
        }
    }
}

#[component]
fn Slot(
    step: usize,
    value: Option<u8>,
    active: bool,
    label: String,
    /// The node in this slot on the selected line (picks only), for role tags.
    node: Option<NodeId>,
    tags: u8,
    /// The slot's node was just added (see `TeamPanel::added`).
    #[props(default)]
    added: bool,
    /// The counter picks shown instead of the champion's name (picks only).
    #[props(default)]
    counters: Option<SlotCounters>,
    on_roles: EventHandler<(NodeId, f64, f64)>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let pos = DRAFT_ORDER[step].pos();
    let ban = pos.action() == Action::Ban;
    let kind = if ban { "ban" } else { "pick" };
    let state = if active {
        "active"
    } else if value.is_some() {
        "filled"
    } else {
        ""
    };
    let (name, tooltip) = match value {
        Some(v) => {
            let tree = ctx.tree.read();
            let groups = tree.groups();
            let tooltip = match groups.get(v) {
                Some(g) => format!("{}\n{}", group_tooltip(g), node_debug(pos, v)),
                None => format!("{}\n{}", value_name(groups, v), node_debug(pos, v)),
            };
            let name = match groups.get(v) {
                Some(g) => g.name.clone(),
                None => value_name(groups, v),
            };
            (name, tooltip)
        }
        None => (String::new(), String::new()),
    };
    let muted = value.is_some_and(|v| v == EMPTY || v == NO_BAN);
    // A ban that actually removed a champion or group gets the red ban sign.
    let banned = if ban && value.is_some() && !muted {
        "banned"
    } else {
        ""
    };

    rsx! {
        div {
            class: "slot {kind} {state} {banned}",
            title: if node.is_some() { format!("{tooltip}\nRoles: {}\nMiddle-click: edit role tags", roles::describe(tags)) } else { tooltip },
            onmousedown: move |e: MouseEvent| {
                if let Some(id) = node
                    && e.trigger_button() == Some(dioxus::html::input_data::MouseButton::Auxiliary)
                {
                    e.prevent_default();
                    let p = e.client_coordinates();
                    on_roles.call((id, p.x, p.y));
                }
            },
            match value {
                Some(v) => rsx! {
                    SlotIcon { value: v, tags, added, name: name.clone() }
                    if !ban {
                        match counters {
                            Some(data) => rsx! { CounterTiles { pick: v, data } },
                            None => rsx! {
                                span { class: if muted { "slot-name muted" } else { "slot-name" }, "{name}" }
                            },
                        }
                    }
                },
                None => rsx! {
                    span { class: "slot-label", "{label}" }
                },
            }
        }
    }
}

/// The role slots at the bottom of a team panel: one per role, showing the picks that can
/// play it. Two share a slot along a diagonal; three to five get pie wedges.
#[component]
fn RoleSlots(slots: Vec<Vec<u8>>) -> Element {
    let ctx = use_context::<ViewContext>();
    let names = |vals: &[u8]| {
        let t = ctx.tree.read();
        vals.iter()
            .map(|&v| value_name(t.groups(), v))
            .collect::<Vec<_>>()
            .join(", ")
    };
    rsx! {
        div { class: "role-slots",
            for (role, vals) in roles::Role::ALL.into_iter().zip(slots) {
                {
                    let n = vals.len().min(roles::MAX_SPLIT);
                    let more = vals.len().saturating_sub(roles::MAX_SPLIT);
                    let title = if vals.is_empty() {
                        format!("{}: nobody yet", role.label())
                    } else {
                        format!("{}: {}", role.label(), names(&vals))
                    };
                    rsx! {
                        div { key: "{role.key()}", class: "role-col", title: "{title}",
                            div { class: "role-head", RoleGlyph { role } }
                            div { class: if n == 0 { "role-slot empty" } else { "role-slot" },
                                for (k, v) in vals.iter().copied().take(n).enumerate() {
                                    {
                                        let (dx, dy) = roles::band_shift(k, n);
                                        rsx! {
                                            div {
                                                key: "{k}",
                                                class: "role-band",
                                                style: "clip-path: {roles::band_css(k, n)};",
                                                div {
                                                    class: "role-band-shift",
                                                    style: "transform: translate({dx * 100.0:.1}%, {dy * 100.0:.1}%);",
                                                    ValueIcon { value: v }
                                                }
                                            }
                                        }
                                    }
                                }
                                if n > 1 {
                                    svg {
                                        class: "role-split",
                                        view_box: "0 0 100 100",
                                        preserve_aspect_ratio: "none",
                                        for (i, (x1, y1, x2, y2)) in roles::split_lines(n).into_iter().enumerate() {
                                            line {
                                                key: "{i}",
                                                x1: "{x1 * 100.0}",
                                                y1: "{y1 * 100.0}",
                                                x2: "{x2 * 100.0}",
                                                y2: "{y2 * 100.0}",
                                            }
                                        }
                                    }
                                }
                                if more > 0 {
                                    span { class: "role-more", "+{more}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Read-only list of every group and its champions.
#[component]
fn GroupsInfo(
    /// Copy the group with this value into the selection groups.
    on_copy: EventHandler<u8>,
    on_close: EventHandler<()>,
) -> Element {
    let ctx = use_context::<ViewContext>();
    let tree = ctx.tree.read();
    let groups = tree.groups();
    let source = groups
        .source()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "no user group config loaded".to_string());
    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| on_close.call(()),
            div {
                class: "modal groups-info",
                onclick: move |e| e.stop_propagation(),
                button { class: "modal-close", title: "Close", onclick: move |_| on_close.call(()), "×" }
                h2 { class: "help-title", "Representational groups" }
                p { class: "help-note", "User groups from: {source}" }
                div { class: "groups-info-list",
                    for g in groups.all() {
                        div { key: "{g.value}", class: "groups-info-row",
                            div { class: "groups-info-icon", ValueIcon { value: g.value } }
                            div { class: "groups-info-text",
                                div { class: "groups-info-name",
                                    "{g.name} "
                                    span { class: "muted", "{g.label} · value {g.value} · {g.len()} champion(s)" }
                                }
                                if !g.note.is_empty() {
                                    div { class: "muted", "{g.note}" }
                                }
                                div { class: "groups-info-members", "{g.member_names().join(\", \")}" }
                            }
                            {
                                let value = g.value;
                                rsx! {
                                    button {
                                        class: "btn small groups-info-copy",
                                        title: "Add a copy of this group to the selection groups (the role lock is not copied)",
                                        onclick: move |_| on_copy.call(value),
                                        "Copy to selection groups"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Compact strip showing the full draft order with the current step highlighted.
#[component]
fn OrderStrip(
    step: usize,
    patch: String,
    on_patch: EventHandler<String>,
    /// Game and locked champion count of an active fearless series.
    fearless: Option<(u8, usize)>,
    on_fearless: EventHandler<()>,
    /// Smooth animations: a highlight slides from step to step (placed by the layout
    /// script, which knows where the steps are).
    smooth: bool,
) -> Element {
    use_effect(use_reactive!(|step, smooth| {
        let _ = step;
        if smooth {
            document::eval("window.__recvrvePlaceStep && window.__recvrvePlaceStep();");
        }
    }));
    rsx! {
        footer { class: "order",
            if let Some((game, count)) = fearless {
                button {
                    class: "fl-badge-top fl-corner",
                    title: format!("Fearless series: game {game}, {count} champion(s) from earlier games. Click to edit{}.", keys::hint_suffix(keys::Action::Fearless)),
                    onclick: move |_| on_fearless.call(()),
                    "G{game} · {count}"
                }
            }
            // The game patch the prep is for, saved with the tree.
            input {
                // A new key when the patch changes from elsewhere (a load) resets the field.
                key: "{patch}",
                class: "patch-input",
                maxlength: "{tree::MAX_PATCH_CHARS}",
                placeholder: "Patch",
                title: "The game patch this prep is for (saved with the tree)",
                initial_value: "{patch}",
                onchange: move |e| on_patch.call(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        document::eval("document.activeElement?.blur()");
                    }
                },
            }
            for (i, s) in DRAFT_ORDER.iter().enumerate() {
                div {
                    key: "{i}",
                    class: "order-step {s.team.class()} {s.action.class()}",
                    class: if i == step { "current" },
                    class: if i < step { "done" },
                    title: "{s.team.label()} {s.action.label()} {s.number}",
                    "{s.code()}"
                }
            }
            if smooth {
                div { class: "order-cursor" }
            }
        }
    }
}
