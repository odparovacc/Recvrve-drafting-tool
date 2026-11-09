//! Exports: the tree as PNG, SVG or XML, draft states as a folder of images, and draft
//! states as a PDF. None of them can be loaded back into the app.
//!
//! The UI takes a [`Snapshot`] of the tree and hands it to [`start`], which runs the export
//! on its own thread so the window stays responsive. Card rendering inside a job is spread
//! over all cores with rayon. The job reports progress through shared atomics that the UI
//! polls, and stops early when the user cancels.

pub mod cards;
pub mod draw;
pub mod icons;
pub mod model;
pub mod pdf;
pub mod pdf_draw;
pub mod raster;
pub mod scene;
pub mod states;
pub mod svg;
pub mod tree_image;
pub mod xml;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub use model::Snapshot;

pub const EXPORTS_DIR_NAME: &str = "exports";

/// The default folder for exports (see [`crate::configio::app_folder`]).
pub fn exports_dir() -> PathBuf {
    crate::configio::app_folder(EXPORTS_DIR_NAME)
}

/// Local time for file names: `20261001-1530`.
pub fn file_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M").to_string()
}

/// Local time for printing on exports: `2026-10-01 15:30`.
pub fn display_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Progress of a running export, shared between the worker and the UI.
#[derive(Debug, Default)]
pub struct Progress {
    total: AtomicUsize,
    done: AtomicUsize,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn step(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Export cancelled.")
    }
}

/// What a finished export produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub message: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeFormat {
    Png,
    Svg,
    Xml,
}

impl TreeFormat {
    pub const ALL: [TreeFormat; 3] = [TreeFormat::Png, TreeFormat::Svg, TreeFormat::Xml];

    pub fn extension(self) -> &'static str {
        match self {
            TreeFormat::Png => "png",
            TreeFormat::Svg => "svg",
            TreeFormat::Xml => "xml",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TreeFormat::Png => "PNG",
            TreeFormat::Svg => "SVG",
            TreeFormat::Xml => "XML",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeOptions {
    pub format: TreeFormat,
    /// Pixels per logical pixel, for PNG.
    pub scale: f32,
    pub legend: bool,
    /// Top to bottom (true) or left to right (false).
    pub portrait: bool,
    pub path: PathBuf,
}

/// Trees with more lines than this are better exported as SVG.
pub const SVG_SUGGESTED_LINES: usize = 400;

/// Logical size of the tree image (title, legend and tree).
pub fn tree_image_size(snap: &Snapshot, legend: bool, portrait: bool) -> (f32, f32) {
    let scene = tree_image::build(
        snap,
        tree_image::TreeImageOptions { legend, portrait },
        "Draft tree",
        "",
    );
    (scene.width, scene.height)
}

pub enum Request {
    Tree(Snapshot, TreeOptions),
    Images(Snapshot, states::ImagesOptions),
    Pdf(Snapshot, pdf::PdfOptions, PathBuf),
}

/// A running export.
#[derive(Clone)]
pub struct Job {
    pub progress: Arc<Progress>,
    result: Arc<Mutex<Option<Result<Outcome, String>>>>,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.progress, &other.progress)
    }
}

impl Job {
    /// The result once the export has finished.
    pub fn take_result(&self) -> Option<Result<Outcome, String>> {
        self.result.lock().ok()?.take()
    }
}

/// Starts `request` on a background thread.
pub fn start(request: Request) -> Job {
    let job = Job {
        progress: Arc::new(Progress::default()),
        result: Arc::new(Mutex::new(None)),
    };
    let worker = job.clone();
    std::thread::spawn(move || {
        // A panic in an export must not leave the UI waiting forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(request, &worker.progress)
        }))
        .unwrap_or_else(|_| Err("The export failed unexpectedly.".to_string()));
        if let Ok(mut slot) = worker.result.lock() {
            *slot = Some(result);
        }
    });
    job
}

/// Writes `bytes` to `path` through a temporary file, so a failed export never leaves a
/// half-written file behind.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".part");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Could not write {}: {e}", path.display())
        })
}

fn saved(path: &Path) -> Outcome {
    Outcome {
        message: format!("Saved {}", path.display()),
        path: path.to_path_buf(),
    }
}

fn run(request: Request, progress: &Progress) -> Result<Outcome, String> {
    match request {
        Request::Tree(snap, options) => {
            progress.set_total(2);
            let date = display_stamp();
            let bytes = match options.format {
                TreeFormat::Xml => {
                    xml::render(&snap, &chrono::Local::now().to_rfc3339()).into_bytes()
                }
                format => {
                    let scene = tree_image::build(
                        &snap,
                        tree_image::TreeImageOptions {
                            legend: options.legend,
                            portrait: options.portrait,
                        },
                        "Draft tree",
                        &date,
                    );
                    let icons = icons::shared();
                    progress.step();
                    if progress.is_cancelled() {
                        return Err(Cancelled.to_string());
                    }
                    if format == TreeFormat::Svg {
                        svg::render(&scene, icons).into_bytes()
                    } else {
                        let (w, h) = raster::pixel_size(scene.width, scene.height, options.scale);
                        if !raster::fits(w, h) {
                            return Err(format!(
                                "The image would be {w} × {h} pixels, too large for PNG. Export it as SVG or pick a smaller scope."
                            ));
                        }
                        let pixmap = raster::render(&scene, options.scale, icons)
                            .ok_or("Could not allocate the image.")?;
                        raster::encode(pixmap, raster::ImageFormat::Png)?
                    }
                }
            };
            if progress.is_cancelled() {
                return Err(Cancelled.to_string());
            }
            write_file(&options.path, &bytes)?;
            progress.step();
            Ok(saved(&options.path))
        }
        Request::Images(snap, options) => states::run(&snap, &options, icons::shared(), progress),
        Request::Pdf(snap, options, path) => {
            let cards: usize = options
                .sections
                .iter()
                .map(|&d| snap.at_depth(d).len())
                .sum();
            progress.set_total(cards);
            let bytes = pdf::render(&snap, &options, icons::shared(), &display_stamp(), progress)
                .map_err(|e| e.to_string())?;
            write_file(&path, &bytes)?;
            Ok(saved(&path))
        }
    }
}

/// Human-readable byte count.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else {
        format!("{:.2} GB", b / KB / KB / KB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 + 1024 * 300), "5.3 MB");
    }

    #[test]
    fn progress_counts_and_cancels() {
        let p = Progress::default();
        p.set_total(3);
        p.step();
        p.step();
        assert_eq!((p.done(), p.total()), (2, 3));
        assert!(!p.is_cancelled());
        p.cancel();
        assert!(p.is_cancelled());
    }
}
