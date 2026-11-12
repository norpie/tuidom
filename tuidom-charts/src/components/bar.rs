use tuidom::canvas::CanvasBuffer;
use tuidom::style::{Color, Style};
use tuidom_components::use_palette;
use tuidom_ui::{Children, Derived, View, component, derived, node_ref, state, view};

use super::{ChartBar, Colors, TickLabels, canvas_style, chart_color, frame};
use crate::bars::{Bar, BarPlot, Orientation, draw_bars};
use crate::plot::Domain;

/// Labelled values drawn as bars from zero, along a value axis.
///
/// It fills its container unless `style` sizes it, and `style` is merged over its own. A
/// `title` is shown bold above the chart. Bars take `--chart-1` unless they set a color, and
/// the axes `--muted-foreground`. The chart redraws when its props, its size or the theme
/// change.
#[component]
pub fn BarChart(
    bars: Vec<ChartBar>,
    #[prop(default)] orientation: Orientation,
    #[prop(default)] domain: Domain,
    #[prop(default)] labels: TickLabels,
    #[prop(default)] title: String,
    #[prop(default)] style: Style,
) -> View {
    let palette = use_palette();
    let size = state((0u16, 0u16));
    let canvas = node_ref();
    let body = Children::new(move || {
        view! {
            canvas {
                ref: canvas,
                style: canvas_style(),
                on_size_change: move |ev| size.set((ev.width, ev.height)),
                buffer: {
                    palette.get();
                    let (width, height) = size.get();
                    let mut buffer = CanvasBuffer::new(width, height);
                    if let Some(colors) = Colors::at(canvas) {
                        let all = bars.get();
                        let drawn: Vec<Bar> = all
                            .iter()
                            .map(|bar| Bar {
                                label: &bar.label,
                                value: bar.value,
                                color: colors.resolve(bar.color.as_ref().unwrap_or(&chart_color(0))),
                            })
                            .collect();
                        let labels = labels.get();
                        let plot = BarPlot {
                            bars: &drawn,
                            orientation: orientation.get(),
                            domain: domain.get(),
                            format: labels.format(),
                            axes: Some(colors.axes()),
                        };
                        draw_bars(&mut buffer, &plot);
                    }
                    buffer
                },
            }
        }
    });
    let no_legend: Derived<Vec<(String, Color)>> = derived(Vec::new);
    frame(title, style, no_legend, body)
}
