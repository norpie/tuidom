use tuidom::style::{Color, ResolvedColor};

/// The concrete color behind a palette entry, for comparing against resolved styles.
pub(crate) fn literal(color: &Color) -> ResolvedColor {
    match color {
        Color::Literal(resolved) => *resolved,
        other => panic!("palette color is not a literal: {other:?}"),
    }
}
