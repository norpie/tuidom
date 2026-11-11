#[cfg(test)]
mod tests;

use std::time::Duration;

use tuidom::NodeId;
use tuidom::animation::{AnimatableProperty, KeyframeAnimation};
use tuidom::style::{
    AlignItems, AnchorAxis, AnchorEdge, Border, Color, EdgeInsets, FlexDirection, Position, Sides,
    Style,
};
use tuidom_ui::{
    Children, Signal, View, component, derived, fragment, node_ref, on_mount, provide_context,
    state, use_context, use_document, view,
};

use crate::theme::{Density, Shape, Surface, Theme, use_shape, use_theme};
use crate::tone::Tone;

/// How long a toast stays at full opacity.
const HOLD: Duration = Duration::from_secs(4);
/// How long it then takes to fade out.
const FADE: Duration = Duration::from_millis(500);

/// A notification for [`ToasterHandle::show`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Toast {
    /// The first line, in bold.
    pub title: String,
    /// A second line in `--muted-foreground`, left out when empty.
    pub description: String,
    /// Colors the toast's edge, or its title where the theme draws no edge.
    pub tone: Tone,
}

/// The handle [`use_toaster`] returns, for showing toasts from anywhere inside a
/// [`Toaster`](fn@Toaster).
///
/// `Copy`, so a listener captures it from the component body that read it.
#[derive(Clone, Copy)]
pub struct ToasterHandle {
    shown: Signal<Vec<(u64, Toast)>>,
    next: Signal<u64>,
}

impl ToasterHandle {
    /// Show `toast` below any already showing. It holds for four seconds, fades out, and is
    /// removed; a click dismisses it sooner.
    pub fn show(&self, toast: Toast) {
        let id = self.next.get();
        self.next.set(id + 1);
        self.shown.update(|shown| shown.push((id, toast)));
    }

    fn dismiss(&self, id: u64) {
        self.shown
            .update(|shown| shown.retain(|(shown, _)| *shown != id));
    }
}

/// The handle of the nearest [`Toaster`](fn@Toaster) above, or `None` when there is none.
///
/// Read it in a component body. Listeners run with no scope, so a handler that shows a toast
/// captures the handle from the body.
pub fn use_toaster() -> Option<ToasterHandle> {
    use_context::<ToasterHandle>()
}

/// Shows the toasts that components inside `children` send through [`use_toaster`].
///
/// Wrap the app in it once. The toasts stack in the bottom-right corner of the screen, newest at
/// the bottom, over everything else, in `--popover`. Only the toasts themselves take presses, so
/// the app around them stays usable. `Toaster` adds no box of its own around `children`.
///
/// `style` is merged over the stack's own style.
#[component]
pub fn Toaster(#[prop(default)] style: Style, children: Children) -> View {
    let toaster = ToasterHandle {
        shown: state(Vec::new()),
        next: state(0),
    };
    provide_context(toaster);
    let theme = use_theme();
    let root = use_document().map(|doc| doc.root());
    let stack = derived(move || {
        let mut stack = stack_style(root, theme);
        stack.merge(&style.get());
        stack
    });

    fragment(vec![
        View::from(children),
        view! {
            portal {
                box {
                    style: stack.get(),
                    each _, (id, toast) in toaster.shown.get() key = id {
                        ToastCard { id: id, toast: toast.clone() }
                    },
                }
            }
        },
    ])
}

/// The stack's frame, against the screen's bottom-right corner. The palette is declared on it
/// because the portal mounts it outside any `ThemeScope` the toaster sits in.
fn stack_style(root: Option<NodeId>, theme: Option<Signal<Theme>>) -> Style {
    let mut style = Style::new();
    if let Some(theme) = theme {
        theme.get().palette.declare_on(&mut style);
    }
    style.flex_direction(FlexDirection::Column);
    style.align_items(AlignItems::FlexEnd);
    if let Some(root) = root {
        style.position(Position::Anchored {
            anchor: root,
            x: AnchorAxis::new(AnchorEdge::End, AnchorEdge::End),
            y: AnchorAxis::new(AnchorEdge::End, AnchorEdge::End),
        });
    }
    style
}

#[component]
fn ToastCard(id: u64, toast: Toast) -> View {
    let shape = use_shape();
    let toaster = use_toaster();
    let done = move || {
        if let Some(toaster) = toaster {
            toaster.dismiss(id.get());
        }
    };
    let card = node_ref();
    on_mount(move || {
        card.animate(fade_out());
    });
    let frame = derived(move || card_style(&shape.get(), toast.get().tone));
    let title = derived(move || title_style(&shape.get(), toast.get().tone));

    view! {
        box {
            ref: card,
            style: frame.get(),
            on_click: move || done(),
            on_animation_end: move || done(),
            text(toast.get().title) { style: title.get() },
            if !toast.get().description.is_empty() {
                text(toast.get().description) { style: description_style() }
            },
        }
    }
}

/// Full opacity for [`HOLD`], then down to nothing over [`FADE`]. The end event removes the
/// toast while the last, invisible frame is still held.
fn fade_out() -> KeyframeAnimation {
    let total = HOLD + FADE;
    let hold_until = HOLD.as_secs_f64() / total.as_secs_f64() * 100.0;
    KeyframeAnimation::new(total)
        .keyframe(0.0, [AnimatableProperty::Opacity(1.0)])
        .keyframe(hold_until, [AnimatableProperty::Opacity(1.0)])
        .keyframe(100.0, [AnimatableProperty::Opacity(0.0)])
}

fn card_style(shape: &Shape, tone: Tone) -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.background(Color::var("--popover"));
    let padding = match shape.density {
        Density::Compact => 1,
        Density::Comfortable => 2,
    };
    match shape.container {
        Surface::Bordered => {
            style.border(Border::new(shape.border));
            style.border_color(tone.color());
            style.padding(EdgeInsets::symmetric(padding, 0));
        }
        Surface::HalfBlock => {
            style.half_block_edges(Sides::new(true, false, true, false));
            style.half_block_inner_color(tone.color());
            style.padding(EdgeInsets::symmetric(padding, 1));
        }
        Surface::Flat => style.padding(EdgeInsets::symmetric(padding, 0)),
    }
    style
}

fn title_style(shape: &Shape, tone: Tone) -> Style {
    let mut style = Style::new();
    style.bold(true);
    let color = if shape.container == Surface::Flat {
        tone.color()
    } else {
        Color::var("--popover-foreground")
    };
    style.color(color);
    style
}

fn description_style() -> Style {
    let mut style = Style::new();
    style.color(Color::var("--muted-foreground"));
    style
}
