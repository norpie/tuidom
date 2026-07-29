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

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};

use tuidom::event::{KeyEvent, ListenerHandle, MouseEvent};
use tuidom::style::{FlexDirection, Length, Overflow, Position, Style};
use tuidom::virtualize::Virtualizer;
use tuidom::{Document, NodeId, Result};

use crate::runtime::{Scope, effect, state};

thread_local! {
    /// Callbacks registered by [`on_mount`] during a subtree's mount, awaiting the moment that
    /// subtree is in place. A `Vec` used as a stack of pending callbacks: each scope-establishing
    /// mount boundary ([`mount_at`] and a component instantiation) records the queue length on
    /// entry and, on success, fires exactly the tail it added — so an inner component's callbacks
    /// run before the outer component's, and a failed mount discards its own rather than leaking
    /// them into the next mount on this thread.
    static ON_MOUNT: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Register `f` to run once, right after the enclosing component's subtree has finished
/// mounting — the deferred counterpart to [`on_cleanup`](crate::on_cleanup).
///
/// A component body builds a [`View`] before any of it is in the DOM, so a side effect that
/// should happen "once the component is live" — focusing through a signal, starting a task,
/// kicking off a fetch — cannot run inline. `on_mount` queues `f`; the mount machinery drains
/// it after the subtree is placed. It runs **once**, never again on a prop change or rerender,
/// and with the component's scope entered, so anything reactive it creates is owned by the
/// component and torn down on unmount.
///
/// Called outside any mount in progress, `f` is queued and fires at the end of the next
/// top-level [`mount`].
pub fn on_mount(f: impl FnOnce() + 'static) {
    ON_MOUNT.with(|q| q.borrow_mut().push(Box::new(f)));
}

/// Run a subtree mount, then fire the [`on_mount`] callbacks it queued — in registration order,
/// once the subtree is placed — or discard them if it failed to mount. Wraps every
/// scope-establishing mount boundary so callbacks resolve to the nearest enclosing subtree.
fn with_mount_callbacks<R>(mount: impl FnOnce() -> Result<R>) -> Result<R> {
    let base = ON_MOUNT.with(|q| q.borrow().len());
    match mount() {
        Ok(value) => {
            // `split_off` leaves everything below `base` queued for an enclosing boundary to
            // drain; we own only the tail this subtree added.
            let pending = ON_MOUNT.with(|q| q.borrow_mut().split_off(base));
            for cb in pending {
                cb();
            }
            Ok(value)
        }
        Err(err) => {
            ON_MOUNT.with(|q| q.borrow_mut().truncate(base));
            Err(err)
        }
    }
}

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
    Keyed(KeyedView),
    // Boxed because a `VirtualizedView` (a style, two boxed closures) dwarfs the other variants.
    Virtualized(Box<VirtualizedView>),
    Component(ComponentBuild),
}

/// A component's deferred body. Unlike a [`DynamicView`]'s closure it runs exactly **once**,
/// at mount, under the component's own child scope — so props flowing in as signals update the
/// body's bindings without ever re-running it, and unmounting the component tears its signals,
/// effects, and nodes down together. `FnOnce`, since the body is built a single time.
type ComponentBuild = Box<dyn FnOnce() -> View + Send + Sync>;

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

/// A keyed list: a closure re-run on its own dependencies, producing an ordered sequence of
/// `(key, item)` pairs. Unlike a [`DynamicView`], which discards and rebuilds its whole content,
/// this diffs the new key sequence against the mounted one — reusing the nodes a surviving key
/// already owns, mounting only genuinely new keys, and disposing only vanished ones. Anchored by
/// the same out-of-flow marker, so an empty list costs no cells.
struct KeyedView {
    produce: Box<dyn Fn() -> Vec<(ErasedKey, ItemThunk)> + Send + Sync>,
}

/// One keyed item's deferred body, built the first time its key appears and never re-run while
/// the key survives — content updates flow through the signals the item reads, not through the
/// differ (a key promises "same item", and re-running would throw away the very node identity,
/// and thus the scroll/focus/transition state, that keys exist to preserve).
type ItemThunk = Box<dyn FnOnce() -> View>;

/// A type-erased list key. The author's `key = …` may be any `Eq + Hash + Clone` type; boxing it
/// behind [`DynKey`] lets the differ diff a key sequence without threading that type through the
/// non-generic [`View`]. Equality is same-type-and-equal: two keys of different concrete types are
/// never equal, so distinct key types simply never collide.
struct ErasedKey(Box<dyn DynKey>);

impl ErasedKey {
    fn new<K: DynKey + 'static>(key: K) -> Self {
        ErasedKey(Box::new(key))
    }
}

impl Clone for ErasedKey {
    fn clone(&self) -> Self {
        ErasedKey(self.0.dyn_clone())
    }
}

impl PartialEq for ErasedKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.dyn_eq(other.0.as_any())
    }
}

impl Eq for ErasedKey {}

impl Hash for ErasedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.dyn_hash(state);
    }
}

/// The object-safe view of a key that [`ErasedKey`] holds — equality, hashing, and cloning
/// forwarded to the concrete key type behind `dyn`.
trait DynKey: Send + Sync {
    fn dyn_eq(&self, other: &dyn Any) -> bool;
    fn dyn_hash(&self, state: &mut dyn Hasher);
    fn dyn_clone(&self) -> Box<dyn DynKey>;
    fn as_any(&self) -> &dyn Any;
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static> DynKey for K {
    fn dyn_eq(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<K>().is_some_and(|o| self == o)
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        self.hash(&mut state);
    }

    fn dyn_clone(&self) -> Box<dyn DynKey> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
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

/// A keyed list whose items keep their node identity — and thus their scroll offset, focus, and
/// running transitions — across reorders.
///
/// `produce` yields an ordered sequence of `(key, item)` pairs and is re-run whenever a signal it
/// read changes, exactly like [`dynamic`]. But instead of rebuilding, the new key sequence is
/// diffed against the mounted one: a key present in both **reuses** the nodes it already owns
/// (moving them into place with the engine's `move_child`), a new key **mounts** its item, and a
/// vanished key **unmounts** its subtree. The item body is built once, the first time its key
/// appears, and is never re-run while the key survives — so an item's displayed data must update
/// through the signals it reads, not through this closure re-running (the [`For`-versus-`Index`]
/// distinction: this is keyed identity, not positional content).
///
/// Keys must be unique within one pass; a duplicate is dropped with a warning. The `#[component]`
/// macro lowers `each … key = …` to this, using the enumerate index as the key when no `key` is
/// given.
///
/// [`For`-versus-`Index`]: https://www.solidjs.com/tutorial/flow_index
pub fn keyed<K, F, I, T>(produce: F) -> View
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    F: Fn() -> I + Send + Sync + 'static,
    I: IntoIterator<Item = (K, T)>,
    T: FnOnce() -> View + 'static,
{
    // Erase each key and box each item body, so the differ works over one non-generic sequence
    // type behind the `View`. The wrapping closure inherits `Send + Sync` from the captured
    // `produce`; the boxed item bodies it returns need neither (they are consumed in the same
    // effect run that produces them).
    let produce = move || {
        produce()
            .into_iter()
            .map(|(key, item)| {
                let item: ItemThunk = Box::new(item);
                (ErasedKey::new(key), item)
            })
            .collect::<Vec<_>>()
    };
    View {
        kind: ViewKind::Keyed(KeyedView {
            produce: Box::new(produce),
        }),
    }
}

/// A virtualized vertical list: materializes only the on-screen window of a large uniform
/// collection, holding the rest open with two spacers.
struct VirtualizedView {
    /// Caller-supplied container style. The list forces `flex_direction: Column` and
    /// `overflow_y: Scroll` on top of it and supplies the rest (height, border, …).
    style: Style,
    /// Every item's extent in cells — the uniform stride.
    stride: u16,
    /// Items materialized beyond each edge of the visible range.
    overscan: usize,
    /// The collection's length, read reactively so the list resizes as it grows or shrinks.
    count: Box<dyn Fn() -> usize + Send + Sync>,
    /// Builds the item at an index. Called when the index enters the window, and its subtree is
    /// disposed when the index leaves — so it is built once per materialization, not per frame.
    render: Arc<dyn Fn(usize) -> View + Send + Sync>,
}

/// A virtualized vertical list over a large uniform collection.
///
/// Only the items in view (plus `overscan` on each side) exist in the DOM at any moment; the rest
/// are stood in for by two spacers, so the scroll container's content size — and thus its
/// scrollbar — stays honest. As the list scrolls, entering items mount and leaving items unmount
/// through the same keyed differ as [`keyed`], so an item that merely shifts within the window
/// keeps its identity.
///
/// `stride` is each item's height in cells and `overscan` the number of extra items kept beyond
/// each edge. `count` is read reactively — grow or shrink the collection and the window follows.
/// `render` builds an item from its index; it runs once when the index enters the window, so an
/// item that must reflect changing data should read it through a signal.
///
/// The container reads its own viewport from the layout snapshot each frame. Until it has been laid
/// out — the first frame, before any size is known — the window holds empty rather than collapsing
/// to nothing, and fills in once the viewport is known. The caller's `style` must give the
/// container a bounded height (a fixed height, or flex within a bounded parent); an unbounded
/// container has no viewport to virtualize against.
pub fn virtual_list<C, R>(style: Style, stride: u16, overscan: usize, count: C, render: R) -> View
where
    C: Fn() -> usize + Send + Sync + 'static,
    R: Fn(usize) -> View + Send + Sync + 'static,
{
    View {
        kind: ViewKind::Virtualized(Box::new(VirtualizedView {
            style,
            stride,
            overscan,
            count: Box::new(count),
            render: Arc::new(render),
        })),
    }
}

/// A component instance: a body that runs once at mount, under its own child scope.
///
/// This is the primitive the `#[component]` macro lowers to. `build` runs when the component
/// mounts, with a fresh child scope of the mount entered, so every signal and effect it creates
/// is owned by that scope and freed when the component unmounts. It runs **once** — props arrive
/// as signals, so a prop change reruns the affected binding rather than the whole body, and the
/// component node keeps its identity (scroll offset, focus, running transitions) across it.
///
/// A component adds no wrapper node: its body's top-level nodes mount directly under the parent,
/// exactly as a [`fragment`] would.
pub fn component<F>(build: F) -> View
where
    F: FnOnce() -> View + Send + Sync + 'static,
{
    View {
        kind: ViewKind::Component(Box::new(build)),
    }
}

/// Deferred child content a container mounts into itself.
///
/// Unlike a constructed [`View`] handed over eagerly, a `Children` thunk is *not* invoked
/// when the parent is built — the framework calls it at mount time, and again when a signal
/// it reads changes. So a container's own body is built once while its children swap beneath
/// it, and the container node keeps its identity (and thus its scroll offset, focus, and
/// running transitions) across the swap. This is the shape a `#[component]` receives a
/// `children: Children` prop in.
///
/// Cheap to clone (an `Arc` bump), so a component body places it with `{ children.clone() }`.
#[derive(Clone)]
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

/// Empty children — a component's `children` prop when the call site supplies none.
impl Default for Children {
    fn default() -> Self {
        Children::new(|| fragment(Vec::<View>::new()))
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
    match scope.run(|| with_mount_callbacks(|| mount_view(doc, parent, before, view, &scope))) {
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
        ViewKind::Keyed(keyed) => {
            // Same anchor discipline as a dynamic block: an out-of-flow end marker the items sit
            // before, so the list can empty to nothing and still hold its position among siblings.
            let marker = doc.create_box()?;
            doc.set_style(marker, &marker_style())?;
            place(doc, parent, marker, before)?;
            register_remove(doc, parent, marker, scope);

            // The live items in DOM order, each its own mount so its scope, nodes, and reactive
            // state are disposed independently when its key leaves.
            let items: Arc<Mutex<Vec<(ErasedKey, Mounted)>>> = Arc::new(Mutex::new(Vec::new()));

            // Each item's content mounts under a root scope (as a dynamic block's does), not under
            // the owner scope, so disposing the owner does not reach them structurally — tear them
            // down explicitly.
            let held = items.clone();
            scope.on_cleanup(move || {
                for (_, mounted) in std::mem::take(&mut *lock(&held)) {
                    mounted.unmount();
                }
            });

            let produce = keyed.produce;
            let doc = doc.clone();
            effect(move || {
                // Reading `produce()` subscribes this effect to the signals the author read while
                // building the key sequence — the collection, not the items' own bindings.
                let produced = produce();
                let mut state = lock(&items);
                reconcile(&doc, parent, marker, &mut state, produced);
            });

            Ok(vec![marker])
        }
        ViewKind::Virtualized(view) => mount_virtualized(doc, parent, before, *view, scope),
        ViewKind::Component(build) => {
            // The body runs under a child of the mount scope, so its signals, effects, and
            // nodes belong to `child` and are freed when the mount is disposed — the child is
            // registered under `scope`, so disposal cascades without an explicit hook. Running
            // `build` with `child` entered is what attaches the body's reactive state there;
            // mounting the produced view under the same scope keeps its bindings owned there too.
            let child = scope.child();
            child.run(|| with_mount_callbacks(|| mount_view(doc, parent, before, build(), &child)))
        }
    }
}

/// What a surviving or newly seen key resolves to during one [`reconcile`] pass.
enum Slot {
    /// A key present last pass: its `usize` is its old DOM position (for the order-preserved
    /// check) and it carries the [`Mounted`] whose nodes are reused untouched.
    Reuse(usize, Mounted),
    /// A key new this pass: its body has not been built yet.
    New(ItemThunk),
}

/// Diff a freshly produced key sequence against the mounted items, in place.
///
/// Reuses the [`Mounted`] behind every surviving key, mounts each new key's body, and unmounts
/// every vanished key. Then it puts the DOM in the new order with the fewest moves it can cheaply
/// justify: if the surviving items kept their relative order (the pure insert/remove/append case)
/// none of them move at all — only new items are inserted and removed ones dropped; otherwise it
/// repositions every surviving item. Placement runs right-to-left so the anchor to insert before
/// is always the already-placed node to the item's right, starting at the end `marker`.
fn reconcile(
    doc: &Document,
    parent: NodeId,
    marker: NodeId,
    state: &mut Vec<(ErasedKey, Mounted)>,
    produced: Vec<(ErasedKey, ItemThunk)>,
) {
    // Index the mounted items by key, remembering each one's position for the order check. The
    // sequence always holds unique keys (we dedup below), so a collision here can only be a prior
    // bug — unmount the evicted subtree rather than leak it.
    let mut old: HashMap<ErasedKey, (usize, Mounted)> = HashMap::with_capacity(state.len());
    for (pos, (key, mounted)) in state.drain(..).enumerate() {
        if let Some((_, evicted)) = old.insert(key, (pos, mounted)) {
            evicted.unmount();
        }
    }

    // Resolve the new order into slots, dropping duplicate keys. A surviving key is *removed* from
    // `old`, so whatever stays in `old` afterward is exactly the vanished keys.
    let mut seen: HashSet<ErasedKey> = HashSet::with_capacity(produced.len());
    let mut order: Vec<(ErasedKey, Slot)> = Vec::with_capacity(produced.len());
    for (key, item) in produced {
        if !seen.insert(key.clone()) {
            tracing::warn!("keyed list has a duplicate key; the later item is dropped");
            continue;
        }
        match old.remove(&key) {
            Some((pos, mounted)) => order.push((key, Slot::Reuse(pos, mounted))),
            None => order.push((key, Slot::New(item))),
        }
    }

    // Vanished keys.
    for (_, (_, mounted)) in old.drain() {
        mounted.unmount();
    }

    // Did the survivors keep their relative order? If their old positions read strictly increasing
    // down the new order, they are already in the right DOM order and need no moves.
    let mut preserved = true;
    let mut last_pos: Option<usize> = None;
    for (_, slot) in &order {
        if let Slot::Reuse(pos, _) = slot {
            if last_pos.is_some_and(|prev| *pos <= prev) {
                preserved = false;
                break;
            }
            last_pos = Some(*pos);
        }
    }

    // Place right-to-left. New items mount before the running anchor; survivors move there only on
    // a real reorder. The anchor walks leftward as each item's first node is settled.
    let mut anchor = marker;
    let mut placed: Vec<(ErasedKey, Mounted)> = Vec::with_capacity(order.len());
    for (key, slot) in order.into_iter().rev() {
        let mounted = match slot {
            Slot::New(item) => match mount_at(doc, parent, Some(anchor), item()) {
                Ok(mounted) => mounted,
                Err(err) => {
                    tracing::warn!(?parent, "keyed item failed to mount: {err}");
                    continue;
                }
            },
            Slot::Reuse(_, mounted) => {
                if !preserved {
                    // Moving each root before the anchor in order rebuilds the run left-to-right:
                    // `[… r0, r1, anchor]`.
                    for &root in mounted.roots() {
                        if let Err(err) = doc.move_child(parent, root, anchor) {
                            tracing::warn!(?root, "keyed reorder failed: {err}");
                        }
                    }
                }
                mounted
            }
        };
        if let Some(&first) = mounted.roots().first() {
            anchor = first;
        }
        placed.push((key, mounted));
    }
    placed.reverse();
    *state = placed;
}

/// Mount a virtualized list: a scroll container holding a lead spacer, the live item window, and a
/// trail spacer, driven by a [`Virtualizer`] off the container's own scroll offset and viewport.
///
/// The item window is a [`keyed`] block over a `window` signal the driver writes, so materializing
/// and dematerializing items reuses the identity-preserving differ. The driver runs off engine
/// events (scroll and post-frame) plus a count effect, and touches the DOM **only** when the
/// virtualizer reports a changed window — otherwise a post-frame handler that mutated every frame
/// would keep the renderer permanently awake.
fn mount_virtualized(
    doc: &Document,
    parent: NodeId,
    before: Option<NodeId>,
    view: VirtualizedView,
    scope: &Scope,
) -> Result<Vec<NodeId>> {
    // The scroll container. We own the structural bits; the caller's style supplies the rest.
    let container = doc.create_box()?;
    let mut style = view.style;
    style.flex_direction(FlexDirection::Column);
    style.overflow_y(Overflow::Scroll);
    doc.set_style(container, &style)?;
    place(doc, parent, container, before)?;
    register_remove(doc, parent, container, scope);

    // Sibling order under the container: lead spacer, the item window, trail spacer.
    let lead = doc.create_box()?;
    doc.set_style(lead, &spacer_style(0))?;
    doc.append_child(container, lead)?;

    // The materialized index range: written by the driver, read by the keyed differ. Created under
    // the mount scope, so it is disposed with the rest of the subtree.
    let window = state(0..0usize);
    let render = view.render;
    let items = keyed(move || {
        let render = render.clone();
        window
            .get()
            .map(move |index| {
                let render = render.clone();
                (index, move || render(index))
            })
            .collect::<Vec<_>>()
    });
    mount_view(doc, container, None, items, scope)?;

    let trail = doc.create_box()?;
    doc.set_style(trail, &spacer_style(0))?;
    doc.append_child(container, trail)?;

    let virt = Arc::new(Mutex::new(Virtualizer::uniform(
        0,
        view.stride,
        view.overscan,
    )));

    // Recompute the window from the current offset and viewport, applying spacer sizes and the new
    // item range only if something actually changed.
    let recompute = {
        let doc = doc.clone();
        let virt = virt.clone();
        Arc::new(move || {
            // The viewport is unknown until the container is in the layout snapshot. Skip rather
            // than window to a zero viewport, which would dematerialize the whole list.
            let Some(node) = doc.get_node(container) else {
                return;
            };
            let Some(layout) = node.layout else {
                return;
            };
            let viewport = layout.scrollport.height;
            let offset = doc.scroll_offset(container).y;
            let update = lock(&virt).update(offset, viewport);
            if let Some(update) = update {
                let lead_cells = clamp_cells(update.window.lead);
                let trail_cells = clamp_cells(update.window.trail);
                let _ = doc.update_style(lead, |s| s.height(Length::Cells(lead_cells)));
                let _ = doc.update_style(trail, |s| s.height(Length::Cells(trail_cells)));
                // Setting the window reruns the keyed differ, mounting and unmounting items.
                window.set(update.window.items);
            }
        })
    };

    // Count changes resize the collection and recompute — in an effect, so it tracks the count's
    // signals. Runs once at mount, where the container is not yet laid out, so `recompute` skips.
    {
        let virt = virt.clone();
        let count = view.count;
        let recompute = recompute.clone();
        effect(move || {
            let n = count();
            lock(&virt).set_count(n);
            recompute();
        });
    }

    // Offset and viewport changes arrive as engine events, not signals. Scrolling always schedules
    // a frame, so post-frame alone would catch it; on_scroll recomputes *before* that frame paints,
    // so a scroll and the items it reveals land in the same frame.
    {
        let recompute = recompute.clone();
        let handle = doc.on_scroll(container, move |_| recompute())?;
        let doc = doc.clone();
        scope.on_cleanup(move || {
            doc.remove_listener(handle);
        });
    }
    {
        let handle = doc.on_post_frame(move |_| recompute());
        let doc = doc.clone();
        scope.on_cleanup(move || {
            doc.remove_listener(handle);
        });
    }

    Ok(vec![container])
}

/// A virtual-list spacer: `cells` of main-axis space that must never shrink, so it faithfully
/// holds open the extent of the items it stands in for.
fn spacer_style(cells: u16) -> Style {
    let mut style = Style::new();
    style.height(Length::Cells(cells));
    style.flex_shrink(0.0);
    style
}

/// Clamp a spacer extent to the range a `Length::Cells` holds. A collection tall enough to overflow
/// `u16` saturates rather than wrapping — the scroll range is `u16` anyway.
fn clamp_cells(cells: u64) -> u16 {
    cells.min(u64::from(u16::MAX)) as u16
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
