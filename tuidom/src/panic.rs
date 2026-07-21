use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, set_hook, take_hook};
use std::sync::Once;

use crate::render::restore_for_panic;

thread_local! {
    /// How many downstream callbacks this thread is currently inside.
    static CATCHING: Cell<usize> = const { Cell::new(0) };
}

static HOOK: Once = Once::new();

/// Whether this thread is running a downstream callback whose panic will be caught.
///
/// The panic hook consults this. A handler panic is reported and survived, so
/// restoring the terminal for one would tear down an application that is still
/// running perfectly well — the hook runs *before* unwinding, so it cannot tell
/// a fatal panic from a caught one on its own.
pub(crate) fn catching_panic() -> bool {
    CATCHING.with(|depth| depth.get() > 0)
}

/// Run a downstream callback, reporting whether it panicked instead of unwinding.
///
/// The catch and the hook guard live together here so they cannot drift apart:
/// every site that swallows a downstream panic gets both, and a new one cannot
/// be written that forgets the guard.
pub(crate) fn catch_handler_panic(f: impl FnOnce()) -> bool {
    let _guard = CatchGuard::enter();
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

/// Install the process-wide panic hook that restores the terminal on the way out.
///
/// Called only when a real terminal is set up: the headless runtime owns no
/// terminal state, and a library has no business touching a process-global hook
/// on behalf of an application that never asked for a terminal.
///
/// The hook chains to whatever was installed before it, and is installed once and
/// never removed — a hook can only be replaced, never popped, so removing ours
/// later would clobber whatever the application installed after it. With no
/// terminal active the restore writes nothing, so leaving it in place costs
/// nothing either.
pub(crate) fn install_hook() {
    HOOK.call_once(|| {
        let previous = take_hook();
        set_hook(Box::new(move |info| {
            if !catching_panic() {
                restore_for_panic();
            }
            previous(info);
        }));
    });
}

/// Marks its thread as inside a downstream callback for as long as it lives.
struct CatchGuard;

impl CatchGuard {
    fn enter() -> Self {
        CATCHING.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}

impl Drop for CatchGuard {
    fn drop(&mut self) {
        CATCHING.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_nests_without_clearing_early() {
        assert!(!catching_panic());
        {
            let _outer = CatchGuard::enter();
            assert!(catching_panic());
            {
                let _inner = CatchGuard::enter();
                assert!(catching_panic());
            }
            assert!(
                catching_panic(),
                "an inner guard ending must not unguard the outer callback"
            );
        }
        assert!(!catching_panic());
    }

    #[test]
    fn guard_clears_after_a_caught_panic() {
        assert!(catch_handler_panic(|| panic!("handler bug")));
        assert!(
            !catching_panic(),
            "a panicking callback must still unguard the thread"
        );
    }

    #[test]
    fn a_callback_that_returns_normally_reports_no_panic() {
        let mut ran = false;
        assert!(!catch_handler_panic(|| ran = true));
        assert!(ran);
    }
}
