//! Chains: the entries a route shows, each a closure building a component's props.

use std::any::{Any, TypeId};
use std::sync::Arc;

use tuidom::style::{FlexDirection, Style};
use tuidom_ui::{Children, ComponentProps, Scope, View, WithChildren, untracked};

/// A mounted entry's props, kept so newer props can be assigned into them.
pub(crate) type Kept = Arc<dyn Any + Send + Sync>;
type Mount = Arc<dyn Fn(Option<Children>) -> (View, Kept) + Send + Sync>;
type Reassign = Arc<dyn Fn(&Kept) + Send + Sync>;

/// What the chain diff compares at each position. A leaf mounts with no slot to fill, so the
/// same component as a layout and as a leaf are different entries. A focus context opens when
/// its box mounts, so trapping and not trapping are different entries too.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Identity {
    props: TypeId,
    pub(crate) layout: bool,
    pub(crate) trap_focus: bool,
}

/// One component in a chain, with the box it mounts in.
#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) identity: Identity,
    /// Build the props, fill the slot when there is one, and return the view with a clone of
    /// the props.
    pub(crate) mount: Mount,
    /// Build the props again and assign them into a mounted entry's kept props.
    pub(crate) reassign: Reassign,
    style: Option<Style>,
    /// Hide the leaf instead of disposing it when navigation leaves it.
    pub(crate) retain: bool,
}

impl Entry {
    fn layout<P, F>(props: F) -> Entry
    where
        P: WithChildren,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Entry::new(props, true, |props: &mut P, children| {
            props.set_children(children)
        })
    }

    fn leaf<P, F>(props: F) -> Entry
    where
        P: ComponentProps,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Entry::new(props, false, |_: &mut P, _| {})
    }

    fn new<P, F>(props: F, layout: bool, fill: fn(&mut P, Children)) -> Entry
    where
        P: ComponentProps,
        F: Fn() -> P + Send + Sync + 'static,
    {
        let props = Arc::new(props);
        let build = Arc::clone(&props);
        let mount: Mount = Arc::new(move |children| {
            let mut props = build();
            if let Some(children) = children {
                fill(&mut props, children);
            }
            let kept: Kept = Arc::new(props.clone());
            (props.into_view(), kept)
        });
        let reassign: Reassign = Arc::new(move |kept| {
            let Some(current) = kept.downcast_ref::<P>() else {
                return;
            };
            let scope = Scope::new();
            let next = untracked(|| scope.run(|| props()));
            current.assign(next);
            scope.dispose();
        });
        Entry {
            identity: Identity {
                props: TypeId::of::<P>(),
                layout,
                trap_focus: false,
            },
            mount,
            reassign,
            style: None,
            retain: false,
        }
    }

    pub(crate) fn box_style(&self) -> Style {
        let mut style = self.style.clone().unwrap_or_else(|| {
            let mut fill = Style::new();
            fill.flex_grow(1.0);
            fill.flex_direction(FlexDirection::Column);
            fill
        });
        if self.identity.trap_focus {
            style.stacking_context(true);
        }
        style
    }
}

/// The layouts a route shows, outermost first, and the leaf inside the innermost one.
///
/// Each entry is a closure building a component's props, called when the entry mounts. A layout
/// is a component taking `children`, which the chain fills with the next entry. Each entry
/// mounts inside a box of its own that grows to fill the slot it is placed in, as a column;
/// [`style`](Chain::style) replaces that box's style. A [retained](Chain::retain) leaf is hidden
/// rather than disposed when navigation leaves it.
///
/// ```ignore
/// let chain = Chain::layout(|| ShellProps::builder().build())
///     .layout(|| SettingsProps::builder().build())
///     .leaf(|| ProfileProps::builder().user(state(id)).build());
/// ```
pub struct Chain {
    pub(crate) entries: Vec<Entry>,
}

/// A chain's layouts, before its leaf is given.
pub struct Layouts {
    entries: Vec<Entry>,
}

impl Chain {
    /// Start a chain with its outermost layout.
    pub fn layout<P, F>(props: F) -> Layouts
    where
        P: WithChildren,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Layouts {
            entries: vec![Entry::layout(props)],
        }
    }

    /// A chain of one leaf and no layouts.
    pub fn leaf<P, F>(props: F) -> Chain
    where
        P: ComponentProps,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Chain {
            entries: vec![Entry::leaf(props)],
        }
    }

    /// Replace the style of the leaf's box.
    pub fn style(mut self, style: Style) -> Self {
        set_style(&mut self.entries, style);
        self
    }

    /// Trap focus inside the leaf's box while it is mounted. The box is made a stacking context.
    pub fn trap_focus(mut self) -> Self {
        set_trap_focus(&mut self.entries);
        self
    }

    /// Keep the leaf mounted but hidden when navigation leaves it, and show it again, with its
    /// nodes and their scroll offsets and focus memory as they were, when navigation comes back
    /// to the same component in the same slot. Its props are assigned from the returning
    /// chain.
    ///
    /// A retained leaf lives in the slot of the layout that held it and is disposed with that
    /// layout. While hidden, any focus context in it traps nothing.
    pub fn retain(mut self) -> Self {
        if let Some(entry) = self.entries.last_mut() {
            entry.retain = true;
        }
        self
    }
}

impl Layouts {
    /// Add a layout inside the previous one.
    pub fn layout<P, F>(mut self, props: F) -> Self
    where
        P: WithChildren,
        F: Fn() -> P + Send + Sync + 'static,
    {
        self.entries.push(Entry::layout(props));
        self
    }

    /// Finish the chain with the leaf inside the innermost layout.
    pub fn leaf<P, F>(mut self, props: F) -> Chain
    where
        P: ComponentProps,
        F: Fn() -> P + Send + Sync + 'static,
    {
        self.entries.push(Entry::leaf(props));
        Chain {
            entries: self.entries,
        }
    }

    /// Replace the style of the last layout's box.
    pub fn style(mut self, style: Style) -> Self {
        set_style(&mut self.entries, style);
        self
    }

    /// Trap focus inside the last layout's box while it is mounted. The box is made a stacking
    /// context.
    pub fn trap_focus(mut self) -> Self {
        set_trap_focus(&mut self.entries);
        self
    }
}

fn set_style(entries: &mut [Entry], style: Style) {
    if let Some(entry) = entries.last_mut() {
        entry.style = Some(style);
    }
}

fn set_trap_focus(entries: &mut [Entry]) {
    if let Some(entry) = entries.last_mut() {
        entry.identity.trap_focus = true;
    }
}
