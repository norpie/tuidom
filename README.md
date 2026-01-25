# tuidom

A DOM-based terminal UI library for Rust.

## What

tuidom is the browser engine layer for terminal UIs — providing the fundamental primitives for building sophisticated TUI applications. Like a web browser's rendering engine, it handles:

- DOM tree structure with reactive updates
- Flexbox layout via taffy
- Modern OKLCH color system with derived colors
- Event system with bubbling and focus management
- Transitions and animations
- Text selection and cursor management

## Why

Existing TUI libraries mix layout, widgets, and application logic. tuidom separates concerns by providing just the engine layer — a foundation for building higher-level component systems, reactive frameworks, or complete UI libraries.

Think of it as the relationship between a browser engine (Chromium, WebKit) and frameworks built on top (React, Vue, Svelte). tuidom is the engine, you build the framework.

## Essentials

**Core Primitives:**
- Box, Text, Input, Frames, Canvas node types
- Arena-based DOM with `NodeId` handles
- Async-first with Tokio runtime
- Thread-safe `Document` with interior mutability

**Rendering:**
- Virtual screen buffer with cell-by-cell diffing
- Completely passive when idle, active during animations
- Crossterm backend with terminal capability detection

**Layering & Focus:**
- Stacking contexts prevent z-index bleed-through
- Spatial arrow navigation and focus trapping
- Screen-wide text selection with boundary respect

**Advanced Features:**
- Color variables and derivations (`CurrentBg.darken(0.1)`)
- Alpha blending at render time
- Built-in scrollbars and optional virtualization
- Headless mode for testing with simulated input
