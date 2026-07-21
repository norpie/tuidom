# Glossary

Terms and concepts used throughout the tuidom codebase. Entries define; the guides in this
directory explain — see [`README.md`](README.md) for how the two divide.

## Core Concepts

*Explained in [architecture](architecture.md).*

**Node** — Any item in the DOM tree (Box, Text, Input, Frames, Canvas). We don't use "element" — just "node".

**NodeId** — Lightweight, `Copy` integer handle referencing a node in one document's arena. See [the document model](architecture.md#the-document-model).

**Document** — The owner and public API surface; wraps `Arc<DocumentInner>`. See [the document model](architecture.md#the-document-model).

**Root Node** — The permanent top-level Box created by `Document::new()`, and the entry point for layout, paint, and document-level dispatch. See [the root node](architecture.md#the-root-node).

**DocumentInner** — Internal state behind the `Arc`: the arena, caches, listener and animation state, layout snapshot, and lifecycle flags. See [where state lives](architecture.md#where-state-lives).

**Arena** — The document's node storage, a `DashMap` from `NodeId` to `NodeData`. Single source of truth for all nodes. See [where state lives](architecture.md#where-state-lives).

**NodeData** — A node's stored representation: kind, parent, children, attributes, style, and cached resolved style. Computed layout is *not* here — see [layout is published, not stored](architecture.md#layout-is-published-not-stored).

## Layout & Positioning

*Flex, sizing, and positioning are explained in [layout](layout.md); the scrolling entries
in [scrolling](scrolling.md), and stacking in [rendering](rendering.md).*

**Layout Snapshot** — The document-level map of latest computed layout by `NodeId`: each node's rectangle plus its maximum scroll per axis, from one taffy pass. See [layout is published, not stored](architecture.md#layout-is-published-not-stored) and [reading computed layout](layout.md#reading-computed-layout).

**Overflow** — Per-axis property for content exceeding a node's box: `Visible`, `Scroll`, or `Clip`. A `Scroll`/`Clip` axis also drops the content-size floor in layout. See [making something scroll](scrolling.md#making-something-scroll).

**Scroll Offset** — How far a scroll container's content is shifted, in cells. Runtime state like focus, not style. See [scroll offset is runtime state](scrolling.md#scroll-offset-is-runtime-state).

**Scrollport** — The padding box of a scrolling or clipping node, bounding its descendants' painting and hit-testing per clipped axis. See [the scrollport](scrolling.md#the-scrollport).

**Scroll Chaining** — Wheel routing to the nearest ancestor scrollable on the wheel's axis that can still move in its direction. See [wheel routing and chaining](scrolling.md#wheel-routing-and-chaining).

**Stacking Context** — Marker set with `stacking_context: true`, making a node eligible to trap focus. Paint isolation is already universal, so that eligibility is all it grants. See [stacking contexts](rendering.md#stacking-contexts).

**z-index** — Integer paint-order value for sibling subtrees; DOM order breaks ties. A descendant's value cannot escape its parent subtree. See [paint order](rendering.md#paint-order).

**Position::Flow** — Default positioning mode. Node participates in normal flex layout. See [positioning](layout.md#positioning).

**Position::Absolute** — Node removed from flow and offset by signed cells from its parent's box origin. See [positioning](layout.md#positioning).

## Virtualization

*Explained in [virtualization](virtualization.md).*

**Virtualization** — Materializing only the visible window of a large collection. Unlike culling, which is paint-only, it decides what exists in the DOM at all. The engine provides the math; downstream owns every node. See [when you actually need it](virtualization.md#when-you-actually-need-it).

**Spacer** — An empty Box holding open the space of every unmaterialized item on one side of the window, so the container's measured content size stays the true total. Must set `flex_shrink(0.0)`. See [the spacer pattern](virtualization.md#the-spacer-pattern).

**Window** — The item range a virtualized collection should have in the DOM, plus the two spacer extents standing in for everything else. See [uniform items](virtualization.md#uniform-items).

**Overscan** — Items materialized beyond each edge of the visible range, measured in items rather than cells. See [uniform items](virtualization.md#uniform-items).

**Stride** — Cells from one item's start to the next: its extent plus any flex gap. Virtualized items must not flex. See [uniform items](virtualization.md#uniform-items).

**Measurement Cache** — Extents for variably sized items, mixing an estimate with recorded measurements over a Fenwick tree so queries stay logarithmic. One axis. See [variable-sized items](virtualization.md#variable-sized-items).

**Anchoring** — Absorbing a measurement's extent change into the scroll offset when the measured item is above the viewport, so on-screen content stays visually pinned. See [anchoring](virtualization.md#anchoring).

## Styling

*Explained in [styling](styling.md); the flex and centering entries in [layout](layout.md).*

**Style** — A node's user-provided style, holding a `StyleValue<T>` per property. See [style is a struct, not a builder](styling.md#style-is-a-struct-not-a-builder).

**StyleValue** — A property's three states: `Unset` (use the default), `Set(v)`, or `Inherit` (take the parent's resolved value). See [three states per property](styling.md#three-states-per-property-stylevalue).

**ResolvedStyle** — A `Style` with every value collapsed to a concrete one: inheritance walked, defaults applied, colors still in OKLCH. See [what the engine actually uses](styling.md#resolvedstyle-what-the-engine-actually-uses).

**PseudoState** — An extra style merged on top of a node's base style: focused, active, or disabled, in that order. See [pseudo-states](styling.md#pseudo-states).

**Active** — The node currently being pressed. See [active](styling.md#active).

**Disabled** — State that blocks interaction across a whole subtree, and is inherited as *effectively disabled* by descendants. See [disabled](styling.md#disabled).

**Centered** — Result of a centering helper: `Even(offset)`, or `Uneven { low, high }` when terminal cells make exact centering impossible. See [centering in discrete cells](layout.md#centering-in-discrete-cells).

**EdgeInsets** — Terminal-cell spacing for a node's top, right, bottom, and left edges; used by `padding` and `margin`. See [cells are not square](layout.md#cells-are-not-square).

**Border** — A node's frame: one `BorderCharset` plus the `Sides` it is drawn on. Occupies real cells. See [borders](styling.md#borders).

**BorderCharset** — The eight characters that draw a box — four edges, four corners. `single`, `double`, `rounded`, `thick`, and `ascii` are named constructors over it. See [borders](styling.md#borders).

**Sides** — Which sides of a node an edge treatment is drawn on. Presence, not width. See [sides are presence, not width](styling.md#sides-are-presence-not-width).

**Half-Block Edge** — A node's fill ending halfway into its outermost row or column, drawn with `▀▄▌▐` or `▗▖▝▘`. Not a border: it frames nothing and costs no layout. See [half-block edges](styling.md#half-block-edges).

**FlexDirection** — Main-axis direction for a flex container: row, column, or their reverse variants. See [flex containers](layout.md#flex-containers).

**FlexGap** — Spacing between flex children and flex lines. `row` is vertical, `column` is horizontal. See [flex containers](layout.md#flex-containers).

**AlignSelf** — Cross-axis alignment override for one flex item; a type alias for `AlignItems`. See [alignment](layout.md#alignment).

**FlexWrap** — `NoWrap`, `Wrap`, or `WrapReverse`, which stacks wrapped lines in reverse cross-axis order. See [alignment](layout.md#alignment).

**AlignContent** — Cross-axis alignment for wrapped flex lines. See [alignment](layout.md#alignment).

**Custom Style Property** — Raw string metadata on a `Style`. Does not inherit, resolve, or affect rendering. See [metadata that does nothing](styling.md#metadata-that-does-nothing).

**Attribute** — Raw string key/value metadata on a node; keys cannot be empty. See [metadata that does nothing](styling.md#metadata-that-does-nothing).

## Colors

*Explained in [colors](colors.md).*

**OKLCH** — Perceptually uniform color space (Lightness, Chroma, Hue, Alpha), used for every color operation. See [OKLCH, and why not RGB](colors.md#oklch-and-why-not-rgb).

**Color** — A color as written in a `Style`: an expression, not a value. It may name a variable or refer to the node it is used on. See [a `Color` is an expression](colors.md#a-color-is-an-expression).

**ResolvedColor** — What a `Color` evaluates to during resolution: a concrete OKLCH color with canonical hue. See [a `Color` is an expression](colors.md#a-color-is-an-expression).

**Color Variable** — A named color (`Color::var("--primary")`) declared on the document or a node, in scope for that node's descendants. See [color variables](colors.md#color-variables).

**Color Derivation** — A computed color, such as `Color::var("--primary").darken(0.1)`. See [derivations](colors.md#derivations).

**CurrentBg / CurrentFg** — Color references resolving relative to the node they are used on, under a fixed resolution order. See [`CurrentBg` and `CurrentFg`](colors.md#currentbg-and-currentfg).

**Effective Background** — The background a node visually sits on; what `CurrentBg` resolves to, and never absent. See [effective background](colors.md#effective-background).

**Terminal Background** — The terminal background the document *assumes*, since the real one is unknowable. Never painted. See [terminal background is an assumption](colors.md#terminal-background-is-an-assumption).

**Rgb** — Final color format sent to the terminal, converted from OKLCH at render time. See [Rgb, and when conversion happens](colors.md#rgb-and-when-conversion-happens).

## Events

*Explained in [events](events.md).*

**Event** — Input or system notification dispatched to handlers. Carries event-specific data and, for targeted events, propagation state. See [two kinds of event](events.md#two-kinds-of-event).

**Listener** — A user-provided handler. `Fn + Send + Sync + 'static`, synchronous, and caught on panic. See [listener handles](events.md#listener-handles).

**ListenerHandle** — Opaque, document-scoped handle for removing a registered listener. See [listener handles](events.md#listener-handles).

**Propagation** — Event flow through the tree: target phase, then bubble phase rootward. There is no capture phase. See [propagation](events.md#propagation).

**Event Loop** — The async runtime waiting on terminal events, document notifications, animation state, and shutdown. See [how a frame happens](architecture.md#how-a-frame-happens).

**Input Event** — An `Input` node's value changed, reported by `on_input` after the key press default action that edited it. Targets the input and bubbles. See [events that report what the engine did](events.md#events-that-report-what-the-engine-did).

**Post-Frame Event** — Document-level notification that a frame finished, carrying its metrics. See [post-frame](events.md#post-frame).

**Window Focus** — Whether the *terminal window* holds OS focus, not DOM focus. Never moves the focused node. See [window focus](events.md#window-focus).

**Input Coalescing** — Collapsing adjacent runs of pointer movement and resize in an already-queued batch; the queue's only bound, since it applies no backpressure. See [input coalescing](events.md#input-coalescing).

**Panic Restore** — The process-wide panic hook that restores terminal modes before a crash reaches the user, chaining to the previously installed hook. Never used for caught callback panics. See [panics and terminal restore](events.md#panics-and-terminal-restore).

**Bell** — `doc.bell()`, emitted as `\x07` by the next flush rather than written when called. Coalesced, and schedules a frame. See [the bell](events.md#the-bell).

## Focus & Selection

*Explained in [focus and selection](focus-and-selection.md).*

**Focus Context** — A subtree that traps focus, opened on a stacking context with `push_focus_context`. Scopes `focused()`, tab order, and navigation; everything outside is inert. See [focus contexts](focus-and-selection.md#focus-contexts).

**Focus Stack** — The stack of open contexts, innermost last, each remembering its own focused node so a pop restores focus. See [the focus stack](focus-and-selection.md#the-focus-stack).

**Inert** — State that blocks interaction outside the active focus context. Unlike disabled, it merges no style. See [inert versus disabled](focus-and-selection.md#inert-versus-disabled).

**Spatial Navigation** — Arrow key focus movement by visual edge-to-edge distance rather than DOM order. See [keyboard navigation](focus-and-selection.md#keyboard-navigation).

**Selection Boundary** — A container marked `selection_boundary: true`, confining a drag to the boundary it started in. An Input is an implicit one. See [selection boundaries](focus-and-selection.md#selection-boundaries).

**SelectionPoint** — A position in a selection: a Text node plus a grapheme-aligned byte offset. Content-addressed, so scrolling never invalidates it. See [`SelectionPoint` is content-addressed](focus-and-selection.md#selectionpoint-is-content-addressed).

**Selection Colors** — `selection_bg` / `selection_fg`; unset means reverse video. See [selection colors](focus-and-selection.md#selection-colors).

## Animation

*Explained in [animation](animation.md).*

**Transition** — Property animation triggered by a change in a node's *merged* resolved value, so explicit style changes and pseudo-state changes animate alike. One per node/property pair. See [transitions](animation.md#transitions).

**Keyframe Animation** — Multi-step animation started with `doc.animate`: typed values at percentages, played for an iteration count in a direction. See [keyframe animations](animation.md#keyframe-animations).

**Frames Node** — Node type cycling through text content on a timer. The current frame is a function of elapsed time, not stored state. See [frames nodes](animation.md#frames-nodes).

**AnimatableProperty** — Typed keyframe value, one variant per animatable property. A non-animatable property is unrepresentable. See [typed values](animation.md#typed-values).

**Easing** — Interpolation curve: `Linear`, `EaseIn`, `EaseOut`, `EaseInOut`, and CSS-style `CubicBezier(x1, y1, x2, y2)`. See [easing](animation.md#easing).

**Animation Tick** — The pacing of animation-driven frames, ~60fps by default, existing only while something animates. See [the animation tick](animation.md#the-animation-tick).

## Rendering

*Explained in [rendering](rendering.md); the scrollbar entries in [scrolling](scrolling.md).*

**Cell** — A single terminal cell position: display content, fg/bg colors, and attributes. Crate-private; `ScreenCell` is the public view. See [the grid](rendering.md#the-grid).

**CellAttrs** — The bold/italic/underline state carried by a cell's glyph, packed on the cell rather than kept separate as on `Style`. See [the grid](rendering.md#the-grid).

**CellContent** — What a cell holds: empty space, a grapheme glyph, or a wide-glyph continuation marker. See [the grid](rendering.md#the-grid).

**WideContinuation** — Marker for the second cell of a width-2 glyph. Never printed directly; the glyph head prints the character. See [the grid](rendering.md#the-grid).

**Grid** — The 2D buffer of cells a frame is painted into, carrying the active clip. See [the grid](rendering.md#the-grid).

**Culling** — Render-time drop of a node whose translated rect lies wholly outside its clip. Paint-only: culled nodes stay in the DOM and in layout. See [culling](scrolling.md#culling).

**Scrollbar** — Overlay strips on a scroll container's last viewport column and row, occupying no layout. See [scrollbars](scrolling.md#scrollbars).

**WhenScrolling** — The auto-hiding scrollbar show mode: appears on offset change, holds, then fades. Scheduled rather than polled. See [`WhenScrolling`](scrolling.md#whenscrolling).

**Render Cursor** — Cursor metadata produced alongside a frame — position, shape, color, clipped visibility — without mutating cell content. See [the cursor](rendering.md#the-cursor).

---

*Additional terms will be added as new concepts are introduced during development.*
