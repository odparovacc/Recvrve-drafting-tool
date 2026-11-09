//! Draft state images: one card per state at each chosen depth, written in parallel into
//! a new folder with one subfolder per draft position, plus an optional group legend
//! image and zip archive.
//!
//! ```text
//! exports/recvrve-states-20261001-1530/
//!   00_legend.webp
//!   A06-RB3/A06-RB3_0001.webp
//!   A06-RB3/A06-RB3_0002.webp
//!   A12-RP3/...
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;

use super::cards::{self, CARD_W};
use super::draw::full_legend;
use super::icons::Icons;
use super::model::{self, Snapshot};
use super::raster::{self, ImageFormat};
use super::{Cancelled, Outcome, Progress};

#[derive(Clone, Debug, PartialEq)]
pub struct ImagesOptions {
    /// Draft depths (1-20) to export.
    pub depths: Vec<usize>,
    pub format: ImageFormat,
    /// Card width in pixels.
    pub width_px: u32,
    pub legend: bool,
    pub zip: bool,
    /// Folder to create; must not exist yet.
    pub folder: PathBuf,
}

/// Pixel widths offered for cards.
pub const WIDTHS: [u32; 3] = [600, 900, 1200];

/// Rough encoded size of one card at 900 px wide, measured on typical drafts.
fn bytes_per_card_900(format: ImageFormat) -> f64 {
    match format {
        ImageFormat::WebP => 185_000.0,
        ImageFormat::Png => 200_000.0,
        ImageFormat::Jpeg => 165_000.0,
    }
}

/// Estimated total size in bytes of `count` cards.
pub fn estimate_bytes(count: usize, format: ImageFormat, width_px: u32) -> u64 {
    let area = (f64::from(width_px) / 900.0).powi(2);
    (count as f64 * bytes_per_card_900(format) * area) as u64
}

/// The states to export: (depth, node, number at that depth, states at that depth).
fn jobs(snap: &Snapshot, depths: &[usize]) -> Vec<(usize, usize, usize, usize)> {
    let mut jobs = Vec::new();
    for &depth in depths {
        let states = snap.at_depth(depth);
        let count = states.len();
        for (k, node) in states.into_iter().enumerate() {
            jobs.push((depth, node, k + 1, count));
        }
    }
    jobs
}

/// `folder`, or `folder-2`, `folder-3`, ... if it already exists.
pub fn unique_folder(folder: &Path) -> PathBuf {
    if !folder.exists() {
        return folder.to_path_buf();
    }
    let name = folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    (2..)
        .map(|n| folder.with_file_name(format!("{name}-{n}")))
        .find(|p| !p.exists())
        .expect("some suffix is free")
}

pub fn run(
    snap: &Snapshot,
    options: &ImagesOptions,
    icons: &Icons,
    progress: &Progress,
) -> Result<Outcome, String> {
    let jobs = jobs(snap, &options.depths);
    let legend_groups = {
        let mut nodes = Vec::new();
        for &(_, node, _, _) in &jobs {
            nodes.extend(snap.path(node));
        }
        snap.groups_in(nodes)
    };
    let with_legend = options.legend && (!legend_groups.is_empty() || snap.fearless.is_active());
    progress.set_total(jobs.len() + usize::from(with_legend) + usize::from(options.zip));

    let folder = &options.folder;
    std::fs::create_dir_all(folder.parent().unwrap_or(Path::new(".")))
        .map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
    std::fs::create_dir(folder)
        .map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
    let result = write_all(
        snap,
        options,
        icons,
        progress,
        &jobs,
        with_legend,
        &legend_groups,
    );
    match result {
        Ok(zip) => {
            let count = jobs.len() + usize::from(with_legend);
            let mut message = format!(
                "Saved {count} image{} to {}",
                if count == 1 { "" } else { "s" },
                folder.display()
            );
            if let Some(zip) = &zip {
                message.push_str(&format!(" and {}", zip.display()));
            }
            Ok(Outcome {
                message,
                path: folder.clone(),
            })
        }
        Err(e) => {
            // The folder is new and ours, so a cancelled or failed export leaves nothing.
            let _ = std::fs::remove_dir_all(folder);
            let _ = std::fs::remove_file(zip_path(folder));
            Err(match e {
                WriteError::Cancelled => Cancelled.to_string(),
                WriteError::Failed(e) => e,
            })
        }
    }
}

enum WriteError {
    Cancelled,
    Failed(String),
}

fn zip_path(folder: &Path) -> PathBuf {
    let mut name = folder.file_name().unwrap_or_default().to_os_string();
    name.push(".zip");
    folder.with_file_name(name)
}

fn write_all(
    snap: &Snapshot,
    options: &ImagesOptions,
    icons: &Icons,
    progress: &Progress,
    jobs: &[(usize, usize, usize, usize)],
    with_legend: bool,
    legend_groups: &[&crate::groups::Group],
) -> Result<Option<PathBuf>, WriteError> {
    let folder = &options.folder;
    let ext = options.format.extension();
    let scale = options.width_px as f32 / CARD_W;
    let mut written: Vec<PathBuf> = Vec::new();

    for &depth in &options.depths {
        std::fs::create_dir_all(folder.join(model::depth_code(depth)))
            .map_err(|e| WriteError::Failed(format!("Could not create a folder: {e}")))?;
    }

    if with_legend {
        let mut scene = full_legend(snap, legend_groups, CARD_W * 1.5, "Groups");
        // Margins around the legend.
        let mut padded = super::scene::Scene::new(scene.width + 32.0, scene.height + 32.0);
        padded.append(&scene, 16.0, 16.0);
        scene = padded;
        let path = folder.join(format!("00_legend.{ext}"));
        render_to(&scene, scale, icons, options.format, &path)?;
        written.push(path);
        progress.step();
    }

    // Cards render and encode in parallel; the first error stops the rest.
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let paths: Vec<PathBuf> = jobs
        .par_iter()
        .filter_map(|&(depth, node, seq, count)| {
            if progress.is_cancelled() || failure.lock().is_ok_and(|f| f.is_some()) {
                return None;
            }
            let code = model::depth_code(depth);
            let path = folder
                .join(&code)
                .join(format!("{code}_{}.{ext}", model::seq_text(seq, count)));
            let scene = cards::card(snap, node, seq, count);
            match render_to(&scene, scale, icons, options.format, &path) {
                Ok(()) => {
                    progress.step();
                    Some(path)
                }
                Err(WriteError::Failed(e)) => {
                    if let Ok(mut f) = failure.lock() {
                        f.get_or_insert(e);
                    }
                    None
                }
                Err(WriteError::Cancelled) => None,
            }
        })
        .collect();
    if progress.is_cancelled() {
        return Err(WriteError::Cancelled);
    }
    if let Some(e) = failure.into_inner().ok().flatten() {
        return Err(WriteError::Failed(e));
    }
    written.extend(paths);

    if !options.zip {
        return Ok(None);
    }
    let zip = zip_path(folder);
    write_zip(&zip, folder, &written)
        .map_err(|e| WriteError::Failed(format!("Could not write the zip file: {e}")))?;
    progress.step();
    Ok(Some(zip))
}

fn render_to(
    scene: &super::scene::Scene,
    scale: f32,
    icons: &Icons,
    format: ImageFormat,
    path: &Path,
) -> Result<(), WriteError> {
    let pixmap = raster::render(scene, scale, icons)
        .ok_or_else(|| WriteError::Failed("Could not allocate an image.".into()))?;
    let bytes = raster::encode(pixmap, format).map_err(WriteError::Failed)?;
    std::fs::write(path, bytes)
        .map_err(|e| WriteError::Failed(format!("Could not write {}: {e}", path.display())))
}

/// Packs `files` (all inside `folder`) into a zip. Images are already compressed, so they
/// are stored as they are.
fn write_zip(zip: &Path, folder: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let base = folder.parent().unwrap_or(Path::new(""));
    let file = std::fs::File::create(zip)?;
    let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut files = files.to_vec();
    files.sort();
    for path in files {
        let name = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        writer
            .start_file(name, options)
            .map_err(std::io::Error::other)?;
        writer.write_all(&std::fs::read(&path)?)?;
    }
    writer.finish().map_err(std::io::Error::other)?.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::icons;
    use crate::tree::DraftTree;
    use std::collections::HashMap;

    #[test]
    fn writes_folders_and_zip() {
        let mut t = DraftTree::new();
        let root = t.root();
        for k in 0..3u8 {
            let mut at = root;
            for v in [16 + k, 1, 190, 50 + k] {
                at = t.add_child(at, if v == 190 { 3 } else { v }).unwrap();
            }
        }
        let snap = Snapshot::build(&t, None, &HashMap::new(), &[], false, true);
        let dir = std::env::temp_dir().join(format!("draftree-states-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let options = ImagesOptions {
            depths: vec![1, 4],
            format: ImageFormat::WebP,
            width_px: 600,
            legend: true,
            zip: true,
            folder: dir.join("out"),
        };
        let progress = Progress::default();
        let outcome = run(&snap, &options, icons::for_tests(), &progress).unwrap();
        assert_eq!(progress.done(), progress.total());
        assert!(outcome.path.join("00_legend.webp").is_file());
        assert!(outcome.path.join("A01-BB1/A01-BB1_0003.webp").is_file());
        assert!(outcome.path.join("A04-RB2/A04-RB2_0001.webp").is_file());
        assert!(dir.join("out.zip").is_file());
        let size = std::fs::metadata(outcome.path.join("A04-RB2/A04-RB2_0001.webp"))
            .unwrap()
            .len();
        eprintln!("600px webp card: {size} bytes");
        assert_eq!(unique_folder(&options.folder), dir.join("out-2"));

        // A cancelled export removes what it wrote.
        let progress = Progress::default();
        progress.cancel();
        let options = ImagesOptions {
            folder: dir.join("cancelled"),
            ..options
        };
        assert!(run(&snap, &options, icons::for_tests(), &progress).is_err());
        assert!(!dir.join("cancelled").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
