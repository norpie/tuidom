# tuidom

A DOM-based terminal UI library for Rust.

**[Documentation](docs/)** — [getting started](docs/getting-started.md),
[architecture](docs/architecture.md), and eleven other guides, plus a
[glossary](docs/GLOSSARY.md) of every concept in the engine.

## What

tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives for building sophisticated TUI applications. Like a web browser's rendering engine, it handles:

- DOM tree structure with arena-allocated nodes and stable handles
- Flexbox layout via taffy
- Modern OKLCH color system with derived colors
- Event system with bubbling and focus management
- Scrolling, clipping, and paint-time culling
- Transitions and animations
- Text selection and cursor management

```rust
use tuidom::{event::KeyCode, Document};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::new()?;

    let hello = doc.create_text("Press q to quit")?;
    doc.append_child(doc.root(), hello)?;

    let d = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        if key.code == KeyCode::Char('q') {
            d.quit();
        }
    })?;

    doc.run().await?;
    Ok(())
}
```

## Why

Existing TUI libraries mix layout, widgets, and application logic. tuidom separates concerns by providing just the engine layer — a foundation for building higher-level component systems, reactive frameworks, or complete UI libraries.

Think of it as the relationship between a browser engine (Chromium, WebKit) and frameworks built on top (React, Vue, Svelte). tuidom is the engine, you build the framework.

That boundary is deliberate, and it is why you will not find reactivity here. The engine has no signals, no subscriptions, and no component model — it exposes the primitives a framework needs and stays out of the way of how you drive them.

## Essentials

**Core Primitives:**
- Box, Text, Input, and Frames node types
- Arena-based DOM with `NodeId` handles
- Permanent document root node as the layout and rendering entry point
- Async-first with Tokio runtime
- Thread-safe `Document` with interior mutability

**Rendering:**
- Virtual screen buffer with cell-by-cell diffing
- Completely passive when idle, active during animations
- Crossterm backend
- Wide-character (CJK, emoji) aware text measurement and rendering

**Stacking & Focus:**
- Every subtree paints as an isolated unit, so `z_index` cannot bleed across siblings
- Spatial arrow navigation and focus trapping
- Screen-wide text selection with boundary respect

**Advanced Features:**
- Color variables and derivations (`CurrentBg.darken(0.1)`)
- Alpha blending at render time
- Built-in scrollbars, plus primitives for building virtualized collections
- Headless mode for testing with simulated input

## License

GPL-3.0-only. See [LICENSE](LICENSE).
