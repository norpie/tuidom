# Testing

tuidom is built to be tested without a terminal. `HeadlessRuntime` computes real layout,
runs the real paint pass into a real cell grid, and accepts simulated input — so behavior is
assertable directly rather than inferred from escape sequences.

This is the supported feedback loop. Running the app and looking at it is not.

## Why headless exists at all

Two hard constraints make it necessary rather than convenient.

**One `Terminal` per process, enforced.** A second `run()` is refused rather than left to
corrupt the screen. Tests run in parallel in one process, so a terminal-based test suite
would be serialized at best and mutually destructive at worst.

**Wall-clock animations are not assertable.** A transition tested against real time races
the test runner. Headless freezes the document clock, so interpolated values are exact.

## The basic shape

```rust
use tuidom::headless::HeadlessRuntime;

let doc = Document::new()?;
let node = doc.create_box()?;
doc.append_child(doc.root(), node)?;

let mut rt = HeadlessRuntime::new(doc.clone(), 80, 24);
rt.render()?;

assert_eq!(rt.get_cell(0, 0).map(|c| c.text), Some(" ".to_string()));
```

`new` takes the document **by value**, so clone it if you still need a handle — the same
rule as `run()`. Nothing renders until you call `render()`; there is no frame loop, and that
is the point. Every frame in a test happens because the test asked for one.

`document()`, `width()`, and `height()` read back what the runtime is driving.

## Time is frozen

`HeadlessRuntime::new` freezes the document clock. Animations progress **only** through
`advance_time`:

```rust
use std::time::Duration;

doc.set_transition(node, TransitionConfig {
    property: TransitionProperty::Opacity,
    duration: Duration::from_millis(100),
    easing: Easing::Linear,
})?;
doc.update_style(node, |s| s.opacity(0.0))?;

rt.advance_time(Duration::from_millis(50));
rt.render()?;
// exactly halfway — not "roughly halfway, if the machine wasn't busy"
```

`advance_time` also settles finished animations and dispatches their end and iteration
events through the same runtime path the real loop uses. So a test can assert that
`on_animation_end` fired, at the right time, without waiting for it.

This is what makes otherwise-untestable behavior testable — a `WhenScrolling` scrollbar's
hold-then-fade, for instance, is a pure function of this clock, so every phase of the fade
renders deterministically.

## Simulating input

Every one of these goes through the **same** `process_runtime_event` path the real event
loop uses. They are not a parallel implementation that can drift.

| Call | |
|---|---|
| `simulate_key(code)` | one key press |
| `simulate_text("hello")` | one key press per character |
| `simulate_click(x, y)` | press and release |
| `simulate_mouse_down(x, y, button)` | |
| `simulate_mouse_up(x, y, button)` | |
| `simulate_mouse_move(x, y)` | pointer motion — focuses on hover, like the real thing |
| `simulate_mouse_drag_move(x, y, button)` | motion with a button held |
| `simulate_mouse_drag(start, end)` | a whole gesture: down, move, up |
| `simulate_scroll(x, y, delta)` | vertical wheel at a position |
| `simulate_horizontal_scroll(x, y, delta)` | horizontal wheel |
| `simulate_window_focus()` / `simulate_window_blur()` | terminal window OS focus |
| `resize(w, h)` | resizes *and* dispatches the resize event |

Position matters on the mouse calls because hit-testing is real — `simulate_click(4, 2)`
finds whatever is actually painted at that cell, through the same paint-order and clip rules
the renderer uses.

```rust
let input = doc.create_input("")?;
doc.append_child(doc.root(), input)?;
doc.focus(input)?;

let mut rt = HeadlessRuntime::new(doc.clone(), 20, 3);
rt.simulate_text("hey");

assert_eq!(doc.input_value(input)?, "hey");
```

## Inspecting the screen

```rust
rt.render()?;

let cell = rt.get_cell(4, 2);            // Option<ScreenCell> — None if out of bounds
let region = rt.get_screen_region(0, 0, 10, 3);
```

A `ScreenCell` is the public view of an internal grid cell:

```rust
cell.text;                    // String — the grapheme, or " "
cell.fg;  cell.bg;            // Option<ScreenColor> — None means terminal default
cell.width;                   // 1 or 2
cell.is_wide_continuation;    // second cell of a wide glyph
cell.bold; cell.italic; cell.underline;
```

`ScreenRegion` is row-major: `cells` in reading order, with the requested `x`, `y`, `width`,
`height` echoed back so an assertion failure says what was asked for.

`ScreenColor` has `from_rgb(r, g, b)` for building an expected value, which is usually what
you want rather than reconstructing an OKLCH conversion by hand.

## The cursor

```rust
if let Some(cursor) = rt.cursor() {
    cursor.x; cursor.y;
    cursor.shape;       // CursorShape
    cursor.color;       // ScreenColor, from the focused node's foreground
    cursor.visible;     // false when clipped away
}
```

Cursor metadata is produced alongside the frame rather than painted into it, so asserting on
it never means fishing through cells. See [rendering](rendering.md#the-cursor).

## Testing the flush

`render()` paints but does not flush — its diff and flush metrics stay zero. When you want to
assert on the bytes that would reach a terminal:

```rust
rt.render_flushed()?;
let bytes = rt.flush_output();
```

This runs the real diff and the real escape-sequence writer into a buffer. Use it for things
that only exist at the byte level — SGR attribute transitions, full-redraw switching, the
bell. For anything about *what is on screen*, `get_cell` is the better assertion, because it
does not couple the test to escape-sequence formatting.

## What is not here

**No recording or playback.** `start_recording`, `EventLog`, and `replay` appear in
`FEATURES.md` as unchecked items, and they are genuinely unimplemented — there is no partial
version to find.

**No snapshot serialization.** Also planned, also absent.

## Testing conventions in this repo

If you are contributing rather than consuming:

- Tests live **in-crate**, next to the code. `tuidom/tests/` exists and is empty on purpose.
- `document/tests.rs` holds the bulk (214); `headless.rs` has 68 of its own.
- Tests may `unwrap` freely. `src/` may not — see [`STYLE.md`](STYLE.md).
- No doc tests. Rustdoc examples use ```ignore.
- Prefer tests that assert behavior over tests that assert structure. A test that only
  checks a setter stored what it was given verifies nothing the type system did not.

The verification path is `cargo test`. The demo at `examples/demo.rs` is for humans — keep
it building, but it is not how you find out whether something works.

## Where to go next

- [Rendering](rendering.md) — what the grid holds and how diffing works
- [Architecture](architecture.md#how-a-frame-happens) — what `render()` is a slice of
