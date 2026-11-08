//! `tuidom-components` — an opinionated, themed component library on `tuidom-ui`.
//!
//! Components draw their colors from the variables a [`Theme`] declares and their shape from
//! the theme in context, so installing or switching a theme restyles everything below it.
#![warn(missing_docs)]

mod button;
mod checkbox;
mod choice;
mod divider;
mod field;
mod label;
mod panel;
mod radio_group;
mod select;
mod slider;
#[cfg(test)]
mod test_support;
mod text_input;
mod theme;

pub use button::{Button, ButtonProps, ButtonVariant};
pub use checkbox::{Checkbox, CheckboxProps};
pub use divider::{Divider, DividerProps, Orientation};
pub use label::{Label, LabelProps, LabelTone};
pub use panel::{Panel, PanelProps};
pub use radio_group::{RadioGroup, RadioGroupProps};
pub use select::{Select, SelectProps};
pub use slider::{Slider, SliderProps};
pub use text_input::{TextInput, TextInputProps};
pub use theme::{
    Density, FocusMark, Palette, RootBackground, Shape, Surface, Theme, ThemeScope,
    ThemeScopeProps, install_theme, use_shape,
};
