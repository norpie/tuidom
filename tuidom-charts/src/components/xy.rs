use tuidom::canvas::CanvasBuffer;
use tuidom::style::{Color, Style};
use tuidom_components::use_palette;
use tuidom_ui::{Children, Derived, Signal, View, component, derived, node_ref, state, view};

use super::{ChartSeries, Colors, TickLabels, canvas_style, chart_color, frame};
use crate::dots::Marker;
use crate::plot::{Domain, Plot, Series, draw_lines, draw_points};

/// Series drawn as lines joining their points, over x and y axes.
///
/// It fills its container unless `style` sizes it, and `style` is merged over its own. A
/// `title` is shown bold above the chart, and with `legend` each named series below it.
/// Series take `--chart-1` to `--chart-5` unless they set a color, and the axes
/// `--muted-foreground`. The chart redraws when its props, its size or the theme change.
#[component]
pub fn LineChart(
    series: Vec<ChartSeries>,
    #[prop(default)] x: Domain,
    #[prop(default)] y: Domain,
    #[prop(default)] marker: Marker,
    #[prop(default)] x_labels: TickLabels,
    #[prop(default)] y_labels: TickLabels,
    #[prop(default)] title: String,
    #[prop(default = true)] legend: bool,
    #[prop(default)] style: Style,
) -> View {
    chart(
        Props {
            series,
            x,
            y,
            marker,
            x_labels,
            y_labels,
            title,
            legend,
            style,
        },
        draw_lines,
    )
}

/// Series drawn as a dot per point, over x and y axes. Points outside a set domain are left
/// out.
///
/// It fills its container unless `style` sizes it, and `style` is merged over its own. A
/// `title` is shown bold above the chart, and with `legend` each named series below it.
/// Series take `--chart-1` to `--chart-5` unless they set a color, and the axes
/// `--muted-foreground`. The chart redraws when its props, its size or the theme change.
#[component]
pub fn ScatterChart(
    series: Vec<ChartSeries>,
    #[prop(default)] x: Domain,
    #[prop(default)] y: Domain,
    #[prop(default)] marker: Marker,
    #[prop(default)] x_labels: TickLabels,
    #[prop(default)] y_labels: TickLabels,
    #[prop(default)] title: String,
    #[prop(default = true)] legend: bool,
    #[prop(default)] style: Style,
) -> View {
    chart(
        Props {
            series,
            x,
            y,
            marker,
            x_labels,
            y_labels,
            title,
            legend,
            style,
        },
        draw_points,
    )
}

#[derive(Clone, Copy)]
struct Props {
    series: Signal<Vec<ChartSeries>>,
    x: Signal<Domain>,
    y: Signal<Domain>,
    marker: Signal<Marker>,
    x_labels: Signal<TickLabels>,
    y_labels: Signal<TickLabels>,
    title: Signal<String>,
    legend: Signal<bool>,
    style: Signal<Style>,
}

fn chart(props: Props, draw: fn(&mut CanvasBuffer, &Plot)) -> View {
    let palette = use_palette();
    let size = state((0u16, 0u16));
    let canvas = node_ref();
    let entries: Derived<Vec<(String, Color)>> = derived(move || {
        if !props.legend.get() {
            return Vec::new();
        }
        props
            .series
            .get()
            .into_iter()
            .enumerate()
            .filter(|(_, series)| !series.name.is_empty())
            .map(|(index, series)| {
                let color = series.color.unwrap_or_else(|| chart_color(index));
                (series.name, color)
            })
            .collect()
    });
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
                        draw_plot(&mut buffer, &colors, props, draw);
                    }
                    buffer
                },
            }
        }
    });
    frame(props.title, props.style, entries, body)
}

fn draw_plot(
    buffer: &mut CanvasBuffer,
    colors: &Colors,
    props: Props,
    draw: fn(&mut CanvasBuffer, &Plot),
) {
    let all = props.series.get();
    let series: Vec<Series> = all
        .iter()
        .enumerate()
        .map(|(index, series)| Series {
            name: &series.name,
            points: &series.points,
            color: colors.resolve(series.color.as_ref().unwrap_or(&chart_color(index))),
        })
        .collect();
    let (x_labels, y_labels) = (props.x_labels.get(), props.y_labels.get());
    let plot = Plot {
        series: &series,
        x: props.x.get(),
        y: props.y.get(),
        x_format: x_labels.format(),
        y_format: y_labels.format(),
        marker: props.marker.get(),
        axes: Some(colors.axes()),
    };
    draw(buffer, &plot);
}
