use tuidom::style::{Color, ResolvedColor};
use tuidom_ui::testing::Harness;

/// The concrete color behind a palette entry, for comparing against resolved styles.
pub(crate) fn literal(color: &Color) -> ResolvedColor {
    match color {
        Color::Literal(resolved) => *resolved,
        other => panic!("palette color is not a literal: {other:?}"),
    }
}

/// Press on the start of `needle` and release one cell to its right, as a click that wobbles.
pub(crate) fn wobble(h: &mut Harness, needle: &str) {
    let (x, y) = h
        .find_text(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not on screen"));
    h.drag((x, y), &[(x + 1, y)]);
}
