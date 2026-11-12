//! `tuidom-charts` — charts for tuidom.
//!
//! Charts are drawn into a canvas buffer from scales that place values along each axis.
#![warn(missing_docs)]

mod scale;

pub use scale::{LinearScale, Ticks, compact};
