/// The result of centering content inside a container.
///
/// Terminal cells are discrete, so content cannot always sit exactly in the middle.
/// When the leftover space is odd, the two closest offsets are equally valid and both
/// are reported rather than one being silently chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centered {
    /// The leftover space divides evenly, so one offset is exactly centered.
    Even(i32),
    /// The leftover space is odd, so these two offsets sit one cell either side of center.
    Uneven {
        /// The left/top-biased offset.
        low: i32,
        /// The right/bottom-biased offset.
        high: i32,
    },
}

impl Centered {
    /// The left/top-biased offset, for callers that do not care about the off-by-one.
    pub fn any(self) -> i32 {
        match self {
            Centered::Even(offset) => offset,
            Centered::Uneven { low, .. } => low,
        }
    }
}

/// Center content horizontally inside a container.
///
/// Returns an offset from the container's origin, which is what
/// [`Position::Absolute`](crate::style::Position::Absolute) expects. Content wider than
/// its container yields a negative offset rather than being clamped, matching the signed
/// coordinate space that layout already publishes.
pub fn center_x(container: u16, content: u16) -> Centered {
    center(container, content)
}

/// Center content vertically inside a container.
///
/// See [`center_x`] for the offset and overflow semantics.
pub fn center_y(container: u16, content: u16) -> Centered {
    center(container, content)
}

/// Center content horizontally, taking the left-biased offset when it cannot be exact.
pub fn any_center_x(container: u16, content: u16) -> i32 {
    center_x(container, content).any()
}

/// Center content vertically, taking the top-biased offset when it cannot be exact.
pub fn any_center_y(container: u16, content: u16) -> i32 {
    center_y(container, content).any()
}

fn center(container: u16, content: u16) -> Centered {
    let free = i32::from(container) - i32::from(content);
    if free % 2 == 0 {
        Centered::Even(free / 2)
    } else {
        // Euclidean division floors toward negative infinity, so `low` stays the
        // left/top-biased offset even when the content overflows its container.
        let low = free.div_euclid(2);
        Centered::Uneven { low, high: low + 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_leftover_space_centers_exactly() {
        assert_eq!(center_x(10, 4), Centered::Even(3));
        assert_eq!(center_y(10, 10), Centered::Even(0));
        assert_eq!(center_x(0, 0), Centered::Even(0));
    }

    #[test]
    fn odd_leftover_space_reports_both_offsets() {
        assert_eq!(center_x(10, 5), Centered::Uneven { low: 2, high: 3 });
        assert_eq!(center_y(3, 2), Centered::Uneven { low: 0, high: 1 });
    }

    #[test]
    fn oversized_content_yields_negative_offsets() {
        assert_eq!(center_x(4, 10), Centered::Even(-3));
        assert_eq!(center_x(4, 9), Centered::Uneven { low: -3, high: -2 });
    }

    #[test]
    fn any_center_takes_the_low_biased_offset() {
        assert_eq!(any_center_x(10, 5), 2);
        assert_eq!(any_center_y(10, 4), 3);
        assert_eq!(any_center_x(4, 9), -3);
    }
}
