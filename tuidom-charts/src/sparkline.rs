//! Sparklines: a column of eighth blocks per value, with no axes.

use tuidom::canvas::CanvasBuffer;
use tuidom::style::ResolvedColor;

use crate::fill::fill_up;
use crate::plot::Domain;
use crate::scale::LinearScale;

/// Draw `values` into `buffer` as a sparkline, replacing what it held: a column per value from
/// the left, each filled up from the bottom row to an eighth of a cell.
///
/// When there are more values than columns, the last ones are shown. Values at or below the
/// bottom of the domain draw nothing, values above its top fill their column, and a value that
/// is not finite leaves a gap.
pub fn draw_sparkline(
    buffer: &mut CanvasBuffer,
    values: &[f64],
    domain: Domain,
    color: ResolvedColor,
) {
    buffer.clear();
    let Some(bottom) = buffer.height().checked_sub(1) else {
        return;
    };
    let shown = &values[values.len().saturating_sub(usize::from(buffer.width()))..];
    let scale = match domain {
        Domain::Fixed(scale) => scale,
        Domain::Data => shown
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .reduce(f64::max)
            .filter(|&max| max > 0.0)
            .and_then(|max| LinearScale::new(0.0, max))
            .unwrap_or(LinearScale::UNIT),
    };
    let rows = f64::from(buffer.height());
    for (column, &value) in (0u16..).zip(shown) {
        if let Some(fraction) = scale.fraction(value) {
            fill_up(buffer, column, bottom, 1, fraction.min(1.0) * rows, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(width: u16, height: u16, values: &[f64], domain: Domain) -> Vec<String> {
        let mut buffer = CanvasBuffer::new(width, height);
        draw_sparkline(&mut buffer, values, domain, ResolvedColor::red());
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| match buffer.get(x, y) {
                        Some(("", _)) | None => " ",
                        Some((symbol, _)) => symbol,
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn values_fill_columns_to_an_eighth() {
        let values: Vec<f64> = (0..9).map(f64::from).collect();
        assert_eq!(drawn(9, 1, &values, Domain::Data), [" ▁▂▃▄▅▆▇█"]);
    }

    #[test]
    fn columns_span_several_rows() {
        assert_eq!(
            drawn(4, 2, &[8.0, 4.0, 12.0, 16.0], Domain::Data),
            ["  ▄█", "█▄██"]
        );
    }

    #[test]
    fn the_last_values_are_shown_and_set_the_domain() {
        assert_eq!(drawn(3, 1, &[100.0, 4.0, 8.0, 16.0], Domain::Data), ["▂▄█"]);
        assert_eq!(drawn(3, 1, &[8.0], Domain::Data), ["█  "]);
    }

    #[test]
    fn gaps_and_values_out_of_range() {
        let domain = Domain::Fixed(LinearScale::new(0.0, 8.0).unwrap());
        let values = [f64::NAN, -3.0, 0.0, 12.0, f64::INFINITY];
        assert_eq!(drawn(5, 1, &values, domain), ["   █ "]);
        assert_eq!(drawn(2, 1, &[-1.0, 0.0], Domain::Data), ["  "]);
        assert_eq!(drawn(2, 0, &[1.0], Domain::Data), Vec::<String>::new());
    }
}
