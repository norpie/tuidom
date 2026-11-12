//! `tuidom-charts` — charts for tuidom.
//!
//! Charts are drawn into a canvas buffer from scales that place values along each axis.
#![warn(missing_docs)]

mod axes;
mod dots;
mod fill;
mod scale;

pub use axes::{Area, Axes, Axis, AxisStyle, band};
pub use dots::{DotGrid, Marker};
pub use fill::{fill_down, fill_left, fill_right, fill_up};
pub use scale::{LinearScale, Ticks, compact};
