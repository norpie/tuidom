use std::sync::{Arc, Mutex};

use tuidom::canvas::CanvasBuffer;
use tuidom::headless::ScreenColor;
use tuidom::style::{Color, FlexDirection, Length, ResolvedColor, Style};
use tuidom_components::{Theme, ThemeScope, ThemeScopeProps, install_theme};
use tuidom_ui::testing::Harness;
use tuidom_ui::{Signal, state, view};

use super::*;
use crate::bars::{Bar, BarPlot, draw_bars};
use crate::plot::{Domain, Plot, Series, draw_lines};
use crate::scale::LinearScale;
use crate::sparkline::draw_sparkline;

fn fixed(min: f64, max: f64) -> Domain {
    Domain::Fixed(LinearScale::new(min, max).unwrap())
}

/// The screen `draw` gives a buffer `width` by `height`, as `Harness::screen` shows it.
fn expected(width: u16, height: u16, draw: impl FnOnce(&mut CanvasBuffer)) -> String {
    let mut buffer = CanvasBuffer::new(width, height);
    draw(&mut buffer);
    (0..height)
        .map(|y| {
            let row: String = (0..width)
                .map(|x| match buffer.get(x, y) {
                    Some(("", _)) | None => " ",
                    Some((symbol, _)) => symbol,
                })
                .collect();
            row.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lines(width: u16, height: u16, points: &[(f64, f64)]) -> String {
    expected(width, height, |buffer| {
        let series = [Series {
            name: "",
            points,
            color: ResolvedColor::red(),
        }];
        let plot = Plot {
            series: &series,
            x: fixed(0.0, 10.0),
            y: fixed(0.0, 100.0),
            ..Plot::default()
        };
        draw_lines(buffer, &plot);
    })
}

/// The foreground of the first braille cell on screen.
fn braille_fg(h: &mut Harness) -> Option<ScreenColor> {
    let screen = h.screen();
    let (x, y) = screen
        .lines()
        .enumerate()
        .find_map(|(y, row)| {
            row.chars()
                .position(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                .map(|x| (x, y))
        })
        .expect("no braille on screen");
    h.cell(x as i32, y as i32).unwrap().fg
}

fn fg_of(h: &mut Harness, needle: &str) -> Option<ScreenColor> {
    let (x, y) = h.find_text(needle)?;
    h.cell(x, y)?.fg
}

fn column() -> Style {
    let mut style = Style::new();
    style.flex_direction(FlexDirection::Column);
    style.width(Length::Percent(100.0));
    style.height(Length::Percent(100.0));
    style
}

const POINTS: [(f64, f64); 3] = [(0.0, 0.0), (5.0, 80.0), (10.0, 30.0)];

#[test]
fn a_line_chart_draws_at_the_size_it_is_given() {
    let mut h = Harness::new(20, 8, || {
        install_theme(Theme::dark());
        view! {
            LineChart {
                series: vec![ChartSeries::new("", POINTS.to_vec())],
                x: fixed(0.0, 10.0),
                y: fixed(0.0, 100.0),
            }
        }
    });
    assert_eq!(h.screen(), lines(20, 8, &POINTS));
}

#[test]
fn a_chart_redraws_when_its_data_or_size_changes() {
    let data = state(vec![ChartSeries::new("", POINTS.to_vec())]);
    let mut h = Harness::new(20, 8, move || {
        install_theme(Theme::dark());
        view! {
            LineChart { series: data.get(), x: fixed(0.0, 10.0), y: fixed(0.0, 100.0) }
        }
    });
    let changed = [(0.0, 100.0), (10.0, 0.0)];
    data.set(vec![ChartSeries::new("", changed.to_vec())]);
    assert_eq!(h.screen(), lines(20, 8, &changed));

    h.resize(30, 10);
    assert_eq!(h.screen(), lines(30, 10, &changed));
}

#[test]
fn a_theme_switch_recolors_the_chart() {
    let slot: Arc<Mutex<Option<Signal<Theme>>>> = Arc::default();
    let handle = Arc::clone(&slot);
    let mut h = Harness::new(20, 8, move || {
        *handle.lock().unwrap() = Some(install_theme(Theme::dark()));
        view! { LineChart { series: vec![ChartSeries::new("cpu", POINTS.to_vec())] } }
    });
    let dark = braille_fg(&mut h);
    assert!(dark.is_some());
    assert_eq!(dark, fg_of(&mut h, "■"));

    let theme = slot.lock().unwrap().unwrap();
    theme.set(Theme::light());
    let light = braille_fg(&mut h);
    assert_ne!(light, dark);
    assert_eq!(light, fg_of(&mut h, "■"));
}

#[test]
fn a_chart_in_a_theme_scope_takes_its_colors() {
    let mut h = Harness::new(20, 8, || {
        install_theme(Theme::dark());
        view! {
            box {
                text("x") { style: swatch(Color::var("--chart-1")) },
                ThemeScope {
                    theme: Theme::light(),
                    style: {
                        let mut style = Style::new();
                        style.flex_grow(1.0);
                        style
                    },
                    LineChart { series: vec![ChartSeries::new("cpu", POINTS.to_vec())] },
                },
            }
        }
    });
    let chart = braille_fg(&mut h);
    assert_eq!(chart, fg_of(&mut h, "■"));
    assert_ne!(chart, fg_of(&mut h, "x"));
}

#[test]
fn the_title_and_legend_sit_around_the_canvas() {
    let mut h = Harness::new(20, 8, || {
        install_theme(Theme::dark());
        view! {
            LineChart {
                title: String::from("Load"),
                series: vec![
                    ChartSeries::new("cpu", POINTS.to_vec()),
                    ChartSeries::new("", vec![]),
                    ChartSeries::new("mem", vec![]),
                ],
            }
        }
    });
    assert_eq!(h.row(0), "Load");
    assert!(h.cell(0, 0).unwrap().bold);
    assert_eq!(h.row(7), "■ cpu  ■ mem");
    assert_ne!(fg_of(&mut h, "■ cpu"), fg_of(&mut h, "■ mem"));
}

#[test]
fn a_series_color_overrides_the_palette() {
    let mut h = Harness::new(20, 8, || {
        install_theme(Theme::dark());
        view! {
            box {
                style: column(),
                text("x") { style: swatch(Color::var("--destructive")) },
                LineChart {
                    series: vec![
                        ChartSeries::new("cpu", POINTS.to_vec()).color(Color::var("--destructive")),
                    ],
                },
            }
        }
    });
    let chart = braille_fg(&mut h);
    assert!(chart.is_some());
    assert_eq!(chart, fg_of(&mut h, "x"));
    assert_eq!(chart, fg_of(&mut h, "■"));
}

#[test]
fn without_a_theme_a_chart_draws_in_the_foreground() {
    let mut h = Harness::new(20, 8, || {
        view! {
            box {
                style: column(),
                text("x"),
                LineChart { series: vec![ChartSeries::new("", POINTS.to_vec())] },
            }
        }
    });
    let chart = braille_fg(&mut h);
    assert_eq!(chart, fg_of(&mut h, "x"));
}

#[test]
fn a_bar_chart_draws_at_the_size_it_is_given() {
    let mut h = Harness::new(16, 6, || {
        install_theme(Theme::dark());
        view! {
            BarChart { bars: vec![ChartBar::new("Mon", 3.0), ChartBar::new("Tue", 7.5)] }
        }
    });
    let drawn = expected(16, 6, |buffer| {
        let bars = [
            Bar {
                label: "Mon",
                value: 3.0,
                color: ResolvedColor::red(),
            },
            Bar {
                label: "Tue",
                value: 7.5,
                color: ResolvedColor::red(),
            },
        ];
        let plot = BarPlot {
            bars: &bars,
            ..BarPlot::default()
        };
        draw_bars(buffer, &plot);
    });
    assert_eq!(h.screen(), drawn);
}

#[test]
fn a_sparkline_takes_one_row_and_redraws_with_its_values() {
    let values = state(vec![1.0, 4.0, 2.0, 8.0]);
    let mut h = Harness::new(6, 2, move || {
        install_theme(Theme::dark());
        view! { Sparkline { values: values.get() } }
    });
    let sparkline = |values: &[f64]| {
        expected(6, 1, |buffer| {
            draw_sparkline(buffer, values, Domain::Data, ResolvedColor::red())
        })
    };
    assert_eq!(
        h.screen(),
        format!("{}\n", sparkline(&[1.0, 4.0, 2.0, 8.0]))
    );

    values.set(vec![8.0, 1.0]);
    assert_eq!(h.screen(), format!("{}\n", sparkline(&[8.0, 1.0])));
}
