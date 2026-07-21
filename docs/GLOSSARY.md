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

**Arena** — The document's node storage, a `DashMap` from `NodeId` to `NodeData`. Single source of truth for all nodes.

**NodeData** — A node's stored representation: kind, parent, children, attributes, style, and cached resolved style. Computed layout is *not* here — see [layout is published, not stored](architecture.md#layout-is-published-not-stored).

## Layout & Positioning

*Flex, sizing, and positioning are explained in [layout](layout.md); the scrolling and
stacking entries below still carry their own reasoning.*

**Layout Snapshot** — The document-level map of latest computed layout by `NodeId`: each node's rectangle plus its maximum scroll per axis, from one taffy pass. See [layout is published, not stored](architecture.md#layout-is-published-not-stored) and [reading computed layout](layout.md#reading-computed-layout).

**Overflow** — Per-axis style property for content exceeding a node's box: `Visible` (default) spills, `Scroll` clips and scrolls, `Clip` clips without scrolling. A `Scroll` or `Clip` axis also drops the automatic content-size floor in layout, so the container may be smaller than its content — which is what makes overflow possible in the first place.

**Scroll Offset** — How far a scroll container's content is shifted, in terminal cells. Runtime state like focus, not style: keyed by `NodeId` on the document, driven by wheel routing and `scroll_to`/`scroll_by`, and clamped to content minus viewport as measured by the same layout pass that produced the rects — a relayout that shrinks content re-clamps stored offsets. The offset is applied at paint time as a translation of the container's descendants; layout is scroll-invariant, so a wheel tick never re-runs taffy.

**Scrollport** — The padding box of a scrolling or clipping node: the region its descendants' painting and hit-testing are bounded to, per clipped axis. Content slides through the padding but never over the border, and an axis left `Visible` stays unbounded — the clip bounds axes independently.

**Scroll Chaining** — Wheel routing walks from the hit node rootward to the nearest container scrollable on the wheel's axis that can still move in the wheel's direction. A container at the end of its range passes the wheel to the ancestor beyond it, and inert or disabled containers are skipped the same way they swallow the wheel event itself.

**Stacking Context** — Explicit isolation marker created with `stacking_context: true`. Paint order already treats every node's subtree as an isolated unit, so the marker's behavior is to make a node eligible to become a focus context: only a stacking context can trap focus, because trapping focus in a subtree a sibling could paint over would leave the user interacting with something they cannot see. Being a stacking context never traps focus on its own.

**z-index** — Integer paint-order value for sibling subtrees. Lower values paint first; higher values paint later. DOM order is the stable tiebreaker for equal values. A descendant's `z_index` cannot escape its parent subtree.

**Position::Flow** — Default positioning mode. Node participates in normal flex layout. See [positioning](layout.md#positioning).

**Position::Absolute** — Node removed from flow and offset by signed cells from its parent's box origin. See [positioning](layout.md#positioning).

## Virtualization

**Virtualization** — Materializing only the visible window of a large collection. Culling is paint-only — everything culled is still laid out — whereas virtualization decides what exists in the DOM at all. The engine does not virtualize on its own: the `virtualize` module provides the range math and window diffing, and downstream owns every node, the way a browser sits under a virtualized list rather than being one.

**Spacer** — An empty Box holding open the space of every unmaterialized item on one side of the window. With a leading and a trailing spacer, a scroll container's measured content size is the true total, so scroll clamping, scrollbar geometry, and wheel routing stay correct with nothing virtual about them. A spacer must set `flex_shrink(0.0)`: an empty box has no content floor, so default flex shrink would collapse it — and with it the scroll range it exists to hold open.

**Window** — The item range a virtualized collection should have in the DOM: the items covering the scrollport (straddlers included) plus overscan, together with the two spacer extents that stand in for everything else. Produced by the uniform math or the measurement cache; diffed against the materialized range by the `Virtualizer`.

**Overscan** — Items materialized beyond each edge of the visible range, so a small scroll reveals rows that already exist instead of waiting for materialization. Measured in items, not cells.

**Stride** — Cells from one item's start to the next: the item's extent plus any flex gap between items. The uniform window math is built on the stride, which is why virtualized items must not flex-grow or shrink.

**Measurement Cache** — Extents for variably sized items: an estimate for items not yet measured, recorded measurements for the rest, answering offset and window queries over the mix. Backed by a Fenwick tree over the deltas from the estimate, so queries stay logarithmic however much of the collection has been measured. One axis, like all virtualization math — a 2D grid runs it per axis.

**Anchoring** — Absorbing a measurement's extent change into the scroll offset when the measured item lies above the viewport, so the content on screen stays visually pinned instead of shifting under the user. `record` returns the signed change as the compensation to apply.

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

**Event** — Input or system notification dispatched to handlers. Carries event-specific data and, for targeted events, propagation state.

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

**Transition** — Property animation triggered by a change in a node's *merged* resolved value — an explicit style change and a pseudo-state change (focus, active, disabled) animate alike. One per node/property pair. An interrupted transition hands over its currently displayed value, so a retarget never jumps; a pure reversal gets only the share of the duration matching the distance it covers. States with no interpolable value — an unset background, an `Auto` size, a `Flow` position, a cells↔percent unit change — snap instead of transitioning, the way CSS cannot animate `auto`. Completion fires a bubbling transition-end event; interrupted transitions and removed nodes fire none. Colors interpolate in OKLCH; cell values interpolate fractionally and round only at application. Layout-affecting properties (position, size, padding, margin) feed the layout engine per animation frame while in flight — and only then.

**Keyframe Animation** — Multi-step animation started with `doc.animate`: keyframes at percentages holding typed [AnimatableProperty] values, played over a duration for an iteration count (finite or infinite) in a direction (normal, reverse, alternate). Easing applies per keyframe segment, like CSS. A property missing an explicit 0%/100% keyframe uses the node's underlying resolved value as the implicit endpoint. Values apply on top of any running transitions — animations win on conflict — and when the animation ends it is removed, returning the node to its underlying style; a handler holds the final state by setting it as the node's style from the end event. The returned `AnimationHandle` pauses (values freeze, no frames driven), resumes (elapsed time excludes the pause), and cancels (no end event). End and iteration events bubble from the animated node; iteration boundaries crossed within one frame coalesce into a single event carrying the latest count.

**Frames Node** — Node type that cycles through text content on a timer (for spinners, ASCII animations). The current frame is a function of elapsed time, not stored state — a flip is nothing but the clock passing a boundary. Measured on the largest frame, so cycling never reflows the content around it. A lone frames node paces rendering at its own interval rather than the animation tick, so a 100ms spinner repaints ten times a second; a single frame or zero interval drives no rendering at all.

**AnimatableProperty** — Typed keyframe value, one variant per animatable property (opacity, colors, absolute position offsets, width/height, padding, margin). A non-animatable property — border style, text content, a boolean — is unrepresentable, so a keyframe cannot be written that the engine would have to ignore. Color values are `Color` expressions evaluated once against the node's scope when `animate` is called.

**Easing** — Interpolation curve: Linear, EaseIn, EaseOut, EaseInOut, and CSS-style `CubicBezier(x1, y1, x2, y2)`. Transitions ease their whole run; keyframe animations ease each segment.

**Animation Tick** — The pacing of animation-driven frames: transitions and keyframe animations render on a document-wide tick (default ~60fps, `set_animation_fps`), where `None` removes the pacing entirely — a stress-test mode, not a default. Frames nodes schedule by their own flip intervals instead. The tick exists only while something animates: an idle document stays fully passive, and paused animations do not count.

## Rendering

**Cell** — Single terminal cell position. Contains display content, fg/bg colors, and terminal attributes.

**CellAttrs** — The bold/italic/underline state carried by a cell's glyph. Packed on the cell — unlike on `Style`, where the three are separate properties — because nothing merges at the cell level: attributes belong to the glyph and are replaced or cleared with it.

**CellContent** — The display content stored in a cell: empty space, a grapheme glyph, or a wide-glyph continuation marker.

**WideContinuation** — Marker for the second terminal cell occupied by a width-2 glyph. It is not printed directly; the glyph head prints the visible character.

**Grid** — 2D buffer of Cells representing screen state (width × height). Carries an active clip while painting, honored at the cell-write level so fills, text, borders, and half-block edges all clip identically.

**Culling** — Render-time drop of a node whose translated rectangle lies wholly outside its clip: it is left out of the paint entries, so it paints nothing and cannot be hit, while remaining in the DOM and in layout. Where a subtree's clip is empty the walk stops entirely.

**Scrollbar** — Overlay strips on a scroll container's last viewport column and row, drawn after the container's subtree so they cover the content they scroll but not what later covers the container. They occupy no layout — showing or hiding a bar never reflows content. `ScrollbarCharset` is the primitive (block and half-block are named constructors); show modes are `Always`, `WhenFocused` (hover focuses, so also "while hovered"), `WhenScrolling` (see below), and `Never`, all gated on the axis actually being scrollable. Hit-testing a bar resolves to its container, and a left press on a bar has *grabbing it* as its default action, in place of selection and click: a thumb press grabs the thumb where it is, a track press jumps the thumb under the cursor, and the same press continues as a drag either way — geometry is re-read from live layout each move, through the inverse of the thumb math, exact at both ends. `prevent_default()` on the mouse down keeps the press an ordinary container press.

**WhenScrolling** — The auto-hiding scrollbar show mode. The bar appears when the scroll offset actually changes and stays while grabbed; afterwards it holds fully visible for the container's `scrollbar_hide_delay`, then fades out over its `scrollbar_fade_duration` (both style properties) by ramping alpha into the bar colors. Visibility is a pure function of the document clock against per-container last-activity instants, so headless time travel renders every phase. A waiting bar schedules one deadline wake at fade start, only the fade itself ticks smoothly, and a fully faded bar leaves the document as passive as if it never existed.

**Render Cursor** — Cursor metadata produced with a rendered frame. It carries cursor position, shape, foreground-derived color, and clipped visibility without mutating grid cell content.

---

*Additional terms will be added as new concepts are introduced during development.*
