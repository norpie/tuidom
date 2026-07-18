//! Frame-time benchmarks over headless scenarios.
//!
//! Each scenario builds a `HeadlessRuntime` at a fixed grid size with an
//! infinite opacity pulse running, so every `advance_time` + `render` iteration
//! measures the active-render loop — layout, style resolution, and paint —
//! without terminal I/O. Numbers are per frame; compare runs with `critcmp`.

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use tuidom::Document;
use tuidom::animation::{AnimatableProperty, AnimationDirection, Easing, KeyframeAnimation};
use tuidom::headless::HeadlessRuntime;
use tuidom::style::{
    AlignItems, Border, BorderCharset, Color, Display, EdgeInsets, FlexDirection, FlexGap,
    FlexWrap, JustifyContent, Length, Position, Style,
};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 50;

type ScenarioBuilder = fn() -> HeadlessRuntime;

/// A busy but ordinary scene: header with a pulsing chip row, then a wrapped
/// grid of bordered cards. The reference point every other scenario is
/// measured against.
fn baseline() -> HeadlessRuntime {
    build_scene(false)
}

/// The baseline scene under the demo's modal: a full-screen translucent
/// stacking context with a centered bordered dialog.
fn modal() -> HeadlessRuntime {
    build_scene(true)
}

/// The baseline scene with its whole background sweeping through colors, so
/// nearly every cell differs from the previous frame.
///
/// The other scenarios change only the few cells under the pulse, which leaves
/// the diff finding almost nothing and the flush encoding almost nothing. This
/// one drives the full-redraw path and the per-cell encoding that goes with it —
/// without it, a change that made flushing slower would not show up anywhere.
fn churn() -> HeadlessRuntime {
    build_scene_with(false, true)
}

fn build_scene(with_modal: bool) -> HeadlessRuntime {
    build_scene_with(with_modal, false)
}

fn build_scene_with(with_modal: bool, with_churn: bool) -> HeadlessRuntime {
    let doc = Document::new().unwrap();

    let mut container_style = Style::new();
    container_style.width(Length::Percent(100.0));
    container_style.height(Length::Percent(100.0));
    container_style.flex_direction(FlexDirection::Column);
    container_style.padding(EdgeInsets::all(1));
    container_style.gap(FlexGap::new(1, 0));
    container_style.background(Color::oklch(0.2, 0.03, 260.0));

    let container = doc.create_box().unwrap();
    doc.set_style(container, &container_style).unwrap();
    doc.append_child(doc.root(), container).unwrap();

    if with_churn {
        // Linear over a wide lightness range at 1ms steps, so consecutive frames
        // land on different 8-bit channel values instead of rounding together.
        doc.animate(
            container,
            KeyframeAnimation::from_to(
                Duration::from_millis(255),
                [AnimatableProperty::Background(Color::oklch(
                    0.15, 0.03, 260.0,
                ))],
                [AnimatableProperty::Background(Color::oklch(
                    0.85, 0.03, 260.0,
                ))],
            )
            .direction(AnimationDirection::Alternate)
            .infinite(),
        )
        .unwrap();
    }

    let mut header_style = Style::new();
    header_style.flex_direction(FlexDirection::Row);
    header_style.align_items(AlignItems::Center);
    header_style.gap(FlexGap::new(0, 2));

    let header = doc.create_box().unwrap();
    doc.set_style(header, &header_style).unwrap();
    doc.append_child(container, header).unwrap();

    let mut title_style = Style::new();
    title_style.color(Color::white());
    title_style.background(Color::oklch(0.35, 0.12, 250.0));

    let title = doc.create_text(" frame bench ").unwrap();
    doc.set_style(title, &title_style).unwrap();
    doc.append_child(header, title).unwrap();

    // The infinite pulse that keeps the scenario an active-render scene: its
    // resolved style changes every advanced millisecond, like the demo's.
    let mut pulse_style = Style::new();
    pulse_style.color(Color::white());
    pulse_style.background(Color::oklch(0.55, 0.18, 300.0));

    let pulse = doc.create_text("  pulse  ").unwrap();
    doc.set_style(pulse, &pulse_style).unwrap();
    doc.append_child(header, pulse).unwrap();
    doc.animate(
        pulse,
        KeyframeAnimation::from_to(
            Duration::from_millis(900),
            [AnimatableProperty::Opacity(1.0)],
            [AnimatableProperty::Opacity(0.35)],
        )
        .easing(Easing::EaseInOut)
        .direction(AnimationDirection::Alternate)
        .infinite(),
    )
    .unwrap();

    for chip in 0..10 {
        let mut chip_style = Style::new();
        chip_style.color(Color::white());
        chip_style.background(Color::oklch(0.4, 0.1, chip as f64 * 36.0));

        let node = doc.create_text(" chip ").unwrap();
        doc.set_style(node, &chip_style).unwrap();
        doc.append_child(header, node).unwrap();
    }

    let mut grid_style = Style::new();
    grid_style.flex_grow(1.0);
    grid_style.flex_direction(FlexDirection::Row);
    grid_style.flex_wrap(FlexWrap::Wrap);
    grid_style.gap(FlexGap::new(1, 2));

    let grid = doc.create_box().unwrap();
    doc.set_style(grid, &grid_style).unwrap();
    doc.append_child(container, grid).unwrap();

    for card in 0..24 {
        let mut card_style = Style::new();
        card_style.flex_direction(FlexDirection::Column);
        card_style.padding(EdgeInsets::symmetric(0, 1));
        card_style.background(Color::oklch(0.28, 0.05, card as f64 * 15.0));
        card_style.border(Border::new(BorderCharset::rounded()));
        card_style.border_color(Color::oklch(0.7, 0.08, card as f64 * 15.0));

        let node = doc.create_box().unwrap();
        doc.set_style(node, &card_style).unwrap();
        doc.append_child(grid, node).unwrap();

        let mut label_style = Style::new();
        label_style.color(Color::oklch(0.92, 0.04, 260.0));

        let label = doc.create_text("card title").unwrap();
        doc.set_style(label, &label_style).unwrap();
        doc.append_child(node, label).unwrap();

        let mut body_style = Style::new();
        body_style.color(Color::oklch(0.7, 0.02, 260.0));

        let body = doc.create_text("some body text here").unwrap();
        doc.set_style(body, &body_style).unwrap();
        doc.append_child(node, body).unwrap();
    }

    if with_modal {
        let mut modal_layer_style = Style::new();
        modal_layer_style.stacking_context(true);
        modal_layer_style.z_index(10);
        modal_layer_style.display(Display::Flex);
        modal_layer_style.position(Position::Absolute { x: 0, y: 0 });
        modal_layer_style.width(Length::Percent(100.0));
        modal_layer_style.height(Length::Percent(100.0));
        modal_layer_style.background(Color::oklcha(0.15, 0.03, 260.0, 0.6));
        modal_layer_style.justify_content(JustifyContent::Center);
        modal_layer_style.align_items(AlignItems::Center);

        let modal_layer = doc.create_box().unwrap();
        doc.set_style(modal_layer, &modal_layer_style).unwrap();
        doc.append_child(container, modal_layer).unwrap();

        let mut dialog_style = Style::new();
        dialog_style.flex_direction(FlexDirection::Column);
        dialog_style.align_items(AlignItems::Center);
        dialog_style.gap(FlexGap::new(1, 0));
        dialog_style.padding(EdgeInsets::all(1));
        dialog_style.background(Color::oklch(0.28, 0.06, 280.0));
        dialog_style.border(Border::new(BorderCharset::rounded()));
        dialog_style.border_color(Color::oklch(0.8, 0.1, 280.0));

        let dialog = doc.create_box().unwrap();
        doc.set_style(dialog, &dialog_style).unwrap();
        doc.append_child(modal_layer, dialog).unwrap();

        let mut dialog_text_style = Style::new();
        dialog_text_style.color(Color::white());

        for line in [" a modal dialog ", "sitting on a translucent layer"] {
            let text = doc.create_text(line).unwrap();
            doc.set_style(text, &dialog_text_style).unwrap();
            doc.append_child(dialog, text).unwrap();
        }
    }

    HeadlessRuntime::new(doc, WIDTH, HEIGHT)
}

const SCENARIOS: [(&str, ScenarioBuilder); 3] = [
    ("baseline", baseline as ScenarioBuilder),
    ("modal", modal),
    ("churn", churn),
];

fn bench_frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");
    for (name, build) in SCENARIOS {
        let mut runtime = build();
        runtime.render().unwrap();
        group.bench_function(name, |b| {
            b.iter(|| {
                runtime.advance_time(Duration::from_millis(1));
                runtime.render().unwrap();
            });
        });
    }
    group.finish();
}

/// The same scenes through the full frame the terminal renderer runs: paint,
/// diff against the previous frame, and encode the changes as terminal output.
///
/// `frame/*` stops after painting, so the diff and flush stages are invisible to
/// it. Anything touching the cell representation shows up on both sides — a
/// paint win that costs more to encode is a wash, and only running both makes
/// that visible.
fn bench_flushed_frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("flushed_frame");
    for (name, build) in SCENARIOS {
        let mut runtime = build();
        // Two renders first: the second is the one with a previous frame to diff
        // against, which is the steady state every measured iteration is in.
        runtime.render_flushed().unwrap();
        runtime.render_flushed().unwrap();
        group.bench_function(name, |b| {
            b.iter(|| {
                runtime.advance_time(Duration::from_millis(1));
                runtime.render_flushed().unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_frame, bench_flushed_frame);
criterion_main!(benches);
