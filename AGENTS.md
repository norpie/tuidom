# tuidom

A DOM-based terminal UI library for Rust — the browser engine layer for TUIs.

## External Files

Read these at the start of every session:

- **General information**: @README.md
- **Glossary of terms**: @GLOSSARY.md
- **Feature requirements**: @FEATURES.md
- **Code style conventions**: @STYLE.md

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
