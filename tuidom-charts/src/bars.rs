//! Bar charts: a labelled value per bar, drawn from a zero baseline along a linear axis.

use tuidom::canvas::CanvasBuffer;
use tuidom::style::ResolvedColor;

use crate::axes::{Area, Axes, Axis, AxisStyle, X_SPACING, Y_SPACING, band};
use crate::fill::{fill_down, fill_left, fill_right, fill_up};
use crate::plot::Domain;
use crate::scale::LinearScale;

/// One bar: its label on the category axis, its value, and its color.
#[derive(Debug, Clone, Copy)]
pub struct Bar<'a> {
    /// The label under or beside the bar.
    pub label: &'a str,
    /// The bar's value. A value that is not finite draws no bar, but keeps its label.
    pub value: f64,
    /// The color the bar is filled in.
    pub color: ResolvedColor,
}

/// Which way bars grow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Upward from the x axis, the first bar on the left.
    #[default]
    Vertical,
    /// Rightward from the y axis, the first bar at the top.
    Horizontal,
}

/// A bar chart, drawn with [`draw_bars`].
#[derive(Clone, Copy)]
pub struct BarPlot<'a> {
    /// The bars, in order along the category axis.
    pub bars: &'a [Bar<'a>],
    /// Which way the bars grow.
    pub orientation: Orientation,
    /// The value axis's scale.
    pub domain: Domain,
    /// The value axis's tick labels, or `None` for [`compact`](crate::compact) ones.
    pub format: Option<&'a dyn Fn(f64) -> String>,
    /// The axes' colors, or `None` to draw no axes and plot over the whole buffer.
    pub axes: Option<AxisStyle>,
}

impl Default for BarPlot<'_> {
    fn default() -> Self {
        Self {
            bars: &[],
            orientation: Orientation::default(),
            domain: Domain::Data,
            format: None,
            axes: Some(AxisStyle::default()),
        }
    }
}

/// Draw `plot` into `buffer`, replacing what it held.
///
/// Each bar fills its [`band`] along the category axis. Bars grow from zero, or from the domain's nearer end when zero is
/// outside it: away from the baseline to an eighth of a cell, toward the axis to half a cell.
/// A bar's length is proportional to its value, so its end can sit up to half a cell from the
/// tick for the same value, which marks the middle of a cell.
pub fn draw_bars(buffer: &mut CanvasBuffer, plot: &BarPlot) {
    buffer.clear();
    let (width, height) = (buffer.width(), buffer.height());
    let vertical = plot.orientation == Orientation::Vertical;
    let scale = plot.scale(width, height);
    let area = match plot.axes {
        Some(style) => {
            let labels: Vec<String> = plot.bars.iter().map(|bar| bar.label.to_owned()).collect();
            let bands = Axis::Bands(&labels);
            let values = Axis::Linear {
                scale,
                format: plot.format,
            };
            let axes = if vertical {
                Axes::layout(width, height, &bands, &values)
            } else {
                Axes::layout(width, height, &values, &bands)
            };
            axes.draw(buffer, style);
            axes.plot()
        }
        None => Area {
            x: 0,
            y: 0,
            width,
            height,
        },
    };

    let (across, along) = if vertical {
        (area.width, area.height)
    } else {
        (area.height, area.width)
    };
    let length = f64::from(along);
    let fraction = |value: f64| {
        scale
            .fraction(value)
            .map(|fraction| fraction.clamp(0.0, 1.0))
    };
    let Some(zero) = fraction(0.0) else {
        return;
    };
    // The baseline falls on a cell edge, counted from the start of the value axis: the bottom
    // for vertical bars, the left for horizontal ones.
    let baseline = (zero * length).round();
    for (index, bar) in plot.bars.iter().enumerate() {
        let span = band(index, plot.bars.len(), across);
        let Some(end) = fraction(bar.value).map(|fraction| fraction * length) else {
            continue;
        };
        if span.is_empty() {
            continue;
        }
        let thickness = span.len() as u16;
        let edge = baseline as u16;
        if vertical {
            let x = area.x + span.start;
            let below = area.y + along - edge;
            if end > baseline {
                if let Some(bottom) = below.checked_sub(1) {
                    fill_up(buffer, x, bottom, thickness, end - baseline, bar.color);
                }
            } else {
                fill_down(buffer, x, below, thickness, baseline - end, bar.color);
            }
        } else {
            let y = area.y + span.start;
            let left = area.x + edge;
            if end > baseline {
                fill_right(buffer, left, y, thickness, end - baseline, bar.color);
            } else if let Some(right) = left.checked_sub(1) {
                fill_left(buffer, right, y, thickness, baseline - end, bar.color);
            }
        }
    }
}

impl BarPlot<'_> {
    /// The value axis's scale for a buffer `width` by `height`.
    fn scale(&self, width: u16, height: u16) -> LinearScale {
        if let Domain::Fixed(scale) = self.domain {
            return scale;
        }
        let values = self.bars.iter().map(|bar| bar.value);
        if !values.clone().any(f64::is_finite) {
            return LinearScale::UNIT;
        }
        let Some(scale) = LinearScale::from_values(values.chain([0.0])) else {
            return LinearScale::UNIT;
        };
        let count = match (self.orientation, self.axes) {
            (Orientation::Vertical, Some(_)) => height.saturating_sub(2) / Y_SPACING,
            (Orientation::Vertical, None) => height / Y_SPACING,
            (Orientation::Horizontal, _) => width / X_SPACING,
        };
        scale.nice(usize::from(count.max(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(buffer: &CanvasBuffer) -> Vec<String> {
        (0..buffer.height())
            .map(|y| {
                (0..buffer.width())
                    .map(|x| match buffer.get(x, y) {
                        Some(("", _)) | None => " ",
                        Some((symbol, _)) => symbol,
                    })
                    .collect()
            })
            .collect()
    }

    fn bar(label: &str, value: f64) -> Bar<'_> {
        Bar {
            label,
            value,
            color: ResolvedColor::red(),
        }
    }

    fn fixed(min: f64, max: f64) -> Domain {
        Domain::Fixed(LinearScale::new(min, max).unwrap())
    }

    fn drawn(width: u16, height: u16, plot: &BarPlot) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(width, height);
        draw_bars(&mut buffer, plot);
        rows(&buffer)
    }

    fn bare<'a>(bars: &'a [Bar<'a>], domain: Domain) -> BarPlot<'a> {
        BarPlot {
            bars,
            domain,
            axes: None,
            ..BarPlot::default()
        }
    }

    #[test]
    fn vertical_bars_fill_their_bands() {
        let bars = [bar("a", 1.0), bar("b", 2.0)];
        assert_eq!(drawn(4, 2, &bare(&bars, fixed(0.0, 2.0))), ["  █ ", "█ █ "]);
    }

    #[test]
    fn bars_end_to_an_eighth() {
        let bars = [bar("a", 3.0), bar("b", 8.0)];
        assert_eq!(drawn(4, 1, &bare(&bars, fixed(0.0, 8.0))), ["▃ █ "]);
    }

    #[test]
    fn vertical_bars_sit_over_their_labels() {
        let bars = [bar("Mon", 1.0), bar("Tue", 2.0)];
        let plot = BarPlot {
            bars: &bars,
            ..BarPlot::default()
        };
        assert_eq!(
            drawn(12, 6, &plot),
            [
                "2 ┤    ███  ",
                "  │    ███  ",
                "  │███ ███  ",
                "0 ┤███ ███  ",
                "  └─┬───┬───",
                "   Mon Tue  ",
            ]
        );
    }

    #[test]
    fn horizontal_bars_run_beside_their_labels() {
        let bars = [bar("x", 1.0), bar("yy", 2.0)];
        let plot = BarPlot {
            bars: &bars,
            orientation: Orientation::Horizontal,
            ..BarPlot::default()
        };
        assert_eq!(
            drawn(12, 4, &plot),
            [
                " x ┤████    ",
                "yy ┤████████",
                "   └┬──────┬",
                "    0      2",
            ]
        );
    }

    #[test]
    fn negative_bars_grow_from_the_baseline_to_half_a_cell() {
        let bars = [bar("a", 2.0), bar("b", -1.0)];
        assert_eq!(
            drawn(2, 6, &bare(&bars, Domain::Data)),
            ["█ ", "█ ", "█ ", " █", " ▀", "  "]
        );

        let bars = [bar("a", 2.0), bar("b", -1.5)];
        let plot = BarPlot {
            orientation: Orientation::Horizontal,
            ..bare(&bars, fixed(-2.0, 2.0))
        };
        assert_eq!(drawn(4, 2, &plot), ["  ██", "▐█  "]);
    }

    #[test]
    fn a_set_domain_cuts_bars_and_moves_the_baseline() {
        let bars = [bar("a", 75.0), bar("b", 150.0)];
        assert_eq!(
            drawn(2, 4, &bare(&bars, fixed(50.0, 100.0))),
            [" █", " █", "██", "██"]
        );
    }

    #[test]
    fn a_value_not_finite_keeps_its_label() {
        let bars = [bar("a", f64::NAN), bar("b", 1.0)];
        let plot = BarPlot {
            bars: &bars,
            ..BarPlot::default()
        };
        let rows = drawn(9, 4, &plot);
        assert_eq!(rows[0], "1 ┤   ██ ");
        assert_eq!(rows[3], "   a  b  ");
    }

    #[test]
    fn with_no_finite_values_the_domain_is_zero_to_one() {
        let bars = [bar("a", f64::NAN)];
        let plot = BarPlot {
            bars: &bars,
            ..BarPlot::default()
        };
        assert_eq!(plot.scale(10, 10), LinearScale::UNIT);
        assert_eq!(BarPlot::default().scale(10, 10), LinearScale::UNIT);
    }
}
