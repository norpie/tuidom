# Colors

tuidom's color system is the most distinctive thing in the library and the easiest thing to
get subtly wrong. Two ideas carry all of it: colors are **OKLCH** until the last possible
moment, and a `Color` is an **expression** rather than a value.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#colors).

## OKLCH, and why not RGB

Every color operation happens in **OKLCH** — Lightness, Chroma, Hue, Alpha. Conversion to
RGB happens once, at render time, through a cache.

```rust
use tuidom::style::Color;

let blue = Color::oklch(0.55, 0.15, 260.0);
//                       L     C     H
```

- **Lightness** `0.0`–`1.0`, perceptually uniform
- **Chroma** `0.0` (gray) to roughly `0.4`, saturation
- **Hue** degrees, `0.0`–`360.0`

The payoff is that arithmetic on colors behaves. In RGB or HSL, "10% lighter" is a
different perceived step for yellow than for blue, because neither space's lightness axis
tracks human perception. In OKLCH it is the same step:

```rust
let a = Color::oklch(0.5, 0.15, 90.0).lighten(0.1);    // yellow-ish
let b = Color::oklch(0.5, 0.15, 260.0).lighten(0.1);   // blue-ish
// both moved the same *visible* amount
```

This is also why **lightness steps are absolute, not proportional**. `darken(0.1)`
subtracts `0.1` from lightness; it does not scale it by 90%. A proportional step would
reintroduce exactly the nonuniformity OKLCH exists to remove.

Named constructors exist for the obvious cases — `Color::white()`, `black()`, `red()`,
`green()`, `blue()`, `cyan()`, `magenta()`, `yellow()` — and `Color::oklcha(l, c, h, a)`
takes alpha.

## A `Color` is an expression

This is the idea that makes the rest of the system work:

```rust
Color::oklch(0.5, 0.1, 200.0)          // a concrete color
Color::var("--primary")                // a name — meaningless without a node
Color::CurrentBg.darken(0.1)           // relative — meaningless without a node
Color::var("--accent").with_alpha(0.5) // a derivation of a name
```

All four have type `Color`. Only the first denotes anything on its own. The other three are
*programs* that produce a color when evaluated against a specific node, and that evaluation
happens during style resolution.

What a `Color` evaluates to is a **ResolvedColor** — concrete OKLCH, and what
`ResolvedStyle` actually holds. Hue is stored canonically in `0`–`360`, so `-90.0` and
`270.0` are one color and share one cache entry rather than two.

The practical consequence: you can write one `Style` and apply it to nodes all over the
tree, and `Color::var("--primary")` or `Color::CurrentBg.darken(0.1)` will mean something
different — and correct — at each of them.

## Color variables

A **color variable** is a named color, declared on the document or on a node, in scope for
that node's descendants:

```rust
doc.set_color_var("--primary", Color::oklch(0.6, 0.18, 260.0));

let mut s = Style::new();
s.color(Color::var("--primary"));
```

Per node, declare on the `Style`:

```rust
let mut panel = Style::new();
panel.color_var("--primary", Color::oklch(0.7, 0.2, 30.0));   // shadows for this subtree
```

Redeclaring a name shadows it for that subtree, exactly like a lexical scope. This is what
you reach for instead of `Inherit` on every node when a value should flow down a tree.

Two rules that are not obvious and both exist to make cycles unwritable:

**A node's own declarations resolve against its *parent's* scope**, never against each
other. So this does not work:

```rust
panel.color_var("--base", Color::oklch(0.5, 0.1, 200.0));
panel.color_var("--hover", Color::var("--base").lighten(0.1));   // --base is the PARENT's
```

The reason is that declarations live in a `HashMap`, which has no order. If they resolved
against each other, whether `--hover` saw the new `--base` or an outer one would depend on
iteration order. Resolving the whole set against an already-concrete parent scope makes the
question disappear — and makes reference cycles impossible to express rather than merely
detected.

**An undefined name makes the whole expression unresolvable**, and the property falls back
to its default:

```rust
s.background(Color::var("--nonexistent").darken(0.2));   // → default background
```

It does not half-apply the derivation to some fallback color. A partially-applied
derivation would produce a color nobody chose, which is harder to debug than a visibly
missing one.

## Derivations

Any `Color` can be transformed, including one that is not concrete yet:

```rust
Color::var("--primary").darken(0.1)
Color::CurrentBg.lighten(0.05).with_alpha(0.8)
Color::var("--accent").with_hue(120.0)
```

Available: `lighten`, `darken`, `with_lightness`, `with_chroma`, `with_hue`, `with_alpha`,
and `mix`.

`mix(other, t)` blends in OKLCH and has two behaviors worth knowing. It takes **the short
way around the hue circle**, so mixing 350° with 10° passes through 0° rather than
travelling backwards through 180°. And when one side is a **gray**, it borrows the other's
hue instead of interpolating toward gray's nominal 0°:

```rust
Color::oklch(0.5, 0.0, 0.0).mix(Color::oklch(0.5, 0.2, 260.0), 0.5)
// → a desaturated blue, not a swing through red
```

A gray has no hue. Interpolating its stored 0° would drag the result through oranges and
reds on the way to blue, which is never what anyone means.

## `CurrentBg` and `CurrentFg`

These resolve relative to the node they are used on:

```rust
let mut s = Style::new();
s.background(Color::oklch(0.3, 0.05, 260.0));
s.border_color(Color::CurrentBg.lighten(0.15));    // a border derived from this node's own bg
```

They are self-referential in the two properties they are defined *from*, so resolution runs
in a fixed order. This is the single most surprising rule in the library:

```
color variables  →  background  →  color  →  every other color property
```

- In a **variable declaration** and in **`background`**, `CurrentBg` and `CurrentFg` mean
  the **parent's** values.
- In **`color`**, `CurrentBg` already means this node's own resolved background — but
  `CurrentFg` still means the parent's, because `color` is the property being defined.
- From every **other** color property onward, both mean this node's own.

Any other reading is circular. `background: CurrentBg.darken(0.1)` has to mean "darker than
what I sit on"; there is no other coherent interpretation, since the node's own background
is precisely what is being computed.

## Effective background

**Effective background** is what a node visually sits on: its own background if it has one,
otherwise the nearest ancestor's, falling back to the document's terminal background.

It is what `CurrentBg` resolves to, and it is **never absent**. A node deriving a color from
what it sits on needs an answer even when nothing in its ancestry paints anything, so the
chain always terminates:

```rust
doc.set_terminal_background(Color::oklch(0.15, 0.0, 0.0));
```

This is why `CurrentBg` on a transparent node sees *through* to the nearest painted
ancestor rather than resolving to nothing.

## Terminal background is an assumption

`set_terminal_background` does not paint anything, and it does not detect anything.

The real terminal background is unknowable without querying the terminal, which not all
terminals answer. So tuidom has you **declare** it, and uses that declaration for two
things: the bottom of the effective-background chain, and the base a translucent color
blends toward over an unpainted cell.

It is never itself painted. An unpainted cell emits the terminal's default, so an app that
styles nothing keeps showing the user's real background — including their transparency and
their image, if they have one. Declaring the wrong value makes derived colors slightly off;
it does not put a wrong-colored rectangle on screen.

## Alpha and blending

Alpha is a component of every color, and blending happens at render time:

```rust
s.background(Color::oklcha(0.2, 0.05, 260.0, 0.7));   // translucent overlay
```

Painting is back-to-front, so a translucent color blends with whatever the buffer already
holds. Two consequences:

**Translucent background fills preserve text.** A modal scrim over a screen of text dims
the text rather than erasing it, because the fill blends into cells that already hold
glyphs instead of replacing their content.

**Over an unpainted cell**, there is nothing in the buffer to blend with, so the declared
terminal background is used as the base — which is the second job of that declaration.

## Rgb, and when conversion happens

**Rgb** is the final form, and the only place it appears is the render pipeline. OKLCH →
RGB is genuinely expensive math, so it is cached per resolved color and reused across
frames and across nodes.

You will not normally construct an `Rgb`. If you find yourself wanting to, the question is
usually really about `Color::oklch` and what its arguments should be.

## Where to go next

- [Styling](styling.md) — how colors sit inside `Style`, and how pseudo-states merge
- [Architecture](architecture.md) — where style resolution happens in a frame
