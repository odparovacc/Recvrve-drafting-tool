//! Easter eggs: one switch turns them all on or off (View → Easter eggs, saved in
//! `config/settings.json`, on by default).
//!
//! Every egg is wrapped in a check:
//!
//! ```ignore
//! if easter::on() {
//!     // the egg
//! }
//! ```
//!
//! [`on`] works anywhere. Called while a component renders, it also subscribes that component,
//! so turning eggs off re-renders it at once and an egg rendered as
//! `if easter::on() { SomeEgg {} }` is removed, together with its timers and tasks. An egg
//! that runs over time (a loop, a timer) should check [`on`] on every step, so it stops
//! when eggs are turned off midway.
//!
//! Eggs outside Rust check the same switch:
//! - CSS: the app root has the class `eggs-on` while eggs are on, so write
//!   `.eggs-on .something { ... }`.
//! - JavaScript in the webview: `window.__recvrveEggs` is `true` while eggs are on.

use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::dioxus_core::Runtime;
use dioxus::prelude::*;

/// The switch for code that runs outside components.
static ON: AtomicBool = AtomicBool::new(true);

/// Shared through context by the app root (see [`provide`]).
#[derive(Clone, Copy)]
#[allow(dead_code)] // Read by `on`, which no egg uses yet.
struct EasterEggs(Signal<bool>);

/// Whether easter eggs are on.
#[allow(dead_code)] // No eggs yet.
pub fn on() -> bool {
    let in_scope = Runtime::try_current().is_some_and(|rt| rt.try_current_scope_id().is_some());
    if in_scope && let Some(EasterEggs(signal)) = try_consume_context::<EasterEggs>() {
        return signal();
    }
    ON.load(Ordering::Relaxed)
}

/// Called by the app root: makes `signal` the switch for every component, the webview and
/// code outside components.
pub fn provide(signal: Signal<bool>) {
    use_context_provider(|| EasterEggs(signal));
    use_effect(move || {
        let on = signal();
        ON.store(on, Ordering::Relaxed);
        document::eval(&format!("window.__recvrveEggs = {on};"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_the_app_the_switch_is_read_directly() {
        assert!(on(), "on by default");
        ON.store(false, Ordering::Relaxed);
        assert!(!on());
        ON.store(true, Ordering::Relaxed);
        assert!(on());
    }
}
