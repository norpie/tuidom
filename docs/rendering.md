# Rendering

How a laid-out tree becomes bytes on a terminal: paint order, the cell grid, canvases,
diffing, and the cursor.

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

## Canvas

A **canvas** is a node whose content is cells the app draws itself, for charts, plots, or
anything text and boxes cannot lay out. The app fills a `CanvasBuffer` and hands it over:

```rust
use tuidom::canvas::{CanvasBuffer, CanvasStyle};
use tuidom::style::ResolvedColor;

let canvas = doc.create_canvas()?;
doc.append_child(doc.root(), canvas)?;

let mut buffer = CanvasBuffer::new(20, 5);
let red = CanvasStyle { fg: Some(ResolvedColor::red()), ..CanvasStyle::default() };
buffer.text(0, 0, "peak", red);
buffer.set(4, 2, "•", CanvasStyle::default());
doc.set_canvas(canvas, buffer)?;
```

**The app pushes; the engine never calls back.** A frame is laid out and painted on the render
task, which never runs user code, so there is no draw callback. `set_canvas` stores the buffer
and schedules a frame, and paint copies whatever was stored last. Setting a buffer equal to
the stored one does nothing, so redrawing from a handler that runs every frame does not keep
frames coming.

**Where it draws.** Buffer cell `(0, 0)` lands at the top-left of the content box, inside
border and padding. The buffer is cropped to the content box and to the node's clip, so a
canvas in a scroll container scrolls like anything else. The node's own background, border
and half-block edges paint first; the buffer goes on top.

**How a cell draws.**

- An empty cell (never written, or cleared) leaves what is under it showing.
- An unset `fg` takes the node's resolved `color`, as text does. An unset `bg` leaves the
  node's background, or what the node sits on.
- Colors blend like any other: their alpha and the node's opacity apply.
- Bold, italic and underline add to the node's own. A cell cannot turn off bold the node has.
- A wide grapheme takes two cells. One that does not fit before the buffer's right edge is not
  written; one cut by the crop or clip is dropped whole. Writing over either half of one
  empties the other half.
- Canvas cells are not part of text selection.

**Sizing.** An axis the canvas's style leaves unset takes the buffer's size, as an HTML
`<canvas>` does. That size is preferred, not a minimum: flex can shrink a canvas below its
buffer, cropping it, unless `flex_shrink` is zero. A canvas takes no children; adding one
returns `NodeCannotHaveChildren`.

**Drawing to the size it gets.** A canvas that should fill its space sets `flex_grow` (or a
size) and redraws when [`on_size_change`](events.md#events-that-report-what-the-engine-did)
reports its content box:

```rust
let d = doc.clone();
doc.on_size_change(canvas, move |event| {
    let _ = d.set_canvas(canvas, draw(event.width, event.height));
})?;
```

The frame that changes the size waits for that handler before it paints, so the new buffer
shows in the same frame, never the old one cropped or with space left over. Because the
buffer is no minimum, a canvas drawn for a larger box can still shrink and be told so.

**Theme colors.** A buffer holds resolved colors, so a canvas does not follow variables the way
a style does. Resolve the colors at the canvas node with
[`resolve_color`](colors.md#evaluating-a-color-from-code) when drawing, and draw again when the
variables they come from change. [Charts](charts.md) draw this way.

## Images

An **image** node shows decoded pixels. The app gives it an `ImageData`, RGBA bytes at a width
and height, behind an `Arc`:

```rust
use std::sync::Arc;
use tuidom::image::{ImageData, ImageFit};

let image = doc.create_image()?;
doc.append_child(doc.root(), image)?;
doc.set_image(image, Arc::new(ImageData::from_bytes(&std::fs::read("cat.png")?)?))?;
doc.set_image_fit(image, ImageFit::Cover)?;
```

`ImageData::from_bytes` decodes PNG, JPEG, GIF (its first frame) and WebP through the `image`
crate, behind the `decode` feature, which is on by default. Without it, `ImageData::from_rgba`
takes pixels the app decoded itself. Setting the same `Arc` again does nothing, decided without
comparing pixels, so an app that keeps its images in `Arc`s can set them freely.

**Sizing.** An axis the style leaves unset takes the image's pixels divided by the cell's pixel
size, rounded up. The cell size comes from [`TerminalInfo::pixel_size`](#what-the-terminal-is);
where the terminal has not reported one, a cell is assumed to be 8 by 16 pixels. When the style
sets one axis and not the other, the other follows the image's aspect ratio, as an HTML `<img>`
does. Only a size the style sets counts: a width known because flex stretched or shrank the
node leaves the height at the image's own, since following it would grow a stretched image off
the screen. As with a canvas, that size is preferred rather than a minimum, and an image takes
no children.

**Fit.** The image is placed in the content box by its `ImageFit`:

- `Contain`, the default, shows the whole image as large as fits with its aspect ratio kept,
  centered. An odd leftover cell goes to the right or bottom. The node's background shows in
  the space around it.
- `Cover` fills the box with the aspect ratio kept, cropping evenly from the sides that
  overflow.
- `Fill` stretches the whole image over the box.

Aspect ratios are worked out in pixels, so a cell's being taller than it is wide is accounted
for.

**How it is drawn** is one choice for the whole document, `doc.set_image_protocol(..)`:

- `Auto`, the default, takes the first the terminal is detected to support: kitty placeholders
  (`graphics.placeholders`), iTerm2 images (`graphics.iterm2`, and only outside a multiplexer),
  sixel (`graphics.sixel`), then half-blocks, which are also used before the probe has answered
  and in headless runs that declare nothing.
- `Kitty`, `Sixel` and `ITerm2` always use that protocol, for a terminal that supports it but is
  not detected.
- `HalfBlock` always uses half-blocks.

When the probe's answer changes what `Auto` resolves to, or changes the cell size, the next
frame redraws.

### Half-blocks

Each cell shows two pixels: `▀` with the upper one as the glyph color and the lower one as
the background. The image is resampled to that resolution by averaging the pixels each one
covers, with color weighted by alpha so transparent pixels do not tint their neighbors. A fully
transparent half leaves that half showing what is behind the cell, and a cell transparent in
both stays empty. A partly transparent upper half blends over the lower half's color rather
than over what is behind, since a cell has only two colors; it shows only on soft edges.

The cells go through the same painting as a canvas, so clipping, scrolling, opacity and
z-order all apply. They are made once per image and size and reused until either changes.

### Kitty placeholders

Kitty's graphics protocol can show an image through ordinary cells. Each covered cell holds the
character `U+10EEEE` and two combining marks numbering its row and column within the image,
and its glyph color is the image's id. The terminal draws the matching part of the uploaded
image in that cell. Because the image is cells, clipping, scrolling, overlays and the diff all
work unchanged: a scrolled image writes the cells still visible, numbered by their place in the
image, and a node drawn over part of it simply replaces those cells.

The pixels behind the ids are uploaded in the flush, before the cells that use them and inside
the same synchronized update. An upload is the image already resampled to the pixels its cells
cover, compressed and sent in chunks. Kitty fits an image into its placeholder rectangle
keeping the aspect ratio, so sending it at exactly that shape is what lets `Fill` stretch and
`Cover` crop. The cost is that an id names one image at one size: when the node's fitted size,
crop or the cell size changes, the image is uploaded again under a new id and the old one is
deleted. Ids a frame no longer draws are deleted too, and so is everything still held when the
app exits normally; a panic exit leaves them to the terminal.

Some limits follow from the cell encoding:

- **Opacity is not applied.** Blending the glyph color would turn it into another image's id,
  so a translucent node's image draws fully opaque.
- **At most 297 cells** wide or tall, the number of marks kitty defines; beyond that the image
  is cropped.
- **Ids are 24-bit colors.** Like everything else the engine draws, they go out as truecolor; a
  terminal or multiplexer that downsamples colors breaks them.

Inside tmux, the upload and delete commands are wrapped for passthrough, which tmux forwards
only with `allow-passthrough` on. The placeholder cells are plain text and need nothing.

Not every terminal that speaks kitty graphics supports placeholders, and none says so when
asked. WezTerm answers the kitty query without them, so `graphics.placeholders` is set only
when the kitty query answered and the terminal is kitty or Ghostty.

### Sixel and iTerm2

Sixel and iTerm2 images are not cells: the terminal draws one at the cursor, over whatever
cells it lands on. So the grid is told where they are. Each cell an image covers holds a
marker, the image's id and the cell's column and row in it, over the background painted
beneath. Clipping, scrolling and z-order decide which markers a frame keeps, exactly as they
would for text, and a marker that appears, moves or changes is a changed cell to the diff. The
id changes with the image, its fitted size, its crop and the cell size, as for kitty.

The flush writes marker cells as blank background, then draws each image that needs it: the
cursor moves to the top-left of the image's visible part, the bounding box of its markers, and
only that part is sent. Drawing it covers everything in that rectangle, so the cells there that
are not its markers, such as a menu over part of it, are written again after it. An image is
drawn when the frame has a marker for it the last frame did not have, which happens when it
first shows, changes, moves or is uncovered. Text drawn over part of an image replaces that
part in the terminal, so covering one needs no redraw. An image whose rectangle covers another
image's markers draws that image again after itself. Once an image is gone, its cells turn
back into ordinary ones and writing them erases it.

Some terminals scroll the whole screen when one of these images touches the last row, and
DECSDM, the mode meant to stop that, means opposite things in different terminals and
versions. So the part of an image on the screen's last row is drawn as half-blocks instead.

**Sixel** sends pixels with a palette. The image is resampled to the pixels its cells cover and
reduced to at most 256 colors by median cut, once per id; a redraw only encodes its crop. A
sixel pixel is either drawn or not, so fully transparent pixels are left undrawn and partly
transparent ones are blended with the node's background beforehand. Sixel draws six pixel rows
at a time, and a band reaching past an image's last row would draw into the row below it, so
heights are rounded down to a multiple of six: on cells whose height is not one, up to five
pixel rows go missing at the bottom of the image or of its visible part.

**iTerm2** sends a PNG with the number of cells to stretch it over, and the terminal scales it.
The image is sent at its own resolution where that is smaller than its cells' pixels, and
nothing is kept in the terminal between draws, so each redraw sends it again.

Opacity is not applied to either. Inside tmux, sixel goes to tmux, which draws it itself when it
was built to; its own device attributes reply says whether it was, and that is what sets
`graphics.sixel` there. iTerm2 images go through passthrough when forced, but `Auto` never
picks them inside a multiplexer: it does not know the image is there, so switching windows
loses it without the app finding out.

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
colors are the pane's. One `tmux display-message` about `$TMUX_PANE` names the terminal tmux
is attached to and says whether `allow-passthrough` is on. It also gives the pane's tty: when
that is not the application's, `$TMUX` was inherited, as by a terminal started from a shell
inside tmux, and the probe carries on as if outside it. Nothing is asked of that terminal directly. A query
wrapped for passthrough does reach it, but its reply comes back to tmux, which keeps it, and a
device attributes reply tmux did not ask for makes it briefly resize the window. So inside
tmux, kitty graphics and iTerm2 images go by the attached terminal's name, and only count while
passthrough is on. Sixel is the exception: tmux draws it itself and says so in its own DA1
reply.

How far each field goes:

- `color_depth` is a floor. A terminal that says nothing is assumed to show 16 colors.
- `no_color` is the user's `NO_COLOR` preference, not something the terminal lacks.
- `graphics.iterm2` has no query, so it comes from the terminal's name alone. Inside tmux,
  `graphics.kitty` does too.
- `graphics.placeholders`, kitty graphics shown through placeholder cells, also goes by name:
  no terminal answers a query for it.
- `pixel_size` is re-read on every resize, and the event fires again when it changes.

Images are the one thing that reads it: `graphics` and `multiplexer` pick how they are drawn
under `ImageProtocol::Auto`, and `pixel_size` sizes them. Nothing else changes how the engine draws.
Colors still go out as 24-bit RGB, and the background color math still uses the
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
