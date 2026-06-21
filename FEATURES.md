# Features

## Core DOM Model

- [x] Arena node storage — all mutations go through `Document`
- [x] `Document` wraps `Arc<DocumentInner>` — cheap cloning, no explicit Arc wrapping needed
- [x] Thread-safe interior mutability — all methods take `&self`, `Document` is `Send + Sync`
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
- [x] Typed style struct for known properties + `HashMap<String, String>` fallback for custom/unknown
- [x] Attributes stored as `HashMap<String, String>`
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

- [ ] Fake cursor — real terminal cursor hidden, we render cursors as styled cells
- [ ] Cursor style (document-wide default, per-node override):
  - [ ] Shape: block (semi-transparent), hollow_block, underline, bar
  - [ ] Colors: `cursor_bg`, `cursor_fg` (part of style system)
- [ ] Default behavior:
  - [ ] Semi-transparent block when window focused (see character underneath)
  - [ ] Hollow block when window unfocused
  - [ ] Blinks for 8 seconds after focus/movement, then static
  - [ ] Configurable blink duration (0 = no blink)
- [ ] Multiple cursors: users sync manually via events (not built-in)

## Focus Management

- [ ] Focus is `Option<NodeId>` per stacking context — can be `None`
- [ ] Tab order follows DOM definition order
- [ ] Arrow key navigation based on visual/spatial distance:
  - [ ] Press arrow → focus nearest focusable node in that direction
  - [ ] Distance measured edge-to-edge
  - [ ] Tiebreaker: topmost (smallest y)
  - [ ] No wrap — if nothing in that direction, do nothing (downstream can override via events)
- [ ] Tab when focus is `None` focuses first focusable node (re-enter cycle)
- [ ] Imperative control:
  - [ ] `doc.focus(node_id)` — succeeds only if node is in active context (not behind a modal)
  - [ ] `doc.blur()` — sets focus to `None` within current context
- [ ] Focusable property: Box defaults to false, Input defaults to true
- [ ] Escape key behavior:
  - [ ] First press: blur current node (focus → None)
  - [ ] Second press (when already None): propagates to handlers (e.g., close modal)
- [ ] Focus stack for modal nesting (see Layering)

## Layering & Stacking Contexts

Solves the "dropdown in modal" problem: a dropdown in App1 shouldn't appear above a modal in App2. 
Each stacking context is an isolated layering environment — nodes can't visually escape their context.

- [ ] Stacking contexts: created explicitly (`stacking_context: true`) or implicitly by modals
  - [ ] Children are stacked relative to their context, not globally
  - [ ] Prevents z-index bleed-through between unrelated UI sections
- [ ] Local layers within each stacking context (ordered lowest to highest):
  - [ ] `content` — default layer for normal nodes
  - [ ] `overlay` — dropdowns, tooltips (above content, below modals)
  - [ ] `modal` — modals (creates a new nested stacking context)
- [ ] Global escape hatch: `top` layer at root level, bypasses all contexts (for toasts, drag visuals, etc.)
- [ ] Focus integration with layering:
  - [ ] Modal layer traps focus — Tab/arrows cycle within, can't escape
  - [ ] Overlay layer (dropdowns): focus moves in, but not hard-trapped (Tab out or Escape closes it, returns focus to trigger)
  - [ ] Content behind active modal is inert (not focusable)
  - [ ] Nested modals: inner modal traps focus, outer modal is inert until inner closes
- [ ] Focus stack for modals:
  - [ ] On modal open: push current focus to stack, auto-focus first focusable in modal
  - [ ] On modal close: pop stack, restore focus to previous node
  - [ ] If stored node no longer exists, fall back to first focusable in context
- [ ] `top` layer nodes: not focusable by default, optionally focusable if they have actions (e.g., dismissable toasts)

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
- [x] 1:1 mapping from DOM nodes to taffy layout nodes
- [x] Custom measure functions for text (terminal cell widths)
- [x] Careful integer rounding of layout results to avoid gaps/overlaps
- [ ] Positioning modes:
  - [ ] `Position::Flow` (default) — normal flexbox layout
  - [ ] `Position::Absolute { x, y }` — coordinates relative to stacking context
- [ ] Centering helpers (terminal cells are discrete — can't always center perfectly):
  - [ ] `center_x()` / `center_y()` — returns `Centered::Even(x)` or `Centered::Uneven(left, right)` when margins differ by 1
  - [ ] `any_center_x()` / `any_center_y()` — returns single coordinate (left/top-biased) when you don't care about off-by-one

## Styling

- [x] Inline styles only — no CSS selectors, no stylesheets, no cascade
- [x] Explicit inheritance via `StyleValue::Inherit` — nothing inherits unless specified
- [x] Style resolution walks parent chain for inherited values, caches results
- [ ] Pseudo-state style overrides (merged on top of base style):
  - [ ] `set_focus_style()` — when node is focused (hover = focus)
  - [ ] `set_active_style()` — when node is being pressed
  - [ ] `set_disabled_style()` — when node is disabled

## Borders

- [ ] Traditional box-drawing borders (style property):
  - [ ] Presets: single, double, rounded, thick, ascii, none
  - [ ] Custom charset support (user-defined characters)
  - [ ] Per-side control (top, right, bottom, left independently)
- [ ] Half-block edges (opt-in, later milestone):
  - [ ] Uses `▀▄▌▐` characters with fg/bg colors to create smooth color transitions
  - [ ] Renderer detects adjacent node colors at boundary cells
  - [ ] Modern look without traditional box-drawing characters

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
- [ ] Support all terminal capabilities (colors, bold, italic, underline, etc.)
- [x] Terminal resize handling:
  - [x] Auto-relayout on resize
  - [x] Resize event fired for user handlers
- [ ] Maximum FPS cap during active rendering (debounces rapid changes)
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
  - [x] Hide real terminal cursor (we render fake cursors as styled cells)
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
- [x] Built-in debug overlay (toggle-able):
  - [x] FPS / frame time
  - [ ] Node count
  - [x] Render latency
  - [x] Layout latency
  - [ ] Event latency

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
