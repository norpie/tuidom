# Glossary

Terms and concepts used throughout the tuidom codebase.

## Core Concepts

**Node** — Any item in the DOM tree. All items are nodes (Box, Text, Input, Frames, Canvas). We don't use "element" — just "node".

**NodeId** — Lightweight, `Copy` integer handle that references a node in the arena.

**Document** — The root container and public API surface. Wraps `Arc<DocumentInner>` for cheap cloning.

**DocumentInner** — Internal state holding arena, caches, event queue, renderer, etc. Behind Arc for thread-safe sharing.

**Arena** — Internal storage using DashMap. Maps `NodeId` to `NodeData`. Single source of truth for all nodes.

**NodeData** — Internal node representation. Enum with variants: Box, Text, Input, Frames, Canvas. Contains parent/children, attributes, styles.

## Layout & Positioning

**Stacking Context** — Isolated layering environment. Created explicitly (`stacking_context: true`) or implicitly by modals. Prevents z-index bleed-through between unrelated UI sections.

**Layer** — Visual stacking order within a stacking context: `content` (default) < `overlay` (dropdowns) < `modal` (modals) < `top` (toasts, drag visuals, bypasses all contexts).

**Position::Flow** — Default positioning mode. Node participates in normal flexbox layout.

**Position::Absolute** — Node positioned at specific coordinates relative to screen root, removed from flow.

## Styling

**Style** — User-provided style with unresolved values. Contains `StyleValue<T>` which can be `Set(T)` or `Inherit`.

**ResolvedStyle** — Computed style with all values resolved. Variables replaced, inheritance computed, colors in OKLCH.

**PseudoState** — Visual state affecting style: `Normal`, `Focused`, `Active`, `Disabled`.

**StyleValue** — Wrapper for style properties. Either `Set(value)` or `Inherit` (resolve from parent).

## Colors

**OKLCH** — Perceptually uniform color space (Lightness, Chroma, Hue, Alpha). Used internally for all color operations.

**Color Variable** — Named color reference (e.g., `Var("--primary")`). Cascades down tree like CSS custom properties.

**Color Derivation** — Computed color from another (e.g., `CurrentBg.darken(0.1)`, `Var("--primary").with_alpha(0.5)`).

**CurrentBg / CurrentFg** — Special color references that resolve to the current node's background/foreground.

**Rgb** — Final color format (Red, Green, Blue) sent to terminal. Converted from OKLCH only at render time.

## Events

**Event** — Input or system notification (keyboard, mouse, focus, animation completion). Carries data and propagation state.

**Listener** — User-provided handler function. Wrapped in `Box<dyn Fn(&Event) + Send>`.

**ListenerHandle** — Opaque handle for removing a registered listener. Contains unique `ListenerId`.

**Propagation** — Event flow through DOM tree. Target phase (fires on target node) → Bubble phase (fires on ancestors, root-ward).

**Event Loop** — Async task that dequeues events, builds propagation path, fires handlers sequentially.

## Focus & Selection

**Focus Context** — Focus state for a stacking context. Tracks currently focused node and focus stack for modal nesting.

**Focus Stack** — Stack of previously focused nodes. Pushed on modal open, popped on modal close to restore focus.

**Spatial Navigation** — Arrow key focus movement based on visual distance (edge-to-edge) rather than DOM order.

**Selection Boundary** — Container marked `selection_boundary: true`. Mouse drag selection respects boundaries, doesn't bleed across.

**SelectionPoint** — Position in selection range: which Text node + character offset.

## Animation

**Transition** — Property animation triggered by value change. One per (NodeId, PropertyName).

**Keyframe Animation** — Multi-step animation with defined frames at percentages (0%, 50%, 100%, etc.).

**Frames Node** — Node type that cycles through text content on a timer (for spinners, ASCII animations).

**AnimatableProperty** — Type-safe enum of properties that can interpolate (numeric, colors). Non-animatable properties (e.g., border style) cause compile errors.

**Easing** — Interpolation curve (Linear, EaseIn, EaseOut, EaseInOut, CubicBezier).

## Rendering

**Cell** — Single terminal cell position. Contains display content plus fg/bg colors.

**CellContent** — The display content stored in a cell: empty space, a grapheme glyph, or a wide-glyph continuation marker.

**WideContinuation** — Marker for the second terminal cell occupied by a width-2 glyph. It is not printed directly; the glyph head prints the visible character.

**Grid** — 2D buffer of Cells representing screen state (width × height).

---

*Additional terms will be added as new concepts are introduced during development.*
