# tuidom

A DOM-based terminal UI library for Rust — the browser engine layer for TUIs.

## External Files

Read these at the start of every session:

- **General information**: @README.md
- **Glossary of terms**: @GLOSSARY.md
- **Feature requirements**: @FEATURES.md
- **Code style conventions**: @STYLE.md

## Planning

Plans live in `.plans/` directory (gitignored, may not show up with glob).

**For larger features:**
1. Always use planning before implementation
2. Discuss approach with user
3. Agree on implementation strategy
4. Write the plan in `.plans/` with checkboxes to track progress
5. Keep checkboxes updated as you work — don't add "implementation details" sections after implementing

**When hitting snags or blockers:**
- **Discuss with user** instead of working around or taking shortcuts
- No silent workarounds or compromise on design
- User needs to know about issues early

## Git

Only applicable when asked to commit automatically.

- We work with small "atomic" commit.
- Messages are in conventional commit format, no body. 
- If you feel more context is needed, the commit is probably too big, discuss with user.
