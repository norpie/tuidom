# Animation

Three mechanisms, for three different jobs: **transitions** react to a value changing,
**keyframe animations** play a defined sequence, and **frames nodes** cycle text content on
a timer. They compose, and the rules for how are below.

Terms in **bold** are defined in the [glossary](GLOSSARY.md#animation).

## Transitions

A **transition** animates a property when its *resolved* value changes:

```rust
use std::time::Duration;
use tuidom::animation::{Easing, TransitionConfig, TransitionProperty};

doc.set_transition(button, TransitionConfig {
    property: TransitionProperty::Background,
    duration: Duration::from_millis(150),
    easing: Easing::EaseOut,
})?;
```

One transition per node/property pair. Setting another for the same property replaces it.

The important word is **resolved**. A transition does not care *why* a value changed, so an
explicit `set_style` and a pseudo-state change animate identically:

```rust
doc.set_focus_style(button, &focused)?;    // hovering now fades, not snaps
```

That is the whole reason hover effects need no special support. You set a focus style, the
merged resolved value moves, and the transition notices.

### What can and cannot animate

Animatable: opacity, background, foreground, border color, absolute position offsets,
width, height, padding, margin.

Not animatable: discrete values — border style, text content, booleans. There is nothing
between `single` and `double` to show.

The subtler case is properties that are animatable but whose *current state* is not
interpolable. These **snap** rather than transitioning:

- an unset background → a set one (there is no color to start from)
- `Length::Auto` → a fixed size (`auto` has no value until layout runs)
- `Position::Flow` → `Position::Absolute`
- a unit change: cells → percent

This mirrors CSS, which cannot animate `auto` for the same reason. Snapping is the honest
outcome; the alternative is inventing a start value nobody specified.

### How values interpolate

Colors interpolate in **OKLCH**, so a fade between two colors passes through the colors
between them perceptually rather than through the muddy midpoints RGB interpolation
produces.

Cell values interpolate **fractionally and round only at application**. A width animating
from 10 to 13 cells passes through 10.4 and 11.7 internally and rounds on the way to layout.
Rounding at each step instead would quantize the motion — a short animation across a few
cells would visibly stair-step, and a slow one would stall on repeated identical values.

### Interruption

Interrupting a transition hands over the **currently displayed value**, so a retarget never
jumps back to the start.

A pure reversal gets special treatment: it is shortened to the share of the duration
matching the distance it actually has to cover. Hover-out at 20% into a fade-in takes 20% of
the duration, not the full time — so flicking the mouse across a row of buttons does not
leave a queue of slow fades finishing after the pointer has gone.

### Layout-affecting transitions

Position, size, padding, and margin feed the layout engine **per animation frame while in
flight** — and only then. An idle node costs nothing extra; an animating one re-lays-out.

Worth knowing before you animate the width of something with a large subtree.

### Completion

```rust
doc.on_transition_end(node, |event| { /* ... */ })?;
```

Node-targeted and bubbling. **Interrupted transitions fire nothing**, and neither do removed
nodes — the event means "this reached its target", not "this stopped".

## Keyframe animations

For a defined sequence rather than a reaction:

```rust
use tuidom::animation::{AnimatableProperty, AnimationDirection, Easing, KeyframeAnimation};

let pulse = KeyframeAnimation::new(Duration::from_millis(900))
    .keyframe(0.0,   [AnimatableProperty::Opacity(1.0)])
    .keyframe(50.0,  [AnimatableProperty::Opacity(0.4)])
    .keyframe(100.0, [AnimatableProperty::Opacity(1.0)])
    .easing(Easing::EaseInOut)
    .infinite();

let handle = doc.animate(node, pulse)?;
```

Percentages are 0–100 and clamped. Easing applies **per segment**, like CSS — not once
across the whole run.

`from_to` is the shorthand for the common two-state case:

```rust
let fade = KeyframeAnimation::from_to(
    Duration::from_millis(200),
    [AnimatableProperty::Opacity(0.0)],
    [AnimatableProperty::Opacity(1.0)],
);
```

### Typed values

**AnimatableProperty** has one variant per animatable property — `Opacity(f64)`,
`Background(Color)`, `Position { x, y }`, `Width(Length)`, `Padding(EdgeInsets)`, and so on.

This is a deliberate type-level constraint: a keyframe holding a non-animatable property is
**unrepresentable**. You cannot write a keyframe that animates `border`, so the engine never
has to silently ignore one, and you never have to wonder why nothing happened.

Color values are `Color` *expressions*, evaluated once against the node's scope when
`animate` is called — so `Color::var("--accent")` works and resolves at the right node.

### Implicit endpoints

A property with no explicit 0% or 100% keyframe uses the node's **underlying resolved
value** as that endpoint. Animating from wherever it currently is needs no boilerplate.

### Iteration and direction

```rust
.iterations(3)                                   // finite
.infinite()                                      // or not
.direction(AnimationDirection::Alternate)        // Normal, Reverse, Alternate
```

### Control

`animate` returns an **`AnimationHandle`**, which is how you reach a running animation:

```rust
doc.pause_animation(handle);     // values freeze, and drive no frames
doc.resume_animation(handle);    // elapsed time excludes the pause
doc.cancel_animation(handle);    // no end event fires
```

Each returns `bool` — whether the handle still referred to a live animation.

A paused animation does not count as active, so pausing every animation lets the document go
fully passive.

### Events

```rust
doc.on_animation_end(node, |event| { /* ... */ })?;
doc.on_animation_iteration(node, |event| { /* ... */ })?;
```

Both node-targeted and bubbling. Iteration boundaries crossed within a single frame
**coalesce** into one event carrying the latest count — a fast animation on a slow frame
cannot flood you with events nobody can act on individually.

### When an animation ends

The animation is **removed**, and the node returns to its underlying style. It does not hold
its final frame.

To keep the end state, set it as the node's style from the end handler:

```rust
let d = doc.clone();
doc.on_animation_end(node, move |_| {
    let _ = d.update_style(node, |s| s.opacity(0.0));
})?;
```

This is the opposite of CSS's `animation-fill-mode: forwards`, and it is the more honest
default: an animation that permanently mutates appearance without touching style leaves the
node's style lying about what it looks like.

### Animations beat transitions

Keyframe values apply **on top of** any running transition for the same property.
Animations win on conflict. When the animation is removed, the transition underneath
resumes control.

## Frames nodes

For content-based animation — spinners, ASCII art — where nothing is being interpolated:

```rust
let spinner = doc.create_frames(
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    Duration::from_millis(80),
)?;
```

The current frame is a **function of elapsed time**, not stored state. A flip is nothing but
the clock passing a boundary — there is no per-flip mutation, no timer task, and no state to
get out of sync.

Two behaviors follow from the design:

**Measured on the largest frame.** Cycling never reflows the content around it, so a spinner
whose frames differ in width does not make its neighbours jitter.

**Self-paced.** A lone frames node paces rendering at *its own* interval rather than the
animation tick — a 100ms spinner repaints ten times a second, not sixty. A single frame, or
a zero interval, drives no rendering at all.

```rust
doc.set_frames_interval(spinner, Duration::from_millis(120))?;
doc.current_frame(spinner)?;      // which frame is showing right now
```

## The animation tick

Transitions and keyframe animations render on a document-wide tick, ~60fps by default:

```rust
doc.set_animation_fps(Some(30.0));
doc.set_animation_fps(None);        // unpaced — renders as fast as the runtime allows
```

`None` removes pacing entirely. That is a **stress-test mode, not a default** — it will
render as fast as it can and burn a core doing it.

Frames nodes schedule by their own flip intervals instead, and `WhenScrolling` scrollbars by
their fade deadlines. See [scrolling](scrolling.md#whenscrolling).

**The tick exists only while something animates.** An idle document is fully passive — not
ticking and skipping redraws, but blocked. Paused animations do not count, so a document
whose animations are all paused is as idle as one with none.

## Easing

`Linear`, `EaseIn`, `EaseOut`, `EaseInOut`, and CSS-style `CubicBezier(x1, y1, x2, y2)`.

```rust
Easing::CubicBezier(0.34, 1.56, 0.64, 1.0)      // overshoot
```

X coordinates are clamped to 0–1 so progress stays a function of time, exactly as CSS's
`cubic-bezier()` requires. Y is unclamped, which is what lets a curve overshoot and settle
back.

Transitions ease their whole run; keyframe animations ease each segment.

## Where to go next

- [Styling](styling.md#pseudo-states) — the pseudo-states that trigger transitions
- [Events](events.md) — how animation events dispatch
- [Architecture](architecture.md#passive-by-default) — why an idle document costs nothing
