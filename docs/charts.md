# Charts

`tuidom-charts` draws line, scatter and bar charts and sparklines into a
[canvas buffer](rendering.md#canvas). It comes in two layers. The draw functions need only the
engine. With the default `components` feature, it also provides chart components on
[`tuidom-ui`](reactive-ui.md), colored by the [`tuidom-components`](components.md) theme and
redrawn on their own. Terms in **bold** are defined in the [glossary](GLOSSARY.md#charts).
`examples/dashboard.rs` in the crate plots live data with the components.

## Drawing on the engine alone

Turn off the default features and the crate depends on `tuidom` alone. Each chart is a
description plus a draw function that fills a buffer, replacing what it held:

```rust
use tuidom::canvas::CanvasBuffer;
use tuidom::style::ResolvedColor;
use tuidom_charts::{Plot, Series, draw_lines};

let points = [(0.0, 3.0), (1.0, 5.0), (2.0, 4.0)];
let series = [Series { name: "load", points: &points, color: ResolvedColor::red() }];
let mut buffer = CanvasBuffer::new(width, height);
draw_lines(&mut buffer, &Plot { series: &series, ..Plot::default() });
doc.set_canvas(canvas, buffer)?;
```

`draw_points` takes the same `Plot` and draws a dot per point. `draw_bars` takes a `BarPlot`,
and `draw_sparkline` a slice of values. Draw at the size the canvas is given, which
`on_size_change` reports, and draw again when the data or that size changes. A redraw always
covers the whole buffer: chart buffers are small, and drawing part of one would mean keeping
track of what each cell came from.

The pieces the draw functions use are public too, for a chart of your own: `LinearScale`,
`DotGrid`, the block fills, `Axes` and `band`.

## Scales and ticks

A **linear scale** maps a **domain** of values onto an axis. `LinearScale::new(min, max)`
sets one; equal bounds are padded apart, so a flat series still has somewhere to sit.
`from_values` spans the finite values of an iterator.

**Ticks** fall on round values, 1, 2 or 5 times a power of ten, which `ticks(count)` aims to
give about `count` of. `nice(count)` widens a scale outward to its nearest ticks, so its ends
are labelled. Tick values are counted in whole steps rather than added up, so a step of 0.1
gives 0.3 and not 0.30000000000000004.

Labels default to `compact`: as many decimals as the step needs, and from a thousand up a k,
M, G or T suffix where that costs at most one decimal. A closure can replace it per axis.

A chart's `Domain` is either `Fixed(scale)` or `Data`. What `Data` means depends on the chart:

- Line and scatter charts span their points. The y axis is widened to round ticks, and the x
  axis is not, since x is usually an index or a time and padding it leaves empty space at the
  ends.
- Bar charts span the values and zero, widened to round ticks, since bars grow from zero.
- Sparklines run from zero to the largest value shown, unrounded: a sparkline shows a shape,
  not values to read off.

With a fixed domain, whatever lies outside it is cut off at the plot's edge.

## Lines and points

Lines and points are drawn on a **dot grid**, several dots to a cell. A **marker** chooses
how the dots are drawn:

- `Braille`, the default, gives 2 by 4 dots a cell. A cell holds one color, so where series
  share a cell the one drawn last shows.
- `HalfBlock` gives 1 by 2, with a color for each half. It is for fonts that draw braille
  badly, or for series that cross often.
- `Dot` draws a `•` per cell.

A line joins each pair of consecutive points. A point with a coordinate that is not finite
breaks it, so a gap in the data shows as a gap in the line, and a point between two gaps is
drawn as a single dot rather than vanishing.

## Bars and sparklines

Bars fill cells with block characters. Unicode has eighth blocks only along a cell's bottom
and left edges, so a bar growing up or right ends to an eighth of a cell, and one growing
down or left, which is a negative value, ends to half a cell.

A bar's length is its value's share of the domain times the plot's cells, measured from a
cell edge, so lengths stay proportional and zero is empty. A tick mark sits in the middle of a
cell, so a bar's end can sit up to half a cell from the tick for the same value. Measuring
bars from the middle of a cell instead would line them up with the ticks, but then a zero bar
would show as a half block.

Bars sit in **bands**, one per bar along the category axis. Every band is as wide, with a
cell left between neighbours when there is room, so bars never differ in thickness.

A sparkline draws a column per value, the newest at the right once they fill its width, with
no axes.

## Axes

`Axes::layout` splits a buffer into the axes and the **plot area** they leave:

- The x axis takes the bottom two rows, a line and its labels.
- The y axis takes a gutter as wide as its widest label, at most a third of the buffer, then
  a column for its line.

Scales map onto the plot area alone, above the x axis and right of the y axis, so data drawn
with the same scales lines up with the ticks and never covers an axis line.

Labels that do not fit are handled per axis. A y label wider than the gutter is cut short with
`…`. Numeric x labels are centred on their tick, and one that would touch the label before it
is dropped. A band label is cut to its band, so every bar keeps one.

## Chart components

The components are `LineChart`, `ScatterChart`, `BarChart` and `Sparkline`. Install a theme,
and give them data:

```rust
use tuidom::style::Color;
use tuidom_charts::{ChartSeries, Domain, LineChart, LineChartProps, LinearScale, TickLabels};

let percent = LinearScale::new(0.0, 100.0).expect("0 to 100 is a valid scale");
view! {
    LineChart {
        title: String::from("Load"),
        series: vec![
            ChartSeries::new("cpu", cpu.get()),
            ChartSeries::new("mem", mem.get()).color(Color::var("--primary")),
        ],
        y: Domain::Fixed(percent),
        y_labels: TickLabels::new(|value| format!("{value}%")),
    }
}
```

Each draws on a canvas that reports its size and redraws when its props, its size or the
theme change. Line and scatter charts show a bold title above the canvas and a legend of the
named series below it, as nodes, so they lay out and theme like the rest of the screen.

Tick labels are a `TickLabels` value rather than a callback: a prop has to be comparable to
skip redraws when it has not changed, and `Compact` is a default the chart can recognize.
Custom labels compare equal when they share a closure.

A chart fills its container: its width and height are 100%. Inside a box sized to its
content, which has no size to take a share of, it collapses to that content. Size the
container, or give the chart a `style` that grows it, such as a zero `flex_basis` and a
`flex_grow`.

## Colors and redraws

Series take the palette's `--chart-1` to `--chart-5` by position, starting over after the
fifth, and bars take `--chart-1`. Any `Color` can replace them, including another variable.
Axes are drawn in `--muted-foreground`.

A canvas cell holds a resolved color, not a variable, so a chart evaluates its colors at its
canvas node with [`resolve_color`](colors.md#evaluating-a-color-from-code) every time it
draws. That finds the nearest declaration, so a chart inside a `ThemeScope` takes the scope's
colors. Without a theme, the chart variables are undeclared and everything is drawn in the
canvas's foreground.

A chart reads the palette through `use_palette`, so a theme switch redraws it. The theme's
variables have to be declared again before that redraw evaluates them. Both are effects of
the same switch, and `tuidom-ui` reruns effects in the order they were created; the theme is
installed, or the `ThemeScope` mounted, before any chart inside it, so its declarations always
go first.

Only the theme triggers that redraw. A chart variable changed some other way, such as
`--chart-1` set on a node's own style, is not seen until the chart next draws for another
reason.

## Where to go next

- [Rendering](rendering.md#canvas) — canvas nodes and their buffers, which every chart draws
  into
- [Components](components.md) — themes and palettes, which the chart components take their
  colors from
- [Reactive UI](reactive-ui.md#testing-with-harness) — `Harness`, which the components' tests
  and the dashboard example's use
