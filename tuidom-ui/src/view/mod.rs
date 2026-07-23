//! The view layer: declarative descriptions of a subtree that *mount* onto real engine
//! nodes and bind signals to them.
//!
//! A [`View`] is inert until [`mount`]ed under a parent `NodeId`. Mounting opens an owner
//! [`Scope`], and every node, binding effect, and (later) listener created during the mount
//! is owned by it — so [`Mounted::unmount`] tears the whole subtree down by disposing that
//! one scope. This is the F2→engine bridge: reads inside a binding subscribe through the
//! runtime, and a signal change reruns only that binding's effect.
//!
//! # Static and reactive alike
//!
//! Every bindable property takes a [`Source`], which is either a constant or a closure. A
//! constant is applied once at mount; a closure becomes an [`effect`], so it reapplies
//! whenever a signal it read changes. Each property is its own effect, so a change to one
//! never reapplies another — the granularity the reactive model promises.

use std::sync::{Arc, Mutex, MutexGuard};

use tuidom::event::{KeyEvent, ListenerHandle, MouseEvent};
use tuidom::style::{Length, Position, Style};
use tuidom::{Document, NodeId, Result};

use crate::runtime::{Scope, effect};

/// A deferred listener registration: run at mount against the created node, yielding the
/// handle used to remove it on unmount.
type ListenerReg = Box<dyn FnOnce(&Document, NodeId) -> Result<ListenerHandle>>;

/// A static or reactive source for one bound property.
///
/// `Static` is applied once at mount time. `Reactive` is wrapped in an [`effect`], so it is
/// applied now and again on every change to a signal it reads.
enum Source<T> {
    Static(T),
    Reactive(Box<dyn Fn() -> T + Send + Sync>),
}

/// A declarative description of a subtree to mount under a parent node.
///
/// Construct one with [`text`], [`dyn_text`], [`boxed`], or [`fragment`], and realize it with
/// [`mount`]. A `View` holds no engine state until mounted.
pub struct View {
    kind: ViewKind,
}

/// The shape a `View` takes. A fragment is a flat run of siblings with no wrapper node — the
/// engine never gains a phantom flex level from a `View` boundary.
enum ViewKind {
    // Boxed because an `Element` (style, attrs, children) dwarfs the other variants.
    Element(Box<Element>),
    Text(TextView),
    Fragment(Vec<View>),
    Dynamic(DynamicView),
}

/// A text node description, static or reactive.
struct TextView {
    content: Source<String>,
}

/// A dynamic block: a closure re-run on its own dependencies, whose produced view replaces
/// the block's content in place. Anchored by an out-of-flow marker node, so an empty block
/// costs no cells and adjacent siblings are undisturbed.
struct DynamicView {
    produce: Box<dyn Fn() -> View + Send + Sync>,
}

/// Which engine node an [`Element`] mounts as.
///
/// Both share the same builder — style, attributes, focusability, listeners, and children —
/// and differ only in the node the engine creates for them. An `Input` is an editable leaf,
/// focusable by default; a `Box` is a plain container.
enum ElementKind {
    Box,
    Input,
}

/// An element description: a builder for its style, attributes, focusability, and children.
///
/// Every setter has a `dyn_` counterpart taking a closure, which binds the property reactively
/// instead of to a constant.
pub struct Element {
    kind: ElementKind,
    style: Option<Source<Style>>,
    attrs: Vec<(String, Source<String>)>,
    focusable: Option<Source<bool>>,
    listeners: Vec<ListenerReg>,
    children: Vec<View>,
}

/// A mounted subtree. Dropping the handle leaves the subtree in place; call [`unmount`] to
/// remove it.
///
/// [`unmount`]: Mounted::unmount
pub struct Mounted {
    scope: Scope,
    roots: Vec<NodeId>,
}

impl Mounted {
    /// The top-level nodes this mount created directly under the parent, in order.
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Remove the subtree and free everything it owns: its nodes, every binding effect, and
    /// (through the owner scope) any child scopes a dynamic block opened.
    pub fn unmount(self) {
        self.scope.dispose();
    }
}

/// A static text node.
pub fn text(content: impl Into<String>) -> View {
    View {
        kind: ViewKind::Text(TextView {
            content: Source::Static(content.into()),
        }),
    }
}

/// A text node whose content tracks the signals its closure reads.
pub fn dyn_text<F>(f: F) -> View
where
    F: Fn() -> String + Send + Sync + 'static,
{
    View {
        kind: ViewKind::Text(TextView {
            content: Source::Reactive(Box::new(f)),
        }),
    }
}

/// A run of sibling views mounted under the same parent, with no wrapper node.
pub fn fragment(views: impl IntoIterator<Item = View>) -> View {
    View {
        kind: ViewKind::Fragment(views.into_iter().collect()),
    }
}

/// A dynamic block whose content is recomputed whenever a signal its closure reads changes.
///
/// The closure produces a fresh [`View`] each run; the old content is unmounted and the new
/// mounted in its place. Only the signals read *while producing* the view are dependencies —
/// a change confined to the produced content's own bindings reruns those bindings, not this
/// block.
pub fn dynamic<F>(f: F) -> View
where
    F: Fn() -> View + Send + Sync + 'static,
{
    View {
        kind: ViewKind::Dynamic(DynamicView {
            produce: Box::new(f),
        }),
    }
}

/// Deferred child content a container mounts into itself.
///
/// Unlike a constructed [`View`] handed over eagerly, a `Children` thunk is *not* invoked
/// when the parent is built — the framework calls it at mount time, and again when a signal
/// it reads changes. So a container's own body is built once while its children swap beneath
/// it, and the container node keeps its identity (and thus its scroll offset, focus, and
/// running transitions) across the swap. This is the shape F5 components will receive
/// children in.
pub struct Children(Arc<dyn Fn() -> View + Send + Sync>);

impl Children {
    /// Wrap a thunk producing child content. The thunk is stored, not called.
    pub fn new<F>(thunk: F) -> Self
    where
        F: Fn() -> View + Send + Sync + 'static,
    {
        Children(Arc::new(thunk))
    }
}

impl From<Children> for View {
    fn from(children: Children) -> View {
        let thunk = children.0;
        dynamic(move || thunk())
    }
}

/// Begin building a box element.
pub fn boxed() -> Element {
    element(ElementKind::Box)
}

/// Begin building an editable input element. The engine node is focusable by default and
/// starts empty; a template sets its value and observes edits through attributes and events
/// in later F4 steps.
pub fn input() -> Element {
    element(ElementKind::Input)
}

/// The shared empty builder both element constructors start from.
fn element(kind: ElementKind) -> Element {
    Element {
        kind,
        style: None,
        attrs: Vec::new(),
        focusable: None,
        listeners: Vec::new(),
        children: Vec::new(),
    }
}

impl Element {
    /// Set a constant style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(Source::Static(style));
        self
    }

    /// Bind the style reactively. The whole `Style` is reapplied on change, so engine
    /// transitions animate the diff rather than snapping.
    pub fn dyn_style<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Style + Send + Sync + 'static,
    {
        self.style = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set a constant attribute.
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((key.into(), Source::Static(value.into())));
        self
    }

    /// Bind an attribute's value reactively.
    pub fn dyn_attr<F>(mut self, key: impl Into<String>, f: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.attrs.push((key.into(), Source::Reactive(Box::new(f))));
        self
    }

    /// Set whether the element is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = Some(Source::Static(focusable));
        self
    }

    /// Bind focusability reactively.
    pub fn dyn_focusable<F>(mut self, f: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.focusable = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Register a click listener, removed automatically on unmount.
    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut MouseEvent) + Send + Sync + 'static,
    {
        self.listeners
            .push(Box::new(move |doc, node| doc.on_click(node, handler)));
        self
    }

    /// Register a key-press listener, removed automatically on unmount.
    pub fn on_key_press<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut KeyEvent) + Send + Sync + 'static,
    {
        self.listeners
            .push(Box::new(move |doc, node| doc.on_key_press(node, handler)));
        self
    }

    /// Append a child view.
    pub fn child(mut self, child: impl Into<View>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Append several child views.
    pub fn children(mut self, children: impl IntoIterator<Item = View>) -> Self {
        self.children.extend(children);
        self
    }
}

impl From<Element> for View {
    fn from(element: Element) -> View {
        View {
            kind: ViewKind::Element(Box::new(element)),
        }
    }
}

/// A `String` becomes a static text node — so a plain string expression drops into a view
/// hole or a child slot without spelling out [`text`]. Reactive text still goes through
/// [`dyn_text`].
impl From<String> for View {
    fn from(content: String) -> View {
        text(content)
    }
}

/// A string slice becomes a static text node. See [`From<String>`](#impl-From<String>-for-View).
impl From<&str> for View {
    fn from(content: &str) -> View {
        text(content)
    }
}

/// Mount `view` under `parent` in `doc`, returning a handle that owns the whole subtree.
///
/// A partial mount that errors partway is fully rolled back before the error returns.
pub fn mount(doc: &Document, parent: NodeId, view: impl Into<View>) -> Result<Mounted> {
    mount_at(doc, parent, None, view.into())
}

/// Mount a view under its own owner scope, placing its top-level nodes before `before` (a
/// dynamic block's marker) or appended. A partial mount that errors is rolled back.
fn mount_at(doc: &Document, parent: NodeId, before: Option<NodeId>, view: View) -> Result<Mounted> {
    let scope = Scope::new();
    match scope.run(|| mount_view(doc, parent, before, view, &scope)) {
        Ok(roots) => Ok(Mounted { scope, roots }),
        Err(err) => {
            scope.dispose();
            Err(err)
        }
    }
}

/// Recursively mount a view, returning the top-level nodes it created directly under
/// `parent`. `before` places those nodes ahead of a sibling (a dynamic block's marker);
/// `None` appends. Runs with `scope` entered, so binding effects attach to it.
fn mount_view(
    doc: &Document,
    parent: NodeId,
    before: Option<NodeId>,
    view: View,
    scope: &Scope,
) -> Result<Vec<NodeId>> {
    match view.kind {
        ViewKind::Fragment(children) => {
            let mut ids = Vec::new();
            for child in children {
                ids.extend(mount_view(doc, parent, before, child, scope)?);
            }
            Ok(ids)
        }
        ViewKind::Text(view) => {
            let node = doc.create_text("")?;
            place(doc, parent, node, before)?;
            register_remove(doc, parent, node, scope);
            let d = doc.clone();
            bind(view.content, move |content| {
                if let Err(err) = d.set_text_content(node, content) {
                    tracing::warn!(?node, "text binding failed: {err}");
                }
            });
            Ok(vec![node])
        }
        ViewKind::Element(element) => {
            let element = *element;
            let node = match element.kind {
                ElementKind::Box => doc.create_box()?,
                ElementKind::Input => doc.create_input("")?,
            };
            place(doc, parent, node, before)?;
            register_remove(doc, parent, node, scope);

            if let Some(style) = element.style {
                let d = doc.clone();
                bind(style, move |style| {
                    if let Err(err) = d.set_style(node, &style) {
                        tracing::warn!(?node, "style binding failed: {err}");
                    }
                });
            }
            for (key, value) in element.attrs {
                let d = doc.clone();
                bind(value, move |value| {
                    if let Err(err) = d.set_attr(node, key.clone(), value) {
                        tracing::warn!(?node, "attribute binding failed: {err}");
                    }
                });
            }
            if let Some(focusable) = element.focusable {
                let d = doc.clone();
                bind(focusable, move |focusable| {
                    if let Err(err) = d.set_focusable(node, focusable) {
                        tracing::warn!(?node, "focusable binding failed: {err}");
                    }
                });
            }
            for register in element.listeners {
                let handle = register(doc, node)?;
                let d = doc.clone();
                scope.on_cleanup(move || {
                    d.remove_listener(handle);
                });
            }
            for child in element.children {
                mount_view(doc, node, None, child, scope)?;
            }
            Ok(vec![node])
        }
        ViewKind::Dynamic(dynamic) => {
            // The marker anchors the block: out of flow (no cells, no gap), it holds a stable
            // position that survives the content emptying to nothing.
            let marker = doc.create_box()?;
            doc.set_style(marker, &marker_style())?;
            place(doc, parent, marker, before)?;
            register_remove(doc, parent, marker, scope);

            // The content mounted before the marker right now, swapped whole on every rerun.
            let current: Arc<Mutex<Option<Mounted>>> = Arc::new(Mutex::new(None));

            // Disposing the owner scope tears down whatever content is live — the content's
            // own scope is a root, so it is not freed structurally and needs this hook.
            let held = current.clone();
            scope.on_cleanup(move || {
                if let Some(mounted) = lock(&held).take() {
                    mounted.unmount();
                }
            });

            let produce = dynamic.produce;
            let doc = doc.clone();
            effect(move || {
                // Reading `produce()` subscribes this effect to exactly the signals the
                // author read while choosing the content — not the content's own bindings.
                let view = produce();
                let mut slot = lock(&current);
                if let Some(old) = slot.take() {
                    old.unmount();
                }
                match mount_at(&doc, parent, Some(marker), view) {
                    Ok(mounted) => *slot = Some(mounted),
                    Err(err) => tracing::warn!(?marker, "dynamic block failed to mount: {err}"),
                }
            });

            Ok(vec![marker])
        }
    }
}

/// The style of a dynamic block's anchor marker: absolute and zero-sized, so it participates
/// in neither flow nor gap and paints nothing.
fn marker_style() -> Style {
    let mut style = Style::new();
    style.position(Position::Absolute { x: 0, y: 0 });
    style.width(Length::Cells(0));
    style.height(Length::Cells(0));
    style
}

/// Lock a mutex, tolerating poison — a panicked holder leaves the guarded content usable
/// rather than propagating the panic, matching the engine's lock discipline.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Apply a source: a constant now, or an effect that reapplies on change. Called with the
/// owner scope entered, so a reactive source's effect is owned by the mount.
fn bind<T, F>(source: Source<T>, apply: F)
where
    T: Send + Sync + 'static,
    F: Fn(T) + Send + Sync + 'static,
{
    match source {
        Source::Static(value) => apply(value),
        Source::Reactive(compute) => {
            effect(move || apply(compute()));
        }
    }
}

/// Place `node` under `parent`, before a sibling when one is given, else appended.
fn place(doc: &Document, parent: NodeId, node: NodeId, before: Option<NodeId>) -> Result<()> {
    match before {
        Some(sibling) => doc.insert_before(parent, node, sibling),
        None => doc.append_child(parent, node),
    }
}

/// Register the node's removal as a scope cleanup, so disposing the owner scope also tears
/// the DOM node out. Redundant removals (a parent's subtree removal beat us to it) are no-ops.
fn register_remove(doc: &Document, parent: NodeId, node: NodeId, scope: &Scope) {
    let doc = doc.clone();
    scope.on_cleanup(move || {
        let _ = doc.remove_child(parent, node);
    });
}

#[cfg(test)]
mod tests;
