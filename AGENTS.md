# tuidom

A DOM-based terminal UI library for Rust — the browser engine layer for TUIs.

## External Files

Read these at the start of every session:

- **General information**: @README.md
- **Glossary of terms**: @docs/GLOSSARY.md
- **Code style conventions**: @docs/STYLE.md

Read on demand, not auto-loaded:

- `docs/FEATURES.md` — what exists yet, as checkboxes. Consult it when the question is
  "is this built", not as background.
- `docs/README.md` — index of the guides, and the rule dividing them from the glossary.
- The guides themselves: `architecture`, `getting-started`, `layout`, `styling`, `colors`,
  `events`, `focus-and-selection`, `scrolling`, `virtualization`, `animation`,
  `rendering`, `testing`.

The glossary defines terms and links to the guide that explains each one. Follow the link
when you need the reasoning; the definition alone is usually enough to work from.

## Repository Layout

```
tuidom/src/
  document/     public API surface — one `impl Document` block per file, split by concern
  event/        event types, key codes, dispatch metadata
  style/        Style, StyleValue, ResolvedStyle, color, border, scrollbar
  layout/       taffy bridge, measurement, rounding
  render/       grid, paint, diff, terminal backend
  animation/    driver, keyframes, interpolable values
  virtualize/   window math, measurement cache, Virtualizer
  inner.rs      DocumentInner — all runtime state, keyed by NodeId
  headless.rs   headless runtime + simulated input (the verification path)
  paint_order.rs, geometry.rs, performance.rs, panic.rs, lock.rs, error.rs, id.rs
tuidom/
  examples/demo.rs    human-facing smoke test
  benches/frame.rs    criterion frame benchmark
  tests/              empty on purpose — tests live in-crate
tuidom-macros/  proc macros usable against the engine alone — today, `style!`
  src/style/          property table, value sugar, expansion
  tests/ui/           trybuild compile-fail cases for the macro's own diagnostics
docs/           guides, glossary, features, style
.plans/         plans + roadmaps (gitignored)
```

## Repo Patterns

Invariants that reading any single file will not teach:

- `Document` is `Arc<DocumentInner>`; every public method takes `&self`. Cloning is cheap
  and expected. Never wrap it in another `Arc`.
- Runtime state — scroll offsets, focus stack, selection, active node, pseudo styles,
  in-flight animations — lives on `DocumentInner` keyed by `NodeId`, **not** on `NodeData`.
  So node identity owns runtime state: rebuilding a subtree loses the user's scroll
  position, focus, and selection. Patch live nodes.
- Computed layout is not on the node either. It is published per frame as one
  `layout_snapshot` map, replaced under a single lock.
- **Never dispatch an event while holding a `nodes.get_mut` guard.** DashMap holds guards
  per key; a handler is downstream code that may touch that very node, and it deadlocks —
  not errors, deadlocks. Clone what the event needs inside the borrow, scope the borrow,
  dispatch after. `document/input.rs:apply_input_default_action_to` is the worked example.
- Default actions run **after** listeners. That is what makes `prevent_default` possible,
  and why observing engine-driven state needs its own event rather than inference.
- The render task never runs user code. Events it produces (post-frame, transition-end,
  animation-end) go through the runtime queue so handlers run on the event task.
- `#![warn(missing_docs)]` is on. Public items need doc comments.
- Tests live in-crate next to the code; `tuidom/tests/` is empty on purpose. Tests may
  `unwrap`; `src/` may not.
- One `Terminal` per process, enforced. Headless is how you test.

## Workflow 

- User is the final authority on all decisions, including design, planning and implementation.
- There will always be a final approval step from the user before any implementation is done. (a "go ahead" or similar, not enough to "we'll be working on this" or "we'll implement this")

## Planning

Plans live in `.plans/` directory (gitignored).

**For larger features (This is user determined, you will not suggest a feature is "large" or "small"):**
1. Always use planning before implementation
2. Discuss approach with user
3. Agree on implementation strategy
4. Write the plan in `.plans/` with checkboxes to track progress
5. Keep checkboxes updated as you work — don't add "implementation details" sections after implementing
6. After implementing a step, introduce the next step in the plan.

**When hitting snags or blockers:**
- **Discuss with user** instead of working around or taking shortcuts
- No silent workarounds or compromise on design
- User needs to know about issues early

## Verification

- **Never run the TUI to verify your work.** Do not launch the demo, do not drive it in a pty, do not scrape its escape output.
- The test suite is the verification path. It exists for this reason — headless rendering, screen inspection, and simulated input all exist so behavior can be asserted without a terminal.
- `examples/demo.rs` is for humans. Keep it working and update it when a feature warrants a smoke surface, but it is not your feedback loop.
- If something cannot be verified through tests, say so and discuss it with the user instead of reaching for the demo.

## Git

Only applicable when asked to commit automatically. If not asked, do not touch git, no "end of feature" summaries, no "final" commits, no "cleanup" commits.

- We work with small "atomic" commit.
- Messages are in conventional commit format, no body. 
- If you feel more context is needed, the commit is probably too big, discuss with user.
