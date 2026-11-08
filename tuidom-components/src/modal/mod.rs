#[cfg(test)]
mod tests;

use tuidom::event::KeyCode;
use tuidom::style::{AlignItems, Color, JustifyContent, Length, Position, Style};
use tuidom_ui::{Children, Signal, View, component, derived, view};

use crate::panel::{Panel, PanelProps};
use crate::theme::{Theme, use_theme};

/// A [`Panel`] centered over everything else while `open` is true, holding `title` and
/// `children`.
///
/// Behind it the screen is dimmed with `--background`, and everything there is inert: focus is
/// trapped in the modal, starting on its first focusable node, and goes back to where it was when
/// the modal closes. While `dismissible` is true, Escape and a press outside the panel set `open`
/// to false. Otherwise closing is up to the content, such as a button that sets `open` itself.
///
/// `style` is merged over the panel's own style.
#[component]
pub fn Modal(
    open: Signal<bool>,
    #[prop(default)] title: String,
    #[prop(default = true)] dismissible: bool,
    #[prop(default)] style: Style,
    children: Children,
) -> View {
    let theme = use_theme();
    let backdrop = derived(move || backdrop(theme));
    let dismiss = move || {
        if dismissible.get() {
            open.set(false);
        }
    };

    view! {
        if open.get() {
            portal {
                box {
                    style: backdrop.get(),
                    box {
                        trap_focus: true,
                        style: card(),
                        on_press_outside: move |_| dismiss(),
                        on_key_press: move |ev| {
                            if ev.code == KeyCode::Esc {
                                ev.prevent_default();
                                ev.stop_propagation();
                                dismiss();
                            }
                        },
                        Panel { title: title.get(), style: style.get(), { children } },
                    },
                }
            }
        }
    }
}

/// The dimmed full-screen layer the panel is centered on. The palette is declared on it because
/// the portal mounts it outside any `ThemeScope` the modal sits in.
fn backdrop(theme: Option<Signal<Theme>>) -> Style {
    let mut style = Style::new();
    if let Some(theme) = theme {
        theme.get().palette.declare_on(&mut style);
    }
    style.position(Position::Absolute { x: 0, y: 0 });
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style.background(Color::var("--background").with_alpha(0.6));
    style.justify_content(JustifyContent::Center);
    style.align_items(AlignItems::Center);
    style
}

fn card() -> Style {
    let mut style = Style::new();
    style.stacking_context(true);
    style
}
