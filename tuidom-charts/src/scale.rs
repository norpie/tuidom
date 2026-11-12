//! Linear scales: the range of values an axis covers, the ticks along it, and where a value
//! falls on it.

/// A linear mapping from a domain of values onto an axis.
///
/// The domain always has a finite, non-zero span, so every finite value has a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearScale {
    min: f64,
    max: f64,
}

/// Tick values inside a scale's domain, and the step between them.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticks {
    /// The distance between consecutive ticks: 1, 2 or 5 times a power of ten.
    pub step: f64,
    /// The ticks, ascending.
    pub values: Vec<f64>,
}

impl LinearScale {
    /// A scale over `min` to `max`, in either order.
    ///
    /// Equal bounds are padded apart: by 1 either side of zero, otherwise by a tenth of the
    /// value. `None` when a bound is not finite or the span overflows.
    pub fn new(min: f64, max: f64) -> Option<Self> {
        if !min.is_finite() || !max.is_finite() {
            return None;
        }
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        let (min, max) = if min == max {
            let pad = if min == 0.0 { 1.0 } else { min.abs() / 10.0 };
            (min - pad, max + pad)
        } else {
            (min, max)
        };
        ((max - min).is_finite() && max > min).then_some(Self { min, max })
    }

    /// A scale over the smallest and largest finite values, or `None` when there are none.
    pub fn from_values(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        let (min, max) =
            values
                .into_iter()
                .filter(|value| value.is_finite())
                .fold(None, |range, value| match range {
                    None => Some((value, value)),
                    Some((min, max)) => Some((f64::min(min, value), f64::max(max, value))),
                })?;
        Self::new(min, max)
    }

    /// The smallest and largest value of the domain.
    pub fn domain(&self) -> (f64, f64) {
        (self.min, self.max)
    }

    /// The scale widened to cover `value`, or unchanged when it already does or `value` is not
    /// finite.
    pub fn include(self, value: f64) -> Self {
        if !value.is_finite() {
            return self;
        }
        Self::new(self.min.min(value), self.max.max(value)).unwrap_or(self)
    }

    /// About `count` ticks inside the domain, a round step apart.
    ///
    /// Where no round step can be counted in, such as a span too small against its values for
    /// `f64` to hold the ticks apart, the ticks are the domain's two ends.
    pub fn ticks(&self, count: usize) -> Ticks {
        let counted = Increment::new(self.min, self.max, count).and_then(|increment| {
            let first = increment.index(self.min, f64::ceil)?;
            let last = increment.index(self.max, f64::floor)?;
            Some(Ticks {
                step: increment.step(),
                values: (first..=last).map(|index| increment.value(index)).collect(),
            })
        });
        counted.unwrap_or_else(|| Ticks {
            step: self.max - self.min,
            values: vec![self.min, self.max],
        })
    }

    /// The scale widened outward to the nearest ticks `ticks(count)` would give, so its ends
    /// land on round values.
    pub fn nice(self, count: usize) -> Self {
        let mut scale = self;
        let mut previous = None;
        // Widening can change the step, which can move the ends again; it settles in a few
        // rounds.
        for _ in 0..10 {
            let Some(increment) = Increment::new(scale.min, scale.max, count) else {
                break;
            };
            if previous == Some(increment) {
                break;
            }
            previous = Some(increment);
            let (Some(first), Some(last)) = (
                increment.index(scale.min, f64::floor),
                increment.index(scale.max, f64::ceil),
            ) else {
                break;
            };
            match Self::new(increment.value(first), increment.value(last)) {
                Some(widened) => scale = widened,
                None => break,
            }
        }
        scale
    }

    /// Where `value` falls, from 0.0 at the domain's minimum to 1.0 at its maximum. Values
    /// outside the domain fall outside that range. `None` when `value` is not finite.
    pub fn fraction(&self, value: f64) -> Option<f64> {
        value
            .is_finite()
            .then(|| (value - self.min) / (self.max - self.min))
    }

    /// The nearest of `length` evenly spaced positions to `value`, the first at the domain's
    /// minimum and the last at its maximum. `None` when `value` is not finite, lies outside the
    /// domain, or `length` is zero.
    pub fn index(&self, value: f64, length: usize) -> Option<usize> {
        let fraction = self.fraction(value)?;
        if length == 0 || !(0.0..=1.0).contains(&fraction) {
            return None;
        }
        Some((fraction * (length - 1) as f64).round() as usize)
    }
}

/// A tick step of 1, 2 or 5 times a power of ten, kept as an integer factor so tick values are
/// computed without accumulating error: a negative power divides by its reciprocal, since
/// `3.0 * 0.1` is not `0.3` but `3.0 / 10.0` is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Increment {
    Multiple(f64),
    Fraction(f64),
}

impl Increment {
    fn new(min: f64, max: f64, count: usize) -> Option<Self> {
        let rough = (max - min) / count.max(1) as f64;
        let power = rough.log10().floor();
        let unit = 10f64.powf(power);
        if !rough.is_finite() || rough <= 0.0 || !unit.is_normal() {
            return None;
        }
        let error = rough / unit;
        let factor = if error >= 50f64.sqrt() {
            10.0
        } else if error >= 10f64.sqrt() {
            5.0
        } else if error >= 2f64.sqrt() {
            2.0
        } else {
            1.0
        };
        if power >= 0.0 {
            Some(Self::Multiple(factor * unit))
        } else {
            let reciprocal = 10f64.powf(-power) / factor;
            reciprocal.is_finite().then_some(Self::Fraction(reciprocal))
        }
    }

    fn step(self) -> f64 {
        match self {
            Self::Multiple(step) => step,
            Self::Fraction(reciprocal) => 1.0 / reciprocal,
        }
    }

    /// How many steps from zero `value` is, rounded by `round`. `None` past the integers an
    /// `f64` holds exactly, where neighbouring ticks would be the same value.
    fn index(self, value: f64, round: fn(f64) -> f64) -> Option<i64> {
        const EXACT: f64 = 9_007_199_254_740_992.0;
        let index = round(match self {
            Self::Multiple(step) => value / step,
            Self::Fraction(reciprocal) => value * reciprocal,
        });
        (index.abs() <= EXACT).then_some(index as i64)
    }

    fn value(self, index: i64) -> f64 {
        match self {
            Self::Multiple(step) => index as f64 * step,
            Self::Fraction(reciprocal) => index as f64 / reciprocal,
        }
    }
}

const SUFFIXES: [(f64, &str); 4] = [(1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k")];

/// A tick label for `value` on an axis whose ticks are `step` apart.
///
/// It shows as many decimals as the step needs, and from a thousand up a k, M, G or T suffix
/// where that costs at most one decimal: `1500` a step of 500 apart is `1.5k`, but a step of 10
/// apart is `1510`. An invalid step shows no decimals, and a value that is not finite is empty.
pub fn compact(value: f64, step: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let (value, step, suffix) = SUFFIXES
        .iter()
        .find(|(divisor, _)| value.abs() >= *divisor && step >= divisor / 10.0)
        .map_or((value, step, ""), |(divisor, suffix)| {
            (value / divisor, step / divisor, *suffix)
        });
    let decimals = decimals(step);
    let mut label = format!("{value:.decimals$}");
    if label.starts_with('-') && label.bytes().all(|byte| matches!(byte, b'-' | b'0' | b'.')) {
        label.remove(0);
    }
    label.push_str(suffix);
    label
}

/// The fewest decimals that write every multiple of `step` exactly.
fn decimals(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    (0..=15)
        .find(|&places| {
            let scaled = step * 10f64.powi(places as i32);
            (scaled - scaled.round()).abs() <= scaled * 1e-9
        })
        .unwrap_or(15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(min: f64, max: f64) -> LinearScale {
        LinearScale::new(min, max).unwrap()
    }

    #[test]
    fn bounds_are_ordered_and_must_be_finite() {
        assert_eq!(scale(10.0, -5.0).domain(), (-5.0, 10.0));
        assert_eq!(LinearScale::new(f64::NAN, 1.0), None);
        assert_eq!(LinearScale::new(0.0, f64::INFINITY), None);
        assert_eq!(LinearScale::new(-f64::MAX, f64::MAX), None);
    }

    #[test]
    fn a_flat_domain_is_padded_apart() {
        assert_eq!(scale(0.0, 0.0).domain(), (-1.0, 1.0));
        assert_eq!(scale(50.0, 50.0).domain(), (45.0, 55.0));
        assert_eq!(scale(-50.0, -50.0).domain(), (-55.0, -45.0));
    }

    #[test]
    fn the_domain_from_values_skips_values_that_are_not_finite() {
        let values = [3.0, f64::NAN, -2.0, f64::INFINITY, 7.5];
        assert_eq!(
            LinearScale::from_values(values).unwrap().domain(),
            (-2.0, 7.5)
        );
        assert_eq!(LinearScale::from_values([f64::NAN]), None);
        assert_eq!(LinearScale::from_values([]), None);
        assert_eq!(
            LinearScale::from_values([4.0]).unwrap().domain(),
            (3.6, 4.4)
        );
    }

    #[test]
    fn include_widens_only_when_needed() {
        assert_eq!(scale(3.0, 9.0).include(0.0).domain(), (0.0, 9.0));
        assert_eq!(scale(-9.0, -3.0).include(0.0).domain(), (-9.0, 0.0));
        assert_eq!(scale(3.0, 9.0).include(5.0).domain(), (3.0, 9.0));
        assert_eq!(scale(3.0, 9.0).include(f64::NAN).domain(), (3.0, 9.0));
    }

    #[test]
    fn ticks_fall_on_round_steps() {
        let ticks = scale(0.0, 1.0).ticks(5);
        assert_eq!(ticks.step, 0.2);
        assert_eq!(ticks.values, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);

        let ticks = scale(0.0, 100.0).ticks(5);
        assert_eq!(ticks.step, 20.0);
        assert_eq!(ticks.values, [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);

        let ticks = scale(3.0, 97.0).ticks(4);
        assert_eq!(ticks.step, 20.0);
        assert_eq!(ticks.values, [20.0, 40.0, 60.0, 80.0]);

        let ticks = scale(-1.0, 1.0).ticks(4);
        assert_eq!(ticks.step, 0.5);
        assert_eq!(ticks.values, [-1.0, -0.5, 0.0, 0.5, 1.0]);

        let ticks = scale(0.0, 0.3).ticks(3);
        assert_eq!(ticks.step, 0.1);
        assert_eq!(ticks.values, [0.0, 0.1, 0.2, 0.3]);

        let ticks = scale(0.0, 1e6).ticks(2);
        assert_eq!(ticks.step, 500_000.0);
        assert_eq!(ticks.values, [0.0, 500_000.0, 1_000_000.0]);
    }

    #[test]
    fn ticks_too_fine_for_f64_are_the_domains_ends() {
        let ticks = scale(1e20, 1e20 + 65536.0).ticks(1000);
        assert_eq!(ticks.values, [1e20, 1e20 + 65536.0]);
    }

    #[test]
    fn a_count_of_zero_asks_for_one_tick_step() {
        assert_eq!(scale(0.0, 10.0).ticks(0).step, 10.0);
    }

    #[test]
    fn nice_widens_the_ends_to_ticks() {
        assert_eq!(scale(3.0, 97.0).nice(5).domain(), (0.0, 100.0));
        assert_eq!(scale(-0.13, 0.87).nice(5).domain(), (-0.2, 1.0));
        assert_eq!(scale(0.0, 100.0).nice(5).domain(), (0.0, 100.0));
        assert_eq!(scale(12.0, 13.0).nice(4).domain(), (12.0, 13.0));
    }

    #[test]
    fn values_map_to_fractions_and_indices() {
        let scale = scale(-10.0, 10.0);
        assert_eq!(scale.fraction(-10.0), Some(0.0));
        assert_eq!(scale.fraction(0.0), Some(0.5));
        assert_eq!(scale.fraction(20.0), Some(1.5));
        assert_eq!(scale.fraction(f64::NAN), None);

        assert_eq!(scale.index(-10.0, 5), Some(0));
        assert_eq!(scale.index(0.0, 5), Some(2));
        assert_eq!(scale.index(10.0, 5), Some(4));
        assert_eq!(scale.index(4.0, 5), Some(3));
        assert_eq!(scale.index(10.5, 5), None);
        assert_eq!(scale.index(f64::INFINITY, 5), None);
        assert_eq!(scale.index(0.0, 0), None);
        assert_eq!(scale.index(0.0, 1), Some(0));
    }

    #[test]
    fn labels_show_the_decimals_the_step_needs() {
        assert_eq!(compact(0.0, 0.2), "0.0");
        assert_eq!(compact(0.4, 0.2), "0.4");
        assert_eq!(compact(1.25, 0.25), "1.25");
        assert_eq!(compact(20.0, 10.0), "20");
        assert_eq!(compact(0.003, 0.001), "0.003");
    }

    #[test]
    fn large_labels_take_a_suffix_when_it_costs_at_most_one_decimal() {
        assert_eq!(compact(1500.0, 500.0), "1.5k");
        assert_eq!(compact(2000.0, 1000.0), "2k");
        assert_eq!(compact(1510.0, 10.0), "1510");
        assert_eq!(compact(2_000_000.0, 1_000_000.0), "2M");
        assert_eq!(compact(2_500_000_000.0, 500_000_000.0), "2.5G");
        assert_eq!(compact(3e12, 1e12), "3T");
        assert_eq!(compact(-1500.0, 500.0), "-1.5k");
        assert_eq!(compact(500.0, 500.0), "500");
    }

    #[test]
    fn labels_never_show_negative_zero() {
        assert_eq!(compact(-0.0, 1.0), "0");
        assert_eq!(compact(-0.0001, 0.1), "0.0");
    }

    #[test]
    fn invalid_labels() {
        assert_eq!(compact(f64::NAN, 1.0), "");
        assert_eq!(compact(2.4, 0.0), "2");
        assert_eq!(compact(2.4, f64::NAN), "2");
    }
}
