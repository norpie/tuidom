# Components

`tuidom-components` is a themed component library on [`tuidom-ui`](reactive-ui.md): buttons,
inputs, containers, feedback, menus and virtualized data views, all styled from one theme.
Terms in **bold** are defined in the [glossary](GLOSSARY.md#components).

Each component's rustdoc lists its props and keys. This guide covers what they share: how the
theme reaches them, how to restyle them, and the conventions they follow. Two examples show
them in use: `examples/showcase.rs` presents each component on its own, and
`examples/tracker.rs` is a small app built from them.

## Installing a theme

Call `install_theme` once in the `App` body, and wrap the app in a `Toaster` if anything will
show toasts:

```rust
use tuidom_components::{Theme, Toaster, install_theme};
use tuidom_ui::{App, view};

App::new(|| {
    install_theme(Theme::default());
    view! { Toaster { Screen {} } }
})
.run()
.await
```

`install_theme` declares the palette as document color variables, points the document's
default foreground at `--foreground`, sets the terminal background assumption, paints the
root, and provides the theme to every component below. Without it, components still mount,
but every color they reference is an undeclared variable.

`Theme::default()` is the dark palette with the default shape. `Theme::light()` is the light
one.

## What a theme holds

A **Theme** is a palette, a shape, and whether the root is painted.

The **Palette** holds twenty colors, one per variable components reference: `--background`
and `--foreground`, `--card`, `--popover`, `--primary`, `--secondary`, `--muted`, `--accent`,
`--destructive`, `--success`, `--warning`, `--border`, `--input` and `--ring`, with a
`*-foreground` partner for those that text sits on. The names follow shadcn's theme variables,
and the dark and light palettes are shadcn's neutral ones. The light one has an off-white
background, so white cards and popovers stand apart from it without a border.

The **Shape** is the rest of the look:

- `border`, the charset bordered surfaces and lines such as dividers draw with. It stands in
  for a corner radius.
- `button`, `field` and `container`, each a **Surface**: how buttons, fields and containers
  are framed, with a border, with [half-block edges](styling.md#half-block-edges), or not at
  all. A border takes a cell on each side; half-block edges take none. Fields are the text
  input, the select trigger and the list a select or menu opens; containers are panels, modals
  and toasts.
- `density`, a **Density**: how much padding sits inside controls, compact or comfortable.
- `focus`, a **FocusMark**: how a focused component shows it, by recoloring its edge to
  `--ring`, underlining its label, or swapping its colors. A half-block button's edges are
  part of its fill, so a ring shifts the whole button 20% toward its text color instead. Where
  there is no edge at all, a ring tints a field's fill toward `--ring` and swaps a button's
  colors.

The default shape gives buttons and containers half-block edges and leaves fields unframed, so
the rounded charset shows only in lines such as dividers. Density is comfortable and the focus
mark is a ring.

Both are plain structs, so a variant of the default is a struct update:

```rust
use tuidom_components::{Shape, Surface, Theme};

let theme = Theme {
    shape: Shape { container: Surface::Bordered, ..Shape::default() },
    ..Theme::default()
};
```

**RootBackground** decides whether the root is filled with `--background`. `Transparent`
leaves the terminal's own background showing. The document still assumes the palette's
background is behind it, since derived colors and blending need some background to work
against, so it only looks right when the terminal's background is close to the palette's.

## Switching and scoping

`install_theme` returns the theme as a signal. Setting it restyles everything in place:

```rust
let theme = install_theme(Theme::dark());
// in a key handler
theme.set(Theme::light());
```

Nothing remounts. Components never name a concrete color, only variables, so redeclaring the
variables recolors them through the engine's [color variables](colors.md#color-variables), and
they read the shape from context through a signal.

A **ThemeScope** re-themes one subtree. It declares its theme's palette on its own box,
shadowing the outer one, and provides its theme to the components inside:

```rust
use tuidom::style::Color;

fn filled() -> Style {
    let mut style = Style::new();
    style.background(Color::var("--background"));
    style
}

view! {
    ThemeScope {
        theme: Theme::light(),
        style: filled(),
        Panel { title: "Preview".to_string(), Label { text: "Light inside".to_string() } },
    }
}
```

The scope paints nothing by itself, so text inside sits on whatever is behind it. A palette that
differs in lightness from the outer one usually wants the scope to fill itself with
`--background`, as above.

## How components use the palette

Unstyled text takes the document's
[default foreground](colors.md#the-default-foreground), which `install_theme` sets to
`Color::var("--foreground")`. That is evaluated at each node against the nearest declaration.
So a container that wants different plain text inside it redeclares `--foreground` for its
subtree. A `Panel` points it at `--card-foreground`, and a `StatusBar` at
`--muted-foreground`. Plain `text(..)` inside either gets the right color with no style.

Every component takes a `style` prop, merged over its own style with
[`Style::merge`](styling.md#layering-one-style-over-another). Whatever the prop sets wins and
whatever it leaves unset keeps the component's value, so adjusting one property never means
restating the rest:

```rust
fn wide() -> Style {
    let mut style = Style::new();
    style.width(Length::Cells(40));
    style
}

view! { TextInput { value: name, style: wide() } }
```

`Button` also takes `focus_style`, `active_style` and `disabled_style`, which merge over its
look in those [pseudo-states](styling.md#pseudo-states).

## Popups and the palette

`Select`, `Menu`, `Tooltip`, `Modal` and the toasts draw over everything, so they mount
through a [portal](reactive-ui.md#portals) under the document root. Color variables follow the
node tree, so a palette declared by a `ThemeScope` around the field does not reach a list
mounted under the root. Each popup therefore declares the nearest theme's palette on its own
root node. A select inside a light scope opens a light list over a dark screen.

Popups place themselves with [anchored positioning](layout.md#anchored-positioning) against the
node that opened them. They do not flip to the other side at the screen edge.

## Keyboard conventions

The components share a few rules, so they combine without fighting over keys:

- A composite control is one [tab stop](focus-and-selection.md#keyboard-navigation) and moves
  inside itself with the arrows. Lists, trees, tables and a split's divider take focus as a
  whole. Radio groups and tabs focus each option on its own, so hovering one shows the focus
  mark on it; only the selected one is a tab stop, and Enter or Space selects the focused one.
- At its end, a control gives the arrow back. Up on a list's first row, or Right on the last
  tab, moves focus on as spatial navigation would, instead of doing nothing.
- An open popup holds the keyboard. Up and Down move its highlight, as hovering a row does,
  Enter or Space picks, and Escape or a press outside closes it. Focus goes back to the control
  that opened it.
- A modal traps focus until it closes, then gives it back.
- Hovering focuses, so tooltips show on hover as well as focus.
- A control's own text is [not selectable](focus-and-selection.md#text-that-cannot-be-selected),
  so a click that wobbles a cell still clicks. Labels, panels, list and tree rows and table cells
  stay selectable.

A key handler on the document root still sees keys typed into a `TextInput`, because key
events bubble. The examples skip their shortcuts when the key's target is an input.

## The catalogue

### Primitives

`Button` calls `on_press` on a click, Enter or Space. Its `variant` sets the colors:
`Primary`, `Secondary`, `Destructive`, `Outline`, or `Ghost`. `Label` is a line of text in a
default or muted tone, and `Divider` is a line in `--border` across its container, in either
orientation.

```rust
view! {
    Button {
        label: "Delete".to_string(),
        variant: ButtonVariant::Destructive,
        on_press: move |_| remove(),
    }
}
```

### Inputs

Inputs are bound to the caller's signals through [bound signal props](reactive-ui.md#bound-signals):
the component writes the signal as the user edits, and a write to the signal updates the
component.

```rust
let name = state(String::new());
let subscribe = state(true);
let plan = state(0_usize);
let size = state(1_usize);
let volume = state(40.0);

view! {
    box {
        TextInput { value: name, placeholder: "Your name".to_string() },
        Checkbox { checked: subscribe, label: "Subscribe".to_string() },
        RadioGroup { options: plans(), selected: plan },
        Select { options: sizes(), selected: size },
        Slider { value: volume, step: 5.0 },
    }
}
```

`TextInput` binds a `String` and can mask it for passwords. `Checkbox` binds a `bool`.
`RadioGroup` and `Select` bind an index into their options. `Slider` binds an `f64` between
`min` and `max`, snapped to `step`. Each takes `disabled`.

### Containers

`Panel` is a titled surface in `--card`. `Split` divides its space between `first` and `second`
at a `ratio` the user can drag or move from the keyboard. `Tabs` draws a strip of tab names
over its children; it does not swap the children itself, so pick the pane with a `match`:

```rust
view! {
    Tabs {
        tabs: vec!["Tasks".to_string(), "Details".to_string()],
        selected: tab,
        match tab.get() {
            0 => { tasks_pane() },
            _ => { details_pane() },
        }
    }
}
```

`Modal` centers a panel over a dimmed, inert screen while its `open` signal is true. Escape or
a press outside closes it unless `dismissible` is false. `Scrollable` is a focusable area that
scrolls its children along one axis, with keyboard scrolling; it needs a size from its style or
its container.

### Feedback

`Spinner` turns on the engine clock while mounted. `ProgressBar` fills a track to a `value`
from 0 to 1, in a `tone`. `Tooltip` shows a line of text beside its children while focus is
inside them.

Toasts go through the `Toaster` wrapped around the app. A component body reads the handle with
`use_toaster`, and a handler shows a toast through it:

```rust
let toaster = use_toaster();
view! {
    Button {
        label: "Save".to_string(),
        on_press: move |_| {
            if let Some(toaster) = toaster {
                toaster.show(Toast { title: "Saved".to_string(), tone: Tone::Success, ..Toast::default() });
            }
        },
    }
}
```

The handle has to come from the body, because listeners run outside any scope and cannot read
context. Toasts stack in the bottom-right corner, hold for four seconds, and fade; a click
dismisses one.

### Navigation

`Menu` is a button that opens a list of items and reports the chosen one's index through
`on_select`. Items can carry a key hint, be disabled, or have a separator drawn above them. A
separator belongs to an item, so it shifts no index:

```rust
fn file_items() -> Vec<MenuItem> {
    vec![
        MenuItem::new("New").hint("n"),
        MenuItem::new("Redo").disabled(),
        MenuItem::new("Quit").hint("q").separated(),
    ]
}

view! { Menu { label: "File".to_string(), items: file_items(), on_select: move |i: usize| run(i) } }
```

A row of menus is a menu bar. `StatusBar` is a bar in `--muted` with a `left` and a `right`
slot, and `KeyHint` draws a key and what it does. The bar does not pin itself, so make it the
last child of a full-height column.

### Data

`List`, `Tree` and `Table` show large collections through a
[virtual list](reactive-ui.md#large-collections): only the rows in view exist. They share their
keys: Up and Down move the selection, PageUp and PageDown a screenful, Home and End to either
end, and the view scrolls as little as needed to keep the selection visible. Enter calls
`on_activate`. Because they window against their own height, their `style` must bound it: a
fixed height, or flex inside a parent that is bounded.

A `List` asks its parent for each row as it scrolls into view:

```rust
view! {
    List {
        count: 100_000,
        selected: item,
        render: move |i| text(format!("Item {i}")).into(),
        style: grow(),
    }
}
```

A `Tree` takes `TreeNode`s and binds the set of expanded ids and the selected id. Right and Left
expand and collapse, or move to the first child and to the parent. A `Table` takes `Column`s
of fixed widths and asks for each cell's text by row and column:

```rust
view! {
    Table {
        columns: vec![Column::new("PID", 6), Column::new("Name", 14)],
        count: processes.with(Vec::len),
        cell: move |(row, column): (usize, usize)| text_at(row, column),
        selected: process,
        style: grow(),
    }
}
```

A cell is asked for again when a signal its closure read changes, so a table over a signal
updates in place. Text wider than its column is cut short with `…`, and the header follows the
rows when they scroll sideways.

## Where to go next

- [Reactive UI](reactive-ui.md) — signals, templates and the component model underneath
- [Colors](colors.md#color-variables) — how variables resolve, which is how themes reach nodes
- [Styling](styling.md#layering-one-style-over-another) — the merge every `style` prop uses
