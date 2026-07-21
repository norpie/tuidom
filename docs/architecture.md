# Architecture

How tuidom is put together, and why it is put together that way. Terms in **bold** are
defined in the [glossary](GLOSSARY.md#core-concepts).

## The document model

A **Document** is a handle, not a container. It wraps an `Arc<DocumentInner>`, every
method takes `&self`, and cloning is a refcount bump:

```rust
let doc = Document::new()?;
let handle = doc.clone();       // same document, cheaply
tokio::spawn(async move { handle.quit(); });
```

This is why nothing in tuidom asks you to wrap a document in an `Arc` yourself, and why
`&Document` and `Document` are interchangeable at call sites. It is also why every
mutating method is `&self` rather than `&mut self`: the mutation happens through interior
mutability, so two handles in two tasks can both build tree without a lock in your code.

Nodes are **not** values you hold. `create_box` hands back a **NodeId** — two `u64`s, a
document identity and an arena index — and the node itself lives in the document's
**arena**:

```rust
pub struct NodeId {
    document_id: u64,
    index: u64,
}
```

The document identity is what makes a stale handle from another document a lookup miss
rather than a silent hit on an unrelated node. Two documents in one process allocate
indices independently, so index `7` exists in both; without the document half, passing a
handle across would quietly address the wrong tree. Errors are the point of that field,
not identity for its own sake.

A `NodeId` stays valid until its node is removed. After that, methods taking it return
`TuidomError` or `None` rather than panicking — handles are not reference-counted and
nothing keeps a removed node alive.

## The root node

Every document owns one permanent **root node**, a Box created by `Document::new()`:

```rust
let doc = Document::new()?;
doc.append_child(doc.root(), my_container)?;
```

It cannot be removed or reparented, and it is the entry point for layout, paint, and
document-level event dispatch. Having it be permanent removes an entire category of
`Option` from the engine's internals — there is no "document with no tree" state to
handle at every layer — and it gives key handling somewhere to dispatch when nothing is
focused.

## Where state lives

This is the most important thing on this page, and the one thing that will bite anything
built on top of tuidom if it is misunderstood.

State is split in three, by lifetime rather than by topic:

| Kind | Lives in | Examples |
|---|---|---|
| Structure and intent | `NodeData` in the arena | kind, parent, children, attributes, `Style` |
| Derived | caches on `NodeData` and the document | resolved style, RGB conversions |
| Runtime | `DocumentInner`, keyed by `NodeId` | scroll offsets, focus stack, selection, active node, pseudo styles, in-flight transitions |

Runtime state is **not** on the node. Scroll offsets live in a map on the document, so do
the focus stack, the selection anchors, the set of disabled nodes, and the animation
driver's in-flight transitions. Each is keyed by `NodeId`.

The consequence is worth stating directly, because it is a design constraint on everything
downstream: **a node's identity is what owns its runtime state.** Remove a node and
recreate an identical one, and it comes back scrolled to the top, unfocused, unselected,
with any transition mid-flight discarded. Nothing about the new node's style or content
differs, but the user's place in it is gone.

So a framework layered over tuidom must **patch live nodes**, never rebuild subtrees to
reflect new data. This is not a performance argument — rebuilding is fast — it is a
correctness one. A list that rebuilds its rows on every update scrolls itself to the top
under the user's hands.

## Layout is published, not stored

Computed layout does not live on `NodeData`. Each layout pass replaces the contents of one
document-level map — the **layout snapshot** — under a single lock:

```rust
layout_snapshot: RwLock<NodeMap<NodeLayout>>,
```

Each entry holds a node's rectangle and its maximum scroll per axis, both produced by the
same taffy pass. Publishing all of it at once is what lets a reader take a consistent view:
hit-testing, scroll clamping, and scrollbar geometry all read rects and scroll maxima
together, and none of them can catch a half-updated tree where some nodes have moved and
others have not.

Rects are screen-absolute regardless of how a node was positioned, and may be negative or
offscreen — clipping to the terminal grid is a render concern, not a layout one.

## How a frame happens

`doc.run()` spawns three tasks and waits for any of them to finish:

```
input_task    terminal → RuntimeEvent → queue
event_task    queue → coalesce → dispatch to listeners → mutate DOM → notify
render_task   notify → layout → paint → diff → flush
```

They are separate tasks for one reason: **the render task must never run user code.** A
handler that blocks, panics, or takes a lock cannot be allowed to stall the frame. So
listeners run on the event task, and anything the renderer wants to report — the
post-frame event, transition-end, animation-end — is pushed back onto the runtime queue to
be dispatched there, in order with input.

A frame, once the render task wakes:

1. **Wait for a slot** if `set_max_fps` capped the rate. Uncapped by default.
2. **Layout** — `compute_layout` runs taffy and publishes a new snapshot.
3. **Paint** — walk the tree in paint order into a fresh `Grid` of cells, applying scroll
   offsets as translations, clipping to scrollports, culling anything outside its clip.
4. **Diff** — compare the new grid against the previous one, cell by cell, producing a
   change list.
5. **Flush** — write escape sequences for the changes. If more than a third of cells
   changed, a full redraw is cheaper than the change list, and the renderer switches.
6. **Record and queue** — frame metrics go to the performance API, and the post-frame
   event goes on the runtime queue.

Then the grids swap: this frame's becomes next frame's comparison basis.

The order matters in one non-obvious way. Layout runs *inside* the frame, not when you
change a style. Mutating the DOM marks it dirty and notifies; nothing is recomputed until
a frame actually happens. Twenty style changes in one handler cost one layout pass.

## Passive by default

There is no tick. The render task sleeps on a `Notify` and wakes only when something asks
it to:

- a DOM mutation that changed anything
- a terminal resize
- an animation tick, while an animation is in flight
- a frames node's flip interval
- a `WhenScrolling` scrollbar's fade deadline

An idle tuidom application uses no CPU at all — not a low amount, none. It is not polling
at 60fps and skipping redraws; it is blocked on a condition variable. When the last
animation finishes, the driver goes empty and the task goes back to sleeping.

This is also why unchanged writes are no-ops rather than merely cheap. `set_text_content`
with identical content does not notify, so it does not schedule a frame. A handler that
recomputes and re-sets the same string thirty times a second keeps the app fully passive.

## Concurrency rules

The arena is a `DashMap`, which shards its locks per key. That makes concurrent access to
different nodes free, and creates exactly one hazard worth knowing:

> **Never dispatch an event while holding a `nodes.get_mut` guard.**

A handler is downstream code. It may touch the very node whose guard you hold, and it will
deadlock — not error, not block briefly, deadlock. The pattern throughout the codebase is
to scope the borrow and clone out what the event needs:

```rust
let (handled, value) = {
    let Some(mut data) = self.inner.nodes.get_mut(&node) else { return Ok(false) };
    // ... mutate ...
    (handled, changed.then(|| state.content.clone()))
};                                  // guard released here

if let Some(value) = value {
    self.dispatch_input_to(node, &mut InputEvent::new(value));
}
```

`document/input.rs`'s `apply_input_default_action_to` is the worked example.

The rest of the runtime state sits behind `Mutex` and `RwLock` on `DocumentInner`, taken
through helpers in `lock.rs` that tolerate poisoning — a panicking handler must not turn
a lock into a permanent failure for the whole application.

## Default actions run last

The engine performs some actions itself: typing edits a focused Input, Tab moves focus,
the wheel scrolls the nearest scrollable ancestor, a left press starts a selection or
grabs a scrollbar.

These are **default actions**, and they run *after* listeners:

```rust
self.dispatch_key_press_to(target, &mut event);
if !event.default_prevented() && !self.apply_input_default_action(event.code) {
```

That ordering is what makes `prevent_default()` possible — a listener gets to veto before
the engine acts. It has one consequence that surprises people: a handler reading state the
default action is about to change sees the *old* value. This is exactly why engine-driven
state changes get their own events (`on_input`, `on_scroll`, `on_focus`) rather than
expecting you to infer them from the input that caused them.

## Errors and panics

Tree operations that cannot be satisfied return `TuidomError` — a cycle, a missing node, a
removed parent. They never panic and never partially mutate: a rejected operation leaves
the tree exactly as it was.

Handler panics are caught. A panicking listener is logged and the application survives,
because one bad callback in a downstream component should not take down a running program.
The panic *hook* that restores the terminal is deliberately skipped for these — restoring
the terminal for an application that is still running would be the actual bug.

## Where to go next

- [Getting started](getting-started.md) — build something with all of the above
- [`GLOSSARY.md`](GLOSSARY.md) — the vocabulary, organized by area
- `examples/demo.rs` — most of the engine's surface exercised in one file
