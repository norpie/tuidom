# Layout

Layout is flexbox, computed by [taffy](https://github.com/DioxusLabs/taffy), in units of
terminal cells. If you know CSS flexbox, you know the model — this guide covers the parts
that are different because the target is a terminal rather than a browser.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#layout--positioning).

## Cells, not pixels

Every length in tuidom is a whole terminal cell. That produces the library's worst naming
wart, which you should know about immediately:

```rust
Length::Pixels(10)     // ten CELLS, not pixels
```

There are no pixels in a terminal. The variant is named for familiarity with CSS and means
columns on the horizontal axis, rows on the vertical one.

The three length modes:

| | |
|---|---|
| `Length::Pixels(n)` | exactly `n` cells |
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
child.flex_basis(Length::Pixels(20));
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

There is no `fixed` or viewport-relative mode. Screen-root placement is expressed by
parenting the node to the document root, which is already absolute in screen terms — a
separate positioning mode would be a second way to say the same thing.

Whatever the mode, **published rectangles are always screen-absolute.** Reading a node's
layout never requires you to walk ancestors and accumulate offsets.

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
        let r = layout.rect;              // screen-absolute border box
        let port = layout.scrollport;     // rect deflated by the border
        let max_y = layout.max_scroll_y;
    }
}
```

`layout` is `Option` because a node that has never been through a layout pass has no
geometry yet — a node created and read in the same handler, before any frame, has `None`.

Rectangles may be negative or extend past the terminal. Layout does not clamp; clipping to
the visible grid is a render concern.

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
