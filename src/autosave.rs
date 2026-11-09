//! Autosave and crash recovery.
//!
//! Each run of the app is a session with its own folder in `saves/autosave/`, holding an
//! exclusive lock on a file inside it for as long as the app runs. Every few minutes
//! (two by default), the tabs whose tree changed since the last autosave are written there
//! in the compact save format, on a background thread, through a temporary file so a crash
//! mid-write can't leave a broken file. Unchanged tabs cost nothing. A small
//! `window-N.json` per window lists its tabs in order (tab ids are unique across windows,
//! so a tab moving to another window keeps its file). A window that closes removes its
//! list and tab files.
//!
//! A clean exit deletes the session's folder. A folder whose lock is free at startup
//! belongs to a session that ended without that (a crash, a kill, a power cut): the app
//! offers to restore its tabs, and deletes the folder either way.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const DIR_NAME: &str = "autosave";
const LOCK_FILE: &str = "lock";
/// The list of tabs before there were several windows (still read when restoring).
const MANIFEST_FILE: &str = "session.json";
const WINDOW_PREFIX: &str = "window-";
const SESSION_PREFIX: &str = "session-";

/// One tab in a session's list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    /// The tab's name when it was saved.
    pub name: String,
    /// The file the tab was loaded from or saved to.
    pub source: Option<PathBuf>,
    /// Whether the tab had changes its file doesn't have.
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub tabs: Vec<Entry>,
}

/// A tab left behind by a session that didn't close cleanly.
#[derive(Clone, Debug)]
pub struct Recovered {
    pub entry: Entry,
    pub bytes: Vec<u8>,
}

/// The sessions found at startup that didn't close cleanly: their folders, and their tabs.
#[derive(Clone, Debug, Default)]
pub struct Crashed {
    pub dirs: Vec<PathBuf>,
    pub tabs: Vec<Recovered>,
}

/// This run's session: its folder, and the open lock file that marks it as running.
struct Session {
    dir: PathBuf,
    _lock: File,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
static CRASHED: OnceLock<Crashed> = OnceLock::new();

fn root() -> PathBuf {
    crate::save::saves_dir().join(DIR_NAME)
}

pub fn tab_file(id: u64) -> String {
    format!("tab-{id}.dts")
}

fn window_file(window: u64) -> String {
    format!("{WINDOW_PREFIX}{window}.json")
}

/// Finds sessions that didn't close cleanly (in `root`), then starts this run's session.
/// Call once, before the window opens.
pub fn start() {
    let root = root();
    CRASHED.get_or_init(|| find_crashed(&root));
    let session = start_in(&root)
        .map_err(|e| {
            eprintln!(
                "autosave is off: could not start a session in {}: {e}",
                root.display()
            )
        })
        .ok();
    SESSION.get_or_init(|| Mutex::new(session));
}

fn start_in(root: &Path) -> std::io::Result<Session> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dir = root.join(format!("{SESSION_PREFIX}{stamp}-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let lock = File::create(dir.join(LOCK_FILE))?;
    lock.try_lock().map_err(std::io::Error::other)?;
    Ok(Session { dir, _lock: lock })
}

/// What [`start`] found.
pub fn crashed() -> Crashed {
    CRASHED.get().cloned().unwrap_or_default()
}

/// Session folders in `root` whose lock nobody holds, with the tabs they list.
fn find_crashed(root: &Path) -> Crashed {
    let mut found = Crashed::default();
    let Ok(read) = std::fs::read_dir(root) else {
        return found;
    };
    let mut dirs: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(SESSION_PREFIX))
        })
        .collect();
    dirs.sort();
    for dir in dirs {
        // A running session holds its lock; a free (or missing) lock means it ended
        // without cleaning up.
        let running = File::options()
            .write(true)
            .open(dir.join(LOCK_FILE))
            .is_ok_and(|f| f.try_lock().is_err());
        if running {
            continue;
        }
        // Every window's list, in the order the windows opened.
        let mut lists: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let number = if name == MANIFEST_FILE {
                    0
                } else {
                    name.strip_prefix(WINDOW_PREFIX)?
                        .strip_suffix(".json")?
                        .parse()
                        .ok()?
                };
                Some((number, e.path()))
            })
            .collect();
        lists.sort();
        for (_, list) in lists {
            let manifest: Manifest = std::fs::read_to_string(list)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            for entry in manifest.tabs {
                if found.tabs.iter().any(|t| t.entry.id == entry.id) {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(dir.join(tab_file(entry.id))) {
                    found.tabs.push(Recovered { entry, bytes });
                }
            }
        }
        found.dirs.push(dir);
    }
    found
}

/// Deletes the folders of sessions that didn't close cleanly (after restoring them, or
/// when the user doesn't want them).
pub fn discard(crashed: &Crashed) {
    for dir in &crashed.dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// This session's folder, if autosave works.
fn session_dir() -> Option<PathBuf> {
    let guard = SESSION.get()?.lock().ok()?;
    guard.as_ref().map(|s| s.dir.clone())
}

/// Writes `bytes` to `path` through a temporary file, so the old file stays whole until
/// the new one is complete.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// One autosave round of window `window`: the tabs whose trees changed (id and encoding),
/// its tab list, and the tabs closed since the last round. Written on a background thread.
pub fn write(window: u64, changed: Vec<(u64, Vec<u8>)>, manifest: Manifest, closed: Vec<u64>) {
    let Some(dir) = session_dir() else {
        return;
    };
    std::thread::spawn(move || {
        for (id, bytes) in changed {
            if let Err(e) = write_atomic(&dir.join(tab_file(id)), &bytes) {
                eprintln!("autosave failed: {e}");
            }
        }
        for id in closed {
            let _ = std::fs::remove_file(dir.join(tab_file(id)));
        }
        let json = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
        if let Err(e) = write_atomic(&dir.join(window_file(window)), json.as_bytes()) {
            eprintln!("autosave failed: {e}");
        }
    });
}

/// Window `window` closed cleanly: removes its list and its tabs' files.
pub fn close_window(window: u64, tabs: Vec<u64>) {
    let Some(dir) = session_dir() else {
        return;
    };
    std::thread::spawn(move || {
        let _ = std::fs::remove_file(dir.join(window_file(window)));
        for id in tabs {
            let _ = std::fs::remove_file(dir.join(tab_file(id)));
        }
    });
}

/// A clean exit: deletes this session's folder (and lets go of the lock).
pub fn finish() {
    let Some(mutex) = SESSION.get() else {
        return;
    };
    let Ok(mut guard) = mutex.lock() else {
        return;
    };
    if let Some(session) = guard.take() {
        let dir = session.dir.clone();
        drop(session);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("recvrve-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_windows_tabs_are_found() {
        let root = temp_root("windows");
        let dead = root.join(format!("{SESSION_PREFIX}20260101-000000-2"));
        std::fs::create_dir_all(&dead).unwrap();
        let entry = |id: u64| Entry {
            id,
            name: format!("Draft {id}"),
            source: None,
            dirty: true,
        };
        let list = |ids: &[u64]| Manifest {
            tabs: ids.iter().map(|&id| entry(id)).collect(),
        };
        // Windows 10 and 2 (listed after window 2's), and a stray list naming tab 3 again.
        for (file, ids) in [
            ("window-10.json", vec![7]),
            ("window-2.json", vec![3, 5]),
            ("window-x.json", vec![3]),
        ] {
            std::fs::write(dead.join(file), serde_json::to_string(&list(&ids)).unwrap()).unwrap();
        }
        for id in [3, 5, 7] {
            std::fs::write(dead.join(tab_file(id)), format!("tree {id}")).unwrap();
        }
        let found = find_crashed(&root);
        let ids: Vec<u64> = found.tabs.iter().map(|t| t.entry.id).collect();
        assert_eq!(ids, [3, 5, 7]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn running_sessions_are_left_alone_and_dead_ones_found() {
        let root = temp_root("find");
        // A running session: its lock is held.
        let running = start_in(&root).unwrap();
        std::fs::write(running.dir.join(tab_file(1)), b"x").unwrap();
        // A dead one: a folder with a free lock, a list and one tab file (the other is
        // missing and skipped).
        let dead = root.join(format!("{SESSION_PREFIX}20260101-000000-1"));
        std::fs::create_dir_all(&dead).unwrap();
        File::create(dead.join(LOCK_FILE)).unwrap();
        let manifest = Manifest {
            tabs: vec![
                Entry {
                    id: 4,
                    name: "G2 vs Fnatic".into(),
                    source: Some("saves/a.dts".into()),
                    dirty: true,
                },
                Entry {
                    id: 5,
                    name: "Draft 2".into(),
                    source: None,
                    dirty: true,
                },
            ],
        };
        std::fs::write(
            dead.join(MANIFEST_FILE),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(dead.join(tab_file(4)), b"tree").unwrap();
        // Something else in the folder is ignored.
        std::fs::create_dir_all(root.join("other")).unwrap();

        let found = find_crashed(&root);
        assert_eq!(found.dirs, std::slice::from_ref(&dead));
        assert_eq!(found.tabs.len(), 1);
        assert_eq!(found.tabs[0].entry, manifest.tabs[0]);
        assert_eq!(found.tabs[0].bytes, b"tree");
        discard(&found);
        assert!(!dead.exists());
        assert!(running.dir.exists());
        // Once the running session ends without cleaning up, it counts as dead.
        let dir = running.dir.clone();
        drop(running);
        assert_eq!(find_crashed(&root).dirs, [dir]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn atomic_writes_replace_files_whole() {
        let root = temp_root("write");
        let path = root.join("tab-1.dts");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!root.join("tab-1.tmp").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
