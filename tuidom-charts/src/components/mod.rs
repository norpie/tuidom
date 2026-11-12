//! Chart components: the draw functions on a canvas that redraws itself when its data, its
//! size or its theme changes.
//!
//! Canvas cells hold resolved colors, so each redraw evaluates the chart's colors at its canvas
//! node. Reading the palette subscribes the redraw to theme switches; effects rerun in creation
//! order, so the theme's variables are already declared again when it runs.

mod bar;
mod sparkline;
#[cfg(test)]
mod tests;
mod xy;

use std::fmt;
use std::sync::Arc;

use tuidom::style::{Color, FlexDirection, FlexGap, FlexWrap, Length, ResolvedColor, Style};
use tuidom::{Document, NodeId};
use tuidom_ui::{Children, Derived, NodeRef, Signal, View, derived, view};

use crate::axes::AxisStyle;

pub use bar::{BarChart, BarChartProps};
pub use sparkline::{Sparkline, SparklineProps};
pub use xy::{LineChart, LineChartProps, ScatterChart, ScatterChartProps};

/// The labels on a chart component's numeric axis.
#[derive(Clone, Default)]
pub enum TickLabels {
    /// [`compact`](crate::compact) labels.
    #[default]
    Compact,
    /// A label per tick value from a closure.
    Custom(Arc<dyn Fn(f64) -> String + Send + Sync>),
}

impl TickLabels {
    /// Labels from `format`.
    pub fn new(format: impl Fn(f64) -> String + Send + Sync + 'static) -> Self {
        Self::Custom(Arc::new(format))
    }

    fn format(&self) -> Option<&dyn Fn(f64) -> String> {
        match self {
            Self::Compact => None,
            Self::Custom(format) => Some(format.as_ref()),
        }
    }
}

/// Custom labels are equal when they share a closure.
impl PartialEq for TickLabels {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Compact, Self::Compact) => true,
            (Self::Custom(a), Self::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Debug for TickLabels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compact => f.write_str("Compact"),
            Self::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

/// A series for [`LineChart`] or [`ScatterChart`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartSeries {
    /// The name in the legend. A series with no name is left out of it.
    pub name: String,
    /// The points as `(x, y)`. A point with a coordinate that is not finite is skipped, and
    /// breaks a line in two.
    pub points: Vec<(f64, f64)>,
    /// The series' color, or `None` for `--chart-1` to `--chart-5` by its position, starting
    /// over after the fifth.
    pub color: Option<Color>,
}

impl ChartSeries {
    /// A series named `name` over `points`, in its position's chart color.
    pub fn new(name: impl Into<String>, points: Vec<(f64, f64)>) -> Self {
        Self {
            name: name.into(),
            points,
            color: None,
        }
    }

    /// The series drawn in `color` instead.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// A bar for [`BarChart`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartBar {
    /// The label on the category axis.
    pub label: String,
    /// The bar's value. A value that is not finite draws no bar, but keeps its label.
    pub value: f64,
    /// The bar's color, or `None` for `--chart-1`.
    pub color: Option<Color>,
}

impl ChartBar {
    /// A bar labelled `label` at `value`, in `--chart-1`.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            color: None,
        }
    }

    /// The bar drawn in `color` instead.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// The chart color for the series at `index`.
fn chart_color(index: usize) -> Color {
    Color::var(format!("--chart-{}", index % 5 + 1))
}

/// Colors evaluated at a mounted canvas node.
struct Colors {
    doc: Document,
    node: NodeId,
    foreground: ResolvedColor,
}

impl Colors {
    /// The colors at `canvas`'s node, or `None` while it is not mounted.
    fn at(canvas: NodeRef) -> Option<Self> {
        let (doc, node) = (canvas.document()?, canvas.node()?);
        let foreground = doc.resolve_color(node, &Color::CurrentFg).ok().flatten()?;
        Some(Self {
            doc,
            node,
            foreground,
        })
    }

    /// `color` at the node, or the node's foreground when it names a variable not in scope.
    fn resolve(&self, color: &Color) -> ResolvedColor {
        self.doc
            .resolve_color(self.node, color)
            .ok()
            .flatten()
            .unwrap_or(self.foreground)
    }

    fn axes(&self) -> AxisStyle {
        let muted = self.resolve(&Color::var("--muted-foreground"));
        AxisStyle {
            line: Some(muted),
            label: Some(muted),
        }
    }
}

/// The canvas element's style: the space its frame leaves, which it is then drawn to.
fn canvas_style() -> Style {
    let mut style = Style::new();
    style.width(Length::Percent(100.0));
    style.flex_grow(1.0);
    style.flex_basis(Length::Cells(0));
    style
}

/// A chart's frame: a column of its title, its canvas, and its legend.
fn frame(
    title: Signal<String>,
    style: Signal<Style>,
    legend: Derived<Vec<(String, Color)>>,
    canvas: Children,
) -> View {
    let column = derived(move || {
        let mut column = Style::new();
        column.flex_direction(FlexDirection::Column);
        column.width(Length::Percent(100.0));
        column.height(Length::Percent(100.0));
        column.merge(&style.get());
        column
    });
    let entry = move |index: usize| legend.get().get(index).cloned().unwrap_or_default();
    view! {
        box {
            style: column.get(),
            if !title.get().is_empty() {
                text(title.get()) { style: heading() }
            },
            { canvas },
            if !legend.get().is_empty() {
                box {
                    style: legend_row(),
                    each index, _ in legend.get() key = index {
                        text(format!("■ {}", entry(index).0)) { style: swatch(entry(index).1) }
                    }
                }
            },
        }
    }
}

fn heading() -> Style {
    let mut style = Style::new();
    style.bold(true);
    style
}

fn legend_row() -> Style {
    let mut style = Style::new();
    style.flex_wrap(FlexWrap::Wrap);
    style.gap(FlexGap { row: 0, column: 2 });
    style
}

fn swatch(color: Color) -> Style {
    let mut style = Style::new();
    style.color(color);
    style
}
