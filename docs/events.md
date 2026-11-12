# Events

Events in tuidom follow the DOM's model closely enough that the differences are the
interesting part. There is no capture phase, `prevent_default` exists on exactly three
events, and several events exist purely to report things the *engine* changed behind your
back.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#events).

## Two kinds of event

**Targeted events** have a target node and bubble rootward through its ancestors:

```rust
doc.on_click(button, |event| { /* ... */ })?;
```

**Document-level events** have no target and do not bubble, because there is no node they
could sensibly belong to:

```rust
doc.on_resize(|event| { /* the terminal resized */ });
```

The distinction shows up in the signature: targeted registration takes a `NodeId` and
returns `Result<ListenerHandle>` (the node might not exist); document-level registration
takes only the handler and returns `ListenerHandle` directly.

| Targeted | Document-level |
|---|---|
| `on_key_press`, `on_focus`, `on_blur` | `on_resize` |
| `on_mouse_down`, `on_mouse_up`, `on_click`, `on_wheel` | `on_post_frame` |
| `on_scroll`, `on_input` | `on_selection_change` |
| `on_transition_end`, `on_animation_end`, `on_animation_iteration` | `on_window_focus`, `on_window_blur` |

## Propagation

There are two phases, not three:

```
Target phase  → listeners on the target node
Bubble phase  → listeners on each ancestor, rootward
```

No capture phase. It was left out because nothing in a terminal UI needed it and every
event carrying an unused phase is a phase every handler has to reason about.

Any handler can inspect where it is:

```rust
use tuidom::event::EventPhase;

doc.on_click(panel, |event| {
    let clicked = event.target();          // where the event originated
    let here = event.current_target();     // the node this listener is registered on
    if event.phase() == EventPhase::Bubble {
        // reached us from a descendant
    }
})?;
```

And stop it going further:

```rust
doc.on_click(button, |event| {
    event.stop_propagation();      // the panel behind will not see this click
})?;
```

### `on_scroll` does not bubble

One targeted event is target-only: `on_scroll`, matching the DOM. Nested scroll containers
are common, and a bubbling scroll event would fire an outer container's handler every time
an inner one moved — which is almost never what the outer handler meant.

## Default actions run *after* listeners

The engine performs some actions itself. Typing edits a focused Input, Tab moves focus, the
wheel scrolls the nearest scrollable ancestor, a left press starts a text selection or grabs
a scrollbar.

These are **default actions**, and dispatch order is:

```
1. listeners run (target, then bubble)
2. if nothing called prevent_default(), the engine acts
```

So a listener always gets to veto first:

```rust
doc.on_wheel(slider, |event| {
    adjust_by(event.delta);
    event.prevent_default();      // the page behind will not scroll
})?;
```

**`prevent_default` exists on exactly three events**, because those are the only three with
a document-level default action to suppress:

| Event | Default action suppressed |
|---|---|
| key press | the Input edit, selection extension, focus movement, and keyboard scrolling |
| wheel | scrolling the nearest scrollable ancestor |
| mouse down | starting a text selection, or grabbing a scrollbar |

Everywhere else, there is nothing to prevent, and an API offering it would be a lie.

### A key press has four defaults, in order

They are tried in sequence, and the first one that claims the key ends it:

```
1. the focused Input edits or moves its cursor
2. shift-extension of a document selection
3. focus movement (Tab, arrows, Esc)
4. scrolling (PageUp/PageDown, Home/End)
```

The order is what keeps the four from colliding. A focused multiline Input consumes Home,
End and the page keys itself, so a scroll container only ever sees them when the keyboard
is not in an Input. And scrolling runs last, so if a rebinding makes the focus and scroll
sets overlap, the established focus binding still wins.

A single-line Input is the visible edge of this: it has no line to move to, so it declines
Up and Down and they reach focus navigation instead of being swallowed.

### Shortcuts and typing

Listeners run before the Input edit, so a key typed into an Input bubbles to every
ancestor first, the root included. A document-level `q`-to-quit shortcut would fire while
someone types "quit" into a field. `edits_input()` on the key event says whether the
focused Input is going to take the key, and a shortcut skips it when it is:

```rust
doc.on_key_press(doc.root(), move |key| {
    if key.edits_input() {
        return;
    }
    match key.code {
        KeyCode::Char('q') => quit.quit(),
        _ => {}
    }
})?;
```

It is decided by running the Input's own rules against a copy of its state, so it is
`false` for everything the Input lets through: Esc, function keys, control and alt chords
other than ctrl+a, Enter in a single-line Input, and Up and Down there too. Checking
whether the target is an Input instead would also skip those, and a ctrl+s shortcut would
stop working whenever a field has focus.

### The consequence: handlers see stale state

Because listeners run first, a handler reading state that a default action is *about* to
change sees the old value. Typing into an Input and reading `input_value()` from
`on_key_press` gives you the value from before the keystroke.

This is not a bug to work around — it is why the engine reports its own changes with
dedicated events instead of expecting you to infer them.

## Events that report what the engine did

These events exist for exactly this reason. Each fires *after* a change the engine already
made:

**`on_input`** — an Input's value changed. Carries the new value, targets the input, and
bubbles, so one listener on a form-shaped container observes every field inside it:

```rust
doc.on_input(form, move |event| {
    println!("{:?} is now {:?}", event.target(), event.value);
})?;
```

Only real changes fire it. A keystroke that edits nothing, cursor movement, and selection
changes all stay silent. So do programmatic writes through `set_input_value` — the caller
already knows what it wrote, and reporting it back would loop a two-way binding through its
own listener.

It has no `prevent_default`, because it reports a change already made. The place to suppress
the change is `prevent_default()` on the key press.

**`on_scroll`** — a container's scroll offset changed, however it changed: wheel, `scroll_to`,
`scroll_by`, or a scrollbar drag.

**`on_size_change`** — a node's content box, inside border and padding, changed size after
layout:

```rust
doc.on_size_change(chart, |event| {
    println!("{} × {}", event.width, event.height);
})?;
```

It also fires once on the first layout after a listener is added, so the listener learns the
starting size without asking. That first report goes to every listener on the node, so adding
one repeats the current size to the others. A node out of layout, hidden or not yet attached,
reports zero by zero. A move alone fires nothing, since scrolling would otherwise fire it on
every wheel tick. It does not bubble. A [canvas](rendering.md#canvas) uses it to draw at the
size it is given.

Unlike the other events here, it arrives before the frame that caused it is drawn. Layout
runs on the render task, which never runs user code, so the frame releases the gate, waits for
the event task to run the listeners, and lays out again before painting. A canvas that redraws
for its new size is drawn at that size in the same frame, with no frame showing the old
drawing cropped. Two limits keep a listener from holding up the screen: the frame repeats this
at most four times, so a listener that keeps resizing its own node is cut off, and it waits
only 50 ms for the listeners to finish. Changes past either limit show a frame later.

**`on_focus` / `on_blur`** — focus moved. These carry a `relation`, distinguishing "I am the
node that gained focus" from "a descendant of mine did":

```rust
use tuidom::event::FocusEventRelation;

doc.on_focus(panel, |event| {
    if event.relation() == FocusEventRelation::SelfNode {
        // the panel itself
    }
})?;
```

**`on_transition_end` / `on_animation_end` / `on_animation_iteration`** — an animation
reached a boundary. See [animation](animation.md).

## Blocked nodes swallow events

A node that is [disabled](styling.md#disabled) or **inert** does not bubble targeted events
to its ancestors — it swallows them.

This matches HTML's disabled controls, and the alternative is worse: if a disabled button
passed its clicks up to the panel containing it, disabling a control would silently reroute
its interactions instead of stopping them.

**Five events are exempt** from the swallow:

```
Focus, Blur, TransitionEnd, AnimationEnd, AnimationIteration
```

They report a change the engine has *already* made. A node losing focus is frequently
losing it *because* it just became disabled, and a transition finishes regardless of either
state. Swallowing those would hide the change from the handler that exists to observe it.

A press swallowed by an inert node is not lost entirely: the active focus context hears it
through `on_press_outside`. See [a press outside the context](focus-and-selection.md#a-press-outside-the-context).

## Listener handles

Registration returns a **ListenerHandle**, which is how you remove one:

```rust
let handle = doc.on_click(button, |_| {})?;
doc.remove_listener(handle);      // → bool: whether it was still registered
```

Handles carry a document-scoped id, so a handle from one document cannot accidentally
remove a listener in another.

Handlers are `Fn + Send + Sync + 'static`. They cannot borrow, so capture clones — the
`let d = doc.clone();` before a `move` closure is the standard shape. They are also
**synchronous**: if you need async work, spawn it.

```rust
let d = doc.clone();
doc.on_click(button, move |_| {
    let d = d.clone();
    tokio::spawn(async move {
        let data = fetch().await;
        let _ = d.set_text_content(label, data);
    });
})?;
```

A panicking handler is caught and logged, and the application survives. One bad callback
in a downstream component should not take down a running program.

### Listeners are snapshotted before dispatch

Dispatch clones the listeners for the whole propagation path *before* invoking any of them,
and releases the registry lock first.

Two things follow. **Registering or removing a listener from inside a handler does not
affect the dispatch in progress** — the change takes effect from the next event, so a
handler that removes itself still finishes, and one that adds a listener does not have it
fire for the event that added it. And because the lock is released first, a handler is free
to register listeners without deadlocking against the dispatch that called it.

## Keyboard

```rust
use tuidom::event::KeyCode;

doc.on_key_press(doc.root(), move |key| {
    match key.code {
        KeyCode::Char('q') => d.quit(),
        KeyCode::Esc => { /* ... */ }
        _ => {}
    }
})?;
```

Key events target the **focused** node and bubble, so a listener on the root sees anything
nothing else consumed. When nothing is focused, keys dispatch from the active focus context
instead — which is what lets a modal's own Escape handler fire with no focused node inside
it.

There is **no key-down or key-up event, and no repeat**, and there will not be. Key release
requires the kitty keyboard protocol, which most terminals do not implement. An API that
silently never fires on most terminals is worse than an absent one, so `Release` and
`Repeat` are dropped at the source.

Which keys move focus is configurable. A binding is a key *and* the modifiers that must be
held exactly, so Tab and Ctrl+Tab are separate bindings and one never fires for the other:

```rust
use tuidom::event::{FocusKeys, KeyModifiers};

let mut keys = FocusKeys::default();      // Tab/Shift-Tab, arrows, Esc to blur
keys.up.push((KeyCode::Char('k'), KeyModifiers::empty()));
keys.down.push((KeyCode::Char('j'), KeyModifiers::empty()));
doc.set_focus_keys(keys);
```

## Modifiers

Key and mouse events carry the modifiers held at the time: `SHIFT`, `CONTROL`, `ALT`.

```rust
use tuidom::event::KeyModifiers;

doc.on_key_press(doc.root(), move |key| {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        d.quit();
    }
})?;
```

That example is the *only* way to handle Ctrl+C. Raw mode clears `ISIG`, so Ctrl+C raises
no SIGINT and no signal handler will ever see it — it arrives as an ordinary key press.

Two properties of terminal input make modifiers behave unlike a naive reading suggests:

**A control chord keeps its plain letter.** Ctrl+A arrives as `Char('a')` with `CONTROL`
held — not as a control character. The code alone cannot distinguish it from a typed `a`,
which is why text entry checks the modifiers before inserting, and why a chord that no
default action claims is refused rather than typed into a focused input.

**Shift is set on every capital.** Terminals report `A` as `Char('A')` *plus* `SHIFT`, so
shift being held does not mean a chord was pressed. Only control and alt do.

Super, hyper, and meta are dropped at conversion. Terminals report them solely under the
kitty keyboard protocol, so a binding on one would look correct and match on almost no
terminal — the same reasoning that keeps key release out.

## Mouse

`on_mouse_down`, `on_mouse_up`, and `on_click` all carry `x`, `y`, `button`, and
`modifiers`. Pointer movement reaches listeners only as a [drag](#dragging). Wheel events carry `delta`, an `axis`, and `modifiers`:

```rust
doc.on_wheel(node, |event| {
    // event.axis is Vertical or Horizontal, event.delta is signed
})?;
```

A click is synthesized when a press and a release of the same button land on the same node,
on any of its cells, unless the press selected text. A wobble while clicking selectable text
is therefore a selection; marking a control's text
[not selectable](focus-and-selection.md#text-that-cannot-be-selected) keeps it a click. The
click reports the modifiers held at **press** time. Letting go of ctrl before the mouse button
still reads as a ctrl+click.

Horizontal wheel arrives two ways, and tuidom normalizes both: terminals that send
`ScrollLeft`/`ScrollRight` natively, and terminals that send a *vertical* scroll with the
shift modifier set, which is the older convention. Both become `WheelAxis::Horizontal`.

When shift is what selected the axis, it is spent there and not reported — otherwise
`modifiers.contains(SHIFT)` on a horizontal wheel would answer differently per terminal
for the same physical gesture. Control and alt pass through untouched, so Ctrl+Shift+wheel
arrives horizontal with `CONTROL` set.

Modifiers are reported, not acted on: a chorded wheel still scrolls and a ctrl+click still
starts a selection. Use `prevent_default()` to claim the gesture for something else.

### Dragging

A node that registers `on_drag` takes the presses that land on it or anything inside it, the
nearest such node winning. From the first pointer move with the button held it receives a
`DragEvent` per move and one more on release, each carrying the pointer's position, where the
press started, and a `phase` of `Move` or `End`:

```rust
doc.on_drag(divider, move |event| {
    let delta = event.x - event.origin_x;
    // resize by `delta`
})?;
```

The node keeps the drag wherever the pointer goes, off the node or off the row it started on,
until the release. A pointer dragged quickly leaves a one-cell divider at once, so events that
followed whatever is under the pointer would drop the drag. Drag events therefore go only to that
node and do not bubble.

Taking the press changes what the rest of the press does. It neither clears nor starts a text
selection, since dragging a divider is not selecting the text it crosses, and a press that moved
is not a click, even if it ends on the cell it started on. A press that never moves produces no
drag events and clicks as usual. Mouse down and mouse up still fire. A press on a scrollbar grabs
the scrollbar instead, and a disabled or inert node takes no drag.

**Hover is focus.** Mousing over a focusable node focuses it. There is no separate hover
event or hover state — see [styling](styling.md#hover-is-focus) for why.

## Post-frame

`on_post_frame` fires after every rendered frame, carrying that frame's metrics:

```rust
doc.on_post_frame(|event| {
    println!("{:.1} fps, {:?}", event.fps, event.metrics);
});
```

It is document-level, like resize. The render task never runs user code, so this event goes
through the runtime queue and its handler runs on the event task, ordered with input.

**Mutating the DOM from a post-frame handler schedules another frame**, whose own
post-frame event fires in turn. An unpaced handler holds the renderer permanently active —
which defeats tuidom's passivity, so pace your mutations and let it go idle.

## Window focus

`on_window_focus` and `on_window_blur` report whether the *terminal window* holds OS focus:

```rust
doc.on_window_focus(|_event| { /* user came back */ });
```

Despite the shared word this is not DOM focus. It names no node, and gaining or losing it
never moves the focused node or disturbs the focus stack — alt-tabbing away and back
returns the user to exactly the node they left.

Terminals that do not support focus reporting simply never send it. There is no capability
check to write; the events just stay silent.

## Terminal info

`on_terminal_info` fires once `run()` has asked the terminal what it is, and again when a
resize changes the terminal's pixel size. It carries the whole `TerminalInfo`, which
`doc.terminal()` returns from then on. Like resize it is document-level, and it goes through
the runtime queue, so the handler runs on the event task, ordered with input. See
[what the terminal is](rendering.md#what-the-terminal-is).

## Input coalescing

The event queue is unbounded and never applies backpressure. There is nobody to push back
on — a terminal will not stop sending — and dropping input would lose keystrokes silently.

Instead, when the event task is *already behind*, runs of redundant events in the queued
batch collapse to the latest. Only three kinds collapse:

- pointer movement, with the move right after it
- resize and terminal info, each with a later one of its kind across any run of the two. A
  resize queues a pixel-size update right behind it, so during a dragged resize the two
  alternate, and merging only neighbors would collapse nothing.

Everything else — keys, presses, releases, wheel ticks — carries information its successor
does not, and is never dropped or reordered.

Two properties are worth being precise about. **Adjacency to input is what makes this
safe**: hover-to-focus means the pointer's position decides which node a key press targets,
so merging movement *across* a key would deliver that key to the wrong node, and a key ends a
run of resizes the same way. And **nothing is
ever delayed to build a batch** — a batch of one is returned unchanged, and collapsing
happens only when work had already piled up.

## The bell

```rust
doc.bell();
```

`\x07`, emitted by the next flush rather than written when you call it. The render task owns
the output stream, and a byte written from another thread could land in the middle of an
escape sequence and corrupt it.

Ringing schedules a frame, so a bell still reaches the terminal when nothing on screen
changed. Several bells before that frame produce one — which is all a terminal can make of
them anyway. What a bell *does* is the terminal's choice: a sound, a visual flash, or
nothing at all.

## Panics and terminal restore

A process-wide panic hook restores the terminal before a crash reaches the user: it undoes
exactly the modes currently turned on, then chains to whatever hook was installed before it.

It is installed only when a real terminal is set up, and deliberately **not** used for
panics inside downstream callbacks — those are caught, logged, and survived, so restoring
the terminal for one would tear down an application that is still running.

There is no built-in Ctrl+C or signal handling. Use `tokio::signal` and call `doc.quit()`.

## Where to go next

- [Focus and selection](focus-and-selection.md) — focus contexts, inert, spatial navigation
- [Architecture](architecture.md) — the three tasks, and why handlers never run on the renderer
