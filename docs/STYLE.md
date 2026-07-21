# Code Style Guide

## Imports

- Use imports to avoid qualified paths in code
- Prefer `use std::collections::HashMap;` then use `HashMap` directly
- Avoid `std::collections::HashMap::new()` in code

## Module Organization

- Nest modules as needed for proper organization
- Split pure models into dedicated files (e.g., `style/models/*.rs`)
- Keep logic-heavy code in single files when splitting doesn't improve clarity

## Naming & Consistency

- Follow standard Rust conventions
- When introducing new concepts, define them in [`GLOSSARY.md`](GLOSSARY.md) and explain
  the reasoning in the guide that owns the area — the glossary says *what*, guides say *why*
- Maintain consistency within the codebase

## Error Handling

- **Never panic** — no `unwrap()`, `expect()`, or `panic!()` in library code
- Use types to communicate errors properly (`Option`, `Result`, custom types)
- Wrap all downstream callbacks (event handlers, Canvas render callbacks, etc.) in `catch_unwind`
- Log panics via `tracing` and continue execution

## Documentation

- Doc comments required on all public items
- Module-level docs where a module needs orientation, not on every file — a `//!` should
  say what a module is *for* and what invariant holds across it, not restate its name
- Examples developed alongside features to verify functionality
- Focus on "why" and usage, not "what" (code is self-documenting)

## Async Patterns

- Use `async fn` syntax
- Use `async_trait` crate when needed for trait methods
- Consistent spawning patterns with Tokio

## Code Quality

- Run `rustfmt` at end of feature implementations
- Run `clippy` at end of feature implementations
- Use autofix where possible
- Default configurations for both tools

## Testing

- Minimal unit tests where they make sense
- No mocking or superficial tests that don't verify actual behavior
- No doc tests
- Integration tests in `tests/` when requested
- Focus on meaningful test coverage, not metrics

## Development Workflow

- **No stubbed functions** — don't create functions with just `todo!()` or `unimplemented!()`
- **No empty placeholder files** — only create files when they contain actual implementation
- **Implement when needed** — build features incrementally, avoid premature scaffolding
