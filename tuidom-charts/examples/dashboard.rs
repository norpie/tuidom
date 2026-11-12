//! Charts over live data: two random walks drawn as lines, their latest values as bars, and the
//! history of one as a sparkline. A new value arrives four times a second.
//!
//! t — switch between the dark and light themes
//! q — quit

use std::time::Duration;

use tuidom::event::KeyCode;
use tuidom::style::{Color, FlexDirection, FlexGap, Length, Style};
use tuidom_charts::{
    BarChart, BarChartProps, ChartBar, ChartSeries, Domain, LineChart, LineChartProps, LinearScale,
    Orientation, Sparkline, SparklineProps, TickLabels,
};
use tuidom_components::{Palette, Theme, install_theme};
use tuidom_ui::{App, Signal, View, batch, component, spawn, state, use_document, view};

/// How many samples each series keeps.
const HISTORY: usize = 120;

/// A walk between 0 and 100, moving at most 4 a step, from a xorshift generator.
#[derive(Clone, Copy)]
struct Walk {
    seed: u64,
    value: f64,
}

impl Walk {
    fn new(seed: u64, value: f64) -> Self {
        Self { seed, value }
    }

    fn step(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let unit = (self.seed % 1000) as f64 / 999.0;
        self.value = (self.value + unit * 8.0 - 4.0).clamp(0.0, 100.0);
        self.value
    }
}

/// The samples so far, and how many came before the oldest one kept.
#[derive(Clone, PartialEq, Default)]
struct Samples {
    first: u64,
    cpu: Vec<f64>,
    mem: Vec<f64>,
}

impl Samples {
    fn new(cpu: &mut Walk, mem: &mut Walk) -> Self {
        Self {
            first: 0,
            cpu: (0..HISTORY).map(|_| cpu.step()).collect(),
            mem: (0..HISTORY).map(|_| mem.step()).collect(),
        }
    }

    fn push(&mut self, cpu: f64, mem: f64) {
        self.cpu.push(cpu);
        self.mem.push(mem);
        if self.cpu.len() > HISTORY {
            self.cpu.remove(0);
            self.mem.remove(0);
            self.first += 1;
        }
    }

    fn points(&self, values: &[f64]) -> Vec<(f64, f64)> {
        (self.first..)
            .zip(values)
            .map(|(index, &value)| (index as f64, value))
            .collect()
    }
}

#[component]
fn Dashboard(samples: Signal<Samples>) -> View {
    let theme = install_theme(Theme::dark());
    let doc = use_document().expect("a component body has a document");
    let keys = doc.clone();
    doc.on_key_press(doc.root(), move |key| match key.code {
        KeyCode::Char('t') => theme.update(|theme| {
            *theme = if theme.palette == Palette::dark() {
                Theme::light()
            } else {
                Theme::dark()
            };
        }),
        KeyCode::Char('q') => keys.quit(),
        _ => {}
    })
    .expect("registering the key handler");

    let line_labels = TickLabels::new(|value| format!("{value}%"));
    let bar_labels = line_labels.clone();
    let range = Domain::Fixed(LinearScale::new(0.0, 100.0).expect("0 to 100 is a valid scale"));
    view! {
        box {
            style: column(),
            LineChart {
                title: String::from("Load"),
                series: {
                    let samples = samples.get();
                    vec![
                        ChartSeries::new("cpu", samples.points(&samples.cpu)),
                        ChartSeries::new("mem", samples.points(&samples.mem)),
                    ]
                },
                y: range,
                y_labels: line_labels.clone(),
                style: grow(2.0),
            },
            box {
                style: {
                    let mut row = grow(1.0);
                    row.gap(FlexGap { row: 0, column: 2 });
                    row
                },
                BarChart {
                    title: String::from("Now"),
                    orientation: Orientation::Horizontal,
                    bars: {
                        let samples = samples.get();
                        let latest = |values: &[f64]| values.last().copied().unwrap_or(f64::NAN);
                        vec![
                            ChartBar::new("cpu", latest(&samples.cpu)),
                            ChartBar::new("mem", latest(&samples.mem))
                                .color(Color::var("--chart-2")),
                        ]
                    },
                    domain: range,
                    labels: bar_labels.clone(),
                    style: {
                        let mut style = Style::new();
                        style.width(Length::Percent(50.0));
                        style
                    },
                },
                box {
                    style: {
                        let mut style = grow(1.0);
                        style.flex_direction(FlexDirection::Column);
                        style
                    },
                    text("cpu history") { style: bold() },
                    Sparkline { values: samples.get().cpu, domain: range, style: grow(1.0) },
                },
            },
            text("t theme  q quit"),
        }
    }
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style
}

/// Grow along the parent's main axis from nothing, by `factor`.
fn grow(factor: f32) -> Style {
    let mut style = Style::new();
    style.flex_basis(Length::Cells(0));
    style.flex_grow(factor);
    style
}

fn bold() -> Style {
    let mut style = Style::new();
    style.bold(true);
    style
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut cpu, mut mem) = (Walk::new(0x9e37_79b9, 40.0), Walk::new(0x85eb_ca6b, 60.0));
    let samples = Samples::new(&mut cpu, &mut mem);
    App::new(move || {
        let samples = state(samples.clone());
        spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_millis(250));
            loop {
                ticks.tick().await;
                let (cpu, mem) = (cpu.step(), mem.step());
                batch(|| samples.update(|samples| samples.push(cpu, mem)));
            }
        });
        view! { Dashboard { samples: samples } }
    })
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use tuidom_ui::testing::Harness;

    use super::*;

    #[test]
    fn the_dashboard_shows_each_chart_and_follows_new_samples() {
        let (mut cpu, mut mem) = (Walk::new(1, 40.0), Walk::new(2, 60.0));
        let samples = state(Samples::new(&mut cpu, &mut mem));
        let mut h = Harness::new(80, 24, move || view! { Dashboard { samples: samples } });
        assert_eq!(h.row(0), "Load");
        assert!(h.find_text("■ cpu  ■ mem").is_some());
        assert!(h.find_text("Now").is_some());
        assert!(h.find_text("cpu history").is_some());
        assert_eq!(h.row(23), "t theme  q quit");

        let before = h.screen();
        samples.update(|samples| samples.push(100.0, 0.0));
        assert_ne!(h.screen(), before);
        assert_eq!(samples.get().cpu.len(), HISTORY);
        assert_eq!(samples.get().first, 1);
    }
}
