# Features

## Core DOM Model

- [x] Arena node storage — all mutations go through `Document`
- [x] `Document` wraps `Arc<DocumentInner>` — cheap cloning, no explicit Arc wrapping needed
- [x] Thread-safe interior mutability — all methods take `&self`, `Document` is `Send + Sync`
- [x] Permanent document root node — created by `Document::new()`, always exists, cannot be reparented or removed
- [x] `NodeId` is a lightweight, `Copy` integer handle
- [ ] Node kinds:
  - [x] Box (generic container, div equivalent)
  - [x] Text (static text content)
  - [x] Frames (cycles through content on a timer — for spinners, ASCII animations)
    - [x] Current frame is a function of elapsed time — no per-flip mutation
    - [x] Measured on the largest frame, so cycling never reflows surrounding content
    - [x] Self-paced: a lone frames node renders at its interval, not the animation tick
  - [ ] Canvas (downstream-controlled rendering region)
    - [ ] Participates in layout like any Box
    - [ ] Cell buffer mode: callback fills 2D grid of cells (char, fg, bg, attrs)
    - [ ] Raw mode: callback writes arbitrary escape sequences (for kitty/sixel images)
    - [ ] Enables: custom graphics, charts, half-block images, native image protocols
- [x] Typed style struct for known properties + raw custom style properties
- [x] Attributes stored as `HashMap<String, String>`
- [x] Public attributes API: set/get/remove
- [x] `get_node(id)` for ID-based lookup — also exposes computed layout info (position, size)
- [x] `node_at(x, y)` for hit testing — returns topmost node at coordinates
- [x] Child ordering: `insert_before(parent, child, before_sibling)`, `move_child()`
- [x] Input (text input with cursor, selection, internal state)
  - [ ] Full selection support:
    - [x] mouse drag — an Input is an implicit selection boundary, so a drag inside it
          drives the input's own selection
    - [ ] ctrl+a, shift+arrows — the key default action handles no modifiers yet
  - [x] `multiline: bool` (default false) — single-line vs textarea
  - [x] `mask: Option<char>` (default None) — for password fields
  - [ ] `show_cursor: Always | WhenFocused | Never` (default WhenFocused)
  - [x] Focusable by default
  - [x] `on_input` fires on value change — the edit is a key *default action* that runs
        after listeners, so nothing outside could otherwise observe a keystroke

## Cursor

- [x] Cursor metadata — render frames expose cursor position/style separately from grid cells
- [x] Cursor style metadata (document-wide default, per-node override):
  - [x] Shape: block, underline, bar
  - [x] Cursor color follows the focused node's resolved foreground color
- [ ] Multiple cursors: users sync manually via events (not built-in)

## Focus Management

- [x] Focus is `Option<NodeId>` per focus context — can be `None`
- [x] Tab order follows DOM definition order — walked from the active focus context, so
      an open modal-like context scopes tab order without a filtering pass
- [x] Arrow key navigation based on visual/spatial distance:
  - [x] Press arrow → focus nearest focusable node in that direction
  - [x] Distance measured edge-to-edge
  - [x] Tiebreaker: **implemented differently** — ties break on cross-axis center distance
        first, then paint order (topmost painted wins), not on smallest y. Center distance
        is what makes the nearest *aligned* node win rather than an arbitrary one sharing
        an edge.
  - [x] No wrap — if nothing in that direction, do nothing (downstream can override via events)
- [x] Tab when focus is `None` focuses first focusable node (re-enter cycle)
- [x] Imperative control:
  - [x] `doc.focus(node_id)` — succeeds only if node is in active context (not behind a modal)
  - [x] `doc.blur()` — sets focus to `None` within current context
- [x] Focusable property: Box defaults to false, Input defaults to true (`set_focusable`)
- [x] Escape key behavior:
  - [x] First press: blur current node (focus → None)
  - [x] Second press (when already None): propagates to handlers (e.g., close modal)
- [x] Focus stack for modal nesting (see Stacking Contexts)

## Stacking Contexts & z-index

Solves the "dropdown in modal" problem: a dropdown in one subtree shouldn't unexpectedly paint above unrelated UI. Paint order treats each node's subtree as an atomic unit.

- [x] `z_index: i32` controls paint order between sibling subtrees
  - [x] Lower values paint first, higher values paint later
  - [x] DOM order is the stable tiebreaker for equal values
  - [x] Descendant `z_index` values cannot escape their parent subtree
- [x] Stacking contexts: created explicitly (`stacking_context: true`)
  - [x] Isolation marker for modal/focus policy and future positioning behavior
  - [x] Prerequisite for trapping focus — being a stacking context never traps focus on its own
- [x] Focus integration with stacking contexts:
  - [x] Modal-like downstream components trap focus within a chosen stacking context
  - [x] Content outside the active context is inert: non-focusable, skipped by navigation, swallows input
  - [x] Inert is not disabled — inert nodes merge no style, so background content keeps its appearance
  - [x] Keys dispatch from the active context when nothing is focused, so its handlers see Escape
  - [x] Nested modal-like contexts restore focus via a focus stack
- [x] Focus stack for modal-like contexts:
  - [x] `push_focus_context()` — auto-focus first focusable in context, remember the interrupted focus
  - [x] `pop_focus_context()` — restore focus to the previous node
  - [x] If stored node no longer exists, is unfocusable, or is disabled, focus is left cleared
  - [x] Removing an open context's node closes it, so focus is never trapped in a dead subtree

## Scrolling & Virtualization

- [x] Per-axis `overflow` style: `Visible` (default), `Scroll`, `Clip`
  - [x] A `Scroll`/`Clip` axis drops the content-size floor, so the container can be smaller than its content
  - [x] Layout is scroll-invariant: the offset is applied at paint time, a wheel tick never re-runs layout
- [x] Scroll offset as document runtime state:
  - [x] `scroll_offset` / `scroll_to` / `scroll_by`, clamped to content minus viewport from the layout snapshot
  - [x] Relayout re-clamps stored offsets when content shrinks
- [x] Wheel routing: nearest scrollable ancestor on the wheel's axis that can still move (scroll chaining)
  - [x] `prevent_default()` on the wheel event suppresses the default scroll
  - [x] Horizontal wheel events (`ScrollLeft`/`ScrollRight`) route to `overflow_x` containers
- [x] Auto-cull at render time for any overflow-scrollable container:
  - [x] Skip painting nodes outside the visible scroll area
  - [x] Nodes still exist in DOM, just not rendered when off-screen
  - [x] Exact for variable-height items — layout still positions everything; culling is paint-only
- [x] Virtualization primitives (`virtualize` module) — not a widget: the engine provides the math, downstream owns every node
  - [x] Spacer pattern: leading/trailing spacers keep the container's content size at the true total, so clamping, scrollbar geometry, and wheel routing need nothing virtual (spacers must set `flex_shrink(0.0)`)
  - [x] Uniform window math, axis-agnostic — vertical, horizontal, and 2D as the same math run per axis
  - [x] Measurement cache for variable extents: estimates, recorded measurements, logarithmic offset queries, invalidation, anchoring compensation
  - [x] `Virtualizer` window diffing helper: add/remove index ranges + spacer sizes per scroll, no-op inside the overscan margin
  - [x] Tree-compatible: downstream supplies flattened visible rows, tuidom virtualizes the flat sequence — no built-in tree semantics
  - [x] `NodeView` exposes the scrollport and per-axis max scroll for downstream window math
- [x] Built-in scrollbars for overflow containers:
  - [x] Overlay bars: no layout cost, drawn over the viewport's last column/row above the content they scroll
  - [x] Configurable characters/styling (track, thumb colors)
  - [x] Full block default (█), half-block style option (▐▌) for thinner look
  - [x] Show behavior: always, when_focused (hover focuses, so also hover), never
  - [x] Show behavior: when_scrolling — appears on offset change, holds for `scrollbar_hide_delay`,
        fades out over `scrollbar_fade_duration` (both style properties); a grabbed bar stays visible
  - [x] Fade frames are scheduled, not polled: one deadline wake at fade start, smooth ticks only
        mid-fade, fully passive once faded
  - [x] Click/drag on the bar: a left press grabs it as the mouse-down default action
        (`prevent_default()` keeps an ordinary press; no selection, no click)
    - [x] Thumb press drags from where it is; track press jumps the thumb under the cursor and
          keeps dragging
    - [x] Drags map through the inverse thumb math against live geometry, exact at both ends

## Reactivity & Change Propagation

- [x] Channel-based notify — renderer wakes on change, not on timer
- [x] Unchanged writes are no-ops: `set_text_content` with identical content triggers no relayout or re-render
- [x] Completely passive while idle — no polling, no fixed tick rate
- [x] Active rendering only during animations — drives frames until animation completes, then returns to passive
- [ ] User-facing subscribe API for node/document changes

## Layout

- [x] Use `taffy` for flexbox layout
- [x] Padding, margin, flex direction (row/column and reverse), flex grow/shrink/basis, flex gap, align-self, flex wrap (including wrap-reverse), align-content
- [x] 1:1 mapping from DOM nodes to taffy layout nodes
- [x] Custom measure functions for text (terminal cell widths)
- [x] Careful integer rounding of layout results to avoid gaps/overlaps
- [x] Positioning modes:
  - [x] `Position::Flow` (default) — normal flexbox layout
  - [x] `Position::Absolute { x, y }` — signed cell offset from the parent's box origin, removed from flow
- [x] Centering helpers (terminal cells are discrete — can't always center perfectly):
  - [x] `center_x()` / `center_y()` — returns `Centered::Even(x)` or `Centered::Uneven { low, high }` when margins differ by 1
  - [x] `any_center_x()` / `any_center_y()` — returns single coordinate (left/top-biased) when you don't care about off-by-one

## Styling

- [x] Inline styles only — no CSS selectors, no stylesheets, no cascade
- [x] Explicit inheritance via `StyleValue::Inherit` — nothing inherits unless specified
- [x] Style resolution walks parent chain for inherited values, caches results
- [x] Pseudo-state style overrides (merged on top of base style):
  - [x] `set_focus_style()` — when node is focused (hover = focus)
  - [x] `set_active_style()` — when node is being pressed
  - [x] `set_disabled_style()` — when node is disabled
  - [x] Merge order: base → focus → active → disabled
  - [x] Disabled is subtree-wide — disabled nodes are non-focusable and swallow targeted events

## Borders

- [x] Traditional box-drawing borders (style property):
  - [x] Presets: single, double, rounded, thick, ascii, none
  - [x] Custom charset support (user-defined characters)
  - [x] Per-side control (top, right, bottom, left independently) — width is always one cell, so per-side control is presence
  - [x] Borders occupy real cells: taffy insets a bordered node's content and children
  - [x] `border_color` is its own property, following the node's foreground when unset
- [x] Half-block edges (opt-in, per side) — not a border: they cost no layout and frame nothing
  - [x] Uses `▀▄▌▐` (and `▗▖▝▘` where two edges meet) with fg/bg colors to end a node's fill on a half cell
  - [x] Balances vertical against horizontal padding, since a cell is twice as tall as it is wide
  - [x] Both colors come from the user: the inner half follows the node's `background` when unset, the outer half keeps whatever is painted underneath
  - [x] Modern look without traditional box-drawing characters

## Colors

- [x] OKLCH as internal representation — all operations work on OKLCH
- [x] Convert to RGB only at render time
- [x] Caching layer for conversions (OKLCH → RGB is math-heavy)
- [x] `Color` is an expression; `ResolvedColor` is the concrete OKLCH value on `ResolvedStyle`
- [x] Color variable system — cascades down the tree
  - [x] Define at document or node level, children inherit
  - [x] Reference in styles: `color: Color::var("--primary")`
  - [x] Redeclaring shadows for the subtree; a declaration resolves against the parent's scope, so cycles cannot be written
  - [x] An undefined name falls back to the property's default rather than half-applying a derivation
- [x] Derived color operations (work on OKLCH):
  - [x] `lighten(amount)`, `darken(amount)` — absolute steps, since OKLCH lightness is perceptually uniform
  - [x] `with_hue(h)`, `with_chroma(c)`, `with_lightness(l)`
  - [x] `with_alpha(a)`
  - [x] `mix(other, t)` — blends in OKLCH; borrows the chromatic hue when one side is a gray
  - [ ] `contrast()` — compute readable contrast color
  - [ ] Explicit color space: `.as_hsl().lighten(0.1)` if you want HSL math
- [x] Derive from current node: `CurrentBg.darken(0.1)`, `CurrentFg.with_alpha(0.5)`
  - [x] Resolution order: variables, then `background`, then `color`, then every other color property
  - [x] In `background` and in a variable declaration, `CurrentBg`/`CurrentFg` mean the *parent's* values — self-reference is otherwise circular
  - [x] `CurrentBg` on a transparent node sees through to the nearest painted ancestor
- [x] Declared terminal background (`doc.set_terminal_background`) — the real one is unknowable, so it is assumed
  - [x] The bottom of the `CurrentBg` chain, and the base a translucent color blends toward over an unpainted cell
  - [x] An assumption for color math, never painted — unpainted cells still emit the terminal default
- [x] Alpha blending at render time:
  - [x] Render back-to-front (painter's algorithm)
  - [x] Semi-transparent colors blend with buffer content below
  - [x] Translucent background fills preserve existing text content
  - [x] Enables modal overlays, frosted effects, etc.
- [x] Selection colors (`selection_bg`, `selection_fg`) — inherited like other colors; unset means reverse video

## Text Selection

- [x] Screen-wide text selection (not just Input fields)
- [x] Selection boundaries — containers marked as `selection_boundary: true`
  - [x] Mouse drag selection respects boundaries, doesn't bleed across — the drag is
        confined to its starting point's boundary for the whole gesture
  - [x] Sidebar, main content, input areas can be separate boundaries
  - [x] An Input is an implicit boundary: dragging inside it drives the input's own
        selection (and click positions the input cursor)
- [x] Selection state tracked at document level:
  - [x] Which boundary container
  - [x] Anchor/focus as `SelectionPoint` (Text node + grapheme-normalized byte offset);
        consumers see the pair in document order with the end extended past the glyph
        under it, so both endpoint cells are included
  - [x] A point on a non-text cell snaps to the nearest character in the boundary
  - [x] Selection survives scrolling (content-addressed) and is pruned on node removal
        and clamped on text content changes
- [x] API:
  - [x] `doc.get_selection() -> Option<String>` — returns selected text in reading order
        (document order; newline between slices that don't share a screen row)
  - [x] `doc.clear_selection()`
  - [x] `doc.selection() -> Option<(SelectionPoint, SelectionPoint)>`
  - [x] Selection changed event (document-level, fires only on actual change)
- [x] Rendering: selected characters use `selection_bg`/`selection_fg` colors (inherited via style system)
  - [x] Unset colors mean reverse video: selected cells swap fg/bg
  - [x] Focused inputs highlight their selection the same way; masked inputs highlight mask glyphs
- [x] No built-in clipboard keybinds — user binds Ctrl+C to `clipboard.set(doc.get_selection())`

## Rendering

- [x] Crossterm as primary backend (possibly backend-agnostic later if easy)
- [x] Virtual screen buffer — full cell-by-cell diff each frame
- [x] Terminal text attributes: bold, italic, underline (sticky SGR state, emitted as transitions)
- [x] Terminal resize handling:
  - [x] Auto-relayout on resize
  - [x] Resize event fired for user handlers
- [x] Optional document-wide maximum FPS cap; uncapped by default
- [ ] Unicode / wide character support:
  - [x] Use existing crates (`unicode-width`, etc.) for character width calculation
  - [x] Handle wide characters (CJK, emoji) transparently in text rendering
  - [x] Affects text measurement and rendering
  - [ ] Affects cursor positioning in Input
  - [ ] Investigate edge cases during implementation
  - [ ] Avoid hardcoding LTR assumptions — prepare for future bidi support

## Event System

- [ ] Target + bubble propagation (no capture phase)
- [ ] `stop_propagation()` to halt bubbling
- [x] Sync handlers only — user spawns for async
- [ ] Events:
  - [x] Keyboard: key press
  - [x] Terminal resize
  - [ ] Keyboard: key down, key up — **not planned**: key release needs the kitty
        keyboard protocol, so the API would silently never fire on most terminals.
        `Repeat` is dropped alongside `Release` for the same reason.
  - [ ] Mouse: click, mouse down, mouse up, wheel (raw input)
  - [ ] Focus: focus, blur
    - [ ] Hover = focus: mousing over a focusable node focuses it (no separate hover state)
  - [x] Scroll: `on_scroll` fires on overflow containers when scroll position changes (target only, no bubble — like the DOM's)
  - [x] Input: `on_input` fires on an `Input` node when its value changes (target, then
        bubbles — like the DOM's, so a form-shaped container observes every field)
    - [x] Real changes only: a keystroke that edits nothing, and cursor or selection
          movement, stay silent
    - [x] Programmatic `set_input_value` stays silent, so a two-way binding cannot loop
          back through its own listener
    - [x] No `prevent_default` — it reports a change already made; `prevent_default()` on
          the key press suppresses the edit itself
  - [x] Frame: `on_post_frame` — document-level like resize, fires after each rendered frame with its metrics; DOM mutation in the handler schedules another frame, so handlers pace their mutations
  - [x] Animation: `on_transition_end`, `on_animation_end`, `on_animation_iteration` — node-targeted and bubbling like the DOM's; exempt from the disabled/inert swallow, since they report a change the engine already made
  - [x] Window: `on_window_focus`, `on_window_blur` — terminal window gains/loses OS focus
    - [x] Document-level like resize: no target node, no bubbling
    - [x] Never touches DOM focus or the focus stack — alt-tabbing back returns the
          user to the node they left
    - [x] Terminals without focus reporting simply never send it; no capability check
- [x] Listener registration returns handle for removal
- [x] `prevent_default()` exists only where a document-level default action does: key presses (focus defaults), wheel (default scroll), and mouse down (selection start)

## Transitions & Animations

- [x] Transitions: property changes animate over time
  - [x] Animatable: opacity
  - [x] Animatable: position (absolute offsets), size (width/height), colors (interpolated
        in OKLCH), padding, margin — layout-affecting values drive the layout engine per
        animation frame while in flight, and only then
  - [x] Not animatable: discrete values (border style, text content, booleans) — and
        non-interpolable states (`Auto` sizes, unset colors, `Flow` position, unit
        changes) snap, like CSS `auto`
  - [x] Configurable duration, easing (including CSS-style `CubicBezier`)
  - [x] Pseudo-state changes (focus/active/disabled) trigger transitions like explicit
        style changes — the merged resolved style is what is diffed
  - [x] Interruption continues from the displayed value; a pure reversal is shortened to
        the share of the duration matching the distance covered
  - [x] `on_transition_end` event (node-targeted, bubbles; interrupted transitions and
        removed nodes fire none)
- [x] Keyframe animations: multi-step property animations
  - [x] Define keyframes at percentages (0%, 50%, 100%, etc.) holding typed
        `AnimatableProperty` values — non-animatable properties are unrepresentable
  - [x] Duration, easing (per segment), iteration count (finite or infinite), direction
        (normal, reverse, alternate)
  - [x] Implicit 0%/100% endpoints from the node's underlying value; animations apply
        over transitions (animations win on conflict)
  - [x] `from_to()` shorthand for simple two-state animations
  - [x] Control: pause (frozen values drive no frames), resume, cancel (no end event)
  - [x] Events: `on_animation_end`, `on_animation_iteration` (node-targeted, bubble;
        boundaries crossed within one frame coalesce)
- [x] Frames node handles content-based animation (see Core DOM Model)
- [x] Paced animation frames: default ~60fps tick, `set_animation_fps(None)` unpaces
      entirely; frames nodes self-pace at their flip intervals; idle stays passive

## Async Runtime

- [x] Tokio as the async runtime

## Application Lifecycle

- [x] Async `doc.run().await` — blocks task until quit, can be spawned as separate task
- [x] `doc.quit()` — trigger shutdown from handlers
- [x] Terminal state management:
  - [x] Enter alternate screen on start
  - [x] Manage real terminal cursor from render-frame cursor metadata
  - [x] Restore terminal state on exit (raw mode, alternate screen, cursor visibility)
  - [x] Drop guard restores terminal state after successful setup
  - [x] Setup guard restores partially initialized terminal state if startup fails
  - [x] Panic hook restores terminal state even on crash
    - [x] Chains to the previously installed hook; installed once, only for a real terminal
    - [x] Skipped for panics inside downstream callbacks, which are caught and survived
    - [x] One `Terminal` per process, enforced — a second `run()` is refused, not left
          to corrupt the screen
- [x] Input queue policy: unbounded, coalesced rather than backpressured
  - [x] Adjacent pointer-move and resize runs collapse to the latest
  - [x] Keys, presses, releases, and wheel ticks are never dropped or reordered
  - [x] Coalescing engages only when the event task is already behind — nothing is
        ever delayed to build a batch
- [x] No built-in Ctrl+C or signal handling — user's responsibility (use `tokio::signal`, etc.)
- [x] `doc.bell()` — trigger terminal bell
  - [x] Emitted by the next flush, never written from the calling thread
  - [x] Schedules a frame, so it rings when nothing on screen changed
  - [x] Coalesced: several bells before one frame ring once

## DSL / Node Macro

**Out of scope for the engine.** A declarative DOM macro was planned here as `node!`, and
the stub crate that would have held it has been removed rather than left exporting a
`todo!()`. Two things changed the placement:

- A tree-building macro is only worth having if it also expresses *updates*, and updates
  are a reactivity question — which is a framework concern, not an engine one. The engine
  deliberately has no opinion about how downstream drives it.
- The framework layer settled on `view!` as the canonical declarative syntax, so `node!`
  would have been a second, weaker spelling of the same idea.

What stays true here regardless of the macro:

- [x] The raw builder API is the supported way to construct a tree, and stays a
      first-class surface rather than something a macro is expected to paper over
- [x] Components are just functions returning `NodeId` — the engine needs no component
      concept for that to work
- [x] Downstream owns reactivity, state management, and any macro layer over this API

## Debugging & Developer Tools

- [ ] Tracing integration (`tracing` crate) for internal insight:
  - [ ] Event dispatch
  - [ ] Layout calculations
  - [ ] Render cycles
  - [ ] Style resolution
  - [ ] Animation ticks
- [ ] Snapshot serialization (bincode for speed):
  - [ ] Full document snapshot for comparison/testing (no restore — handlers can't be serialized)
  - [ ] Per-node serialization: `doc.serialize_node(node_id)` for external storage/comparison
  - [ ] Captures: DOM structure, styles, text content, computed layout, focus/selection state
- [x] Public performance metrics API:
  - [x] FPS / frame time
  - [ ] Node count
  - [x] Render latency
  - [x] Layout latency
  - [ ] Event latency
  - [x] Opt-in detailed paint and diff profiling

## Testing

- [ ] Headless mode — run without real terminal (for CI, tests)
  - [ ] Still computes layout, fills virtual screen buffer
- [ ] Simulated input:
  - [ ] `simulate_click(x, y)`
  - [ ] `simulate_key(key)`
  - [ ] `simulate_text("hello")`
  - [ ] `simulate_mouse_drag(start, end)`
  - [ ] `simulate_scroll(delta)`
- [ ] Screen buffer inspection:
  - [ ] `get_cell(x, y) -> Cell`
  - [ ] `get_screen_region(x, y, w, h) -> Vec<Vec<Cell>>`
- [ ] Recording/playback:
  - [ ] `start_recording()` / `stop_recording() -> EventLog`
  - [ ] `replay(log)` — replay with timing via simulated input
  - [ ] Events are serializable (for saving/loading recordings)

## Error Handling

- [x] Infallible where possible — most operations return values directly, no Result
- [x] Invalid tree operations return typed `TuidomError` values instead of panicking
- [x] Event handlers wrapped in `catch_unwind` — one bad handler doesn't crash the app
- [x] Handler panics logged

## Future Considerations

- RTL / bidirectional text support (text rendering, layout direction, Input cursor/selection)
