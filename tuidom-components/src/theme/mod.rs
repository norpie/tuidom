//! Themes: a palette written as color variables, and a shape read from context.
//!
//! Components never name a concrete color. They reference the palette's variables, so a theme
//! reaches them through the engine's variable scoping, and they read the [`Shape`] through
//! [`use_shape`]. A switch therefore restyles in place without remounting anything.

mod palette;
mod shape;
#[cfg(test)]
mod tests;

use tuidom::Document;
use tuidom::style::{Color, Style};
use tuidom_ui::{
    Children, Derived, Signal, View, component, derived, effect, provide_context, state,
    use_context, use_document, view,
};

pub use palette::Palette;
pub use shape::{Density, FocusMark, Shape, Surface};

/// A palette and a shape, plus whether the root paints the palette's background.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Theme {
    /// The colors, declared as variables.
    pub palette: Palette,
    /// Everything about components' look that is not a color.
    pub shape: Shape,
    /// Whether [`install_theme`] paints the root.
    pub root: RootBackground,
}

impl Theme {
    /// The dark palette with the default shape.
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            shape: Shape::default(),
            root: RootBackground::Paint,
        }
    }

    /// The light palette with the default shape.
    pub fn light() -> Self {
        Self {
            palette: Palette::light(),
            shape: Shape::default(),
            root: RootBackground::Paint,
        }
    }
}

/// Whether the document root paints the palette's background.
///
/// Either way the document assumes the palette's background is behind it, so derived colors and
/// blending work out against it. With [`Transparent`](Self::Transparent) that is only true when
/// the terminal's own background matches the palette, which nothing can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootBackground {
    /// The root is filled with `--background`.
    #[default]
    Paint,
    /// The root is left unpainted, so the terminal's background shows.
    Transparent,
}

#[derive(Clone, Copy)]
struct ThemeContext(Signal<Theme>);

/// Theme the document the current component is mounted in, and every component below it.
///
/// Call it in the `App` body. It declares the palette as document variables, points the
/// default foreground at `--foreground`, sets the terminal background assumption and the root's
/// background, and provides the theme to descendants. The returned signal is the switch: setting
/// it rewrites all of that in place.
///
/// Only the root's `background` is touched, so the rest of a root style the app sets is kept.
pub fn install_theme(theme: Theme) -> Signal<Theme> {
    let theme = state(theme);
    provide_context(ThemeContext(theme));
    match use_document() {
        Some(doc) => effect(move || apply_to_document(&doc, &theme.get())),
        None => tracing::warn!("install_theme called outside a mount; the document is not themed"),
    }
    theme
}

fn apply_to_document(doc: &Document, theme: &Theme) {
    theme.palette.declare_on_document(doc);
    doc.set_terminal_background(theme.palette.background.clone());
    doc.set_default_color(Color::var("--foreground"));
    let root = theme.root;
    let painted = doc.update_style(doc.root(), |style| match root {
        RootBackground::Paint => style.background(Color::var("--background")),
        RootBackground::Transparent => style.unset_background(),
    });
    if let Err(err) = painted {
        tracing::warn!("painting the themed root failed: {err}");
    }
}

/// The nearest theme's shape, or the default shape when no theme is installed above.
///
/// Read it in a component body. Listeners run with no scope, so a handler that needs it gets it
/// from the body.
pub fn use_shape() -> Derived<Shape> {
    match use_context::<ThemeContext>() {
        Some(ThemeContext(theme)) => derived(move || theme.get().shape),
        None => derived(Shape::default),
    }
}

/// Re-theme a subtree.
///
/// Declares `theme`'s palette on its own box, shadowing the outer one, and provides `theme` to
/// the components inside. Unstyled text inside takes the new `--foreground` through the
/// document's default foreground. The scope paints nothing, though, so that text sits on
/// whatever surface is behind the box: a scope whose palette differs in lightness usually wants
/// `style` to set `background: Color::var("--background")`. The theme's `root` setting does not
/// apply here.
#[component]
pub fn ThemeScope(theme: Theme, #[prop(default)] style: Style, children: Children) -> View {
    provide_context(ThemeContext(theme));
    let scoped = derived(move || {
        let mut scoped = Style::new();
        theme.get().palette.declare_on(&mut scoped);
        scoped.merge(&style.get());
        scoped
    });
    view! {
        box {
            style: scoped.get(),
            { children.clone() },
        }
    }
}
