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
mod list;
mod menu;
mod modal;
mod panel;
mod popup_list;
mod progress_bar;
mod radio_group;
mod roving;
mod scrollable;
mod select;
mod slider;
mod spinner;
mod split;
mod status_bar;
mod table;
mod tabs;
#[cfg(test)]
mod test_support;
mod text_input;
mod theme;
mod toast;
mod tone;
mod tooltip;
mod tree;

pub use button::{Button, ButtonProps, ButtonVariant};
pub use checkbox::{Checkbox, CheckboxProps};
pub use divider::{Divider, DividerProps, Orientation};
pub use label::{Label, LabelProps, LabelTone};
pub use list::{List, ListProps};
pub use menu::{Menu, MenuItem, MenuProps};
pub use modal::{Modal, ModalProps};
pub use panel::{Panel, PanelProps};
pub use progress_bar::{ProgressBar, ProgressBarProps};
pub use radio_group::{RadioGroup, RadioGroupProps};
pub use scrollable::{ScrollAxis, Scrollable, ScrollableProps};
pub use select::{Select, SelectProps};
pub use slider::{Slider, SliderProps};
pub use spinner::{Spinner, SpinnerProps};
pub use split::{Split, SplitProps};
pub use status_bar::{KeyHint, KeyHintProps, StatusBar, StatusBarProps};
pub use table::{Column, Table, TableProps};
pub use tabs::{Tabs, TabsProps};
pub use text_input::{TextInput, TextInputProps};
pub use theme::{
    Density, FocusMark, Palette, RootBackground, Shape, Surface, Theme, ThemeScope,
    ThemeScopeProps, install_theme, use_palette, use_shape,
};
pub use toast::{Toast, Toaster, ToasterHandle, ToasterProps, use_toaster};
pub use tone::Tone;
pub use tooltip::{Side, Tooltip, TooltipProps};
pub use tree::{Tree, TreeNode, TreeProps};
