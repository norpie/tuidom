# Virtualization

tuidom does not virtualize. It provides the arithmetic, and downstream owns every node.

That is a deliberate position rather than an unfinished feature, and it is the same one a
browser takes: a browser sits *under* a virtualized list rather than being one. The
`virtualize` module is window math and window diffing — no widget, no list component, no
opinion about what your rows look like.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#virtualization).

## When you actually need it

[Culling](scrolling.md#culling) already skips painting anything off screen, and it is exact.
Ten thousand rows in a scroll container paint at the cost of the visible dozen.

What culling does not save is **layout**. Every one of those rows is still a node taffy
measures and positions each pass. Virtualization is what you reach for when the *count*
itself is the problem — when laying out the collection costs more than you want to spend,
not when painting it does.

If your list is a few hundred rows, culling is enough. Reach for this at thousands.

## The spacer pattern

The whole design rests on one trick. Materialize only the visible window, and hold open the
space of everything else with two empty boxes:

```
┌─ scroll container ────────┐
│  leading spacer (N cells) │   ← stands in for items 0..start
│  item 41                  │
│  item 42                  │   ← the materialized window
│  item 43                  │
│  trailing spacer (M cells)│   ← stands in for items end..count
└───────────────────────────┘
```

Because the spacers hold real extents, the container's **measured content size is the true
total**. Scroll clamping, scrollbar geometry, and wheel routing all keep working with
nothing virtual about them — they are measuring a container that genuinely is that tall.

That is the point of the pattern: no part of the engine needs to know virtualization is
happening.

**A spacer must set `flex_shrink(0.0)`:**

```rust
let mut spacer = Style::new();
spacer.height(Length::Cells(cells));
spacer.flex_shrink(0.0);
```

An empty Box has no content to establish a minimum size, so with default shrink it collapses
to fit the container — and the scroll range it exists to hold open collapses with it. This
is the single most likely thing to go wrong, and it fails as "scrolling mysteriously stops
after a few rows" rather than as anything that looks like a spacer problem.

## Uniform items

When every item is the same size, the math is closed-form:

```rust
use tuidom::virtualize::Uniform;

let uniform = Uniform { count: 10_000, stride: 3 };
let window = uniform.window(scroll_offset, viewport_height, 5);
//                          offset        viewport         overscan
```

**Stride** is cells from one item's start to the next — the item's extent *plus any flex gap
between items*. Forgetting the gap is the classic off-by-one-per-row error; with a gap of 1
and a height of 2, the stride is 3.

Because the math is built on a constant stride, **virtualized items must not flex-grow or
shrink**. An item whose size depends on available space breaks the assumption that item `i`
starts at `i * stride`.

The **window** it returns is the item range that should exist in the DOM — the items
covering the scrollport, straddlers included, plus **overscan** on each edge — together with
the two spacer extents. Overscan is measured in *items*, not cells, and exists so a small
scroll reveals rows that already exist rather than waiting for materialization.

`total_extent()` and `offset_of(index)` answer the other two questions you will have.

## Variable-sized items

When items differ, a **measurement cache** answers the same queries over a mix of estimates
and real measurements:

```rust
use tuidom::virtualize::MeasurementCache;

let mut cache = MeasurementCache::new(10_000, 3);   // count, estimate per item
let window = cache.window(offset, viewport, overscan);
```

Items you have not measured contribute the estimate; items you have contribute their real
extent. It is backed by a Fenwick tree over the *deltas from the estimate*, so
`offset_of` and `window` stay logarithmic no matter how much of the collection has been
measured — the common case of "mostly unmeasured" costs nothing extra.

Measurements go back in as you learn them:

```rust
let shift = cache.record(index, measured_extent);
```

Invalidation is available at three granularities — `invalidate(i)`, `invalidate_range(r)`,
`invalidate_all()` — for when content changes underneath you.

Like all the virtualization math, a cache is **one axis**. A 2D grid runs one per axis.

### Anchoring

That return value from `record` is the interesting part.

When you measure an item that lies *above* the viewport and it turns out bigger than
estimated, everything below it shifts down — including the content the user is looking at.
The screen jumps under their hands, which is the single most irritating bug in virtualized
lists.

`record` returns the **signed change** as the compensation to apply:

```rust
let shift = cache.record(index, extent);
if shift != 0 {
    doc.scroll_by(container, 0, shift as i32)?;
}
```

Absorbing it into the scroll offset keeps what is on screen visually pinned. The scrollbar
moves, because the content genuinely got taller; the content does not.

## The `Virtualizer`

`Virtualizer` wraps either strategy and diffs windows for you, so you handle *changes*
rather than recomputing a full window every scroll:

```rust
use tuidom::virtualize::Virtualizer;

let mut v = Virtualizer::uniform(10_000, 3, 5);      // count, stride, overscan
// or
let mut v = Virtualizer::measured(10_000, 3, 5);     // count, estimate, overscan

if let Some(update) = v.update(scroll_offset, viewport) {
    // update tells you which index ranges to add and remove,
    // and the two spacer sizes to set
}
```

`update` returns `None` when the new window matches the materialized one — which is most
scroll events, since a scroll inside the overscan margin changes nothing. That no-op is
what keeps this cheap enough to run on every wheel tick.

The rest of the surface: `count()` / `set_count()` for collections that change size,
`materialized()` for the current range, `record()` to feed measurements through to the
cache, `reset()` to start over, and `cache()` / `cache_mut()` for direct access when the
`Virtualizer`'s own surface is not enough.

`examples/demo.rs` has a working implementation against a 10,000-row pane.

## Trees

There is no tree virtualization, and there does not need to be.

Flatten your visible rows — expanded nodes contribute children, collapsed ones do not — and
virtualize the resulting flat sequence. tuidom has no tree semantics to get in the way, and
you keep full control over what "visible" means, which is where every tree widget's
disagreements with its users actually live.

## Where to go next

- [Scrolling](scrolling.md) — overflow, offsets, culling, scrollbars
- [Layout](layout.md#sizing-children) — why `flex_shrink(0.0)` matters
