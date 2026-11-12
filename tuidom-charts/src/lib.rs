//! `tuidom-charts` — charts for tuidom.
//!
//! Charts are drawn into a canvas buffer from scales that place values along each axis. The
//! default `components` feature adds themed chart components on `tuidom-ui`; without it the
//! crate needs only the engine.
#![warn(missing_docs)]

mod axes;
mod bars;
#[cfg(feature = "components")]
mod components;
mod dots;
mod fill;
mod plot;
mod scale;
mod sparkline;

pub use axes::{Area, Axes, Axis, AxisStyle, band};
pub use bars::{Bar, BarPlot, Orientation, draw_bars};
#[cfg(feature = "components")]
pub use components::{
    BarChart, BarChartProps, ChartBar, ChartSeries, LineChart, LineChartProps, ScatterChart,
    ScatterChartProps, Sparkline, SparklineProps, TickLabels,
};
pub use dots::{DotGrid, Marker};
pub use fill::{fill_down, fill_left, fill_right, fill_up};
pub use plot::{Domain, Plot, Series, draw_lines, draw_points};
pub use scale::{LinearScale, Ticks, compact};
pub use sparkline::draw_sparkline;
