# tuidom

A DOM-based terminal UI library for Rust, built in layers: an engine that owns the node tree,
a reactive framework on top of it, and a component library and a router on top of that.

**[Documentation](docs/)** — [getting started](docs/getting-started.md),
[architecture](docs/architecture.md), and thirteen other guides, plus a
[glossary](docs/GLOSSARY.md) of every concept in the engine and the crates above it.

## Crates

| Crate | What it is | Builds on |
|---|---|---|
| `tuidom` | The engine: node tree, layout, styling, events, focus, scrolling, animation, rendering | — |
| `tuidom-macros` | `style!`, for writing a `Style` as a property list | — |
| `tuidom-ui` | Signals, scopes, the `view!` template, components, and `App` to run them | `tuidom` |
| `tuidom-ui-macros` | `view!` and `#[component]` | — |
| `tuidom-components` | Themed components: buttons, inputs, menus, tables, modals, toasts, and more | `tuidom-ui` |
| `tuidom-router` | Layout chains: frames that stay mounted while the screen inside them changes | `tuidom-ui` |

```
tuidom-components    tuidom-router
          \              /
           tuidom-ui          (+ tuidom-ui-macros)
               |
            tuidom            (+ tuidom-macros)
```

The engine holds every node and does all layout, painting, event dispatch and focus handling.
`tuidom-ui` decides which engine calls happen and when: a signal read inside a view becomes a
subscription, and a change reruns only the bindings that read it, each ending in an ordinary
call such as `update_style` or `set_text_content`. There is no virtual DOM. It uses only the
engine's public API, and the component library and router in turn use `tuidom-ui` and the
engine's style types.

You can stop at any layer. Drive the engine by hand, use the framework and write your own
components, or build from the component library.

## What it looks like

On the engine alone, you create nodes and attach listeners yourself:

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

With `tuidom-ui`, state lives in signals and the template updates itself from them:

```rust
use tuidom_ui::{App, View, component, state, view};

#[component]
fn Counter(start: i32) -> View {
    let count = state(start.get());
    view! {
        box {
            focusable: true,
            on_click: move || count.update(|n| *n += 1),
            text(count.get()),
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    App::new(|| view! { Counter { start: 0 } }).run().await
}
```

## Why

Existing TUI libraries mix layout, widgets, and application logic. tuidom separates those
concerns, starting with an engine layer that is a foundation for component systems, reactive
frameworks, or complete UI libraries.

Think of the relationship between a browser engine (Chromium, WebKit) and the frameworks built
on top of it (React, Vue, Svelte). `tuidom` is the engine. `tuidom-ui` is one framework on it,
kept in this repository but in its own crate.

That boundary is deliberate. The `tuidom` crate has no signals, no subscriptions, and no
component model: it exposes the primitives a framework needs and stays out of the way of how
you drive them. Because `tuidom-ui` reaches the engine only through its public API, a
different framework could be built the same way.

## Essentials

### Engine (`tuidom`)

**Core Primitives:**
- Box, Text, Input, and Frames node types
- Canvas nodes showing cells the app draws, for charts and custom graphics
- Image nodes, drawn with kitty Unicode placeholders or half-block cells
- Arena-based DOM with `NodeId` handles
- Permanent document root node as the layout and rendering entry point
- Async-first with Tokio runtime
- Thread-safe `Document` with interior mutability

**Rendering:**
- Virtual screen buffer with cell-by-cell diffing
- Completely passive when idle, active during animations
- Crossterm backend
- Wide-character (CJK, emoji) aware text measurement and rendering
- Terminal detection: name, multiplexer, color depth, image protocols, default colors, pixel size

**Stacking & Focus:**
- Every subtree paints as an isolated unit, so `z_index` cannot bleed across siblings
- Spatial arrow navigation and focus trapping
- Screen-wide text selection with boundary respect

**Advanced Features:**
- Color variables and derivations (`CurrentBg.darken(0.1)`)
- Alpha blending at render time
- Built-in scrollbars, plus primitives for building virtualized collections
- Headless mode for testing with simulated input

### Framework (`tuidom-ui`)

- `Copy`, `Send + Sync` signals, derived values, effects, and batching
- Scopes that own what is created in them, with cleanup and spawned tasks
- `view!` templates with `if`, `match`, keyed `each`, and portals
- `#[component]` functions with typed props, children, slots, and callbacks
- Context, node refs, and virtual lists for large collections
- `App` to run it, and `Harness` to test it headless

### Components (`tuidom-components`)

- One theme, a palette plus a shape, switchable at runtime and scoped per subtree
- Controls, containers, feedback, menus, and virtualized tables, lists, and trees

### Routing (`tuidom-router`)

- The route is your own enum; a `match` over it builds the chain of layouts and leaf
- Layouts kept by navigation stay mounted, and their props update in place
- Opt-in retention, so returning to a screen finds it as it was left

## Running the examples

Tests are the verification path for every crate; the examples are for trying it by hand.

```sh
cargo run -p tuidom --example demo                  # most of the engine in one file
cargo run -p tuidom --example term_info             # what was detected about your terminal
cargo run -p tuidom --example canvas                # a canvas redrawn to whatever size it gets
cargo run -p tuidom --example image                 # an image in each fit and protocol
cargo run -p tuidom-components --example showcase   # each component on its own
cargo run -p tuidom-components --example tracker    # an app built from the components
cargo run -p tuidom-router --example mail           # a routed app
```

## License

GPL-3.0-only. See [LICENSE](LICENSE).
