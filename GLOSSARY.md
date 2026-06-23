# Glossary

Terms and concepts used throughout the tuidom codebase.

## Core Concepts

**Node** — Any item in the DOM tree. All items are nodes (Box, Text, Input, Frames, Canvas). We don't use "element" — just "node".

**NodeId** — Lightweight, `Copy` integer handle that references a node in one document's arena. Encodes internal document identity so handles from different documents do not collide.

**Document** — The owner and public API surface. Wraps `Arc<DocumentInner>` for cheap cloning and owns one permanent root node.

**Root Node** — The permanent top-level Box node created by `Document::new()`. It is the entry point for layout, rendering, and current runtime event dispatch; it always exists and cannot be reparented or removed.

**DocumentInner** — Internal state holding the arena, document/root ids, caches, event/listener state, animation state, layout snapshots, notifications, renderer-facing state, and lifecycle flags. Behind Arc for thread-safe sharing.

**Arena** — Internal storage using DashMap. Maps `NodeId` to `NodeData`. Single source of truth for all nodes.

**NodeData** — Internal node representation. Contains the node kind, parent/children, attributes, style, cached resolved style, and animation/transition metadata. Computed layout is published separately as a document-level layout snapshot.

## Layout & Positioning

**Layout Snapshot** — The document-level map of latest computed layout rectangles by `NodeId`. Layout is published by replacing the contents of this map under one lock, so readers do not observe partially updated per-node layout state.

**Stacking Context** — Explicit isolation marker created with `stacking_context: true`. Used by modal/focus policy and future positioning behavior; paint order already treats every node's subtree as an isolated unit.

**z-index** — Integer paint-order value for sibling subtrees. Lower values paint first; higher values paint later. DOM order is the stable tiebreaker for equal values. A descendant's `z_index` cannot escape its parent subtree.

**Position::Flow** — Default positioning mode. Node participates in normal flexbox layout.

**Position::Absolute** — Node positioned at specific coordinates relative to screen root, removed from flow.

## Styling

**Style** — User-provided style with unresolved values. Contains `StyleValue<T>` which can be `Unset`, `Set(T)`, or `Inherit`.

**ResolvedStyle** — Computed style with all values resolved. Inheritance is computed and defaults are applied; colors remain in OKLCH until render-time RGB conversion.

**PseudoState** — Visual state affecting style: `Normal`, `Focused`, `Active`, `Disabled`.

**StyleValue** — Wrapper for style properties. `Unset` uses the document/default style, `Inherit` resolves from the parent, and `Set(value)` uses an explicit value.

## Colors

**OKLCH** — Perceptually uniform color space (Lightness, Chroma, Hue, Alpha). Used internally for all color operations.

**Color Variable** — Named color reference (e.g., `Var("--primary")`). Cascades down tree like CSS custom properties.

**Color Derivation** — Computed color from another (e.g., `CurrentBg.darken(0.1)`, `Var("--primary").with_alpha(0.5)`).

**CurrentBg / CurrentFg** — Special color references that resolve to the current node's background/foreground.

**Rgb** — Final color format (Red, Green, Blue) sent to terminal. Converted from OKLCH only at render time.

## Events

**Event** — Input or system notification dispatched to handlers. Carries event-specific data and, for targeted events, propagation state.

**Listener** — User-provided handler function. Internally stored with a stable id and shared callback so dispatch can snapshot listeners before invocation.

**ListenerHandle** — Opaque handle for removing a registered listener. Contains a document-scoped listener id so handles from different documents do not collide.

**Propagation** — Event flow through DOM tree. Target phase (fires on target node) → Bubble phase (fires on ancestors, root-ward).

**Event Loop** — Async runtime that waits for terminal events, document notifications, animation state, and shutdown. It dispatches terminal events to listeners and renders when needed.

## Focus & Selection

**Focus Context** — Focus state for a stacking context. Tracks currently focused node and focus stack for modal nesting.

**Focus Stack** — Stack of previously focused nodes. Pushed on modal open, popped on modal close to restore focus.

**Spatial Navigation** — Arrow key focus movement based on visual distance (edge-to-edge) rather than DOM order.

**Selection Boundary** — Container marked `selection_boundary: true`. Mouse drag selection respects boundaries, doesn't bleed across.

**SelectionPoint** — Position in selection range: which Text node + character offset.

## Animation

**Transition** — Property animation triggered by value change. One per node/property pair.

**Keyframe Animation** — Multi-step animation with defined frames at percentages (0%, 50%, 100%, etc.).

**Frames Node** — Node type that cycles through text content on a timer (for spinners, ASCII animations).

**AnimatableProperty** — Type-safe enum of properties that can interpolate. Non-animatable properties (e.g., border style) cause compile errors.

**Easing** — Interpolation curve (Linear, EaseIn, EaseOut, EaseInOut, CubicBezier).

## Rendering

**Cell** — Single terminal cell position. Contains display content plus fg/bg colors.

**CellContent** — The display content stored in a cell: empty space, a grapheme glyph, or a wide-glyph continuation marker.

**WideContinuation** — Marker for the second terminal cell occupied by a width-2 glyph. It is not printed directly; the glyph head prints the visible character.

**Grid** — 2D buffer of Cells representing screen state (width × height).

---

*Additional terms will be added as new concepts are introduced during development.*
