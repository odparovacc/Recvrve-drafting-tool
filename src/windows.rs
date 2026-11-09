//! Several windows: a tab can move to a window of its own and between windows.
//!
//! Each window runs its own app (its own tabs, selection, popups and panel sizes) on the
//! same UI thread. So the registry here is per thread: the windows open now, by number
//! (1 for the first), each with an inbox its app reads. A tab moves by leaving its window
//! with all of its state (see [`MovedTab`]) and arriving in another one's inbox, or in a
//! new window's start (see [`stash_start`]). Tab ids are unique across windows, so a
//! moved tab keeps its id and with it its autosave file.
//!
//! The settings, shortcuts, selection groups and themes are shared: a window that changes
//! them tells the others (see [`broadcast`]). So is the clipboard, which lives here, so
//! branches can be copied from one window to another.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dioxus::desktop::tao::window::WindowId;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::clip::Clip;
use crate::configio::UiSettings;
use crate::keys::Keybinds;
use crate::selection::SelectionGroup;
use crate::tabs::{Parked, TabInfo};

/// A tab on its way to another window: what its strip shows, and everything else it keeps.
pub struct MovedTab {
    pub info: TabInfo,
    pub parked: Parked,
}

/// What a window's app receives.
pub enum Message {
    /// A tab moved here from another window.
    Adopt(Box<MovedTab>),
    /// The window's close button (or Alt+F4): the window closes, after asking if any of
    /// its tabs have unsaved changes.
    CloseRequested,
    /// Another window changed the shared settings.
    Settings(Box<UiSettings>),
    Keybinds(Box<Keybinds>),
    SelectionGroups(Vec<SelectionGroup>),
    /// Another window saved a theme: the theme files changed.
    Themes,
    /// Another window downloaded new counter data.
    Counters,
}

/// A new window's app context: the key of the tab it opens with (see [`stash_start`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start(pub u64);

struct Window {
    number: u64,
    native: WindowId,
    /// What the window shows, for menus in the other windows.
    label: String,
    inbox: UnboundedSender<Message>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Window>> = const { RefCell::new(Vec::new()) };
    static STARTS: RefCell<HashMap<u64, MovedTab>> = RefCell::new(HashMap::new());
    static CLIPBOARD: RefCell<Option<Clip>> = const { RefCell::new(None) };
}

/// Copied or cut branches (Ctrl+C / Ctrl+X), for any window to paste. Kept across loads
/// and group config changes (a copy carries its own group definitions).
pub fn set_clipboard(clip: Clip) {
    CLIPBOARD.set(Some(clip));
}

pub fn clipboard() -> Option<Clip> {
    CLIPBOARD.with_borrow(Clone::clone)
}

static NEXT_WINDOW: AtomicU64 = AtomicU64::new(1);
static NEXT_START: AtomicU64 = AtomicU64::new(1);
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TAB_NUMBER: AtomicUsize = AtomicUsize::new(1);

/// A tab id no window has used.
pub fn next_tab_id() -> u64 {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

/// The number of the next "Draft N", counted across windows.
pub fn next_tab_number() -> usize {
    NEXT_TAB_NUMBER.fetch_add(1, Ordering::Relaxed)
}

/// Adds the window with the native id `native`: its number, and the inbox its app reads.
pub fn register(native: WindowId) -> (u64, UnboundedReceiver<Message>) {
    let number = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
    let (inbox, receiver) = unbounded_channel();
    WINDOWS.with_borrow_mut(|w| {
        w.push(Window {
            number,
            native,
            label: String::new(),
            inbox,
        })
    });
    (number, receiver)
}

/// Removes a window that is closing.
pub fn unregister(number: u64) {
    WINDOWS.with_borrow_mut(|w| w.retain(|x| x.number != number));
}

/// Sets what the other windows' menus call this window.
pub fn set_label(number: u64, label: String) {
    WINDOWS.with_borrow_mut(|w| {
        if let Some(x) = w.iter_mut().find(|x| x.number == number) {
            x.label = label;
        }
    });
}

/// The windows besides `number`, in the order they opened: number and label.
pub fn others(number: u64) -> Vec<(u64, String)> {
    WINDOWS.with_borrow(|w| {
        w.iter()
            .filter(|x| x.number != number)
            .map(|x| (x.number, x.label.clone()))
            .collect()
    })
}

/// Sends `message` to window `number`; gives it back if that window is gone.
pub fn send(number: u64, message: Message) -> Result<(), Message> {
    let inbox = WINDOWS.with_borrow(|w| {
        w.iter()
            .find(|x| x.number == number)
            .map(|x| x.inbox.clone())
    });
    match inbox {
        Some(inbox) => inbox.send(message).map_err(|e| e.0),
        None => Err(message),
    }
}

/// Sends a message made by `make` to every window but `from`.
pub fn broadcast(from: u64, make: impl Fn() -> Message) {
    let inboxes: Vec<UnboundedSender<Message>> = WINDOWS.with_borrow(|w| {
        w.iter()
            .filter(|x| x.number != from)
            .map(|x| x.inbox.clone())
            .collect()
    });
    for inbox in inboxes {
        let _ = inbox.send(make());
    }
}

/// Passes a close request for the native window `native` to its app. False if no app
/// has that window (then nothing stops it closing).
pub fn route_close(native: WindowId) -> bool {
    let inbox = WINDOWS.with_borrow(|w| {
        w.iter()
            .find(|x| x.native == native)
            .map(|x| x.inbox.clone())
    });
    inbox.is_some_and(|i| i.send(Message::CloseRequested).is_ok())
}

/// Keeps `tab` for a window about to open; its app takes it with [`take_start`].
pub fn stash_start(tab: MovedTab) -> Start {
    let key = NEXT_START.fetch_add(1, Ordering::Relaxed);
    STARTS.with_borrow_mut(|s| s.insert(key, tab));
    Start(key)
}

/// The tab a new window opens with.
pub fn take_start(start: Start) -> Option<MovedTab> {
    STARTS.with_borrow_mut(|s| s.remove(&start.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_ids_and_numbers_are_unique() {
        let ids: Vec<u64> = (0..50).map(|_| next_tab_id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(next_tab_number() < next_tab_number());
    }
}
