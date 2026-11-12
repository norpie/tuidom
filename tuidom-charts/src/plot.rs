//! Line and scatter charts: series of points plotted as dots over a pair of linear axes.

use tuidom::canvas::CanvasBuffer;
use tuidom::style::ResolvedColor;

use crate::axes::{Area, Axes, Axis, AxisStyle, Y_SPACING};
use crate::dots::{DotGrid, Marker};
use crate::scale::LinearScale;

/// A named run of points in one color.
#[derive(Debug, Clone, Copy)]
pub struct Series<'a> {
    /// The series' name, for a legend.
    pub name: &'a str,
    /// The points as `(x, y)`, in drawing order. A point with a coordinate that is not finite
    /// is skipped, and breaks a line in two.
    pub points: &'a [(f64, f64)],
    /// The color its dots are drawn in.
    pub color: ResolvedColor,
}

/// Where an axis's scale comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Domain {
    /// Taken from the values drawn. Line and scatter charts span them, widened on the y axis to
    /// the nearest round ticks. Bar charts span them and zero, widened the same way. A
    /// sparkline runs from zero to the largest.
    #[default]
    Data,
    /// A set scale. What lies outside it is cut off at the plot's edge.
    Fixed(LinearScale),
}

/// A line or scatter chart, drawn with [`draw_lines`] or [`draw_points`].
#[derive(Clone, Copy)]
pub struct Plot<'a> {
    /// The series, drawn in order, so where two share a cell the later one's color shows.
    pub series: &'a [Series<'a>],
    /// The x axis's scale.
    pub x: Domain,
    /// The y axis's scale.
    pub y: Domain,
    /// The x tick labels, or `None` for [`compact`](crate::compact) ones.
    pub x_format: Option<&'a dyn Fn(f64) -> String>,
    /// The y tick labels, or `None` for [`compact`](crate::compact) ones.
    pub y_format: Option<&'a dyn Fn(f64) -> String>,
    /// How dots are drawn.
    pub marker: Marker,
    /// The axes' colors, or `None` to draw no axes and plot over the whole buffer.
    pub axes: Option<AxisStyle>,
}

impl Default for Plot<'_> {
    fn default() -> Self {
        Self {
            series: &[],
            x: Domain::Data,
            y: Domain::Data,
            x_format: None,
            y_format: None,
            marker: Marker::default(),
            axes: Some(AxisStyle::default()),
        }
    }
}

/// Draw `plot` into `buffer` as lines joining each series' consecutive points, replacing what
/// the buffer held. A point between two gaps is drawn as a single dot.
pub fn draw_lines(buffer: &mut CanvasBuffer, plot: &Plot) {
    draw(buffer, plot, |grid, points, color| {
        for run in points.split(|point| !point.0.is_finite() || !point.1.is_finite()) {
            match run {
                [] => {}
                [point] => grid.line(*point, *point, color),
                _ => {
                    for pair in run.windows(2) {
                        grid.line(pair[0], pair[1], color);
                    }
                }
            }
        }
    });
}

/// Draw `plot` into `buffer` as a dot per point, replacing what the buffer held. Points outside
/// the scales are left out.
pub fn draw_points(buffer: &mut CanvasBuffer, plot: &Plot) {
    draw(buffer, plot, |grid, points, color| {
        let (width, height) = grid.dots();
        for &(x, y) in points {
            let inside = |value: f64, dots: u32| (0.0..=f64::from(dots - 1)).contains(&value);
            if inside(x, width) && inside(y, height) {
                grid.set(x.round() as u32, y.round() as u32, color);
            }
        }
    });
}

/// Lay out and draw the axes, then plot each series with its points already in dot
/// coordinates, a coordinate that is not finite marking a gap.
fn draw(
    buffer: &mut CanvasBuffer,
    plot: &Plot,
    mut series: impl FnMut(&mut DotGrid, &[(f64, f64)], ResolvedColor),
) {
    buffer.clear();
    let (width, height) = (buffer.width(), buffer.height());
    let (x, y) = plot.scales(height);
    let area = match plot.axes {
        Some(style) => {
            let axes = Axes::layout(
                width,
                height,
                &Axis::Linear {
                    scale: x,
                    format: plot.x_format,
                },
                &Axis::Linear {
                    scale: y,
                    format: plot.y_format,
                },
            );
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

    let mut grid = DotGrid::new(area.width, area.height, plot.marker);
    let (dots_x, dots_y) = grid.dots();
    if dots_x == 0 || dots_y == 0 {
        return;
    }
    let (last_x, last_y) = (f64::from(dots_x - 1), f64::from(dots_y - 1));
    let mut points = Vec::new();
    for each in plot.series {
        points.clear();
        points.extend(
            each.points
                .iter()
                .map(|&(px, py)| match (x.fraction(px), y.fraction(py)) {
                    (Some(fx), Some(fy)) => (fx * last_x, (1.0 - fy) * last_y),
                    _ => (f64::NAN, f64::NAN),
                }),
        );
        series(&mut grid, &points, each.color);
    }
    grid.draw(buffer, area.x, area.y);
}

impl Plot<'_> {
    /// The x and y scales for a buffer `height` rows high.
    fn scales(&self, height: u16) -> (LinearScale, LinearScale) {
        let finite = || {
            self.series
                .iter()
                .flat_map(|series| series.points)
                .filter(|(x, y)| x.is_finite() && y.is_finite())
        };
        let resolve = |domain: Domain, values: Vec<f64>| match domain {
            Domain::Fixed(scale) => Some(scale),
            Domain::Data => LinearScale::from_values(values),
        };
        let x =
            resolve(self.x, finite().map(|point| point.0).collect()).unwrap_or(LinearScale::UNIT);
        let mut y =
            resolve(self.y, finite().map(|point| point.1).collect()).unwrap_or(LinearScale::UNIT);
        if self.y == Domain::Data {
            let rows = match self.axes {
                Some(_) => height.saturating_sub(2),
                None => height,
            };
            y = y.nice(usize::from((rows / Y_SPACING).max(1)));
        }
        (x, y)
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

    fn fixed(min: f64, max: f64) -> Domain {
        Domain::Fixed(LinearScale::new(min, max).unwrap())
    }

    fn series(points: &[(f64, f64)]) -> Series<'_> {
        Series {
            name: "",
            points,
            color: ResolvedColor::red(),
        }
    }

    fn bare<'a>(series: &'a [Series<'a>]) -> Plot<'a> {
        Plot {
            series,
            x: fixed(0.0, 3.0),
            y: fixed(0.0, 3.0),
            axes: None,
            ..Plot::default()
        }
    }

    fn lines(width: u16, height: u16, plot: &Plot) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(width, height);
        draw_lines(&mut buffer, plot);
        rows(&buffer)
    }

    fn points(width: u16, height: u16, plot: &Plot) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(width, height);
        draw_points(&mut buffer, plot);
        rows(&buffer)
    }

    #[test]
    fn lines_join_consecutive_points() {
        let data = [series(&[(0.0, 0.0), (3.0, 3.0)])];
        assert_eq!(lines(2, 1, &bare(&data)), ["⡠⠊"]);
    }

    #[test]
    fn a_point_not_finite_breaks_the_line() {
        let data = [series(&[
            (0.0, 0.0),
            (f64::NAN, 1.0),
            (2.0, 2.0),
            (3.0, 3.0),
        ])];
        assert_eq!(lines(2, 1, &bare(&data)), ["⡀⠊"]);
    }

    #[test]
    fn lines_are_cut_at_the_edge_of_a_set_domain() {
        let data = [series(&[(0.0, 0.0), (3.0, 6.0)])];
        assert_eq!(lines(2, 1, &bare(&data)), ["⡰⠁"]);
    }

    #[test]
    fn points_outside_a_set_domain_are_left_out() {
        let data = [series(&[(0.0, 0.0), (3.0, 3.0), (5.0, 1.0), (1.0, -1.0)])];
        assert_eq!(points(2, 1, &bare(&data)), ["⡀⠈"]);
    }

    #[test]
    fn the_later_series_wins_a_shared_cell() {
        let data = [
            series(&[(0.0, 0.0)]),
            Series {
                color: ResolvedColor::blue(),
                ..series(&[(0.0, 3.0)])
            },
        ];
        let mut buffer = CanvasBuffer::new(2, 1);
        draw_points(&mut buffer, &bare(&data));
        assert_eq!(rows(&buffer), ["⡁ "]);
        assert_eq!(buffer.get(0, 0).unwrap().1.fg, Some(ResolvedColor::blue()));
    }

    #[test]
    fn markers_set_how_dots_are_drawn() {
        let data = [
            series(&[(0.0, 1.0)]),
            Series {
                color: ResolvedColor::blue(),
                ..series(&[(0.0, 0.0)])
            },
        ];
        let plot = Plot {
            x: fixed(0.0, 1.0),
            y: fixed(0.0, 1.0),
            marker: Marker::HalfBlock,
            ..bare(&data)
        };
        let mut buffer = CanvasBuffer::new(1, 1);
        draw_points(&mut buffer, &plot);
        let (symbol, style) = buffer.get(0, 0).unwrap();
        assert_eq!(symbol, "▀");
        assert_eq!(style.fg, Some(ResolvedColor::red()));
        assert_eq!(style.bg, Some(ResolvedColor::blue()));

        let data = [series(&[(0.0, 0.0), (2.0, 0.0)])];
        let plot = Plot {
            x: fixed(0.0, 2.0),
            y: fixed(0.0, 1.0),
            marker: Marker::Dot,
            ..bare(&data)
        };
        assert_eq!(lines(3, 1, &plot), ["•••"]);
    }

    #[test]
    fn a_data_domain_spans_the_points_and_rounds_the_y_ends() {
        let data = [
            series(&[(2.0, 3.0), (f64::NAN, 500.0), (5.0, f64::INFINITY)]),
            series(&[(9.0, 97.0), (4.0, 40.0)]),
        ];
        let plot = Plot {
            series: &data,
            ..Plot::default()
        };
        let (x, y) = plot.scales(20);
        assert_eq!(x.domain(), (2.0, 9.0));
        assert_eq!(y.domain(), (0.0, 100.0));

        let plot = Plot {
            series: &[],
            ..Plot::default()
        };
        let (x, y) = plot.scales(20);
        assert_eq!(x.domain(), (0.0, 1.0));
        assert_eq!(y.domain(), (0.0, 1.0));
    }

    #[test]
    fn the_plot_sits_inside_the_axes() {
        let data = [series(&[(0.0, 0.0), (10.0, 100.0)])];
        let plot = Plot {
            series: &data,
            x: fixed(0.0, 10.0),
            y: fixed(0.0, 100.0),
            ..Plot::default()
        };
        let mut buffer = CanvasBuffer::new(20, 8);
        draw_lines(&mut buffer, &plot);

        let x = Axis::Linear {
            scale: LinearScale::new(0.0, 10.0).unwrap(),
            format: None,
        };
        let y = Axis::Linear {
            scale: LinearScale::new(0.0, 100.0).unwrap(),
            format: None,
        };
        let area = Axes::layout(20, 8, &x, &y).plot();
        let cell = |x, y| buffer.get(x, y).unwrap().0;
        assert_eq!(cell(area.x - 1, 0), "┤");
        assert_eq!(cell(area.x, area.height - 1), "⡠");
        assert_eq!(cell(area.x + area.width - 1, 0), "⠊");
    }

    #[test]
    fn with_no_points_only_the_axes_are_drawn_over_a_cleared_buffer() {
        let mut buffer = CanvasBuffer::new(12, 5);
        buffer.text(0, 0, "leftover", Default::default());
        draw_lines(&mut buffer, &Plot::default());

        let unit = Axis::Linear {
            scale: LinearScale::UNIT,
            format: None,
        };
        let mut expected = CanvasBuffer::new(12, 5);
        Axes::layout(12, 5, &unit, &unit).draw(&mut expected, AxisStyle::default());
        assert_eq!(rows(&buffer), rows(&expected));
    }
}
