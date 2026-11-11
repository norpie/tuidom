//! Mounting a chain, and following it as the route changes.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tuidom::style::Style;
use tuidom_ui::{
    Children, Signal, View, batch, boxed, component, dynamic, effect, fragment, state, untracked,
};

use crate::chain::{Chain, Entry, Kept};

/// Mount the chain `chain` produces, and follow it whenever a signal it read changes.
///
/// `chain` is where the route is matched: `move || match route.get() { .. }`. On a change the new
/// chain is compared with the mounted one position by position, by component and by whether the
/// entry is a layout or the leaf, and by whether it traps focus. Entries before the first
/// difference stay mounted: their props closures run again, untracked, the values are assigned
/// into their props, and their boxes take the new chain's styles, so their nodes, scroll
/// offsets, focus and running transitions are kept and their bodies do not run again.
/// From the first difference on, the old entries are disposed, innermost first, and the new ones
/// mount in the slot of the last kept layout.
pub fn routed<F>(chain: F) -> View
where
    F: Fn() -> Chain + Send + Sync + 'static,
{
    component(move || {
        let router = Arc::new(Mutex::new(Router {
            entries: Arc::from(Vec::new()),
            levels: Vec::new(),
        }));
        let root = state(0u64);
        lock(&router).level(0).slot = Some(root);
        let follow = Arc::clone(&router);
        effect(move || {
            let next: Arc<[Entry]> = chain().entries.into();
            untracked(|| navigate(&follow, next));
        });
        dynamic(move || {
            root.get();
            slot(&router, 0)
        })
    })
}

type Shared = Arc<Mutex<Router>>;

struct Router {
    /// The chain as last navigated to.
    entries: Arc<[Entry]>,
    /// Per chain position, what the mounted chain holds there.
    levels: Vec<Level>,
}

#[derive(Default)]
struct Level {
    /// Bumped to remount this position. Owned by the layout before it, or by the router for the
    /// first position.
    slot: Option<Signal<u64>>,
    /// The mounted entry's props.
    kept: Option<Kept>,
    /// The mounted entry's box style.
    style: Option<Signal<Style>>,
}

impl Router {
    fn level(&mut self, index: usize) -> &mut Level {
        if self.levels.len() <= index {
            self.levels.resize_with(index + 1, Level::default);
        }
        &mut self.levels[index]
    }
}

fn lock(router: &Shared) -> MutexGuard<'_, Router> {
    router.lock().unwrap_or_else(PoisonError::into_inner)
}

fn navigate(router: &Shared, next: Arc<[Entry]>) {
    let (reassign, restyle, remount) = {
        let mut r = lock(router);
        let kept_len = r
            .entries
            .iter()
            .zip(next.iter())
            .take_while(|(old, new)| old.identity == new.identity)
            .count();
        let unchanged = kept_len == r.entries.len() && kept_len == next.len();
        let reassign: Vec<_> = (0..kept_len)
            .filter_map(|i| {
                let kept = r.levels.get(i)?.kept.clone()?;
                Some((Arc::clone(&next[i].reassign), kept))
            })
            .collect();
        let restyle: Vec<_> = (0..kept_len)
            .filter_map(|i| Some((r.levels.get(i)?.style?, next[i].box_style())))
            .collect();
        let remount = if unchanged {
            None
        } else {
            for level in r.levels.iter_mut().skip(kept_len) {
                level.kept = None;
                level.style = None;
            }
            r.levels.get(kept_len).and_then(|level| level.slot)
        };
        r.entries = next;
        (reassign, restyle, remount)
    };
    // Props closures and the writes they cause run with the router unlocked.
    batch(|| {
        for (reassign, kept) in &reassign {
            reassign(kept);
        }
        for (signal, style) in restyle {
            signal.set(style);
        }
        if let Some(slot) = remount
            && slot.try_get().is_some()
        {
            slot.update(|generation| *generation += 1);
        }
    });
}

/// The entry at `index` in its box, with a slot for the next entry when it is a layout.
fn slot(router: &Shared, index: usize) -> View {
    let Some(entry) = lock(router).entries.get(index).cloned() else {
        return fragment(Vec::<View>::new());
    };
    let router = Arc::clone(router);
    // Deferred to mount, so the props closure runs untracked and under the entry's own scope,
    // which then owns the props, the box style and the next position's slot.
    component(move || {
        let style = state(entry.box_style());
        let children = entry.identity.layout.then(|| {
            let generation = state(0u64);
            lock(&router).level(index + 1).slot = Some(generation);
            let router = Arc::clone(&router);
            Children::new(move || {
                generation.get();
                slot(&router, index + 1)
            })
        });
        let (view, kept) = (entry.mount)(children);
        {
            let mut r = lock(&router);
            let level = r.level(index);
            level.kept = Some(kept);
            level.style = Some(style);
        }
        boxed()
            .dyn_style(move || style.get())
            .trap_focus(entry.identity.trap_focus)
            .child(view)
            .into()
    })
}
