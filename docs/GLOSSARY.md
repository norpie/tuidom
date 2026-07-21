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

**Event** — Input or system notification dispatched to handlers. Carries event-specific data and, for targeted events, propagation state.

**Listener** — User-provided handler function. Internally stored with a stable id and shared callback so dispatch can snapshot listeners before invocation.

**ListenerHandle** — Opaque handle for removing a registered listener. Contains a document-scoped listener id so handles from different documents do not collide.

**Propagation** — Event flow through DOM tree. Target phase (fires on target node) → Bubble phase (fires on ancestors, root-ward).

**Event Loop** — Async runtime that waits for terminal events, document notifications, animation state, and shutdown. It dispatches terminal events to listeners and renders when needed.

**Input Event** — An `Input` node's value changed, reported by `on_input` after the key press default action that edited it. It targets the input and bubbles, so a form-shaped container can observe every field inside it without registering on each. It exists because the edit is a *default action*, which runs after listeners: an `on_key_press` handler reading `input_value()` sees the value as it was before the keystroke, with no way to observe the change short of deferring a frame. Only real changes fire it — a keystroke that edits nothing, and cursor or selection movement, leave the value alone and stay silent. Programmatic writes through `set_input_value` are silent too: the caller already knows what it wrote, and reporting it back would loop a two-way binding through itself. It carries no `prevent_default`, because it reports a change already made; the place to suppress the change is `prevent_default()` on the key press. It needs no disabled/inert exemption either, unlike the animation events — the default action only runs on the focused node, and a node cannot hold focus while blocked.

**Post-Frame Event** — Document-level notification that a frame just finished, carrying that frame's recorded metrics. Like resize it has no target node and does not bubble. The render task never runs user code: the event goes through the runtime queue so handlers run on the event task, ordered with input handlers. Mutating the DOM from a post-frame handler schedules another frame, whose own post-frame event fires in turn — an unpaced handler keeps the renderer permanently active, so handlers pace their mutations to let it go idle.

**Window Focus** — Whether the *terminal window* holds OS focus, reported as the document-level `on_window_focus` / `on_window_blur` events. Not DOM focus, despite the shared word: it names no node, and gaining or losing it never moves the focused node or disturbs the focus stack — alt-tabbing away and back returns the user to the node they left. Only terminals that support focus reporting send it; one that does not simply stays silent.

**Input Coalescing** — Collapsing redundant runs in the batch of events already queued behind the one being processed. Only pointer movement and resize collapse, and only when adjacent: everything else — keys, presses, releases, wheel ticks — carries information its successor does not. Adjacency is the rule that makes it safe, because hover-to-focus lets the pointer's position decide which node a key press targets, so merging movement *across* a key would deliver it to the wrong node. Nothing is ever delayed to build a batch; a batch of one is returned unchanged, and collapsing only happens when the event task was already behind. This is also the queue's only bound — there is no backpressure, because there is nobody to push back on a terminal and dropping input would lose keystrokes silently.

**Panic Restore** — The process-wide panic hook that puts the terminal back before a crash reaches the user: it undoes exactly the modes currently turned on, then chains to whatever hook was installed before it. It is installed only when a real terminal is set up, and never for a panic inside a downstream callback — those are caught, logged, and survived, so restoring for one would tear down an application that is still running. Terminal modes are tracked in one place that the setup guard, the normal drop, and the hook all restore from, and exactly one of them ever claims it.

**Bell** — `doc.bell()`, emitted as `\x07` by the next flush rather than written when called: the render task owns the output stream, and a byte written from another thread could land inside an escape sequence and corrupt it. Ringing schedules a frame, so a bell still reaches the terminal when nothing on screen changed, and several bells before that frame produce one — which is all a terminal can make of them. What a bell *does* is the terminal's choice: a sound, a visual flash, or nothing.

## Focus & Selection

**Focus Context** — A subtree that traps focus, opened on a stacking context with `push_focus_context` and closed with `pop_focus_context`. The active context scopes everything about focus: `focused()` reports the focused node *within* it, tab order and spatial navigation search only inside it, and everything outside it is inert. The document root is a permanent focus context, so with nothing open the whole tree is in scope.

**Focus Stack** — The stack of open focus contexts, innermost last, with the permanent root context at the bottom. Each level remembers its own focused node, so restoring focus when a modal closes is just a pop rather than separate bookkeeping. Nested modals unwind in order. If a remembered node no longer exists, is no longer focusable, or has been disabled, focus is left cleared instead of jumping to a node the user never selected.

**Inert** — State that blocks interaction on everything outside the active focus context. Inert nodes cannot be focused, are skipped by tab and spatial navigation, and swallow input events rather than bubbling them. Unlike a disabled node, an inert node merges no style — content behind a modal keeps its own appearance. Focus and blur events are exempt from the swallow, since they report a focus change the engine has already made.

**Spatial Navigation** — Arrow key focus movement based on visual distance (edge-to-edge) rather than DOM order.

**Selection Boundary** — Container marked `selection_boundary: true`. A drag is confined to the boundary of its *starting* point — the nearest marked ancestor-or-self, the root when nothing is marked — for the whole gesture: crossing into another boundary just snaps the focus point to the nearest text inside the original one. An Input is an implicit boundary whose drag drives the input's own selection instead. Within one boundary, the selected range follows document order, so two unmarked columns select browser-style — the tail of one plus the head of the other.

**SelectionPoint** — A position in a document selection: a Text node plus a byte offset on a grapheme boundary. Content-addressed rather than screen-addressed, so scrolling never moves or invalidates it — rendering re-maps it through the current layout each frame. Consumers see the anchor/focus pair normalized to document order with the end extended past the glyph under it, so both endpoint cells of a drag are included, the way terminals select.

**Selection Colors** — `selection_bg` / `selection_fg`, the style colors selected glyphs render with. Unset means reverse video: each selected cell swaps its foreground and background, visible on any theme with zero configuration. A drag that starts on a non-text cell snaps to the nearest character in the boundary, and a left press both clears the selection and arms a new drag — `prevent_default()` on the mouse down suppresses that default, keeping the selection.

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
