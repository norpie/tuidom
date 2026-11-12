# Layout

Layout is flexbox, computed by [taffy](https://github.com/DioxusLabs/taffy), in units of
terminal cells. If you know CSS flexbox, you know the model — this guide covers the parts
that are different because the target is a terminal rather than a browser.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#layout--positioning).

## Lengths

Every length in tuidom is a whole terminal cell — columns on the horizontal axis, rows on
the vertical one. There is no smaller unit, so nothing here is fractional and nothing is
subpixel.

The three length modes:

| | |
|---|---|
| `Length::Cells(n)` | exactly `n` cells |
| `Length::Percent(p)` | `p` percent of the parent's content area |
| `Length::Auto` | size to content — for a flex container, to its children |

### Cells are not square

A terminal cell is roughly twice as tall as it is wide. This is the single adjustment that
trips up anyone coming from CSS, and it applies to every symmetric value you write:

```rust
s.padding(EdgeInsets::all(1));           // looks squashed — 1 row reads like 2 columns
s.padding(EdgeInsets::symmetric(2, 1));  // 2 horizontal, 1 vertical — visually even
```

`EdgeInsets::symmetric(horizontal, vertical)` takes them in that order.
`EdgeInsets::new(top, right, bottom, left)` is the explicit form, clockwise from the top,
matching CSS.

The same ratio is why [half-block edges](styling.md#half-block-edges) exist — they let a
fill end on a half cell so vertical and horizontal padding can actually balance.

## Flex containers

```rust
use tuidom::style::{AlignItems, FlexDirection, FlexGap, JustifyContent, Style};

let mut row = Style::new();
row.flex_direction(FlexDirection::Row);
row.justify_content(JustifyContent::SpaceBetween);   // main axis
row.align_items(AlignItems::Center);                 // cross axis
row.gap(FlexGap::new(0, 2));
```

**FlexDirection** is `Row`, `Column`, `RowReverse`, or `ColumnReverse`. The reverse variants
lay children out from the end of the main axis — useful for a chat log that grows upward,
and it changes only visual order, not DOM order, so tab order is unaffected.

**FlexGap** takes its two values as `(row, column)`, which is worth pausing on because it
reads backwards at first:

```rust
FlexGap::new(1, 0)    // 1 cell of VERTICAL space — between rows
FlexGap::new(0, 4)    // 4 cells of HORIZONTAL space — between columns
```

`row` is the gap *between rows*, therefore vertical. `column` is the gap between columns,
therefore horizontal. Naming follows CSS `row-gap`/`column-gap`. In a `Column` container
you usually want the first; in a `Row` container, the second.

`FlexGap::all(n)` sets both.

## Sizing children

```rust
child.flex_grow(1.0);        // take a share of leftover space
child.flex_shrink(0.0);      // never shrink below basis
child.flex_basis(Length::Cells(20));
```

`flex_shrink(0.0)` deserves special mention because forgetting it causes a real bug rather
than a cosmetic one. An empty Box has no content to establish a minimum size, so with
default shrink it collapses to nothing when space is tight. That is exactly what breaks the
[spacer pattern](GLOSSARY.md#virtualization) in virtualized lists — the spacer collapses,
and the scroll range it existed to hold open collapses with it.

## Alignment

| Property | Axis | Applies to |
|---|---|---|
| `justify_content` | main | container |
| `align_items` | cross | container |
| `align_self` | cross | one item, overriding the container |
| `align_content` | cross | wrapped flex *lines* |

**AlignSelf** is a type alias for `AlignItems`, so the variants are identical. When unset,
an item follows its container's `align_items`.

`align_content` only does anything when wrapping is on and there is more than one line:

```rust
container.flex_wrap(FlexWrap::Wrap);
container.align_content(AlignContent::SpaceBetween);
```

**FlexWrap** is `NoWrap` (default), `Wrap`, or `WrapReverse` — the last wraps identically
but stacks the resulting lines in reverse cross-axis order.

`Display::None` removes a node and its subtree from layout entirely. `Display::Flex` is the
default; there is no block or inline mode, because a terminal grid has no use for one.

## Positioning

**`Position::Flow`** is the default — the node participates in normal flex layout.

**`Position::Absolute { x, y }`** removes it from flow and offsets it from its parent's box
origin:

```rust
use tuidom::style::Position;

let mut overlay = Style::new();
overlay.position(Position::Absolute { x: 4, y: 2 });
```

Offsets are **signed cells**, so negative values place a node above or left of its parent,
and an absolute node may overflow its parent's bounds freely.

**`Position::Anchored { anchor, x, y }`** removes it from flow and places it against where
another node is drawn. See [anchored positioning](#anchored-positioning).

There is no `fixed` or viewport-relative mode. Screen-root placement is expressed by
parenting the node to the document root, which is already absolute in screen terms — a
separate positioning mode would be a second way to say the same thing.

Whatever the mode, **published rectangles are always screen-absolute.** Reading a node's
layout never requires you to walk ancestors and accumulate offsets. The one thing a rect
leaves out is scrolling, which [`screen_rect`](#reading-computed-layout) adds.

## Anchored positioning

A tooltip, dropdown, or popover belongs next to a node elsewhere in the tree, usually drawn
over everything, so it is parented to the root rather than to the node it points at. Its
offset from the root then depends on where that node ended up, scrolling included.
`Position::Anchored` names the node and lets layout do the placing:

```rust
use tuidom::style::{AnchorAxis, AnchorEdge, Position};

let mut tip = Style::new();
tip.position(Position::Anchored {
    anchor: field,
    x: AnchorAxis::new(AnchorEdge::Start, AnchorEdge::Start),
    y: AnchorAxis::new(AnchorEdge::End, AnchorEdge::Start).offset(1),
});
```

Each axis lines one of the node's edges (`Start`, `Center`, or `End`) up with one of the
anchor's, then adds a signed offset. The example puts the tip's top one row below the field's
bottom, left edges aligned. Above the anchor is `anchor: Start, own: End`, which depends on
the node's own height. That is why placement happens after taffy has sized it rather than
being a plain offset. An uneven center rounds toward the start.

The node is placed in the same layout pass as its anchor, from where the anchor is drawn:
after every ancestor's scroll, so it follows a scroll in the same frame. That costs a layout
pass per scroll while anchored nodes exist (see [scrolling](scrolling.md#layout-is-scroll-invariant)).
It is sized as an absolute child of its own parent, and paints and clips as part of its
parent's subtree, which is why it usually lives under the root.

Its published `rect` is written so that its screen rect lands beside the anchor: inside a
scrolled container of its own, the rect carries that scroll added back. Read
[`screen_rect`](#reading-computed-layout) for where it draws.

The node and its whole subtree are left out of the frame, with no layout, so nothing paints or
hits them, when:

- the anchor has no layout: removed, never attached, under `display: None`, or in another
  document;
- no cell of the anchor reaches the screen, for example when it is scrolled out of its
  container. Otherwise a tooltip under the root would float over unrelated content;
- anchors form a cycle, including a node anchored to its own descendant.

Opacity does not hide an anchor here. An opacity change reaches no layout pass, so honoring it
would leave the node showing or hidden until something unrelated relaid out.

There is no flipping to the other side of the anchor at the terminal edge, and no sizing from
the anchor, such as a dropdown matching its field's width.

## Borders take space

A [border](styling.md#borders) occupies real cells. Layout insets a bordered node's content
and children by one cell per drawn side, so adding a border to a node shrinks its content
area rather than painting over its first row and column.

This is the opposite of the CSS default (`content-box`) and matches `border-box` thinking:
the rect you get back is the border box, and children live inside it.

## Reading computed layout

Layout results are published per frame as the **layout snapshot** — see
[architecture](architecture.md#layout-is-published-not-stored) for why it is a
document-level map rather than a field on each node.

To read a node's geometry:

```rust
if let Some(view) = doc.get_node(node) {
    if let Some(layout) = view.layout {
        let r = layout.rect;              // border box as laid out
        let port = layout.scrollport;     // rect deflated by the border
        let max_y = layout.max_scroll_y;
    }
}
```

`layout` is `Option` because a node that has never been through a layout pass has no
geometry yet — a node created and read in the same handler, before any frame, has `None`.

The rect is where layout placed the node, before any scrolling. Scroll offsets are applied
at paint time, so a scroll does not move layout (see
[scrolling](scrolling.md#scroll-offset-is-runtime-state)), and a node inside a scrolled
container keeps the same `rect` while it moves on screen. To know where the node is drawn,
ask for its screen rect:

```rust
let on_screen = doc.screen_rect(node);   // Option<LayoutRect>
```

That is `rect` shifted by the offset of every ancestor scrolling on each axis, the same
shift paint applies. It is position only: a node scrolled out of its container's viewport
still gets a rect, lying outside that viewport. To know how much of the node actually
shows, ask for its visible rect:

```rust
let shown = doc.visible_rect(node);      // Option<LayoutRect>
```

That is the screen rect cut down by every ancestor that clips on an axis (`Scroll` or `Clip`,
to its padding box) and by the terminal edge, which is what paint does. It is `None` when no
cell of the node is drawn, which includes a zero-size node and one under an ancestor with
`display: None` or zero opacity. Being painted over does not count: a node under a modal is
still visible. To place a tooltip, prefer [anchored positioning](#anchored-positioning),
which does both in the layout pass rather than a frame later.

Rectangles may be negative or extend past the terminal. Layout does not clamp; clipping to
the visible grid is a render concern.

These are reads of the last frame. To act when a node's size changes rather than checking it,
listen with [`on_size_change`](events.md#events-that-report-what-the-engine-did), which
reports the content box after each layout that resizes it.

## Centering in discrete cells

Centering is where terminal cells stop being a detail. Leftover space is often odd, and
then there is no center — there are two cells equally close to it.

tuidom refuses to pick silently:

```rust
use tuidom::geometry::{center_x, Centered};

match center_x(container_width, content_width) {
    Centered::Even(x) => { /* exactly centered */ }
    Centered::Uneven { low, high } => { /* two equally valid offsets */ }
}
```

When you genuinely do not care, ask for the biased answer explicitly:

```rust
use tuidom::geometry::any_center_x;

let x = any_center_x(container_width, content_width);   // left/top-biased
```

`Centered::any()` does the same thing on a value you already have. Both helpers return an
offset from the container's origin — exactly what `Position::Absolute` wants — and content
wider than its container yields a *negative* offset rather than being clamped, matching the
signed space layout already publishes.

The reason this is an enum rather than a rounded integer: a dialog centered with one
convention and a title centered inside it with another end up one cell out of alignment,
and that is invisible in the code and obvious on screen.

## Rounding

Taffy computes in floats; terminal cells are integers. tuidom runs taffy with rounding
enabled, so edges are rounded in absolute terms rather than each node's size being rounded
on its own — a naive per-node round leaves one-cell gaps and overlaps wherever two
rectangles meet.

You should not have to think about this, but it is why a node's width plus its sibling's
always covers the space they were given, rather than occasionally coming up a cell short.

## Overflow

When content exceeds a node's box, `overflow_x` and `overflow_y` decide what happens —
`Visible` (default) spills, `Scroll` clips and scrolls, `Clip` clips without scrolling.

Overflow has a layout-level effect beyond clipping, which is covered with scrolling; see
[the glossary](GLOSSARY.md#layout--positioning) until that guide lands.

## Where to go next

- [Styling](styling.md) — borders, spacing, pseudo-states, inheritance
- [Colors](colors.md) — the color system in full
- [Architecture](architecture.md) — when layout runs, and what it publishes
