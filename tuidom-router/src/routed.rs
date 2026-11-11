//! Mounting a chain, and following it as the route changes.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tuidom::style::{Display, Style};
use tuidom_ui::{
    Children, Signal, View, batch, boxed, component, effect, fragment, keyed, state, untracked,
};

use crate::chain::{Chain, Entry, Identity, Kept};

/// Mount the chain `chain` produces, and follow it whenever a signal it read changes.
///
/// `chain` is where the route is matched: `move || match route.get() { .. }`. On a change the new
/// chain is compared with the mounted one position by position, by component and by whether the
/// entry is a layout or the leaf, and by whether it traps focus. Entries before the first
/// difference stay mounted: their props closures run again, untracked, the values are assigned
/// into their props, and their boxes take the new chain's styles, so their nodes, scroll
/// offsets, focus and running transitions are kept and their bodies do not run again.
/// From the first difference on, the old entries are disposed, innermost first, and the new ones
/// mount in the slot of the last kept layout. A [retained](Chain::retain) leaf is hidden instead,
/// and shown again when the chain returns to it in the same slot.
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
        slot(router, 0, root)
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
    /// Bumped when the entry at this position changes. Owned by the layout before it, or by the
    /// router for the first position.
    slot: Option<Signal<u64>>,
    /// The entries mounted at this position: the current one, and retained leaves hidden here.
    instances: Vec<Instance>,
}

struct Instance {
    identity: Identity,
    /// The props it mounted with, which newer props are assigned into.
    kept: Kept,
    /// Its box style, `display: none` while it is a hidden retained leaf.
    style: Signal<Style>,
    retain: bool,
}

impl Router {
    fn level(&mut self, index: usize) -> &mut Level {
        if self.levels.len() <= index {
            self.levels.resize_with(index + 1, Level::default);
        }
        &mut self.levels[index]
    }

    fn instance(&mut self, index: usize, identity: Identity) -> Option<&mut Instance> {
        self.levels
            .get_mut(index)?
            .instances
            .iter_mut()
            .find(|instance| instance.identity == identity)
    }
}

fn lock(router: &Shared) -> MutexGuard<'_, Router> {
    router.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a navigation writes once the router is unlocked.
#[derive(Default)]
struct Writes {
    reassign: Vec<(Entry, Kept)>,
    styles: Vec<(Signal<Style>, Style)>,
    remount: Option<Signal<u64>>,
}

fn navigate(router: &Shared, next: Arc<[Entry]>) {
    let writes = {
        let mut r = lock(router);
        let previous = Arc::clone(&r.entries);
        let kept_len = previous
            .iter()
            .zip(next.iter())
            .take_while(|(old, new)| old.identity == new.identity)
            .count();
        let mut writes = Writes::default();
        for (index, entry) in next[..kept_len].iter().enumerate() {
            refresh(&mut r, index, entry, &mut writes);
        }
        if kept_len < previous.len().max(next.len()) {
            if let Some(old) = previous.get(kept_len) {
                leave(&mut r, kept_len, old.identity, &mut writes);
            }
            if let Some(entry) = next.get(kept_len) {
                refresh(&mut r, kept_len, entry, &mut writes);
            }
            for level in r.levels.iter_mut().skip(kept_len + 1) {
                level.instances.clear();
            }
            writes.remount = r.levels.get(kept_len).and_then(|level| level.slot);
        }
        r.entries = next;
        writes
    };
    // Props closures and the writes they cause run with the router unlocked.
    batch(|| {
        for (entry, kept) in &writes.reassign {
            (entry.reassign)(kept);
        }
        for (signal, style) in writes.styles {
            signal.set(style);
        }
        if let Some(slot) = writes.remount
            && slot.try_get().is_some()
        {
            slot.update(|generation| *generation += 1);
        }
    });
}

/// Queue new props and style for the instance of `entry` mounted at `index`, if there is one.
fn refresh(r: &mut Router, index: usize, entry: &Entry, writes: &mut Writes) {
    if let Some(instance) = r.instance(index, entry.identity) {
        instance.retain = entry.retain;
        writes
            .reassign
            .push((entry.clone(), Arc::clone(&instance.kept)));
        writes.styles.push((instance.style, entry.box_style()));
    }
}

/// Hide the instance navigation is leaving at `index` if it is retained, or forget it so the
/// slot unmounts it.
fn leave(r: &mut Router, index: usize, identity: Identity, writes: &mut Writes) {
    let Some(level) = r.levels.get_mut(index) else {
        return;
    };
    let Some(position) = level
        .instances
        .iter()
        .position(|instance| instance.identity == identity)
    else {
        return;
    };
    let instance = &level.instances[position];
    if instance.retain {
        let mut hidden = instance.style.try_get().unwrap_or_default();
        hidden.display(Display::None);
        writes.styles.push((instance.style, hidden));
    } else {
        level.instances.remove(position);
    }
}

/// The instances mounted at `index`, keyed by identity: retained leaves kept hidden, and the
/// current entry, mounted when it is new.
fn slot(router: Shared, index: usize, generation: Signal<u64>) -> View {
    keyed(move || {
        generation.get();
        let r = lock(&router);
        let mut identities: Vec<Identity> = r
            .levels
            .get(index)
            .map(|level| level.instances.iter().map(|i| i.identity).collect())
            .unwrap_or_default();
        if let Some(current) = r.entries.get(index).map(|entry| entry.identity)
            && !identities.contains(&current)
        {
            identities.push(current);
        }
        drop(r);
        identities
            .into_iter()
            .map(|identity| {
                let router = Arc::clone(&router);
                (identity, move || instance(router, index, identity))
            })
            .collect::<Vec<_>>()
    })
}

/// The current entry at `index` in its box, with a slot for the next entry when it is a layout.
fn instance(router: Shared, index: usize, identity: Identity) -> View {
    let entry = lock(&router)
        .entries
        .get(index)
        .filter(|entry| entry.identity == identity)
        .cloned();
    let Some(entry) = entry else {
        return fragment(Vec::<View>::new());
    };
    // Deferred to mount, so the props closure runs untracked and under the entry's own scope,
    // which then owns the props, the box style and the next position's slot.
    component(move || {
        let style = state(entry.box_style());
        let children = entry.identity.layout.then(|| {
            let generation = state(0u64);
            lock(&router).level(index + 1).slot = Some(generation);
            let router = Arc::clone(&router);
            Children::new(move || slot(Arc::clone(&router), index + 1, generation))
        });
        let (view, kept) = (entry.mount)(children);
        lock(&router).level(index).instances.push(Instance {
            identity,
            kept,
            style,
            retain: entry.retain,
        });
        boxed()
            .dyn_style(move || style.get())
            .trap_focus(entry.identity.trap_focus)
            .child(view)
            .into()
    })
}
