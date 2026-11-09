//! Themes: colors, a background picture and graph line styles.
//!
//! Each theme is a folder in `config/themes/` holding a `theme.json` and, optionally, a
//! picture next to it:
//!
//! ```json
//! {
//!   "name": "Starlit Dusk",
//!   "base": "dark",
//!   "background": "background.jpg",
//!   "fit": "cover",
//!   "dim": 35,
//!   "panel_opacity": 78,
//!   "blur": 8,
//!   "colors": { "bg": "#141a33", "panel": "#1a2142", "panel2": "#252d55",
//!               "border": "#3b4675", "text": "#eef0ff", "muted": "#9aa3cc",
//!               "accent": "#f4a462" },
//!   "group_colors": ["#f4a462", "#5fd3c2"],
//!   "graph": { "edge": "#56608f", "edge_active": "#f4a462",
//!              "turn_line": "#2f3863", "turn_width": 1,
//!              "phase_line": "#4a5590", "phase_width": 3 }
//! }
//! ```
//!
//! Every field except `name` may be left out; it then takes the value of the `base`
//! palette (`dark` or `light`). Invalid or unknown fields are reported as warnings and
//! ignored instead of rejecting the whole theme. Team blue and red, and the fearless
//! colors, carry meaning and can't be changed; group colors too close to them are dropped.
//!
//! The built-in Dark and Light themes live in `config/themes/dark` and `config/themes/light`
//! like any other theme, and are written back from the defaults here if they go missing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::configio;

pub const THEMES_DIR_NAME: &str = "themes";
pub const THEME_FILE_NAME: &str = "theme.json";
pub const DEFAULT_THEME: &str = "dark";
/// Picture types the background protocol serves.
pub const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Base {
    #[default]
    Dark,
    Light,
}

impl Base {
    pub fn class(self) -> &'static str {
        match self {
            Base::Dark => "dark",
            Base::Light => "light",
        }
    }
}

/// How the background picture fills the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Tile,
    Center,
}

impl Fit {
    pub const ALL: [Fit; 4] = [Fit::Cover, Fit::Contain, Fit::Tile, Fit::Center];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Cover => "Cover",
            Fit::Contain => "Contain",
            Fit::Tile => "Tile",
            Fit::Center => "Center",
        }
    }

    /// CSS background-size and background-repeat.
    fn css(self) -> (&'static str, &'static str) {
        match self {
            Fit::Cover => ("cover", "no-repeat"),
            Fit::Contain => ("contain", "no-repeat"),
            Fit::Tile => ("auto", "repeat"),
            Fit::Center => ("auto", "no-repeat"),
        }
    }
}

/// The page's dim layer over the picture, as `.app.has-bg` in style.css draws it.
const DIM: &str = "color-mix(in srgb, var(--bg) var(--bg-dim, 35%), transparent)";

/// The interface colors a theme may set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colors {
    pub bg: String,
    pub panel: String,
    pub panel2: String,
    pub border: String,
    pub text: String,
    pub muted: String,
    /// Highlights, selection and the current path (the app's gold).
    pub accent: String,
}

impl Colors {
    /// (key, label) for every color, in display order.
    pub const FIELDS: [(&'static str, &'static str); 7] = [
        ("bg", "Background"),
        ("panel", "Panels"),
        ("panel2", "Tiles and inputs"),
        ("border", "Borders"),
        ("text", "Text"),
        ("muted", "Muted text"),
        ("accent", "Accent"),
    ];

    pub fn get(&self, key: &str) -> &str {
        match key {
            "bg" => &self.bg,
            "panel" => &self.panel,
            "panel2" => &self.panel2,
            "border" => &self.border,
            "text" => &self.text,
            "muted" => &self.muted,
            _ => &self.accent,
        }
    }

    pub fn set(&mut self, key: &str, value: String) {
        match key {
            "bg" => self.bg = value,
            "panel" => self.panel = value,
            "panel2" => self.panel2 = value,
            "border" => self.border = value,
            "text" => self.text = value,
            "muted" => self.muted = value,
            _ => self.accent = value,
        }
    }
}

/// Lines of the draft graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    /// Branch lines.
    pub edge: String,
    /// Branch lines on the path to the selected node.
    pub edge_active: String,
    /// Separators between draft turns.
    pub turn_line: String,
    pub turn_width: f32,
    /// Separators between ban and pick phases.
    pub phase_line: String,
    pub phase_width: f32,
}

/// A loaded theme with every value filled in.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Theme {
    /// Folder name; how settings refer to the theme.
    #[serde(skip)]
    pub id: String,
    pub name: String,
    pub base: Base,
    /// Picture file name inside the theme folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    pub fit: Fit,
    /// How strongly the background color covers the picture, 0-90 %.
    pub dim: u8,
    /// Panel opacity, 20-100 %.
    pub panel_opacity: u8,
    /// Blur behind panels, 0-20 px.
    pub blur: u8,
    pub colors: Colors,
    pub group_colors: Vec<String>,
    pub graph: Graph,
    /// Problems found while loading.
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// Changes when the picture file changes, so the webview reloads it.
    #[serde(skip)]
    pub background_version: u64,
    /// Blur panels live with `backdrop-filter` instead of the cached blurred picture: set
    /// on the theme editor's preview while picture or blur differ from the saved theme.
    #[serde(skip)]
    pub live_blur: bool,
}

/// Default group colors (the original palette, minus anything close to team red).
pub const DEFAULT_GROUP_COLORS: [&str; 12] = [
    "#f59e0b", "#10b981", "#a16207", "#ec4899", "#06b6d4", "#84cc16", "#f97316", "#64748b",
    "#14b8a6", "#4ade80", "#78716c", "#eab308",
];

impl Theme {
    /// The built-in theme for `base`, with the given folder id and name.
    pub fn builtin(base: Base) -> Theme {
        let (id, name, colors, graph) = match base {
            Base::Dark => (
                "dark",
                "Dark",
                Colors {
                    bg: "#12141b".into(),
                    panel: "#1b1e28".into(),
                    panel2: "#232735".into(),
                    border: "#2f3445".into(),
                    text: "#e6e8ef".into(),
                    muted: "#8a90a3".into(),
                    accent: "#e0b44c".into(),
                },
                Graph {
                    edge: "#2f3445".into(),
                    edge_active: "#e0b44c".into(),
                    turn_line: "#2f3445".into(),
                    turn_width: 1.0,
                    phase_line: "#4a5170".into(),
                    phase_width: 3.0,
                },
            ),
            Base::Light => (
                "light",
                "Light",
                Colors {
                    bg: "#dddddd".into(),
                    panel: "#d3d3d3".into(),
                    panel2: "#eceef4".into(),
                    border: "#d5d9e3".into(),
                    text: "#1b1e28".into(),
                    muted: "#626a80".into(),
                    accent: "#b7860f".into(),
                },
                Graph {
                    edge: "#7d859a".into(),
                    edge_active: "#b7860f".into(),
                    turn_line: "#d5d9e3".into(),
                    turn_width: 1.0,
                    phase_line: "#a6adbd".into(),
                    phase_width: 3.0,
                },
            ),
        };
        Theme {
            id: id.into(),
            name: name.into(),
            base,
            background: None,
            fit: Fit::Cover,
            dim: 35,
            panel_opacity: 100,
            blur: 0,
            colors,
            group_colors: DEFAULT_GROUP_COLORS.iter().map(|c| c.to_string()).collect(),
            graph,
            warnings: Vec::new(),
            background_version: 0,
            live_blur: false,
        }
    }

    pub fn dir(&self) -> PathBuf {
        themes_dir().join(&self.id)
    }

    /// The `theme.json` contents for this theme.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("themes serialize") + "\n"
    }

    /// The color for the `index`th group to be used: from the theme's list, then generated.
    pub fn group_color(&self, index: usize) -> String {
        match self.group_colors.get(index) {
            Some(c) => c.clone(),
            None => extra_group_color(index - self.group_colors.len()),
        }
    }

    /// CSS variables for the app root.
    pub fn css(&self) -> String {
        let c = &self.colors;
        let g = &self.graph;
        let mut css = format!(
            "--bg: {}; --panel: {}; --panel-2: {}; --border: {}; --text: {}; --muted: {}; \
             --gold: {}; --edge: {}; --edge-active: {}; --turn-line: {}; --turn-w: {}px; \
             --phase-line: {}; --phase-w: {}px; --panel-alpha: {}%; --bg-dim: {}%;",
            c.bg,
            c.panel,
            c.panel2,
            c.border,
            c.text,
            c.muted,
            c.accent,
            g.edge,
            g.edge_active,
            g.turn_line,
            g.turn_width,
            g.phase_line,
            g.phase_width,
            self.panel_opacity,
            self.dim,
        );
        let (size, repeat) = self.fit.css();
        // Panels show the blurred picture from the cache once it exists, aligned with the
        // page background; until then (and while previewing) the webview blurs live.
        let blurred = match &self.background {
            Some(file) if self.blur > 0 && !self.live_blur => blur_cache::get(self, file),
            _ => blur_cache::Lookup::Live,
        };
        match &blurred {
            blur_cache::Lookup::Ready(b) => {
                let size = match self.fit {
                    Fit::Cover | Fit::Contain => size.to_string(),
                    // Shown at the original picture's size, which the cache file isn't.
                    Fit::Tile | Fit::Center => format!("{}px {}px", b.width, b.height),
                };
                css.push_str(&format!(
                    " --panel-under: linear-gradient({DIM}, {DIM}), \
                     url(\"{}\") center / {size} {repeat} fixed var(--bg);",
                    cache_url(&self.id, &b.file)
                ));
            }
            // Being built: plain panels until it's ready, since blurring live on every
            // repaint would slow down the first edits after a start.
            blur_cache::Lookup::Building => {}
            blur_cache::Lookup::Live if self.blur > 0 => {
                css.push_str(&format!(" --backdrop: blur({}px);", self.blur));
            }
            blur_cache::Lookup::Live => {}
        }
        if let Some(file) = &self.background {
            css.push_str(&format!(
                " --bg-image: url(\"{}\"); --bg-size: {size}; --bg-repeat: {repeat};",
                background_url(&self.id, file, self.background_version)
            ));
        }
        css
    }
}

/// `#rgb` or `#rrggbb` as `#rrggbb` (lowercase), or `None`.
pub fn normalize_color(text: &str) -> Option<String> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => Some(format!("#{}", hex.to_lowercase())),
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_lowercase()
        )),
        _ => None,
    }
}

fn rgb(hex: &str) -> (f32, f32, f32) {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| f32::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0)) / 255.0;
    (p(0), p(2), p(4))
}

/// Hue (degrees), saturation and lightness of a `#rrggbb` color.
fn hsl(hex: &str) -> (f32, f32, f32) {
    let (r, g, b) = rgb(hex);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, s, l)
}

/// Hues that mean something else in the app: team red, team blue, fearless purple.
const RESERVED_HUES: [(f32, &str); 3] = [
    (0.0, "team red"),
    (217.0, "team blue"),
    (292.0, "fearless purple"),
];
const RESERVED_RANGE: f32 = 15.0;

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

/// What a group color would be confused with, if anything.
pub fn reserved_clash(hex: &str) -> Option<&'static str> {
    let (h, s, l) = hsl(hex);
    if s < 0.35 || !(0.2..=0.85).contains(&l) {
        return None;
    }
    RESERVED_HUES
        .iter()
        .find(|(hue, _)| hue_distance(h, *hue) < RESERVED_RANGE)
        .map(|(_, what)| *what)
}

fn hsl_to_hex(h: f32, s: f32, l: f32) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}

/// A well-visible color for groups beyond a theme's list. Hues follow the golden angle,
/// so neighbours look different, and skip the reserved hues.
pub fn extra_group_color(n: usize) -> String {
    let mut found = 0;
    for k in 0.. {
        let hue = (k as f32 * 137.508 + 25.0) % 360.0;
        if RESERVED_HUES
            .iter()
            .any(|(r, _)| hue_distance(hue, *r) < RESERVED_RANGE + 10.0)
        {
            continue;
        }
        if found == n {
            let light = if k % 2 == 0 { 0.58 } else { 0.5 };
            return hsl_to_hex(hue, 0.72, light);
        }
        found += 1;
    }
    unreachable!("the hue sequence never ends")
}

// ---- Loading ----

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTheme {
    name: Option<String>,
    base: Option<String>,
    background: Option<String>,
    fit: Option<String>,
    dim: Option<f64>,
    panel_opacity: Option<f64>,
    blur: Option<f64>,
    colors: BTreeMap<String, String>,
    group_colors: Option<Vec<String>>,
    graph: BTreeMap<String, Value>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

fn percent(
    value: Option<f64>,
    field: &str,
    range: (u8, u8),
    default: u8,
    warnings: &mut Vec<String>,
) -> u8 {
    match value {
        None => default,
        Some(v) if v >= f64::from(range.0) && v <= f64::from(range.1) => v.round() as u8,
        Some(v) => {
            warnings.push(format!(
                "\"{field}\" must be {}-{}, got {v}",
                range.0, range.1
            ));
            (v.round().clamp(f64::from(range.0), f64::from(range.1))) as u8
        }
    }
}

/// Reads a theme from `theme.json` text. `id` is its folder name; `dir` is used to check
/// that the picture exists.
pub fn parse(id: &str, text: &str, dir: Option<&Path>) -> Result<Theme, String> {
    let raw: RawTheme = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
    let mut warnings = Vec::new();
    for key in raw.unknown.keys() {
        warnings.push(format!("unknown field \"{key}\" ignored"));
    }
    let base = match raw.base.as_deref() {
        None | Some("dark") => Base::Dark,
        Some("light") => Base::Light,
        Some(other) => {
            warnings.push(format!(
                "\"base\" must be \"dark\" or \"light\", got \"{other}\""
            ));
            Base::Dark
        }
    };
    let mut theme = Theme::builtin(base);
    theme.id = id.to_string();
    theme.name = match raw.name.map(|n| n.trim().to_string()) {
        Some(n) if !n.is_empty() => n,
        _ => {
            warnings.push("missing \"name\"; using the folder name".into());
            id.to_string()
        }
    };

    if let Some(file) = raw.background.filter(|f| !f.trim().is_empty()) {
        if !is_plain_image_name(&file) {
            warnings.push(format!(
                "\"background\" must be the name of a png, jpg, webp, gif or bmp file in the theme folder, got \"{file}\""
            ));
        } else if let Some(dir) = dir {
            let path = dir.join(&file);
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    theme.background_version = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(1, |d| d.as_secs());
                    theme.background = Some(file);
                }
                Err(_) => warnings.push(format!("background picture \"{file}\" not found")),
            }
        } else {
            theme.background = Some(file);
        }
    }
    theme.fit = match raw.fit.as_deref() {
        None => Fit::Cover,
        Some(f) => match serde_json::from_value(Value::String(f.to_string())) {
            Ok(fit) => fit,
            Err(_) => {
                warnings.push(format!(
                    "\"fit\" must be cover, contain, tile or center, got \"{f}\""
                ));
                Fit::Cover
            }
        },
    };
    theme.dim = percent(raw.dim, "dim", (0, 90), theme.dim, &mut warnings);
    theme.panel_opacity = percent(
        raw.panel_opacity,
        "panel_opacity",
        (20, 100),
        theme.panel_opacity,
        &mut warnings,
    );
    theme.blur = percent(raw.blur, "blur", (0, 20), theme.blur, &mut warnings);

    let keys: Vec<&str> = Colors::FIELDS.iter().map(|(k, _)| *k).collect();
    for (key, value) in raw.colors {
        if matches!(key.as_str(), "blue" | "red") {
            warnings.push(format!("team color \"{key}\" can't be changed by a theme"));
        } else if !keys.contains(&key.as_str()) {
            warnings.push(format!("unknown color \"{key}\" ignored"));
        } else {
            match normalize_color(&value) {
                Some(c) => theme.colors.set(&key, c),
                None => warnings.push(format!(
                    "color \"{key}\" must look like #rrggbb, got \"{value}\""
                )),
            }
        }
    }

    if let Some(list) = raw.group_colors {
        let mut colors = Vec::new();
        for value in list {
            match normalize_color(&value) {
                None => warnings.push(format!("group color \"{value}\" must look like #rrggbb")),
                Some(c) => match reserved_clash(&c) {
                    Some(what) => warnings.push(format!(
                        "group color {c} is too close to {what} and was left out"
                    )),
                    None => colors.push(c),
                },
            }
        }
        theme.group_colors = colors;
    }

    for (key, value) in raw.graph {
        let as_color = || value.as_str().and_then(normalize_color);
        let as_width = |max: f64| {
            value
                .as_f64()
                .filter(|w| *w >= 0.0 && *w <= max)
                .map(|w| w as f32)
        };
        let ok = match key.as_str() {
            "edge" => as_color().map(|c| theme.graph.edge = c).is_some(),
            "edge_active" => as_color().map(|c| theme.graph.edge_active = c).is_some(),
            "turn_line" => as_color().map(|c| theme.graph.turn_line = c).is_some(),
            "phase_line" => as_color().map(|c| theme.graph.phase_line = c).is_some(),
            "turn_width" => as_width(8.0).map(|w| theme.graph.turn_width = w).is_some(),
            "phase_width" => as_width(12.0)
                .map(|w| theme.graph.phase_width = w)
                .is_some(),
            _ => {
                warnings.push(format!("unknown graph setting \"{key}\" ignored"));
                true
            }
        };
        if !ok {
            warnings.push(format!(
                "graph setting \"{key}\" has an invalid value: {value}"
            ));
        }
    }

    theme.warnings = warnings;
    Ok(theme)
}

/// Whether `name` is a plain file name (no folders) with a picture extension.
pub fn is_plain_image_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Whether `id` is a usable theme folder name.
pub fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn themes_dir() -> PathBuf {
    configio::config_dir().join(THEMES_DIR_NAME)
}

/// Writes the built-in Dark and Light themes if their files are missing.
fn ensure_builtins(dir: &Path) {
    for base in [Base::Dark, Base::Light] {
        let theme = Theme::builtin(base);
        let path = dir.join(&theme.id).join(THEME_FILE_NAME);
        if !path.exists() {
            let _ = configio::write_with_backup(&path, &theme.to_json());
        }
    }
}

/// Every theme in the themes folder: Dark and Light first, the rest by name. Themes that
/// can't be read are skipped with a message.
pub fn scan() -> (Vec<Theme>, Vec<String>) {
    let dir = themes_dir();
    ensure_builtins(&dir);
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    entries.sort();
    for path in entries {
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !is_plain_id(&id) {
            continue;
        }
        let file = path.join(THEME_FILE_NAME);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        match parse(&id, &text, Some(&path)) {
            Ok(theme) => themes.push(theme),
            Err(e) => errors.push(format!("Theme \"{id}\": {e}")),
        }
    }
    for builtin in [Base::Light, Base::Dark] {
        let b = Theme::builtin(builtin);
        if !themes.iter().any(|t| t.id == b.id) {
            themes.push(b);
        }
    }
    let rank = |t: &Theme| match t.id.as_str() {
        "dark" => 0,
        "light" => 1,
        _ => 2,
    };
    themes.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    (themes, errors)
}

/// Picture files directly in a theme's folder, for the settings panel.
pub fn images_in(id: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(themes_dir().join(id))
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| is_plain_image_name(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Saves `theme` to its folder's `theme.json` (keeping a `.bak` of the previous file).
pub fn save(theme: &Theme) -> std::io::Result<()> {
    configio::write_with_backup(&theme.dir().join(THEME_FILE_NAME), &theme.to_json())
}

/// A folder id for a new theme called `name` that isn't taken yet.
pub fn new_id(name: &str) -> String {
    let mut slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let base = if slug.is_empty() { "theme" } else { slug };
    let dir = themes_dir();
    if !dir.join(base).exists() {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !dir.join(id).exists())
        .expect("some suffix is free")
}

// ---- Background protocol ----

pub const PROTOCOL: &str = "bg";
/// Folder inside a theme folder for generated files (the blurred picture).
pub const CACHE_DIR_NAME: &str = ".cache";

pub fn background_url(id: &str, file: &str, version: u64) -> String {
    if cfg!(target_os = "windows") {
        format!("http://{PROTOCOL}.localhost/{id}/{file}?v={version}")
    } else {
        format!("{PROTOCOL}://localhost/{id}/{file}?v={version}")
    }
}

/// URL of a theme's blurred picture in its cache folder. The name holds a content hash,
/// so it needs no version.
pub fn cache_url(id: &str, file: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("http://{PROTOCOL}.localhost/{id}/{CACHE_DIR_NAME}/{file}")
    } else {
        format!("{PROTOCOL}://localhost/{id}/{CACHE_DIR_NAME}/{file}")
    }
}

/// The file a protocol request path refers to: `/<theme id>/<picture>` for a picture
/// directly inside a theme folder, or `/<theme id>/.cache/<blur file>` for its blurred
/// copy. Anything else is refused, so a request can never reach other files.
pub fn resolve_request(path: &str) -> Option<(PathBuf, &'static str)> {
    let mut parts = path.trim_start_matches('/').split('/');
    let id = parts.next()?;
    let file = parts.next()?;
    if !is_plain_id(id) {
        return None;
    }
    if file == CACHE_DIR_NAME {
        // Only names the cache writes itself, compared undecoded.
        let name = parts.next()?;
        if parts.next().is_some() || !blur_cache::is_cache_name(name) {
            return None;
        }
        let mime = if name.ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        };
        return Some((themes_dir().join(id).join(CACHE_DIR_NAME).join(name), mime));
    }
    if parts.next().is_some() {
        return None;
    }
    // Names with spaces arrive percent-encoded.
    let file = percent_decode(file)?;
    if !is_plain_image_name(&file) {
        return None;
    }
    let ext = Path::new(&file).extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/bmp",
    };
    Some((themes_dir().join(id).join(file), mime))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ---- Blurred background cache ----

/// Pre-blurred copies of theme pictures for the panels, so the webview doesn't re-blur the
/// page behind every panel on each repaint (`backdrop-filter`). Each one is a file in the
/// theme's `.cache` folder named after a hash of everything that shapes it, built off the
/// UI thread the first time a theme needs it. An index next to it remembers the picture it
/// was built from by size and change time, so later starts use it at once, without reading
/// the picture.
mod blur_cache {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{LazyLock, Mutex};

    use dioxus::core::ReactiveContext;
    use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage, imageops};

    use super::{CACHE_DIR_NAME, Fit, Theme, themes_dir};

    /// Bump when the rendering changes, so older cache files are rebuilt.
    const FORMAT: u8 = 1;
    /// The window size the blur radius is scaled for: CSS blurs in screen pixels, on the
    /// picture as the page shows it.
    const REFERENCE: (f32, f32) = (1920.0, 1080.0);
    /// The blur's spread in output pixels: the picture is shrunk until the blur is about
    /// this wide, since it has no detail finer than that.
    const OUTPUT_SIGMA: f32 = 3.0;
    /// Longest side of the cache file.
    const MAX_SIDE: f32 = 1280.0;
    const PREFIX: &str = "blur-";
    /// The cache folder's index: which picture and settings its file was built from.
    const INDEX: &str = "blur.idx";

    /// A ready blurred picture: its file name in the cache folder, and the source picture's
    /// size (for fits that show the picture at its own size).
    #[derive(Clone, Debug, PartialEq)]
    pub struct Blurred {
        pub file: String,
        pub width: u32,
        pub height: u32,
    }

    /// What the cache was asked for. The picture's version follows its file, so a changed
    /// picture is a new request.
    #[derive(Clone, PartialEq, Eq, Hash)]
    struct Request {
        id: String,
        file: String,
        version: u64,
        blur: u8,
        fit: Fit,
    }

    enum State {
        /// Being built; these renders are redone when it's ready.
        Pending(Vec<ReactiveContext>),
        Ready(Blurred),
        /// Couldn't be built (e.g. a gif); the panels keep blurring live.
        Failed,
    }

    static STATES: LazyLock<Mutex<HashMap<Request, State>>> = LazyLock::new(Default::default);
    /// One build at a time, so two builds for a theme never remove each other's file.
    static BUILDING: Mutex<()> = Mutex::new(());

    /// What the panels show for a theme's picture.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Lookup {
        Ready(Blurred),
        /// Being built; the caller's render is redone once it is ready.
        Building,
        /// Blur live (it can't be built, or the theme editor is previewing).
        Live,
    }

    /// The blurred picture for `theme`.
    pub fn get(theme: &Theme, file: &str) -> Lookup {
        let request = Request {
            id: theme.id.clone(),
            file: file.to_string(),
            version: theme.background_version,
            blur: theme.blur,
            fit: theme.fit,
        };
        let Ok(mut states) = STATES.lock() else {
            return Lookup::Live;
        };
        match states.get_mut(&request) {
            Some(State::Ready(b)) => return Lookup::Ready(b.clone()),
            Some(State::Failed) => return Lookup::Live,
            Some(State::Pending(waiting)) => {
                if let Some(rc) = ReactiveContext::current()
                    && !waiting.contains(&rc)
                {
                    waiting.push(rc);
                }
                return Lookup::Building;
            }
            None => {}
        }
        // Built on an earlier run from the same picture: ready for the first frame. Only
        // the picture's metadata and the small index are read.
        let dir = themes_dir().join(&request.id);
        if let Some(b) = lookup(&dir, &request.file, request.blur, request.fit) {
            states.insert(request, State::Ready(b.clone()));
            return Lookup::Ready(b);
        }
        let waiting = ReactiveContext::current().into_iter().collect();
        states.insert(request.clone(), State::Pending(waiting));
        drop(states);
        std::thread::spawn(move || {
            let built = {
                let _one = BUILDING.lock();
                let dir = themes_dir().join(&request.id);
                build(&dir, &request.file, request.blur, request.fit)
            };
            let Ok(mut states) = STATES.lock() else {
                return;
            };
            let state = match built {
                Ok((b, removed)) => {
                    // Requests whose file was just removed as stale must be built again.
                    states.retain(|r, s| {
                        !(r.id == request.id
                            && matches!(s, State::Ready(old) if removed.contains(&old.file)))
                    });
                    State::Ready(b)
                }
                Err(e) => {
                    eprintln!("could not blur the background of theme {}: {e}", request.id);
                    State::Failed
                }
            };
            if let Some(State::Pending(waiting)) = states.insert(request, state) {
                for rc in waiting {
                    rc.mark_dirty();
                }
            }
        });
        Lookup::Building
    }

    /// Whether `name` is a file name the cache writes.
    pub fn is_cache_name(name: &str) -> bool {
        let Some(rest) = name.strip_prefix(PREFIX) else {
            return false;
        };
        let Some(hash) = rest
            .strip_suffix(".jpg")
            .or_else(|| rest.strip_suffix(".png"))
        else {
            return false;
        };
        hash.len() == 16 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    /// FNV-1a over everything that shapes the result: stable across builds and runs, unlike
    /// the std hasher.
    pub(super) fn cache_key(bytes: &[u8], blur: u8, fit: Fit) -> u64 {
        let fit = Fit::ALL.iter().position(|f| *f == fit).unwrap_or(0) as u8;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let reference = [REFERENCE.0 as u16, REFERENCE.1 as u16].map(u16::to_le_bytes);
        let head = [FORMAT, blur, fit];
        for b in head.iter().chain(reference.iter().flatten()).chain(bytes) {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    /// The picture's size and change time (nanoseconds since 1970), which the index matches.
    fn stamp(path: &Path) -> Option<(u64, u128)> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        Some((meta.len(), modified))
    }

    fn fit_index(fit: Fit) -> usize {
        Fit::ALL.iter().position(|f| *f == fit).unwrap_or(0)
    }

    /// The cache file the index lists for picture `file` in theme folder `dir`, if the index
    /// matches the picture as it is now and these settings, and the file is there.
    pub(super) fn lookup(dir: &Path, file: &str, blur: u8, fit: Fit) -> Option<Blurred> {
        let (len, modified) = stamp(&dir.join(file))?;
        let cache = dir.join(CACHE_DIR_NAME);
        let text = std::fs::read(cache.join(INDEX)).ok()?;
        // One short line; anything else isn't ours.
        if text.len() > 1024 {
            return None;
        }
        let text = String::from_utf8(text).ok()?;
        let fields: Vec<&str> = text.trim_end().split('\t').collect();
        let [format, f, b, l, m, source, name, w, h] = fields[..] else {
            return None;
        };
        let matches = format == FORMAT.to_string()
            && f == fit_index(fit).to_string()
            && b == blur.to_string()
            && l == len.to_string()
            && m == modified.to_string()
            && source == file;
        if !matches || !is_cache_name(name) || !cache.join(name).is_file() {
            return None;
        }
        Some(Blurred {
            file: name.to_string(),
            width: w.parse().ok()?,
            height: h.parse().ok()?,
        })
    }

    /// Records in the index which picture (`stamp`) and settings `blurred` was built from.
    /// Best effort: without an index the next start reads and hashes the picture again.
    fn write_index(
        cache: &Path,
        file: &str,
        (len, modified): (u64, u128),
        blur: u8,
        fit: Fit,
        blurred: &Blurred,
    ) {
        // Tabs and line breaks would break the line; such names just aren't indexed.
        if file.contains(['\t', '\n', '\r']) {
            return;
        }
        let line = format!(
            "{FORMAT}\t{}\t{blur}\t{len}\t{modified}\t{file}\t{}\t{}\t{}\n",
            fit_index(fit),
            blurred.file,
            blurred.width,
            blurred.height
        );
        let part = cache.join(format!("{INDEX}.part"));
        if std::fs::write(&part, line).is_ok() {
            let _ = std::fs::rename(&part, cache.join(INDEX));
        }
    }

    /// Makes sure the cache file for the picture `file` in theme folder `dir` exists, and
    /// removes the folder's other cache files. Returns it and the removed names.
    pub(super) fn build(
        dir: &Path,
        file: &str,
        blur: u8,
        fit: Fit,
    ) -> Result<(Blurred, Vec<String>), String> {
        // Taken before reading, so a picture changed meanwhile never matches the index.
        let stamp = stamp(&dir.join(file));
        let bytes = std::fs::read(dir.join(file)).map_err(|e| e.to_string())?;
        let key = cache_key(&bytes, blur, fit);
        let cache = dir.join(CACHE_DIR_NAME);
        let existing = ["jpg", "png"]
            .map(|ext| format!("{PREFIX}{key:016x}.{ext}"))
            .into_iter()
            .find(|name| cache.join(name).is_file());
        let (name, width, height) = match existing {
            Some(name) => {
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?
                    .into_dimensions()
                    .map_err(|e| e.to_string())?;
                (name, w, h)
            }
            None => {
                let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
                let (w, h) = img.dimensions();
                let (data, ext) = render(&img, blur, fit)?;
                let name = format!("{PREFIX}{key:016x}.{ext}");
                std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
                // Written aside first, so a half-written file is never served.
                let part = cache.join(format!("{name}.part"));
                std::fs::write(&part, data).map_err(|e| e.to_string())?;
                std::fs::rename(&part, cache.join(&name)).map_err(|e| e.to_string())?;
                (name, w, h)
            }
        };
        let removed = remove_stale(&cache, &name);
        let blurred = Blurred {
            file: name,
            width,
            height,
        };
        if let Some(stamp) = stamp {
            write_index(&cache, file, stamp, blur, fit, &blurred);
        }
        Ok((blurred, removed))
    }

    /// Removes cache files other than `keep` (and leftover partial writes); other files
    /// in the folder are left alone.
    pub(super) fn remove_stale(cache: &Path, keep: &str) -> Vec<String> {
        let mut removed = Vec::new();
        let Ok(entries) = std::fs::read_dir(cache) else {
            return removed;
        };
        for name in entries.filter_map(|e| e.ok()?.file_name().into_string().ok()) {
            let ours = is_cache_name(&name)
                || name
                    .strip_suffix(".part")
                    .is_some_and(|n| is_cache_name(n) && n != keep);
            if ours && name != keep && std::fs::remove_file(cache.join(&name)).is_ok() {
                removed.push(name);
            }
        }
        removed
    }

    /// The blurred picture, shrunk, as file bytes and extension: jpg, or png when the
    /// picture has transparency.
    fn render(img: &DynamicImage, blur: u8, fit: Fit) -> Result<(Vec<u8>, &'static str), String> {
        let (w, h) = img.dimensions();
        let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
        // Screen pixels per picture pixel at the reference size.
        let scale = match fit {
            Fit::Cover => (REFERENCE.0 / wf).max(REFERENCE.1 / hf),
            Fit::Contain => (REFERENCE.0 / wf).min(REFERENCE.1 / hf),
            Fit::Tile | Fit::Center => 1.0,
        };
        let sigma = f32::from(blur) / scale;
        let shrink = (OUTPUT_SIGMA / sigma).min(MAX_SIDE / wf.max(hf)).min(1.0);
        let ow = ((wf * shrink).round() as u32).max(8);
        let oh = ((hf * shrink).round() as u32).max(8);
        let small = img
            .resize_exact(ow, oh, imageops::FilterType::Triangle)
            .to_rgba8();
        let sigma = sigma * ow as f32 / wf;
        let alpha = small.pixels().any(|p| p[3] < 255);
        let mut out = blur_padded(&small, sigma, fit == Fit::Tile, alpha);
        let mut data = Vec::new();
        if alpha {
            unpremultiply(&mut out);
            out.write_to(
                &mut std::io::Cursor::new(&mut data),
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
            Ok((data, "png"))
        } else {
            let rgb = DynamicImage::ImageRgba8(out).to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 90)
                .encode_image(&rgb)
                .map_err(|e| e.to_string())?;
            Ok((data, "jpg"))
        }
    }

    /// Gaussian blur with the edges extended (repeated for tiles, which then stay
    /// seamless), like the webview's backdrop blur.
    fn blur_padded(img: &RgbaImage, sigma: f32, wrap: bool, premultiply: bool) -> RgbaImage {
        let (w, h) = img.dimensions();
        let pad = (sigma * 3.0).ceil() as u32 + 1;
        let at = |x: i64, size: u32| -> u32 {
            if wrap {
                x.rem_euclid(i64::from(size)) as u32
            } else {
                x.clamp(0, i64::from(size) - 1) as u32
            }
        };
        let padded: RgbaImage = ImageBuffer::from_fn(w + 2 * pad, h + 2 * pad, |x, y| {
            let p = *img.get_pixel(
                at(i64::from(x) - i64::from(pad), w),
                at(i64::from(y) - i64::from(pad), h),
            );
            if premultiply {
                let a = u16::from(p[3]);
                let m = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
                Rgba([m(p[0]), m(p[1]), m(p[2]), p[3]])
            } else {
                p
            }
        });
        let blurred = imageops::blur(&padded, sigma);
        imageops::crop_imm(&blurred, pad, pad, w, h).to_image()
    }

    fn unpremultiply(img: &mut RgbaImage) {
        for p in img.pixels_mut() {
            let a = u16::from(p[3]);
            if a > 0 {
                for c in 0..3 {
                    p[c] = ((u16::from(p[c]) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_normalize() {
        assert_eq!(normalize_color("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_color(" #12Ab9f ").as_deref(), Some("#12ab9f"));
        assert_eq!(normalize_color("red"), None);
        assert_eq!(normalize_color("#12345"), None);
    }

    #[test]
    fn reserved_hues_are_refused() {
        assert_eq!(reserved_clash("#3b82f6"), Some("team blue"));
        assert_eq!(reserved_clash("#ef4444"), Some("team red"));
        assert_eq!(reserved_clash("#d946ef"), Some("fearless purple"));
        assert_eq!(reserved_clash("#10b981"), None);
        assert_eq!(reserved_clash("#777777"), None, "grays are fine");
        for c in DEFAULT_GROUP_COLORS {
            assert_eq!(reserved_clash(c), None, "{c}");
        }
        for n in 0..40 {
            let c = extra_group_color(n);
            assert_eq!(reserved_clash(&c), None, "{n}: {c}");
            assert_ne!(c, extra_group_color(n + 1));
        }
    }

    #[test]
    fn parses_a_partial_theme_with_warnings() {
        let theme = parse(
            "mine",
            r##"{
                "name": "Mine",
                "base": "light",
                "dim": 120,
                "colors": { "accent": "#f00", "blue": "#000000", "text": "nope" },
                "group_colors": ["#10b981", "#3b82f6", "bad"],
                "graph": { "phase_width": 5, "edge": "#123456", "turn_width": -1 },
                "extra": 1
            }"##,
            None,
        )
        .unwrap();
        assert_eq!(theme.base, Base::Light);
        assert_eq!(theme.colors.accent, "#ff0000");
        // Unset values come from the base.
        assert_eq!(theme.colors.bg, Theme::builtin(Base::Light).colors.bg);
        assert_eq!(theme.colors.text, Theme::builtin(Base::Light).colors.text);
        assert_eq!(theme.dim, 90);
        assert_eq!(theme.group_colors, ["#10b981"]);
        assert_eq!(theme.graph.phase_width, 5.0);
        assert_eq!(theme.graph.edge, "#123456");
        assert_eq!(theme.graph.turn_width, 1.0);
        let all = theme.warnings.join("\n");
        for needle in [
            "dim",
            "team color \"blue\"",
            "\"text\"",
            "team blue",
            "\"bad\"",
            "turn_width",
            "\"extra\"",
        ] {
            assert!(
                all.contains(needle),
                "missing warning about {needle}: {all}"
            );
        }
        assert!(parse("x", "{", None).is_err());
    }

    #[test]
    fn builtins_round_trip() {
        for base in [Base::Dark, Base::Light] {
            let b = Theme::builtin(base);
            let back = parse(&b.id, &b.to_json(), None).unwrap();
            assert_eq!(back, b);
        }
    }

    #[test]
    fn protocol_only_serves_pictures_in_theme_folders() {
        assert!(resolve_request("/dusk/background.jpg").is_some());
        assert!(resolve_request("/dusk/my%20pic.PNG").is_some());
        assert!(resolve_request("/dusk/theme.json").is_none());
        assert!(resolve_request("/../secret.png").is_none());
        assert!(resolve_request("/dusk/../x.png").is_none());
        assert!(resolve_request("/dusk/sub/x.png").is_none());
        assert!(resolve_request("/dusk/%2e%2e%2fx.png").is_none());
        assert!(resolve_request("/dusk").is_none());
    }

    #[test]
    fn protocol_serves_blur_cache_files_only() {
        let (path, mime) = resolve_request("/dusk/.cache/blur-0123456789abcdef.jpg").unwrap();
        assert!(path.ends_with("dusk/.cache/blur-0123456789abcdef.jpg"));
        assert_eq!(mime, "image/jpeg");
        assert_eq!(
            resolve_request("/dusk/.cache/blur-0123456789abcdef.png")
                .unwrap()
                .1,
            "image/png"
        );
        for refused in [
            "/dusk/.cache/theme.json",
            "/dusk/.cache/background.jpg",
            "/dusk/.cache/blur-0123456789ABCDEF.jpg",
            "/dusk/.cache/blur-0123.jpg",
            "/dusk/.cache/blur-0123456789abcdef.jpg.part",
            "/dusk/.cache/blur-0123456789abcdef.jpg/x",
            "/dusk/.cache/../theme.json",
            "/dusk/.cache/%2e%2e/blur-0123456789abcdef.jpg",
            "/dusk/%2ecache/blur-0123456789abcdef.jpg",
            "/../.cache/blur-0123456789abcdef.jpg",
            "/.cache/blur-0123456789abcdef.jpg",
            "/dusk/.cache",
        ] {
            assert!(resolve_request(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn blur_cache_key_follows_picture_and_blur() {
        let key = blur_cache::cache_key(b"picture", 8, Fit::Cover);
        assert_eq!(key, blur_cache::cache_key(b"picture", 8, Fit::Cover));
        assert_ne!(key, blur_cache::cache_key(b"picturf", 8, Fit::Cover));
        assert_ne!(key, blur_cache::cache_key(b"picture", 9, Fit::Cover));
        assert_ne!(key, blur_cache::cache_key(b"picture", 8, Fit::Tile));
    }

    /// A fresh empty folder under the system temp folder.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draftree-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stale_blur_files_are_removed() {
        let dir = temp_dir("stale");
        let keep = "blur-00000000000000aa.jpg";
        for name in [
            keep,
            "blur-00000000000000bb.jpg",
            "blur-00000000000000cc.png",
            "blur-00000000000000dd.jpg.part",
            "notes.txt",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let mut removed = blur_cache::remove_stale(&dir, keep);
        removed.sort();
        assert_eq!(
            removed,
            [
                "blur-00000000000000bb.jpg",
                "blur-00000000000000cc.png",
                "blur-00000000000000dd.jpg.part"
            ]
        );
        assert!(dir.join(keep).exists());
        assert!(dir.join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blur_cache_rebuilds_when_blur_or_picture_changes() {
        let dir = temp_dir("build");
        let picture =
            image::RgbImage::from_fn(64, 36, |x, y| image::Rgb([x as u8 * 4, y as u8 * 7, 90]));
        picture.save(dir.join("bg.png")).unwrap();
        let cache = dir.join(CACHE_DIR_NAME);
        let files = || {
            let mut names: Vec<String> = std::fs::read_dir(&cache)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .filter(|n| blur_cache::is_cache_name(n))
                .collect();
            names.sort();
            names
        };

        let (first, removed) = blur_cache::build(&dir, "bg.png", 8, Fit::Cover).unwrap();
        assert!(removed.is_empty());
        assert_eq!((first.width, first.height), (64, 36));
        assert!(first.file.ends_with(".jpg"));
        assert_eq!(files(), std::slice::from_ref(&first.file));
        // The same picture and blur reuse the file.
        let (again, _) = blur_cache::build(&dir, "bg.png", 8, Fit::Cover).unwrap();
        assert_eq!(again, first);

        let (blurrier, removed) = blur_cache::build(&dir, "bg.png", 12, Fit::Cover).unwrap();
        assert_ne!(blurrier.file, first.file);
        assert_eq!(removed, std::slice::from_ref(&first.file));
        assert_eq!(files(), std::slice::from_ref(&blurrier.file));

        let picture = image::RgbImage::from_pixel(64, 36, image::Rgb([200, 10, 10]));
        picture.save(dir.join("bg.png")).unwrap();
        let (changed, removed) = blur_cache::build(&dir, "bg.png", 12, Fit::Cover).unwrap();
        assert_ne!(changed.file, blurrier.file);
        assert_eq!(removed, [blurrier.file]);
        assert_eq!(files(), [changed.file]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blur_cache_index_finds_the_file_without_reading_the_picture() {
        let dir = temp_dir("index");
        let picture =
            image::RgbImage::from_fn(64, 36, |x, y| image::Rgb([x as u8 * 4, y as u8 * 7, 90]));
        picture.save(dir.join("bg.png")).unwrap();
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover), None);
        let (built, _) = blur_cache::build(&dir, "bg.png", 8, Fit::Cover).unwrap();
        assert_eq!(
            blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover),
            Some(built.clone())
        );
        // Other settings or another picture don't match.
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 9, Fit::Cover), None);
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Tile), None);
        assert_eq!(blur_cache::lookup(&dir, "other.png", 8, Fit::Cover), None);
        // A changed picture doesn't match until it is built again.
        let picture = image::RgbImage::from_pixel(80, 36, image::Rgb([200, 10, 10]));
        picture.save(dir.join("bg.png")).unwrap();
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover), None);
        let (rebuilt, _) = blur_cache::build(&dir, "bg.png", 8, Fit::Cover).unwrap();
        assert_ne!(rebuilt, built);
        assert_eq!(
            blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover),
            Some(rebuilt.clone())
        );
        // A missing cache file or a damaged index isn't trusted.
        let cache = dir.join(CACHE_DIR_NAME);
        std::fs::remove_file(cache.join(&rebuilt.file)).unwrap();
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover), None);
        blur_cache::build(&dir, "bg.png", 8, Fit::Cover).unwrap();
        let index = cache.join("blur.idx");
        let line = std::fs::read_to_string(&index).unwrap();
        std::fs::write(&index, line.replace(&rebuilt.file, "../../escape.jpg")).unwrap();
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover), None);
        std::fs::write(&index, "garbage").unwrap();
        assert_eq!(blur_cache::lookup(&dir, "bg.png", 8, Fit::Cover), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn group_colors_extend_past_the_list() {
        let mut t = Theme::builtin(Base::Dark);
        t.group_colors = vec!["#10b981".into()];
        assert_eq!(t.group_color(0), "#10b981");
        assert_eq!(t.group_color(1), extra_group_color(0));
    }
}
