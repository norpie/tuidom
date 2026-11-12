//! Axes along a chart's left and bottom edges: their lines, tick marks and labels, and the plot
//! area they leave.
//!
//! Scales map onto the plot area alone, which sits above the x axis line and right of the y axis
//! line, so data drawn with the same scales lines up with the ticks and never covers an axis.

use std::ops::Range;

use tuidom::canvas::{CanvasBuffer, CanvasStyle};
use tuidom::style::ResolvedColor;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::scale::{LinearScale, compact};

/// Rows per y tick and columns per x tick the axes aim for.
const Y_SPACING: u16 = 3;
const X_SPACING: u16 = 10;

/// What one edge of a chart shows.
#[derive(Clone, Copy)]
pub enum Axis<'a> {
    /// Values along a scale, ticked at round steps.
    Linear {
        /// The scale the plot area is mapped by along this axis.
        scale: LinearScale,
        /// The label for a tick value, or `None` for [`compact`] labels.
        format: Option<&'a dyn Fn(f64) -> String>,
    },
    /// Categories in equal slots, the first at the left or the top, each labelled at its
    /// centre. See [`band`] for the slots.
    Bands(&'a [String]),
    /// No line and no labels, taking no space.
    Hidden,
}

/// A rectangle of cells in a buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Area {
    /// The leftmost column.
    pub x: u16,
    /// The top row.
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

/// Colors for axes. `None` draws in the canvas node's foreground.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AxisStyle {
    /// The axis lines and tick marks.
    pub line: Option<ResolvedColor>,
    /// The tick labels.
    pub label: Option<ResolvedColor>,
}

/// Axes laid out over a buffer, and the plot area they leave.
///
/// The x axis takes the bottom two rows, a line and a row of labels. The y axis takes a gutter
/// as wide as its widest label, at most a third of the buffer, then a column for its line. Labels
/// that do not fit are cut short with `…`, and x labels that would touch are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Axes {
    plot: Area,
    x: Option<Ruler>,
    y: Option<Ruler>,
}

#[derive(Debug, Clone, PartialEq)]
struct Ruler {
    /// The x axis line's row, or the y axis line's column.
    line: u16,
    /// Tick columns for the x axis, rows for the y axis.
    ticks: Vec<u16>,
    labels: Vec<Label>,
}

#[derive(Debug, Clone, PartialEq)]
struct Label {
    x: u16,
    y: u16,
    text: String,
}

/// A tick or band centre, as an offset along the plot area, with its label.
struct Mark {
    offset: u16,
    text: String,
    /// The band's cells, for a band axis.
    slot: Option<Range<u16>>,
}

impl Axes {
    /// Lay out axes `x` and `y` over a buffer of `width` by `height` cells.
    pub fn layout(width: u16, height: u16, x: &Axis, y: &Axis) -> Self {
        let x_shown = !matches!(x, Axis::Hidden);
        let y_shown = !matches!(y, Axis::Hidden);
        let plot_height = if x_shown {
            height.saturating_sub(2)
        } else {
            height
        };

        let y_marks = marks(y, plot_height, Y_SPACING, true);
        let label_width = y_marks
            .iter()
            .map(|mark| text_width(&mark.text))
            .max()
            .unwrap_or(0)
            .min(width / 3);
        let gutter = if label_width == 0 { 0 } else { label_width + 1 };
        let plot_x = if y_shown { gutter + 1 } else { 0 };
        let plot_width = width.saturating_sub(plot_x);

        let y_ruler = y_shown.then(|| Ruler {
            line: gutter,
            ticks: y_marks.iter().map(|mark| mark.offset).collect(),
            labels: y_marks
                .into_iter()
                .filter_map(|mark| {
                    let text = fit(&mark.text, label_width);
                    let x = label_width.saturating_sub(text_width(&text));
                    (!text.is_empty()).then_some(Label {
                        x,
                        y: mark.offset,
                        text,
                    })
                })
                .collect(),
        });

        let x_ruler = x_shown.then(|| {
            let marks = marks(x, plot_width, X_SPACING, false);
            Ruler {
                line: plot_height,
                ticks: marks.iter().map(|mark| plot_x + mark.offset).collect(),
                labels: x_labels(marks, plot_x, plot_height.saturating_add(1), width),
            }
        });

        Self {
            plot: Area {
                x: plot_x,
                y: 0,
                width: plot_width,
                height: plot_height,
            },
            x: x_ruler,
            y: y_ruler,
        }
    }

    /// The cells left for the chart itself.
    pub fn plot(&self) -> Area {
        self.plot
    }

    /// Draw the axes into `buffer`.
    pub fn draw(&self, buffer: &mut CanvasBuffer, style: AxisStyle) {
        let line = CanvasStyle {
            fg: style.line,
            ..CanvasStyle::default()
        };
        let label = CanvasStyle {
            fg: style.label,
            ..CanvasStyle::default()
        };
        if let Some(y) = &self.y {
            for row in self.plot.y..self.plot.y + self.plot.height {
                let symbol = if y.ticks.contains(&row) { "┤" } else { "│" };
                buffer.set(y.line, row, symbol, line);
            }
        }
        if let Some(x) = &self.x {
            for column in self.plot.x..self.plot.x + self.plot.width {
                let symbol = if x.ticks.contains(&column) {
                    "┬"
                } else {
                    "─"
                };
                buffer.set(column, x.line, symbol, line);
            }
            if let Some(y) = &self.y {
                buffer.set(y.line, x.line, "└", line);
            }
        }
        for each in self.x.iter().chain(&self.y).flat_map(|ruler| &ruler.labels) {
            buffer.text(each.x, each.y, &each.text, label);
        }
    }
}

/// The cells of slot `index` when `length` cells are split into `count` slots, as offsets from
/// the start. Slots differ in size by at most a cell. A slot past `count`, or one too narrow to
/// get a cell, is empty.
pub fn band(index: usize, count: usize, length: u16) -> Range<u16> {
    if index >= count {
        return length..length;
    }
    let edge = |index: usize| (index as u128 * u128::from(length) / count as u128) as u16;
    edge(index)..edge(index + 1)
}

/// The marks along an axis `length` cells long. `upward` counts a linear axis's offsets from
/// its far end, for a y axis whose values rise toward the top.
fn marks(axis: &Axis, length: u16, spacing: u16, upward: bool) -> Vec<Mark> {
    if length == 0 {
        return Vec::new();
    }
    let mut marks: Vec<Mark> = Vec::new();
    match axis {
        Axis::Linear { scale, format } => {
            let ticks = scale.ticks(usize::from((length / spacing).max(1)));
            for value in ticks.values {
                let Some(index) = scale.index(value, usize::from(length)) else {
                    continue;
                };
                let Ok(index) = u16::try_from(index) else {
                    continue;
                };
                let offset = if upward { length - 1 - index } else { index };
                if marks.last().is_some_and(|last| last.offset == offset) {
                    continue;
                }
                let text = match format {
                    Some(format) => format(value),
                    None => compact(value, ticks.step),
                };
                marks.push(Mark {
                    offset,
                    text,
                    slot: None,
                });
            }
        }
        Axis::Bands(labels) => {
            for (index, text) in labels.iter().enumerate() {
                let slot = band(index, labels.len(), length);
                if slot.is_empty() {
                    continue;
                }
                marks.push(Mark {
                    offset: slot.start + (slot.len() as u16 - 1) / 2,
                    text: text.clone(),
                    slot: Some(slot),
                });
            }
        }
        Axis::Hidden => {}
    }
    marks
}

/// Labels for x marks on `row`, in a buffer `width` wide. A band label is cut to its slot, less
/// a column to keep it apart from the next. A tick label is centred on its tick, kept inside the
/// buffer, and dropped when it would touch the label before it.
fn x_labels(marks: Vec<Mark>, plot_x: u16, row: u16, width: u16) -> Vec<Label> {
    let mut labels = Vec::new();
    let mut free = 0;
    for mark in marks {
        let (text, x) = match mark.slot {
            Some(slot) => {
                let length = slot.len() as u16;
                let room = if length >= 2 { length - 1 } else { length };
                let text = fit(&mark.text, room);
                let x = plot_x + slot.start + (length - text_width(&text)) / 2;
                (text, x)
            }
            None => {
                let text = fit(&mark.text, width);
                let extent = text_width(&text);
                let x = (plot_x + mark.offset)
                    .saturating_sub(extent / 2)
                    .min(width - extent);
                if x < free {
                    continue;
                }
                free = x.saturating_add(extent).saturating_add(1);
                (text, x)
            }
        };
        if !text.is_empty() {
            labels.push(Label { x, y: row, text });
        }
    }
    labels
}

/// `text` cut to `width` columns, ending in `…` when cut.
fn fit(text: &str, width: u16) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    let mut fitted = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let grapheme_width = grapheme_width(grapheme);
        if used + grapheme_width >= width {
            break;
        }
        used += grapheme_width;
        fitted.push_str(grapheme);
    }
    if width > 0 {
        fitted.push('…');
    }
    fitted
}

/// The columns `text` takes in a canvas buffer, saturating at `u16::MAX`.
fn text_width(text: &str) -> u16 {
    text.graphemes(true).fold(0u16, |width, grapheme| {
        width.saturating_add(grapheme_width(grapheme))
    })
}

/// A grapheme's width as a canvas buffer writes it.
fn grapheme_width(grapheme: &str) -> u16 {
    if grapheme.chars().any(char::is_control) {
        return 0;
    }
    grapheme.width().min(2) as u16
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

    fn drawn(width: u16, height: u16, x: &Axis, y: &Axis) -> (Area, Vec<String>) {
        let axes = Axes::layout(width, height, x, y);
        let mut buffer = CanvasBuffer::new(width, height);
        axes.draw(&mut buffer, AxisStyle::default());
        (axes.plot(), rows(&buffer))
    }

    fn linear(min: f64, max: f64) -> Axis<'static> {
        Axis::Linear {
            scale: LinearScale::new(min, max).unwrap(),
            format: None,
        }
    }

    #[test]
    fn axes_take_a_gutter_and_the_bottom_rows() {
        let (plot, rows) = drawn(20, 8, &linear(0.0, 10.0), &linear(0.0, 100.0));
        assert_eq!(
            plot,
            Area {
                x: 5,
                y: 0,
                width: 15,
                height: 6
            }
        );
        assert_eq!(
            rows,
            [
                "100 ┤               ",
                "    │               ",
                " 50 ┤               ",
                "    │               ",
                "    │               ",
                "  0 ┤               ",
                "    └┬─────────────┬",
                "     0            10",
            ]
        );
    }

    #[test]
    fn ticks_sit_where_the_scale_puts_their_values() {
        let scale = LinearScale::new(-3.0, 17.0).unwrap();
        let axis = Axis::Linear {
            scale,
            format: None,
        };
        let axes = Axes::layout(40, 20, &axis, &axis);
        let plot = axes.plot();
        let mut buffer = CanvasBuffer::new(40, 20);
        axes.draw(&mut buffer, AxisStyle::default());

        let ticks = scale.ticks(usize::from(plot.width / X_SPACING)).values;
        for value in ticks {
            let column = plot.x + scale.index(value, usize::from(plot.width)).unwrap() as u16;
            assert_eq!(buffer.get(column, plot.height).unwrap().0, "┬");
        }
        let ticks = scale.ticks(usize::from(plot.height / Y_SPACING)).values;
        for value in ticks {
            let index = scale.index(value, usize::from(plot.height)).unwrap() as u16;
            assert_eq!(
                buffer.get(plot.x - 1, plot.height - 1 - index).unwrap().0,
                "┤"
            );
        }
    }

    #[test]
    fn x_labels_that_would_touch_are_dropped() {
        let format = |value: f64| format!("{value} items!!");
        let x = Axis::Linear {
            scale: LinearScale::new(0.0, 100.0).unwrap(),
            format: Some(&format),
        };
        let (_, rows) = drawn(30, 2, &x, &Axis::Hidden);
        assert_eq!(
            rows,
            [
                "┬──────────────┬─────────────┬",
                "0 items!! 50 items!!          ",
            ]
        );
    }

    #[test]
    fn band_labels_are_cut_to_their_slot() {
        let days = ["Monday", "Tuesday", "Wednesday"].map(String::from);
        let (_, rows) = drawn(15, 2, &Axis::Bands(&days), &Axis::Hidden);
        assert_eq!(rows, ["──┬────┬────┬──", "Mon… Tue… Wed… "]);
    }

    #[test]
    fn bands_on_the_y_axis_run_from_the_top() {
        let labels = ["a", "bb"].map(String::from);
        let (plot, rows) = drawn(10, 4, &Axis::Hidden, &Axis::Bands(&labels));
        assert_eq!(
            plot,
            Area {
                x: 4,
                y: 0,
                width: 6,
                height: 4
            }
        );
        assert_eq!(
            rows,
            [" a ┤      ", "   │      ", "bb ┤      ", "   │      "]
        );
    }

    #[test]
    fn bands_split_a_length_evenly() {
        assert_eq!(band(0, 3, 10), 0..3);
        assert_eq!(band(1, 3, 10), 3..6);
        assert_eq!(band(2, 3, 10), 6..10);
        assert!(band(3, 3, 10).is_empty());
        assert!(band(0, 0, 10).is_empty());
        let slots: Vec<_> = (0..20).map(|index| band(index, 20, 10).len()).collect();
        assert_eq!(slots.iter().sum::<usize>(), 10);
        assert!(slots.iter().all(|&length| length <= 1));
    }

    #[test]
    fn the_gutter_is_capped_and_its_labels_cut() {
        let format = |value: f64| format!("{value}kg/m²");
        let y = Axis::Linear {
            scale: LinearScale::new(0.0, 10.0).unwrap(),
            format: Some(&format),
        };
        let (plot, rows) = drawn(12, 3, &Axis::Hidden, &y);
        assert_eq!(plot.x, 6);
        assert_eq!(rows, ["10k… ┤      ", "     │      ", "0kg… ┤      "]);
    }

    #[test]
    fn hidden_axes_leave_the_whole_buffer() {
        let (plot, rows) = drawn(4, 2, &Axis::Hidden, &Axis::Hidden);
        assert_eq!(
            plot,
            Area {
                x: 0,
                y: 0,
                width: 4,
                height: 2
            }
        );
        assert_eq!(rows, ["    ", "    "]);
    }

    #[test]
    fn tiny_buffers_draw_what_fits() {
        let (plot, rows) = drawn(3, 1, &linear(0.0, 10.0), &linear(0.0, 10.0));
        assert_eq!((plot.width, plot.height), (2, 0));
        assert_eq!(rows, ["└┬┬"]);

        let (plot, rows) = drawn(1, 3, &linear(0.0, 10.0), &linear(0.0, 100.0));
        assert_eq!((plot.width, plot.height), (0, 1));
        assert_eq!(rows, ["┤", "└", " "]);

        let (plot, rows) = drawn(0, 0, &linear(0.0, 10.0), &linear(0.0, 10.0));
        assert_eq!((plot.width, plot.height), (0, 0));
        assert!(rows.is_empty());
    }

    #[test]
    fn lines_and_labels_take_their_colors() {
        let axes = Axes::layout(20, 8, &linear(0.0, 10.0), &linear(0.0, 100.0));
        let mut buffer = CanvasBuffer::new(20, 8);
        axes.draw(
            &mut buffer,
            AxisStyle {
                line: Some(ResolvedColor::red()),
                label: Some(ResolvedColor::blue()),
            },
        );
        assert_eq!(buffer.get(4, 0).unwrap().1.fg, Some(ResolvedColor::red()));
        assert_eq!(buffer.get(4, 6).unwrap().1.fg, Some(ResolvedColor::red()));
        assert_eq!(buffer.get(0, 0).unwrap().1.fg, Some(ResolvedColor::blue()));
        assert_eq!(buffer.get(5, 7).unwrap().1.fg, Some(ResolvedColor::blue()));

        let mut buffer = CanvasBuffer::new(20, 8);
        axes.draw(&mut buffer, AxisStyle::default());
        assert_eq!(buffer.get(4, 0).unwrap().1.fg, None);
    }
}
