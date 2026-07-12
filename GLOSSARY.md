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

**Layout Snapshot** — The document-level map of latest computed layout rectangles by `NodeId`. Layout is published by replacing the contents of this map under one lock, so readers do not observe partially updated per-node layout state. Layout positions may be negative/offscreen; rendering clips them to the grid.

**Stacking Context** — Explicit isolation marker created with `stacking_context: true`. Paint order already treats every node's subtree as an isolated unit, so the marker's behavior is to make a node eligible to become a focus context: only a stacking context can trap focus, because trapping focus in a subtree a sibling could paint over would leave the user interacting with something they cannot see. Being a stacking context never traps focus on its own.

**z-index** — Integer paint-order value for sibling subtrees. Lower values paint first; higher values paint later. DOM order is the stable tiebreaker for equal values. A descendant's `z_index` cannot escape its parent subtree.

**Position::Flow** — Default positioning mode. Node participates in normal flexbox layout.

**Position::Absolute** — Node removed from normal flow and positioned at a signed cell offset from its parent's box origin. Screen-root placement is expressed by parenting the node to the root. Published layout rectangles remain screen-absolute regardless of positioning mode.

## Styling

**Style** — User-provided style with unresolved values. Contains `StyleValue<T>` which can be `Unset`, `Set(T)`, or `Inherit`.

**ResolvedStyle** — Computed style with all values resolved. Inheritance is computed and defaults are applied; colors remain in OKLCH until render-time RGB conversion.

**PseudoState** — State that merges an extra style on top of a node's base style: focused, active, or disabled. Styles merge in the order base → focus → active → disabled, so disabled wins on conflict.

**Active** — The node currently being pressed. The engine sets it from mouse down on the hit's focus target and clears it on mouse up anywhere, so a drag off the node leaves nothing stuck pressed. `Document::set_active` drives it for activation the engine cannot see, such as keyboard presses.

**Disabled** — State that blocks interaction across a whole subtree. A node is *effectively disabled* when it or any ancestor is disabled: it cannot be focused, is skipped by tab and spatial navigation, and swallows targeted events instead of bubbling them to enabled ancestors. Each node merges its own disabled style whenever it is effectively disabled.

**Centered** — Result of a centering helper. `Even(offset)` when the leftover space divides evenly; `Uneven { low, high }` when terminal cells make exact centering impossible and the two closest offsets are equally valid.

**StyleValue** — Wrapper for style properties. `Unset` uses the document/default style, `Inherit` resolves from the parent, and `Set(value)` uses an explicit value.

**EdgeInsets** — Terminal-cell spacing for the top, right, bottom, and left edges of a node. Used by padding and margin style fields.

**Border** — A node's frame: one `BorderCharset` plus the sides it is drawn on. A border occupies real cells — layout insets the node's content and children by one cell per drawn side — so it frames content instead of painting over it. Its color is the separate `border_color` property, which follows the node's resolved foreground when unset.

**BorderCharset** — The eight characters that draw a box: four edges and four corners. The charset is the primitive; `single`, `double`, `rounded`, `thick`, and `ascii` are named constructors, not special cases. One charset per node, because a corner is drawn from the charset and a double-top/single-left corner has no coherent character.

**BorderSides** — Which sides of a border are drawn. A terminal border is always exactly one cell thick, so per-side control is presence, not width. A corner cell gets its corner character only when both adjacent sides are drawn; otherwise the one side present runs straight through it, so a top-only border is a clean rule.

**FlexDirection** — Main-axis direction for flex containers: row, column, and their reverse variants, which lay children out from the end of the main axis.

**FlexGap** — Terminal-cell spacing between flex children and flex lines. `column` is horizontal spacing and `row` is vertical spacing.

**AlignSelf** — Cross-axis alignment override for one flex item. When unset, the item follows its parent container's `AlignItems` behavior.

**FlexWrap** — Flex container wrapping behavior. `NoWrap` keeps children on one line; `Wrap` allows children to move onto additional lines when they exceed the available main-axis space; `WrapReverse` wraps the same way but stacks the resulting lines in reverse cross-axis order.

**AlignContent** — Cross-axis alignment for wrapped flex lines. Controls how multiple flex lines are packed or distributed inside a flex container.

**Custom Style Property** — Raw inline style metadata stored on `Style`. Custom properties do not inherit, do not resolve into `ResolvedStyle`, and do not affect layout or rendering.

**Attribute** — Raw string key/value metadata stored on a node. Attribute keys cannot be empty.

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

**Focus Context** — A subtree that traps focus, opened on a stacking context with `push_focus_context` and closed with `pop_focus_context`. The active context scopes everything about focus: `focused()` reports the focused node *within* it, tab order and spatial navigation search only inside it, and everything outside it is inert. The document root is a permanent focus context, so with nothing open the whole tree is in scope.

**Focus Stack** — The stack of open focus contexts, innermost last, with the permanent root context at the bottom. Each level remembers its own focused node, so restoring focus when a modal closes is just a pop rather than separate bookkeeping. Nested modals unwind in order. If a remembered node no longer exists, is no longer focusable, or has been disabled, focus is left cleared instead of jumping to a node the user never selected.

**Inert** — State that blocks interaction on everything outside the active focus context. Inert nodes cannot be focused, are skipped by tab and spatial navigation, and swallow input events rather than bubbling them. Unlike a disabled node, an inert node merges no style — content behind a modal keeps its own appearance. Focus and blur events are exempt from the swallow, since they report a focus change the engine has already made.

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

**Cell** — Single terminal cell position. Contains display content, fg/bg colors, and terminal attributes.

**CellAttrs** — The bold/italic/underline state carried by a cell's glyph. Packed on the cell — unlike on `Style`, where the three are separate properties — because nothing merges at the cell level: attributes belong to the glyph and are replaced or cleared with it.

**CellContent** — The display content stored in a cell: empty space, a grapheme glyph, or a wide-glyph continuation marker.

**WideContinuation** — Marker for the second terminal cell occupied by a width-2 glyph. It is not printed directly; the glyph head prints the visible character.

**Grid** — 2D buffer of Cells representing screen state (width × height).

**Render Cursor** — Cursor metadata produced with a rendered frame. It carries cursor position, shape, foreground-derived color, and clipped visibility without mutating grid cell content.

---

*Additional terms will be added as new concepts are introduced during development.*
