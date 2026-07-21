# Getting started

Building a tuidom application from nothing. This assumes you have read none of the rest of
the docs; [architecture](architecture.md) explains *why* any of this is shaped the way it
is, and is worth reading second.

## Setup

tuidom is async and runs on Tokio:

```toml
[dependencies]
tuidom = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## The smallest thing that runs

```rust
use tuidom::Document;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::new()?;

    let hello = doc.create_text("Hello, terminal")?;
    doc.append_child(doc.root(), hello)?;

    doc.run().await?;
    Ok(())
}
```

Four things are happening.

`Document::new()` builds the document *and* its permanent root node. There is no separate
"create a root" step and no empty-document state — `doc.root()` is valid immediately.

`create_text` allocates a node in the document's arena and returns a `NodeId`, a `Copy`
handle. You do not own the node; the document does.

`append_child` puts it in the tree. Until a node is parented it exists in the arena but
takes part in nothing — no layout, no paint, no events.

`doc.run()` **consumes** the document and blocks until something calls `quit()`. That
matters immediately, and the next section deals with it.

Note that this program has no way to exit. There is no built-in quit key and no Ctrl+C
handler — that is deliberate, since an engine that reserves keys is an engine you have to
fight. You add one next.

## Handlers, and cloning the document

`run(self)` takes ownership, so any handler that needs the document must capture a clone
made beforehand. Cloning is a refcount bump on an `Arc`:

```rust
use tuidom::event::KeyCode;
use tuidom::Document;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::new()?;

    let hello = doc.create_text("Press q to quit")?;
    doc.append_child(doc.root(), hello)?;

    let d = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        if key.code == KeyCode::Char('q') {
            d.quit();
        }
    })?;

    doc.run().await?;
    Ok(())
}
```

`let d = doc.clone()` before a `move` closure is the idiom you will write constantly.
Handlers are `Fn + Send + Sync + 'static`, so they cannot borrow.

The listener is registered on `doc.root()`. Key events target the focused node and bubble
rootward, so a listener on the root sees every key that nothing else consumed — which is
what you want for global shortcuts.

## Styling

`Style` is a plain struct with setter methods. They take `&mut self` and return nothing,
so this is not a chaining builder:

```rust
use tuidom::style::{Color, EdgeInsets, FlexDirection, Length, Style};

let mut panel = Style::new();
panel.width(Length::Percent(100.0));
panel.height(Length::Auto);
panel.flex_direction(FlexDirection::Column);
panel.padding(EdgeInsets::all(1));
panel.background(Color::oklch(0.25, 0.03, 260.0));

doc.set_style(container, &panel)?;
```

One thing to know up front:

**Colors are OKLCH, not RGB.** `Color::oklch(lightness, chroma, hue)` — lightness `0.0` to
`1.0`, chroma from `0.0` (gray) up to around `0.4`, hue in degrees. It converts to RGB only
at render time. This is what makes `darken(0.1)` mean the same perceptual step on every
hue instead of a different one per color. See [colors](colors.md) for the whole model.

For a one-off tweak, `update_style` edits in place rather than replacing:

```rust
doc.update_style(node, |s| s.background(Color::oklch(0.4, 0.1, 30.0)))?;
```

## Layout

Layout is flexbox, via [taffy](https://github.com/DioxusLabs/taffy). If you know CSS
flexbox you know this already — `flex_direction`, `flex_grow`, `align_items`,
`justify_content`, `gap` all behave as expected, in units of terminal cells.

```rust
use tuidom::style::{AlignItems, FlexDirection, FlexGap, JustifyContent, Length, Style};

let mut row = Style::new();
row.flex_direction(FlexDirection::Row);
row.align_items(AlignItems::Center);
row.gap(FlexGap::new(0, 2));      // (row gap, column gap) in cells
```

The one adjustment terminals force: **a cell is about twice as tall as it is wide.** One
row of vertical padding reads as roughly two columns of horizontal padding, so
`EdgeInsets::all(1)` looks lopsided. `EdgeInsets::symmetric(2, 0)` — two horizontal, no
vertical — is usually closer to what you meant.

## Focus and input

Boxes are not focusable by default; Inputs are.

```rust
let input = doc.create_input("")?;
doc.append_child(form, input)?;

let button = doc.create_box()?;
doc.set_focusable(button, true)?;
```

Focus moves with Tab and with arrow keys — arrows use *spatial* distance rather than DOM
order, so focus goes to the nearest thing in that direction on screen. Hovering a focusable
node focuses it; there is no separate hover state to manage.

When an Input holds focus the arrow keys move its cursor instead, since a focused text
field has a better claim on them than navigation does.

To react to typing, use `on_input` rather than `on_key_press`:

```rust
let d = doc.clone();
doc.on_input(input, move |event| {
    let _ = d.set_text_content(status, format!("value: {}", event.value));
})?;
```

The reason is ordering. Editing the input is a *default action*, and default actions run
after listeners — so an `on_key_press` handler that calls `input_value()` reads the value
from **before** the keystroke. `on_input` fires after the edit and carries the new value.
It also bubbles, so one listener on a container observes every field inside it.

## Pseudo-states

Focus, active, and disabled styles merge on top of the base style rather than replacing it:

```rust
let mut focused = Style::new();
focused.background(Color::oklch(0.45, 0.12, 260.0));
doc.set_focus_style(button, &focused)?;
```

Since hover *is* focus, that single style covers both. Merge order is base → focus →
active → disabled, so disabled wins any conflict.

## Making it move

Transitions animate a property when its resolved value changes — including when it changes
because a pseudo-state kicked in:

```rust
use std::time::Duration;
use tuidom::animation::{Easing, TransitionConfig, TransitionProperty};

doc.set_transition(button, TransitionConfig {
    property: TransitionProperty::Background,
    duration: Duration::from_millis(150),
    easing: Easing::EaseOut,
})?;
```

Now hovering the button fades its background in rather than snapping. Nothing else changes:
you set a focus style, the engine notices the resolved value moved, and it interpolates.

While that transition runs, the renderer is active. When it finishes, the app goes fully
passive again — an idle tuidom program consumes no CPU, because there is no tick to
consume it.

## Testing what you built

Do not test by running it. tuidom ships a headless runtime that computes real layout, paints
into a real grid, and accepts simulated input, so behavior is assertable without a terminal:

```rust
use tuidom::headless::HeadlessRuntime;

let doc = Document::new()?;
let input = doc.create_input("")?;
doc.append_child(doc.root(), input)?;
doc.focus(input)?;

let mut rt = HeadlessRuntime::new(doc.clone(), 20, 3);
rt.simulate_text("hello");
rt.render()?;

assert_eq!(doc.input_value(input)?, "hello");
assert_eq!(rt.get_cell(0, 0).map(|c| c.text), Some("h".to_string()));
```

`HeadlessRuntime::new` takes the document by value, so clone it if you still need a
handle — the same rule as `run()`.

This is the supported feedback loop, and it is why the engine has a headless mode at all.
[testing](testing.md) covers the full surface.

## Where to go next

- [Architecture](architecture.md) — what the document actually is, and how a frame happens
- [`GLOSSARY.md`](GLOSSARY.md) — the vocabulary, organized by area rather than alphabetically
- `examples/demo.rs` — most of the engine exercised in one file, with a key map at the top
