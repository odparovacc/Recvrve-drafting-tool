//! Shared config file handling: finding the config folder, safe writes with a `.bak`
//! backup, startup loading with fallbacks, and the small persisted UI settings file.
//!
//! Startup loads try, in order: the config file itself, its `.bak` backup (the previous
//! version, kept by every save), and the shipped copy in `config/defaults/`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::groups::{CONFIG_DIR_NAME, ConfigError};

pub const DEFAULTS_DIR_NAME: &str = "defaults";
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// A file next to the executable that marks an installed copy (the Windows installer puts
/// it there). An installed copy keeps the user's files in per-user folders, since the
/// install folder (e.g. Program Files) can't be written to:
///
/// - `config` in the user's settings folder (`%APPDATA%\Recvrve\config` on Windows),
///   seeded from the shipped `config` next to the executable on first start;
/// - `saves`, `exports` (and autosaves) in `Documents\Recvrve`.
///
/// Without it (running from source, or a copy unpacked anywhere), everything stays next to
/// the working directory or the executable, as before.
pub const INSTALLED_MARKER: &str = "recvrve-installed";
const APP_DIR_NAME: &str = "Recvrve";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The per-user folders of an installed copy.
struct Installed {
    config: PathBuf,
    documents: PathBuf,
}

fn installed() -> Option<&'static Installed> {
    static INSTALLED: OnceLock<Option<Installed>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            let exe = exe_dir()?;
            if !exe.join(INSTALLED_MARKER).is_file() {
                return None;
            }
            let config = dirs::config_dir()?.join(APP_DIR_NAME).join(CONFIG_DIR_NAME);
            let documents = dirs::document_dir()
                .or_else(dirs::home_dir)?
                .join(APP_DIR_NAME);
            if let Err(e) = seed(&exe.join(CONFIG_DIR_NAME), &config) {
                eprintln!(
                    "could not copy the shipped config to {}: {e}",
                    config.display()
                );
            }
            Some(Installed { config, documents })
        })
        .as_ref()
}

/// Copies the shipped config folder `from` into the user's `to`: files the user doesn't
/// have yet, and always the `defaults` folder (so updates bring new defaults). The user's
/// own files are never overwritten.
fn seed(from: &Path, to: &Path) -> std::io::Result<()> {
    fn walk(from: &Path, to: &Path, refresh: bool) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let name = entry.file_name();
            let (src, dst) = (entry.path(), to.join(&name));
            if entry.file_type()?.is_dir() {
                walk(&src, &dst, refresh || name == DEFAULTS_DIR_NAME)?;
            } else if refresh || !dst.exists() {
                std::fs::copy(&src, &dst)?;
            }
        }
        Ok(())
    }
    if from.is_dir() {
        walk(from, to, false)?;
    }
    Ok(())
}

/// Folders a config file may be in, most preferred first.
pub fn candidate_dirs() -> Vec<PathBuf> {
    if let Some(i) = installed() {
        return vec![i.config.clone()];
    }
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(CONFIG_DIR_NAME));
    }
    if let Some(exe_dir) = exe_dir() {
        dirs.push(exe_dir.join(CONFIG_DIR_NAME));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(CONFIG_DIR_NAME));
    dirs
}

/// The config folder in use: the first existing one of `./config`, `config` next to the
/// executable and the crate's `config`, or `./config` if none exists yet (for an installed
/// copy, the per-user one; see [`INSTALLED_MARKER`]).
pub fn config_dir() -> PathBuf {
    let dirs = candidate_dirs();
    dirs.iter()
        .find(|d| d.is_dir())
        .cloned()
        .unwrap_or_else(|| dirs[0].clone())
}

/// A folder of the app such as `exports` or `saves`: the first existing one of `./<name>`,
/// `<name>` next to the executable and the crate's `<name>`, or `./<name>` if none exists
/// yet (for an installed copy, `Documents/Recvrve/<name>`).
pub fn app_folder(name: &str) -> PathBuf {
    if let Some(i) = installed() {
        return i.documents.join(name);
    }
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(name));
    }
    if let Some(exe_dir) = exe_dir() {
        dirs.push(exe_dir.join(name));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(name));
    dirs.iter()
        .find(|d| d.is_dir())
        .cloned()
        .unwrap_or_else(|| dirs[0].clone())
}

/// The shipped default copy of a config file.
pub fn defaults_path(file_name: &str) -> PathBuf {
    config_dir().join(DEFAULTS_DIR_NAME).join(file_name)
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Writes `contents` to `path`. The previous file, if any, is first copied to
/// `<path>.bak`, and the new contents go through a temporary file that is then renamed, so
/// a crash never leaves a half-written config.
pub fn write_with_backup(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.is_file() {
        std::fs::copy(path, backup_path(path))?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Loads a config at startup with fallbacks. `path` is the config file (if it exists);
/// `parse` reads one file. On success returns the value, the file it came from and a
/// notice if a fallback was used. On failure returns every error that occurred.
pub fn load_with_fallback<T>(
    path: Option<PathBuf>,
    file_name: &str,
    parse: impl Fn(&Path) -> Result<T, ConfigError>,
) -> Result<(T, PathBuf, Option<String>), String> {
    let mut errors = Vec::new();
    let mut candidates = Vec::new();
    if let Some(path) = &path {
        candidates.push((path.clone(), None));
        candidates.push((backup_path(path), Some("its backup (.bak)")));
    }
    candidates.push((defaults_path(file_name), Some("the shipped defaults")));

    for (candidate, fallback) in candidates {
        if !candidate.is_file() {
            continue;
        }
        match parse(&candidate) {
            Ok(value) => {
                let notice = match (fallback, errors.is_empty()) {
                    (Some(which), false) => Some(format!(
                        "{} could not be loaded ({}), so {which} was used instead.",
                        file_name,
                        errors.join("; ")
                    )),
                    _ => None,
                };
                return Ok((value, candidate, notice));
            }
            Err(e) => errors.push(format!("{}: {e}", candidate.display())),
        }
    }
    Err(errors.join("; "))
}

/// UI preferences that persist between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// Hide the Top/Jungle/Mid/ADC/Support selection filter buttons.
    pub hide_selection_presets: bool,
    /// Hide the preset representational group tiles Top (3) to 15.
    pub hide_rgroup_presets: bool,
    /// Tag new picks with the roles of the active role filters (Top, Jungle, ...).
    pub auto_tag_roles: bool,
    /// Panel sizes, set by dragging the splitters.
    pub layout: Layout,
    /// Champion tile size in pixels (the top bar slider).
    pub tile_size: u32,
    /// Folder name of the selected theme in `config/themes/`.
    pub theme: String,
    /// Panels the user has hidden.
    pub hidden: Hidden,
    /// Easter eggs on or off (see `easter.rs`).
    pub easter_eggs: bool,
    /// The smooth animation profile (on by default) or the low-spec one: the app as it was
    /// before animations were added (see the `.anim-smooth` rules in `style.css`).
    pub smooth_animations: bool,
    /// The graph runs top to bottom (true) or left to right (false).
    pub graph_vertical: bool,
    /// Graph zoom (Ctrl+wheel), 1 = 100%: top to bottom and left to right.
    pub graph_zoom_v: f64,
    pub graph_zoom_h: f64,
    /// Show the draft state when hovering a graph node.
    pub node_previews: bool,
    /// Show the browser's tooltips (with debug details) on graph nodes.
    pub node_tooltips: bool,
    /// Minutes between autosaves of changed tabs; 0 turns autosave off.
    pub autosave_minutes: u32,
    /// Panel buttons shown or hidden differently from their defaults (Button settings).
    pub buttons: crate::buttons::Overrides,
    /// Help has been opened once, so its button no longer glows.
    pub help_seen: bool,
}

/// Which panels are hidden (collapsed to a bar). The team panels hide as a pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hidden {
    pub teams: bool,
    pub pool: bool,
    pub groups: bool,
    pub tree: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            hide_selection_presets: false,
            hide_rgroup_presets: false,
            auto_tag_roles: false,
            layout: Layout::default(),
            tile_size: TILE_DEFAULT,
            theme: crate::theme::DEFAULT_THEME.to_string(),
            hidden: Hidden::default(),
            easter_eggs: true,
            smooth_animations: true,
            graph_vertical: true,
            graph_zoom_v: 1.0,
            graph_zoom_h: 1.0,
            node_previews: true,
            node_tooltips: false,
            autosave_minutes: 2,
            buttons: Default::default(),
            help_seen: false,
        }
    }
}

pub const TILE_MIN: u32 = 40;
pub const TILE_MAX: u32 = 128;
pub const TILE_DEFAULT: u32 = 76;
pub const ZOOM_MIN: f64 = 0.3;
pub const ZOOM_MAX: f64 = 2.0;

/// Sizes of the resizable panels. Both side panels share one width in pixels (they resize
/// as a mirrored pair); the champion pool and the groups area are shares of the middle
/// column's height, and the graph gets the rest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Width of each side panel.
    pub side: f32,
    pub pool: f32,
    pub groups: f32,
    /// Left to right: the champion pool and groups area size themselves to the window
    /// (whole rows of tiles, see layout.rs) until the user drags them.
    pub h_auto: bool,
    /// Fullscreen: share of the height for the champion pool and groups row.
    pub fs_top: f32,
    /// Fullscreen: width of the groups column, in pixels.
    pub fs_groups: f32,
    /// Top-to-bottom graph: width of each side panel.
    pub v_side: f32,
    /// Top-to-bottom graph: share of the height for the champion pool and groups row, once
    /// the user has dragged it (see `v_auto`).
    pub v_top: f32,
    /// Top-to-bottom graph: the champion pool and groups row sizes itself to the window
    /// (up to four whole rows of champion tiles, see layout.rs) until the user drags it.
    pub v_auto: bool,
    /// Top-to-bottom graph: width of the groups column, in pixels.
    pub v_groups: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            side: 250.0,
            pool: 0.5,
            groups: 0.2,
            h_auto: true,
            fs_top: 0.36,
            fs_groups: 280.0,
            v_side: 250.0,
            v_top: 0.64,
            v_auto: true,
            v_groups: 280.0,
        }
    }
}

impl Layout {
    pub const SIDE_MIN: f32 = 160.0;
    pub const SIDE_MAX: f32 = 560.0;

    /// Keeps values from a hand-edited settings file usable.
    pub fn clamped(self) -> Layout {
        let pool = self.pool.clamp(0.1, 0.85);
        Layout {
            side: self.side.clamp(Self::SIDE_MIN, Self::SIDE_MAX),
            pool,
            groups: self.groups.clamp(0.05, (0.95 - pool).max(0.05)),
            fs_top: self.fs_top.clamp(0.15, 0.8),
            h_auto: self.h_auto,
            fs_groups: self.fs_groups.clamp(Self::SIDE_MIN, Self::SIDE_MAX),
            v_side: self.v_side.clamp(Self::SIDE_MIN, Self::SIDE_MAX),
            v_top: self.v_top.clamp(0.15, 0.8),
            v_auto: self.v_auto,
            v_groups: self.v_groups.clamp(Self::SIDE_MIN, Self::SIDE_MAX),
        }
    }

    /// CSS variables for the app root. The top-to-bottom graph has its own side width.
    pub fn css(&self, vertical: bool) -> String {
        let side = if vertical { self.v_side } else { self.side };
        format!(
            "--side-w: {side}px; --pool-frac: {}; --groups-frac: {}; --fs-top: {}; --fs-groups-w: {}px; --v-top: {}; --v-groups-w: {}px;",
            self.pool, self.groups, self.fs_top, self.fs_groups, self.v_top, self.v_groups
        )
    }
}

impl UiSettings {
    /// Loads `config/settings.json`; a missing or unreadable file gives the defaults.
    pub fn load() -> Self {
        let mut settings: UiSettings =
            std::fs::read_to_string(config_dir().join(SETTINGS_FILE_NAME))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
        settings.layout = settings.layout.clamped();
        settings.tile_size = settings.tile_size.clamp(TILE_MIN, TILE_MAX);
        settings.graph_zoom_v = settings.graph_zoom_v.clamp(ZOOM_MIN, ZOOM_MAX);
        settings.graph_zoom_h = settings.graph_zoom_h.clamp(ZOOM_MIN, ZOOM_MAX);
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let text = serde_json::to_string_pretty(self).expect("settings serialize");
        std::fs::write(dir.join(SETTINGS_FILE_NAME), text + "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draftree-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_keep_a_backup() {
        let dir = scratch_dir("backup");
        let path = dir.join("x.json");
        write_with_backup(&path, "one").unwrap();
        assert!(!backup_path(&path).exists());
        write_with_backup(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), "one");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn falls_back_to_the_backup() {
        let dir = scratch_dir("fallback");
        let path = dir.join("x.json");
        std::fs::write(&path, "broken").unwrap();
        std::fs::write(backup_path(&path), "good").unwrap();
        let parse = |p: &Path| {
            let text = std::fs::read_to_string(p).unwrap();
            if text == "good" {
                Ok(text)
            } else {
                Err(ConfigError("bad".into()))
            }
        };
        let (value, from, notice) =
            load_with_fallback(Some(path.clone()), "no-such-default.json", parse).unwrap();
        assert_eq!(value, "good");
        assert_eq!(from, backup_path(&path));
        assert!(notice.unwrap().contains("backup"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_round_trip() {
        let s = UiSettings {
            hide_selection_presets: true,
            hide_rgroup_presets: false,
            auto_tag_roles: true,
            layout: Layout {
                side: 300.0,
                ..Layout::default()
            },
            tile_size: 90,
            theme: "light".into(),
            hidden: Hidden {
                teams: true,
                tree: true,
                ..Hidden::default()
            },
            easter_eggs: false,
            smooth_animations: false,
            graph_vertical: false,
            graph_zoom_v: 1.5,
            graph_zoom_h: 0.5,
            node_previews: false,
            node_tooltips: true,
            autosave_minutes: 5,
            buttons: [("merge".to_string(), true), ("save".to_string(), false)].into(),
            help_seen: true,
        };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<UiSettings>(&text).unwrap(), s);
        // Missing fields fall back to defaults.
        assert_eq!(
            serde_json::from_str::<UiSettings>("{}").unwrap(),
            UiSettings::default()
        );
        // Settings files from before the layout was saved still load.
        let old: UiSettings = serde_json::from_str(r#"{"hide_rgroup_presets": true}"#).unwrap();
        assert!(old.hide_rgroup_presets);
        assert_eq!(old.layout, Layout::default());
        // Settings from before Button settings: default buttons, and Help glows once.
        assert!(old.buttons.is_empty() && !old.help_seen);
        assert_eq!(old.tile_size, TILE_DEFAULT);
        assert_eq!(old.hidden, Hidden::default());
        assert!(old.easter_eggs, "easter eggs are on unless turned off");
        assert!(old.smooth_animations, "smooth animations unless turned off");
        assert!(
            old.graph_vertical,
            "the graph runs top to bottom unless changed"
        );
        assert_eq!((old.graph_zoom_v, old.graph_zoom_h), (1.0, 1.0));
        assert!(old.node_previews && !old.node_tooltips);
    }

    #[test]
    fn layout_is_clamped() {
        let wild = Layout {
            side: 5000.0,
            pool: 2.0,
            groups: 0.5,
            h_auto: false,
            fs_top: 3.0,
            fs_groups: 1.0,
            v_side: 1.0,
            v_top: 0.0,
            v_auto: false,
            v_groups: 9000.0,
        }
        .clamped();
        assert_eq!((wild.fs_top, wild.fs_groups), (0.8, Layout::SIDE_MIN));
        assert_eq!(
            (wild.v_side, wild.v_top, wild.v_groups),
            (Layout::SIDE_MIN, 0.15, Layout::SIDE_MAX)
        );
        assert_eq!(wild.side, Layout::SIDE_MAX);
        assert!(wild.pool + wild.groups <= 0.95 + f32::EPSILON);
    }

    #[test]
    fn seeding_copies_missing_files_and_refreshes_defaults() {
        let root = std::env::temp_dir().join(format!("recvrve-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (from, to) = (root.join("shipped"), root.join("user"));
        std::fs::create_dir_all(from.join("defaults")).unwrap();
        std::fs::create_dir_all(from.join("themes/dark")).unwrap();
        std::fs::write(from.join("reprgroups.json"), "shipped").unwrap();
        std::fs::write(from.join("defaults/reprgroups.json"), "new default").unwrap();
        std::fs::write(from.join("themes/dark/theme.json"), "dark").unwrap();
        std::fs::create_dir_all(to.join("defaults")).unwrap();
        std::fs::write(to.join("reprgroups.json"), "mine").unwrap();
        std::fs::write(to.join("defaults/reprgroups.json"), "old default").unwrap();
        seed(&from, &to).unwrap();
        let read = |p: &str| std::fs::read_to_string(to.join(p)).unwrap();
        assert_eq!(read("reprgroups.json"), "mine");
        assert_eq!(read("defaults/reprgroups.json"), "new default");
        assert_eq!(read("themes/dark/theme.json"), "dark");
        // A missing source is fine.
        seed(&root.join("nothing"), &to).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
