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
  - [ ] Frames (cycles through content on a timer — for spinners, ASCII animations)
  - [ ] Canvas (downstream-controlled rendering region)
    - [ ] Participates in layout like any Box
    - [ ] Cell buffer mode: callback fills 2D grid of cells (char, fg, bg, attrs)
    - [ ] Raw mode: callback writes arbitrary escape sequences (for kitty/sixel images)
    - [ ] Enables: custom graphics, charts, half-block images, native image protocols
- [x] Typed style struct for known properties + raw custom style properties
- [x] Attributes stored as `HashMap<String, String>`
- [x] Public attributes API: set/get/remove
- [x] `get_node(id)` for ID-based lookup — also exposes computed layout info (position, size)
- [ ] `node_at(x, y)` for hit testing — returns topmost node at coordinates
- [x] Child ordering: `insert_before(parent, child, before_sibling)`, `move_child()`
- [ ] Input (text input with cursor, selection, internal state)
  - [ ] Full selection support (mouse drag, ctrl+a, shift+arrows)
  - [ ] `multiline: bool` (default false) — single-line vs textarea
  - [ ] `mask: Option<char>` (default None) — for password fields
  - [ ] `show_cursor: Always | WhenFocused | Never` (default WhenFocused)
  - [ ] Focusable by default

## Cursor

- [x] Cursor metadata — render frames expose cursor position/style separately from grid cells
- [x] Cursor style metadata (document-wide default, per-node override):
  - [x] Shape: block, underline, bar
  - [x] Cursor color follows the focused node's resolved foreground color
- [ ] Multiple cursors: users sync manually via events (not built-in)

## Focus Management

- [x] Focus is `Option<NodeId>` per focus context — can be `None`
- [ ] Tab order follows DOM definition order
- [ ] Arrow key navigation based on visual/spatial distance:
  - [ ] Press arrow → focus nearest focusable node in that direction
  - [ ] Distance measured edge-to-edge
  - [ ] Tiebreaker: topmost (smallest y)
  - [ ] No wrap — if nothing in that direction, do nothing (downstream can override via events)
- [ ] Tab when focus is `None` focuses first focusable node (re-enter cycle)
- [x] Imperative control:
  - [x] `doc.focus(node_id)` — succeeds only if node is in active context (not behind a modal)
  - [x] `doc.blur()` — sets focus to `None` within current context
- [ ] Focusable property: Box defaults to false, Input defaults to true
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

- [ ] Auto-cull at render time for any overflow-scrollable container:
  - [ ] Skip painting nodes outside the visible scroll area
  - [ ] Nodes still exist in DOM, just not rendered when off-screen
  - [ ] Best-effort for variable-height items (may need measurement)
- [ ] Explicit opt-in virtualization widget (name TBD) for large uniform-sized collections
  - [ ] Works for both vertical and horizontal scrolling
- [ ] Built-in scrollbars for overflow containers:
  - [ ] Configurable characters/styling (track, thumb colors)
  - [ ] Full block default (█), half-block style option (▐▌) for thinner look
  - [ ] Show behavior: always, when_scrolling, when_hovering, never

## Reactivity & Change Propagation

- [x] Channel-based notify — renderer wakes on change, not on timer
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
- [ ] Color variable system — cascades down the tree
  - [ ] Define at document or node level, children inherit
  - [ ] Reference in styles: `color: Var("--primary")`
- [ ] Derived color operations (work on OKLCH):
  - [ ] `lighten(amount)`, `darken(amount)`
  - [ ] `with_hue(h)`, `with_chroma(c)`, `with_lightness(l)`
  - [ ] `with_alpha(a)`
  - [ ] `contrast()` — compute readable contrast color
  - [ ] Explicit color space: `.as_hsl().lighten(0.1)` if you want HSL math
- [ ] Derive from current node: `CurrentBg.darken(0.1)`, `CurrentFg.with_alpha(0.5)`
  - [ ] Resolution order: variables resolved first, then `CurrentBg`/`CurrentFg` reference the node's resolved colors, then derivations applied
- [x] Alpha blending at render time:
  - [x] Render back-to-front (painter's algorithm)
  - [x] Semi-transparent colors blend with buffer content below
  - [x] Translucent background fills preserve existing text content
  - [x] Enables modal overlays, frosted effects, etc.
- [ ] Selection colors (`selection_bg`, `selection_fg`) — inherited like other colors

## Text Selection

- [ ] Screen-wide text selection (not just Input fields)
- [ ] Selection boundaries — containers marked as `selection_boundary: true`
  - [ ] Mouse drag selection respects boundaries, doesn't bleed across
  - [ ] Sidebar, main content, input areas can be separate boundaries
- [ ] Selection state tracked at document level:
  - [ ] Which boundary container
  - [ ] Start/end positions or character ranges
- [ ] API:
  - [ ] `doc.get_selection() -> Option<String>` — returns selected text in reading order
  - [ ] `doc.clear_selection()`
  - [ ] Selection changed event
- [ ] Rendering: selected characters use `selection_bg`/`selection_fg` colors (inherited via style system)
- [ ] No built-in clipboard keybinds — user binds Ctrl+C to `clipboard.set(doc.get_selection())`

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
  - [ ] Keyboard: key down, key up
  - [ ] Mouse: click, mouse down, mouse up, wheel (raw input)
  - [ ] Focus: focus, blur
    - [ ] Hover = focus: mousing over a focusable node focuses it (no separate hover state)
  - [ ] Scroll: `on_scroll` fires on overflow containers when scroll position changes
  - [ ] Window: `on_window_focus`, `on_window_blur` — terminal window gains/loses OS focus
- [x] Listener registration returns handle for removal
- [ ] No `prevent_default()` — users react to undo default behaviors if needed (simpler API)

## Transitions & Animations

- [x] Transitions: property changes animate over time
  - [x] Animatable: opacity
  - [ ] Animatable: position, size, colors (interpolated in OKLCH), numeric style values (padding, margin, etc.)
  - [ ] Not animatable: discrete values (border style, text content, booleans)
  - [x] Configurable duration, easing
  - [ ] `on_transition_end` event
- [ ] Keyframe animations: multi-step property animations
  - [ ] Define keyframes at percentages (0%, 50%, 100%, etc.)
  - [ ] Duration, easing, iteration count, direction (normal, reverse, alternate)
  - [ ] `from_to()` shorthand for simple two-state animations
  - [ ] Control: pause, resume, cancel
  - [ ] Events: `on_animation_end`, `on_animation_iteration`
- [ ] Frames node handles content-based animation (see Core DOM Model)

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
  - [ ] Panic hook restores terminal state even on crash
- [x] No built-in Ctrl+C or signal handling — user's responsibility (use `tokio::signal`, etc.)
- [ ] `doc.bell()` — trigger terminal bell

## DSL / Node Macro

- [ ] RSX-style `node!` macro for declaring DOM structure:
  ```rust
  node!(doc,
      box id="container" style={my_style} {
          text { "Hello World" }
          box focusable=true on_click={handler} {
              text { "Click me" }
          }
      }
  )
  ```
- [ ] Macro takes `&Document`, uses interior mutability — nested expressions work
- [ ] Returns root `NodeId`
- [ ] Expression escape hatch `{expr}` for inserting dynamic children:
  ```rust
  node!(doc,
      box {
          {some_component(doc, props)}  // Any expr returning NodeId
      }
  )
  ```
- [ ] Downstream component systems build on top:
  - [ ] Components are just functions/structs returning `NodeId`
  - [ ] Downstream creates their own macros for reactivity, state management
  - [ ] We provide primitives, they provide abstractions

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
