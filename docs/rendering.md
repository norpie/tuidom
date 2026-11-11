# Rendering

How a laid-out tree becomes bytes on a terminal: paint order, the cell grid, diffing, and
the cursor.

Most of this is machinery you never touch — it is here because paint order is genuinely
part of the API surface, and because knowing what the diff does explains several
otherwise-surprising performance characteristics.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#rendering).

## Paint order

Nodes paint in tree order, back to front — painter's algorithm. Later paints cover earlier.

Between siblings, **z-index** decides:

```rust
let mut s = Style::new();
s.z_index(10);      // paints after siblings with lower values
```

Lower values paint first, higher later, and **DOM order is the stable tiebreaker** for equal
values — so siblings with no z-index paint in the order you added them.

### A subtree is atomic

This is the rule that matters, and it is different from CSS.

**A descendant's `z_index` cannot escape its parent subtree.** Every node's subtree paints as
one unit, at that node's position in its parent's ordering. A child with `z_index: 9999` in a
panel that paints early still paints under the panel that paints after it.

The web's version of this — where a positioned descendant can escape and paint above
unrelated UI unless a stacking context is deliberately introduced — is the direct cause of
the "my dropdown renders under the modal" class of bug. tuidom makes isolation the default
and never offers the escape.

### Stacking contexts

Because subtrees are already isolated, a **stacking context** does not exist to isolate
painting — that is already true everywhere.

What `stacking_context: true` does is make a node **eligible to trap focus**:

```rust
let mut s = Style::new();
s.stacking_context(true);
doc.set_style(modal_layer, &s)?;
doc.push_focus_context(modal_layer)?;    // only accepted on a stacking context
```

The requirement exists because trapping focus inside a subtree a sibling could paint over
would leave the user interacting with something they cannot see. Marking a node asserts that
it is a self-contained visual unit, which makes the trap safe.

Being a stacking context never traps focus on its own. See
[focus contexts](focus-and-selection.md#focus-contexts).

## The grid

A frame is painted into a **grid**: a 2D buffer of cells, one per terminal character
position. Each cell holds display content, foreground and background color, and text
attributes.

Cell content is one of three things:

- **empty** — renders as a space
- **a glyph** — a grapheme cluster, with its terminal width (1 or 2)
- **a wide continuation** — the second cell of a double-width glyph

That third case is how CJK and emoji work. A width-2 glyph occupies two cells: the head
holds the character and prints it; the continuation is a marker that is never printed
directly, but must exist so the grid's geometry stays honest about which positions are taken.

Attributes (bold, italic, underline) are **packed onto the cell**, unlike on `Style` where
they are three separate properties. Nothing merges at the cell level — attributes belong to
the glyph and are replaced or cleared with it — so there is no reason to keep them apart.

The grid also carries an active **clip** while painting, honored at the cell-write level.
That is what makes fills, text, borders, and half-block edges all clip identically instead
of each implementing their own bounds check.

### These types are internal

`Cell`, `CellContent`, `CellAttrs`, and `Grid` are crate-private. You cannot construct or
inspect them directly.

What you *can* inspect is `ScreenCell`, the public view the headless runtime exposes:

```rust
if let Some(cell) = rt.get_cell(4, 2) {
    cell.text;                    // String — the glyph, or " "
    cell.fg;  cell.bg;            // Option<ScreenColor>
    cell.width;                   // 1 or 2
    cell.is_wide_continuation;
    cell.bold; cell.italic; cell.underline;
}
```

See [testing](testing.md) for the rest of that surface.

## Culling

Painting drops any node whose translated rectangle lies wholly outside its clip, and stops
descending where a subtree's clip is empty.

Culled nodes remain in the DOM and in layout — culling is paint-only, and therefore exact
for variably sized content. See [scrolling](scrolling.md#culling), and
[virtualization](virtualization.md) for when it is not enough.

## Diffing and flushing

Each frame is painted into a fresh grid and compared against the previous one, cell by cell.
Only differing cells produce output.

This is why an idle-but-awake application is cheap: a frame where nothing visually changed
diffs to zero cells and writes nothing to the terminal.

**Above about a third of cells changed, the renderer switches to a full redraw.** Past that
threshold, the escape sequences to address and update scattered cells individually cost more
than rewriting every cell in order. Resizes always take this path. The screen is not cleared
first: every cell is written anyway, and a clear would show the terminal's own background until
they arrive.

Each flush is wrapped in a **synchronized update**, which tells the terminal to show the frame
only once all of it has arrived. Without it, a large frame can appear half old and half new for
a moment. Terminals that do not support it ignore the markers.

Text attributes emit as **sticky SGR state** — a transition is written only when attributes
change between adjacent cells, so a long bold run costs one sequence rather than one per
character.

Alpha blending happens here too. Because painting is back-to-front, a translucent color
blends with whatever the buffer already holds — which is why a translucent background fill
**preserves the text underneath** rather than erasing it, and what makes modal scrims work.
Over an unpainted cell there is nothing to blend with, so the declared
[terminal background](colors.md#terminal-background-is-an-assumption) is the base.

## The cursor

Cursor metadata is produced *with* a frame rather than painted into it:

```rust
let mut s = Style::new();
s.cursor_shape(CursorShape::Bar);      // Block (default), Underline, Bar
```

A **render cursor** carries position, shape, a foreground-derived color, and whether it is
visible after clipping. It never mutates grid cell content — the real terminal cursor is
moved and shaped from this metadata at flush time.

Keeping it out of the grid is what lets the cursor sit on a cell without altering that
cell's contents, so diffing a frame where only the cursor moved changes no cells at all.

Cursor color follows the focused node's resolved foreground, so a cursor in a themed input
is visible without separate configuration.

## Terminal lifecycle

`doc.run()` enters the alternate screen, enables raw mode, and restores everything on exit.
Three separate mechanisms cover the ways an application can end:

- a **drop guard** restores after a successful run
- a **setup guard** restores partial state if startup fails halfway
- a **panic hook** restores before a crash surfaces

All three read from one place that tracks which modes are actually on, and exactly one of
them ever claims responsibility. See
[panics and terminal restore](events.md#panics-and-terminal-restore).

There is **one `Terminal` per process, enforced** — a second `run()` is refused rather than
left to corrupt the screen. Which is also why [headless](testing.md) exists.

## What the terminal is

`run()` asks the terminal what it is before anything else happens, and reports the answer as a
`TerminalInfo`:

```rust
doc.on_terminal_info(|event| {
    if event.info.graphics.kitty {
        // the kitty graphics protocol will work
    }
});
```

`doc.terminal()` returns the same value afterwards. It is `None` until `run()` has probed, so
code that builds the document before calling `run()` has to listen for the event.
`TerminalInfo::from_env()` gives the environment's guess at any time, without asking the
terminal. Headless never probes; a test declares the info with
`HeadlessRuntime::set_terminal_info`.

**The environment is a first guess.** `TERM`, `TERM_PROGRAM`, `COLORTERM` and a few
terminal-specific variables name the terminal and its color depth. They are often wrong: over
SSH most of them never arrive, and inside a multiplexer they describe whichever terminal
started it. Inside one, the environment's terminal name is not used at all.

**Replies are the answer.** Raw mode goes on, then the probe writes its queries and reads the
replies itself, before crossterm's reader starts and would take them for key presses:

- XTVERSION for the name and version
- OSC 10 and 11 for the default foreground and background
- the kitty graphics query
- XTGETTCAP `RGB` and `Tc` for 24-bit color
- DA1 last. Every terminal answers it, so its reply means everything else that will be
  answered has been, and it carries the sixel flag.

A terminal that ignores DA1 costs up to 300 ms of startup before the probe gives up. Keys
typed while the probe runs are lost. Queries need a Unix terminal; elsewhere the info comes
from the environment alone.

**Inside tmux**, tmux answers the queries itself: XTVERSION gives tmux's version, and the
colors are the pane's. One `tmux display-message` names the terminal tmux is attached to
and says whether `allow-passthrough` is on. Only when it is does a second batch go out,
wrapped so tmux forwards it, to find out whether that terminal has kitty graphics. With
passthrough off nothing waits on it, and no image protocol is reported as usable except sixel,
which tmux can draw itself.

How far each field goes:

- `color_depth` is a floor. A terminal that says nothing is assumed to show 16 colors.
- `no_color` is the user's `NO_COLOR` preference, not something the terminal lacks.
- `graphics.iterm2` has no query, so it comes from the terminal's name alone.
- `pixel_size` is re-read on every resize, and the event fires again when it changes.

All of it is reported, and none of it changes how the engine draws. Colors still go out as
24-bit RGB, and the background color math still uses the
[declared terminal background](colors.md#terminal-background-is-an-assumption).

## Performance metrics

Every frame records timings, available without instrumenting anything:

```rust
let snapshot = doc.performance_snapshot();
```

Or per frame, via [`on_post_frame`](events.md#post-frame). Frame time, layout time, and
render latency are always collected; paint and diff profiling is opt-in through
`set_performance_detail`, since the instrumentation itself costs something.

## Where to go next

- [Scrolling](scrolling.md) — clipping, culling, and scrollbars
- [Colors](colors.md#alpha-and-blending) — how alpha resolves at paint time
- [Architecture](architecture.md#how-a-frame-happens) — where painting sits in a frame
