//! `tuidom-components` — an opinionated, themed component library on `tuidom-ui`.
//!
//! Components draw their colors from the variables a [`Theme`] declares and their shape from
//! the theme in context, so installing or switching a theme restyles everything below it.
#![warn(missing_docs)]

mod button;
#[cfg(test)]
mod test_support;
mod theme;

pub use button::{Button, ButtonProps, ButtonVariant};
pub use theme::{
    Density, FocusMark, Palette, RootBackground, Shape, Surface, Theme, ThemeScope,
    ThemeScopeProps, install_theme, use_shape,
};
