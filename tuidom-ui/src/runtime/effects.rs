//! `effect` — a side effect that reruns when its reactive dependencies change.

use std::sync::Arc;

use super::{Kind, OBSERVER, SlotId, detach_sources, insert, rt};

/// Register a side effect. It runs once immediately — tracking whatever signals and deriveds
/// it reads — and reruns whenever any of them changes.
///
/// Dependencies are discovered by reading, not declared, and re-collected on every run: an
/// effect that stops reading a signal (because a branch changed) stops depending on it.
///
/// The effect is owned by the current [`Scope`](super::Scope), if one is open, and stops
/// when that scope is disposed.
pub fn effect<F: Fn() + Send + Sync + 'static>(f: F) {
    let run: Arc<dyn Fn() + Send + Sync> = Arc::new(f);
    let id = insert(Kind::Effect { run, dirty: false });
    run_effect(id);
}

/// Run one effect: detach its old dependencies, install it as the observer, and invoke its
/// body with the lock released so the body's reads re-track and its writes can flush.
pub(super) fn run_effect(id: SlotId) {
    let run = {
        let mut w = rt().write();
        let run = {
            let Some(node) = w.nodes.get_mut(&id) else {
                return;
            };
            match &mut node.kind {
                Kind::Effect { run, dirty } => {
                    *dirty = false;
                    run.clone()
                }
                _ => return,
            }
        };
        detach_sources(&mut w, id);
        run
    };

    let prev = OBSERVER.with(|o| o.replace(Some(id)));
    (run)();
    OBSERVER.with(|o| o.set(prev));
}
