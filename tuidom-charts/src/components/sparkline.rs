use tuidom::canvas::CanvasBuffer;
use tuidom::style::{Color, Length, Style};
use tuidom_components::use_palette;
use tuidom_ui::{View, component, derived, node_ref, state, view};

use super::Colors;
use crate::plot::Domain;
use crate::sparkline::draw_sparkline;

/// Values drawn as a column of blocks each, with no axes, the newest at the right once they
/// fill its width.
///
/// It fills its container's width and one row unless `style` sizes it, and `style` is merged
/// over its own. It is drawn in `color`, `--chart-1` unless set, and redraws when its props,
/// its size or the theme change.
#[component]
pub fn Sparkline(
    values: Vec<f64>,
    #[prop(default)] domain: Domain,
    #[prop(default = Color::var("--chart-1"))] color: Color,
    #[prop(default)] style: Style,
) -> View {
    let palette = use_palette();
    let size = state((0u16, 0u16));
    let canvas = node_ref();
    let sized = derived(move || {
        let mut sized = Style::new();
        sized.width(Length::Percent(100.0));
        sized.height(Length::Cells(1));
        sized.merge(&style.get());
        sized
    });
    view! {
        canvas {
            ref: canvas,
            style: sized.get(),
            on_size_change: move |ev| size.set((ev.width, ev.height)),
            buffer: {
                palette.get();
                let (width, height) = size.get();
                let mut buffer = CanvasBuffer::new(width, height);
                if let Some(colors) = Colors::at(canvas) {
                    let color = colors.resolve(&color.get());
                    draw_sparkline(&mut buffer, &values.get(), domain.get(), color);
                }
                buffer
            },
        }
    }
}
