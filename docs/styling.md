# Styling

tuidom has inline styles and nothing else — no selectors, no stylesheets, no cascade. A
node has a `Style`, and that is the whole model. What replaces the cascade is explicit
inheritance and pseudo-state merging, both covered below.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#styling).
Colors get [their own guide](colors.md); this one covers everything else.

## Style is a struct, not a builder

```rust
use tuidom::style::{Color, EdgeInsets, Length, Style};

let mut panel = Style::new();
panel.width(Length::Percent(100.0));
panel.padding(EdgeInsets::symmetric(2, 1));
panel.background(Color::oklch(0.25, 0.03, 260.0));

doc.set_style(node, &panel)?;
```

Setters take `&mut self` and return nothing, so calls do not chain. This is deliberate:
a `Style` is a value you build once and reuse across many nodes, not a fluent expression
you construct per call site.

```rust
let mut section = base.clone();          // clone and specialize
section.color(Color::oklch(0.82, 0.12, 80.0));
```

For a single change to an already-applied style, `update_style` edits in place:

```rust
doc.update_style(node, |s| s.background(Color::oklch(0.4, 0.1, 30.0)))?;
```

The difference matters for animation, not just ergonomics: both go through the same
change detection, and a property whose *resolved* value moved is what triggers a
transition.

## Three states per property: `StyleValue`

Every property on `Style` is a **StyleValue**, which is one of three things:

| Value | Meaning |
|---|---|
| `Unset` | use the default (the default is *not* the parent's value) |
| `Set(v)` | use `v` |
| `Inherit` | take the parent's resolved value |

This is the whole inheritance model, and it is the one place tuidom deliberately diverges
from CSS. On the web, `color` inherits and `border` does not, and which is which is a
property of the property. Here **nothing inherits unless you ask**:

```rust
let mut child = Style::new();
child.inherit_color();          // explicitly take the parent's color
```

Each property has three setters — `color(v)`, `inherit_color()`, `unset_color()` — so all
three states are reachable for everything. The cost is verbosity when you want a colour
scheme to flow down a subtree; the payoff is that reading a node's style tells you where
every value comes from without knowing a table of which properties are inherited.

For values that *should* flow down a whole subtree, [color variables](colors.md) are
usually the better tool than `Inherit` on every node.

## `ResolvedStyle`: what the engine actually uses

**ResolvedStyle** is a `Style` with every `StyleValue` collapsed to a concrete value:
inheritance walked, defaults applied. It is what layout and paint read, and it is cached
per node and invalidated when an ancestor changes.

```rust
let resolved = doc.resolved_style(node)?;
assert_eq!(resolved.width, Length::Percent(100.0));
```

Two things about it are worth knowing:

Colors in a `ResolvedStyle` are still **OKLCH**, not RGB. Conversion happens at render
time, per frame, through a cache. Resolution answers *which* color; rendering answers what
bytes that is.

Some fields stay `Option` after resolution, and the `None` carries meaning rather than
representing failure. `border_color: None` means *follow this node's foreground*.
`background: None` means transparent — whatever is behind shows through. Collapsing those
to a concrete default would lose the distinction between "no background" and "a background
that happens to match".

## Pseudo-states

A **PseudoState** merges an additional style on top of the base one. There are three, and
they merge in a fixed order:

```
base → focus → active → disabled
```

Later wins, so a disabled style always beats a focus style on a conflicting property.

```rust
let mut focused = Style::new();
focused.background(Color::oklch(0.45, 0.12, 260.0));
doc.set_focus_style(button, &focused)?;

let mut pressed = Style::new();
pressed.background(Color::oklch(0.35, 0.14, 260.0));
doc.set_active_style(button, &pressed)?;
```

Only the properties you set are merged; everything else falls through to the base style.
So a focus style is a diff, not a replacement.

### Hover is focus

There is no hover state. Mousing over a focusable node **focuses** it, so the focus style
is the hover style. One state, one style, one set of transitions.

This is not a shortcut around implementing hover — it is what a terminal can actually
support coherently. A terminal has one pointer and one focused thing, keyboard and mouse
users need the same affordance, and maintaining two nearly-identical visual states that
can both be active at once buys nothing.

### Active

**Active** is the node currently being pressed. The engine sets it on mouse down — on the
hit node's focus target — and clears it on mouse up *anywhere*.

That "anywhere" is the point: press a button, drag off it, release. Without a
global clear, the button stays visually pressed forever. Clearing on any release means a
gesture that ends outside the node still ends the press.

For activation the engine cannot observe — a keyboard Enter on a focusable box, say —
drive it yourself:

```rust
doc.set_active(button, true)?;      // and `false` to release
```

Setting it on a node that blocks interaction is a silent no-op rather than an error — a
disabled node cannot be pressed, and making callers check first would be noise.

### Disabled

**Disabled** blocks interaction across a whole subtree. A node is *effectively disabled*
when it or any ancestor is disabled, and effectively disabled nodes:

- cannot be focused
- are skipped by tab and spatial navigation
- **swallow** targeted events rather than bubbling them to enabled ancestors

That last one is the one to be deliberate about. A disabled button inside an enabled panel
does not deliver its clicks to the panel — the event stops. Otherwise disabling a control
would silently reroute its interactions to whatever contains it, which is worse than doing
nothing.

Every effectively disabled node merges its own disabled style, so the whole subtree greys
out from one flag rather than needing a style per descendant.

Note that disabled is *not* the same as inert, which blocks interaction outside an active
focus context. Inert nodes merge no style at all — content behind a modal keeps its
appearance. See the focus guide when it lands.

## Borders

A **Border** is one **BorderCharset** plus the **Sides** it is drawn on:

```rust
use tuidom::style::{Border, BorderCharset, Sides};

let mut s = Style::new();
s.border(Border::new(BorderCharset::rounded()));                    // all four sides
s.border(Border::new(BorderCharset::single()).with_sides(Sides::new(true, false, false, false)));
```

`BorderCharset` has named constructors — `single`, `double`, `rounded`, `thick`, `ascii` —
but the charset itself is the primitive: eight characters, four edges and four corners.
You can supply your own. There is exactly **one charset per node**, and that is a real
constraint rather than an oversight: a corner character is drawn from the charset, and a
double-top-meets-single-left corner has no character that exists.

**Borders occupy real cells.** Layout insets a bordered node's content and children by one
cell per drawn side, so a border frames content instead of painting over it. Adding a
border to a node makes its content area smaller — which is what you want, and worth
knowing before you wonder why text reflowed.

Border color is a separate property, and follows the node's foreground when unset:

```rust
s.border_color(Color::oklch(0.7, 0.15, 30.0));
```

### Sides are presence, not width

**Sides** is four booleans. Every edge treatment in tuidom is either on a side or it is
not — there is no width, because a terminal cell is the unit and half a cell is not
available.

Corners follow from that: a corner cell gets its corner character only when *both* adjacent
sides are drawn. With only one of them present, that side runs straight through the corner,
so a top-only border comes out as a clean horizontal rule rather than a rule with two
stray corner glyphs on the ends.

## Half-block edges

A **half-block edge** ends a node's fill halfway into its own outermost row or column,
using a half block (`▀▄▌▐`) — or a quadrant block (`▗▖▝▘`) where two edges meet:

```rust
s.half_block_edges(Sides::new(true, false, true, false));   // top and bottom
s.half_block_inner_color(Color::oklch(0.3, 0.05, 260.0));
```

**This is not a border.** It frames nothing, and it costs no layout — it repaints cells the
node already owns, so turning it on never reflows anything. Its entire purpose is the
boundary between two colors.

The reason it exists is the terminal's cell aspect ratio. A cell is about twice as tall as
it is wide, so one row of vertical padding reads as roughly two columns of horizontal
padding, and a box padded `1` on every side looks squashed. Ending the fill on a half cell
is what balances the two.

Both colors are yours. The inner half follows the node's `background` when unset; the outer
half, when unset, keeps whatever is painted underneath — which is what makes the edge blend
into arbitrary content rather than requiring you to know what is behind it.

## Text attributes

`bold`, `italic`, and `underline` are three independent boolean properties on `Style`.
They are separate here because they merge separately — a focus style can add bold without
disturbing italic.

```rust
let mut heading = Style::new();
heading.bold(true);
```

They are emitted as sticky SGR state and only written when they change between cells, so
a long run of bold text costs one escape sequence rather than one per character.

## Metadata that does nothing

Two escape hatches exist for downstream data, and neither affects rendering:

**Custom style properties** are string key/value pairs on a `Style`:

```rust
s.set_custom("data-role", "primary");
```

They do not inherit, do not resolve into `ResolvedStyle`, and do not affect layout or
paint. They exist so a component system can stash information where its styles already are.

**Attributes** are string key/value pairs on the *node*:

```rust
doc.set_attr(node, "id", "submit-button")?;
let id = doc.get_attr(node, "id")?;
```

Keys cannot be empty. Like custom properties, the engine stores them and otherwise ignores
them — tuidom has no selectors, so there is nothing for an `id` to be looked up by except
your own code.

## Where to go next

- [Colors](colors.md) — OKLCH, variables, derivations, and the resolution order
- [Layout](layout.md) — flexbox, sizing, positioning, and centering in discrete cells
- [Architecture](architecture.md) — when resolution and layout actually run
