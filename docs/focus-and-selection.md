# Focus and selection

Two systems that both answer "what is the user pointing at" — one for keyboard interaction,
one for text. They share a page because they share a constraint: both are scoped by
containers you mark, and both keep working correctly when content scrolls under them.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#focus--selection).

## Focus basics

Focus is `Option<NodeId>` — nothing is focused by default, and nothing needs to be.

```rust
doc.set_focusable(button, true)?;    // Box defaults to false; Input defaults to true

doc.focus(button)?;
doc.blur();                          // focus → None
let current = doc.focused();         // Option<NodeId>
```

`focus()` fails if the node is not in the active focus context — you cannot focus something
behind a modal, and the error says so rather than silently doing nothing.

### Hover is focus

Mousing over a focusable node focuses it. There is no separate hover state, no hover event,
and no hover style: the focus style *is* the hover style.

A terminal has one pointer and one focused thing. Keyboard and mouse users need the same
affordance, and two nearly-identical visual states that can both be active at once buys
nothing but ambiguity about which wins.

### Keyboard navigation

Tab and Shift-Tab move through focusable nodes in **DOM order**. Arrow keys move
**spatially** — to the nearest focusable node in that direction on screen, measured
edge-to-edge rather than by DOM position.

Spatial navigation does not wrap. If there is nothing in that direction, nothing happens;
downstream can implement wrapping in a key handler if it wants it.

When ties happen — two candidates the same distance away — they break on cross-axis center
distance first, then paint order. Center distance is what makes the nearest *aligned* node
win, rather than an arbitrary one that happens to share an edge.

Escape has two behaviors, in order: the first press blurs the focused node, and a second
press with nothing focused propagates to your handlers. That is what lets Escape mean "back
out of this field" and then "close this dialog" without either meaning being hard-coded.

Tab with nothing focused focuses the first focusable node, so the cycle is re-enterable
after an Escape.

All of these keys are configurable:

```rust
use tuidom::event::{FocusKeys, KeyCode};

let mut keys = FocusKeys::default();   // Tab / BackTab, arrows, Esc to blur
keys.up.push(KeyCode::Char('k'));
keys.down.push(KeyCode::Char('j'));
doc.set_focus_keys(keys);
```

## Focus contexts

A **focus context** is a subtree that traps focus. It is what a modal is built from:

```rust
let mut modal_style = Style::new();
modal_style.stacking_context(true);        // required
doc.set_style(modal_layer, &modal_style)?;

doc.push_focus_context(modal_layer)?;      // opens the trap
// ... later
doc.pop_focus_context()?;                  // closes it, restoring focus
```

Pushing a context **auto-focuses the first focusable node inside it** and remembers the
focus it interrupted. A modal that opens with nothing focused would leave the keyboard user
stranded — they would have to Tab in blind — so the engine does it rather than making every
caller remember to.

While a context is active it scopes *everything* about focus. `focused()` reports the
focused node within it, tab order and spatial navigation search only inside it, and
everything outside is **inert**.

The document root is a permanent focus context, so with nothing pushed the whole tree is in
scope. There is no special "no modal open" case in the engine.

### Why a stacking context is required

`push_focus_context` only accepts a node marked `stacking_context: true`. This is not
bookkeeping — it is a correctness rule.

Paint order treats every node's subtree as an atomic unit, so a sibling subtree can paint
*over* an unmarked node. Trapping focus inside a subtree that something else covers would
leave the user typing into a thing they cannot see. Requiring the marker makes that
unrepresentable.

Being a stacking context never traps focus on its own. It only makes a node *eligible*.

### The focus stack

Contexts form a stack, innermost last, with the root context at the bottom. **Each level
remembers its own focused node**, so restoring focus when a modal closes is just a pop
rather than bookkeeping you maintain:

```rust
doc.push_focus_context(dialog)?;       // remembers what was focused underneath
doc.push_focus_context(confirm)?;      // nested modal
doc.pop_focus_context()?;              // back in `dialog`, on the node you left
doc.pop_focus_context()?;              // back underneath, on the node you left
```

If a remembered node no longer exists, is no longer focusable, or has since been disabled,
focus is left **cleared** rather than moved somewhere else. Jumping to a node the user never
selected is worse than an unfocused moment they can resolve with Tab.

Removing an open context's node closes the context, so focus can never be trapped inside a
subtree that no longer exists.

Useful when you are building on this:

```rust
doc.active_focus_context();      // NodeId of the innermost open context
doc.focus_context_depth();       // how many are open
doc.is_inert(node)?;             // is this node outside the active context
```

## Inert versus disabled

Both block interaction. They are not the same thing, and the difference is deliberate:

| | Inert | Disabled |
|---|---|---|
| Cause | outside the active focus context | `set_disabled`, on the node or an ancestor |
| Focusable | no | no |
| Skipped by tab and spatial nav | yes | yes |
| Swallows targeted events | yes | yes |
| **Merges a style** | **no** | **yes** |

That last row is the whole point. Content behind a modal is inert but keeps its normal
appearance — dimming it is a decision for the modal's own scrim, not something the engine
imposes. A disabled control, on the other hand, *should* look disabled, so it merges its
disabled style.

Focus and blur events are exempt from the swallow in both cases, since they report a focus
change the engine has already made. See [events](events.md#blocked-nodes-swallow-events).

## Text selection

Selection is screen-wide, not per-widget: dragging across a Text node selects its content,
the same way a terminal or a browser does.

```rust
doc.get_selection();     // Option<String>, in reading order
doc.selection();         // Option<(SelectionPoint, SelectionPoint)>
doc.clear_selection();
```

There are **no built-in clipboard keybinds**. Bind one yourself:

```rust
let d = doc.clone();
doc.on_key_press(doc.root(), move |key| {
    if key.code == KeyCode::Char('c') {
        if let Some(text) = d.get_selection() {
            clipboard.set(text);
        }
    }
})?;
```

The engine has no business deciding that Ctrl+C means copy rather than interrupt, and no
way to know which clipboard crate you use.

### Selection boundaries

A **selection boundary** confines a drag:

```rust
let mut sidebar = Style::new();
sidebar.selection_boundary(true);
```

The rule is that a drag belongs to the boundary of its **starting point** — the nearest
marked ancestor-or-self, or the root if nothing is marked — for the whole gesture. Dragging
out of it does not escape: the focus point snaps to the nearest text still inside the
original boundary.

This is what stops a drag beginning in a sidebar from swallowing the main content when the
cursor wanders. Within one boundary the selected range follows document order, so two
unmarked columns select browser-style — the tail of one plus the head of the other.

An Input is an **implicit boundary**. Dragging inside one drives that input's own selection
rather than the document's, and a click positions its cursor.

### `SelectionPoint` is content-addressed

A **SelectionPoint** is a Text node plus a byte offset on a grapheme boundary — not a screen
coordinate.

That is what makes selection survive scrolling. The point identifies a position in *content*,
so scrolling cannot move or invalidate it; rendering re-maps it through the current layout
every frame. Points are pruned when nodes are removed and clamped when text content changes.

What you get back from `selection()` is normalized: the anchor/focus pair in document order,
with the end extended past the glyph under it. That extension is why both endpoint cells of a
drag are included — the way terminals select, rather than leaving the last character out.

### Selection colors

```rust
s.selection_bg(Color::oklch(0.5, 0.1, 260.0));
s.selection_fg(Color::white());
```

Both inherit like other colors. **Unset means reverse video** — each selected cell swaps its
own foreground and background. That is the default because it is visible on any theme with
zero configuration, including themes the author never saw.

A drag starting on a non-text cell snaps to the nearest character within the boundary rather
than selecting nothing. A left press clears the existing selection and arms a new drag; if
you want a press that keeps the selection, `prevent_default()` on the mouse down:

```rust
doc.on_mouse_down(node, |event| {
    event.prevent_default();     // no selection cleared, no drag armed
})?;
```

Selection changes are observable, and fire only on actual change:

```rust
doc.on_selection_change(|event| {
    // event.selection: Option<(SelectionPoint, SelectionPoint)>
});
```

## Where to go next

- [Events](events.md) — dispatch, default actions, and what `prevent_default` covers
- [Styling](styling.md#pseudo-states) — focus, active, and disabled styles
