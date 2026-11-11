//! Chains, and the view that mounts them.

use std::sync::Arc;

use tuidom::style::{FlexDirection, Style};
use tuidom_ui::{
    Children, ComponentProps, View, WithChildren, boxed, component, dynamic, fragment,
};

type Build = Arc<dyn Fn(Option<Children>) -> View + Send + Sync>;

/// One component in a chain, with the box it mounts in.
struct Entry {
    build: Build,
    style: Option<Style>,
    trap_focus: bool,
}

impl Entry {
    fn layout<P, F>(props: F) -> Entry
    where
        P: WithChildren,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Entry::new(Arc::new(move |children| {
            let mut props = props();
            if let Some(children) = children {
                props.set_children(children);
            }
            props.into_view()
        }))
    }

    fn leaf<P, F>(props: F) -> Entry
    where
        P: ComponentProps,
        F: Fn() -> P + Send + Sync + 'static,
    {
        Entry::new(Arc::new(move |_| props().into_view()))
    }

    fn new(build: Build) -> Entry {
        Entry {
            build,
            style: None,
            trap_focus: false,
        }
    }

    fn box_style(&self) -> Style {
        let mut style = self.style.clone().unwrap_or_else(|| {
            let mut fill = Style::new();
            fill.flex_grow(1.0);
            fill.flex_direction(FlexDirection::Column);
            fill
        });
        if self.trap_focus {
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
/// [`style`](Chain::style) replaces that box's style.
///
/// ```ignore
/// let chain = Chain::layout(|| ShellProps::builder().build())
///     .layout(|| SettingsProps::builder().build())
///     .leaf(|| ProfileProps::builder().user(state(id)).build());
/// ```
pub struct Chain {
    entries: Vec<Entry>,
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
        entry.trap_focus = true;
    }
}

/// Mount the chain `chain` produces, and mount it again whenever a signal it read changes.
///
/// `chain` is where the route is matched: `move || match route.get() { .. }`.
pub fn routed<F>(chain: F) -> View
where
    F: Fn() -> Chain + Send + Sync + 'static,
{
    dynamic(move || {
        let entries: Arc<[Entry]> = chain().entries.into();
        level(entries, 0)
    })
}

/// The entry at `index` in its box, with the entries after it as its children.
fn level(entries: Arc<[Entry]>, index: usize) -> View {
    let Some(entry) = entries.get(index) else {
        return fragment(Vec::<View>::new());
    };
    let build = Arc::clone(&entry.build);
    let style = entry.box_style();
    let trap_focus = entry.trap_focus;
    let has_next = index + 1 < entries.len();
    // Deferred to mount, so the props closure runs untracked and under the entry's own scope,
    // which then owns the props and the children slot.
    let content = component(move || {
        let children = has_next.then(|| {
            let entries = Arc::clone(&entries);
            Children::new(move || level(Arc::clone(&entries), index + 1))
        });
        build(children)
    });
    boxed()
        .style(style)
        .trap_focus(trap_focus)
        .child(content)
        .into()
}
