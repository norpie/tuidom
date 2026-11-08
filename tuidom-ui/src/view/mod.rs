//! The view layer: declarative descriptions of a subtree that *mount* onto real engine
//! nodes and bind signals to them.
//!
//! A [`View`] is inert until [`mount`]ed under a parent `NodeId`. Mounting opens an owner
//! [`Scope`], and every node, binding effect, and listener created during the mount is owned
//! by it — so [`Mounted::unmount`] tears the whole subtree down by disposing that
//! one scope. This module connects the signal runtime to the engine: reads inside a binding
//! subscribe through the runtime, and a signal change reruns only that binding's effect.
//!
//! # Static and reactive alike
//!
//! Every bindable property takes a [`Source`], which is either a constant or a closure. A
//! constant is applied once at mount; a closure becomes an [`effect`], so it reapplies
//! whenever a signal it read changes. Each property is its own effect, so a change to one
//! never reapplies another — the granularity the reactive model promises.
//!
//! # Structure that reacts, not just properties
//!
//! Three view kinds let the *shape* of the subtree react, not only its leaf properties.
//! [`dynamic`] re-runs a closure and swaps its whole produced view in place. [`keyed`] does the
//! same over an ordered `(key, item)` sequence, but diffs it instead of rebuilding: a key present
//! in both the old and new sequence keeps the nodes it already owns — and with them its scroll
//! offset, focus, and running transitions, all of which the engine keys by `NodeId`, not by
//! position — so a reorder *moves* an item's nodes rather than minting new ones and dropping that
//! state. [`virtual_list`] and [`virtual_list_measured`] build on that same differ to materialize
//! only the on-screen window of a large collection, standing in for the rest with two spacers so
//! the scroll container's size — and thus its scrollbar — stays honest.
//!
//! # Reaching nodes
//!
//! Some engine operations are imperative rather than declared: moving focus, scrolling, starting
//! a keyframe animation, reading layout, placing an input's cursor. A component body never sees
//! the `NodeId`s its view creates, so it creates a [`NodeRef`] with [`node_ref`] and binds it to
//! an element with `ref:`. The ref holds the node while that element is mounted and carries the
//! document with it, since event handlers run outside any scope. [`use_document`] gives a body
//! the document itself, for document-wide operations such as quitting or setting a color
//! variable.
//!
//! Following another node's geometry goes through the same ref. Inside an effect,
//! [`NodeRef::layout`] reruns it when the node is laid out differently, and
//! [`NodeRef::screen_rect`], the position to place a tooltip from, also reruns it when an
//! ancestor scrolls. [`NodeRef::visible_rect`] is the part of that rect left after clipping, or
//! `None` once the node is scrolled or clipped out of view, for hiding the tooltip. All three
//! report the frame that just finished, so anything drawn from them lands one frame later. An
//! effect that resizes the node it reads can keep changing the layout it reads and never
//! settle.
//!
//! Focus trapping is declared instead: `trap_focus: true` traps focus inside an element for as
//! long as it is mounted, and gives focus back to where it was when the element unmounts.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};

use tuidom::animation::TransitionConfig;
use tuidom::event::{
    AnimationEndEvent, AnimationIterationEvent, FocusEvent, InputEvent, KeyEvent, ListenerHandle,
    MouseEvent, ScrollEvent, TransitionEndEvent, WheelEvent,
};
use tuidom::style::{FlexDirection, Length, Overflow, Position, Style};
use tuidom::virtualize::Virtualizer;
use tuidom::{Document, NodeId, Result};

use crate::runtime::{Scope, ScopeId, Signal, effect, state, with_inline_flush};

mod layout_watch;
mod node_ref;

pub(crate) use node_ref::provide_document;
pub use node_ref::{NodeRef, node_ref, use_document};

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

/// An element description: a builder for its style, attributes, focusability, disabled state,
/// pseudo-state styles, listeners, and children.
///
/// Every setter has a `dyn_` counterpart taking a closure, which binds the property reactively
/// instead of to a constant.
pub struct Element {
    kind: ElementKind,
    style: Option<Source<Style>>,
    attrs: Vec<(String, Source<String>)>,
    focusable: Option<Source<bool>>,
    disabled: Option<Source<bool>>,
    focus_style: Option<Source<Style>>,
    active_style: Option<Source<Style>>,
    disabled_style: Option<Source<Style>>,
    transitions: Vec<TransitionConfig>,
    trap_focus: bool,
    input: InputProps,
    listeners: Vec<ListenerReg>,
    node_ref: Option<NodeRef>,
    children: Vec<View>,
}

/// The properties only an `Input` node has. Set on a `Box`, they are ignored with a warning.
#[derive(Default)]
struct InputProps {
    value: Option<Source<String>>,
    multiline: Option<Source<bool>>,
    mask: Option<Source<Option<char>>>,
}

impl InputProps {
    fn is_empty(&self) -> bool {
        self.value.is_none() && self.multiline.is_none() && self.mask.is_none()
    }
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

/// How a [`VirtualizedView`] sizes its items.
enum VirtualMode {
    /// Every item is `stride` cells — no measurement needed.
    Uniform { stride: u16 },
    /// Items vary; each starts at `estimate` cells and is corrected to its real extent once laid
    /// out, with the scroll offset anchored so measuring off-screen items does not shift the view.
    Measured { estimate: u16 },
}

/// A virtualized vertical list: materializes only the on-screen window of a large collection,
/// holding the rest open with two spacers.
struct VirtualizedView {
    /// Caller-supplied container style. The list forces `flex_direction: Column` and
    /// `overflow_y: Scroll` on top of it and supplies the rest (height, border, …).
    style: Style,
    /// Uniform stride or measured estimate.
    mode: VirtualMode,
    /// Items materialized beyond each edge of the visible range.
    overscan: usize,
    /// The collection's length, read reactively so the list resizes as it grows or shrinks.
    count: Box<dyn Fn() -> usize + Send + Sync>,
    /// Builds the item at an index. Called when the index enters the window, and its subtree is
    /// disposed when the index leaves — so it is built once per materialization, not per frame.
    render: Arc<dyn Fn(usize) -> View + Send + Sync>,
}

/// A virtualized vertical list over a large **uniform** collection — every item the same height.
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
///
/// The windowing arithmetic — which items the window holds, the overscan margin, the two spacer
/// extents — is the engine's `tuidom::virtualize::Virtualizer`; this drives it against live scroll
/// and layout. See the engine's virtualization guide (`docs/virtualization.md`) for the math.
///
/// For items whose height is not known ahead of time, see [`virtual_list_measured`].
pub fn virtual_list<C, R>(style: Style, stride: u16, overscan: usize, count: C, render: R) -> View
where
    C: Fn() -> usize + Send + Sync + 'static,
    R: Fn(usize) -> View + Send + Sync + 'static,
{
    virtualized(
        style,
        VirtualMode::Uniform { stride },
        overscan,
        count,
        render,
    )
}

/// A virtualized vertical list over a large collection of **variably sized** items.
///
/// Like [`virtual_list`], but item heights are not known in advance. Each item starts at `estimate`
/// cells; once an item has been laid out, its real height is recorded, so the spacers and the
/// scroll range converge on the truth as the user scrolls. Measuring an item *above* the viewport
/// would shift the visible content, so the scroll offset is anchored to absorb the change and keep
/// what is on screen pinned.
///
/// A good `estimate` matters only for scroll-bar accuracy before an item has been seen; it must be
/// at least one cell, or an unmeasured item would occupy nothing and never scroll into view to be
/// measured. Item heights depend on the container's width, so a width change discards every recorded
/// height and re-measures — otherwise the total extent, and the scrollbar, would lie.
pub fn virtual_list_measured<C, R>(
    style: Style,
    estimate: u16,
    overscan: usize,
    count: C,
    render: R,
) -> View
where
    C: Fn() -> usize + Send + Sync + 'static,
    R: Fn(usize) -> View + Send + Sync + 'static,
{
    virtualized(
        style,
        VirtualMode::Measured { estimate },
        overscan,
        count,
        render,
    )
}

/// Shared constructor behind [`virtual_list`] and [`virtual_list_measured`].
fn virtualized<C, R>(style: Style, mode: VirtualMode, overscan: usize, count: C, render: R) -> View
where
    C: Fn() -> usize + Send + Sync + 'static,
    R: Fn(usize) -> View + Send + Sync + 'static,
{
    View {
        kind: ViewKind::Virtualized(Box::new(VirtualizedView {
            style,
            mode,
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
/// starts empty.
///
/// [`value`](Element::value) and [`dyn_value`](Element::dyn_value) write its value, and
/// [`on_input`](Element::on_input) reports each edit a key press makes;
/// [`bind_value`](Element::bind_value) does both against one signal. The signal does not force the
/// value: an edit the handler does not record stays in the input until something writes a new
/// value. To block an edit, call `prevent_default` on the key press.
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
        disabled: None,
        focus_style: None,
        active_style: None,
        disabled_style: None,
        transitions: Vec::new(),
        trap_focus: false,
        input: InputProps::default(),
        listeners: Vec::new(),
        node_ref: None,
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

    /// Declare the transitions this element's style changes animate with, at most one per
    /// property. A later call replaces the list.
    ///
    /// They are installed once at mount, after the initial style is applied, so the element
    /// does not animate in from default values. Changes to the base style and to the focus,
    /// active, and disabled styles all animate, since the engine compares the merged style.
    pub fn transitions(mut self, transitions: impl IntoIterator<Item = TransitionConfig>) -> Self {
        self.transitions = transitions.into_iter().collect();
        self
    }

    /// Trap focus inside this element while it is mounted, as a modal does: nodes outside it
    /// cannot be focused or interacted with, and Tab and the arrow keys stay within it. The
    /// element's style must make it a stacking context; otherwise the trap is not opened and a
    /// warning is logged.
    ///
    /// The trap opens before the children mount, so it remembers the focus from outside and gives
    /// it back when the element unmounts. Once the children are in, focus moves to the first
    /// focusable node inside unless something inside already took it, such as an input focusing
    /// itself in [`on_mount`]. Set once at mount; to close the trap, unmount the element.
    pub fn trap_focus(mut self, trap: bool) -> Self {
        self.trap_focus = trap;
        self
    }

    /// Set whether the element is disabled. Disabling covers the whole subtree: it blocks
    /// interaction, blurs focus inside it, and applies each node's disabled style.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = Some(Source::Static(disabled));
        self
    }

    /// Bind the disabled state reactively.
    pub fn dyn_disabled<F>(mut self, f: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.disabled = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set a style merged over the base style while the element is focused. Unset properties
    /// leave the base style's value.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = Some(Source::Static(style));
        self
    }

    /// Bind the focus style reactively.
    pub fn dyn_focus_style<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Style + Send + Sync + 'static,
    {
        self.focus_style = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set a style merged over the base style while the element is being pressed.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = Some(Source::Static(style));
        self
    }

    /// Bind the active style reactively.
    pub fn dyn_active_style<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Style + Send + Sync + 'static,
    {
        self.active_style = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set a style merged over the base style while the element is disabled, directly or
    /// through an ancestor.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = Some(Source::Static(style));
        self
    }

    /// Bind the disabled style reactively.
    pub fn dyn_disabled_style<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Style + Send + Sync + 'static,
    {
        self.disabled_style = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set an input's value once, at mount.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.input.value = Some(Source::Static(value.into()));
        self
    }

    /// Bind an input's value reactively. A write is skipped when the input already holds the
    /// same string, which is the usual case right after a key press the signal just recorded.
    ///
    /// When a new value replaces the old one, the engine clamps the cursor and selection into it,
    /// so clearing the input moves the cursor to the start and a longer value leaves it where it
    /// was.
    pub fn dyn_value<F>(mut self, f: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.input.value = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Bind an input's value to `signal` in both directions: the signal writes the value, and
    /// each edit a key press makes sets the signal.
    pub fn bind_value(self, signal: Signal<String>) -> Self {
        self.dyn_value(move || signal.get())
            .on_input(move |ev| signal.set(ev.value.clone()))
    }

    /// Set whether an input accepts newlines. A multiline input inserts one on Enter.
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.input.multiline = Some(Source::Static(multiline));
        self
    }

    /// Bind whether an input accepts newlines reactively.
    pub fn dyn_multiline<F>(mut self, f: F) -> Self
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.input.multiline = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Set the character an input paints in place of each character of its value, such as `*`
    /// for a password. The stored value is unchanged.
    pub fn mask(mut self, mask: Option<char>) -> Self {
        self.input.mask = Some(Source::Static(mask));
        self
    }

    /// Bind an input's mask reactively.
    pub fn dyn_mask<F>(mut self, f: F) -> Self
    where
        F: Fn() -> Option<char> + Send + Sync + 'static,
    {
        self.input.mask = Some(Source::Reactive(Box::new(f)));
        self
    }

    /// Bind `node_ref` to this element's node while it is mounted. It fills once the element's
    /// children have mounted and empties when the element unmounts.
    pub fn node_ref(mut self, node_ref: NodeRef) -> Self {
        self.node_ref = Some(node_ref);
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

/// One listener method per targeted engine event. They differ only in name and event type, so
/// they come from this table. `view!` keeps its own copy of the names in `is_known_event`.
macro_rules! element_events {
    ($($(#[$doc:meta])* $name:ident: $event:ty;)*) => {
        impl Element {
            $(
                $(#[$doc])*
                ///
                /// The listener is removed when the element unmounts.
                pub fn $name<F>(mut self, handler: F) -> Self
                where
                    F: Fn(&mut $event) + Send + Sync + 'static,
                {
                    self.listeners.push(Box::new(move |doc, node| {
                        doc.$name(node, move |ev| with_inline_flush(|| handler(ev)))
                    }));
                    self
                }
            )*
        }
    };
}

element_events! {
    /// Register a click listener.
    on_click: MouseEvent;
    /// Register a key-press listener. It runs before the engine's default action, so
    /// `prevent_default` can stop an input from editing.
    on_key_press: KeyEvent;
    /// Register a mouse-button-down listener.
    on_mouse_down: MouseEvent;
    /// Register a mouse-button-up listener.
    on_mouse_up: MouseEvent;
    /// Register a focus listener. It bubbles, so it also fires when a descendant gains focus.
    on_focus: FocusEvent;
    /// Register a blur listener. It bubbles, so it also fires when a descendant loses focus.
    on_blur: FocusEvent;
    /// Register a mouse-wheel listener.
    on_wheel: WheelEvent;
    /// Register a listener for this element's scroll offset changing.
    on_scroll: ScrollEvent;
    /// Register a listener for an input's value changing from a key press. It bubbles, so a
    /// container sees every input inside it.
    on_input: InputEvent;
    /// Register a listener for a transition finishing. It bubbles.
    on_transition_end: TransitionEndEvent;
    /// Register a listener for a keyframe animation finishing. It bubbles.
    on_animation_end: AnimationEndEvent;
    /// Register a listener for a keyframe animation completing an iteration other than
    /// its last, which is reported as the end instead. It bubbles.
    on_animation_iteration: AnimationIterationEvent;
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
    mount_at(doc, parent, None, view.into(), None)
}

/// Mount `view` under `parent` with its scope a child of `owner`, so it inherits the owner's
/// session and context.
pub(crate) fn mount_owned(
    doc: &Document,
    parent: NodeId,
    view: View,
    owner: &Scope,
) -> Result<Mounted> {
    mount_at(doc, parent, None, view, Some(owner.id()))
}

/// Mount a view under a fresh scope, placing its top-level nodes before `before` (a dynamic
/// block's marker) or appended. A partial mount that errors is rolled back.
///
/// With an `owner`, the fresh scope is its child, so content a dynamic block or keyed list
/// remounts stays on the owner's parent chain: context resolves through it, and disposing the
/// owner disposes the content structurally.
fn mount_at(
    doc: &Document,
    parent: NodeId,
    before: Option<NodeId>,
    view: View,
    owner: Option<ScopeId>,
) -> Result<Mounted> {
    let scope = match owner {
        Some(owner) => Scope::child_of(owner),
        None => {
            let scope = Scope::new();
            scope.run(|| provide_document(doc));
            scope
        }
    };
    // Mounting is trusted event-task work: the binding effects it runs, and any signal writes an
    // `on_mount` callback makes, flush inline in this frame rather than deferring to a session mailbox.
    let mounted = with_inline_flush(|| {
        scope.run(|| with_mount_callbacks(|| mount_view(doc, parent, before, view, &scope)))
    });
    match mounted {
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

            bind_node(doc, node, "style", element.style, |d, n, s: Style| {
                d.set_style(n, &s)
            });
            for transition in element.transitions {
                doc.set_transition(node, transition)?;
            }
            for (key, value) in element.attrs {
                let d = doc.clone();
                bind(value, move |value| {
                    if let Err(err) = d.set_attr(node, key.clone(), value) {
                        tracing::warn!(?node, "attribute binding failed: {err}");
                    }
                });
            }
            bind_node(
                doc,
                node,
                "focusable",
                element.focusable,
                Document::set_focusable,
            );
            bind_node(
                doc,
                node,
                "disabled",
                element.disabled,
                Document::set_disabled,
            );
            bind_node(
                doc,
                node,
                "focus style",
                element.focus_style,
                |d, n, s: Style| d.set_focus_style(n, &s),
            );
            bind_node(
                doc,
                node,
                "active style",
                element.active_style,
                |d, n, s: Style| d.set_active_style(n, &s),
            );
            bind_node(
                doc,
                node,
                "disabled style",
                element.disabled_style,
                |d, n, s: Style| d.set_disabled_style(n, &s),
            );
            match element.kind {
                ElementKind::Input => bind_input(doc, node, element.input),
                ElementKind::Box if !element.input.is_empty() => {
                    tracing::warn!(?node, "input-only properties set on a box; ignored");
                }
                ElementKind::Box => {}
            }
            for register in element.listeners {
                let handle = register(doc, node)?;
                let d = doc.clone();
                scope.on_cleanup(move || {
                    d.remove_listener(handle);
                });
            }
            let trapped = element.trap_focus
                && doc
                    .push_focus_context(node)
                    .inspect_err(|err| tracing::warn!(?node, "focus trap not opened: {err}"))
                    .is_ok();
            for child in element.children {
                mount_view(doc, node, None, child, scope)?;
            }
            // Focus is scoped to the active context, so anything focused now is inside the trap.
            if trapped
                && doc.focused().is_none()
                && let Err(err) = doc.focus_first()
            {
                tracing::warn!(?node, "focusing inside the trap failed: {err}");
            }
            if let Some(node_ref) = element.node_ref {
                node_ref.fill(doc, node);
                scope.on_cleanup(move || node_ref.release(node));
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

            let owner = scope.id();
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
                match mount_at(&doc, parent, Some(marker), view, Some(owner)) {
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

            let owner = scope.id();
            let produce = keyed.produce;
            let doc = doc.clone();
            effect(move || {
                // Reading `produce()` subscribes this effect to the signals the author read while
                // building the key sequence — the collection, not the items' own bindings.
                let produced = produce();
                let mut state = lock(&items);
                reconcile(&doc, parent, marker, owner, &mut state, produced);
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
    owner: ScopeId,
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
            Slot::New(item) => match mount_at(doc, parent, Some(anchor), item(), Some(owner)) {
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

/// The driver's mutable state behind one lock: the window math, the range currently in the DOM (so
/// the measure pass can find each item's node), and the cross-axis extent measurements were taken
/// at (so a width change can invalidate them).
struct DriverState {
    virt: Virtualizer,
    materialized: Range<usize>,
    last_cross: Option<u16>,
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

    // An inner content box sized to its children, so items keep their natural height and overflow
    // rather than being shrunk to fit the (smaller) viewport. It is the single child of the scroll
    // container; the spacers and item window live inside it.
    let content = doc.create_box()?;
    doc.set_style(content, &content_style())?;
    doc.append_child(container, content)?;

    // Sibling order under the content box: lead spacer, the item window, trail spacer.
    let lead = doc.create_box()?;
    doc.set_style(lead, &spacer_style(0))?;
    doc.append_child(content, lead)?;

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
    mount_view(doc, content, None, items, scope)?;

    let trail = doc.create_box()?;
    doc.set_style(trail, &spacer_style(0))?;
    doc.append_child(content, trail)?;

    let measured = matches!(view.mode, VirtualMode::Measured { .. });
    let virt = match view.mode {
        VirtualMode::Uniform { stride } => Virtualizer::uniform(0, stride, view.overscan),
        VirtualMode::Measured { estimate } => Virtualizer::measured(0, estimate, view.overscan),
    };
    let state = Arc::new(Mutex::new(DriverState {
        virt,
        materialized: 0..0,
        last_cross: None,
    }));

    // Recompute the window from the current offset and viewport, applying spacer sizes and the new
    // item range only when something changed. In measured mode it first records the real extents of
    // the on-screen items and anchors the offset so measuring an item above the fold does not
    // shift the view.
    let recompute = {
        let doc = doc.clone();
        let state = state.clone();
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
            let cross = layout.scrollport.width;
            let offset = doc.scroll_offset(container).y;

            let mut anchor_shift: i64 = 0;
            let update = {
                let mut st = lock(&state);

                if measured {
                    // Recorded heights are only valid at the width they were measured at: a
                    // narrower container wraps text taller. On a cross-axis change, drop every
                    // measurement and let the on-screen items re-measure below.
                    if st.last_cross != Some(cross) {
                        if let Some(cache) = st.virt.cache_mut() {
                            cache.invalidate_all();
                        }
                        st.last_cross = Some(cross);
                    }

                    // Measure the currently materialized items against the layout snapshot. An item
                    // that sits entirely above the viewport top and changes size shifts the visible
                    // content, so its delta is absorbed into the offset (anchoring).
                    let range = st.materialized.clone();
                    let kids = doc.get_children(content);
                    // kids are [lead, items…, keyed marker, trail]; the items map to `range` in
                    // ascending order. A mismatch means the DOM is mid-change — skip this pass.
                    if kids.len() == range.len() + 3 {
                        for (slot, index) in range.clone().enumerate() {
                            let item = kids[1 + slot];
                            let height = doc.get_node(item).and_then(|n| n.layout).map(|l| l.rect);
                            if let Some(rect) = height {
                                let above = st.virt.offset_of(index + 1) <= u64::from(offset);
                                let delta = st.virt.record(index, rect.height);
                                if above {
                                    anchor_shift += delta;
                                }
                            }
                        }
                    }
                }

                let effective =
                    (i64::from(offset) + anchor_shift).clamp(0, i64::from(u16::MAX)) as u16;
                let update = st.virt.update(effective, viewport);
                if let Some(ref update) = update {
                    st.materialized = update.window.items.clone();
                }
                update
            };

            if let Some(update) = update {
                let lead_cells = clamp_cells(update.window.lead);
                let trail_cells = clamp_cells(update.window.trail);
                let _ = doc.update_style(lead, |s| s.height(Length::Cells(lead_cells)));
                let _ = doc.update_style(trail, |s| s.height(Length::Cells(trail_cells)));
                // Setting the window reruns the keyed differ, mounting and unmounting items.
                window.set(update.window.items);
            }

            // Apply the anchoring compensation last, with the lock released — `scroll_by` fires
            // `on_scroll`, which re-enters `recompute`; that pass records nothing new and windows
            // the same range, so it settles at once.
            if anchor_shift != 0 {
                let dy = anchor_shift.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                let _ = doc.scroll_by(container, 0, dy);
            }
        })
    };

    // Count changes resize the collection and recompute — in an effect, so it tracks the count's
    // signals. Runs once at mount, where the container is not yet laid out, so `recompute` skips.
    {
        let state = state.clone();
        let count = view.count;
        let recompute = recompute.clone();
        effect(move || {
            let n = count();
            lock(&state).virt.set_count(n);
            recompute();
        });
    }

    // Offset and viewport changes arrive as engine events, not signals. Scrolling always schedules
    // a frame, so post-frame alone would catch it; on_scroll recomputes *before* that frame paints,
    // so a scroll and the items it reveals land in the same frame.
    {
        let recompute = recompute.clone();
        // `recompute` writes the `window` signal; wrapping keeps that write inline so a scroll
        // and the items it reveals land in the same frame rather than a frame later.
        let handle = doc.on_scroll(container, move |_| with_inline_flush(|| recompute()))?;
        let doc = doc.clone();
        scope.on_cleanup(move || {
            doc.remove_listener(handle);
        });
    }
    {
        let handle = doc.on_post_frame(move |_| with_inline_flush(|| recompute()));
        let doc = doc.clone();
        scope.on_cleanup(move || {
            doc.remove_listener(handle);
        });
    }

    Ok(vec![container])
}

/// The scroll container's inner content box: a column that sizes to its children and never shrinks,
/// so the item window overflows the viewport rather than being squeezed into it.
fn content_style() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.flex_shrink(0.0);
    style
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

/// [`bind`] an optional source to one engine setter on `node`, logging a failed apply under
/// `what`.
fn bind_node<T>(
    doc: &Document,
    node: NodeId,
    what: &'static str,
    source: Option<Source<T>>,
    set: fn(&Document, NodeId, T) -> Result<()>,
) where
    T: Send + Sync + 'static,
{
    let Some(source) = source else {
        return;
    };
    let d = doc.clone();
    bind(source, move |value| {
        if let Err(err) = set(&d, node, value) {
            tracing::warn!(?node, "{what} binding failed: {err}");
        }
    });
}

/// Bind the properties only an input has.
fn bind_input(doc: &Document, node: NodeId, props: InputProps) {
    bind_node(
        doc,
        node,
        "multiline",
        props.multiline,
        Document::set_input_multiline,
    );
    bind_node(doc, node, "mask", props.mask, Document::set_input_mask);
    bind_node(doc, node, "value", props.value, |d, n, value: String| {
        if d.input_value(n)? == value {
            return Ok(());
        }
        d.set_input_value(n, value)
    });
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
