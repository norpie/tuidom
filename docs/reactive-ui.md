# Reactive UI

`tuidom-ui` is a framework on top of the engine: signals for state, a `view!` template for
building node trees from them, and components for reusing those trees. Terms in **bold** are
defined in the [glossary](GLOSSARY.md#reactive-ui).

The engine stays unaware of all of it. Everything here ends as ordinary engine calls —
`create_box`, `update_style`, `set_text_content`, listeners — so the engine guides still
describe what is on screen. This guide covers what decides which calls happen and when.

## Two layers

The crate is two layers, and only the second knows a `Document` exists.

The **signal runtime** is a reactive graph: values you write, computations that read them,
and side effects that rerun when what they read changes. It is usable and testable with no
document at all.

The **view layer** binds that runtime to engine nodes. A **View** is a description of a
subtree; mounting it creates the nodes and turns each signal read inside it into a
subscription. When a signal changes, only the bindings that read it rerun: one text node's
content, one node's style. Nothing is re-rendered wholesale, and there is no virtual DOM to
diff.

## Signals

A **Signal** holds a value. `state` creates one:

```rust
use tuidom_ui::state;

let count = state(0);
count.set(1);
count.update(|n| *n += 1);
assert_eq!(count.get(), 2);
```

The handle is an index into the runtime's arena, `Copy` and `Send + Sync`, so a closure
captures it with `move` and nothing else. There is no `Arc::clone` before each closure.

`set` does nothing when the new value equals the old one, so writing the same value twice
reruns nothing. `update` mutates in place and always counts as a change, since it never
clones the old value to compare.

`get` clones. For a large value, `with` reads through a projection instead:

```rust
let total = tasks.with(|tasks| tasks.len());
```

The projection runs while the runtime lock is held, so it must not read or write another
signal; doing so deadlocks. Read what you need first, then project.

Handles are never reused. A handle to a signal that has been dropped with its scope is inert:
a write does nothing and a read falls back to the default, with a warning. `try_get` and
`try_with` return `None` instead, for code that expects the signal may be gone.

## Derived values and effects

A **Derived** is a memoized computation over signals:

```rust
use tuidom_ui::{derived, state};

let tasks = state(vec![3, 5, 8]);
let total = derived(move || tasks.with(|t| t.iter().sum::<i32>()));
```

Dependencies are found by reading, not declared, and collected again on every run, so a
derived that stops reading a signal behind a branch stops depending on it. It is lazy: a
change marks it stale, and it recomputes on the next read. Anything reading it depends on the
derived itself, so when its inputs change but its value does not, nothing downstream reruns.
It is read like a signal, with `get` and `with`, but has no setter. `derived!(expr)` is
shorthand for `derived(move || expr)`.

An **Effect** is a side effect with tracked dependencies. It runs once immediately and again
whenever something it read changes:

```rust
use tuidom_ui::effect;

effect(move || tracing::info!("{} tasks", tasks.with(Vec::len)));
```

Most code never calls `effect` directly; every reactive binding in a view is one. An effect
that writes a signal it reads reschedules itself. The runtime stops such a loop after a fixed
number of passes and logs it, but the effect is still wrong.

### Batching

Each write flushes its effects before returning. `batch` holds the flush until the outermost
batch closes, so a group of writes reruns each affected effect once:

```rust
use tuidom_ui::batch;

batch(|| {
    first.set(a);
    last.set(b);
});
```

Listeners are not batched for you. The screen only paints after a listener returns, so it
never shows a half-done group of writes either way; `batch` saves the effect runs between
them.

## Scopes own everything

A **Scope** owns the signals, deriveds, effects, cleanups and tasks created while it is
entered. Disposing it disposes its child scopes first, runs its cleanups in reverse order, and
aborts its tasks.

You rarely open one. Mounting a view opens a scope, every component body and every `if`,
`match` and `each` branch gets its own, and the free functions `state`, `effect`,
`on_cleanup` and `spawn` attach to whichever scope is current. That is why state created in a
component body disappears when the component unmounts, and why no unmount code needs writing:

```rust
use tuidom_ui::{on_cleanup, spawn};

spawn(async move {
    let data = fetch().await;
    resource.set(Some(data));
});
on_cleanup(move || tracing::info!("closed"));
```

A task from `spawn` is aborted when its scope ends, so an unmounted component stops writing
into state nothing shows. The framework ships no resource type; a task and a signal holding
"loading" or the result are the whole pattern.

A write from the event task, inside a listener or during a mount, flushes in the same frame.
A write from a spawned task or another thread cannot run effects there, so it is handed to the
event task and reaches the screen on the next flush. `App` wires that hand-off; see
[running an app](#running-an-app).

## Templates

`view!` is a Rust-like template that lowers to the view layer's builder calls:

```rust
use tuidom_ui::view;

let ui = view! {
    box {
        focusable: true,
        style: frame.get(),
        on_key_press: move |ev| {
            if ev.code == KeyCode::Char('+') {
                count.update(|n| *n += 1);
            }
        },
        text("count: "),
        text(count.get()),
    }
};
```

Inside braces sit attributes (`key: value`) and children, separated by commas. The elements
are `box` and `input`; `text(..)` is a text node, `frames(..)` cycles text for a spinner, and
`{ expr }` drops in anything that converts to a `View`. `fragment` turns several views into
one with no wrapper node. The macro's rustdoc has the full grammar.

What a value does depends on how it is written. A literal is applied once. Any other
expression becomes its own effect, so it reapplies whenever a signal it read changes. Above,
`text("count: ")` is fixed and `text(count.get())` follows `count`. Style keys always bind
reactively, and event attributes take their closure as the handler. A text node inherits
nothing from its box, so color or bold for text goes on the text: `text(..) { style: s }`.

An attribute key the macro does not know becomes a string attribute on the node, which is
how tests tag nodes (`test_id: "save"`). A misspelled typed key lands there too, silently;
only `on_*` names are checked.

## Structure that changes

Bindings handle values. Three forms let the shape of the tree change too.

`if` and `match` each lower to a **Dynamic Block**: when a signal read by the condition or
scrutinee changes, the block unmounts its content and mounts the new branch in place.

```rust
view! {
    box {
        match tab.get() {
            0 => { tasks_pane() },
            _ => { details_pane() },
        }
    }
}
```

Only reads made while choosing and building the branch are its dependencies. A binding
inside the branch that reads a signal reruns on its own without rebuilding the branch.

Rebuilding has a cost beyond the nodes. Scroll position, focus, selection and running
transitions belong to node identity in the engine ([where state lives](architecture.md#where-state-lives)),
so a branch that is rebuilt loses them. Keep conditions about what is shown, not about
values the branch could bind instead.

`each` lowers to a **Keyed List**, which diffs instead of rebuilding:

```rust
view! {
    box {
        each _, task in tasks.get() key = task.id {
            text(task.title.clone())
        },
    }
}
```

When the collection changes, an item whose key is still present keeps its nodes and is moved
into place; new keys mount and vanished keys unmount. An item's body is built once per key
and never rerun while the key survives, so what it shows only updates through signals it
reads. Above, renaming a task with the same `id` does not change its text; the item has to
read the title from a signal for that. Without `key = …` an `each` keys by position, which
reuses nodes on append but shows stale content when the value at an index changes.

## Components

A **Component** is a function that builds a subtree, marked `#[component]`:

```rust
use tuidom_ui::{component, state, view, View};

#[component]
fn Counter(start: i32, #[prop(default)] step: i32) -> View {
    let count = state(start.get());
    view! {
        box {
            focusable: true,
            on_click: move || count.update(|n| *n += step.get().max(1)),
            text(count.get()),
        }
    }
}

let ui = view! { Counter { start: 10 } };
```

A PascalCase name in a template instantiates a component; lowercase names are engine
elements. The body runs once, when the component mounts, and is never rerun. That is why
every **Prop** arrives as a signal: `start` is a `Signal<i32>` inside the body, and a prop
change reruns only the bindings that read it.

The call site decides how a prop is fed. A literal (`start: 10`) seeds it once; any other
expression (`start: base.get()`) keeps it synced to the caller's signals, which requires the
type to be `PartialEq`. Such an expression is evaluated again on every change, so it cannot move
a captured value out; build the value in the expression, often with a function call
(`style: frame()`), or clone it. The expression may also be a signal or derived holding the
prop's type (`start: base`), which the prop follows the same way. A prop is required unless `#[prop(default)]`, `#[prop(default = expr)]`
or an `Option<T>` type makes it optional, and leaving out a required prop is a compile error.

### Children and slots

A `children: Children` parameter receives the content written inside the instantiation, and
the body places it with `{ children }`:

```rust
#[component]
fn Card(title: String, children: Children) -> View {
    view! { box { text(title.get()), { children } } }
}

view! { Card { title: "Totals".to_string(), Counter { start: 0 } } }
```

**Children** is a deferred thunk, built where the body places it, so the content mounts under
the component's node and sees the context of the component placing it. It is `Copy`.

More than one piece of caller content needs more `Children` parameters, called slots. The
caller fills each with a view, and `fragment` puts several items in one slot:

```rust
view! {
    StatusBar {
        left: fragment([view! { text("a") }, view! { text("b") }]),
        right: view! { text("c") },
    }
}
```

### Callbacks

A child talks back through a **Callback** prop. The parent passes a closure and the child
calls it:

```rust
#[component]
fn Picker(on_pick: Callback<usize>) -> View {
    view! { box { focusable: true, on_click: move || on_pick.call(3) } }
}

view! { Picker { on_pick: move |n: usize| chosen.set(n) } }
```

A closure written at the call site becomes the callback, and an existing `Callback` passes
through as it is. When the compiler cannot infer the closure's parameter type, annotate it,
as above. `Callback<A, R>` can return a value, which lets a parent hand a child a builder:
a list prop `render: Callback<usize, View>` asks the parent for each row as it needs one.

A callback holds an `Arc`, so it is `Clone` but not `Copy`. A body calling it from several
closures clones it for each, or keeps it in a signal (`state(on_pick)`) that every closure can
copy. `#[prop(default)]` makes a callback optional with a no-op default.

### Bound signals

A parameter declared `Signal<T>` takes the caller's own signal rather than a copy synced from
it, so the component can write back. This is how an input field updates the string its caller
holds:

```rust
#[component]
fn Toggle(on: Signal<bool>) -> View {
    view! { box { focusable: true, on_click: move || on.update(|v| *v = !*v) } }
}

let dark = state(false);
view! { Toggle { on: dark } }
```

### Context

`provide_context` makes a value available to everything below the current scope, and
`use_context` finds the nearest one, keyed by type:

```rust
provide_context(Accent(Color::var("--primary")));
// in any descendant component's body
let accent = use_context::<Accent>();
```

A nearer provider shadows a farther one, and an absent context is `None`. Context is cloned
while the runtime lock is held, so its `Clone` must not touch a signal; a `Copy` signal handle
or an `Arc` is the usual payload.

## Reaching the document

A component body never sees the `NodeId`s its view creates. Operations that are imperative in
the engine — focusing, scrolling, starting an animation, reading layout — go through a
**NodeRef**:

```rust
use tuidom_ui::{node_ref, on_mount};

let field = node_ref();
on_mount(move || field.focus());
view! { input { ref: field, bind_value: name } }
```

The ref holds the node while that element is mounted and carries the document with it, since
listeners run outside any scope. Read inside an effect, `layout()` and `screen_rect()` rerun it
when the node's geometry changes. They report the frame that just finished, so anything drawn
from them lands a frame later. To place one node beside another, use
[anchored positioning](layout.md#anchored-positioning) instead, which the engine settles in
the same frame.

`on_mount` runs once, after the component's subtree is in the document; the body itself runs
before any of it exists. `use_document` gives a body the document for document-wide calls:
quitting, a root key handler, color variables.

`trap_focus: true` on an element traps focus inside it while it is mounted and returns focus to
where it was when it unmounts. The element's style has to make it a
[stacking context](rendering.md#stacking-contexts).

### Portals

`portal { … }` mounts its content as the last children of the document root, so it draws over
everything. A node's `z_index` cannot escape its parent's subtree, so a dropdown or modal has
to live under the root to cover what is around it. The content stays in the scope it was
declared in: it sees that scope's context and unmounts with it.

Color variables follow the node tree, not the scope, so variables declared on an ancestor of
the portal do not reach its content. Content that needs them declares them itself, which is
what the [components](components.md#popups-and-the-palette) do.

## Large collections

`each` materializes every item. For thousands of rows, `virtual_list` materializes only those
in view, through the same keyed differ, and holds open the rest of the scroll range with
spacers:

```rust
use tuidom_ui::{virtual_list, virtual_list_ref};

let list = virtual_list_ref();
let rows = virtual_list(frame(), 1, 2, move || count.get(), move |i| {
    view! { text(format!("row {i}")) }
})
.list_ref(list);
// later, from a key handler
list.scroll_into_view(selected.get());
```

The arguments are the container's style, each item's height, the extra items kept past each
edge, the count (read reactively), and a builder for item `i`. The builder runs when an index
enters the window, so changing data must reach an item through signals it reads. The style
must bound the container's height, or there is no viewport to window against.
`virtual_list_measured` takes an estimated height instead and measures each item once it is
laid out.

A **VirtualListRef** reaches a mounted list's window math. `scroll_into_view` scrolls the
least distance that shows an item whole. In a measured list, an item not yet laid out is
placed by its estimate, so the scroll can land a little off until the item has been seen.
The [virtualization guide](virtualization.md) covers the window and the measurement cache.

## Running an app

`App` owns the document, mounts the root view, and runs the engine loop:

```rust
use tuidom_ui::App;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| view! { Counter { start: 0 } })
        .on_quit(|| tracing::info!("bye"))
        .run()
        .await
}
```

The root closure runs once, like a component body, and everything it creates belongs to the
root scope, disposed together after the loop returns. `on_quit` runs before that disposal, so
it can still read the app's signals.

`App` also connects the runtime to the document, so writes from spawned tasks reach the
screen. `mount_on` does that wiring onto a document you provide, without the terminal loop,
and returns an `AppSession` that tears everything down when dropped. That is the piece tests
build on.

## Testing with Harness

`Harness` mounts a view on a headless document through `mount_on`, so a test runs the same
wiring a real app does. It needs the crate's `testing` feature:

```toml
[dev-dependencies]
tuidom-ui = { path = "../tuidom-ui", features = ["testing"] }
```

```rust
use tuidom_ui::testing::Harness;

let mut h = Harness::new(20, 3, || view! { Counter { start: 0 } });
h.click_text("0");
assert_eq!(h.row(0), "1");
```

Every action settles before and after: pending writes flush and frames render until two in a
row match, since a frame can schedule work that only shows on the next one. A virtualized list
learning its viewport is the usual case. So a write from the test body is visible on the next
read with nothing to call.

The screen is read with `row`, `screen` and `cell`. Nodes are found the way a user would, with
`find_text` and `click_text`, or by attribute with `find_attr` for a node tagged in the
template. The clock is frozen and moves only with `advance`, so animations can be checked
partway through. Timers inside spawned tasks use Tokio's clock instead, which
`#[tokio::test(start_paused = true)]` controls.

Unlike the rest of the crate, `Harness` panics on failure, naming the step that failed. It
only runs in tests, where that is a test failure. The headless runtime it drives is covered
in [testing](testing.md).

## Where to go next

- [Components](components.md) — the themed library built on all of this
- [Architecture](architecture.md#where-state-lives) — why node identity matters
- [Virtualization](virtualization.md) — the window math behind `virtual_list`
