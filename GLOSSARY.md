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

**Sides** — Which sides of a node an edge treatment is drawn on. Presence, not width: every edge treatment tuidom draws — a border, a half-block edge — is either on a side or not. A border's corner cell gets its corner character only when both adjacent sides are drawn; otherwise the one side present runs straight through it, so a top-only border is a clean rule.

**Half-Block Edge** — A node's fill ending halfway into its own outermost row or column, drawn with a half block (`▀▄▌▐`) or, where two edges meet, a quadrant block (`▗▖▝▘`). It is not a border: it frames nothing and costs no layout — it repaints cells the node already owns. Its purpose is the boundary between two colors. A terminal cell is about twice as tall as it is wide, so a cell of vertical padding reads as two cells of horizontal padding; ending the fill on a half cell is what balances them.

**FlexDirection** — Main-axis direction for flex containers: row, column, and their reverse variants, which lay children out from the end of the main axis.

**FlexGap** — Terminal-cell spacing between flex children and flex lines. `column` is horizontal spacing and `row` is vertical spacing.

**AlignSelf** — Cross-axis alignment override for one flex item. When unset, the item follows its parent container's `AlignItems` behavior.

**FlexWrap** — Flex container wrapping behavior. `NoWrap` keeps children on one line; `Wrap` allows children to move onto additional lines when they exceed the available main-axis space; `WrapReverse` wraps the same way but stacks the resulting lines in reverse cross-axis order.

**AlignContent** — Cross-axis alignment for wrapped flex lines. Controls how multiple flex lines are packed or distributed inside a flex container.

**Custom Style Property** — Raw inline style metadata stored on `Style`. Custom properties do not inherit, do not resolve into `ResolvedStyle`, and do not affect layout or rendering.

**Attribute** — Raw string key/value metadata stored on a node. Attribute keys cannot be empty.

## Colors

**OKLCH** — Perceptually uniform color space (Lightness, Chroma, Hue, Alpha). Used internally for all color operations.

**Color** — A color as written in a `Style`: an expression, not a value. It can name a variable or refer to the node it is used on, neither of which means anything until it is resolved against a specific node.

**ResolvedColor** — What a `Color` evaluates to during style resolution: a concrete OKLCH color, and what `ResolvedStyle` holds. Hue is stored canonically (0–360), so two spellings of one angle are one color and share one cache entry.

**Color Variable** — Named color reference (e.g. `Color::var("--primary")`), declared on the document or on a node and in scope for that node's descendants. Redeclaring a name shadows it for the subtree. A node's own declarations resolve against its *parent's* scope, never against each other — a `HashMap` has no declaration order, and resolving against an already-concrete scope makes reference cycles impossible to write. A name nothing defines makes the whole expression unresolvable, and the property falls back to its default rather than half-applying a derivation.

**Color Derivation** — A computed color: `Color::var("--primary").darken(0.1)`, `CurrentBg.with_alpha(0.5)`. Lightness steps are absolute, not proportional, because OKLCH's lightness is perceptually uniform. `mix` blends two colors in OKLCH, taking the short way around the hue circle and borrowing the other color's hue when one is a gray — a gray has no hue, and interpolating its nominal 0° would swing the result through unrelated colors.

**CurrentBg / CurrentFg** — Color references that resolve relative to the node they are used on. They are self-referential in the two properties they are defined from, so resolution is ordered: in `background`, and in a variable declaration, they mean the *parent's* values; from `color` onward they mean the node's own. This is the only reading that is not circular.

**Effective Background** — The background a node visually sits on: its own if it has one, otherwise the nearest ancestor's, falling back to the document's declared terminal background. It is what `CurrentBg` resolves to, and it is never absent — a node deriving a color from what it sits on needs an answer even when nothing in its ancestry paints one.

**Terminal Background** — The terminal background color the document *assumes*. The real one is unknowable without querying the terminal, so it is declared rather than detected. It is the bottom of the effective-background chain and the base a translucent color blends toward over an unpainted cell. It is an assumption for color math, never a color that gets painted: an unpainted cell still emits the terminal default, so an unstyled app keeps showing the user's real background.

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
