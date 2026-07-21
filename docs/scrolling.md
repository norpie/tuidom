# Scrolling

Scrolling in tuidom is deliberately cheap: a wheel tick never re-runs layout. Everything
below follows from that one decision.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#layout--positioning).

## Making something scroll

Overflow is per axis:

```rust
use tuidom::style::{Overflow, Style};

let mut pane = Style::new();
pane.overflow_y(Overflow::Scroll);      // clips and scrolls vertically
pane.overflow_x(Overflow::Clip);        // clips horizontally, no scrolling
```

- `Visible` (default) — content spills out and stays visible
- `Scroll` — content is clipped to the box, and the axis scrolls
- `Clip` — clipped with no scrolling and no scrollbar

`overflow(v)` sets both axes at once.

### Overflow does something to *layout*, not just painting

This is the part that is easy to miss. A `Scroll` or `Clip` axis **drops the automatic
content-size floor** for that axis.

Normally a flex container is at least as large as its content — that floor is what stops
boxes collapsing. But a container that cannot be smaller than its content can never
overflow, and a container that never overflows can never scroll. Dropping the floor is what
makes overflow possible in the first place.

The practical consequence: setting `overflow_y(Scroll)` on a container whose height is
`Auto` usually does nothing visible, because the container still sizes to its content.
Scroll containers need a bounded height — a fixed `Length::Cells`, a `Percent`, or
`flex_grow` inside a bounded parent.

## Scroll offset is runtime state

A **scroll offset** is not style. Like focus, it lives on the document keyed by `NodeId`:

```rust
let offset = doc.scroll_offset(node);    // ScrollOffset { x, y }

doc.scroll_to(node, 0, 40)?;             // absolute, in cells
doc.scroll_by(node, 0, -3)?;             // relative
```

Offsets are clamped to content minus viewport, measured by the same layout pass that
produced the rectangles. When a relayout shrinks the content, stored offsets are **re-clamped**
— so a list that loses half its rows does not stay scrolled into empty space.

The consequence of it being runtime state, and the reason it matters more than it looks:
**removing a node and recreating an identical one loses its scroll position.** Anything
built on tuidom must patch live nodes rather than rebuild subtrees. See
[architecture](architecture.md#where-state-lives).

## Layout is scroll-invariant

Scrolling never re-runs taffy. The offset is applied at **paint** time, as a translation of
the container's descendants.

That is why a wheel tick is nearly free, and why scroll offsets can be clamped against a
layout pass that ran before the scroll happened — the geometry has not moved, only the
window onto it.

## The scrollport

The **scrollport** is the region descendants are bounded to: the node's *padding box*, which
is its rectangle deflated by any border.

Content therefore slides through the padding but never over the border — which is what you
want, since a border is a frame and content scrolling across it would look like a rendering
bug.

Clipping is **per axis**. An axis left `Visible` stays unbounded, so a pane that scrolls
vertically but lets content spill horizontally behaves exactly that way rather than clipping
both.

```rust
if let Some(view) = doc.get_node(node) {
    if let Some(layout) = view.layout {
        layout.scrollport;      // the clip region
        layout.max_scroll_y;    // how far it can scroll
    }
}
```

## Wheel routing and chaining

A wheel event does not go to whatever the mouse is over. Routing walks from the hit node
**rootward** to the nearest container that is scrollable on the wheel's axis *and can still
move in the wheel's direction*.

That last clause is **scroll chaining**. A container already at the end of its range passes
the wheel to the ancestor beyond it, so scrolling to the bottom of an inner list continues
into the page behind it instead of dead-ending.

Inert and disabled containers are skipped, the same way they swallow the wheel event itself.

To take the wheel yourself:

```rust
doc.on_wheel(slider, |event| {
    adjust(event.delta);
    event.prevent_default();     // suppress the default scroll entirely
})?;
```

Horizontal wheel input arrives two ways — `ScrollLeft`/`ScrollRight` from terminals that
send it natively, and shift+vertical from terminals using the older convention. Both are
normalized to `WheelAxis::Horizontal` and routed to `overflow_x` containers.

## Culling

Painting drops any node whose translated rectangle lies entirely outside its clip. Where a
subtree's clip is empty, the walk stops rather than descending.

Culled nodes are **still in the DOM and still laid out** — culling is paint-only. That is
what makes it exact for variably sized items: layout has already positioned everything, so
there is no estimation involved and nothing can be wrong about which nodes were skipped.

Culling is not [virtualization](virtualization.md), which decides what exists in the DOM at
all. Culling makes ten thousand rows cheap to paint; it does not make them cheap to lay out.

## Scrollbars

Bars are **overlays**. They draw on the viewport's last column and row and occupy no
layout, so showing or hiding one never reflows content.

```rust
use tuidom::style::{ScrollbarCharset, ScrollbarShow};

let mut s = Style::new();
s.scrollbar_show(ScrollbarShow::WhenScrolling);
s.scrollbar_charset(ScrollbarCharset::half_block());
s.scrollbar_thumb_color(Color::oklch(0.7, 0.02, 260.0));
s.scrollbar_track_color(Color::oklch(0.3, 0.01, 260.0));
```

They are drawn *after* the container's subtree, so a bar covers the content it scrolls but
stays under anything that paints over the container later — a modal above a scrolling pane
covers its scrollbar, as it should.

Four show modes, all gated on the axis actually being scrollable:

| Mode | Behavior |
|---|---|
| `Always` | whenever the axis can scroll (default) |
| `WhenFocused` | while the container or a descendant holds focus — and since hover is focus, also while hovered |
| `WhenScrolling` | appears on offset change, then hides — see below |
| `Never` | no bar |

`ScrollbarCharset::block()` is the full-block default; `half_block()` gives a thinner look.
The charset is the primitive — supply your own four characters if you want.

### `WhenScrolling`

The auto-hiding mode, and the one with real machinery behind it:

```rust
s.scrollbar_show(ScrollbarShow::WhenScrolling);
s.scrollbar_hide_delay(Duration::from_millis(800));
s.scrollbar_fade_duration(Duration::from_millis(300));
```

The bar appears when the offset actually changes, stays while grabbed, then holds fully
visible for `scrollbar_hide_delay` before fading out over `scrollbar_fade_duration` by
ramping alpha into its colors.

Two properties worth knowing. Visibility is a **pure function of the document clock**
against per-container last-activity instants — which is why headless tests can time-travel
and render any phase of the fade deterministically. And the fade is **scheduled, not
polled**: a waiting bar sets one deadline wake at fade start, only the fade itself ticks
smoothly, and a fully faded bar leaves the document as passive as if it never existed.

### Dragging a bar

A left press on a bar **grabs it** as the mouse-down default action — in place of starting
a selection, and in place of the click.

- pressing the **thumb** drags it from where it is
- pressing the **track** jumps the thumb under the cursor and keeps dragging

Either way the press continues as a drag, and each move re-reads geometry from live layout
through the inverse of the thumb math, so it stays exact at both ends of the range rather
than drifting.

Hit-testing a bar resolves to its **container**, not to a phantom node — `node_at` over a
scrollbar gives you the thing that scrolls.

To keep a press on a bar as an ordinary container press:

```rust
doc.on_mouse_down(pane, |event| event.prevent_default())?;
```

## Observing scrolls

```rust
doc.on_scroll(pane, |event| {
    // fires on any offset change: wheel, scroll_to, scroll_by, or a bar drag
})?;
```

`on_scroll` is **target-only** — it does not bubble, matching the DOM. Nested scroll
containers are common, and a bubbling scroll event would fire an outer handler every time
an inner container moved.

## Where to go next

- [Virtualization](virtualization.md) — when culling is not enough
- [Layout](layout.md) — sizing, and why a scroll container needs a bounded one
- [Events](events.md#default-actions-run-after-listeners) — what `prevent_default` covers
