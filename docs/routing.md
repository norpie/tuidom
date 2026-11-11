# Routing

`tuidom-router` keeps an app's frames mounted while the screen inside them changes: a sidebar
keeps its scroll offset, a running transition finishes, and the frame's body never runs again.
It builds on [`tuidom-ui`](reactive-ui.md) alone. Terms in **bold** are defined in the
[glossary](GLOSSARY.md#routing). `examples/mail.rs` in the crate is a small app built on it.

## The route is your enum

There is no path matcher, no route table and no router component. The **route** is an enum the
app defines, held in a signal, and a `match` over it is the routing:

```rust
#[derive(Clone, Copy, Default, PartialEq)]
enum Route {
    #[default]
    Inbox,
    Message(usize),
    Settings(Tab),
}
```

A variant's fields are its parameters, already typed, and the compiler checks that every
route is handled, so there is no "not found" screen to design. Strings like `/messages/4`
exist on the web because a URL bar does; a terminal app has none. If yours wants a command
palette that names screens, implement `FromStr for Route` and set the result.

Navigation is `route.set(next)`. The router never navigates on its own.

## Chains

A route shows a stack of frames, outermost first, around one screen. That stack is a
**chain**: its **layouts** are components taking `children`, and its **leaf** is the screen.

```rust
use tuidom_router::{Chain, routed};

fn chain(route: Route) -> Chain {
    let shell = || ShellProps::builder().build();
    match route {
        Route::Inbox => Chain::layout(shell).leaf(|| InboxProps::builder().build()),
        Route::Message(id) => {
            Chain::layout(shell).leaf(move || MessageProps::builder().id(state(id)).build())
        }
        Route::Settings(tab) => Chain::layout(shell)
            .layout(move || SettingsProps::builder().tab(state(tab)).build())
            .leaf(|| TabProps::builder().build()),
    }
}

let ui = routed(move || chain(route.get()));
```

Each entry is a closure that builds the component's props, through the builder `#[component]`
generates. A signal prop takes a signal, so a value is wrapped in `state(..)`. A component
without a `children` parameter cannot be a layout: `Chain::layout` will not accept its props.
`Chain::leaf(..)` alone is a chain with no layouts.

`routed` mounts the chain its closure returns and follows it as the signals that closure read
change. The props closures run when an entry mounts, untracked and under the entry's own
scope, so a read inside one does not make the router rerun.

Each entry mounts inside an **entry box** the router creates. It fills the slot it is placed
in, as a column, so a layout's `{ children }` gets the space the layout gives it.
`.style(..)` after an entry replaces that box's style.

## What navigation keeps

When the route changes, the new chain is compared with the mounted one position by position.
Two entries match when they are the same component, both layouts or both leaves, and both
trapping focus or neither. Everything before the first mismatch stays mounted:

- its props closure runs again, and the values are assigned into the mounted props through
  [`ComponentProps::assign`](reactive-ui.md#props-built-by-hand), so the component updates through its
  signals and its body does not run again;
- its entry box takes the new chain's style.

From the first mismatch on, the old entries are disposed, innermost first, and the new ones
mount in the slot of the last kept layout. So `Inbox` → `Message(4)` keeps the shell and swaps
the leaf, and `Message(4)` → `Message(5)` keeps both and assigns `id`. `[Shell, Settings, Tab]`
→ `[Shell, Tab]` remounts `Tab`: the comparison is by position, and moving `Tab` under the
shell's slot would mean a new parent anyway.

Assignment covers the component's own signal props. Callbacks, node refs, slots and a
`Signal<T>` bound to the caller keep what they mounted with, and an equal value writes nothing,
so nothing that reads it reruns.

## A chain or a `match`?

A layout body runs once, so a `match` inside a `view!` already keeps the layout around it:
only the matched block rebuilds. Use that when data *within* a screen decides what part of it
shows, such as a details pane for the selected row.

Use a chain when the *route* decides the stack of frames. Its advantage is that the stack is
data: unrelated routes can share a shell, or share it and add a layout, in patterns that nested
`match` blocks cannot express without duplicating the frames. And the order things are kept
and disposed in is defined, so it can be tested.

## Focus

`.trap_focus()` after an entry opens a [focus context](focus-and-selection.md#focus-contexts)
on its box while it is mounted, and makes the box a stacking context. Contexts follow the
chain: a remounted suffix closes its contexts innermost first, the same order they opened in,
so the router's stack and the engine's focus stack never disagree. Turning a trap on or off
changes the entry's identity and remounts it.

A modal is not a route. It is a component with an open flag and `trap_focus:`, mounted inside
whichever screen opens it.

## Retention

`.retain()` after the leaf keeps it mounted but hidden when navigation leaves it, instead of
disposing it. When the route returns to the same component in the same slot, the leaf is shown
again with its nodes, scroll offsets and focus memory as they were, and its props
are assigned from the returning chain. A retained leaf is disposed with the layout whose slot
holds it.

A hidden leaf keeps working state without getting in the way: a focus context inside it, its
own or a dialog's left open, [traps nothing while hidden](focus-and-selection.md#hidden-contexts-trap-nothing),
and a virtual list in it keeps its window rather than collapsing to an empty viewport.

Retention is opt-in because every retained screen stays in memory and in the tree for as long
as its layout does, and stays live: its effects keep running and its spawned tasks keep
going while it is hidden. A screen that polls or streams should pause on a signal it reads,
or not be retained.

Whether a leaf is retained is read from the chain when navigation leaves it, and the router
only ever builds the chain for the current route. So make retention a fixed property of a
route. If it depends on state, such as a setting that turns it off, a leaf already hidden
when the state changes stays hidden until the route returns to it, after which it is
disposed the next time it is left, or until its layout goes.

## Back

The router keeps no history. A history entry is a route value, so the app keeps its own:

```rust
#[derive(Clone, Copy)]
struct Nav {
    route: Signal<Route>,
    history: Signal<Vec<Route>>,
}

impl Nav {
    fn go(self, to: Route) {
        let from = self.route.get();
        if from != to {
            self.history.update(|h| h.push(from));
            self.route.set(to);
        }
    }

    fn back(self) {
        let mut previous = None;
        self.history.update(|h| previous = h.pop());
        if let Some(previous) = previous {
            self.route.set(previous);
        }
    }
}
```

Back is then an ordinary navigation. What users usually expect from it, coming back to a list
scrolled where they left it, is retention: retain the list's leaf, and back returns to it as
it was.

## Leaving a screen

Two different things get called a leave guard. Cleanup when a screen goes is
[`on_cleanup`](reactive-ui.md#scopes-own-everything) in its body, which runs when the router
disposes it. A prompt before leaving, such as "discard changes?", is app code, because only the
app navigates: store the requested route in a pending signal instead of setting it, show a
modal while the pending signal holds one, and have its confirm button set the route. Keeping
the route signal private behind a `go` that does this is what makes the prompt unskippable.

## Where to go next

- [Reactive UI](reactive-ui.md) — components, props and `children`, which chains are built from
- [Focus and selection](focus-and-selection.md#focus-contexts) — the focus stack chains open
  contexts on
- [Testing](reactive-ui.md#testing-with-harness) — `Harness`, which the router's own tests and
  the mail example's use
